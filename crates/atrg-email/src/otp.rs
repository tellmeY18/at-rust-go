//! OTP generation, storage, and verification.

use crate::config::EmailConfig;
use atrg_db::DbPool;

/// Generate a 6-digit OTP code.
pub fn generate_otp() -> String {
    use rand::Rng;
    let code: u32 = rand::thread_rng().gen_range(100_000..1_000_000);
    format!("{:06}", code)
}

/// Generate an OTP, store it in the database, and send it via email.
///
/// If email config is `None`, the OTP is logged to stdout (dev mode).
pub async fn send_otp(
    pool: &DbPool,
    email_config: Option<&EmailConfig>,
    did: &str,
    email: &str,
) -> anyhow::Result<()> {
    let code = generate_otp();
    let expiry_secs = email_config.map(|c| c.otp_expiry_secs).unwrap_or(600);
    let expires_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
        + expiry_secs as i64;

    // Store OTP in database
    match pool {
        #[cfg(feature = "sqlite")]
        DbPool::Sqlite(p) => {
            sqlx::query(
                "INSERT INTO atrg_otp_codes (did, email, code, expires_at) VALUES (?1, ?2, ?3, ?4)",
            )
            .bind(did)
            .bind(email)
            .bind(&code)
            .bind(expires_at)
            .execute(p)
            .await?;
        }
        #[cfg(feature = "postgres")]
        DbPool::Postgres(p) => {
            sqlx::query(
                "INSERT INTO atrg_otp_codes (did, email, code, expires_at) VALUES ($1, $2, $3, $4)",
            )
            .bind(did)
            .bind(email)
            .bind(&code)
            .bind(expires_at)
            .execute(p)
            .await?;
        }
    }

    // Send email (or log in dev mode)
    let subject = "Your verification code";
    let body = format!(
        "Your verification code is: {}\n\nThis code expires in {} minutes.\nIf you did not request this, ignore this email.",
        code,
        expiry_secs / 60
    );

    crate::send::send_email(email_config, email, subject, &body).await?;
    Ok(())
}

/// Verify an OTP code. Returns `true` if valid and not expired/used.
/// Marks the OTP as used on success.
pub async fn verify_otp(pool: &DbPool, did: &str, email: &str, code: &str) -> anyhow::Result<bool> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let valid: i64 = match pool {
        #[cfg(feature = "sqlite")]
        DbPool::Sqlite(p) => {
            sqlx::query_scalar(
                "SELECT COUNT(*) FROM atrg_otp_codes WHERE did = ?1 AND email = ?2 AND code = ?3 AND expires_at > ?4 AND used = 0"
            )
            .bind(did).bind(email).bind(code).bind(now)
            .fetch_one(p).await?
        }
        #[cfg(feature = "postgres")]
        DbPool::Postgres(p) => {
            sqlx::query_scalar(
                "SELECT COUNT(*) FROM atrg_otp_codes WHERE did = $1 AND email = $2 AND code = $3 AND expires_at > $4 AND used = FALSE"
            )
            .bind(did).bind(email).bind(code).bind(now)
            .fetch_one(p).await?
        }
    };

    if valid == 0 {
        return Ok(false);
    }

    // Mark as used
    match pool {
        #[cfg(feature = "sqlite")]
        DbPool::Sqlite(p) => {
            sqlx::query(
                "UPDATE atrg_otp_codes SET used = 1 WHERE did = ?1 AND email = ?2 AND code = ?3",
            )
            .bind(did)
            .bind(email)
            .bind(code)
            .execute(p)
            .await?;
        }
        #[cfg(feature = "postgres")]
        DbPool::Postgres(p) => {
            sqlx::query(
                "UPDATE atrg_otp_codes SET used = TRUE WHERE did = $1 AND email = $2 AND code = $3",
            )
            .bind(did)
            .bind(email)
            .bind(code)
            .execute(p)
            .await?;
        }
    }

    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_otp_format() {
        let otp = generate_otp();
        assert_eq!(otp.len(), 6);
        assert!(otp.chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn test_generate_otp_varies() {
        // Generate 10 OTPs — at least 2 should be different
        let otps: Vec<String> = (0..10).map(|_| generate_otp()).collect();
        let unique: std::collections::HashSet<&String> = otps.iter().collect();
        assert!(unique.len() > 1, "OTPs should vary");
    }

    #[cfg(feature = "sqlite")]
    #[tokio::test]
    async fn test_otp_roundtrip() {
        let pool = atrg_db::connect("sqlite::memory:").await.unwrap();
        if let DbPool::Sqlite(p) = &pool {
            sqlx::query(crate::CREATE_OTP_TABLE_SQLITE)
                .execute(p)
                .await
                .unwrap();
        }

        // Send OTP (dev mode — no SMTP)
        send_otp(&pool, None, "did:plc:test", "user@example.com")
            .await
            .unwrap();

        // We can't verify the exact code since it's random, but we can test the flow
        // by inserting a known code directly
        if let DbPool::Sqlite(p) = &pool {
            let expires = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64
                + 600;
            sqlx::query(
                "INSERT INTO atrg_otp_codes (did, email, code, expires_at) VALUES (?1, ?2, ?3, ?4)",
            )
            .bind("did:plc:test2")
            .bind("test@example.com")
            .bind("123456")
            .bind(expires)
            .execute(p)
            .await
            .unwrap();
        }

        // Verify correct code
        let result = verify_otp(&pool, "did:plc:test2", "test@example.com", "123456")
            .await
            .unwrap();
        assert!(result);

        // Second verification fails (already used)
        let result = verify_otp(&pool, "did:plc:test2", "test@example.com", "123456")
            .await
            .unwrap();
        assert!(!result);

        // Wrong code fails
        let result = verify_otp(&pool, "did:plc:test2", "test@example.com", "999999")
            .await
            .unwrap();
        assert!(!result);
    }
}
