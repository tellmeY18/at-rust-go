//! Ergonomic event router for dispatching Jetstream events by collection and operation.
//!
//! Instead of writing manual match statements in your `on_event` handler,
//! use [`EventRouterBuilder`] to declaratively register handlers per collection
//! and operation type:
//!
//! ```rust,ignore
//! use atrg_stream::{EventRouterBuilder, CommitEvent, Operation};
//!
//! let router = EventRouterBuilder::new()
//!     .on_create("app.bsky.feed.post", handle_new_post)
//!     .on_delete("app.bsky.feed.post", handle_deleted_post)
//!     .on("app.bsky.feed.like", handle_any_like)
//!     .build();
//! ```

use std::fmt;
use std::sync::Arc;

use futures::future::BoxFuture;

use crate::event::JetstreamEvent;

/// Operation filter for event routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Operation {
    /// Match only `"create"` operations.
    Create,
    /// Match only `"update"` operations.
    Update,
    /// Match only `"delete"` operations.
    Delete,
    /// Match all operations on the collection.
    Any,
}

impl fmt::Display for Operation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Create => write!(f, "create"),
            Self::Update => write!(f, "update"),
            Self::Delete => write!(f, "delete"),
            Self::Any => write!(f, "*"),
        }
    }
}

/// A typed commit event with extracted fields for handler convenience.
///
/// This is the value passed to each route handler. It contains all the
/// information from the raw [`JetstreamEvent`] and [`CommitData`] in a
/// flat, ergonomic structure.
#[derive(Debug, Clone)]
pub struct CommitEvent {
    /// DID of the account that produced this event.
    pub did: String,
    /// Record key.
    pub rkey: String,
    /// Collection NSID.
    pub collection: String,
    /// Operation type.
    pub operation: Operation,
    /// The record value (present for create/update, absent for delete).
    pub record: Option<serde_json::Value>,
    /// Commit revision (if available).
    pub rev: Option<String>,
    /// CID of the record (if available).
    pub cid: Option<String>,
    /// Original event timestamp in microseconds.
    pub time_us: i64,
}

/// Handler function type for the event router.
pub type RouteHandler<S> =
    Arc<dyn Fn(CommitEvent, S) -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync>;

/// A routing entry: (collection, operation) -> handler.
struct Route<S> {
    collection: String,
    operation: Operation,
    handler: RouteHandler<S>,
}

/// Builder for constructing an [`EventRouter`].
///
/// Use the `on_create`, `on_update`, `on_delete`, and `on` methods to
/// register handlers, then call [`build`](Self::build) to produce a
/// dispatch function compatible with [`AtrgApp::on_event`](crate::EventHandler).
pub struct EventRouterBuilder<S> {
    routes: Vec<Route<S>>,
}

impl<S: Clone + Send + Sync + 'static> EventRouterBuilder<S> {
    /// Create a new empty router builder.
    pub fn new() -> Self {
        Self { routes: Vec::new() }
    }

    /// Register a handler for `create` operations on a collection.
    pub fn on_create<F, Fut>(mut self, collection: impl Into<String>, handler: F) -> Self
    where
        F: Fn(CommitEvent, S) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        self.routes.push(Route {
            collection: collection.into(),
            operation: Operation::Create,
            handler: Arc::new(move |event, state| Box::pin(handler(event, state))),
        });
        self
    }

    /// Register a handler for `update` operations on a collection.
    pub fn on_update<F, Fut>(mut self, collection: impl Into<String>, handler: F) -> Self
    where
        F: Fn(CommitEvent, S) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        self.routes.push(Route {
            collection: collection.into(),
            operation: Operation::Update,
            handler: Arc::new(move |event, state| Box::pin(handler(event, state))),
        });
        self
    }

    /// Register a handler for `delete` operations on a collection.
    pub fn on_delete<F, Fut>(mut self, collection: impl Into<String>, handler: F) -> Self
    where
        F: Fn(CommitEvent, S) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        self.routes.push(Route {
            collection: collection.into(),
            operation: Operation::Delete,
            handler: Arc::new(move |event, state| Box::pin(handler(event, state))),
        });
        self
    }

    /// Register a handler for ALL operations on a collection.
    ///
    /// The handler will be called for create, update, and delete events
    /// on the specified collection.
    pub fn on<F, Fut>(mut self, collection: impl Into<String>, handler: F) -> Self
    where
        F: Fn(CommitEvent, S) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        self.routes.push(Route {
            collection: collection.into(),
            operation: Operation::Any,
            handler: Arc::new(move |event, state| Box::pin(handler(event, state))),
        });
        self
    }

    /// Build the router into a dispatch function compatible with `AtrgApp::on_event`.
    ///
    /// The returned closure can be passed directly to `AtrgApp::on_event(...)`.
    /// Events that don't match any registered route are silently ignored (with
    /// a `tracing::debug!` log).
    pub fn build(
        self,
    ) -> impl Fn(JetstreamEvent, S) -> BoxFuture<'static, anyhow::Result<()>> + Send + Sync + 'static
    {
        let router = Arc::new(EventRouter {
            routes: self.routes,
        });
        move |event, state| {
            let router = router.clone();
            Box::pin(async move { router.dispatch(event, state).await })
        }
    }
}

