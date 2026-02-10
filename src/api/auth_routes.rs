use axum::{
    extract::{ConnectInfo, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use std::{net::SocketAddr, time::Duration};
use tracing::error;

use btc_forum_shared::{
    AuthMeResponse, AuthResponse, AuthUser, ErrorCode, LoginRequest, RegisterRequest,
    RegisterResponse,
};

use super::{
    auth::bearer_from_headers,
    error::{api_error, api_error_from_status, rainbow_auth_error_response},
    guards::enforce_rate,
    state::AppState,
};

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

    match state.rainbow_auth.register(email, &payload.password).await {
        Ok(message) => (
            StatusCode::OK,
            Json(RegisterResponse {
                status: "ok".to_string(),
                message,
            }),
        )
            .into_response(),
        Err(err) => rainbow_auth_error_response(err).into_response(),
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
            let forum_user = match state
                .surreal
                .ensure_user(&login.user.email, None, None)
                .await
            {
                Ok(user) => user,
                Err(err) => {
                    error!(error = %err, "failed to ensure user after login");
                    return api_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        ErrorCode::Internal,
                        "failed to sync user",
                    )
                    .into_response();
                }
            };
            let member_id = forum_user.legacy_id();
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
            let member_id = match state.surreal.ensure_user(&user.email, None, None).await {
                Ok(forum_user) => forum_user.legacy_id(),
                Err(err) => {
                    error!(error = %err, "failed to sync user for auth/me");
                    0
                }
            };
            (
                StatusCode::OK,
                Json(AuthMeResponse {
                    status: "ok".to_string(),
                    user: AuthUser {
                        name: user.email,
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
