use axum::{
    extract::{ConnectInfo, Multipart, Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use std::{net::SocketAddr, time::Duration};
use tokio::fs;
use tracing::error;

use btc_forum_rust::{auth::AuthClaims, services::ForumService};
use btc_forum_shared::{
    AttachmentCreateResponse, AttachmentDeletePayload, AttachmentDeleteResponse,
    AttachmentListResponse, AttachmentUploadResponse, CreateAttachmentPayload,
};

use super::{
    auth::{ensure_user_ctx, require_auth},
    error::api_error_from_status,
    guards::{enforce_rate, verify_csrf},
    state::{
        allowed_mime, max_upload_bytes, run_forum_blocking, upload_base_url, upload_dir, AppState,
    },
    utils::sanitize_filename,
};

fn to_attachment_meta(
    att: btc_forum_rust::surreal::SurrealAttachment,
) -> btc_forum_shared::AttachmentMeta {
    btc_forum_shared::AttachmentMeta {
        id: att.id,
        filename: att.filename,
        size_bytes: att.size_bytes,
        mime_type: att.mime_type,
        created_at: att.created_at,
    }
}

pub(crate) async fn list_attachments(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c,
        Err(resp) => return resp.into_response(),
    };
    if let Err(resp) = ensure_user_ctx(&state, claims).await.map(|_| ()) {
        return resp.into_response();
    }
    let base_url = upload_base_url();
    match state.surreal.list_attachments_for_user(&claims.sub).await {
        Ok(items) => (
            StatusCode::OK,
            Json(AttachmentListResponse {
                status: "ok".to_string(),
                attachments: items.into_iter().map(to_attachment_meta).collect(),
                base_url: Some(base_url),
            }),
        )
            .into_response(),
        Err(err) => {
            error!(error = %err, "failed to list attachments");
            api_error_from_status(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
                .into_response()
        }
    }
}

pub(crate) async fn create_attachment_meta(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<CreateAttachmentPayload>,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c,
        Err(resp) => return resp.into_response(),
    };
    if let Err(resp) = verify_csrf(&headers) {
        return resp.into_response();
    }
    let (_user, ctx) = match ensure_user_ctx(&state, claims).await {
        Ok(value) => value,
        Err(resp) => return resp.into_response(),
    };
    if ctx.session.bool("ban_cannot_access") || ctx.session.bool("ban_cannot_post") {
        return api_error_from_status(StatusCode::FORBIDDEN, "banned").into_response();
    }
    let key = format!("attach:{}", addr.ip());
    if let Err(resp) = enforce_rate(&state, &key, 30, Duration::from_secs(60)) {
        return resp.into_response();
    }
    if payload.filename.trim().is_empty() || payload.filename.len() > 255 {
        return api_error_from_status(StatusCode::BAD_REQUEST, "filename must be 1-255 chars")
            .into_response();
    }
    if payload.size_bytes < 0 || payload.size_bytes > max_upload_bytes() {
        return api_error_from_status(StatusCode::BAD_REQUEST, "size_bytes invalid")
            .into_response();
    }
    if let Some(list) = allowed_mime() {
        if let Some(mt) = payload.mime_type.as_deref() {
            let mt_lower = mt.to_lowercase();
            if !list.iter().any(|allowed| mt_lower.starts_with(allowed)) {
                return api_error_from_status(StatusCode::BAD_REQUEST, "mime_type not allowed")
                    .into_response();
            }
        }
    }
    let base_url = upload_base_url();
    match state
        .surreal
        .create_attachment_meta(
            &claims.sub,
            &payload.filename,
            payload.size_bytes,
            payload.mime_type.as_deref(),
            payload.board_id.as_deref(),
            payload.topic_id.as_deref(),
        )
        .await
    {
        Ok(att) => (
            StatusCode::CREATED,
            Json(AttachmentCreateResponse {
                status: "ok".to_string(),
                attachment: to_attachment_meta(att),
                base_url: Some(base_url),
                url: None,
            }),
        )
            .into_response(),
        Err(err) => {
            error!(error = %err, "failed to create attachment meta");
            api_error_from_status(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
                .into_response()
        }
    }
}

pub(crate) async fn upload_attachment(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: axum::http::HeaderMap,
    mut multipart: Multipart,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c,
        Err(resp) => return resp.into_response(),
    };
    if let Err(resp) = verify_csrf(&headers) {
        return resp.into_response();
    }
    let (_user, ctx) = match ensure_user_ctx(&state, claims).await {
        Ok(value) => value,
        Err(resp) => return resp.into_response(),
    };
    if ctx.session.bool("ban_cannot_access") || ctx.session.bool("ban_cannot_post") {
        return api_error_from_status(StatusCode::FORBIDDEN, "banned").into_response();
    }
    let key = format!("attach_upload:{}", addr.ip());
    if let Err(resp) = enforce_rate(&state, &key, 10, Duration::from_secs(60)) {
        return resp.into_response();
    }

    let mut file_bytes: Option<axum::body::Bytes> = None;
    let mut file_name: Option<String> = None;
    let mut mime: Option<String> = None;
    let mut board_id: Option<String> = None;
    let mut topic_id: Option<String> = None;

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().map(|s| s.to_string());
        match name.as_deref() {
            Some("file") => {
                file_name = field.file_name().map(|s| s.to_string());
                mime = field.content_type().map(|s| s.to_string());
                match field.bytes().await {
                    Ok(bytes) => file_bytes = Some(bytes),
                    Err(err) => {
                        error!(error = %err, "failed to read upload");
                        return api_error_from_status(
                            StatusCode::BAD_REQUEST,
                            "failed to read upload",
                        )
                        .into_response();
                    }
                }
            }
            Some("board_id") => {
                if let Ok(text) = field.text().await {
                    board_id = Some(text);
                }
            }
            Some("topic_id") => {
                if let Ok(text) = field.text().await {
                    topic_id = Some(text);
                }
            }
            _ => {}
        }
    }

    let Some(bytes) = file_bytes else {
        return api_error_from_status(StatusCode::BAD_REQUEST, "missing file field")
            .into_response();
    };
    let raw_name = file_name.unwrap_or_else(|| "upload.bin".into());
    let safe_name = sanitize_filename(&raw_name);
    let size_bytes = bytes.len() as i64;
    if size_bytes == 0 || size_bytes > max_upload_bytes() {
        return api_error_from_status(StatusCode::BAD_REQUEST, "file size must be 1..max")
            .into_response();
    }
    if let Some(list) = allowed_mime() {
        if let Some(mt) = mime.clone() {
            let mt_lower = mt.to_lowercase();
            if !list.iter().any(|allowed| mt_lower.starts_with(allowed)) {
                return api_error_from_status(StatusCode::BAD_REQUEST, "mime_type not allowed")
                    .into_response();
            }
        }
    }

    let dir = upload_dir();
    if let Err(err) = fs::create_dir_all(&dir).await {
        error!(error = %err, "failed to create upload dir");
        return api_error_from_status(
            StatusCode::INTERNAL_SERVER_ERROR,
            "server cannot create upload dir",
        )
        .into_response();
    }

    let mut path = dir.join(&safe_name);
    let (stem, ext) = match path.file_stem().and_then(|s| s.to_str()) {
        Some(stem) => (
            stem.to_string(),
            path.extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_string()),
        ),
        None => (safe_name.clone(), None),
    };
    let mut counter = 1;
    while fs::try_exists(&path).await.unwrap_or(false) {
        let new_name = if let Some(ext) = &ext {
            format!("{stem}-{counter}.{ext}")
        } else {
            format!("{stem}-{counter}")
        };
        path = dir.join(&new_name);
        counter += 1;
    }
    let final_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(&safe_name)
        .to_string();

    if let Err(err) = fs::write(&path, &bytes).await {
        error!(error = %err, "failed to write upload");
        return api_error_from_status(StatusCode::INTERNAL_SERVER_ERROR, "failed to write upload")
            .into_response();
    }

    let base_url = upload_base_url();
    let public_url = format!("{}/{}", base_url.trim_end_matches('/'), &final_name);

    match state
        .surreal
        .create_attachment_meta(
            &claims.sub,
            &final_name,
            size_bytes,
            mime.as_deref(),
            board_id.as_deref(),
            topic_id.as_deref(),
        )
        .await
    {
        Ok(att) => (
            StatusCode::CREATED,
            Json(AttachmentUploadResponse {
                status: "ok".to_string(),
                attachment: to_attachment_meta(att),
                base_url: Some(base_url),
                url: Some(public_url),
            }),
        )
            .into_response(),
        Err(err) => {
            error!(error = %err, "failed to store attachment meta");
            api_error_from_status(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
                .into_response()
        }
    }
}

