use axum::{http::StatusCode, Json};

use btc_forum_rust::{
    auth::AuthClaims, rainbow_auth::RainbowUser, services::ForumContext, surreal::SurrealUser,
};

use super::{error::api_error_from_status, state::AppState};

pub(crate) fn require_auth(
    claims: &Option<AuthClaims>,
) -> Result<&AuthClaims, (StatusCode, Json<btc_forum_shared::ApiError>)> {
    if let Some(claims) = claims {
        Ok(claims)
    } else {
        Err(api_error_from_status(
            StatusCode::UNAUTHORIZED,
            "authorization required",
        ))
    }
}

pub(crate) fn bearer_from_headers(headers: &axum::http::HeaderMap) -> Option<String> {
    let header = headers.get(axum::http::header::AUTHORIZATION)?;
    let value = header.to_str().ok()?.trim();
    let token = value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))?;
    let token = token.trim();
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

pub(crate) async fn resolve_rainbow_user(
    state: &AppState,
    claims: &AuthClaims,
) -> Option<RainbowUser> {
    let token = claims.token.as_deref()?;
    match state.rainbow_auth.me(token).await {
        Ok(user) => Some(user),
        Err(err) => {
            tracing::error!(error = %err, "rainbow-auth user lookup failed");
            None
        }
    }
}

pub(crate) async fn ensure_user_ctx(
    state: &AppState,
    claims: &AuthClaims,
) -> Result<(SurrealUser, ForumContext), (StatusCode, Json<btc_forum_shared::ApiError>)> {
    let resolved_user = resolve_rainbow_user(state, claims).await;
    let name = resolved_user
        .as_ref()
        .map(|user| user.email.as_str())
        .unwrap_or(&claims.sub);
    match state
        .surreal
        .ensure_user(name, claims.role.as_deref(), claims.permissions.as_deref())
        .await
    {
        Ok(user) => {
            if let Some(user_info) = resolved_user {
                let mut user = user;
                user.name = user_info.email;
                let ctx = build_ctx_from_user(&user, claims);
                return Ok((user, ctx));
            }
            let ctx = build_ctx_from_user(&user, claims);
            Ok((user, ctx))
        }
        Err(err) => {
            tracing::error!(error = %err, "failed to ensure user");
            Err(api_error_from_status(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to ensure user",
            ))
        }
    }
}

pub(crate) fn build_ctx_from_user(user: &SurrealUser, claims: &AuthClaims) -> ForumContext {
    let mut ctx = ForumContext::default();
    ctx.user_info.is_guest = false;
    ctx.user_info.name = user.name.clone();
    ctx.user_info.id = user.legacy_id();

    if let Some(role) = user.role.as_deref().or_else(|| claims.role.as_deref()) {
        match role {
            "admin" => ctx.user_info.is_admin = true,
            "mod" => ctx.user_info.is_mod = true,
            _ => {}
        }
    }

    if let Some(perms) = user
        .permissions
        .clone()
        .or_else(|| claims.permissions.clone())
    {
        ctx.user_info.permissions.extend(perms);
    }

    ctx.user_info.groups.clear();
    if ctx.user_info.is_admin {
        ctx.user_info.groups.push(0);
    } else if ctx.user_info.is_mod {
        ctx.user_info.groups.extend([2]);
    } else {
        ctx.user_info.groups.push(4);
    }

    ctx
}

pub(crate) fn user_groups(ctx: &ForumContext) -> Vec<i64> {
    if !ctx.user_info.groups.is_empty() {
        return ctx.user_info.groups.clone();
    }
    if ctx.user_info.is_admin {
        return vec![0];
    }
    if ctx.user_info.is_mod {
        return vec![2];
    }
    vec![4]
}
