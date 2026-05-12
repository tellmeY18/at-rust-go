#![deny(unsafe_code)]
#![warn(missing_docs)]
//! SMTP email sending and OTP verification for at-rust-go.
//!
//! Provides async email delivery via SMTP and a two-step OTP verification
//! flow for institution/organization membership verification.
//!
//! When SMTP is not configured, OTPs are logged to stdout (dev mode).

pub mod config;
pub mod otp;
pub mod send;

pub use config::EmailConfig;
pub use otp::{generate_otp, send_otp, verify_otp};
pub use send::send_email;

/// SQL for creating the OTP codes table (SQLite).
pub const CREATE_OTP_TABLE_SQLITE: &str = r#"
CREATE TABLE IF NOT EXISTS atrg_otp_codes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    did TEXT NOT NULL,
    email TEXT NOT NULL,
    code TEXT NOT NULL,
    expires_at INTEGER NOT NULL,
    used INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE INDEX IF NOT EXISTS idx_atrg_otp_did_email ON atrg_otp_codes(did, email);
"#;

/// SQL for creating the OTP codes table (PostgreSQL).
pub const CREATE_OTP_TABLE_POSTGRES: &str = r#"
CREATE TABLE IF NOT EXISTS atrg_otp_codes (
    id BIGSERIAL PRIMARY KEY,
    did TEXT NOT NULL,
    email TEXT NOT NULL,
    code TEXT NOT NULL,
    expires_at BIGINT NOT NULL,
    used BOOLEAN NOT NULL DEFAULT FALSE,
    created_at BIGINT NOT NULL DEFAULT EXTRACT(EPOCH FROM NOW())::bigint
);
CREATE INDEX IF NOT EXISTS idx_atrg_otp_did_email ON atrg_otp_codes(did, email);
"#;

/// Validate that an email domain is in the allowed list.
/// Returns `Ok(())` if allowed, `Err` with a message if not.
pub fn validate_domain(email: &str, allowed_domains: &[String]) -> Result<(), String> {
    if allowed_domains.is_empty() {
        return Ok(()); // No restriction
    }
    let domain = email.split('@').nth(1).unwrap_or("").to_lowercase();
    if allowed_domains.iter().any(|d| d.to_lowercase() == domain) {
        Ok(())
    } else {
        Err(format!(
            "Email domain '{}' is not allowed. Allowed: {}",
            domain,
            allowed_domains.join(", ")
        ))
    }
}
