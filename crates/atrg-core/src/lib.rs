#![deny(unsafe_code)]
#![warn(missing_docs)]
//! Core framework crate for at-rust-go: AppState, Config, AtrgApp builder, and error types.

pub mod app;
pub mod config;
pub mod cors;
pub mod error;
pub mod health;
pub mod pagination;
pub mod request_id;
pub mod security;
pub mod state;

pub use app::AtrgApp;
pub use config::Config;
pub use error::{AtrgError, AtrgResult};
pub use state::AppState;

/// Returns the crate version.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
