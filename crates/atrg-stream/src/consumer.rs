//! Jetstream WebSocket consumer with bounded backpressure.
//!
//! The consumer connects to a Jetstream relay over WebSocket, reads events,
//! and dispatches them through a bounded `mpsc` channel to a user-supplied
//! handler. When the channel fills up, events are dropped and metrics are
//! updated.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use futures::StreamExt;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use atrg_db::DbPool;

use crate::backoff::Backoff;
use crate::event::JetstreamEvent;
use crate::metrics::MetricsCounter;
use crate::EventHandler;
use crate::StreamConfig;

/// Spawn the Jetstream consumer as a pair of background tasks.
///
/// Returns a join handle for the reader task. The consumer architecture:
///
/// 1. **Reader task** — connects to the Jetstream WebSocket, deserializes
///    incoming messages into [`JetstreamEvent`]s, and sends them into a
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
pub async fn spawn_consumer<S>(
    config: &StreamConfig,
    state: S,
    handler: EventHandler<S>,
) -> anyhow::Result<tokio::task::JoinHandle<()>>
where
    S: Clone + Send + Sync + 'static,
{
    let metrics = MetricsCounter::new();
    let channel_capacity = config.channel_capacity;
    let max_lag = config.max_lag_events;

    // Build the WebSocket URL with collection filters.
    let url = build_ws_url(&config.host, &config.collections, None);

    tracing::info!(
        url = %url,
        channel_capacity = channel_capacity,
        max_lag = max_lag,
        "starting Jetstream consumer"
    );

    let (tx, rx) = mpsc::channel::<JetstreamEvent>(channel_capacity);

    // Spawn the dispatcher task.
    spawn_dispatcher(rx, handler, state, metrics.clone());

    // Spawn the reader task.
    let handle = spawn_reader(url, tx, metrics, max_lag);

    Ok(handle)
}

/// Spawn a Jetstream consumer with cursor persistence.
///
/// Like [`spawn_consumer`], but loads the initial cursor from the database
/// and periodically saves the latest processed event timestamp. This allows
/// the consumer to resume from where it left off after a restart.
///
/// The `consumer_id` is a stable identifier for this consumer instance,
/// used as the key in the cursor persistence table. Use a meaningful name
/// like `"my-app-aggregator"` so multiple consumers can coexist.
///
/// Cursor behaviour is controlled by [`StreamConfig::cursor`]:
/// - `None` or `"live"` — always start from now (no cursor in the URL).
/// - `"auto"` — resume from the last stored cursor in the database.
/// - A numeric string — use that value as the initial cursor timestamp.
pub async fn spawn_consumer_with_cursor<S>(
    config: &StreamConfig,
    pool: &DbPool,
    consumer_id: &str,
    state: S,
    handler: EventHandler<S>,
) -> anyhow::Result<tokio::task::JoinHandle<()>>
where
    S: Clone + Send + Sync + 'static,
{
    // Ensure cursor table exists.
    crate::cursor::ensure_cursor_table(pool).await?;

    // Load stored cursor.
    let stored_cursor = crate::cursor::load_cursor(pool, consumer_id).await?;

    let initial_cursor = match config.cursor.as_deref() {
        Some("live") | None => None,
        Some("auto") => stored_cursor,
        Some(numeric) => numeric.parse::<i64>().ok(),
    };

    if let Some(cursor) = initial_cursor {
        tracing::info!(
            cursor = cursor,
            consumer_id = consumer_id,
            "resuming Jetstream from stored cursor"
        );
    } else {
        tracing::info!(
            consumer_id = consumer_id,
            "starting Jetstream from live (no cursor)"
        );
    }

    // Build URL with cursor.
    let url = build_ws_url(&config.host, &config.collections, initial_cursor);

    let metrics = MetricsCounter::new();
    let channel_capacity = config.channel_capacity;
    let max_lag = config.max_lag_events;

    tracing::info!(
        url = %url,
        channel_capacity = channel_capacity,
        max_lag = max_lag,
        "starting Jetstream consumer with cursor persistence"
    );

    let (tx, rx) = mpsc::channel::<JetstreamEvent>(channel_capacity);

    // Spawn the cursor-persisting dispatcher task.
    let pool_clone = pool.clone();
    let cid = consumer_id.to_string();
    spawn_cursor_dispatcher(rx, handler, state, metrics.clone(), pool_clone, cid);

    // Spawn the reader task.
    let handle = spawn_reader(url, tx, metrics, max_lag);

    Ok(handle)
}

