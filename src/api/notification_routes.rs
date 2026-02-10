use axum::{
    extract::{ConnectInfo, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use std::{net::SocketAddr, time::Duration};
use tracing::error;

use btc_forum_rust::auth::AuthClaims;
use btc_forum_shared::{
    CreateNotificationPayload, MarkNotificationPayload, MarkNotificationResponse,
    NotificationCreateResponse, NotificationListResponse,
};

use super::{
    auth::{ensure_user_ctx, require_auth},
    error::api_error_from_status,
    guards::{enforce_rate, verify_csrf},
    state::AppState,
    utils::sanitize_input,
};

fn to_notification(
    note: btc_forum_rust::surreal::SurrealNotification,
) -> btc_forum_shared::Notification {
    btc_forum_shared::Notification {
        id: note.id.unwrap_or_default(),
        user: note.user,
        subject: note.subject,
        body: note.body,
        created_at: note.created_at,
        is_read: note.is_read,
    }
}

pub(crate) async fn create_notification(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<CreateNotificationPayload>,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c,
        Err(resp) => return resp.into_response(),
    };
    let (_user, ctx) = match ensure_user_ctx(&state, &claims).await {
        Ok(value) => value,
        Err(resp) => return resp.into_response(),
    };
    if let Err(resp) = verify_csrf(&headers) {
        return resp.into_response();
    }
    let key = format!("notify:{}", addr.ip());
    if let Err(resp) = enforce_rate(&state, &key, 20, Duration::from_secs(60)) {
        return resp.into_response();
    }
    let target_user = if ctx.user_info.is_admin {
        payload.user.unwrap_or_else(|| claims.sub.clone())
    } else {
        claims.sub.clone()
    };
    if payload.subject.trim().is_empty() || payload.subject.len() > 200 {
        return api_error_from_status(StatusCode::BAD_REQUEST, "subject must be 1-200 chars")
            .into_response();
    }
    if payload.body.trim().is_empty() || payload.body.len() > 4000 {
        return api_error_from_status(StatusCode::BAD_REQUEST, "body must be 1-4000 chars")
            .into_response();
    }
    match state
        .surreal
        .create_notification(
            &target_user,
            &sanitize_input(&payload.subject),
            &sanitize_input(&payload.body),
        )
        .await
    {
        Ok(note) => (
            StatusCode::CREATED,
            Json(NotificationCreateResponse {
                status: "ok".to_string(),
                notification: to_notification(note),
            }),
        )
            .into_response(),
        Err(err) => {
            error!(error = %err, "failed to create notification");
            api_error_from_status(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
                .into_response()
        }
    }
}

pub(crate) async fn mark_notification_read(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<MarkNotificationPayload>,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c,
        Err(resp) => return resp.into_response(),
    };
    if let Err(resp) = verify_csrf(&headers) {
        return resp.into_response();
    }
    if payload.id.trim().is_empty() {
        return api_error_from_status(StatusCode::BAD_REQUEST, "id required").into_response();
    }
    match state.surreal.mark_notification_read(&payload.id).await {
        Ok(_) => (
            StatusCode::OK,
            Json(MarkNotificationResponse {
                status: "ok".to_string(),
                id: payload.id,
                user: claims.sub.clone(),
            }),
        )
            .into_response(),
        Err(err) => {
            error!(error = %err, "failed to mark notification read");
            api_error_from_status(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
                .into_response()
        }
    }
}

pub(crate) async fn list_notifications(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c,
        Err(resp) => return resp.into_response(),
    };
    let target = claims.sub.clone();
    match state.surreal.list_notifications(&target).await {
        Ok(items) => (
            StatusCode::OK,
            Json(NotificationListResponse {
                status: "ok".to_string(),
                notifications: items.into_iter().map(to_notification).collect(),
            }),
        )
            .into_response(),
        Err(err) => {
            error!(error = %err, "failed to list notifications");
            api_error_from_status(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
                .into_response()
        }
    }
}