pub(crate) async fn delete_attachment_api(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<AttachmentDeletePayload>,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c,
        Err(resp) => return resp.into_response(),
    };
    if let Err(resp) = verify_csrf(&headers) {
        return resp.into_response();
    }
    let (_user, ctx) = match ensure_user_ctx(&state, claims).await {
        Ok(value) => value,
        Err(resp) => return resp.into_response(),
    };
    let id_num = payload
        .id
        .split(':')
        .next_back()
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0);
    if id_num <= 0 {
        return api_error_from_status(StatusCode::BAD_REQUEST, "invalid id").into_response();
    }
    if !ctx.user_info.is_admin {
        match state.surreal.list_attachments_for_user(&claims.sub).await {
            Ok(items) => {
                if !items.iter().any(|a| a.id.as_deref() == Some(&payload.id)) {
                    return api_error_from_status(StatusCode::FORBIDDEN, "not allowed to delete")
                        .into_response();
                }
            }
            Err(err) => {
                error!(error = %err, "failed to load attachments for delete");
                return api_error_from_status(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "cannot verify ownership",
                )
                .into_response();
            }
        }
    }
    match run_forum_blocking(&state, move |forum| forum.delete_attachment(id_num)).await {
        Ok(_) => (
            StatusCode::OK,
            Json(AttachmentDeleteResponse {
                status: "ok".to_string(),
                id: payload.id,
            }),
        )
            .into_response(),
        Err(err) => {
            error!(error = %err, user = %claims.sub, "failed to delete attachment");
            api_error_from_status(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
                .into_response()
        }
    }
}

pub(crate) async fn serve_upload(Path(path): Path<String>) -> impl IntoResponse {
    let mut full = upload_dir();
    for segment in path.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            continue;
        }
        full.push(segment);
    }
    match fs::read(&full).await {
        Ok(bytes) => (
            StatusCode::OK,
            [(axum::http::header::CONTENT_TYPE, "application/octet-stream")],
            bytes,
        )
            .into_response(),
        Err(err) => {
            if err.kind() == std::io::ErrorKind::NotFound {
                StatusCode::NOT_FOUND.into_response()
            } else {
                error!(error = %err, ?full, "failed to read uploaded file");
                StatusCode::INTERNAL_SERVER_ERROR.into_response()
            }
        }
    }
}
