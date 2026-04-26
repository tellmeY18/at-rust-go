//! Firehose WebSocket consumer with bounded backpressure.
//!
//! The consumer connects to a relay's `com.atproto.sync.subscribeRepos`
//! endpoint over WebSocket, decodes binary CBOR frames, extracts records
//! from CAR blocks, and dispatches [`FirehoseEvent`]s through a bounded
//! `mpsc` channel to a user-supplied handler.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use futures::StreamExt;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use crate::backoff::Backoff;
use crate::car;
use crate::event::{FirehoseCommit, FirehoseEvent, OpAction, RepoOp};
use crate::metrics::MetricsCounter;
use crate::FirehoseConfig;
use crate::FirehoseHandler;

/// Spawn the firehose consumer as a pair of background tasks.
///
/// Returns a join handle for the reader task. The consumer architecture:
///
/// 1. **Reader task** — connects to the relay WebSocket, decodes incoming
///    binary CBOR frames into [`FirehoseEvent`]s, and sends them into a
///    bounded `mpsc` channel. Reconnects with exponential backoff on error.
/// 2. **Dispatcher task** — reads events from the channel and invokes the
///    user-supplied handler for each one.
///
/// Backpressure: when the channel is full, the reader drops events and
/// increments the `events_dropped` metric counter.
///
/// The `state` parameter is an arbitrary `Clone + Send + 'static` value
/// that is forwarded to the handler on every event. In a typical atrg app
/// this is `AppState`, but the consumer itself does not depend on
/// `atrg-core` to avoid a cyclic dependency.
pub async fn spawn_firehose<S>(
    config: &FirehoseConfig,
    state: S,
    handler: FirehoseHandler<S>,
) -> anyhow::Result<tokio::task::JoinHandle<()>>
where
    S: Clone + Send + Sync + 'static,
{
    let metrics = MetricsCounter::new();
    let channel_capacity = config.channel_capacity;

    let url = build_ws_url(&config.relay, config.cursor);

    tracing::info!(
        url = %url,
        channel_capacity = channel_capacity,
        cursor = ?config.cursor,
        "starting firehose consumer"
    );

    let (tx, rx) = mpsc::channel::<FirehoseEvent>(channel_capacity);

    // Spawn the dispatcher task.
    spawn_dispatcher(rx, handler, state, metrics.clone());

    // Spawn the reader task.
    let handle = spawn_reader(url, tx, metrics);

    Ok(handle)
}

/// Build the firehose WebSocket subscription URL.
fn build_ws_url(relay: &str, cursor: Option<i64>) -> String {
    let base = relay.trim_end_matches('/');
    let endpoint = format!("{}/xrpc/com.atproto.sync.subscribeRepos", base);

    match cursor {
        Some(seq) => format!("{}?cursor={}", endpoint, seq),
        None => endpoint,
    }
}

/// Spawn the dispatcher task that reads from the channel and calls the handler.
fn spawn_dispatcher<S>(
    mut rx: mpsc::Receiver<FirehoseEvent>,
    handler: FirehoseHandler<S>,
    state: S,
    metrics: Arc<MetricsCounter>,
) where
    S: Clone + Send + Sync + 'static,
{
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let Err(e) = handler(event, state.clone()).await {
                tracing::error!(error = %e, "firehose event handler error");
                metrics.errors.fetch_add(1, Ordering::Relaxed);
            }
        }
        tracing::info!("firehose dispatcher task exiting");
    });
}

/// Spawn the reader task that connects to the WebSocket and feeds the channel.
fn spawn_reader(
    url: String,
    tx: mpsc::Sender<FirehoseEvent>,
    metrics: Arc<MetricsCounter>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut backoff = Backoff::new();

        loop {
            match connect_and_read(&url, &tx, &metrics).await {
                Ok(()) => {
                    tracing::info!("firehose WebSocket closed cleanly");
                }
                Err(e) => {
                    metrics.reconnects.fetch_add(1, Ordering::Relaxed);
                    tracing::warn!(error = %e, "firehose connection error, will reconnect");
                }
            }

            let delay = backoff.next_delay();
            metrics
                .current_backoff_ms
                .store(delay.as_millis() as u64, Ordering::Relaxed);
            tracing::info!(delay_ms = %delay.as_millis(), "reconnecting to firehose");
            tokio::time::sleep(delay).await;
        }
    })
}

