use axum::{
    extract::{ConnectInfo, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use serde_json::json;
use std::{collections::HashMap, net::SocketAddr, time::Duration};
use tracing::error;

use btc_forum_rust::{
    auth::AuthClaims,
    services::{BanAffects, BanCondition, BanRule, ForumService, SendPersonalMessage},
    surreal::SurrealUser,
};
use btc_forum_shared::{
    ActionLogEntry, ActionLogsResponse, AdminAccount, AdminAccountsResponse, AdminGroup,
    AdminGroupsResponse, AdminNotifyPayload, AdminNotifyResponse, AdminUser, AdminUsersResponse,
    BanApplyResponse, BanListResponse, BanMemberView, BanPayload, BanRevokeResponse, BanRuleView,
    BoardAccessPayload, BoardAccessResponse, BoardPermissionEntry, BoardPermissionPayload,
    BoardPermissionResponse, UpdateBoardAccessResponse, UpdateBoardPermissionResponse,
};

use super::{
    auth::{ensure_user_ctx, require_auth},
    error::api_error_from_status,
    guards::{enforce_rate, ensure_admin, load_board_access, validate_content, verify_csrf},
    state::{run_forum_blocking, AppState},
    utils::sanitize_input,
};

#[derive(Deserialize)]
pub(crate) struct AdminUsersQuery {
    pub(crate) q: Option<String>,
    pub(crate) limit: Option<usize>,
}

#[derive(Deserialize)]
pub(crate) struct AdminPageQuery {
    pub(crate) _limit: Option<usize>,
    pub(crate) _offset: Option<usize>,
}

pub(crate) async fn list_users(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    Query(params): Query<AdminUsersQuery>,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c.clone(),
        Err(resp) => return resp.into_response(),
    };
    let (_user, ctx) = match ensure_user_ctx(&state, &claims).await {
        Ok(value) => value,
        Err(resp) => return resp.into_response(),
    };
    if let Err(resp) = ensure_admin(&ctx) {
        return resp.into_response();
    }
    match run_forum_blocking(&state, move |forum| forum.list_members()).await {
        Ok(members) => {
            let filtered: Vec<AdminUser> = members
                .into_iter()
                .filter(|m| {
                    if let Some(ref q) = params.q {
                        m.name.to_lowercase().contains(&q.to_lowercase())
                    } else {
                        true
                    }
                })
                .take(params.limit.unwrap_or(200))
                .map(|m| AdminUser {
                    id: m.id,
                    name: m.name,
                    primary_group: m.primary_group,
                    additional_groups: m.additional_groups,
                    warning: m.warning,
                })
                .collect();
            (
                StatusCode::OK,
                Json(AdminUsersResponse {
                    status: "ok".to_string(),
                    members: filtered,
                }),
            )
                .into_response()
        }
        Err(err) => {
            error!(error = %err, "failed to list members");
            api_error_from_status(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
                .into_response()
        }
    }
}

pub(crate) async fn list_admins(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c.clone(),
        Err(resp) => return resp.into_response(),
    };
    let (_user, ctx) = match ensure_user_ctx(&state, &claims).await {
        Ok(value) => value,
        Err(resp) => return resp.into_response(),
    };
    if let Err(resp) = ensure_admin(&ctx) {
        return resp.into_response();
    }

    let mut response = match state
        .surreal
        .client()
        .query(
            r#"
            SELECT meta::id(id) as id, name, role, permissions, password_hash, created_at
            FROM users
            WHERE role = 'admin' OR permissions CONTAINS 'manage_boards'
            ORDER BY created_at ASC;
            "#,
        )
        .await
    {
        Ok(resp) => resp,
        Err(err) => {
            error!(error = %err, "failed to list admin users");
            return api_error_from_status(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
                .into_response();
        }
    };

    let admins: Vec<SurrealUser> = match response.take(0) {
        Ok(value) => value,
        Err(err) => {
            error!(error = %err, "failed to parse admin users");
            return api_error_from_status(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
                .into_response();
        }
    };

    let output: Vec<AdminAccount> = admins
        .into_iter()
        .map(|user| AdminAccount {
            id: user.legacy_id(),
            name: user.name,
            role: user.role,
            permissions: user.permissions.unwrap_or_default(),
        })
        .collect();

    (
        StatusCode::OK,
        Json(AdminAccountsResponse {
            status: "ok".to_string(),
            admins: output,
        }),
    )
        .into_response()
}

pub(crate) async fn list_groups(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c.clone(),
        Err(resp) => return resp.into_response(),
    };
    let (_user, ctx) = match ensure_user_ctx(&state, &claims).await {
        Ok(value) => value,
        Err(resp) => return resp.into_response(),
    };
    if let Err(resp) = ensure_admin(&ctx) {
        return resp.into_response();
    }
    match run_forum_blocking(&state, move |forum| forum.list_membergroups()).await {
        Ok(groups) => {
            let output: Vec<AdminGroup> = groups
                .into_iter()
                .map(|g| AdminGroup {
                    id: g.id,
                    name: g.name,
                })
                .collect();
            (
                StatusCode::OK,
                Json(AdminGroupsResponse {
                    status: "ok".to_string(),
                    groups: output,
                }),
            )
                .into_response()
        }
        Err(err) => {
            error!(error = %err, "failed to list membergroups");
            api_error_from_status(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
                .into_response()
        }
    }
}

pub(crate) async fn admin_notify(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<AdminNotifyPayload>,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c.clone(),
        Err(resp) => return resp.into_response(),
    };
    let (_user, ctx) = match ensure_user_ctx(&state, &claims).await {
        Ok(value) => value,
        Err(resp) => return resp.into_response(),
    };
    if let Err(resp) = ensure_admin(&ctx) {
        return resp.into_response();
    }
    let key = format!("{}:{}", claims.sub, addr.ip());
    if let Err(resp) = enforce_rate(&state, &key, 5, Duration::from_secs(60)) {
        return resp.into_response();
    }
    if let Err(resp) = verify_csrf(&headers) {
        return resp.into_response();
    }
    if payload.user_ids.is_empty() {
        return api_error_from_status(StatusCode::BAD_REQUEST, "user_ids required").into_response();
    }
    if payload.user_ids.len() > 50 {
        return api_error_from_status(StatusCode::BAD_REQUEST, "user_ids too many (max 50)")
            .into_response();
    }
    if let Err(resp) = validate_content(&payload.subject, &payload.body) {
        return resp.into_response();
    }
    let message = SendPersonalMessage {
        sender_id: 0,
        sender_name: "admin".into(),
        to: payload.user_ids.clone(),
        bcc: Vec::new(),
        subject: sanitize_input(&payload.subject),
        body: sanitize_input(&payload.body),
    };
    let subject = payload.subject.clone();
    let user_ids = payload.user_ids.clone();
    match run_forum_blocking(&state, move |forum| forum.send_personal_message(message)).await {
        Ok(result) => (
            StatusCode::OK,
            Json(AdminNotifyResponse {
                status: "ok".to_string(),
                sent_to: result.recipient_ids,
            }),
        )
            .into_response(),
        Err(err) => {
            error!(error = %err, "failed to send admin notification");
            let log_payload = json!({
                "error": err.to_string(),
                "subject": subject,
                "user_ids": user_ids,
            });
            let _ = run_forum_blocking(&state, move |forum| {
                forum.log_action("admin_notify_error", None, &log_payload)
            })
            .await;
            api_error_from_status(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
                .into_response()
        }
    }
}

pub(crate) async fn list_bans(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    Query(_): Query<AdminPageQuery>,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c.clone(),
        Err(resp) => return resp.into_response(),
    };
    let (_user, ctx) = match ensure_user_ctx(&state, &claims).await {
        Ok(value) => value,
        Err(resp) => return resp.into_response(),
    };
    if let Err(resp) = ensure_admin(&ctx) {
        return resp.into_response();
    }
    match run_forum_blocking(&state, move |forum| {
        let bans = forum.list_ban_rules()?;
        let members = forum.list_members()?;
        Ok((bans, members))
    })
    .await
    {
        Ok((bans, members)) => {
            let member_map: HashMap<i64, String> =
                members.into_iter().map(|m| (m.id, m.name)).collect();
            let mut view: Vec<BanRuleView> = Vec::new();
            for ban in bans {
                let mut member_ids: Vec<i64> = Vec::new();
                let mut emails = Vec::new();
                let mut ips = Vec::new();
                for cond in &ban.conditions {
                    match &cond.affects {
                        BanAffects::Account { member_id } => member_ids.push(*member_id),
                        BanAffects::Email { value } => emails.push(value.clone()),
                        BanAffects::Ip { value } => ips.push(value.clone()),
                    }
                }
                member_ids.sort_unstable();
                member_ids.dedup();
                emails.sort();
                emails.dedup();
                ips.sort();
                ips.dedup();
                let members = member_ids
                    .iter()
                    .map(|id| BanMemberView {
                        member_id: *id,
                        name: member_map.get(id).cloned().unwrap_or_default(),
                    })
                    .collect::<Vec<_>>();
                view.push(BanRuleView {
                    id: ban.id,
                    reason: ban.reason,
                    expires_at: ban.expires_at.map(|dt| dt.to_rfc3339()),
                    members,
                    emails,
                    ips,
                });
            }
            (
                StatusCode::OK,
                Json(BanListResponse {
                    status: "ok".to_string(),
                    bans: view,
                }),
            )
                .into_response()
        }
        Err(err) => {
            error!(error = %err, "failed to list bans");
            api_error_from_status(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
                .into_response()
        }
    }
}

pub(crate) async fn apply_ban(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<BanPayload>,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c.clone(),
        Err(resp) => return resp.into_response(),
    };
    let (_user, ctx) = match ensure_user_ctx(&state, &claims).await {
        Ok(value) => value,
        Err(resp) => return resp.into_response(),
    };
    if let Err(resp) = ensure_admin(&ctx) {
        return resp.into_response();
    }
    if let Err(resp) = verify_csrf(&headers) {
        return resp.into_response();
    }
    let member_id = payload.member_id.unwrap_or(0);
    if member_id == 0 {
        return api_error_from_status(StatusCode::BAD_REQUEST, "member_id required")
            .into_response();
    }
    let hours = payload.hours.unwrap_or(24).clamp(1, 24 * 365);
    let until = chrono::Utc::now()
        .checked_add_signed(chrono::Duration::hours(hours))
        .map(|dt| dt.timestamp())
        .unwrap_or_else(|| chrono::Utc::now().timestamp());
    let rule = BanRule {
        id: 0,
        reason: payload.reason.clone(),
        expires_at: Some(chrono::DateTime::from_timestamp(until, 0).unwrap()),
        conditions: vec![BanCondition {
            id: 0,
            reason: payload.reason.clone(),
            affects: BanAffects::Account { member_id },
            expires_at: Some(chrono::DateTime::from_timestamp(until, 0).unwrap()),
        }],
    };
    match run_forum_blocking(&state, move |forum| forum.save_ban_rule(rule)).await {
        Ok(id) => (
            StatusCode::OK,
            Json(BanApplyResponse {
                status: "ok".to_string(),
                ban_id: id,
                member_id,
            }),
        )
            .into_response(),
        Err(err) => {
            error!(error = %err, "failed to apply ban");
            api_error_from_status(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
                .into_response()
        }
    }
}

pub(crate) async fn revoke_ban(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<BanPayload>,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c.clone(),
        Err(resp) => return resp.into_response(),
    };
    let (_user, ctx) = match ensure_user_ctx(&state, &claims).await {
        Ok(value) => value,
        Err(resp) => return resp.into_response(),
    };
    if let Err(resp) = ensure_admin(&ctx) {
        return resp.into_response();
    }
    if let Err(resp) = verify_csrf(&headers) {
        return resp.into_response();
    }
    let ban_id = payload.ban_id.or(payload.member_id).unwrap_or(0);
    if ban_id == 0 {
        return api_error_from_status(StatusCode::BAD_REQUEST, "ban_id required").into_response();
    }
    match run_forum_blocking(&state, move |forum| forum.delete_ban_rule(ban_id)).await {
        Ok(_) => (
            StatusCode::OK,
            Json(BanRevokeResponse {
                status: "ok".to_string(),
                ban_id,
            }),
        )
            .into_response(),
        Err(err) => {
            error!(error = %err, "failed to revoke ban");
            api_error_from_status(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
                .into_response()
        }
    }
}

pub(crate) async fn list_action_logs(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    Query(_): Query<AdminPageQuery>,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c.clone(),
        Err(resp) => return resp.into_response(),
    };
    let (_user, ctx) = match ensure_user_ctx(&state, &claims).await {
        Ok(value) => value,
        Err(resp) => return resp.into_response(),
    };
    if let Err(resp) = ensure_admin(&ctx) {
        return resp.into_response();
    }
    match run_forum_blocking(&state, move |forum| forum.list_action_logs()).await {
        Ok(logs) => (
            StatusCode::OK,
            Json(ActionLogsResponse {
                status: "ok".to_string(),
                logs: logs
                    .into_iter()
                    .map(|log| ActionLogEntry {
                        id: log.id,
                        action: log.action,
                        member_id: log.member_id,
                        details: log.details,
                        timestamp: log.timestamp.to_rfc3339(),
                    })
                    .collect(),
            }),
        )
            .into_response(),
        Err(err) => {
            error!(error = %err, "failed to list action logs");
            api_error_from_status(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
                .into_response()
        }
    }
}

pub(crate) async fn get_board_access(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c.clone(),
        Err(resp) => return resp.into_response(),
    };
    let (_user, ctx) = match ensure_user_ctx(&state, &claims).await {
        Ok(value) => value,
        Err(resp) => return resp.into_response(),
    };
    if let Err(resp) = ensure_admin(&ctx) {
        return resp.into_response();
    }
    match load_board_access(&state).await {
        Ok(entries) => (
            StatusCode::OK,
            Json(BoardAccessResponse {
                status: "ok".to_string(),
                entries: entries
                    .into_iter()
                    .map(|entry| btc_forum_shared::BoardAccessEntry {
                        id: entry.id,
                        name: entry.name,
                        allowed_groups: entry.allowed_groups,
                    })
                    .collect(),
            }),
        )
            .into_response(),
        Err(err) => {
            error!(error = %err, "failed to list board access");
            api_error_from_status(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
                .into_response()
        }
    }
}

pub(crate) async fn update_board_access(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<BoardAccessPayload>,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c.clone(),
        Err(resp) => return resp.into_response(),
    };
    let (_user, ctx) = match ensure_user_ctx(&state, &claims).await {
        Ok(value) => value,
        Err(resp) => return resp.into_response(),
    };
    if let Err(resp) = ensure_admin(&ctx) {
        return resp.into_response();
    }
    if let Err(resp) = verify_csrf(&headers) {
        return resp.into_response();
    }
    if payload.allowed_groups.len() > 1000 {
        return api_error_from_status(StatusCode::BAD_REQUEST, "too many groups").into_response();
    }
    let board_id = payload.board_id.clone();
    let allowed_groups = payload.allowed_groups.clone();
    match run_forum_blocking(&state, move |forum| {
        forum.set_board_access(&board_id, &allowed_groups)
    })
    .await
    {
        Ok(_) => (
            StatusCode::OK,
            Json(UpdateBoardAccessResponse {
                status: "ok".to_string(),
                board_id: payload.board_id,
                allowed_groups: payload.allowed_groups,
            }),
        )
            .into_response(),
        Err(err) => {
            error!(error = %err, "failed to update board access");
            api_error_from_status(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
                .into_response()
        }
    }
}

pub(crate) async fn get_board_permissions(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c.clone(),
        Err(resp) => return resp.into_response(),
    };
    let (_user, ctx) = match ensure_user_ctx(&state, &claims).await {
        Ok(value) => value,
        Err(resp) => return resp.into_response(),
    };
    if let Err(resp) = ensure_admin(&ctx) {
        return resp.into_response();
    }
    let mut response = match state
        .surreal
        .client()
        .query(
            r#"
            SELECT board_id, group_id, allow, deny
            FROM board_permissions;
            "#,
        )
        .await
    {
        Ok(resp) => resp,
        Err(err) => {
            error!(error = %err, "failed to list board permissions");
            return api_error_from_status(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
                .into_response();
        }
    };
    let entries: Vec<BoardPermissionEntry> = response.take(0).unwrap_or_default();
    (
        StatusCode::OK,
        Json(BoardPermissionResponse {
            status: "ok".to_string(),
            entries,
        }),
    )
        .into_response()
}

pub(crate) async fn update_board_permissions(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<BoardPermissionPayload>,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c.clone(),
        Err(resp) => return resp.into_response(),
    };
    let (_user, ctx) = match ensure_user_ctx(&state, &claims).await {
        Ok(value) => value,
        Err(resp) => return resp.into_response(),
    };
    if let Err(resp) = ensure_admin(&ctx) {
        return resp.into_response();
    }
    if let Err(resp) = verify_csrf(&headers) {
        return resp.into_response();
    }
    if payload.allow.len() + payload.deny.len() > 500 {
        return api_error_from_status(StatusCode::BAD_REQUEST, "too many permissions")
            .into_response();
    }

    let result = state
        .surreal
        .client()
        .query(
            r#"
            UPSERT type::thing("board_permissions", string::concat("bp:", $board_id, ":", $group_id)) SET
                board_id = $board_id,
                group_id = $group_id,
                allow = $allow,
                deny = $deny;
            "#,
        )
        .bind(("board_id", payload.board_id.clone()))
        .bind(("group_id", payload.group_id))
        .bind(("allow", payload.allow.clone()))
        .bind(("deny", payload.deny.clone()))
        .await;

    match result {
        Ok(_) => (
            StatusCode::OK,
            Json(UpdateBoardPermissionResponse {
                status: "ok".to_string(),
                board_id: payload.board_id,
                group_id: payload.group_id,
                allow: payload.allow,
                deny: payload.deny,
            }),
        )
            .into_response(),
        Err(err) => {
            error!(error = %err, "failed to update board permissions");
            api_error_from_status(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
                .into_response()
        }
    }
}
