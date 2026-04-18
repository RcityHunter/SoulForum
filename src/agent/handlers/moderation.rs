use std::collections::HashMap;

use axum::{
    extract::{rejection::JsonRejection, State},
    http::{Extensions, StatusCode},
    response::IntoResponse,
    Extension, Json,
};
use serde::{Deserialize, Serialize};
use tracing::{error, warn};

use btc_forum_rust::{
    auth::AuthClaims,
    services::{BanAffects, BanCondition, BanRule, ForumError, ForumService},
};
use btc_forum_shared::{
    ApiError, BanApplyResponse, BanListResponse, BanMemberView, BanRuleView, ErrorCode,
};
use serde_json::json;

use crate::agent::{
    auth::require_scope,
    request_id::RequestId,
    response::{err_response, ok_response},
};
use crate::api::{
    auth::ensure_user_ctx,
    guards::ensure_permission,
    state::{run_forum_blocking, AppState},
};

const BAN_LIST_SCOPE: &str = "forum:moderation:ban:read";
const BAN_APPLY_SCOPE: &str = "forum:moderation:ban:write";
const BAN_LIST_LEGACY_PERMISSIONS: &[&str] = &["manage_bans", "moderate_forum"];
const BAN_APPLY_LEGACY_PERMISSIONS: &[&str] = &["manage_bans"];
const MAX_BAN_HOURS: i64 = 24 * 365;

#[derive(Debug, Serialize)]
pub struct AgentBanListData {
    pub bans: Vec<BanRuleView>,
}

#[derive(Debug, Serialize)]
pub struct AgentBanApplyData {
    pub ban_id: i64,
    pub member_id: i64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentBanApplyPayload {
    pub member_id: Option<i64>,
    pub reason: Option<String>,
    pub hours: Option<i64>,
    #[serde(default)]
    pub cannot_post: bool,
    #[serde(default)]
    pub cannot_access: bool,
}

fn request_extensions(request_id: &RequestId) -> Extensions {
    let mut extensions = Extensions::new();
    extensions.insert(request_id.clone());
    extensions
}

fn json_error(error: JsonRejection) -> ApiError {
    ApiError {
        code: ErrorCode::Validation,
        message: "invalid JSON payload".to_string(),
        details: Some(json!({
            "reason": error.body_text(),
        })),
    }
}

fn forum_error(error: ForumError) -> (StatusCode, ApiError) {
    match error {
        ForumError::Validation(message) => (
            StatusCode::BAD_REQUEST,
            ApiError {
                code: ErrorCode::Validation,
                message,
                details: None,
            },
        ),
        ForumError::PermissionDenied(message) => (
            StatusCode::FORBIDDEN,
            ApiError {
                code: ErrorCode::Forbidden,
                message,
                details: None,
            },
        ),
        ForumError::SessionTimeout => (
            StatusCode::UNAUTHORIZED,
            ApiError {
                code: ErrorCode::Unauthorized,
                message: "session timeout".to_string(),
                details: None,
            },
        ),
        ForumError::Lang(message) | ForumError::Internal(message) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            ApiError {
                code: ErrorCode::Internal,
                message,
                details: None,
            },
        ),
    }
}

fn validate_apply_payload(payload: &AgentBanApplyPayload) -> Result<(i64, i64), ApiError> {
    let member_id = payload.member_id.unwrap_or(0);
    if member_id <= 0 {
        return Err(ApiError {
            code: ErrorCode::Validation,
            message: "member_id required".to_string(),
            details: None,
        });
    }

    let hours = payload.hours.unwrap_or(24);
    if !(1..=MAX_BAN_HOURS).contains(&hours) {
        return Err(ApiError {
            code: ErrorCode::Validation,
            message: format!("hours must be within 1..={MAX_BAN_HOURS}"),
            details: Some(json!({ "max_hours": MAX_BAN_HOURS })),
        });
    }

    Ok((member_id, hours))
}

fn normalize_reason(reason: Option<String>) -> Result<Option<String>, ApiError> {
    match reason.map(|value| value.trim().to_string()) {
        Some(value) if value.is_empty() => Err(ApiError {
            code: ErrorCode::Validation,
            message: "reason cannot be empty".to_string(),
            details: None,
        }),
        Some(value) if value.len() > 500 => Err(ApiError {
            code: ErrorCode::Validation,
            message: "reason too long (max 500 chars)".to_string(),
            details: None,
        }),
        Some(value) => Ok(Some(value)),
        None => Ok(None),
    }
}

