use axum::{http::StatusCode, Json};

use btc_forum_rust::{
    auth::AuthClaims, rainbow_auth::RainbowUser, security::is_not_banned, services::{ForumContext, ForumError}, surreal::SurrealUser,
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
            if err.is_retryable() {
                tokio::time::sleep(std::time::Duration::from_millis(120)).await;
                match state.rainbow_auth.me(token).await {
                    Ok(user) => return Some(user),
                    Err(retry_err) => {
                        tracing::error!(error = %retry_err, "rainbow-auth user lookup failed after retry");
                        return None;
                    }
                }
            }
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
                let ctx = match enrich_ctx_with_ban_state(state, build_ctx_from_user(&user, claims)).await {
                    Ok(ctx) => ctx,
                    Err(resp) => return Err(resp),
                };
                return Ok((user, ctx));
            }
            let ctx = match enrich_ctx_with_ban_state(state, build_ctx_from_user(&user, claims)).await {
                Ok(ctx) => ctx,
                Err(resp) => return Err(resp),
            };
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

async fn enrich_ctx_with_ban_state(
    state: &AppState,
    ctx: ForumContext,
) -> Result<ForumContext, (StatusCode, Json<btc_forum_shared::ApiError>)> {
    let forum_service = state.forum_service.clone();
    let fallback_ctx = ctx.clone();
    match tokio::task::spawn_blocking(move || {
        let mut ctx = ctx;
        is_not_banned(&forum_service, &mut ctx, false)?;
        Ok::<ForumContext, ForumError>(ctx)
    })
    .await
    {
        Ok(Ok(ctx)) => Ok(ctx),
        Ok(Err(ForumError::PermissionDenied(message))) => {
            Err(api_error_from_status(StatusCode::FORBIDDEN, message))
        }
        Ok(Err(other)) => {
            tracing::warn!(error = %other, "failed to evaluate ban state; continuing without ban enforcement");
            Ok(fallback_ctx.clone())
        }
        Err(err) => {
            tracing::warn!(error = %err, "failed to evaluate ban state task; continuing without ban enforcement");
            Ok(fallback_ctx)
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

    // Treat forum-level manage permission as admin-equivalent for admin APIs.
    if ctx.user_info.permissions.contains("manage_boards") {
        ctx.user_info.is_admin = true;
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