/// Connect to the WebSocket and read events until the connection drops.
async fn connect_and_read(
    url: &str,
    tx: &mpsc::Sender<FirehoseEvent>,
    metrics: &Arc<MetricsCounter>,
) -> anyhow::Result<()> {
    let (ws_stream, _response) = tokio_tungstenite::connect_async(url).await?;
    tracing::info!(url = %url, "connected to firehose relay");

    // Reset backoff on successful connection.
    metrics.current_backoff_ms.store(0, Ordering::Relaxed);

    let (_write, mut read) = ws_stream.split();

    while let Some(msg_result) = read.next().await {
        let msg = msg_result?;
        match msg {
            Message::Binary(data) => {
                handle_binary_frame(&data, tx, metrics);
            }
            Message::Close(_) => {
                tracing::info!("firehose WebSocket closed by server");
                break;
            }
            // Ping/Pong are handled automatically by tungstenite.
            // Text frames are not expected from the firehose.
            _ => {}
        }
    }

    Ok(())
}

/// Decode and dispatch a single binary CBOR frame from the firehose.
fn handle_binary_frame(
    data: &[u8],
    tx: &mpsc::Sender<FirehoseEvent>,
    metrics: &Arc<MetricsCounter>,
) {
    metrics.events_received.fetch_add(1, Ordering::Relaxed);
    update_last_event_timestamp(metrics);

    let event = match decode_frame(data) {
        Ok(Some(ev)) => ev,
        Ok(None) => {
            // Unknown or unsupported frame type — silently skip.
            return;
        }
        Err(e) => {
            tracing::debug!(error = %e, "failed to decode firehose frame");
            metrics.errors.fetch_add(1, Ordering::Relaxed);
            return;
        }
    };

    // Update last_seq for cursor tracking.
    if let Some(seq) = event.seq() {
        metrics.last_seq.store(seq, Ordering::Relaxed);
    }

    // Backpressure: drop if channel is full.
    if tx.capacity() == 0 {
        metrics.events_dropped.fetch_add(1, Ordering::Relaxed);
        tracing::debug!("firehose channel full, dropping event");
        return;
    }

    if tx.try_send(event).is_err() {
        metrics.events_dropped.fetch_add(1, Ordering::Relaxed);
        tracing::debug!("firehose channel full on try_send, dropping event");
    }
}

/// Decode a binary CBOR frame into a [`FirehoseEvent`].
///
/// The firehose wire format is two concatenated CBOR values:
/// 1. Header: `{ "op": 1, "t": "#commit" }` (or other type tag)
/// 2. Body: the event payload as a CBOR map
///
/// Returns `Ok(None)` for unrecognized event types.
fn decode_frame(data: &[u8]) -> anyhow::Result<Option<FirehoseEvent>> {
    let mut cursor = data;

    // Decode the header.
    let header: ciborium::Value = ciborium::from_reader(&mut cursor)
        .map_err(|e| anyhow::anyhow!("failed to decode CBOR header: {}", e))?;

    let header_map =
        cbor_as_map(&header).ok_or_else(|| anyhow::anyhow!("CBOR header is not a map"))?;

    let op = cbor_map_get_int(header_map, "op").unwrap_or(0);
    let frame_type = cbor_map_get_str(header_map, "t").unwrap_or_default();

    // op=1 is a regular message frame, op=-1 is an error frame.
    if op == -1 {
        return decode_error_frame(&mut cursor);
    }

    if op != 1 {
        return Ok(None);
    }

    // Decode the body.
    let body: ciborium::Value = ciborium::from_reader(&mut cursor)
        .map_err(|e| anyhow::anyhow!("failed to decode CBOR body: {}", e))?;

    let body_map = cbor_as_map(&body).ok_or_else(|| anyhow::anyhow!("CBOR body is not a map"))?;

    match frame_type.as_str() {
        "#commit" => decode_commit(body_map).map(Some),
        "#handle" => decode_handle(body_map).map(Some),
        "#identity" => decode_identity(body_map).map(Some),
        "#tombstone" => decode_tombstone(body_map).map(Some),
        "#info" => decode_info(body_map).map(Some),
        _ => {
            tracing::trace!(frame_type = %frame_type, "unknown firehose frame type");
            Ok(None)
        }
    }
}

/// Decode an error frame (op=-1) into an info event.
fn decode_error_frame(cursor: &mut &[u8]) -> anyhow::Result<Option<FirehoseEvent>> {
    let body: ciborium::Value = ciborium::from_reader(cursor)
        .map_err(|e| anyhow::anyhow!("failed to decode error body: {}", e))?;

    let body_map = cbor_as_map(&body).unwrap_or_default();
    let name = cbor_map_get_str(body_map, "error").unwrap_or_else(|| "UnknownError".to_string());
    let message = cbor_map_get_str(body_map, "message");

    tracing::warn!(name = %name, message = ?message, "firehose error frame");

    Ok(Some(FirehoseEvent::Info { name, message }))
}

