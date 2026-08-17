use axum::{
    async_trait,
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

const GOOGLE_CERTS_URL: &str =
    "https://www.googleapis.com/robot/v1/metadata/x509/securetoken@system.gserviceaccount.com";

lazy_static::lazy_static! {
    static ref KEY_CACHE: Arc<RwLock<HashMap<String, String>>> = Arc::new(RwLock::new(HashMap::new()));
}

#[derive(Debug, Serialize)]
pub struct AuthError {
    pub code: &'static str,
    pub message: String,
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let status = match self.code {
            "token_expired" => StatusCode::UNAUTHORIZED,
            "not_authorized" => StatusCode::FORBIDDEN,
            _ => StatusCode::UNAUTHORIZED,
        };
        (status, Json(self)).into_response()
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Claims {
    pub iss: String,
    pub aud: String,
    pub exp: usize,
    pub sub: String,
}

/// Verified Firebase claims, attached to a request by the extractor below.
/// Handlers do not read the claims yet; per-user behaviour arrives in Phase 5.
#[allow(dead_code)]
pub struct ClaimsExtracted(pub Claims);

async fn fetch_google_certs() -> Result<HashMap<String, String>, reqwest::Error> {
    let client = Client::new();
    let res = client.get(GOOGLE_CERTS_URL).send().await?;
    let certs: HashMap<String, String> = res.json().await?;
    Ok(certs)
}

pub async fn verify_token(
    token: &str,
    project_id: &str,
    allowed_uids: &[String],
) -> Result<Claims, AuthError> {
    // Break-glass admin token
    if let Ok(admin_token) = std::env::var("ADMIN_TOKEN") {
        if token == admin_token {
            return Ok(Claims {
                iss: "admin".into(),
                aud: "admin".into(),
                exp: usize::MAX,
                sub: "admin".into(),
            });
        }
    }

    let header = decode_header(token).map_err(|e| AuthError {
        code: "invalid_header",
        message: e.to_string(),
    })?;

    let kid = header.kid.ok_or_else(|| AuthError {
        code: "missing_kid",
        message: "No kid in header".into(),
    })?;

    let mut certs = KEY_CACHE.read().await.clone();
    if !certs.contains_key(&kid) {
        // Refresh cache
        match fetch_google_certs().await {
            Ok(new_certs) => {
                let mut write_cache = KEY_CACHE.write().await;
                *write_cache = new_certs.clone();
                certs = new_certs;
            }
            Err(e) => {
                return Err(AuthError {
                    code: "cert_fetch_failed",
                    message: e.to_string(),
                });
            }
        }
    }

    let cert = certs.get(&kid).ok_or_else(|| AuthError {
        code: "unknown_kid",
        message: "Unknown key ID".into(),
    })?;

    let decoding_key = DecodingKey::from_rsa_pem(cert.as_bytes()).map_err(|e| AuthError {
        code: "invalid_cert",
        message: e.to_string(),
    })?;

    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_audience(&[project_id]);
    let iss = format!("https://securetoken.google.com/{}", project_id);
    validation.set_issuer(&[iss]);

    let token_data = decode::<Claims>(token, &decoding_key, &validation).map_err(|e| {
        let code = if e.kind() == &jsonwebtoken::errors::ErrorKind::ExpiredSignature {
            "token_expired"
        } else {
            "invalid_signature"
        };
        AuthError {
            code,
            message: e.to_string(),
        }
    })?;

    // UID allow-list check
    if !allowed_uids.contains(&token_data.claims.sub) && token_data.claims.sub != "admin" {
        return Err(AuthError {
            code: "not_authorized",
            message: "User not on allow-list".into(),
        });
    }

    Ok(token_data.claims)
}

#[async_trait]
impl<S> FromRequestParts<S> for ClaimsExtracted
where
    S: Send + Sync,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let auth_header = parts
            .headers
            .get("Authorization")
            .ok_or_else(|| AuthError {
                code: "missing_auth_header",
                message: "Missing Authorization header".into(),
            })?;

        let auth_str = auth_header.to_str().map_err(|_| AuthError {
            code: "invalid_auth_header",
            message: "Invalid Authorization header".into(),
        })?;

        if !auth_str.starts_with("Bearer ") {
            return Err(AuthError {
                code: "invalid_bearer",
                message: "Authorization header must start with Bearer".into(),
            });
        }

        let token = &auth_str[7..];

        let project_id =
            std::env::var("FIREBASE_PROJECT_ID").unwrap_or_else(|_| "masterphototools".into());
        // For testing, let's just parse an env var
        let allowed_uids_str = std::env::var("ALLOWED_UIDS").unwrap_or_default();
        let allowed_uids: Vec<String> = allowed_uids_str
            .split(',')
            .map(|s| s.trim().to_string())
            .collect();

        let claims = verify_token(token, &project_id, &allowed_uids).await?;
        Ok(ClaimsExtracted(claims))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{encode, EncodingKey, Header};
    use std::time::{SystemTime, UNIX_EPOCH};

    const TEST_PRIVATE_KEY: &[u8] = b"-----BEGIN PRIVATE KEY-----\nMIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQDDgTuaUUsi1A/7\ntJHnfp9wBMSVaGpMgGXS11jXPwQaqSPJ+7DYb73Lf6XK7a2PkNtmQOk8vJpp99dZ\nnrmamdEJmS/U/rfFJRjMIFXSOx9pIwbteL+3TPwt48kmKSO/TVdGa+JXZT+utZMQ\natbi7Ta3cVBuy7iRRPqav/xD8gbubCARCxtjymVoUTNTkyYEpYOjMLniX3AQuejC\n1x6e/qCUVVWfE+/CUS2vehYTPtsenQ8XOmbXq0CfURuIIapqGJwjXYV67dWuY1jK\nfYd2Z6s3ZoTAu8EzBP9zflK+vAB3ZyLsg7gthBtdhrmGIH6YqmuiYERjA5SlXZ1J\nzoeWoZnFAgMBAAECggEACBpuEaO6CkD4n+VxL3IQ2bGTFFWHmDQl1bxy51BNVie8\njXe9iRgeY5MTO2PReLWDP5Sm/uhg3hOJ5dxQhRcw1/RGkitLIqdGPx49zXsYxGCi\n7IHuMFQ7c/QzlFT462zyrXlG5jQSrAMh6PinlrvrYh8WxxggXY3JRsgEJ6Ep7L8g\nWrHNTUxJab1UR2T9sld2joFvjuJ31qE9ohzCMflA7VLEI26Ki68guvsGGY1kc5WE\nm46JBQlTwo+CutczZGoCk+hBiNMaMjDyQ66KHZtAfhVGZKJ0O3WbDNFFr5XZ8XHt\nI5xFJRP8KYYaejYW8Y0dEkLidWUfI8AfXCIbwVuT4QKBgQDoCfCPlLOMFeRWGjVb\n9qRt2NaUs9HWmi24vT+Y6jfyUjqdtdaJeSo5b+OM8s96ruM/pTcLIPdoUcRTsPee\n9TAe/T+uZ/Hf6yooka0VzqnA3MT+N+tmIArpRIPUJkOGUgFSSbf4FVYJg0sF1vs4\n6o/ucIDeRaECD7pMwk5fikTrqwKBgQDXsX+k6UJbUwYItq0o101a3YtVnCohRqxG\nL+4pxnownrKarUnKWeyNca4mIjsQA7m5Rh/8/xDqt3rfZw4cC6sr2aHRE+sFGL2I\nNBPj3WIc4T/7N3HTJhBMGjbqzyzwK9GOueX/tdq+iTXF4ui12MPHEnwvzdlE6gNA\ntN+TZjagTwKBgBja17XJi+H5hlfivsx3Au3xSCrtiBCguz0KqIFMtWlzfWvfSne3\nTtqQLaOvbqIJkbYDkH3UriuydoEwd5XDVcA8CFI6OCJwIjfuQsgPNwe9nixM+R4b\nWI/cEvLqllkQ96tE0jv0rR6fva2GdaqHFZvI2UT12GVMIfyO465ANVm5AoGAc6N2\nC7QDH3MjiQhnTb4getbMHNncvHpnYjnQNhVy7R4oI0VEingrmqmX9Fnl0HAu4mX2\nQG1/ZFd6SMu3hNG8s4W6e51yIwlgk+VXxJKsR098PfM70zhVBHgJeVoZfaoAb8S6\nyp106TIm4jEFEnlkfRYr/nUeRxQvKkHOm/fw0YECgYEAiYsibzGcZxVzVOwpG7P5\neBNEEDtnUfydzOfyWDh8F2eCpmZCCgw2C+SOK00rvb9Z74G98I8U4lgF9oSvNljc\nBKpLBdGmIi3Co7F4eoTjEHw6Cvhjy4MHFTRlzMcGwCdhk8buODCoQR2P5lJqF6rk\n3KYEVrmjZQOnYuH0vGQVYtk=\n-----END PRIVATE KEY-----";
    const TEST_PUBLIC_KEY: &str = "-----BEGIN PUBLIC KEY-----\nMIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAw4E7mlFLItQP+7SR536f\ncATElWhqTIBl0tdY1z8EGqkjyfuw2G+9y3+lyu2tj5DbZkDpPLyaaffXWZ65mpnR\nCZkv1P63xSUYzCBV0jsfaSMG7Xi/t0z8LePJJikjv01XRmviV2U/rrWTEGrW4u02\nt3FQbsu4kUT6mr/8Q/IG7mwgEQsbY8plaFEzU5MmBKWDozC54l9wELnowtcenv6g\nlFVVnxPvwlEtr3oWEz7bHp0PFzpm16tAn1EbiCGqahicI12Feu3VrmNYyn2Hdmer\nN2aEwLvBMwT/c35SvrwAd2ci7IO4LYQbXYa5hiB+mKpromBEYwOUpV2dSc6HlqGZ\nxQIDAQAB\n-----END PUBLIC KEY-----";

    async fn setup_cache() {
        let mut cache = KEY_CACHE.write().await;
        cache.insert("test_kid".to_string(), TEST_PUBLIC_KEY.to_string());
    }

    fn make_token(iss: &str, aud: &str, sub: &str, exp: usize) -> String {
        let claims = Claims {
            iss: iss.into(),
            aud: aud.into(),
            exp,
            sub: sub.into(),
        };
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some("test_kid".into());
        encode(
            &header,
            &claims,
            &EncodingKey::from_rsa_pem(TEST_PRIVATE_KEY).unwrap(),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn test_valid_token_accepted() {
        setup_cache().await;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as usize;
        let token = make_token(
            "https://securetoken.google.com/test_proj",
            "test_proj",
            "user123",
            now + 3600,
        );
        let res = verify_token(&token, "test_proj", &["user123".into()]).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn test_expired_token_rejected() {
        setup_cache().await;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as usize;
        let token = make_token(
            "https://securetoken.google.com/test_proj",
            "test_proj",
            "user123",
            now - 3600,
        );
        let res = verify_token(&token, "test_proj", &["user123".into()]).await;
        assert_eq!(res.unwrap_err().code, "token_expired");
    }

    #[tokio::test]
    async fn test_wrong_aud_rejected() {
        setup_cache().await;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as usize;
        let token = make_token(
            "https://securetoken.google.com/test_proj",
            "wrong_proj",
            "user123",
            now + 3600,
        );
        let res = verify_token(&token, "test_proj", &["user123".into()]).await;
        assert_eq!(res.unwrap_err().code, "invalid_signature");
    }

    #[tokio::test]
    async fn test_wrong_iss_rejected() {
        setup_cache().await;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as usize;
        let token = make_token(
            "https://securetoken.google.com/wrong_proj",
            "test_proj",
            "user123",
            now + 3600,
        );
        let res = verify_token(&token, "test_proj", &["user123".into()]).await;
        assert_eq!(res.unwrap_err().code, "invalid_signature");
    }

    #[tokio::test]
    async fn test_valid_sig_but_uid_not_on_allowlist_rejected() {
        setup_cache().await;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as usize;
        let token = make_token(
            "https://securetoken.google.com/test_proj",
            "test_proj",
            "unknown_hacker",
            now + 3600,
        );
        let res = verify_token(&token, "test_proj", &["user123".into()]).await;
        assert_eq!(res.unwrap_err().code, "not_authorized");
    }

    #[tokio::test]
    async fn test_admin_break_glass() {
        std::env::set_var("ADMIN_TOKEN", "super_secret");
        let res = verify_token("super_secret", "test_proj", &[]).await;
        assert!(res.is_ok());
        assert_eq!(res.unwrap().sub, "admin");
    }
}
