//! Firebase ID token verification (specification §5.2).
//!
//! Firebase authenticates *anyone* with a Google account. The UID allow-list is
//! the only thing restricting access to the library (§5.3), so it is checked on
//! every request and is not optional.

use axum::extract::{FromRef, FromRequestParts};
use axum::http::{request::Parts, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{async_trait, Json};
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Google's JWK endpoint for Firebase ID tokens.
///
/// Deliberately the JWK endpoint rather than the x509 one: that endpoint serves
/// PEM **certificates**, and `DecodingKey::from_rsa_pem` expects a public key,
/// so feeding it certificates fails at runtime. JWK gives the modulus and
/// exponent directly.
/// Clock-skew tolerance when checking `exp`.
pub const TOKEN_LEEWAY_SECONDS: u64 = 10;

// Leeway is security-relevant: it is the window in which an expired token is
// still honoured. Pinned at compile time so a later "just bump it" cannot pass
// review unnoticed.
const _: () = assert!(TOKEN_LEEWAY_SECONDS <= 30);

pub const GOOGLE_JWK_URL: &str =
    "https://www.googleapis.com/service_accounts/v1/jwk/securetoken@system.gserviceaccount.com";

#[derive(Debug, Deserialize)]
struct JwkSet {
    keys: Vec<Jwk>,
}

#[derive(Debug, Deserialize)]
struct Jwk {
    kid: String,
    n: String,
    e: String,
}

/// Cached signing keys.
///
/// Once populated, verification works offline — §5.3 notes that sign-in needs
/// the internet but verification does not.
pub struct KeyStore {
    keys: RwLock<HashMap<String, Arc<DecodingKey>>>,
    jwk_url: Option<String>,
}

impl KeyStore {
    /// A store that refreshes from Google.
    pub fn google() -> Self {
        Self {
            keys: RwLock::new(HashMap::new()),
            jwk_url: Some(GOOGLE_JWK_URL.to_string()),
        }
    }

    /// A store that never reaches the network. Tests inject keys directly.
    pub fn offline() -> Self {
        Self {
            keys: RwLock::new(HashMap::new()),
            jwk_url: None,
        }
    }

    pub async fn insert(&self, kid: &str, key: DecodingKey) {
        self.keys
            .write()
            .await
            .insert(kid.to_string(), Arc::new(key));
    }

    /// The key for `kid`, refreshing once from the source if it is unknown.
    pub async fn get(&self, kid: &str) -> Result<Arc<DecodingKey>, AuthError> {
        if let Some(key) = self.keys.read().await.get(kid) {
            return Ok(Arc::clone(key));
        }

        let Some(url) = &self.jwk_url else {
            return Err(AuthError::unknown_key(kid));
        };

        self.refresh(url).await?;

        self.keys
            .read()
            .await
            .get(kid)
            .map(Arc::clone)
            .ok_or_else(|| AuthError::unknown_key(kid))
    }

    async fn refresh(&self, url: &str) -> Result<(), AuthError> {
        let response = reqwest::get(url).await.map_err(|e| AuthError {
            code: "signing_keys_unavailable",
            message: format!("Could not fetch signing keys: {e}"),
        })?;

        let set: JwkSet = response.json().await.map_err(|e| AuthError {
            code: "signing_keys_malformed",
            message: format!("Signing key response could not be parsed: {e}"),
        })?;

        let mut cache = self.keys.write().await;
        for jwk in set.keys {
            match DecodingKey::from_rsa_components(&jwk.n, &jwk.e) {
                Ok(key) => {
                    cache.insert(jwk.kid, Arc::new(key));
                }
                Err(e) => {
                    tracing::warn!(kid = %jwk.kid, error = %e, "skipping unusable signing key");
                }
            }
        }
        Ok(())
    }
}

/// Everything the server needs to decide whether a request may proceed.
pub struct AuthConfig {
    pub project_id: String,
    pub allowed_uids: Vec<String>,
    /// Break-glass token for when Firebase is unreachable (§5.3).
    pub admin_token: Option<String>,
    pub keys: KeyStore,
}

impl AuthConfig {
    pub fn from_env() -> Self {
        let allowed_uids = std::env::var("ALLOWED_UIDS")
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        Self {
            project_id: std::env::var("FIREBASE_PROJECT_ID").unwrap_or_default(),
            allowed_uids,
            admin_token: std::env::var("ADMIN_TOKEN").ok().filter(|t| !t.is_empty()),
            keys: KeyStore::google(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct Claims {
    pub iss: String,
    pub aud: String,
    pub exp: usize,
    pub sub: String,
}

/// A rejected request, carrying a code a client can branch on.
///
/// §5.3: the client must be able to tell "token expired, refresh and retry"
/// from "not authorised" without being dropped to a login screen.
#[derive(Debug, Serialize)]
pub struct AuthError {
    pub code: &'static str,
    pub message: String,
}

impl AuthError {
    fn unknown_key(kid: &str) -> Self {
        Self {
            code: "unknown_signing_key",
            message: format!("No signing key matches kid {kid}"),
        }
    }

    /// True when the right client response is to refresh and retry.
    pub fn is_retryable(&self) -> bool {
        self.code == "token_expired"
    }
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        // Always 401. Build plan Phase 5 task 3 asks for the reason code to
        // carry the distinction, not the status, so a client can refresh on
        // `token_expired` and stop on anything else.
        (StatusCode::UNAUTHORIZED, Json(self)).into_response()
    }
}

/// Verify a bearer token and confirm its subject may use the system.
pub async fn verify_token(config: &AuthConfig, token: &str) -> Result<Claims, AuthError> {
    // Break-glass path, for when Firebase cannot be reached at all (§5.3).
    if let Some(expected) = &config.admin_token {
        if constant_time_eq(token.as_bytes(), expected.as_bytes()) {
            return Ok(Claims {
                iss: "local-admin".into(),
                aud: config.project_id.clone(),
                exp: usize::MAX,
                sub: "admin".into(),
            });
        }
    }

    let header = decode_header(token).map_err(|e| AuthError {
        code: "malformed_token",
        message: e.to_string(),
    })?;

    let kid = header.kid.ok_or(AuthError {
        code: "malformed_token",
        message: "Token header carries no key id".into(),
    })?;

    let key = config.keys.get(&kid).await?;

    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_audience(&[&config.project_id]);
    validation.set_issuer(&[format!(
        "https://securetoken.google.com/{}",
        config.project_id
    )]);
    validation.set_required_spec_claims(&["exp", "aud", "iss", "sub"]);
    // Explicit rather than inherited: `jsonwebtoken` defaults to 60 seconds of
    // leeway, which is a security-relevant number that should be a decision.
    // Some tolerance is right — server and client clocks do drift — but a token
    // is not accepted a full minute past its expiry.
    validation.leeway = TOKEN_LEEWAY_SECONDS;

    let data = decode::<Claims>(token, &key, &validation).map_err(|e| {
        use jsonwebtoken::errors::ErrorKind;
        let code = match e.kind() {
            ErrorKind::ExpiredSignature => "token_expired",
            ErrorKind::InvalidAudience => "wrong_audience",
            ErrorKind::InvalidIssuer => "wrong_issuer",
            ErrorKind::InvalidSignature => "invalid_signature",
            _ => "invalid_token",
        };
        AuthError {
            code,
            message: e.to_string(),
        }
    })?;

    if data.claims.sub.is_empty() {
        return Err(AuthError {
            code: "invalid_token",
            message: "Token carries no subject".into(),
        });
    }

    // The allow-list is the only thing restricting access to the library.
    if !config
        .allowed_uids
        .iter()
        .any(|uid| uid == &data.claims.sub)
    {
        // The uid is logged because this is the first thing anybody hits when
        // setting the server up: the account is real, the token is valid, and
        // the only missing step is putting the uid in ALLOWED_UIDS. Without
        // this, finding it means hunting through the Firebase console for a
        // column that is usually off-screen.
        //
        // A uid is an identifier, not a credential — it grants nothing on its
        // own, and the token that carried it is not logged.
        tracing::warn!(
            uid = %data.claims.sub,
            "refused: this uid is not in ALLOWED_UIDS. Add it to admit this account."
        );
        return Err(AuthError {
            code: "not_authorized",
            message: "This account is not on the allow-list for this library".into(),
        });
    }

    Ok(data.claims)
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Extractor proving a request carried a valid, permitted token.
pub struct Authenticated(pub Claims);

impl Authenticated {
    /// The Firebase UID this request is acting as.
    pub fn uid(&self) -> &str {
        &self.0.sub
    }
}

#[async_trait]
impl<S> FromRequestParts<S> for Authenticated
where
    S: Send + Sync,
    Arc<AuthConfig>: FromRef<S>,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let config = <Arc<AuthConfig> as FromRef<S>>::from_ref(state);

        let header = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .ok_or(AuthError {
                code: "missing_token",
                message: "Authorization header required".into(),
            })?;

        let value = header.to_str().map_err(|_| AuthError {
            code: "malformed_token",
            message: "Authorization header is not valid text".into(),
        })?;

        let token = value.strip_prefix("Bearer ").ok_or(AuthError {
            code: "malformed_token",
            message: "Authorization header must be a Bearer token".into(),
        })?;

        Ok(Authenticated(verify_token(&config, token.trim()).await?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{encode, EncodingKey, Header};
    use std::time::{SystemTime, UNIX_EPOCH};

    const TEST_PRIVATE_KEY: &[u8] = b"-----BEGIN PRIVATE KEY-----\nMIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQDDgTuaUUsi1A/7\ntJHnfp9wBMSVaGpMgGXS11jXPwQaqSPJ+7DYb73Lf6XK7a2PkNtmQOk8vJpp99dZ\nnrmamdEJmS/U/rfFJRjMIFXSOx9pIwbteL+3TPwt48kmKSO/TVdGa+JXZT+utZMQ\natbi7Ta3cVBuy7iRRPqav/xD8gbubCARCxtjymVoUTNTkyYEpYOjMLniX3AQuejC\n1x6e/qCUVVWfE+/CUS2vehYTPtsenQ8XOmbXq0CfURuIIapqGJwjXYV67dWuY1jK\nfYd2Z6s3ZoTAu8EzBP9zflK+vAB3ZyLsg7gthBtdhrmGIH6YqmuiYERjA5SlXZ1J\nzoeWoZnFAgMBAAECggEACBpuEaO6CkD4n+VxL3IQ2bGTFFWHmDQl1bxy51BNVie8\njXe9iRgeY5MTO2PReLWDP5Sm/uhg3hOJ5dxQhRcw1/RGkitLIqdGPx49zXsYxGCi\n7IHuMFQ7c/QzlFT462zyrXlG5jQSrAMh6PinlrvrYh8WxxggXY3JRsgEJ6Ep7L8g\nWrHNTUxJab1UR2T9sld2joFvjuJ31qE9ohzCMflA7VLEI26Ki68guvsGGY1kc5WE\nm46JBQlTwo+CutczZGoCk+hBiNMaMjDyQ66KHZtAfhVGZKJ0O3WbDNFFr5XZ8XHt\nI5xFJRP8KYYaejYW8Y0dEkLidWUfI8AfXCIbwVuT4QKBgQDoCfCPlLOMFeRWGjVb\n9qRt2NaUs9HWmi24vT+Y6jfyUjqdtdaJeSo5b+OM8s96ruM/pTcLIPdoUcRTsPee\n9TAe/T+uZ/Hf6yooka0VzqnA3MT+N+tmIArpRIPUJkOGUgFSSbf4FVYJg0sF1vs4\n6o/ucIDeRaECD7pMwk5fikTrqwKBgQDXsX+k6UJbUwYItq0o101a3YtVnCohRqxG\nL+4pxnownrKarUnKWeyNca4mIjsQA7m5Rh/8/xDqt3rfZw4cC6sr2aHRE+sFGL2I\nNBPj3WIc4T/7N3HTJhBMGjbqzyzwK9GOueX/tdq+iTXF4ui12MPHEnwvzdlE6gNA\ntN+TZjagTwKBgBja17XJi+H5hlfivsx3Au3xSCrtiBCguz0KqIFMtWlzfWvfSne3\nTtqQLaOvbqIJkbYDkH3UriuydoEwd5XDVcA8CFI6OCJwIjfuQsgPNwe9nixM+R4b\nWI/cEvLqllkQ96tE0jv0rR6fva2GdaqHFZvI2UT12GVMIfyO465ANVm5AoGAc6N2\nC7QDH3MjiQhnTb4getbMHNncvHpnYjnQNhVy7R4oI0VEingrmqmX9Fnl0HAu4mX2\nQG1/ZFd6SMu3hNG8s4W6e51yIwlgk+VXxJKsR098PfM70zhVBHgJeVoZfaoAb8S6\nyp106TIm4jEFEnlkfRYr/nUeRxQvKkHOm/fw0YECgYEAiYsibzGcZxVzVOwpG7P5\neBNEEDtnUfydzOfyWDh8F2eCpmZCCgw2C+SOK00rvb9Z74G98I8U4lgF9oSvNljc\nBKpLBdGmIi3Co7F4eoTjEHw6Cvhjy4MHFTRlzMcGwCdhk8buODCoQR2P5lJqF6rk\n3KYEVrmjZQOnYuH0vGQVYtk=\n-----END PRIVATE KEY-----";
    const TEST_PUBLIC_KEY: &[u8] = b"-----BEGIN PUBLIC KEY-----\nMIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAw4E7mlFLItQP+7SR536f\ncATElWhqTIBl0tdY1z8EGqkjyfuw2G+9y3+lyu2tj5DbZkDpPLyaaffXWZ65mpnR\nCZkv1P63xSUYzCBV0jsfaSMG7Xi/t0z8LePJJikjv01XRmviV2U/rrWTEGrW4u02\nt3FQbsu4kUT6mr/8Q/IG7mwgEQsbY8plaFEzU5MmBKWDozC54l9wELnowtcenv6g\nlFVVnxPvwlEtr3oWEz7bHp0PFzpm16tAn1EbiCGqahicI12Feu3VrmNYyn2Hdmer\nN2aEwLvBMwT/c35SvrwAd2ci7IO4LYQbXYa5hiB+mKpromBEYwOUpV2dSc6HlqGZ\nxQIDAQAB\n-----END PUBLIC KEY-----";

    const TEST_KID: &str = "test-kid";
    const PROJECT: &str = "phototools-test";

    /// An offline config with the test key injected — no network at any point.
    async fn config_with_allowed(uids: &[&str]) -> AuthConfig {
        let config = AuthConfig {
            project_id: PROJECT.into(),
            allowed_uids: uids.iter().map(|s| s.to_string()).collect(),
            admin_token: None,
            keys: KeyStore::offline(),
        };
        config
            .keys
            .insert(
                TEST_KID,
                DecodingKey::from_rsa_pem(TEST_PUBLIC_KEY).unwrap(),
            )
            .await;
        config
    }

    fn now() -> usize {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as usize
    }

    fn token(iss: &str, aud: &str, sub: &str, exp: usize) -> String {
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some(TEST_KID.into());
        encode(
            &header,
            &Claims {
                iss: iss.into(),
                aud: aud.into(),
                exp,
                sub: sub.into(),
            },
            &EncodingKey::from_rsa_pem(TEST_PRIVATE_KEY).unwrap(),
        )
        .unwrap()
    }

    fn valid_token(sub: &str) -> String {
        token(
            &format!("https://securetoken.google.com/{PROJECT}"),
            PROJECT,
            sub,
            now() + 3600,
        )
    }

    #[tokio::test]
    async fn a_valid_token_from_a_permitted_uid_is_accepted() {
        let config = config_with_allowed(&["uid-1"]).await;
        let claims = verify_token(&config, &valid_token("uid-1")).await.unwrap();
        assert_eq!(claims.sub, "uid-1");
    }

    #[tokio::test]
    async fn an_expired_token_is_rejected_as_retryable() {
        let config = config_with_allowed(&["uid-1"]).await;
        let expired = token(
            &format!("https://securetoken.google.com/{PROJECT}"),
            PROJECT,
            "uid-1",
            now() - 3600,
        );

        let err = verify_token(&config, &expired).await.unwrap_err();
        assert_eq!(err.code, "token_expired");
        assert!(
            err.is_retryable(),
            "a client should refresh and retry, not drop to a login screen"
        );
    }

    /// A token just past expiry is still refused: the leeway is for clock skew,
    /// not for a grace period.
    #[tokio::test]
    async fn expiry_leeway_is_small_and_deliberate() {
        let config = config_with_allowed(&["uid-1"]).await;
        let just_expired = token(
            &format!("https://securetoken.google.com/{PROJECT}"),
            PROJECT,
            "uid-1",
            now() - (TOKEN_LEEWAY_SECONDS as usize + 5),
        );

        let err = verify_token(&config, &just_expired).await.unwrap_err();
        assert_eq!(err.code, "token_expired");
    }

    #[tokio::test]
    async fn a_token_for_another_project_is_rejected() {
        let config = config_with_allowed(&["uid-1"]).await;
        let wrong_aud = token(
            &format!("https://securetoken.google.com/{PROJECT}"),
            "some-other-project",
            "uid-1",
            now() + 3600,
        );

        let err = verify_token(&config, &wrong_aud).await.unwrap_err();
        assert_eq!(err.code, "wrong_audience");
        assert!(!err.is_retryable());
    }

    #[tokio::test]
    async fn a_token_from_another_issuer_is_rejected() {
        let config = config_with_allowed(&["uid-1"]).await;
        let wrong_iss = token(
            "https://securetoken.google.com/somebody-elses-project",
            PROJECT,
            "uid-1",
            now() + 3600,
        );

        let err = verify_token(&config, &wrong_iss).await.unwrap_err();
        assert_eq!(err.code, "wrong_issuer");
    }

    /// The case that actually protects the library: Firebase will happily
    /// authenticate any Google account in existence.
    #[tokio::test]
    async fn a_perfectly_valid_token_from_an_uninvited_account_is_rejected() {
        let config = config_with_allowed(&["uid-1"]).await;

        let err = verify_token(&config, &valid_token("some-stranger"))
            .await
            .unwrap_err();

        assert_eq!(err.code, "not_authorized");
        assert!(!err.is_retryable(), "refreshing will never help here");
    }

    #[tokio::test]
    async fn an_empty_allow_list_admits_nobody() {
        let config = config_with_allowed(&[]).await;
        let err = verify_token(&config, &valid_token("uid-1"))
            .await
            .unwrap_err();
        assert_eq!(err.code, "not_authorized");
    }

    #[tokio::test]
    async fn a_token_signed_by_an_unknown_key_is_rejected_without_network() {
        let config = config_with_allowed(&["uid-1"]).await;

        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some("a-kid-we-have-never-seen".into());
        let forged = encode(
            &header,
            &Claims {
                iss: format!("https://securetoken.google.com/{PROJECT}"),
                aud: PROJECT.into(),
                exp: now() + 3600,
                sub: "uid-1".into(),
            },
            &EncodingKey::from_rsa_pem(TEST_PRIVATE_KEY).unwrap(),
        )
        .unwrap();

        let err = verify_token(&config, &forged).await.unwrap_err();
        assert_eq!(err.code, "unknown_signing_key");
    }

    #[tokio::test]
    async fn garbage_is_rejected_as_malformed() {
        let config = config_with_allowed(&["uid-1"]).await;
        let err = verify_token(&config, "not-a-jwt").await.unwrap_err();
        assert_eq!(err.code, "malformed_token");
    }

    #[tokio::test]
    async fn the_break_glass_token_works_when_firebase_cannot_be_reached() {
        let mut config = config_with_allowed(&[]).await;
        config.admin_token = Some("break-glass-secret".into());

        let claims = verify_token(&config, "break-glass-secret").await.unwrap();
        assert_eq!(claims.sub, "admin");

        // And a near-miss is still rejected.
        assert!(verify_token(&config, "break-glass-secre").await.is_err());
    }

    #[tokio::test]
    async fn the_break_glass_path_is_off_unless_configured() {
        let config = config_with_allowed(&[]).await;
        assert!(config.admin_token.is_none());
        assert!(verify_token(&config, "").await.is_err());
    }

    #[test]
    fn constant_time_comparison_is_correct() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
        assert!(constant_time_eq(b"", b""));
    }
}
