//! AT Protocol OAuth primitives: PKCE, DPoP, PDS metadata discovery, and token exchange.
//!
//! Implements the cryptographic building blocks and HTTP helpers required to
//! complete an AT Protocol OAuth 2.0 authorization code flow with PKCE
//! ([RFC 7636](https://datatracker.ietf.org/doc/html/rfc7636)) and DPoP
//! ([RFC 9449](https://datatracker.ietf.org/doc/html/rfc9449)) proof-of-possession.

use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::Digest;

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Base64url-encode raw bytes (no padding).
fn base64_url_encode(data: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(data)
}

/// Base64url-encode the UTF-8 bytes of a string (no padding).
#[allow(dead_code)]
fn base64_url_encode_str(s: &str) -> String {
    base64_url_encode(s.as_bytes())
}

/// Generate a random string from `len` random bytes, base64url-encoded.
///
/// The returned string is `ceil(len * 4/3)` characters long (without padding).
fn generate_random_string(len: usize) -> String {
    use rand::Rng;
    let mut bytes = vec![0u8; len];
    rand::thread_rng().fill(&mut bytes[..]);
    base64_url_encode(&bytes)
}

// ---------------------------------------------------------------------------
// 1. PKCE (RFC 7636)
// ---------------------------------------------------------------------------

/// Generate a random PKCE code verifier (43 chars, URL-safe).
///
/// Produces 32 random bytes and base64url-encodes them (no padding),
/// yielding a 43-character string that satisfies RFC 7636 §4.1
/// (43–128 unreserved characters).
pub fn generate_code_verifier() -> String {
    generate_random_string(32)
}

/// Compute the S256 code challenge from a PKCE verifier.
///
/// Returns `BASE64URL(SHA256(ASCII(code_verifier)))` per RFC 7636 §4.2.
pub fn compute_code_challenge(verifier: &str) -> String {
    let hash = sha2::Sha256::digest(verifier.as_bytes());
    base64_url_encode(&hash)
}

// ---------------------------------------------------------------------------
// 2. DPoP (RFC 9449)
// ---------------------------------------------------------------------------

/// An ephemeral ES256 DPoP keypair for one OAuth session.
///
/// The private key is serialised as a JWK string so it can be stored in the
/// database alongside the OAuth state. The public key is kept as a
/// [`serde_json::Value`] for embedding in JWT headers.
#[derive(Debug, Clone)]
pub struct DpopKeyPair {
    /// The private key serialized as a JWK string (for DB storage).
    pub private_key_jwk: String,
    /// The public key as a JWK JSON value (for JWT headers).
    pub public_key_jwk: serde_json::Value,
}

/// Generate a new ephemeral ES256 DPoP keypair.
///
/// Uses `p256::SecretKey::random` with the OS CSPRNG.
pub fn generate_dpop_keypair() -> anyhow::Result<DpopKeyPair> {
    let secret_key = p256::SecretKey::random(&mut rand::rngs::OsRng);
    let private_key_jwk = secret_key.to_jwk_string().to_string();
    let public_key_jwk_str = secret_key.public_key().to_jwk_string();
    let public_key_jwk: serde_json::Value = serde_json::from_str(&public_key_jwk_str)?;

    Ok(DpopKeyPair {
        private_key_jwk,
        public_key_jwk,
    })
}

/// Claims embedded in a DPoP proof JWT (RFC 9449 §4.2).
#[derive(Debug, Serialize, Deserialize)]
struct DpopClaims {
    /// Unique token identifier.
    jti: String,
    /// HTTP method bound to this proof.
    htm: String,
    /// HTTP target URI bound to this proof.
    htu: String,
    /// Issued-at (Unix timestamp).
    iat: u64,
    /// Server-provided nonce (if any).
    #[serde(skip_serializing_if = "Option::is_none")]
    nonce: Option<String>,
    /// Access-token hash (`base64url(sha256(at))`), present when the DPoP
    /// proof accompanies a resource request.
    #[serde(skip_serializing_if = "Option::is_none")]
    ath: Option<String>,
}

