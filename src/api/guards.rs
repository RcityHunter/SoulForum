use axum::{http::StatusCode, Json};

use btc_forum_rust::{
    security::load_permissions,
    services::{BoardAccessEntry, ForumContext, ForumError, ForumService},
    surreal::SurrealClient,
};

use super::{
    auth::user_groups,
    error::{api_error, api_error_from_status},
    state::AppState,
};

pub(crate) fn ensure_permission(
    state: &AppState,
    ctx: &ForumContext,
    permission: &str,
) -> Result<(), (StatusCode, Json<btc_forum_shared::ApiError>)> {
    if state.forum_service.allowed_to(ctx, permission, None, false) {
        Ok(())
    } else {
        Err(api_error(
            StatusCode::FORBIDDEN,
            btc_forum_shared::ErrorCode::Forbidden,
            format!("missing permission: {permission}"),
        ))
    }
}

pub(crate) async fn ensure_permission_for_board(
    state: &AppState,
    ctx: &ForumContext,
    permission: &str,
    board_id: Option<&str>,
) -> Result<(), (StatusCode, Json<btc_forum_shared::ApiError>)> {
    let mut working = ctx.clone();
    if let Some(board) = board_id {
        let forum_service = state.forum_service.clone();
        let board = board.to_string();
        working = match tokio::task::spawn_blocking(move || {
            let mut updated = working;
            load_permissions(&forum_service, &mut updated, Some(board))?;
            Ok::<ForumContext, ForumError>(updated)
        })
        .await
        .map_err(|err| {
            tracing::error!(error = %err, "failed to load permissions");
            api_error_from_status(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to load permissions",
            )
        })? {
            Ok(updated) => updated,
            Err(err) => match &err {
                ForumError::PermissionDenied(message) => {
                    return Err(api_error_from_status(
                        StatusCode::FORBIDDEN,
                        message.clone(),
                    ));
                }
                _ => {
                    tracing::error!(error = %err, "failed to load permissions");
                    return Err(api_error_from_status(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "failed to load permissions",
                    ));
                }
            },
        };
    }
    ensure_permission(state, &working, permission)
}

pub(crate) async fn ensure_board_access(
    state: &AppState,
    ctx: &ForumContext,
    board_id: &str,
) -> Result<(), (StatusCode, Json<btc_forum_shared::ApiError>)> {
    if ctx.user_info.is_admin {
        return Ok(());
    }
    let entries: Vec<BoardAccessEntry> = match load_board_access(state).await {
        Ok(entries) => entries,
        Err(err) => {
            tracing::error!(error = %err, "failed to load board access");
            return Err(api_error_from_status(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to load board access",
            ));
        }
    };
    let Some(entry) = entries.iter().find(|e| e.id == board_id) else {
        return Ok(()); // no explicit rule: allow
    };
    if entry.allowed_groups.is_empty() {
        return Ok(());
    }
    let groups = user_groups(ctx);
    if entry
        .allowed_groups
        .iter()
        .any(|gid| groups.iter().any(|g| g == gid))
    {
        return Ok(());
    }
    Err(api_error_from_status(
        StatusCode::FORBIDDEN,
        "board access denied",
    ))
}

pub(crate) fn ensure_admin(
    ctx: &ForumContext,
) -> Result<(), (StatusCode, Json<btc_forum_shared::ApiError>)> {
    if ctx.user_info.is_admin || ctx.user_info.permissions.contains("admin") {
        Ok(())
    } else {
        Err(api_error_from_status(
            StatusCode::FORBIDDEN,
            "admin permission required",
        ))
    }
}

pub(crate) fn verify_csrf(
    headers: &axum::http::HeaderMap,
) -> Result<(), (StatusCode, Json<btc_forum_shared::ApiError>)> {
    let header_token = headers
        .get("x-csrf-token")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    if header_token.is_empty() {
        return Err(api_error_from_status(
            StatusCode::FORBIDDEN,
            "missing csrf token",
        ));
    }
    if let (Some(header_token), Some(cookie_header)) = (
        headers.get("x-csrf-token"),
        headers.get(axum::http::header::COOKIE),
    ) {
        let header_val = header_token.to_str().unwrap_or_default();
        let cookie_val = cookie_header.to_str().unwrap_or_default();
        let mut ok = false;
        for part in cookie_val.split(';') {
            let trimmed = part.trim();
            if let Some(rest) = trimmed.strip_prefix("XSRF-TOKEN=") {
                if rest == header_val {
                    ok = true;
                    break;
                }
            }
        }
        if !ok {
            return Err(api_error_from_status(
                StatusCode::FORBIDDEN,
                "csrf token mismatch",
            ));
        }
    }
    Ok(())
}

pub(crate) fn validate_content(
    subject: &str,
    body: &str,
) -> Result<(), (StatusCode, Json<btc_forum_shared::ApiError>)> {
    let s = subject.trim();
    let b = body.trim();
    if s.is_empty() || s.len() > 200 {
        return Err(api_error_from_status(
            StatusCode::BAD_REQUEST,
            "subject must be 1..200 chars",
        ));
    }
    if b.is_empty() || b.len() > 10_000 {
        return Err(api_error_from_status(
            StatusCode::BAD_REQUEST,
            "body must be 1..10000 chars",
        ));
    }
    Ok(())
}

pub(crate) fn enforce_rate(
    state: &AppState,
    key: &str,
    limit: u32,
    window: std::time::Duration,
) -> Result<(), (StatusCode, Json<btc_forum_shared::ApiError>)> {
    if state.rate_limiter.allow(key, limit, window) {
        Ok(())
    } else {
        Err(api_error(
            StatusCode::TOO_MANY_REQUESTS,
            btc_forum_shared::ErrorCode::RateLimited,
            "rate limit exceeded",
        ))
    }
}

pub(crate) async fn fetch_topic_board_id(client: &SurrealClient, topic_id: &str) -> Option<String> {
    let topic_id_owned = topic_id.to_string();
    let mut response = client
        .query(
            r#"
            SELECT board_id FROM type::thing("topics", $id) LIMIT 1;
            "#,
        )
        .bind(("id", topic_id_owned))
        .await
        .ok()?;
    #[derive(serde::Deserialize)]
    struct Row {
        board_id: Option<String>,
    }
    let rows: Vec<Row> = response.take(0).ok()?;
    rows.into_iter().find_map(|r| r.board_id)
}

pub(crate) async fn load_board_access(
    state: &AppState,
) -> Result<Vec<BoardAccessEntry>, ForumError> {
    let forum_service = state.forum_service.clone();
    tokio::task::spawn_blocking(move || forum_service.list_board_access())
        .await
        .map_err(|e| ForumError::Internal(format!("board access task failed: {e}")))?
}
