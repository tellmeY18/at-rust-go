//! Email configuration.

use serde::Deserialize;

/// SMTP email configuration, deserialized from `[email]` in atrg.toml.
#[derive(Debug, Clone, Deserialize)]
pub struct EmailConfig {
    /// SMTP host (e.g. "smtp.gmail.com")
    pub host: String,
    /// SMTP port (default: 587)
    #[serde(default = "default_port")]
    pub port: u16,
    /// SMTP username
    pub username: String,
    /// SMTP password (use env var in production)
    pub password: String,
    /// From address (e.g. "MyApp <noreply@example.com>")
    pub from: String,
    /// Encryption mode: "starttls" (default), "tls", or "none"
    #[serde(default = "default_encryption")]
    pub encryption: String,
    /// OTP expiry in seconds (default: 600 = 10 minutes)
    #[serde(default = "default_otp_expiry")]
    pub otp_expiry_secs: u64,
}

fn default_port() -> u16 {
    587
}
fn default_encryption() -> String {
    "starttls".to_string()
}
fn default_otp_expiry() -> u64 {
    600
}