fn build_rule(
    member_id: i64,
    hours: i64,
    reason: Option<String>,
    cannot_post: bool,
    cannot_access: bool,
) -> BanRule {
    let until = chrono::Utc::now()
        .checked_add_signed(chrono::Duration::hours(hours))
        .unwrap_or_else(chrono::Utc::now);

    BanRule {
        id: 0,
        reason: reason.clone(),
        expires_at: Some(until),
        cannot_post: cannot_post || cannot_access,
        cannot_access,
        conditions: vec![BanCondition {
            id: 0,
            reason,
            affects: BanAffects::Account { member_id },
            expires_at: Some(until),
        }],
    }
}

fn map_bans(response: BanListResponse) -> AgentBanListData {
    AgentBanListData {
        bans: response.bans,
    }
}

pub async fn list(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    Extension(request_id): Extension<RequestId>,
) -> impl IntoResponse {
    let request_extensions = request_extensions(&request_id);

    let claims = match require_scope(&claims, BAN_LIST_SCOPE, BAN_LIST_LEGACY_PERMISSIONS) {
        Ok(claims) => claims,
        Err((status, Json(error))) => {
            return err_response::<AgentBanListData>(status, &request_extensions, error)
        }
    };

    let (_user, ctx) = match ensure_user_ctx(&state, claims).await {
        Ok(value) => value,
        Err((status, Json(error))) => {
            return err_response::<AgentBanListData>(status, &request_extensions, error)
        }
    };

    if let Err((status, Json(error))) = ensure_permission(&state, &ctx, "manage_bans") {
        return err_response::<AgentBanListData>(status, &request_extensions, error);
    }

    match run_forum_blocking(&state, move |forum| {
        let bans = forum.list_ban_rules()?;
        let members = forum.list_members()?;
        Ok::<BanListResponse, ForumError>({
            let member_map: HashMap<i64, String> =
                members.into_iter().map(|m| (m.id, m.name)).collect();
            let mut view = Vec::new();
            for ban in bans {
                let mut member_ids: Vec<i64> = Vec::new();
                let mut emails = Vec::new();
                let mut ips = Vec::new();
                for cond in &ban.conditions {
                    match cond.affects {
                        BanAffects::Account { member_id } => member_ids.push(member_id),
                        BanAffects::Email { ref value } => emails.push(value.clone()),
                        BanAffects::Ip { ref value } => ips.push(value.clone()),
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
                    expires_at: ban.expires_at.as_ref().map(chrono::DateTime::to_rfc3339),
                    cannot_post: ban.cannot_post,
                    cannot_access: ban.cannot_access,
                    members,
                    emails,
                    ips,
                });
            }
            BanListResponse {
                status: "ok".to_string(),
                bans: view,
            }
        })
    })
    .await
    {
        Ok(response) => ok_response(StatusCode::OK, &request_extensions, map_bans(response)),
        Err(err) => {
            let (status, api_error) = forum_error(err);
            error!(
                error = %api_error.message,
                request_id = %request_id.0,
                actor_member_id = ctx.user_info.id,
                "agent v1 ban list failed"
            );
            err_response::<AgentBanListData>(status, &request_extensions, api_error)
        }
    }
}

