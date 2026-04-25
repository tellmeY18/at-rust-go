//! AT Protocol JWT verification.
//!
//! Verifies PDS-issued JWTs by resolving the issuer's signing key
//! via the identity resolver.

use jsonwebtoken::{decode, decode_header, DecodingKey, Validation};
use serde::{Deserialize, Serialize};

/// Claims extracted from an AT Protocol JWT.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwtClaims {
    /// Issuer — the PDS DID.
    pub iss: String,
    /// Subject — the user's DID.
    pub sub: String,
    /// Audience — should match this server's host.
    pub aud: Option<String>,
    /// Expiration time (Unix timestamp).
    pub exp: Option<u64>,
    /// Not before (Unix timestamp).
    pub nbf: Option<u64>,
    /// Scope string.
    pub scope: Option<String>,
}

/// Errors from JWT verification.
#[derive(Debug, thiserror::Error)]
pub enum JwtError {
    /// The token is not structurally valid.
    #[error("malformed JWT: {0}")]
    Malformed(String),
    /// The token has expired.
    #[error("JWT expired")]
    Expired,
    /// The audience claim doesn't match.
    #[error("JWT audience mismatch: expected {expected}, got {actual}")]
    AudienceMismatch {
        /// Expected audience.
        expected: String,
        /// Actual audience in the token.
        actual: String,
    },
    /// The issuer could not be resolved.
    #[error("could not resolve JWT issuer: {0}")]
    IssuerResolution(String),
    /// Signature verification failed.
    #[error("JWT signature verification failed: {0}")]
    SignatureInvalid(String),
}

/// Check if a token string looks like a JWT (3 base64url segments separated by dots).
pub fn looks_like_jwt(token: &str) -> bool {
    let parts: Vec<&str> = token.split('.').collect();
    parts.len() == 3 && parts.iter().all(|p| !p.is_empty())
}

/// Decode JWT claims WITHOUT verifying the signature.
///
/// This is used for the initial dispatch to determine if a bearer token
/// is a JWT or an atrg session token.
pub fn decode_claims_unverified(token: &str) -> Result<JwtClaims, JwtError> {
    let header = decode_header(token).map_err(|e| JwtError::Malformed(e.to_string()))?;

    // Decode payload without verification
    let mut validation = Validation::new(header.alg);
    validation.insecure_disable_signature_validation();
    validation.validate_exp = false;
    validation.validate_nbf = false;
    validation.validate_aud = false;
    validation.required_spec_claims.clear();

    let token_data = decode::<JwtClaims>(token, &DecodingKey::from_secret(b""), &validation)
        .map_err(|e| JwtError::Malformed(e.to_string()))?;

    Ok(token_data.claims)
}

/// Verify a JWT's expiration claim.
pub fn verify_expiration(claims: &JwtClaims) -> Result<(), JwtError> {
    if let Some(exp) = claims.exp {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if now > exp {
            return Err(JwtError::Expired);
        }
    }
    Ok(())
}

/// Verify the audience claim matches the expected value.
pub fn verify_audience(claims: &JwtClaims, expected_host: &str) -> Result<(), JwtError> {
    if let Some(ref aud) = claims.aud {
        if !aud.contains(expected_host) {
            return Err(JwtError::AudienceMismatch {
                expected: expected_host.to_string(),
                actual: aud.clone(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks_like_jwt_valid() {
        assert!(looks_like_jwt(
            "eyJhbGciOiJIUzI1NiJ9.eyJpc3MiOiJ0ZXN0In0.sig"
        ));
    }

    #[test]
    fn looks_like_jwt_not_jwt() {
        assert!(!looks_like_jwt("just-a-session-token"));
        assert!(!looks_like_jwt("two.parts"));
        assert!(!looks_like_jwt(""));
        assert!(!looks_like_jwt("a..b"));
    }

    #[test]
    fn decode_unverified_valid() {
        // Create a minimal unsigned JWT for testing
        // Header: {"alg":"HS256"}
        // Payload: {"iss":"did:plc:test","sub":"did:plc:user"}
        let header = base64_url_encode(br#"{"alg":"HS256"}"#);
        let payload = base64_url_encode(br#"{"iss":"did:plc:test","sub":"did:plc:user"}"#);
        let token = format!("{header}.{payload}.fakesig");

        let claims = decode_claims_unverified(&token).unwrap();
        assert_eq!(claims.iss, "did:plc:test");
        assert_eq!(claims.sub, "did:plc:user");
    }

    #[test]
    fn decode_unverified_malformed() {
        let result = decode_claims_unverified("not-a-jwt");
        assert!(result.is_err());
    }

    #[test]
    fn verify_expiration_valid() {
        let claims = JwtClaims {
            iss: "test".into(),
            sub: "test".into(),
            aud: None,
            exp: Some(u64::MAX),
            nbf: None,
            scope: None,
        };
        assert!(verify_expiration(&claims).is_ok());
    }

    #[test]
    fn verify_expiration_expired() {
        let claims = JwtClaims {
            iss: "test".into(),
            sub: "test".into(),
            aud: None,
            exp: Some(0),
            nbf: None,
            scope: None,
        };
        assert!(matches!(verify_expiration(&claims), Err(JwtError::Expired)));
    }

    #[test]
    fn verify_expiration_none_is_ok() {
        let claims = JwtClaims {
            iss: "test".into(),
            sub: "test".into(),
            aud: None,
            exp: None,
            nbf: None,
            scope: None,
        };
        assert!(verify_expiration(&claims).is_ok());
    }

    #[test]
    fn verify_audience_match() {
        let claims = JwtClaims {
            iss: "test".into(),
            sub: "test".into(),
            aud: Some("https://myapp.example.com".into()),
            exp: None,
            nbf: None,
            scope: None,
        };
        assert!(verify_audience(&claims, "myapp.example.com").is_ok());
    }

    #[test]
    fn verify_audience_mismatch() {
        let claims = JwtClaims {
            iss: "test".into(),
            sub: "test".into(),
            aud: Some("https://other.example.com".into()),
            exp: None,
            nbf: None,
            scope: None,
        };
        assert!(matches!(
            verify_audience(&claims, "myapp.example.com"),
            Err(JwtError::AudienceMismatch { .. })
        ));
    }

    #[test]
    fn verify_audience_none_is_ok() {
        let claims = JwtClaims {
            iss: "test".into(),
            sub: "test".into(),
            aud: None,
            exp: None,
            nbf: None,
            scope: None,
        };
        assert!(verify_audience(&claims, "myapp.example.com").is_ok());
    }

    fn base64_url_encode(data: &[u8]) -> String {
        use base64::Engine;
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(data)
    }
}