/// Create a DPoP proof JWT.
///
/// # Arguments
///
/// * `private_key_jwk` — the stored JWK string of the ES256 private key.
/// * `htm` — HTTP method (e.g. `"POST"`).
/// * `htu` — HTTP target URI (e.g. the token endpoint URL).
/// * `nonce` — optional server-provided nonce.
/// * `access_token` — optional access token; when present the `ath` claim is
///   included (base64url-encoded SHA-256 hash of the token).
pub fn create_dpop_proof(
    private_key_jwk: &str,
    htm: &str,
    htu: &str,
    nonce: Option<&str>,
    access_token: Option<&str>,
) -> anyhow::Result<String> {
    use jsonwebtoken::jwk::{
        AlgorithmParameters, CommonParameters, EllipticCurve, EllipticCurveKeyParameters,
        EllipticCurveKeyType, Jwk,
    };
    use p256::elliptic_curve::sec1::ToEncodedPoint;
    use p256::pkcs8::EncodePrivateKey;

    // 1. Reconstruct the secret key from JWK
    let secret_key = p256::SecretKey::from_jwk_str(private_key_jwk)
        .map_err(|e| anyhow::anyhow!("invalid DPoP private key JWK: {e}"))?;

    // 2. Extract public key x, y coordinates (uncompressed SEC1 point)
    let public_key = secret_key.public_key();
    let point = public_key.to_encoded_point(false);
    let x_bytes = point
        .x()
        .ok_or_else(|| anyhow::anyhow!("missing x coordinate on public key"))?;
    let y_bytes = point
        .y()
        .ok_or_else(|| anyhow::anyhow!("missing y coordinate on public key"))?;
    let x_b64 = base64_url_encode(x_bytes);
    let y_b64 = base64_url_encode(y_bytes);

    // 3. Build the JWK for the JWT header (public key only — no `d` field)
    let jwk = Jwk {
        common: CommonParameters {
            public_key_use: None,
            key_operations: None,
            key_algorithm: None,
            key_id: None,
            x509_url: None,
            x509_chain: None,
            x509_sha1_fingerprint: None,
            x509_sha256_fingerprint: None,
        },
        algorithm: AlgorithmParameters::EllipticCurve(EllipticCurveKeyParameters {
            key_type: EllipticCurveKeyType::EC,
            curve: EllipticCurve::P256,
            x: x_b64,
            y: y_b64,
        }),
    };

    // 4. Build JWT header: alg=ES256, typ=dpop+jwt, jwk=<public key>
    let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::ES256);
    header.typ = Some("dpop+jwt".to_string());
    header.jwk = Some(jwk);

    // 5. Build claims
    let iat = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let ath = access_token.map(|at| {
        let hash = sha2::Sha256::digest(at.as_bytes());
        base64_url_encode(&hash)
    });

    let claims = DpopClaims {
        jti: generate_random_string(16),
        htm: htm.to_string(),
        htu: htu.to_string(),
        iat,
        nonce: nonce.map(|s| s.to_string()),
        ath,
    };

    // 6. Convert private key to PKCS8 PEM for jsonwebtoken's EncodingKey
    let pem = secret_key
        .to_pkcs8_pem(p256::pkcs8::LineEnding::LF)
        .map_err(|e| anyhow::anyhow!("failed to encode private key as PEM: {e}"))?;
    let encoding_key = jsonwebtoken::EncodingKey::from_ec_pem(pem.as_bytes())?;

    // 7. Sign and return the JWT
    let token = jsonwebtoken::encode(&header, &claims, &encoding_key)?;
    Ok(token)
}

// ---------------------------------------------------------------------------
// 3. PDS OAuth Metadata Discovery
// ---------------------------------------------------------------------------

/// OAuth authorization server metadata from a PDS.
///
/// Fetched from `{pds_endpoint}/.well-known/oauth-authorization-server`.
/// See the AT Protocol OAuth specification for the full set of fields;
/// only the ones atrg needs are captured here.
#[derive(Debug, Clone, Deserialize)]
pub struct PdsOAuthMetadata {
    /// The issuer identifier for this authorization server.
    pub issuer: Option<String>,
    /// URL of the authorization endpoint.
    pub authorization_endpoint: String,
    /// URL of the token endpoint.
    pub token_endpoint: String,
    /// URL of the pushed authorization request endpoint (PAR).
    pub pushed_authorization_request_endpoint: Option<String>,
    /// DPoP signing algorithms the server supports.
    #[serde(default)]
    pub dpop_signing_alg_values_supported: Vec<String>,
    /// Scopes the server supports.
    #[serde(default)]
    pub scopes_supported: Vec<String>,
}

