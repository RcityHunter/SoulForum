use axum::{
    extract::{ConnectInfo, Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use serde_json::json;
use std::{
    collections::HashMap,
    hash::{DefaultHasher, Hash, Hasher},
    net::SocketAddr,
    time::Duration,
};
use surrealdb_types::SurrealValue;
use tracing::{error, warn};

use btc_forum_rust::{
    auth::AuthClaims,
    services::{BanAffects, BanCondition, BanRule, ForumService, SendPersonalMessage},
    surreal::SurrealUser,
};
use btc_forum_shared::{
    ActionLogEntry, ActionLogsResponse, AdminAccount, AdminAccountsResponse, AdminGroup,
    AdminGroupsResponse, AdminNotifyPayload, AdminNotifyResponse, AdminTransferPayload,
    AdminTransferResponse, AdminUser, AdminUsersResponse, BanApplyResponse, BanListResponse,
    BanMemberView, BanPayload, BanRevokeResponse, BanRuleView, BoardAccessPayload,
    BoardAccessResponse, BoardPermissionEntry, BoardPermissionPayload, BoardPermissionResponse,
    DocsPermissionGrantByRecordPayload, DocsPermissionGrantResponse,
    DocsPermissionRevokeByRecordPayload, DocsPermissionRevokeResponse,
    ModeratorUpdateByRecordPayload, ModeratorUpdateResponse, UpdateBoardAccessResponse,
    UpdateBoardPermissionResponse,
};

use super::{
    auth::{ensure_user_ctx, require_auth},
    error::{api_error_from_status, rainbow_auth_error_response},
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

type ApiErr = (StatusCode, Json<btc_forum_shared::ApiError>);

#[derive(Debug, Clone, SurrealValue)]
struct UserAdminRow {
    role: Option<String>,
    permissions: Option<Vec<String>>,
    primary_group: Option<i64>,
    additional_groups: Option<Vec<i64>>,
}

#[derive(Debug, Clone, SurrealValue)]
struct UserListRow {
    id: Option<String>,
    name: String,
    primary_group: Option<i64>,
    additional_groups: Option<Vec<i64>>,
    warning: Option<i32>,
}

#[derive(Debug, Clone, SurrealValue)]
struct UserIdentityRow {
    id: Option<String>,
    name: String,
}

fn legacy_id_from_record(record_id: Option<&str>, name: &str) -> i64 {
    if let Some(id) = record_id
        .and_then(|rid| rid.split(':').next_back())
        .and_then(|s| s.parse::<i64>().ok())
    {
        if id != 0 {
            return id;
        }
    }
    let mut hasher = DefaultHasher::new();
    name.hash(&mut hasher);
    let hashed = (hasher.finish() & 0x7FFF_FFFF_FFFF_FFFF) as i64;
    if hashed == 0 { 1 } else { hashed }
}

fn normalize_user_record_id(raw: &str) -> String {
    let trimmed = raw.trim().trim_matches('⟨').trim_matches('⟩');
    if trimmed.contains(':') {
        trimmed.to_string()
    } else {
        format!("users:{trimmed}")
    }
}

fn user_key_from_record_id(raw: &str) -> String {
    let rid = normalize_user_record_id(raw);
    rid.split_once(':')
        .map(|(_, key)| key.trim().trim_matches('`').trim_matches('⟨').trim_matches('⟩').to_string())
        .unwrap_or_else(|| rid.trim().trim_matches('`').trim_matches('⟨').trim_matches('⟩').to_string())
}

fn auth_user_id_from_record_id(record_id: &str) -> Option<String> {
    record_id
        .split(':')
        .next_back()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(|v| v.to_string())
}

fn dedup_groups(mut groups: Vec<i64>) -> Vec<i64> {
    groups.sort_unstable();
    groups.dedup();
    groups
}

async fn resolve_member_name_by_id(state: &AppState, member_id: i64) -> Result<String, ApiErr> {
    let members = run_forum_blocking(state, move |forum| forum.list_members())
        .await
        .map_err(|err| {
            error!(error = %err, "failed to list members for admin action");
            api_error_from_status(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
        })?;
    members
        .into_iter()
        .find(|m| m.id == member_id)
        .map(|m| m.name)
        .ok_or_else(|| api_error_from_status(StatusCode::NOT_FOUND, "member not found"))
}

async fn resolve_user_identity_by_legacy_id(
    state: &AppState,
    legacy_id: i64,
) -> Result<(String, String), ApiErr> {
    let mut response = state
        .surreal
        .client()
        .query(
            r#"
            SELECT type::string(id) as id, name
            FROM users;
            "#,
        )
        .await
        .map_err(|err| {
            error!(error = %err, "failed to query users for legacy id resolution");
            api_error_from_status(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
        })?;
    let rows: Vec<UserIdentityRow> = response.take(0).map_err(|err| {
        error!(error = %err, "failed to parse users for legacy id resolution");
        api_error_from_status(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
    })?;

    rows.into_iter()
        .find_map(|row| {
            let resolved = legacy_id_from_record(row.id.as_deref(), &row.name);
            if resolved == legacy_id {
                Some((row.id.unwrap_or_default(), row.name))
            } else {
                None
            }
        })
        .ok_or_else(|| api_error_from_status(StatusCode::NOT_FOUND, "member not found"))
}

async fn load_user_admin_row_by_name(state: &AppState, name: &str) -> Result<UserAdminRow, ApiErr> {
    let mut response = state
        .surreal
        .client()
        .query(
            r#"
            SELECT role, permissions, primary_group, additional_groups
            FROM users
            WHERE name = $name
            LIMIT 1;
            "#,
        )
        .bind(("name", name.to_string()))
        .await
        .map_err(|err| {
            error!(error = %err, "failed to query user admin row");
            api_error_from_status(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
        })?;
    let mut rows: Vec<UserAdminRow> = response.take(0).map_err(|err| {
        error!(error = %err, "failed to parse user admin row");
        api_error_from_status(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
    })?;
    rows.pop()
        .ok_or_else(|| api_error_from_status(StatusCode::NOT_FOUND, "member not found"))
}

async fn load_user_admin_row_by_record_id(
    state: &AppState,
    record_id: &str,
) -> Result<(String, String, UserAdminRow), ApiErr> {
    #[derive(Debug, Clone, SurrealValue)]
    struct UserByRecordRow {
        id: Option<String>,
        name: String,
        role: Option<String>,
        permissions: Option<Vec<String>>,
        primary_group: Option<i64>,
        additional_groups: Option<Vec<i64>>,
    }
    let rid = normalize_user_record_id(record_id);
    let rid_key = user_key_from_record_id(record_id);
    let query_user = || {
        state
            .surreal
            .client()
            .query(
                r#"
                SELECT meta::id(id) as id, name, role, permissions, primary_group, additional_groups
                FROM users
                WHERE id = type::record('users', $rid_key)
                LIMIT 1;
                "#,
            )
            .bind(("rid_key", rid_key.clone()))
    };

    let mut response = match query_user().await {
        Ok(resp) => resp,
        Err(err) => {
            let msg = err.to_string();
            if msg.contains("401") || msg.contains("Unauthorized") {
                warn!(error = %err, "query user by record_id unauthorized, trying reauth");
                let _ = btc_forum_rust::surreal::reauth_from_env(state.surreal.client()).await;
                query_user().await.map_err(|retry_err| {
                    error!(error = %retry_err, "failed to query user by record_id after reauth");
                    api_error_from_status(StatusCode::INTERNAL_SERVER_ERROR, retry_err.to_string())
                })?
            } else {
                error!(error = %err, "failed to query user by record_id");
                return Err(api_error_from_status(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    err.to_string(),
                ));
            }
        }
    };
    let mut rows: Vec<UserByRecordRow> = response.take(0).map_err(|err| {
        error!(error = %err, "failed to parse user row by record_id");
        api_error_from_status(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
    })?;
    let row = rows
        .pop()
        .ok_or_else(|| api_error_from_status(StatusCode::NOT_FOUND, "member not found"))?;
    Ok((
        row.id.unwrap_or(rid),
        row.name.clone(),
        UserAdminRow {
            role: row.role,
            permissions: row.permissions,
            primary_group: row.primary_group,
            additional_groups: row.additional_groups,
        },
    ))
}

async fn persist_user_admin_row_by_name(
    state: &AppState,
    name: &str,
    role: &str,
    permissions: Vec<String>,
    primary_group: Option<i64>,
    additional_groups: Vec<i64>,
) -> Result<(), ApiErr> {
    let full_update = || {
        state
            .surreal
            .client()
            .query(
                r#"
                UPDATE users
                SET
                    role = $role,
                    permissions = $permissions,
                    primary_group = $primary_group,
                    additional_groups = $additional_groups
                WHERE name = $name
                RETURN NONE;
                "#,
            )
            .bind(("name", name.to_string()))
            .bind(("role", role.to_string()))
            .bind(("permissions", permissions.clone()))
            .bind(("primary_group", primary_group))
            .bind(("additional_groups", additional_groups.clone()))
    };

    match full_update().await {
        Ok(_) => Ok(()),
        Err(err) => {
            let msg = err.to_string();
            let missing_group_fields = msg.contains("no such field exists for table 'users'")
                || msg.contains("primary_group")
                || msg.contains("additional_groups");
            if !missing_group_fields {
                error!(error = %err, "failed to update user admin row");
                return Err(api_error_from_status(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    msg,
                ));
            }

            warn!(
                error = %err,
                "users table lacks group fields, fallback to role/permissions-only update"
            );
            state
                .surreal
                .client()
                .query(
                    r#"
                    UPDATE users
                    SET
                        role = $role,
                        permissions = $permissions
                    WHERE name = $name
                    RETURN NONE;
                    "#,
                )
                .bind(("name", name.to_string()))
                .bind(("role", role.to_string()))
                .bind(("permissions", permissions))
                .await
                .map_err(|fallback_err| {
                    error!(error = %fallback_err, "failed to update user admin row (fallback)");
                    api_error_from_status(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        fallback_err.to_string(),
                    )
                })?;
            Ok(())
        }
    }
}

async fn persist_user_admin_row_by_record_id(
    state: &AppState,
    record_id: &str,
    role: &str,
    permissions: Vec<String>,
    primary_group: Option<i64>,
    additional_groups: Vec<i64>,
) -> Result<(), ApiErr> {
    let rid = normalize_user_record_id(record_id);
    let rid_key = user_key_from_record_id(record_id);

    let full_update = || {
        state
            .surreal
            .client()
            .query(
                r#"
                UPDATE users
                SET
                    role = $role,
                    permissions = $permissions,
                    primary_group = $primary_group,
                    additional_groups = $additional_groups
                WHERE id = type::record('users', $rid_key)
                RETURN NONE;
                "#,
            )
            .bind(("role", role.to_string()))
            .bind(("permissions", permissions.clone()))
            .bind(("primary_group", primary_group))
            .bind(("additional_groups", additional_groups.clone()))
            .bind(("rid_key", rid_key.clone()))
    };

    match full_update().await {
        Ok(_) => Ok(()),
        Err(err) => {
            let msg = err.to_string();
            let missing_group_fields = msg.contains("no such field exists for table 'users'")
                || msg.contains("primary_group")
                || msg.contains("additional_groups");
            if !missing_group_fields {
                error!(error = %err, "failed to update user admin row by record_id");
                return Err(api_error_from_status(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    msg,
                ));
            }

            warn!(
                error = %err,
                "users table lacks group fields, fallback to role/permissions-only update by record_id"
            );
            state
                .surreal
                .client()
                .query(
                    r#"
                    UPDATE users
                    SET
                        role = $role,
                        permissions = $permissions
                    WHERE id = type::record('users', $rid_key)
                    RETURN NONE;
                    "#,
                )
                .bind(("role", role.to_string()))
                .bind(("permissions", permissions))
                .bind(("rid_key", rid_key))
                .await
                .map_err(|fallback_err| {
                    error!(
                        error = %fallback_err,
                        "failed to update user admin row by record_id (fallback)"
                    );
                    api_error_from_status(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        fallback_err.to_string(),
                    )
                })?;
            Ok(())
        }
    }
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
    let query_users = || {
        state.surreal.client().query(
            r#"
            SELECT type::string(id) as id, name, primary_group, additional_groups, warning, type::string(created_at) as created_at
            FROM users
            ORDER BY created_at DESC;
            "#,
        )
    };
    let mut response = match query_users().await {
        Ok(resp) => resp,
        Err(err) => {
            let msg = err.to_string();
            if msg.contains("401") || msg.contains("Unauthorized") {
                warn!(error = %err, "list_users unauthorized, trying reauth");
                let _ = btc_forum_rust::surreal::reauth_from_env(state.surreal.client()).await;
                match query_users().await {
                    Ok(resp) => resp,
                    Err(retry_err) => {
                        error!(error = %retry_err, "failed to list users after reauth");
                        return api_error_from_status(StatusCode::INTERNAL_SERVER_ERROR, retry_err.to_string())
                            .into_response();
                    }
                }
            } else {
                error!(error = %err, "failed to list users");
                return api_error_from_status(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
                    .into_response();
            }
        }
    };
    let rows: Vec<UserListRow> = match response.take(0) {
        Ok(value) => value,
        Err(err) => {
            error!(error = %err, "failed to parse user list rows");
            return api_error_from_status(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
                .into_response();
        }
    };
    let q = params.q.as_deref().map(|s| s.to_lowercase());
    let filtered: Vec<AdminUser> = rows
        .into_iter()
        .filter(|r| {
            if let Some(ref q) = q {
                r.name.to_lowercase().contains(q)
            } else {
                true
            }
        })
        .take(params.limit.unwrap_or(200))
        .map(|r| {
            let record_id = r.id.clone();
            let auth_user_id = r
                .id
                .as_deref()
                .and_then(|rid| rid.split(':').next_back())
                .map(|s| s.to_string());
            AdminUser {
                id: legacy_id_from_record(record_id.as_deref(), &r.name),
                record_id,
                auth_user_id,
                name: r.name,
                primary_group: r.primary_group,
                additional_groups: r.additional_groups.unwrap_or_default(),
                warning: r.warning.unwrap_or(0),
            }
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

    let query_admins = || {
        state.surreal.client().query(
            r#"
            SELECT type::string(id) as id, name, role, permissions, password_hash, type::string(created_at) as created_at
            FROM users
            WHERE role = 'admin' OR permissions CONTAINS 'manage_boards'
            ORDER BY created_at ASC;
            "#,
        )
    };
    let mut response = match query_admins().await {
        Ok(resp) => resp,
        Err(err) => {
            let msg = err.to_string();
            if msg.contains("401") || msg.contains("Unauthorized") {
                warn!(error = %err, "list_admins unauthorized, trying reauth");
                let _ = btc_forum_rust::surreal::reauth_from_env(state.surreal.client()).await;
                match query_admins().await {
                    Ok(resp) => resp,
                    Err(retry_err) => {
                        error!(error = %retry_err, "failed to list admin users after reauth");
                        return api_error_from_status(StatusCode::INTERNAL_SERVER_ERROR, retry_err.to_string())
                            .into_response();
                    }
                }
            } else {
                error!(error = %err, "failed to list admin users");
                return api_error_from_status(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
                    .into_response();
            }
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
            record_id: user.id.clone(),
            auth_user_id: user
                .id
                .as_deref()
                .and_then(|rid| rid.split(':').next_back())
                .map(|s| s.to_string()),
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
                    cannot_post: ban.cannot_post,
                    cannot_access: ban.cannot_access,
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
        cannot_post: payload.cannot_post || payload.cannot_access || (!payload.cannot_post && !payload.cannot_access),
        cannot_access: payload.cannot_access,
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

pub(crate) async fn assign_moderator(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    headers: axum::http::HeaderMap,
    Path(member_id): Path<i64>,
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
    if member_id <= 0 {
        return api_error_from_status(StatusCode::BAD_REQUEST, "invalid member_id").into_response();
    }

    let target_name = match resolve_member_name_by_id(&state, member_id).await {
        Ok(name) => name,
        Err(resp) => return resp.into_response(),
    };
    let row = match load_user_admin_row_by_name(&state, &target_name).await {
        Ok(row) => row,
        Err(resp) => return resp.into_response(),
    };

    let role = if row.role.as_deref() == Some("admin") {
        "admin".to_string()
    } else {
        "mod".to_string()
    };
    let mut permissions = row.permissions.unwrap_or_default();
    if !permissions.iter().any(|p| p == "moderate_forum") {
        permissions.push("moderate_forum".to_string());
    }
    let primary_group = row.primary_group;
    let mut additional_groups = dedup_groups(row.additional_groups.unwrap_or_default());
    if primary_group != Some(2) && !additional_groups.iter().any(|g| *g == 2) {
        additional_groups.push(2);
    }
    additional_groups = dedup_groups(additional_groups);

    if let Err(resp) = persist_user_admin_row_by_name(
        &state,
        &target_name,
        &role,
        permissions,
        primary_group,
        additional_groups.clone(),
    )
    .await
    {
        return resp.into_response();
    }

    (
        StatusCode::OK,
        Json(ModeratorUpdateResponse {
            status: "ok".to_string(),
            member_id,
            record_id: None,
            role,
            primary_group,
            additional_groups,
        }),
    )
        .into_response()
}

pub(crate) async fn revoke_moderator(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    headers: axum::http::HeaderMap,
    Path(member_id): Path<i64>,
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
    if member_id <= 0 {
        return api_error_from_status(StatusCode::BAD_REQUEST, "invalid member_id").into_response();
    }

    let target_name = match resolve_member_name_by_id(&state, member_id).await {
        Ok(name) => name,
        Err(resp) => return resp.into_response(),
    };
    let row = match load_user_admin_row_by_name(&state, &target_name).await {
        Ok(row) => row,
        Err(resp) => return resp.into_response(),
    };

    let role = if row.role.as_deref() == Some("admin") {
        "admin".to_string()
    } else {
        "member".to_string()
    };
    let permissions = row
        .permissions
        .unwrap_or_default()
        .into_iter()
        .filter(|p| p != "moderate_forum")
        .collect::<Vec<_>>();
    let primary_group = if row.primary_group == Some(2) {
        Some(4)
    } else {
        row.primary_group
    };
    let mut additional_groups = row.additional_groups.unwrap_or_default();
    additional_groups.retain(|g| *g != 2);
    additional_groups = dedup_groups(additional_groups);

    if let Err(resp) = persist_user_admin_row_by_name(
        &state,
        &target_name,
        &role,
        permissions,
        primary_group,
        additional_groups.clone(),
    )
    .await
    {
        return resp.into_response();
    }

    (
        StatusCode::OK,
        Json(ModeratorUpdateResponse {
            status: "ok".to_string(),
            member_id,
            record_id: None,
            role,
            primary_group,
            additional_groups,
        }),
    )
        .into_response()
}

pub(crate) async fn transfer_admin(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<AdminTransferPayload>,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c.clone(),
        Err(resp) => return resp.into_response(),
    };
    let (current_user, ctx) = match ensure_user_ctx(&state, &claims).await {
        Ok(value) => value,
        Err(resp) => return resp.into_response(),
    };
    if let Err(resp) = ensure_admin(&ctx) {
        return resp.into_response();
    }
    if let Err(resp) = verify_csrf(&headers) {
        return resp.into_response();
    }
    let (target_member_id, _target_record_id, target_name, _target_row) = if let Some(target_record_id) =
        payload.target_record_id.as_deref()
    {
        let (resolved_record_id, name, row) =
            match load_user_admin_row_by_record_id(&state, target_record_id).await {
                Ok(v) => v,
                Err(resp) => {
                    // Backward-compat: allow passing legacy numeric member id in target_record_id.
                    if resp.0 == StatusCode::NOT_FOUND {
                        if let Ok(legacy_id) = target_record_id.trim().parse::<i64>() {
                            let (rid, resolved_name) =
                                match resolve_user_identity_by_legacy_id(&state, legacy_id).await {
                                    Ok(v) => v,
                                    Err(resp2) => {
                                        error!(
                                            status = %resp2.0,
                                            code = ?resp2.1.0.code,
                                            message = %resp2.1.0.message,
                                            target_record_id = %target_record_id,
                                            legacy_id,
                                            "transfer_admin: fallback resolve by legacy id failed"
                                        );
                                        return resp2.into_response();
                                    }
                                };
                            let resolved_row =
                                match load_user_admin_row_by_name(&state, &resolved_name).await {
                                    Ok(v) => v,
                                    Err(resp2) => {
                                        error!(
                                            status = %resp2.0,
                                            code = ?resp2.1.0.code,
                                            message = %resp2.1.0.message,
                                            target_record_id = %target_record_id,
                                            legacy_id,
                                            resolved_name = %resolved_name,
                                            "transfer_admin: fallback load target row failed"
                                        );
                                        return resp2.into_response();
                                    }
                                };
                            (rid, resolved_name, resolved_row)
                        } else {
                            error!(
                                status = %resp.0,
                                code = ?resp.1.0.code,
                                message = %resp.1.0.message,
                                target_record_id = %target_record_id,
                                "transfer_admin: failed to load target by record id"
                            );
                            return resp.into_response();
                        }
                    } else {
                        error!(
                            status = %resp.0,
                            code = ?resp.1.0.code,
                            message = %resp.1.0.message,
                            target_record_id = %target_record_id,
                            "transfer_admin: failed to load target by record id"
                        );
                        return resp.into_response();
                    }
                }
            };
        let legacy_id = legacy_id_from_record(Some(&resolved_record_id), &name);
        (legacy_id, resolved_record_id, name, row)
    } else {
        let Some(target_member_id) = payload.target_member_id else {
            return api_error_from_status(
                StatusCode::BAD_REQUEST,
                "target_member_id or target_record_id required",
            )
            .into_response();
        };
        if target_member_id <= 0 {
            return api_error_from_status(StatusCode::BAD_REQUEST, "invalid target_member_id")
                .into_response();
        }
        let (resolved_record_id, target_name) =
            match resolve_user_identity_by_legacy_id(&state, target_member_id).await {
                Ok(v) => v,
                Err(resp) => {
                    error!(
                        status = %resp.0,
                        code = ?resp.1.0.code,
                        message = %resp.1.0.message,
                        target_member_id,
                        "transfer_admin: failed to resolve target by legacy id"
                    );
                    return resp.into_response();
                }
            };
        let target_row = match load_user_admin_row_by_name(&state, &target_name).await {
            Ok(row) => row,
            Err(resp) => {
                error!(
                    status = %resp.0,
                    code = ?resp.1.0.code,
                    message = %resp.1.0.message,
                    target_name = %target_name,
                    "transfer_admin: failed to load target row by name"
                );
                return resp.into_response();
            }
        };
        let resolved_target_member_id = legacy_id_from_record(Some(&resolved_record_id), &target_name);
        (
            resolved_target_member_id,
            resolved_record_id,
            target_name,
            target_row,
        )
    };
    if target_member_id == ctx.user_info.id {
        return api_error_from_status(
            StatusCode::BAD_REQUEST,
            "target member cannot be current admin",
        )
        .into_response();
    }
    // Hard-switch mode: keep transfer robust across schema drift and id format variants.
    if let Err(err) = state
        .surreal
        .client()
        .query(
            r#"
            UPDATE users
            SET role = 'admin', permissions = ['manage_boards']
            WHERE name = $name
            RETURN NONE;
            "#,
        )
        .bind(("name", target_name.clone()))
        .await
    {
        error!(error = %err, target_name = %target_name, "transfer_admin: failed to promote target");
        return api_error_from_status(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
            .into_response();
    }

    // Always demote the transfer initiator to avoid dual-admin residue from stale clients.
    let source_name = current_user.name.clone();
    if let Err(err) = state
        .surreal
        .client()
        .query(
            r#"
            UPDATE users
            SET role = 'member', permissions = []
            WHERE name = $name
            RETURN NONE;
            "#,
        )
        .bind(("name", source_name.clone()))
        .await
    {
        error!(error = %err, source_name = %source_name, "transfer_admin: failed to demote source");
        return api_error_from_status(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
            .into_response();
    }

    (
        StatusCode::OK,
        Json(AdminTransferResponse {
            status: "ok".to_string(),
            from_member_id: ctx.user_info.id,
            to_member_id: target_member_id,
        }),
    )
        .into_response()
}

pub(crate) async fn assign_moderator_by_record(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<ModeratorUpdateByRecordPayload>,
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
    let (record_id, target_name, row) = match load_user_admin_row_by_record_id(&state, &payload.record_id).await {
        Ok(v) => v,
        Err(resp) => return resp.into_response(),
    };
    let role = if row.role.as_deref() == Some("admin") {
        "admin".to_string()
    } else {
        "mod".to_string()
    };
    let mut permissions = row.permissions.unwrap_or_default();
    if !permissions.iter().any(|p| p == "moderate_forum") {
        permissions.push("moderate_forum".to_string());
    }
    let primary_group = row.primary_group;
    let mut additional_groups = dedup_groups(row.additional_groups.unwrap_or_default());
    if primary_group != Some(2) && !additional_groups.iter().any(|g| *g == 2) {
        additional_groups.push(2);
    }
    additional_groups = dedup_groups(additional_groups);
    if let Err(resp) = persist_user_admin_row_by_name(
        &state,
        &target_name,
        &role,
        permissions,
        primary_group,
        additional_groups.clone(),
    )
    .await
    {
        return resp.into_response();
    }
    let member_id = legacy_id_from_record(Some(&record_id), &target_name);
    (
        StatusCode::OK,
        Json(ModeratorUpdateResponse {
            status: "ok".to_string(),
            member_id,
            record_id: Some(record_id),
            role,
            primary_group,
            additional_groups,
        }),
    )
        .into_response()
}

pub(crate) async fn grant_docs_space_create_by_record(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<DocsPermissionGrantByRecordPayload>,
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

    let admin_token = if let Some(token) = claims.token.clone() {
        token
    } else {
        return api_error_from_status(StatusCode::UNAUTHORIZED, "missing bearer token")
            .into_response();
    };

    let (record_id, _target_name, _row) =
        match load_user_admin_row_by_record_id(&state, &payload.record_id).await {
            Ok(v) => v,
            Err(resp) => return resp.into_response(),
        };
    let auth_user_id = if let Some(value) = auth_user_id_from_record_id(&record_id) {
        value
    } else {
        return api_error_from_status(StatusCode::BAD_REQUEST, "invalid record_id")
            .into_response();
    };

    let role_name = "docs_admin".to_string();
    let perms = match state
        .rainbow_auth
        .get_user_permissions(&admin_token, &auth_user_id)
        .await
    {
        Ok(value) => value,
        Err(err) => {
            return rainbow_auth_error_response(err).into_response();
        }
    };

    let already_granted = perms.iter().any(|p| p == "spaces.write" || p == "docs.admin");
    if !already_granted {
        if let Err(err) = state
            .rainbow_auth
            .assign_role_to_user(&admin_token, &auth_user_id, &role_name)
            .await
        {
            return rainbow_auth_error_response(err).into_response();
        }
    }

    (
        StatusCode::OK,
        Json(DocsPermissionGrantResponse {
            status: "ok".to_string(),
            record_id,
            auth_user_id,
            granted_role: role_name,
            already_granted,
        }),
    )
        .into_response()
}

pub(crate) async fn revoke_docs_space_create_by_record(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<DocsPermissionRevokeByRecordPayload>,
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

    let admin_token = if let Some(token) = claims.token.clone() {
        token
    } else {
        return api_error_from_status(StatusCode::UNAUTHORIZED, "missing bearer token")
            .into_response();
    };

    let (record_id, _target_name, _row) =
        match load_user_admin_row_by_record_id(&state, &payload.record_id).await {
            Ok(v) => v,
            Err(resp) => return resp.into_response(),
        };
    let auth_user_id = if let Some(value) = auth_user_id_from_record_id(&record_id) {
        value
    } else {
        return api_error_from_status(StatusCode::BAD_REQUEST, "invalid record_id")
            .into_response();
    };

    let role_name = "docs_admin".to_string();
    let perms = match state
        .rainbow_auth
        .get_user_permissions(&admin_token, &auth_user_id)
        .await
    {
        Ok(value) => value,
        Err(err) => {
            return rainbow_auth_error_response(err).into_response();
        }
    };

    let has_docs_create = perms.iter().any(|p| p == "spaces.write" || p == "docs.admin");
    if has_docs_create {
        if let Err(err) = state
            .rainbow_auth
            .remove_role_from_user(&admin_token, &auth_user_id, &role_name)
            .await
        {
            return rainbow_auth_error_response(err).into_response();
        }
    }

    (
        StatusCode::OK,
        Json(DocsPermissionRevokeResponse {
            status: "ok".to_string(),
            record_id,
            auth_user_id,
            revoked_role: role_name,
            already_revoked: !has_docs_create,
        }),
    )
        .into_response()
}

pub(crate) async fn revoke_moderator_by_record(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<ModeratorUpdateByRecordPayload>,
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
    let (record_id, target_name, row) = match load_user_admin_row_by_record_id(&state, &payload.record_id).await {
        Ok(v) => v,
        Err(resp) => return resp.into_response(),
    };
    let role = if row.role.as_deref() == Some("admin") {
        "admin".to_string()
    } else {
        "member".to_string()
    };
    let permissions = row
        .permissions
        .unwrap_or_default()
        .into_iter()
        .filter(|p| p != "moderate_forum")
        .collect::<Vec<_>>();
    let primary_group = if row.primary_group == Some(2) {
        Some(4)
    } else {
        row.primary_group
    };
    let mut additional_groups = row.additional_groups.unwrap_or_default();
    additional_groups.retain(|g| *g != 2);
    additional_groups = dedup_groups(additional_groups);
    if let Err(resp) = persist_user_admin_row_by_name(
        &state,
        &target_name,
        &role,
        permissions,
        primary_group,
        additional_groups.clone(),
    )
    .await
    {
        return resp.into_response();
    }
    let member_id = legacy_id_from_record(Some(&record_id), &target_name);
    (
        StatusCode::OK,
        Json(ModeratorUpdateResponse {
            status: "ok".to_string(),
            member_id,
            record_id: Some(record_id),
            role,
            primary_group,
            additional_groups,
        }),
    )
        .into_response()
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
    let query_board_permissions = || {
        state.surreal.client().query(
            r#"
            SELECT board_id, group_id, allow, deny
            FROM board_permissions;
            "#,
        )
    };
    let mut response = match query_board_permissions().await {
        Ok(resp) => resp,
        Err(err) => {
            let msg = err.to_string();
            if msg.contains("401") || msg.contains("Unauthorized") {
                warn!(error = %err, "get_board_permissions unauthorized, trying reauth");
                let _ = btc_forum_rust::surreal::reauth_from_env(state.surreal.client()).await;
                match query_board_permissions().await {
                    Ok(resp) => resp,
                    Err(retry_err) => {
                        error!(error = %retry_err, "failed to list board permissions after reauth");
                        return api_error_from_status(StatusCode::INTERNAL_SERVER_ERROR, retry_err.to_string())
                            .into_response();
                    }
                }
            } else {
                error!(error = %err, "failed to list board permissions");
                return api_error_from_status(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
                    .into_response();
            }
        }
    };
    #[derive(Debug, Clone, SurrealValue)]
    struct BoardPermissionRow {
        board_id: String,
        group_id: i64,
        allow: Vec<String>,
        deny: Vec<String>,
    }
    let rows: Vec<BoardPermissionRow> = response.take(0).unwrap_or_default();
    let entries: Vec<BoardPermissionEntry> = rows
        .into_iter()
        .map(|r| BoardPermissionEntry {
            board_id: r.board_id,
            group_id: r.group_id,
            allow: r.allow,
            deny: r.deny,
        })
        .collect();
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
