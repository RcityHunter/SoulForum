use axum::{
    extract::{rejection::JsonRejection, State},
    http::{Extensions, StatusCode},
    response::IntoResponse,
    Extension, Json,
};
use serde::{Deserialize, Serialize};
use tracing::error;

use btc_forum_rust::{
    auth::AuthClaims,
    pm_ops::{self, PmSendLog, RecipientFailure, RecipientInput},
    services::ForumError,
};
use btc_forum_shared::{ApiError, ErrorCode};

use crate::agent::{
    auth::require_scope,
    request_id::RequestId,
    response::{err_response, ok_response},
};
use crate::api::{
    auth::ensure_user_ctx,
    guards::ensure_permission,
    state::{run_forum_blocking, AppState},
    utils::sanitize_input,
};

const PM_WRITE_SCOPE: &str = "forum:pm:write";
const PM_WRITE_LEGACY_PERMISSIONS: &[&str] = &["manage_boards", "pm_send"];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PmSendPayload {
    to: Vec<String>,
    #[serde(default)]
    bcc: Vec<String>,
    subject: String,
    body: String,
}

#[derive(Debug, Serialize)]
pub struct PmSendFailureData {
    pub target: String,
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct PmSendData {
    pub message_id: Option<i64>,
    pub sent_to: Vec<i64>,
    pub failed: Vec<PmSendFailureData>,
}

fn request_extensions(request_id: &RequestId) -> Extensions {
    let mut extensions = Extensions::new();
    extensions.insert(request_id.clone());
    extensions
}

fn build_recipient_input(payload: &PmSendPayload) -> RecipientInput {
    RecipientInput {
        to: payload.to.clone(),
        bcc: payload.bcc.clone(),
    }
}

fn map_failed_recipient(entry: RecipientFailure) -> PmSendFailureData {
    PmSendFailureData {
        target: entry.target,
        reason: entry.reason,
    }
}

fn map_send_log(log: PmSendLog) -> PmSendData {
    PmSendData {
        message_id: log.message_id,
        sent_to: log.sent,
        failed: log.failed.into_iter().map(map_failed_recipient).collect(),
    }
}

fn validate_payload(payload: &PmSendPayload) -> Result<(), ApiError> {
    if payload.to.is_empty() && payload.bcc.is_empty() {
        return Err(ApiError {
            code: ErrorCode::Validation,
            message: "recipient required".to_string(),
            details: None,
        });
    }

    let subject = payload.subject.trim();
    if subject.is_empty() || subject.len() > 200 {
        return Err(ApiError {
            code: ErrorCode::Validation,
            message: "subject must be 1..200 chars".to_string(),
            details: None,
        });
    }

    let body = payload.body.trim();
    if body.is_empty() || body.len() > 4000 {
        return Err(ApiError {
            code: ErrorCode::Validation,
            message: "body must be 1..4000 chars".to_string(),
            details: None,
        });
    }

    Ok(())
}

fn json_error(error: JsonRejection) -> ApiError {
    ApiError {
        code: ErrorCode::Validation,
        message: "invalid JSON payload".to_string(),
        details: Some(serde_json::json!({
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

pub async fn send(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    Extension(request_id): Extension<RequestId>,
    payload: Result<Json<PmSendPayload>, JsonRejection>,
) -> impl IntoResponse {
    let request_extensions = request_extensions(&request_id);

    let claims = match require_scope(&claims, PM_WRITE_SCOPE, PM_WRITE_LEGACY_PERMISSIONS) {
        Ok(claims) => claims,
        Err((status, Json(error))) => {
            return err_response::<PmSendData>(status, &request_extensions, error)
        }
    };

    let payload = match payload {
        Ok(Json(payload)) => payload,
        Err(error) => {
            return err_response::<PmSendData>(
                StatusCode::BAD_REQUEST,
                &request_extensions,
                json_error(error),
            )
        }
    };

    if let Err(error) = validate_payload(&payload) {
        return err_response::<PmSendData>(StatusCode::BAD_REQUEST, &request_extensions, error);
    }

    let (_user, ctx) = match ensure_user_ctx(&state, claims).await {
        Ok(value) => value,
        Err((status, Json(error))) => {
            return err_response::<PmSendData>(status, &request_extensions, error)
        }
    };

    if let Err((status, Json(error))) = ensure_permission(&state, &ctx, "pm_send") {
        return err_response::<PmSendData>(status, &request_extensions, error);
    }

    if ctx.session.bool("ban_cannot_access") || ctx.session.bool("ban_cannot_post") {
        return err_response::<PmSendData>(
            StatusCode::FORBIDDEN,
            &request_extensions,
            ApiError {
                code: ErrorCode::Forbidden,
                message: "banned".to_string(),
                details: None,
            },
        );
    }

    let recipients = build_recipient_input(&payload);
    let subject = sanitize_input(&payload.subject);
    let body = sanitize_input(&payload.body);

    match run_forum_blocking(&state, move |forum| {
        pm_ops::send_pm(forum, &ctx, recipients, &subject, &body)
    })
    .await
    {
        Ok(log) => ok_response(StatusCode::CREATED, &request_extensions, map_send_log(log)),
        Err(err) => {
            let (status, api_error) = forum_error(err);
            error!(
                error = %api_error.message,
                request_id = %request_id.0,
                subject = %payload.subject,
                "agent v1 pm send failed"
            );
            err_response::<PmSendData>(status, &request_extensions, api_error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{build_recipient_input, map_send_log, PmSendPayload};
    use btc_forum_rust::pm_ops::{PmSendLog, RecipientFailure};

    #[test]
    fn payload_rejects_impersonation_fields() {
        let payload = serde_json::from_value::<PmSendPayload>(serde_json::json!({
            "to": ["bob"],
            "subject": "Hello",
            "body": "Body",
            "sender_id": 99
        }));

        assert!(payload.is_err());
    }

    #[test]
    fn recipient_input_preserves_to_and_bcc_lists() {
        let payload = serde_json::from_value::<PmSendPayload>(serde_json::json!({
            "to": ["bob", "2"],
            "bcc": ["carol"],
            "subject": "Hello",
            "body": "Body"
        }))
        .unwrap();

        let recipients = build_recipient_input(&payload);
        assert_eq!(recipients.to, vec!["bob", "2"]);
        assert_eq!(recipients.bcc, vec!["carol"]);
    }

    #[test]
    fn maps_send_log_into_agent_response_shape() {
        let data = map_send_log(PmSendLog {
            message_id: Some(42),
            sent: vec![2, 3],
            failed: vec![RecipientFailure {
                target: "unknown".into(),
                reason: "unknown_recipient".into(),
            }],
        });

        assert_eq!(data.message_id, Some(42));
        assert_eq!(data.sent_to, vec![2, 3]);
        assert_eq!(data.failed.len(), 1);
        assert_eq!(data.failed[0].target, "unknown");
        assert_eq!(data.failed[0].reason, "unknown_recipient");
    }
}
