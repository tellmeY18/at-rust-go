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

    // -----------------------------------------------------------------------
    // Helpers for constructing CBOR frames
    // -----------------------------------------------------------------------

    fn encode_frame(header: &ciborium::Value, body: &ciborium::Value) -> Vec<u8> {
        let mut buf = Vec::new();
        ciborium::into_writer(header, &mut buf).unwrap();
        ciborium::into_writer(body, &mut buf).unwrap();
        buf
    }

    fn cbor_map(entries: Vec<(&str, ciborium::Value)>) -> ciborium::Value {
        ciborium::Value::Map(
            entries
                .into_iter()
                .map(|(k, v)| (ciborium::Value::Text(k.to_string()), v))
                .collect(),
        )
    }

    fn cbor_map_pairs(
        entries: Vec<(&str, ciborium::Value)>,
    ) -> Vec<(ciborium::Value, ciborium::Value)> {
        entries
            .into_iter()
            .map(|(k, v)| (ciborium::Value::Text(k.to_string()), v))
            .collect()
    }

    // -----------------------------------------------------------------------
    // decode_frame tests
    // -----------------------------------------------------------------------

    #[test]
    fn decode_frame_commit() {
        let header = cbor_map(vec![
            ("op", ciborium::Value::Integer(1.into())),
            ("t", ciborium::Value::Text("#commit".into())),
        ]);
        let body = cbor_map(vec![
            ("seq", ciborium::Value::Integer(42.into())),
            ("repo", ciborium::Value::Text("did:plc:abc".into())),
            ("rev", ciborium::Value::Text("rev1".into())),
            ("time", ciborium::Value::Text("2024-01-01".into())),
            ("ops", ciborium::Value::Array(vec![])),
            ("blocks", ciborium::Value::Bytes(vec![])),
        ]);
        let data = encode_frame(&header, &body);
        let result = decode_frame(&data).unwrap().unwrap();
        match result {
            FirehoseEvent::Commit(c) => {
                assert_eq!(c.seq, 42);
                assert_eq!(c.repo, "did:plc:abc");
                assert_eq!(c.rev, "rev1");
                assert_eq!(c.time, "2024-01-01");
                assert!(c.ops.is_empty());
            }
            other => panic!("expected Commit, got {:?}", other),
        }
    }

    #[test]
    fn decode_frame_handle() {
        let header = cbor_map(vec![
            ("op", ciborium::Value::Integer(1.into())),
            ("t", ciborium::Value::Text("#handle".into())),
        ]);
        let body = cbor_map(vec![
            ("seq", ciborium::Value::Integer(1.into())),
            ("did", ciborium::Value::Text("did:plc:abc".into())),
            ("handle", ciborium::Value::Text("alice.bsky.social".into())),
        ]);
        let data = encode_frame(&header, &body);
        let result = decode_frame(&data).unwrap().unwrap();
        match result {
            FirehoseEvent::Handle { seq, did, handle } => {
                assert_eq!(seq, 1);
                assert_eq!(did, "did:plc:abc");
                assert_eq!(handle, "alice.bsky.social");
            }
            other => panic!("expected Handle, got {:?}", other),
        }
    }

    #[test]
    fn decode_frame_identity() {
        let header = cbor_map(vec![
            ("op", ciborium::Value::Integer(1.into())),
            ("t", ciborium::Value::Text("#identity".into())),
        ]);
        let body = cbor_map(vec![
            ("seq", ciborium::Value::Integer(2.into())),
            ("did", ciborium::Value::Text("did:plc:abc".into())),
        ]);
        let data = encode_frame(&header, &body);
        let result = decode_frame(&data).unwrap().unwrap();
        match result {
            FirehoseEvent::Identity { seq, did } => {
                assert_eq!(seq, 2);
                assert_eq!(did, "did:plc:abc");
            }
            other => panic!("expected Identity, got {:?}", other),
        }
    }

    #[test]
    fn decode_frame_tombstone() {
        let header = cbor_map(vec![
            ("op", ciborium::Value::Integer(1.into())),
            ("t", ciborium::Value::Text("#tombstone".into())),
        ]);
        let body = cbor_map(vec![
            ("seq", ciborium::Value::Integer(3.into())),
            ("did", ciborium::Value::Text("did:plc:abc".into())),
        ]);
        let data = encode_frame(&header, &body);
        let result = decode_frame(&data).unwrap().unwrap();
        match result {
            FirehoseEvent::Tombstone { seq, did } => {
                assert_eq!(seq, 3);
                assert_eq!(did, "did:plc:abc");
            }
            other => panic!("expected Tombstone, got {:?}", other),
        }
    }

    #[test]
    fn decode_frame_info() {
        let header = cbor_map(vec![
            ("op", ciborium::Value::Integer(1.into())),
            ("t", ciborium::Value::Text("#info".into())),
        ]);
        let body = cbor_map(vec![
            ("name", ciborium::Value::Text("OutdatedCursor".into())),
            ("message", ciborium::Value::Text("you are behind".into())),
        ]);
        let data = encode_frame(&header, &body);
        let result = decode_frame(&data).unwrap().unwrap();
        match result {
            FirehoseEvent::Info { name, message } => {
                assert_eq!(name, "OutdatedCursor");
                assert_eq!(message, Some("you are behind".to_string()));
            }
            other => panic!("expected Info, got {:?}", other),
        }
    }

    #[test]
    fn decode_frame_error_op_minus_one() {
        let header = cbor_map(vec![("op", ciborium::Value::Integer((-1).into()))]);
        let body = cbor_map(vec![
            ("error", ciborium::Value::Text("FutureCursor".into())),
            ("message", ciborium::Value::Text("bad cursor".into())),
        ]);
        let data = encode_frame(&header, &body);
        let result = decode_frame(&data).unwrap().unwrap();
        match result {
            FirehoseEvent::Info { name, message } => {
                assert_eq!(name, "FutureCursor");
                assert_eq!(message, Some("bad cursor".to_string()));
            }
            other => panic!("expected Info from error frame, got {:?}", other),
        }
    }

    #[test]
    fn decode_frame_unknown_type_returns_none() {
        let header = cbor_map(vec![
            ("op", ciborium::Value::Integer(1.into())),
            ("t", ciborium::Value::Text("#unknown".into())),
        ]);
        let body = cbor_map(vec![]);
        let data = encode_frame(&header, &body);
        assert!(decode_frame(&data).unwrap().is_none());
    }

    #[test]
    fn decode_frame_unknown_op_returns_none() {
        let header = cbor_map(vec![
            ("op", ciborium::Value::Integer(99.into())),
            ("t", ciborium::Value::Text("#commit".into())),
        ]);
        // Body isn't read for unknown op, but we need valid CBOR after header
        // Actually for op != 1 and op != -1, we return Ok(None) before reading body.
        let mut data = Vec::new();
        ciborium::into_writer(&header, &mut data).unwrap();
        assert!(decode_frame(&data).unwrap().is_none());
    }

    // -----------------------------------------------------------------------
    // decode_commit tests
    // -----------------------------------------------------------------------

    #[test]
    fn decode_commit_with_ops() {
        let op_create = cbor_map(vec![
            ("action", ciborium::Value::Text("create".into())),
            (
                "path",
                ciborium::Value::Text("app.bsky.feed.post/abc".into()),
            ),
        ]);
        let op_update = cbor_map(vec![
            ("action", ciborium::Value::Text("update".into())),
            (
                "path",
                ciborium::Value::Text("app.bsky.feed.post/def".into()),
            ),
        ]);
        let op_delete = cbor_map(vec![
            ("action", ciborium::Value::Text("delete".into())),
            (
                "path",
                ciborium::Value::Text("app.bsky.feed.post/ghi".into()),
            ),
        ]);

        let body = cbor_map_pairs(vec![
            ("seq", ciborium::Value::Integer(10.into())),
            ("repo", ciborium::Value::Text("did:plc:xyz".into())),
            ("rev", ciborium::Value::Text("r2".into())),
            ("time", ciborium::Value::Text("2024-06-01".into())),
            (
                "ops",
                ciborium::Value::Array(vec![op_create, op_update, op_delete]),
            ),
            ("blocks", ciborium::Value::Bytes(vec![])),
        ]);

        let result = decode_commit(&body).unwrap();
        match result {
            FirehoseEvent::Commit(c) => {
                assert_eq!(c.seq, 10);
                assert_eq!(c.repo, "did:plc:xyz");
                assert_eq!(c.ops.len(), 3);
                assert_eq!(c.ops[0].action, OpAction::Create);
                assert_eq!(c.ops[0].path, "app.bsky.feed.post/abc");
                assert_eq!(c.ops[1].action, OpAction::Update);
                assert_eq!(c.ops[2].action, OpAction::Delete);
            }
            other => panic!("expected Commit, got {:?}", other),
        }
    }

    #[test]
    fn decode_commit_empty_ops() {
        let body = cbor_map_pairs(vec![
            ("seq", ciborium::Value::Integer(5.into())),
            ("repo", ciborium::Value::Text("did:plc:abc".into())),
            ("rev", ciborium::Value::Text("r1".into())),
            ("time", ciborium::Value::Text("2024-01-01".into())),
            ("ops", ciborium::Value::Array(vec![])),
            ("blocks", ciborium::Value::Bytes(vec![])),
        ]);
        let result = decode_commit(&body).unwrap();
        match result {
            FirehoseEvent::Commit(c) => assert!(c.ops.is_empty()),
            other => panic!("expected Commit, got {:?}", other),
        }
    }

    #[test]
    fn decode_commit_missing_seq_errors() {
        let body = cbor_map_pairs(vec![("repo", ciborium::Value::Text("did:plc:abc".into()))]);
        assert!(decode_commit(&body).is_err());
    }

    // -----------------------------------------------------------------------
    // decode_handle / decode_identity / decode_tombstone / decode_info tests
    // -----------------------------------------------------------------------

    #[test]
    fn decode_handle_success() {
        let body = cbor_map_pairs(vec![
            ("seq", ciborium::Value::Integer(7.into())),
            ("did", ciborium::Value::Text("did:plc:h".into())),
            ("handle", ciborium::Value::Text("bob.test".into())),
        ]);
        match decode_handle(&body).unwrap() {
            FirehoseEvent::Handle { seq, did, handle } => {
                assert_eq!(seq, 7);
                assert_eq!(did, "did:plc:h");
                assert_eq!(handle, "bob.test");
            }
            other => panic!("expected Handle, got {:?}", other),
        }
    }

    #[test]
    fn decode_handle_missing_seq_errors() {
        let body = cbor_map_pairs(vec![("did", ciborium::Value::Text("did:plc:h".into()))]);
        assert!(decode_handle(&body).is_err());
    }

    #[test]
    fn decode_identity_success() {
        let body = cbor_map_pairs(vec![
            ("seq", ciborium::Value::Integer(8.into())),
            ("did", ciborium::Value::Text("did:plc:i".into())),
        ]);
        match decode_identity(&body).unwrap() {
            FirehoseEvent::Identity { seq, did } => {
                assert_eq!(seq, 8);
                assert_eq!(did, "did:plc:i");
            }
            other => panic!("expected Identity, got {:?}", other),
        }
    }

    #[test]
    fn decode_identity_missing_seq_errors() {
        let body = cbor_map_pairs(vec![("did", ciborium::Value::Text("did:plc:i".into()))]);
        assert!(decode_identity(&body).is_err());
    }

    #[test]
    fn decode_tombstone_success() {
        let body = cbor_map_pairs(vec![
            ("seq", ciborium::Value::Integer(9.into())),
            ("did", ciborium::Value::Text("did:plc:t".into())),
        ]);
        match decode_tombstone(&body).unwrap() {
            FirehoseEvent::Tombstone { seq, did } => {
                assert_eq!(seq, 9);
                assert_eq!(did, "did:plc:t");
            }
            other => panic!("expected Tombstone, got {:?}", other),
        }
    }

    #[test]
    fn decode_tombstone_missing_seq_errors() {
        let body = cbor_map_pairs(vec![("did", ciborium::Value::Text("did:plc:t".into()))]);
        assert!(decode_tombstone(&body).is_err());
    }

    #[test]
    fn decode_info_success() {
        let body = cbor_map_pairs(vec![
            ("name", ciborium::Value::Text("OutdatedCursor".into())),
            ("message", ciborium::Value::Text("old cursor".into())),
        ]);
        match decode_info(&body).unwrap() {
            FirehoseEvent::Info { name, message } => {
                assert_eq!(name, "OutdatedCursor");
                assert_eq!(message, Some("old cursor".to_string()));
            }
            other => panic!("expected Info, got {:?}", other),
        }
    }

    #[test]
    fn decode_info_no_message() {
        let body = cbor_map_pairs(vec![("name", ciborium::Value::Text("SomeInfo".into()))]);
        match decode_info(&body).unwrap() {
            FirehoseEvent::Info { name, message } => {
                assert_eq!(name, "SomeInfo");
                assert_eq!(message, None);
            }
            other => panic!("expected Info, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // extract_cid_string tests
    // -----------------------------------------------------------------------

    #[test]
    fn extract_cid_tag42_with_prefix() {
        let map = cbor_map_pairs(vec![(
            "cid",
            ciborium::Value::Tag(42, Box::new(ciborium::Value::Bytes(vec![0x00, 0xde, 0xad]))),
        )]);
        assert_eq!(extract_cid_string(&map), Some("dead".to_string()));
    }

    #[test]
    fn extract_cid_tag42_no_prefix() {
        let map = cbor_map_pairs(vec![(
            "cid",
            ciborium::Value::Tag(42, Box::new(ciborium::Value::Bytes(vec![0xab, 0xcd]))),
        )]);
        assert_eq!(extract_cid_string(&map), Some("abcd".to_string()));
    }

    #[test]
    fn extract_cid_raw_bytes() {
        let map = cbor_map_pairs(vec![(
            "cid",
            ciborium::Value::Bytes(vec![0x00, 0x01, 0x02]),
        )]);
        assert_eq!(extract_cid_string(&map), Some("0102".to_string()));
    }

    #[test]
    fn extract_cid_link_map() {
        let link_map = ciborium::Value::Map(vec![(
            ciborium::Value::Text("$link".into()),
            ciborium::Value::Text("bafyabc".into()),
        )]);
        let map = cbor_map_pairs(vec![("cid", link_map)]);
        assert_eq!(extract_cid_string(&map), Some("bafyabc".to_string()));
    }

    #[test]
    fn extract_cid_other_type_returns_none() {
        let map = cbor_map_pairs(vec![("cid", ciborium::Value::Integer(123.into()))]);
        assert_eq!(extract_cid_string(&map), None);
    }

    #[test]
    fn extract_cid_missing_key_returns_none() {
        let map = cbor_map_pairs(vec![("other", ciborium::Value::Text("x".into()))]);
        assert_eq!(extract_cid_string(&map), None);
    }

    // -----------------------------------------------------------------------
    // cbor_map_get_bytes / cbor_map_get_array tests
    // -----------------------------------------------------------------------

    #[test]
    fn cbor_map_get_bytes_present() {
        let map = cbor_map_pairs(vec![("data", ciborium::Value::Bytes(vec![1, 2, 3]))]);
        assert_eq!(cbor_map_get_bytes(&map, "data"), Some(vec![1, 2, 3]));
    }

    #[test]
    fn cbor_map_get_bytes_missing() {
        let map = cbor_map_pairs(vec![("other", ciborium::Value::Integer(1.into()))]);
        assert_eq!(cbor_map_get_bytes(&map, "data"), None);
    }

    #[test]
    fn cbor_map_get_array_present() {
        let map = cbor_map_pairs(vec![(
            "items",
            ciborium::Value::Array(vec![
                ciborium::Value::Integer(1.into()),
                ciborium::Value::Integer(2.into()),
            ]),
        )]);
        let arr = cbor_map_get_array(&map, "items").unwrap();
        assert_eq!(arr.len(), 2);
    }

    #[test]
    fn cbor_map_get_array_missing() {
        let map = cbor_map_pairs(vec![("other", ciborium::Value::Integer(1.into()))]);
        assert_eq!(cbor_map_get_array(&map, "items"), None);
    }

    // -----------------------------------------------------------------------
    // handle_binary_frame tests
    // -----------------------------------------------------------------------

    #[test]
    fn handle_binary_frame_sends_event() {
        let header = cbor_map(vec![
            ("op", ciborium::Value::Integer(1.into())),
            ("t", ciborium::Value::Text("#identity".into())),
        ]);
        let body = cbor_map(vec![
            ("seq", ciborium::Value::Integer(77.into())),
            ("did", ciborium::Value::Text("did:plc:test".into())),
        ]);
        let data = encode_frame(&header, &body);

        let (tx, mut rx) = mpsc::channel(16);
        let metrics = MetricsCounter::new();

        handle_binary_frame(&data, &tx, &metrics);

        let event = rx.try_recv().unwrap();
        match event {
            FirehoseEvent::Identity { seq, did } => {
                assert_eq!(seq, 77);
                assert_eq!(did, "did:plc:test");
            }
            other => panic!("expected Identity, got {:?}", other),
        }
        assert_eq!(metrics.events_received.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.events_dropped.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn handle_binary_frame_drops_when_channel_full() {
        let header = cbor_map(vec![
            ("op", ciborium::Value::Integer(1.into())),
            ("t", ciborium::Value::Text("#identity".into())),
        ]);
        let body = cbor_map(vec![
            ("seq", ciborium::Value::Integer(1.into())),
            ("did", ciborium::Value::Text("did:plc:x".into())),
        ]);
        let data = encode_frame(&header, &body);

        // Channel with capacity 1, pre-fill it.
        let (tx, _rx) = mpsc::channel(1);
        let metrics = MetricsCounter::new();

        // Fill the channel.
        handle_binary_frame(&data, &tx, &metrics);
        // Second send should drop.
        handle_binary_frame(&data, &tx, &metrics);

        assert_eq!(metrics.events_received.load(Ordering::Relaxed), 2);
        assert_eq!(metrics.events_dropped.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn handle_binary_frame_bad_data_increments_errors() {
        let (tx, _rx) = mpsc::channel(16);
        let metrics = MetricsCounter::new();

        handle_binary_frame(&[0xff, 0xff], &tx, &metrics);

        assert_eq!(metrics.events_received.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.errors.load(Ordering::Relaxed), 1);
    }

    // -----------------------------------------------------------------------
    // update_last_event_timestamp tests
    // -----------------------------------------------------------------------

    #[test]
    fn update_last_event_timestamp_sets_nonzero() {
        let metrics = MetricsCounter::new();
        assert_eq!(metrics.last_event_at.load(Ordering::Relaxed), 0);
        update_last_event_timestamp(&metrics);
        assert_ne!(metrics.last_event_at.load(Ordering::Relaxed), 0);
    }
}
