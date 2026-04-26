#![deny(unsafe_code)]
#![warn(missing_docs)]
//! Labeler framework for at-rust-go.
//!
//! Provides a [`LabelService`] for creating, storing, and streaming
//! AT Protocol labels per the `com.atproto.label` lexicon.

pub mod label;
pub mod routes;
pub mod signing;
pub mod store;
pub mod types;

pub use label::LabelService;
pub use types::{Label, LabelValue, LabelerConfig, SignedLabel};