/// Decode a `#commit` frame body.
fn decode_commit(body: &[(ciborium::Value, ciborium::Value)]) -> anyhow::Result<FirehoseEvent> {
    let seq = cbor_map_get_int(body, "seq").ok_or_else(|| anyhow::anyhow!("commit missing seq"))?;
    let repo = cbor_map_get_str(body, "repo").unwrap_or_default();
    let rev = cbor_map_get_str(body, "rev").unwrap_or_default();
    let time = cbor_map_get_str(body, "time").unwrap_or_default();

    // Decode the ops array.
    let raw_ops = cbor_map_get_array(body, "ops").unwrap_or_default();

    // Decode the CAR blocks.
    let blocks_data = cbor_map_get_bytes(body, "blocks").unwrap_or_default();

    let car_decoded = if blocks_data.is_empty() {
        car::CarDecoded::empty()
    } else {
        car::decode_car(&blocks_data).unwrap_or_else(|e| {
            tracing::debug!(error = %e, "failed to decode CAR blocks");
            car::CarDecoded::empty()
        })
    };

    let mut ops = Vec::with_capacity(raw_ops.len());
    for op_val in &raw_ops {
        if let Some(op_map) = cbor_as_map(op_val) {
            let action_str = cbor_map_get_str(op_map, "action").unwrap_or_default();
            let action = match action_str.as_str() {
                "create" => OpAction::Create,
                "update" => OpAction::Update,
                "delete" => OpAction::Delete,
                _ => continue,
            };

            let path = cbor_map_get_str(op_map, "path").unwrap_or_default();
            let cid = extract_cid_string(op_map);

            // Look up the record in CAR blocks by CID.
            let record = cid
                .as_deref()
                .and_then(|c| car_decoded.blocks.get(c).cloned());

            ops.push(RepoOp {
                action,
                path,
                record,
                cid,
            });
        }
    }

    Ok(FirehoseEvent::Commit(FirehoseCommit {
        seq,
        repo,
        rev,
        ops,
        time,
    }))
}

/// Extract the CID string from an op's `cid` field.
///
/// In the CBOR encoding, the CID is represented as a CBOR tag 42 wrapping bytes,
/// or sometimes as a map with a `$link` field. We hex-encode the raw bytes.
fn extract_cid_string(op_map: &[(ciborium::Value, ciborium::Value)]) -> Option<String> {
    let cid_val = cbor_map_get_value(op_map, "cid")?;
    match cid_val {
        ciborium::Value::Tag(_tag, inner) => {
            if let ciborium::Value::Bytes(b) = inner.as_ref() {
                // Skip the leading 0x00 identity multibase prefix if present.
                let bytes = if b.first() == Some(&0x00) { &b[1..] } else { b };
                Some(hex_encode(bytes))
            } else {
                None
            }
        }
        ciborium::Value::Bytes(b) => {
            let bytes = if b.first() == Some(&0x00) { &b[1..] } else { b };
            Some(hex_encode(bytes))
        }
        ciborium::Value::Map(m) => cbor_map_get_str(m, "$link"),
        _ => None,
    }
}

/// Decode a `#handle` frame body.
fn decode_handle(body: &[(ciborium::Value, ciborium::Value)]) -> anyhow::Result<FirehoseEvent> {
    let seq =
        cbor_map_get_int(body, "seq").ok_or_else(|| anyhow::anyhow!("handle event missing seq"))?;
    let did = cbor_map_get_str(body, "did").unwrap_or_default();
    let handle = cbor_map_get_str(body, "handle").unwrap_or_default();

    Ok(FirehoseEvent::Handle { seq, did, handle })
}

/// Decode an `#identity` frame body.
fn decode_identity(body: &[(ciborium::Value, ciborium::Value)]) -> anyhow::Result<FirehoseEvent> {
    let seq = cbor_map_get_int(body, "seq")
        .ok_or_else(|| anyhow::anyhow!("identity event missing seq"))?;
    let did = cbor_map_get_str(body, "did").unwrap_or_default();

    Ok(FirehoseEvent::Identity { seq, did })
}

/// Decode a `#tombstone` frame body.
fn decode_tombstone(body: &[(ciborium::Value, ciborium::Value)]) -> anyhow::Result<FirehoseEvent> {
    let seq = cbor_map_get_int(body, "seq")
        .ok_or_else(|| anyhow::anyhow!("tombstone event missing seq"))?;
    let did = cbor_map_get_str(body, "did").unwrap_or_default();

    Ok(FirehoseEvent::Tombstone { seq, did })
}