/// Discover the PDS's OAuth authorization server metadata.
///
/// Fetches `{pds_endpoint}/.well-known/oauth-authorization-server` and
/// parses the JSON response into [`PdsOAuthMetadata`].
pub async fn discover_pds_oauth_metadata(
    http: &reqwest::Client,
    pds_endpoint: &str,
) -> anyhow::Result<PdsOAuthMetadata> {
    let url = format!(
        "{}/.well-known/oauth-authorization-server",
        pds_endpoint.trim_end_matches('/')
    );

    tracing::debug!(url = %url, "discovering PDS OAuth metadata");

    let response = http.get(&url).send().await?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!(
            "PDS OAuth metadata discovery failed: status={}, url={}, body={}",
            status,
            url,
            body
        );
    }

    let metadata: PdsOAuthMetadata = response.json().await?;

    tracing::debug!(
        authorization_endpoint = %metadata.authorization_endpoint,
        token_endpoint = %metadata.token_endpoint,
        par_endpoint = ?metadata.pushed_authorization_request_endpoint,
        "discovered PDS OAuth metadata"
    );

    Ok(metadata)
}

// ---------------------------------------------------------------------------
// 4. Token Exchange
// ---------------------------------------------------------------------------

/// Response from the PDS token endpoint.
///
/// Contains the DPoP-bound access token and associated metadata.
#[derive(Debug, Clone, Deserialize)]
pub struct TokenResponse {
    /// The access token issued by the PDS.
    pub access_token: String,
    /// The refresh token (if issued).
    pub refresh_token: Option<String>,
    /// Token type (typically `"DPoP"`).
    pub token_type: String,
    /// Lifetime of the access token in seconds.
    pub expires_in: Option<u64>,
    /// The DID of the authenticated user.
    pub sub: String,
    /// The granted scope string.
    pub scope: Option<String>,
}