/// Build the Jetstream WebSocket subscription URL.
///
/// If `cursor` is provided, it is appended as a query parameter so that
/// the relay replays events from that timestamp onward.
fn build_ws_url(host: &str, collections: &[String], cursor: Option<i64>) -> String {
    let mut params: Vec<String> = collections
        .iter()
        .map(|c| format!("wantedCollections={}", c))
        .collect();

    if let Some(cursor_us) = cursor {
        params.push(format!("cursor={}", cursor_us));
    }

    if params.is_empty() {
        format!("wss://{}/subscribe", host)
    } else {
        format!("wss://{}/subscribe?{}", host, params.join("&"))
    }
}

/// Spawn the dispatcher task that reads from the channel and calls the handler.
fn spawn_dispatcher<S>(
    mut rx: mpsc::Receiver<JetstreamEvent>,
    handler: EventHandler<S>,
    state: S,
    metrics: Arc<MetricsCounter>,
) where
    S: Clone + Send + Sync + 'static,
{
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let Err(e) = handler(event, state.clone()).await {
                tracing::error!(error = %e, "Jetstream event handler error");
                metrics.errors.fetch_add(1, Ordering::Relaxed);
            }
        }
        tracing::info!("Jetstream dispatcher task exiting");
    });
}

/// Interval (in number of events) between cursor saves.
///
/// Saving on every event would be too expensive for high-throughput streams.
/// 100 events provides a good balance between resume precision and I/O load.
const CURSOR_SAVE_INTERVAL: u64 = 100;

/// Dispatcher that persists the cursor every [`CURSOR_SAVE_INTERVAL`] events.
///
/// On graceful shutdown (channel closed), the final cursor is saved so that
/// no more than one interval's worth of events needs to be replayed.
fn spawn_cursor_dispatcher<S>(
    mut rx: mpsc::Receiver<JetstreamEvent>,
    handler: EventHandler<S>,
    state: S,
    metrics: Arc<MetricsCounter>,
    pool: DbPool,
    consumer_id: String,
) where
    S: Clone + Send + Sync + 'static,
{
    tokio::spawn(async move {
        let mut event_count: u64 = 0;
        let mut last_time_us: Option<i64> = None;

        while let Some(event) = rx.recv().await {
            let time_us = event.time_us;

            if let Err(e) = handler(event, state.clone()).await {
                tracing::error!(error = %e, "Jetstream event handler error");
                metrics.errors.fetch_add(1, Ordering::Relaxed);
            }

            // Only track non-zero timestamps (zero means the field was absent).
            if time_us > 0 {
                last_time_us = Some(time_us);
            }
            event_count += 1;

            // Periodically persist the cursor.
            if event_count % CURSOR_SAVE_INTERVAL == 0 {
                if let Some(cursor) = last_time_us {
                    if let Err(e) = crate::cursor::save_cursor(&pool, &consumer_id, cursor).await {
                        tracing::warn!(error = %e, "failed to save Jetstream cursor");
                    }
                }
            }
        }

        // Save final cursor on shutdown.
        if let Some(cursor) = last_time_us {
            if let Err(e) = crate::cursor::save_cursor(&pool, &consumer_id, cursor).await {
                tracing::warn!(error = %e, "failed to save final Jetstream cursor");
            } else {
                tracing::info!(cursor = cursor, "saved final Jetstream cursor on shutdown");
            }
        }

        tracing::info!("Jetstream cursor dispatcher task exiting");
    });
}

/// Spawn the reader task that connects to the WebSocket and feeds the channel.
fn spawn_reader(
    url: String,
    tx: mpsc::Sender<JetstreamEvent>,
    metrics: Arc<MetricsCounter>,
    max_lag: usize,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut backoff = Backoff::new();

        loop {
            match connect_and_read(&url, &tx, &metrics, max_lag).await {
                Ok(()) => {
                    tracing::info!("Jetstream WebSocket closed cleanly");
                }
                Err(e) => {
                    metrics.reconnects.fetch_add(1, Ordering::Relaxed);
                    tracing::warn!(error = %e, "Jetstream connection error, will reconnect");
                }
            }

            let delay = backoff.next_delay();
            metrics
                .current_backoff_ms
                .store(delay.as_millis() as u64, Ordering::Relaxed);
            tracing::info!(delay_ms = %delay.as_millis(), "reconnecting to Jetstream");
            tokio::time::sleep(delay).await;
        }
    })
}

