use axum::{
    extract::{ConnectInfo, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::{net::SocketAddr, time::Duration};
use tracing::error;

use btc_forum_rust::{
    auth::AuthClaims,
    security::is_not_banned,
    services::{ForumContext, ForumError},
    surreal::reauth_from_env,
};
use btc_forum_shared::{
    AuthMeResponse, AuthResponse, AuthUser, ErrorCode, LoginRequest, RegisterRequest,
    RegisterResponse,
};

use super::{
    auth::bearer_from_headers,
    auth::build_ctx_from_user,
    error::{api_error, api_error_from_status, rainbow_auth_error_response},
    guards::enforce_rate,
    state::AppState,
};

fn ban_markers_from_ctx(ctx: &ForumContext) -> Vec<String> {
    let mut permissions = Vec::new();
    if ctx.session.bool("ban_cannot_post") {
        permissions.push("ban_cannot_post".to_string());
    }
    if ctx.session.bool("ban_cannot_access") {
        permissions.push("ban_cannot_access".to_string());
    }
    permissions
}

fn fallback_username(email: &str) -> String {
    let seed = email
        .trim()
        .replace('@', "-")
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '_' | '-' => ch,
            _ => '-',
        })
        .collect::<String>();

    let mut collapsed = String::with_capacity(seed.len());
    let mut prev_dash = false;
    for ch in seed.chars() {
        if ch == '-' {
            if !prev_dash {
                collapsed.push(ch);
            }
            prev_dash = true;
        } else {
            collapsed.push(ch);
            prev_dash = false;
        }
    }

    let collapsed = collapsed.trim_matches('-');
    if collapsed.is_empty() {
        "user".to_string()
    } else {
        collapsed.to_string()
    }
}

