//! SMTP email delivery.

use crate::config::EmailConfig;
use anyhow::Context;

/// Send an email via SMTP.
///
/// If `config` is `None`, logs the email to stdout (dev mode).
pub async fn send_email(
    config: Option<&EmailConfig>,
    to: &str,
    subject: &str,
    body: &str,
) -> anyhow::Result<()> {
    let Some(cfg) = config else {
        tracing::info!(
            to = %to,
            subject = %subject,
            "EMAIL (dev mode, SMTP not configured):\n{}", body
        );
        return Ok(());
    };

    use lettre::message::header::ContentType;
    use lettre::transport::smtp::authentication::Credentials;
    use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

    let email = Message::builder()
        .from(cfg.from.parse().context("invalid 'from' address")?)
        .to(to.parse().context("invalid 'to' address")?)
        .subject(subject)
        .header(ContentType::TEXT_PLAIN)
        .body(body.to_string())
        .context("failed to build email message")?;

    let transport = match cfg.encryption.as_str() {
        "tls" => AsyncSmtpTransport::<Tokio1Executor>::relay(&cfg.host)?
            .port(cfg.port)
            .credentials(Credentials::new(cfg.username.clone(), cfg.password.clone()))
            .build(),
        "none" => AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&cfg.host)
            .port(cfg.port)
            .credentials(Credentials::new(cfg.username.clone(), cfg.password.clone()))
            .build(),
        _ => {
            // Default: starttls
            AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&cfg.host)?
                .port(cfg.port)
                .credentials(Credentials::new(cfg.username.clone(), cfg.password.clone()))
                .build()
        }
    };

    transport.send(email).await.context("SMTP send failed")?;
    tracing::info!(to = %to, subject = %subject, "email sent successfully");
    Ok(())
}