impl<S: Clone + Send + Sync + 'static> Default for EventRouterBuilder<S> {
    fn default() -> Self {
        Self::new()
    }
}

/// The compiled event router. Dispatches events to registered handlers.
struct EventRouter<S> {
    routes: Vec<Route<S>>,
}

impl<S: Clone + Send + Sync + 'static> EventRouter<S> {
    /// Dispatch a single event to all matching handlers.
    ///
    /// - Non-commit events (identity, account) are skipped.
    /// - Unknown operation strings are skipped.
    /// - Multiple handlers can match the same event (e.g. both an `on_create`
    ///   and an `on` handler for the same collection). All matching handlers
    ///   are invoked in registration order.
    async fn dispatch(&self, event: JetstreamEvent, state: S) -> anyhow::Result<()> {
        let commit = match event.commit {
            Some(c) => c,
            None => return Ok(()), // identity/account events — skip
        };

        let operation = match commit.operation.as_str() {
            "create" => Operation::Create,
            "update" => Operation::Update,
            "delete" => Operation::Delete,
            _ => return Ok(()),
        };

        let commit_event = CommitEvent {
            did: event.did,
            rkey: commit.rkey.clone(),
            collection: commit.collection.clone(),
            operation,
            record: commit.record,
            rev: commit.rev,
            cid: commit.cid,
            time_us: event.time_us,
        };

        let mut handled = false;
        for route in &self.routes {
            if route.collection == commit.collection
                && (route.operation == Operation::Any || route.operation == operation)
            {
                (route.handler)(commit_event.clone(), state.clone()).await?;
                handled = true;
            }
        }

        if !handled {
            tracing::debug!(
                collection = %commit.collection,
                operation = %commit.operation,
                "no handler registered for event, ignoring"
            );
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::CommitData;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// Helper to build a create commit event.
    fn make_event(collection: &str, operation: &str) -> JetstreamEvent {
        JetstreamEvent {
            did: "did:plc:test123".to_string(),
            time_us: 1_700_000_000_000_000,
            kind: "commit".to_string(),
            commit: Some(CommitData {
                collection: collection.to_string(),
                rkey: "abc123".to_string(),
                operation: operation.to_string(),
                record: Some(serde_json::json!({"text": "hello"})),
                cid: Some("bafytest".to_string()),
                rev: Some("rev1".to_string()),
            }),
            identity: None,
            account: None,
        }
    }

    /// Helper to build an identity (non-commit) event.
    fn make_identity_event() -> JetstreamEvent {
        JetstreamEvent {
            did: "did:plc:test123".to_string(),
            time_us: 1_700_000_000_000_000,
            kind: "identity".to_string(),
            commit: None,
            identity: Some(serde_json::json!({"handle": "alice.test"})),
            account: None,
        }
    }

    #[tokio::test]
    async fn on_create_handler_is_called_for_create_events() {
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();

        let handler = EventRouterBuilder::new()
            .on_create(
                "app.bsky.feed.post",
                move |event: CommitEvent, _state: ()| {
                    let c = counter_clone.clone();
                    async move {
                        assert_eq!(event.did, "did:plc:test123");
                        assert_eq!(event.collection, "app.bsky.feed.post");
                        assert_eq!(event.operation, Operation::Create);
                        assert_eq!(event.rkey, "abc123");
                        assert!(event.record.is_some());
                        c.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    }
                },
            )
            .build();

        let event = make_event("app.bsky.feed.post", "create");
        handler(event, ()).await.unwrap();

        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn unregistered_collections_are_ignored() {
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();

        let handler = EventRouterBuilder::new()
            .on_create(
                "app.bsky.feed.post",
                move |_event: CommitEvent, _state: ()| {
                    let c = counter_clone.clone();
                    async move {
                        c.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    }
                },
            )
            .build();

        // Send an event for a different collection
        let event = make_event("app.bsky.feed.like", "create");
        handler(event, ()).await.unwrap();

        // Handler should NOT have been called
        assert_eq!(counter.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn on_delete_is_not_triggered_by_create_events() {
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();

        let handler = EventRouterBuilder::new()
            .on_delete(
                "app.bsky.feed.post",
                move |_event: CommitEvent, _state: ()| {
                    let c = counter_clone.clone();
                    async move {
                        c.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    }
                },
            )
            .build();

        // Send a create event — the delete handler should NOT fire
        let event = make_event("app.bsky.feed.post", "create");
        handler(event, ()).await.unwrap();

        assert_eq!(counter.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn on_any_handler_is_triggered_for_all_operation_types() {
        let counter = Arc::new(AtomicU32::new(0));

        let handler = {
            let c = counter.clone();
            EventRouterBuilder::new()
                .on(
                    "app.bsky.feed.post",
                    move |_event: CommitEvent, _state: ()| {
                        let c = c.clone();
                        async move {
                            c.fetch_add(1, Ordering::SeqCst);
                            Ok(())
                        }
                    },
                )
                .build()
        };

        // Send create
        handler(make_event("app.bsky.feed.post", "create"), ())
            .await
            .unwrap();
        // Send update
        handler(make_event("app.bsky.feed.post", "update"), ())
            .await
            .unwrap();
        // Send delete
        handler(make_event("app.bsky.feed.post", "delete"), ())
            .await
            .unwrap();

        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn identity_events_are_silently_skipped() {
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();

        let handler = EventRouterBuilder::new()
            .on(
                "app.bsky.feed.post",
                move |_event: CommitEvent, _state: ()| {
                    let c = counter_clone.clone();
                    async move {
                        c.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    }
                },
            )
            .build();

        handler(make_identity_event(), ()).await.unwrap();

        assert_eq!(counter.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn multiple_handlers_for_same_collection_all_fire() {
        let create_counter = Arc::new(AtomicU32::new(0));
        let any_counter = Arc::new(AtomicU32::new(0));

        let handler = {
            let cc = create_counter.clone();
            let ac = any_counter.clone();
            EventRouterBuilder::new()
                .on_create(
                    "app.bsky.feed.post",
                    move |_event: CommitEvent, _state: ()| {
                        let c = cc.clone();
                        async move {
                            c.fetch_add(1, Ordering::SeqCst);
                            Ok(())
                        }
                    },
                )
                .on(
                    "app.bsky.feed.post",
                    move |_event: CommitEvent, _state: ()| {
                        let c = ac.clone();
                        async move {
                            c.fetch_add(1, Ordering::SeqCst);
                            Ok(())
                        }
                    },
                )
                .build()
        };

        handler(make_event("app.bsky.feed.post", "create"), ())
            .await
            .unwrap();

        // Both handlers should have fired
        assert_eq!(create_counter.load(Ordering::SeqCst), 1);
        assert_eq!(any_counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn state_is_passed_to_handlers() {
        #[derive(Clone)]
        struct TestState {
            prefix: String,
        }

        let result = Arc::new(tokio::sync::Mutex::new(String::new()));
        let result_clone = result.clone();

        let handler = EventRouterBuilder::new()
            .on_create(
                "app.bsky.feed.post",
                move |event: CommitEvent, state: TestState| {
                    let r = result_clone.clone();
                    async move {
                        let mut locked = r.lock().await;
                        *locked = format!("{}:{}", state.prefix, event.did);
                        Ok(())
                    }
                },
            )
            .build();

        let state = TestState {
            prefix: "hello".to_string(),
        };
        handler(make_event("app.bsky.feed.post", "create"), state)
            .await
            .unwrap();

        let locked = result.lock().await;
        assert_eq!(*locked, "hello:did:plc:test123");
    }
}