/// Decode an `#info` frame body.
fn decode_info(body: &[(ciborium::Value, ciborium::Value)]) -> anyhow::Result<FirehoseEvent> {
    let name = cbor_map_get_str(body, "name").unwrap_or_else(|| "Unknown".to_string());
    let message = cbor_map_get_str(body, "message");

    Ok(FirehoseEvent::Info { name, message })
}

// ---------------------------------------------------------------------------
// CBOR value helpers
// ---------------------------------------------------------------------------

/// Try to interpret a `ciborium::Value` as a map (list of key-value pairs).
fn cbor_as_map(val: &ciborium::Value) -> Option<&[(ciborium::Value, ciborium::Value)]> {
    match val {
        ciborium::Value::Map(pairs) => Some(pairs.as_slice()),
        _ => None,
    }
}

/// Look up a string key in a CBOR map and return the value.
fn cbor_map_get_value<'a>(
    map: &'a [(ciborium::Value, ciborium::Value)],
    key: &str,
) -> Option<&'a ciborium::Value> {
    map.iter().find_map(|(k, v)| {
        if let ciborium::Value::Text(s) = k {
            if s == key {
                return Some(v);
            }
        }
        None
    })
}

/// Look up a string key in a CBOR map and return it as a string.
fn cbor_map_get_str(map: &[(ciborium::Value, ciborium::Value)], key: &str) -> Option<String> {
    cbor_map_get_value(map, key).and_then(|v| {
        if let ciborium::Value::Text(s) = v {
            Some(s.clone())
        } else {
            None
        }
    })
}

/// Look up a string key in a CBOR map and return it as an i64.
fn cbor_map_get_int(map: &[(ciborium::Value, ciborium::Value)], key: &str) -> Option<i64> {
    cbor_map_get_value(map, key).and_then(|v| match v {
        ciborium::Value::Integer(i) => {
            let val: i128 = (*i).into();
            i64::try_from(val).ok()
        }
        _ => None,
    })
}

/// Look up a string key in a CBOR map and return it as a byte vector.
fn cbor_map_get_bytes(map: &[(ciborium::Value, ciborium::Value)], key: &str) -> Option<Vec<u8>> {
    cbor_map_get_value(map, key).and_then(|v| {
        if let ciborium::Value::Bytes(b) = v {
            Some(b.clone())
        } else {
            None
        }
    })
}

/// Look up a string key in a CBOR map and return it as an array of values.
fn cbor_map_get_array(
    map: &[(ciborium::Value, ciborium::Value)],
    key: &str,
) -> Option<Vec<ciborium::Value>> {
    cbor_map_get_value(map, key).and_then(|v| {
        if let ciborium::Value::Array(a) = v {
            Some(a.clone())
        } else {
            None
        }
    })
}

/// Hex-encode a byte slice.
fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(s, "{:02x}", b);
    }
    s
}

/// Record the current wall-clock time as the last-event timestamp.
fn update_last_event_timestamp(metrics: &Arc<MetricsCounter>) {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    metrics.last_event_at.store(now_ms, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_ws_url_no_cursor() {
        let url = build_ws_url("wss://bsky.network", None);
        assert_eq!(
            url,
            "wss://bsky.network/xrpc/com.atproto.sync.subscribeRepos"
        );
    }

    #[test]
    fn build_ws_url_with_cursor() {
        let url = build_ws_url("wss://bsky.network", Some(12345));
        assert_eq!(
            url,
            "wss://bsky.network/xrpc/com.atproto.sync.subscribeRepos?cursor=12345"
        );
    }

    #[test]
    fn build_ws_url_strips_trailing_slash() {
        let url = build_ws_url("wss://bsky.network/", None);
        assert_eq!(
            url,
            "wss://bsky.network/xrpc/com.atproto.sync.subscribeRepos"
        );
    }

    #[test]
    fn hex_encode_works() {
        assert_eq!(hex_encode(&[0xde, 0xad, 0xbe, 0xef]), "deadbeef");
        assert_eq!(hex_encode(&[]), "");
        assert_eq!(hex_encode(&[0x00, 0x01, 0xff]), "0001ff");
    }

    #[test]
    fn cbor_helpers_work() {
        let map = vec![
            (
                ciborium::Value::Text("name".to_string()),
                ciborium::Value::Text("hello".to_string()),
            ),
            (
                ciborium::Value::Text("seq".to_string()),
                ciborium::Value::Integer(42.into()),
            ),
        ];

        assert_eq!(cbor_map_get_str(&map, "name"), Some("hello".to_string()));
        assert_eq!(cbor_map_get_int(&map, "seq"), Some(42));
        assert_eq!(cbor_map_get_str(&map, "missing"), None);
        assert_eq!(cbor_map_get_int(&map, "missing"), None);
    }
}
