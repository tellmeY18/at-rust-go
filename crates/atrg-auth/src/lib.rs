#![deny(unsafe_code)]
#![warn(missing_docs)]
//! OAuth authentication wiring for at-rust-go on top of the AT Protocol.
//!
//! Provides OAuth login/callback/logout routes, session management,
//! AT Protocol JWT verification, and `AuthUser`/`RequireAuth` extractors.

pub mod extractor;
pub mod jwt;
pub mod oauth;
pub mod routes;
pub mod session;

pub use extractor::{AuthUser, RequireAuth};
pub use session::{AtrgSession, AuthSource, OAuthState};

/// Returns the crate version.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