/// Connect to the WebSocket and read events until the connection drops.
///
/// On a successful connection the backoff counter is reset (via metrics).
/// Returns `Ok(())` on a clean close, or an error on disconnect/failure.
async fn connect_and_read(
    url: &str,
    tx: &mpsc::Sender<JetstreamEvent>,
    metrics: &Arc<MetricsCounter>,
    max_lag: usize,
) -> anyhow::Result<()> {
    let (ws_stream, _response) = tokio_tungstenite::connect_async(url).await?;
    tracing::info!(url = %url, "connected to Jetstream");

    // Reset backoff on successful connection.
    metrics.current_backoff_ms.store(0, Ordering::Relaxed);

    let (_write, mut read) = ws_stream.split();

    while let Some(msg_result) = read.next().await {
        let msg = msg_result?;
        match msg {
            Message::Text(text) => {
                handle_text_message(&text, tx, metrics, max_lag);
            }
            Message::Close(_) => {
                tracing::info!("Jetstream WebSocket closed by server");
                break;
            }
            // Ping/Pong are handled automatically by tungstenite.
            // Binary frames are not expected from Jetstream.
            _ => {}
        }
    }

    Ok(())
}

/// Parse and dispatch a single text message from the WebSocket.
fn handle_text_message(
    text: &str,
    tx: &mpsc::Sender<JetstreamEvent>,
    metrics: &Arc<MetricsCounter>,
    max_lag: usize,
) {
    metrics.events_received.fetch_add(1, Ordering::Relaxed);
    update_last_event_timestamp(metrics);

    let event = match serde_json::from_str::<JetstreamEvent>(text) {
        Ok(ev) => ev,
        Err(e) => {
            tracing::debug!(error = %e, "failed to parse Jetstream event");
            metrics.errors.fetch_add(1, Ordering::Relaxed);
            return;
        }
    };

    // Lag detection: if remaining capacity is zero and the channel is large
    // enough that we've hit the lag threshold, drop the event.
    let remaining = tx.capacity();
    if remaining == 0 {
        metrics.events_dropped.fetch_add(1, Ordering::Relaxed);
        if tx.max_capacity() >= max_lag {
            tracing::warn!(
                max_lag = max_lag,
                "Jetstream consumer lagging beyond threshold, dropping event"
            );
        }
        return;
    }

    // Try non-blocking send. If it fails (shouldn't after the capacity check,
    // but races are possible), drop the event.
    if tx.try_send(event).is_err() {
        metrics.events_dropped.fetch_add(1, Ordering::Relaxed);
        tracing::debug!("Jetstream channel full on try_send, dropping event");
    }
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
    fn build_ws_url_no_collections() {
        let url = build_ws_url("jetstream1.example.com", &[], None);
        assert_eq!(url, "wss://jetstream1.example.com/subscribe");
    }

    #[test]
    fn build_ws_url_single_collection() {
        let url = build_ws_url(
            "jetstream1.example.com",
            &["app.bsky.feed.post".to_string()],
            None,
        );
        assert_eq!(
            url,
            "wss://jetstream1.example.com/subscribe?wantedCollections=app.bsky.feed.post"
        );
    }

    #[test]
    fn build_ws_url_multiple_collections() {
        let url = build_ws_url(
            "jetstream1.example.com",
            &[
                "app.bsky.feed.post".to_string(),
                "app.bsky.feed.like".to_string(),
            ],
            None,
        );
        assert_eq!(
            url,
            "wss://jetstream1.example.com/subscribe?wantedCollections=app.bsky.feed.post&wantedCollections=app.bsky.feed.like"
        );
    }

    #[test]
    fn build_ws_url_with_cursor_no_collections() {
        let url = build_ws_url("jetstream1.example.com", &[], Some(1700000000000000));
        assert_eq!(
            url,
            "wss://jetstream1.example.com/subscribe?cursor=1700000000000000"
        );
    }

    #[test]
    fn build_ws_url_with_cursor_and_collections() {
        let url = build_ws_url(
            "jetstream1.example.com",
            &["app.bsky.feed.post".to_string()],
            Some(1700000000000000),
        );
        assert_eq!(
            url,
            "wss://jetstream1.example.com/subscribe?wantedCollections=app.bsky.feed.post&cursor=1700000000000000"
        );
    }
}