pub(crate) async fn register(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(payload): Json<RegisterRequest>,
) -> Response {
    let key = format!("register:{}", addr.ip());
    if let Err(resp) = enforce_rate(&state, &key, 5, Duration::from_secs(60)) {
        return resp.into_response();
    }
    let email = payload.email.trim();
    if email.is_empty() || !email.contains('@') {
        return api_error(
            StatusCode::BAD_REQUEST,
            ErrorCode::Validation,
            "valid email required",
        )
        .into_response();
    }
    if payload.password.len() < 6 || payload.password.len() > 128 {
        return api_error(
            StatusCode::BAD_REQUEST,
            ErrorCode::Validation,
            "password must be 6-128 chars",
        )
        .into_response();
    }

    let username = payload
        .username
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| fallback_username(email));

    match state
        .rainbow_auth
        .register(email, &payload.password, &username)
        .await
    {
        Ok(message) => (
            StatusCode::OK,
            Json(RegisterResponse {
                status: "ok".to_string(),
                message,
            }),
        )
            .into_response(),
        Err(err) => {
            error!(error = %err, "login failed via rainbow-auth");
            rainbow_auth_error_response(err).into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fallback_username;

    #[test]
    fn fallback_username_uses_email_shape() {
        assert_eq!(fallback_username("Soul.Forum+test@example.com"), "Soul-Forum-test-example-com");
    }

    #[test]
    fn fallback_username_falls_back_for_empty_input() {
        assert_eq!(fallback_username(""), "user");
    }
}

pub(crate) async fn login(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(payload): Json<LoginRequest>,
) -> Response {
    let key = format!("login:{}", addr.ip());
    if let Err(resp) = enforce_rate(&state, &key, 10, Duration::from_secs(60)) {
        return resp.into_response();
    }
    let email = payload.email.trim();
    if email.is_empty() {
        return api_error_from_status(StatusCode::BAD_REQUEST, "email required").into_response();
    }

    match state.rainbow_auth.login(email, &payload.password).await {
        Ok(login) => {
            let fallback_member_id = {
                let mut hasher = DefaultHasher::new();
                login.user.email.hash(&mut hasher);
                let hashed = (hasher.finish() & 0x7FFF_FFFF_FFFF_FFFF) as i64;
                if hashed == 0 {
                    1
                } else {
                    hashed
                }
            };

            let member_id = match state
                .surreal
                .ensure_user(&login.user.email, None, None)
                .await
            {
                Ok(user) => user.legacy_id(),
                Err(err) => {
                    let msg = err.to_string();
                    if msg.contains("401") || msg.contains("Unauthorized") {
                        error!(error = %err, "ensure_user failed with 401, reauth and retry");
                        if let Err(reauth_err) = reauth_from_env(state.surreal.client()).await {
                            error!(error = %reauth_err, "surreal reauth failed in login route, fallback member_id will be used");
                            fallback_member_id
                        } else {
                            match state
                                .surreal
                                .ensure_user(&login.user.email, None, None)
                                .await
                            {
                                Ok(user) => user.legacy_id(),
                                Err(retry_err) => {
                                    error!(error = %retry_err, "failed to ensure user after login retry, fallback member_id will be used");
                                    fallback_member_id
                                }
                            }
                        }
                    } else {
                        error!(error = %err, "failed to ensure user after login, fallback member_id will be used");
                        fallback_member_id
                    }
                }
            };
            (
                StatusCode::OK,
                Json(AuthResponse {
                    status: "ok".to_string(),
                    token: login.token,
                    user: AuthUser {
                        name: login.user.email,
                        role: None,
                        permissions: Some(Vec::new()),
                        member_id: Some(member_id),
                    },
                }),
            )
                .into_response()
        }
        Err(err) => rainbow_auth_error_response(err).into_response(),
    }
}

pub(crate) async fn auth_me(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Response {
    let Some(token) = bearer_from_headers(&headers) else {
        return api_error(
            StatusCode::UNAUTHORIZED,
            ErrorCode::Unauthorized,
            "authorization required",
        )
        .into_response();
    };

    match state.rainbow_auth.me(&token).await {
        Ok(user) => {
            let (member_id, permissions) = match state
                .surreal
                .ensure_user(&user.email, None, None)
                .await
            {
                Ok(forum_user) => {
                    let claims = AuthClaims {
                        sub: user.email.clone(),
                        ..AuthClaims::default()
                    };
                    let fallback_ctx = build_ctx_from_user(&forum_user, &claims);
                    let forum_service = state.forum_service.clone();
                    match tokio::task::spawn_blocking(move || {
                        let mut ctx = fallback_ctx;
                        is_not_banned(&forum_service, &mut ctx, false)?;
                        Ok::<ForumContext, ForumError>(ctx)
                    })
                    .await
                    {
                        Ok(Ok(ctx)) => (forum_user.legacy_id(), ban_markers_from_ctx(&ctx)),
                        Ok(Err(ForumError::PermissionDenied(_))) => (
                            forum_user.legacy_id(),
                            vec!["ban_cannot_access".to_string()],
                        ),
                        Ok(Err(err)) => {
                            error!(error = %err, "failed to evaluate ban flags for auth/me");
                            (forum_user.legacy_id(), Vec::new())
                        }
                        Err(err) => {
                            error!(error = %err, "failed to join ban evaluation task for auth/me");
                            (forum_user.legacy_id(), Vec::new())
                        }
                    }
                }
                Err(err) => {
                    let msg = err.to_string();
                    if msg.contains("401") || msg.contains("Unauthorized") {
                        error!(error = %err, "ensure_user failed with 401 in auth/me, reauth and retry");
                        if reauth_from_env(state.surreal.client()).await.is_ok() {
                            match state.surreal.ensure_user(&user.email, None, None).await {
                                Ok(forum_user) => (forum_user.legacy_id(), Vec::new()),
                                Err(retry_err) => {
                                    error!(error = %retry_err, "failed to sync user for auth/me after retry");
                                    (0, Vec::new())
                                }
                            }
                        } else {
                            (0, Vec::new())
                        }
                    } else {
                        error!(error = %err, "failed to sync user for auth/me");
                        (0, Vec::new())
                    }
                }
            };
            (
                StatusCode::OK,
                Json(AuthMeResponse {
                    status: "ok".to_string(),
                    user: AuthUser {
                        name: user.email,
                        role: None,
                        permissions: Some(permissions),
                        member_id: Some(member_id),
                    },
                }),
            )
                .into_response()
        }
        Err(err) => rainbow_auth_error_response(err).into_response(),
    }
}
