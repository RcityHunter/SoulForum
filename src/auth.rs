use std::{env, sync::OnceLock};

use axum::{
    async_trait,
    extract::FromRequestParts,
    http::request::Parts,
    response::{IntoResponse, Response},
    RequestPartsExt,
};
use axum_extra::headers::{authorization::Bearer, Authorization};
use axum_extra::TypedHeader;
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Deserializer, Serialize};

fn deserialize_scope<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum ScopeValue {
        String(String),
        Array(Vec<String>),
    }

    let value = Option::<ScopeValue>::deserialize(deserializer)?;
    Ok(match value {
        Some(ScopeValue::String(scopes)) => {
            let scopes = scopes
                .split_whitespace()
                .map(str::trim)
                .filter(|scope| !scope.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>();
            if scopes.is_empty() {
                None
            } else {
                Some(scopes)
            }
        }
        Some(ScopeValue::Array(scopes)) => {
            let scopes = scopes
                .into_iter()
                .map(|scope| scope.trim().to_string())
                .filter(|scope| !scope.is_empty())
                .collect::<Vec<_>>();
            if scopes.is_empty() {
                None
            } else {
                Some(scopes)
            }
        }
        None => None,
    })
}

/// JWT Claims expected from Rainbow-Auth tokens.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AuthClaims {
    pub sub: String,
    pub exp: i64,
    pub iat: i64,
    pub role: Option<String>,
    pub permissions: Option<Vec<String>>,
    #[serde(default, deserialize_with = "deserialize_scope")]
    pub scope: Option<Vec<String>>,
    pub session_id: Option<String>,
    pub subject_type: Option<String>,
    pub client_id: Option<String>,
    #[serde(default, skip)]
    pub token: Option<String>,
}

impl AuthClaims {
    pub fn is_agent(&self) -> bool {
        matches!(self.subject_type.as_deref(), Some("agent"))
    }

    pub fn effective_permissions(&self) -> Vec<&str> {
        let mut values = Vec::new();
        if let Some(permissions) = self.permissions.as_ref() {
            values.extend(permissions.iter().map(String::as_str));
        }
        if let Some(scopes) = self.scope.as_ref() {
            values.extend(scopes.iter().map(String::as_str));
        }
        values
    }
}

#[cfg(test)]
mod tests {
    use super::AuthClaims;

    #[test]
    fn scope_string_deserializes_into_list() {
        let claims: AuthClaims = serde_json::from_value(serde_json::json!({
            "sub": "subject:123",
            "exp": 1,
            "iat": 1,
            "scope": "forum:topic:read forum:reply:write"
        }))
        .unwrap();

        assert_eq!(
            claims.scope,
            Some(vec!["forum:topic:read".into(), "forum:reply:write".into()])
        );
    }

    #[test]
    fn effective_permissions_merges_permissions_and_scope() {
        let claims: AuthClaims = serde_json::from_value(serde_json::json!({
            "sub": "subject:123",
            "exp": 1,
            "iat": 1,
            "permissions": ["manage_boards"],
            "scope": "forum:topic:read"
        }))
        .unwrap();

        let effective = claims.effective_permissions();
        assert!(effective.contains(&"manage_boards"));
        assert!(effective.contains(&"forum:topic:read"));
    }
}

/// Rejection type returned when auth fails.
#[derive(Debug)]
pub enum AuthError {
    MissingToken,
    InvalidToken,
    MissingKey,
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        use axum::http::StatusCode;
        let status = match self {
            AuthError::MissingToken => StatusCode::UNAUTHORIZED,
            AuthError::InvalidToken => StatusCode::UNAUTHORIZED,
            AuthError::MissingKey => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let msg = match self {
            AuthError::MissingToken => "missing bearer token",
            AuthError::InvalidToken => "invalid token",
            AuthError::MissingKey => "jwt key not configured",
        };
        (status, msg).into_response()
    }
}

fn decoding_config() -> Result<&'static (DecodingKey, Validation), AuthError> {
    static DECODING: OnceLock<(DecodingKey, Validation)> = OnceLock::new();

    if let Some(cfg) = DECODING.get() {
        return Ok(cfg);
    }

    let computed = if let Ok(pem) = env::var("JWT_PUBLIC_KEY_PEM") {
        let key = DecodingKey::from_rsa_pem(pem.as_bytes()).map_err(|_| AuthError::InvalidToken)?;
        let mut validation = Validation::new(Algorithm::RS256);
        validation.validate_exp = true;
        (key, validation)
    } else {
        let secret = env::var("JWT_SECRET").map_err(|_| AuthError::MissingKey)?;
        let mut validation = Validation::new(Algorithm::HS256);
        validation.validate_exp = true;
        (DecodingKey::from_secret(secret.as_bytes()), validation)
    };

    Ok(DECODING.get_or_init(|| computed))
}

#[async_trait]
impl<S> FromRequestParts<S> for AuthClaims
where
    S: Send + Sync,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let TypedHeader(Authorization(bearer)) = parts
            .extract::<TypedHeader<Authorization<Bearer>>>()
            .await
            .map_err(|_| AuthError::MissingToken)?;

        let (decoding_key, validation) = decoding_config()?;

        let mut token_data = decode::<AuthClaims>(bearer.token(), decoding_key, validation)
            .map_err(|_| AuthError::InvalidToken)?;
        token_data.claims.token = Some(bearer.token().to_string());

        Ok(token_data.claims)
    }
}
