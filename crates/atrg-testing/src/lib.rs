#![deny(unsafe_code)]
#![warn(missing_docs)]
//! Test utilities for at-rust-go: mock clients, fake Jetstream, test app builder.
//!
//! Pull this crate in under `[dev-dependencies]` to write fast, deterministic
//! tests for your atrg handlers without any network access.

pub mod fake_jetstream;
pub mod mock_client;
pub mod test_app;

pub use fake_jetstream::FakeJetstream;
pub use mock_client::MockAtprotoClient;
pub use test_app::{seed_session, test_app, test_state};