/// Exchange an authorization code for tokens at the PDS token endpoint.
///
/// Performs the OAuth 2.0 authorization code exchange with PKCE and DPoP.
/// If the server responds with a `DPoP-Nonce` header on the first attempt,
/// the request is retried once with the provided nonce (servers may require
/// a nonce on first try per RFC 9449 §8).
pub async fn exchange_code_for_tokens(
    http: &reqwest::Client,
    token_endpoint: &str,
    code: &str,
    code_verifier: &str,
    redirect_uri: &str,
    client_id: &str,
    dpop_private_key_jwk: &str,
) -> anyhow::Result<TokenResponse> {
    // 1. Create initial DPoP proof (no nonce, no access token)
    let dpop_proof = create_dpop_proof(dpop_private_key_jwk, "POST", token_endpoint, None, None)?;

    // 2. Build form body
    let form = [
        ("grant_type", "authorization_code"),
        ("code", code),
        ("code_verifier", code_verifier),
        ("redirect_uri", redirect_uri),
        ("client_id", client_id),
    ];

    // 3. Send POST request with DPoP header
    let response = http
        .post(token_endpoint)
        .header("DPoP", &dpop_proof)
        .form(&form)
        .send()
        .await?;

    let status = response.status();

    // 4. If 4xx with DPoP-Nonce header, retry once with the provided nonce
    if status.is_client_error() {
        let maybe_nonce = response
            .headers()
            .get("dpop-nonce")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let error_body = response.text().await.unwrap_or_default();

        if let Some(nonce_str) = maybe_nonce {
            tracing::debug!(
                nonce = %nonce_str,
                "received DPoP-Nonce, retrying token exchange"
            );

            let dpop_proof_with_nonce = create_dpop_proof(
                dpop_private_key_jwk,
                "POST",
                token_endpoint,
                Some(&nonce_str),
                None,
            )?;

            let retry_response = http
                .post(token_endpoint)
                .header("DPoP", &dpop_proof_with_nonce)
                .form(&form)
                .send()
                .await?;

            let retry_status = retry_response.status();
            if !retry_status.is_success() {
                let retry_body = retry_response.text().await.unwrap_or_default();
                anyhow::bail!(
                    "token exchange failed after DPoP-Nonce retry: status={}, body={}",
                    retry_status,
                    retry_body
                );
            }

            let token_response: TokenResponse = retry_response.json().await?;
            tracing::info!(sub = %token_response.sub, "token exchange successful (with nonce)");
            return Ok(token_response);
        }

        anyhow::bail!(
            "token exchange failed: status={}, body={}",
            status,
            error_body
        );
    }

    // 5. Parse successful response
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("token exchange failed: status={}, body={}", status, body);
    }

    let token_response: TokenResponse = response.json().await?;
    tracing::info!(sub = %token_response.sub, "token exchange successful");
    Ok(token_response)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Helpers ------------------------------------------------------------

    #[test]
    fn base64_url_encode_known_value() {
        // "hello" → base64url
        assert_eq!(base64_url_encode(b"hello"), "aGVsbG8");
    }

    #[test]
    fn base64_url_encode_str_known_value() {
        assert_eq!(base64_url_encode_str("hello"), "aGVsbG8");
    }

    #[test]
    fn generate_random_string_length() {
        // 16 random bytes → ceil(16 * 4/3) = 22 chars base64url
        let s = generate_random_string(16);
        assert_eq!(s.len(), 22);
    }

    #[test]
    fn generate_random_string_uniqueness() {
        let a = generate_random_string(16);
        let b = generate_random_string(16);
        assert_ne!(a, b);
    }

    // -- PKCE ---------------------------------------------------------------

    #[test]
    fn pkce_verifier_length() {
        let verifier = generate_code_verifier();
        // 32 random bytes → 43-char base64url (no padding)
        assert_eq!(verifier.len(), 43, "32 random bytes → 43-char base64url");
    }

    #[test]
    fn pkce_verifier_uniqueness() {
        let a = generate_code_verifier();
        let b = generate_code_verifier();
        assert_ne!(a, b);
    }

    #[test]
    fn pkce_verifier_url_safe_chars() {
        let verifier = generate_code_verifier();
        assert!(
            verifier
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "verifier must contain only URL-safe chars: {verifier}"
        );
    }

    #[test]
    fn pkce_code_challenge_rfc7636_test_vector() {
        // RFC 7636 Appendix B test vector
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let challenge = compute_code_challenge(verifier);
        assert_eq!(challenge, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn pkce_code_challenge_deterministic() {
        let verifier = generate_code_verifier();
        let c1 = compute_code_challenge(&verifier);
        let c2 = compute_code_challenge(&verifier);
        assert_eq!(c1, c2, "same verifier must produce the same challenge");
    }

    #[test]
    fn pkce_different_verifiers_different_challenges() {
        let v1 = generate_code_verifier();
        let v2 = generate_code_verifier();
        let c1 = compute_code_challenge(&v1);
        let c2 = compute_code_challenge(&v2);
        assert_ne!(c1, c2);
    }

    // -- DPoP keypair -------------------------------------------------------

    #[test]
    fn dpop_keypair_generation() {
        let kp = generate_dpop_keypair().expect("keypair generation should succeed");
        assert!(!kp.private_key_jwk.is_empty());

        // Public key should have standard EC JWK fields
        assert_eq!(kp.public_key_jwk["kty"], "EC");
        assert_eq!(kp.public_key_jwk["crv"], "P-256");
        assert!(kp.public_key_jwk["x"].is_string());
        assert!(kp.public_key_jwk["y"].is_string());
    }

    #[test]
    fn dpop_keypair_uniqueness() {
        let a = generate_dpop_keypair().unwrap();
        let b = generate_dpop_keypair().unwrap();
        assert_ne!(
            a.private_key_jwk, b.private_key_jwk,
            "two keypairs must differ"
        );
    }

    #[test]
    fn dpop_private_key_roundtrips() {
        let kp = generate_dpop_keypair().unwrap();
        // Should be able to reconstruct the secret key from the stored JWK
        let sk = p256::SecretKey::from_jwk_str(&kp.private_key_jwk);
        assert!(sk.is_ok(), "private key JWK should round-trip");
    }

    #[test]
    fn dpop_public_key_has_no_private_component() {
        let kp = generate_dpop_keypair().unwrap();
        // The public JWK must NOT contain the "d" (private key) field
        assert!(
            kp.public_key_jwk.get("d").is_none(),
            "public key JWK must not contain 'd' field"
        );
    }

    // -- DPoP proof ---------------------------------------------------------

    #[test]
    fn dpop_proof_is_valid_jwt_structure() {
        let kp = generate_dpop_keypair().unwrap();
        let proof = create_dpop_proof(
            &kp.private_key_jwk,
            "POST",
            "https://pds.example.com/oauth/token",
            None,
            None,
        )
        .expect("proof creation should succeed");

        // A JWT has three dot-separated segments
        let parts: Vec<&str> = proof.split('.').collect();
        assert_eq!(parts.len(), 3, "JWT must have 3 dot-separated parts");
        assert!(parts.iter().all(|p| !p.is_empty()), "no part may be empty");
    }

    #[test]
    fn dpop_proof_header_fields() {
        let kp = generate_dpop_keypair().unwrap();
        let proof = create_dpop_proof(
            &kp.private_key_jwk,
            "POST",
            "https://pds.example.com/oauth/token",
            None,
            None,
        )
        .unwrap();

        let parts: Vec<&str> = proof.split('.').collect();
        let header_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(parts[0])
            .expect("header should be valid base64url");
        let header: serde_json::Value =
            serde_json::from_slice(&header_bytes).expect("header should be valid JSON");

        assert_eq!(header["alg"], "ES256");
        assert_eq!(header["typ"], "dpop+jwt");
        assert!(header["jwk"].is_object(), "header must contain jwk");
        assert_eq!(header["jwk"]["kty"], "EC");
        assert_eq!(header["jwk"]["crv"], "P-256");
        assert!(header["jwk"]["x"].is_string());
        assert!(header["jwk"]["y"].is_string());
        // The header JWK must not contain the private key
        assert!(header["jwk"].get("d").is_none());
    }

    #[test]
    fn dpop_proof_claims_without_optional_fields() {
        let kp = generate_dpop_keypair().unwrap();
        let proof = create_dpop_proof(
            &kp.private_key_jwk,
            "POST",
            "https://pds.example.com/oauth/token",
            None,
            None,
        )
        .unwrap();

        let parts: Vec<&str> = proof.split('.').collect();
        let claims_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(parts[1])
            .expect("claims should be valid base64url");
        let claims: serde_json::Value =
            serde_json::from_slice(&claims_bytes).expect("claims should be valid JSON");

        assert_eq!(claims["htm"], "POST");
        assert_eq!(claims["htu"], "https://pds.example.com/oauth/token");
        assert!(claims["jti"].is_string());
        assert!(
            claims["jti"].as_str().unwrap().len() > 10,
            "jti should be a reasonably long random string"
        );
        assert!(claims["iat"].is_number());
        // Optional fields must be absent (not null) when not provided
        assert!(claims.get("nonce").is_none());
        assert!(claims.get("ath").is_none());
    }

    #[test]
    fn dpop_proof_with_nonce_and_ath() {
        let kp = generate_dpop_keypair().unwrap();
        let proof = create_dpop_proof(
            &kp.private_key_jwk,
            "GET",
            "https://pds.example.com/xrpc/com.atproto.repo.getRecord",
            Some("server-nonce-123"),
            Some("access_token_value"),
        )
        .expect("proof with nonce+ath should succeed");

        let parts: Vec<&str> = proof.split('.').collect();
        let claims_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(parts[1])
            .unwrap();
        let claims: serde_json::Value = serde_json::from_slice(&claims_bytes).unwrap();

        assert_eq!(claims["htm"], "GET");
        assert_eq!(
            claims["htu"],
            "https://pds.example.com/xrpc/com.atproto.repo.getRecord"
        );
        assert_eq!(claims["nonce"], "server-nonce-123");
        assert!(
            claims["ath"].is_string(),
            "ath claim must be present when access_token is provided"
        );

        // Verify ath = base64url(sha256(access_token))
        let expected_ath = {
            let hash = sha2::Sha256::digest(b"access_token_value");
            base64_url_encode(&hash)
        };
        assert_eq!(claims["ath"], expected_ath);
    }

    #[test]
    fn dpop_proof_jti_is_unique_across_proofs() {
        let kp = generate_dpop_keypair().unwrap();
        let proof1 = create_dpop_proof(
            &kp.private_key_jwk,
            "POST",
            "https://example.com/token",
            None,
            None,
        )
        .unwrap();
        let proof2 = create_dpop_proof(
            &kp.private_key_jwk,
            "POST",
            "https://example.com/token",
            None,
            None,
        )
        .unwrap();

        let extract_jti = |proof: &str| -> String {
            let parts: Vec<&str> = proof.split('.').collect();
            let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(parts[1])
                .unwrap();
            let claims: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            claims["jti"].as_str().unwrap().to_string()
        };

        assert_ne!(
            extract_jti(&proof1),
            extract_jti(&proof2),
            "each proof must have a unique jti"
        );
    }

    // -- PDS metadata deserialization ---------------------------------------

    #[test]
    fn pds_metadata_deserializes_full() {
        let json = r#"{
            "issuer": "https://bsky.social",
            "authorization_endpoint": "https://bsky.social/oauth/authorize",
            "token_endpoint": "https://bsky.social/oauth/token",
            "pushed_authorization_request_endpoint": "https://bsky.social/oauth/par",
            "dpop_signing_alg_values_supported": ["ES256"],
            "scopes_supported": ["atproto", "transition:generic"]
        }"#;

        let meta: PdsOAuthMetadata = serde_json::from_str(json).unwrap();
        assert_eq!(meta.issuer.as_deref(), Some("https://bsky.social"));
        assert_eq!(
            meta.authorization_endpoint,
            "https://bsky.social/oauth/authorize"
        );
        assert_eq!(meta.token_endpoint, "https://bsky.social/oauth/token");
        assert_eq!(
            meta.pushed_authorization_request_endpoint.as_deref(),
            Some("https://bsky.social/oauth/par")
        );
        assert_eq!(meta.dpop_signing_alg_values_supported, vec!["ES256"]);
        assert_eq!(meta.scopes_supported, vec!["atproto", "transition:generic"]);
    }

    #[test]
    fn pds_metadata_deserializes_minimal() {
        // Only the required fields
        let json = r#"{
            "authorization_endpoint": "https://pds.example.com/oauth/authorize",
            "token_endpoint": "https://pds.example.com/oauth/token"
        }"#;

        let meta: PdsOAuthMetadata = serde_json::from_str(json).unwrap();
        assert!(meta.issuer.is_none());
        assert_eq!(
            meta.authorization_endpoint,
            "https://pds.example.com/oauth/authorize"
        );
        assert_eq!(meta.token_endpoint, "https://pds.example.com/oauth/token");
        assert!(meta.pushed_authorization_request_endpoint.is_none());
        assert!(meta.dpop_signing_alg_values_supported.is_empty());
        assert!(meta.scopes_supported.is_empty());
    }

    #[test]
    fn pds_metadata_ignores_unknown_fields() {
        let json = r#"{
            "issuer": "https://example.com",
            "authorization_endpoint": "https://example.com/auth",
            "token_endpoint": "https://example.com/token",
            "some_future_field": "should be ignored",
            "registration_endpoint": "https://example.com/register"
        }"#;

        let meta: PdsOAuthMetadata = serde_json::from_str(json).unwrap();
        assert_eq!(meta.issuer.as_deref(), Some("https://example.com"));
    }

    // -- Token response deserialization -------------------------------------

    #[test]
    fn token_response_deserializes_full() {
        let json = r#"{
            "access_token": "eyJ0eXAiOiJhdCtqd3QiLCJhbGciOiJFUzI1NiJ9.test.sig",
            "refresh_token": "ref_abc123",
            "token_type": "DPoP",
            "expires_in": 3600,
            "sub": "did:plc:abc123",
            "scope": "atproto transition:generic"
        }"#;

        let resp: TokenResponse = serde_json::from_str(json).unwrap();
        assert!(resp.access_token.starts_with("eyJ"));
        assert_eq!(resp.refresh_token.as_deref(), Some("ref_abc123"));
        assert_eq!(resp.token_type, "DPoP");
        assert_eq!(resp.expires_in, Some(3600));
        assert_eq!(resp.sub, "did:plc:abc123");
        assert_eq!(resp.scope.as_deref(), Some("atproto transition:generic"));
    }

    #[test]
    fn token_response_deserializes_minimal() {
        let json = r#"{
            "access_token": "tok",
            "token_type": "DPoP",
            "sub": "did:plc:test"
        }"#;

        let resp: TokenResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.access_token, "tok");
        assert_eq!(resp.token_type, "DPoP");
        assert_eq!(resp.sub, "did:plc:test");
        assert!(resp.refresh_token.is_none());
        assert!(resp.expires_in.is_none());
        assert!(resp.scope.is_none());
    }
}