pub async fn apply(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    Extension(request_id): Extension<RequestId>,
    payload: Result<Json<AgentBanApplyPayload>, JsonRejection>,
) -> impl IntoResponse {
    let request_extensions = request_extensions(&request_id);

    let claims = match require_scope(&claims, BAN_APPLY_SCOPE, BAN_APPLY_LEGACY_PERMISSIONS) {
        Ok(claims) => claims,
        Err((status, Json(error))) => {
            return err_response::<AgentBanApplyData>(status, &request_extensions, error)
        }
    };

    let payload = match payload {
        Ok(Json(payload)) => payload,
        Err(error) => {
            return err_response::<AgentBanApplyData>(
                StatusCode::BAD_REQUEST,
                &request_extensions,
                json_error(error),
            )
        }
    };

    let (member_id, hours) = match validate_apply_payload(&payload) {
        Ok(values) => values,
        Err(error) => {
            return err_response::<AgentBanApplyData>(
                StatusCode::BAD_REQUEST,
                &request_extensions,
                error,
            )
        }
    };

    let reason = match normalize_reason(payload.reason.clone()) {
        Ok(reason) => reason,
        Err(error) => {
            return err_response::<AgentBanApplyData>(
                StatusCode::BAD_REQUEST,
                &request_extensions,
                error,
            )
        }
    };

    let (_user, ctx) = match ensure_user_ctx(&state, claims).await {
        Ok(value) => value,
        Err((status, Json(error))) => {
            return err_response::<AgentBanApplyData>(status, &request_extensions, error)
        }
    };

    if let Err((status, Json(error))) = ensure_permission(&state, &ctx, "manage_bans") {
        return err_response::<AgentBanApplyData>(status, &request_extensions, error);
    }

    if !payload.cannot_post && !payload.cannot_access {
        return err_response::<AgentBanApplyData>(
            StatusCode::BAD_REQUEST,
            &request_extensions,
            ApiError {
                code: ErrorCode::Validation,
                message: "at least one ban effect must be enabled".to_string(),
                details: Some(json!({
                    "cannot_post": false,
                    "cannot_access": false,
                })),
            },
        );
    }

    let actor_member_id = ctx.user_info.id;
    let audit_reason = reason.clone();
    let request_id_value = request_id.0.clone();
    let cannot_post = payload.cannot_post;
    let cannot_access = payload.cannot_access;
    let rule = build_rule(member_id, hours, reason, cannot_post, cannot_access);

    match run_forum_blocking(&state, move |forum| {
        let ban_id = forum.save_ban_rule(rule)?;
        let audit = json!({
            "request_id": request_id_value,
            "ban_id": ban_id,
            "member_id": member_id,
            "hours": hours,
            "cannot_post": cannot_post || cannot_access,
            "cannot_access": cannot_access,
            "reason": audit_reason,
            "surface": "agent_v1",
            "capability": "moderation.ban.apply"
        });
        forum.log_action("agent_v1_ban_apply", Some(actor_member_id), &audit)?;
        Ok::<BanApplyResponse, ForumError>(BanApplyResponse {
            status: "ok".to_string(),
            ban_id,
            member_id,
        })
    })
    .await
    {
        Ok(response) => ok_response(
            StatusCode::CREATED,
            &request_extensions,
            AgentBanApplyData {
                ban_id: response.ban_id,
                member_id: response.member_id,
            },
        ),
        Err(err) => {
            let (status, api_error) = forum_error(err);
            warn!(
                request_id = %request_id.0,
                actor_member_id,
                target_member_id = member_id,
                hours,
                cannot_post,
                cannot_access,
                error = %api_error.message,
                "agent v1 ban apply failed"
            );
            err_response::<AgentBanApplyData>(status, &request_extensions, api_error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{normalize_reason, validate_apply_payload, AgentBanApplyPayload, MAX_BAN_HOURS};

    #[test]
    fn apply_payload_rejects_unknown_fields() {
        let payload = serde_json::from_value::<AgentBanApplyPayload>(serde_json::json!({
            "member_id": 7,
            "cannot_post": true,
            "unexpected": true
        }));

        assert!(payload.is_err());
    }

    #[test]
    fn validate_apply_payload_requires_member_id() {
        let err = validate_apply_payload(&AgentBanApplyPayload {
            member_id: None,
            reason: None,
            hours: Some(24),
            cannot_post: true,
            cannot_access: false,
        })
        .unwrap_err();

        assert_eq!(err.message, "member_id required");
    }

    #[test]
    fn validate_apply_payload_rejects_hours_out_of_range() {
        let err = validate_apply_payload(&AgentBanApplyPayload {
            member_id: Some(9),
            reason: None,
            hours: Some(MAX_BAN_HOURS + 1),
            cannot_post: true,
            cannot_access: false,
        })
        .unwrap_err();

        assert!(err.message.contains("hours must be within"));
    }

    #[test]
    fn normalize_reason_trims_and_preserves_meaningful_text() {
        let reason = normalize_reason(Some("  spam wave  ".to_string())).unwrap();
        assert_eq!(reason.as_deref(), Some("spam wave"));
    }

    #[test]
    fn normalize_reason_rejects_blank_reason() {
        let err = normalize_reason(Some("   ".to_string())).unwrap_err();
        assert_eq!(err.message, "reason cannot be empty");
    }
}
