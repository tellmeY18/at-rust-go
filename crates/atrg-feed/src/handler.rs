//! Feed handler type for producing feed skeletons.
//!
//! A [`FeedHandler`] is a function that takes a [`FeedRequest`] and the
//! application state, returning a [`FeedSkeleton`] or an XRPC error.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use atrg_core::state::AppState;
use atrg_xrpc::XrpcError;

use crate::types::FeedSkeleton;

/// Parameters passed to a feed handler when a skeleton is requested.
#[derive(Debug, Clone)]
pub struct FeedRequest {
    /// The full AT-URI of the feed being requested.
    pub feed: String,
    /// Pagination cursor from the client. `None` on the first page.
    pub cursor: Option<String>,
    /// Maximum number of items to return (already clamped to 1..=100).
    pub limit: usize,
    /// DID of the requesting user, if authenticated.
    pub requester_did: Option<String>,
}

/// A boxed future returned by feed handlers.
type FeedFuture = Pin<Box<dyn Future<Output = Result<FeedSkeleton, XrpcError>> + Send>>;

/// A feed handler function.
///
/// Takes a [`FeedRequest`] and [`AppState`], returning a future that
/// resolves to a [`FeedSkeleton`] or an [`XrpcError`].
///
/// Construct one by wrapping an async function with [`Arc`]:
///
/// ```rust,ignore
/// let handler: FeedHandler = Arc::new(|req, state| {
///     Box::pin(async move {
///         Ok(FeedSkeleton { feed: vec![], cursor: None })
///     })
/// });
/// ```
pub type FeedHandler = Arc<dyn Fn(FeedRequest, AppState) -> FeedFuture + Send + Sync>;
