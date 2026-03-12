use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use tracing::error;

use btc_forum_rust::{
    auth::AuthClaims,
    services::{ForumError, ForumService, PersonalMessageFolder, SendPersonalMessage},
};
use btc_forum_shared::{
    PersonalMessageIdsPayload, PersonalMessageIdsResponse, PersonalMessageListResponse,
    PersonalMessageSendPayload, PersonalMessageSendResponse,
};

use super::{
    auth::{ensure_user_ctx, require_auth},
    error::api_error_from_status,
    guards::verify_csrf,
    state::{run_forum_blocking, AppState},
    utils::sanitize_input,
};

fn to_personal_message(
    msg: btc_forum_rust::services::PersonalMessageSummary,
) -> btc_forum_shared::PersonalMessage {
    btc_forum_shared::PersonalMessage {
        id: msg.id,
        subject: msg.subject,
        body: msg.body_preview,
        sender_id: msg.sender_id,
        sender_name: msg.sender_name,
        sent_at: msg.sent_at.to_rfc3339(),
        is_read: msg.is_read,
        recipients: msg
            .recipients
            .into_iter()
            .map(|peer| btc_forum_shared::PersonalMessagePeer {
                member_id: peer.member_id,
                name: peer.name,
            })
            .collect(),
    }
}

#[derive(Deserialize)]
pub(crate) struct PersonalMessageListQuery {
    pub(crate) folder: Option<String>,
    pub(crate) label: Option<i64>,
    pub(crate) offset: Option<usize>,
    pub(crate) limit: Option<usize>,
}

pub(crate) async fn list_personal_messages(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    Query(query): Query<PersonalMessageListQuery>,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c,
        Err(resp) => return resp.into_response(),
    };
    let (_user, ctx) = match ensure_user_ctx(&state, &claims).await {
        Ok(value) => value,
        Err(resp) => return resp.into_response(),
    };
    let folder = match query
        .folder
        .as_deref()
        .unwrap_or("inbox")
        .to_lowercase()
        .as_str()
    {
        "sent" => PersonalMessageFolder::Sent,
        _ => PersonalMessageFolder::Inbox,
    };
    let limit = query.limit.unwrap_or(50).min(200);
    let offset = query.offset.unwrap_or(0);
    let label = query.label;
    match run_forum_blocking(&state, move |forum| {
        forum.personal_message_page(ctx.user_info.id, folder.clone(), label, offset, limit)
    })
    .await
    {
        Ok(page) => (
            StatusCode::OK,
            Json(PersonalMessageListResponse {
                status: "ok".to_string(),
                messages: page.messages.into_iter().map(to_personal_message).collect(),
                total: page.total,
                unread: page.unread,
            }),
        )
            .into_response(),
        Err(err) => {
            error!(error = %err, "failed to list personal messages");
            api_error_from_status(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
                .into_response()
        }
    }
}

pub(crate) async fn send_personal_message_api(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<PersonalMessageSendPayload>,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c,
        Err(resp) => return resp.into_response(),
    };
    if let Err(resp) = verify_csrf(&headers) {
        return resp.into_response();
    }
    if payload.to.is_empty() {
        return api_error_from_status(StatusCode::BAD_REQUEST, "recipient required")
            .into_response();
    }
    if payload.subject.trim().is_empty() || payload.subject.len() > 200 {
        return api_error_from_status(StatusCode::BAD_REQUEST, "subject must be 1-200 chars")
            .into_response();
    }
    if payload.body.trim().is_empty() || payload.body.len() > 4000 {
        return api_error_from_status(StatusCode::BAD_REQUEST, "body must be 1-4000 chars")
            .into_response();
    }
    let (user, ctx) = match ensure_user_ctx(&state, &claims).await {
        Ok(value) => value,
        Err(resp) => return resp.into_response(),
    };
    if ctx.session.bool("ban_cannot_access") || ctx.session.bool("ban_cannot_post") {
        return api_error_from_status(StatusCode::FORBIDDEN, "banned").into_response();
    }

    let recipients = payload.to.clone();
    let recipient_ids = match run_forum_blocking(&state, move |forum| {
        let mut ids = Vec::new();
        for name in recipients {
            match forum.find_member_by_name(&name) {
                Ok(Some(member)) => ids.push(member.id),
                _ => return Err(ForumError::Validation(format!("unknown recipient: {name}"))),
            }
        }
        Ok(ids)
    })
    .await
    {
        Ok(ids) => ids,
        Err(err) => {
            return api_error_from_status(StatusCode::BAD_REQUEST, err.to_string()).into_response();
        }
    };

    let message = SendPersonalMessage {
        sender_id: ctx.user_info.id,
        sender_name: user.name.clone(),
        to: recipient_ids,
        bcc: Vec::new(),
        subject: sanitize_input(&payload.subject),
        body: sanitize_input(&payload.body),
    };
    match run_forum_blocking(&state, move |forum| forum.send_personal_message(message)).await {
        Ok(result) => (
            StatusCode::CREATED,
            Json(PersonalMessageSendResponse {
                status: "ok".to_string(),
                sent_to: result.recipient_ids,
            }),
        )
            .into_response(),
        Err(err) => {
            error!(error = %err, "failed to send personal message");
            api_error_from_status(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
                .into_response()
        }
    }
}

pub(crate) async fn mark_personal_messages_read(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<PersonalMessageIdsPayload>,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c,
        Err(resp) => return resp.into_response(),
    };
    if let Err(resp) = verify_csrf(&headers) {
        return resp.into_response();
    }
    if payload.ids.is_empty() {
        return api_error_from_status(StatusCode::BAD_REQUEST, "ids required").into_response();
    }
    let (_user, ctx) = match ensure_user_ctx(&state, &claims).await {
        Ok(value) => value,
        Err(resp) => return resp.into_response(),
    };
    let ids = payload.ids.clone();
    match run_forum_blocking(&state, move |forum| {
        forum.mark_personal_messages(ctx.user_info.id, &ids, true)
    })
    .await
    {
        Ok(_) => (
            StatusCode::OK,
            Json(PersonalMessageIdsResponse {
                status: "ok".to_string(),
                ids: payload.ids,
            }),
        )
            .into_response(),
        Err(err) => {
            error!(error = %err, "failed to mark personal messages read");
            api_error_from_status(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
                .into_response()
        }
    }
}

pub(crate) async fn delete_personal_messages_api(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<PersonalMessageIdsPayload>,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c,
        Err(resp) => return resp.into_response(),
    };
    if let Err(resp) = verify_csrf(&headers) {
        return resp.into_response();
    }
    if payload.ids.is_empty() {
        return api_error_from_status(StatusCode::BAD_REQUEST, "ids required").into_response();
    }
    let (_user, ctx) = match ensure_user_ctx(&state, &claims).await {
        Ok(value) => value,
        Err(resp) => return resp.into_response(),
    };
    let ids = payload.ids.clone();
    match run_forum_blocking(&state, move |forum| {
        forum.delete_personal_messages(ctx.user_info.id, PersonalMessageFolder::Inbox, &ids)
    })
    .await
    {
        Ok(_) => (
            StatusCode::OK,
            Json(PersonalMessageIdsResponse {
                status: "ok".to_string(),
                ids: payload.ids,
            }),
        )
            .into_response(),
        Err(err) => {
            error!(error = %err, "failed to delete personal messages");
            api_error_from_status(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
                .into_response()
        }
    }
}
