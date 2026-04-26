#![deny(unsafe_code)]
#![warn(missing_docs)]
//! Feed generator framework for at-rust-go.
//!
//! Provides a [`FeedGenerator`] builder for registering AT Protocol feeds
//! and automatically serving `app.bsky.feed.describeFeedGenerator` and
//! `app.bsky.feed.getFeedSkeleton` XRPC endpoints.
//!
//! # Example
//!
//! ```rust,ignore
//! use atrg_feed::FeedGenerator;
//!
//! let feeds = FeedGenerator::new("did:web:feeds.example.com")
//!     .feed("my-feed", "My Custom Feed", None, my_handler)
//!     .into_router();
//!
//! AtrgApp::new()
//!     .mount(feeds)
//!     .run()
//!     .await?;
//! ```

pub mod generator;
pub mod handler;
pub mod routes;
pub mod types;

pub use generator::FeedGenerator;
pub use handler::{FeedHandler, FeedRequest};
pub use types::{DescribeFeedGeneratorResponse, FeedConfig, FeedDescription, FeedSkeleton, SkeletonItem};
