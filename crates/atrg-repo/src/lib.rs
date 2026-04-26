#![deny(unsafe_code)]
#![warn(missing_docs)]

//! # atrg-repo
//!
//! Record repository helpers for the at-rust-go framework.
//!
//! Provides ergonomic typed helpers for AT Protocol record repository
//! operations (CRUD), blob uploads, and AT-URI/TID utilities.
//!
//! # Overview
//!
//! - [`Repo`] — high-level client for `com.atproto.repo.*` XRPC calls
//! - [`AtUri`] — parsed AT Protocol URI (`at://did/collection/rkey`)
//! - [`Tid`] — base32-sortable timestamp identifiers for record keys
//! - [`BlobRef`] / [`StrongRef`] — typed references to blobs and records
//! - [`Page`] / [`Record`] — paginated listing and record wrapper types

pub mod at_uri;
pub mod blob;
pub mod error;
pub mod repo;
pub mod tid;
pub mod types;

pub use at_uri::AtUri;
pub use blob::{upload_blob, upload_blob_from_url};
pub use error::RepoError;
pub use repo::Repo;
pub use tid::Tid;
pub use types::{BlobLink, BlobRef, Page, Record, StrongRef};
