//! Feed generator builder.
//!
//! [`FeedGenerator`] collects feed registrations and produces an Axum router
//! that serves `app.bsky.feed.describeFeedGenerator` and
//! `app.bsky.feed.getFeedSkeleton` XRPC endpoints.

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;

use axum::Router;

use atrg_core::AppState;

use crate::handler::{FeedHandler, FeedRequest};
use crate::routes;
use crate::types::{FeedConfig, FeedSkeleton};

/// A feed generator that manages multiple feeds and produces Axum routes.
///
/// # Example
///
/// ```rust,ignore
/// let feeds = FeedGenerator::new("did:web:feeds.example.com")
///     .feed("my-feed", "My Custom Feed", None, my_handler)
///     .feed("other-feed", "Other Feed", Some("A description"), other_handler)
///     .into_router();
/// ```
pub struct FeedGenerator {
    /// The DID of the feed generator service.
    did: String,
    /// Registered feeds: id -> (config, handler).
    feeds: HashMap<String, (FeedConfig, FeedHandler)>,
}

impl FeedGenerator {
    /// Create a new feed generator with the given service DID.
    ///
    /// The DID identifies this feed generator on the AT Protocol network
    /// (e.g. `"did:web:feeds.example.com"`).
    pub fn new(did: impl Into<String>) -> Self {
        Self {
            did: did.into(),
            feeds: HashMap::new(),
        }
    }

    /// Register a feed with the given ID, display name, optional description,
    /// and handler function.
    ///
    /// The handler is called each time a client requests the feed skeleton.
    /// It receives a [`FeedRequest`] and the [`AppState`], and must return
    /// a [`FeedSkeleton`] or an [`XrpcError`](atrg_xrpc::XrpcError).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// async fn chronological(
    ///     req: FeedRequest,
    ///     state: AppState,
    /// ) -> Result<FeedSkeleton, XrpcError> {
    ///     // query DB, build skeleton ...
    ///     Ok(FeedSkeleton { feed: vec![], cursor: None })
    /// }
    ///
    /// let gen = FeedGenerator::new("did:web:example.com")
    ///     .feed("chrono", "Chronological", Some("Latest posts"), chronological);
    /// ```
    pub fn feed<F, Fut>(
        mut self,
        id: &str,
        display_name: &str,
        description: Option<&str>,
        handler: F,
    ) -> Self
    where
        F: Fn(FeedRequest, AppState) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<FeedSkeleton, atrg_xrpc::XrpcError>> + Send + 'static,
    {
        let config = FeedConfig {
            id: id.to_string(),
            display_name: display_name.to_string(),
            description: description.map(|s| s.to_string()),
            avatar: None,
        };
        let handler: FeedHandler = Arc::new(move |req, state| Box::pin(handler(req, state)));
        self.feeds.insert(id.to_string(), (config, handler));
        self
    }

    /// Register a feed from an existing [`FeedConfig`] and handler function.
    ///
    /// This is useful when feed configurations are loaded from `atrg.toml`
    /// or another config source.
    pub fn feed_with_config<F, Fut>(mut self, config: FeedConfig, handler: F) -> Self
    where
        F: Fn(FeedRequest, AppState) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<FeedSkeleton, atrg_xrpc::XrpcError>> + Send + 'static,
    {
        let id = config.id.clone();
        let handler: FeedHandler = Arc::new(move |req, state| Box::pin(handler(req, state)));
        self.feeds.insert(id, (config, handler));
        self
    }

    /// Return the service DID for this feed generator.
    pub fn did(&self) -> &str {
        &self.did
    }

    /// Return the number of registered feeds.
    pub fn feed_count(&self) -> usize {
        self.feeds.len()
    }

    /// Build an Axum router with the XRPC feed endpoints.
    ///
    /// Registers:
    /// - `GET /xrpc/app.bsky.feed.describeFeedGenerator`
    /// - `GET /xrpc/app.bsky.feed.getFeedSkeleton`
    pub fn into_router(self) -> Router<AppState> {
        routes::build_router(self.did, self.feeds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_generator_has_no_feeds() {
        let gen = FeedGenerator::new("did:web:example.com");
        assert_eq!(gen.did(), "did:web:example.com");
        assert_eq!(gen.feed_count(), 0);
    }

    #[test]
    fn register_feeds_increments_count() {
        let gen = FeedGenerator::new("did:web:example.com")
            .feed("a", "Feed A", None, |_req, _state| async {
                Ok(FeedSkeleton {
                    feed: vec![],
                    cursor: None,
                })
            })
            .feed("b", "Feed B", Some("desc"), |_req, _state| async {
                Ok(FeedSkeleton {
                    feed: vec![],
                    cursor: None,
                })
            });
        assert_eq!(gen.feed_count(), 2);
    }

    #[test]
    fn duplicate_id_overwrites() {
        let gen = FeedGenerator::new("did:web:example.com")
            .feed("a", "First", None, |_req, _state| async {
                Ok(FeedSkeleton {
                    feed: vec![],
                    cursor: None,
                })
            })
            .feed("a", "Second", None, |_req, _state| async {
                Ok(FeedSkeleton {
                    feed: vec![],
                    cursor: None,
                })
            });
        assert_eq!(gen.feed_count(), 1);
    }

    #[test]
    fn feed_with_config_registers_feed() {
        let config = FeedConfig {
            id: "custom-feed".to_string(),
            display_name: "Custom Feed".to_string(),
            description: Some("A custom feed from config".to_string()),
            avatar: None,
        };
        let gen = FeedGenerator::new("did:web:example.com").feed_with_config(
            config,
            |_req, _state| async {
                Ok(FeedSkeleton {
                    feed: vec![],
                    cursor: None,
                })
            },
        );
        assert_eq!(gen.feed_count(), 1);
    }
}
