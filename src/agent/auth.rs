use axum::{http::StatusCode, Json};

use btc_forum_rust::auth::AuthClaims;
use btc_forum_shared::{ApiError, ErrorCode};

pub type AgentApiError = (StatusCode, Json<ApiError>);

fn api_error(status: StatusCode, code: ErrorCode, message: impl Into<String>) -> AgentApiError {
    (
        status,
        Json(ApiError {
            code,
            message: message.into(),
            details: None,
        }),
    )
}

fn has_scope_or_permission(claims: &AuthClaims, scope: &str, legacy_permissions: &[&str]) -> bool {
    if claims.role.as_deref() == Some("admin") {
        return true;
    }

    let permissions = claims.effective_permissions();
    permissions.contains(&scope)
        || legacy_permissions
            .iter()
            .any(|legacy| permissions.contains(legacy))
}

pub fn require_scope<'a>(
    claims: &'a Option<AuthClaims>,
    scope: &str,
    legacy_permissions: &[&str],
) -> Result<&'a AuthClaims, AgentApiError> {
    let claims = claims.as_ref().ok_or_else(|| {
        api_error(
            StatusCode::UNAUTHORIZED,
            ErrorCode::Unauthorized,
            "authorization required",
        )
    })?;

    if has_scope_or_permission(claims, scope, legacy_permissions) {
        Ok(claims)
    } else {
        Err(api_error(
            StatusCode::FORBIDDEN,
            ErrorCode::Forbidden,
            format!("missing scope: {scope}"),
        ))
    }
}
