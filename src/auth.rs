use std::{env, sync::OnceLock};

use axum::{
    RequestPartsExt, async_trait,
    extract::FromRequestParts,
    http::request::Parts,
    response::{IntoResponse, Response},
};
use axum_extra::TypedHeader;
use axum_extra::headers::{Authorization, authorization::Bearer};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
use serde::{Deserialize, Serialize};

/// JWT Claims expected from Rainbow-Auth tokens.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthClaims {
    pub sub: String,
    pub exp: i64,
    pub iat: i64,
    pub role: Option<String>,
    pub permissions: Option<Vec<String>>,
    pub session_id: Option<String>,
    #[serde(default, skip)]
    pub token: Option<String>,
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

        let mut token_data =
            decode::<AuthClaims>(bearer.token(), decoding_key, validation).map_err(|_| AuthError::InvalidToken)?;
        token_data.claims.token = Some(bearer.token().to_string());

        Ok(token_data.claims)
    }
}
