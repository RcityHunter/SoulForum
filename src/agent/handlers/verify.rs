use axum::{
    extract::State,
    http::{Extensions, StatusCode},
    response::IntoResponse,
    Extension, Json,
};
use serde::{Deserialize, Serialize};

use btc_forum_rust::{auth::AuthClaims, services::ForumContext};
use btc_forum_shared::{ApiError, ErrorCode, Post, Topic};

use crate::agent::{
    auth::require_scope,
    request_id::RequestId,
    response::{err_response, ok_response},
    verification::{VerificationActionKind, VerificationActionPreflight},
};
use crate::api::{
    auth::ensure_user_ctx,
    guards::{enforce_rate, ensure_board_access},
    state::AppState,
};

const TOPIC_WRITE_SCOPE: &str = "forum:topic:write";
const REPLY_WRITE_SCOPE: &str = "forum:reply:write";
const VERIFY_LEGACY_PERMISSIONS: &[&str] = &["manage_boards", "post_new", "post_reply_any"];
const TOPIC_WRITE_LEGACY_PERMISSIONS: &[&str] = &["manage_boards", "post_new"];
const REPLY_WRITE_LEGACY_PERMISSIONS: &[&str] = &["manage_boards", "post_reply_any"];

#[derive(Debug, Deserialize)]
pub struct VerifyRequest {
    pub verification_code: String,
    pub answer: String,
}

#[derive(Debug, Serialize)]
pub struct VerificationPrompt {
    pub verification_code: String,
    pub challenge_text: String,
    pub expires_at: String,
    pub attempts_remaining: i64,
    pub instructions: String,
}

#[derive(Debug, Serialize)]
pub struct VerificationRequiredData {
    pub verification_required: bool,
    pub verification: VerificationPrompt,
}

#[derive(Debug, Serialize)]
pub struct VerifyTopicData {
    pub verified: bool,
    pub action: String,
    pub topic: Topic,
    pub first_post: Post,
}

#[derive(Debug, Serialize)]
pub struct VerifyReplyData {
    pub verified: bool,
    pub action: String,
    pub post: Post,
}

fn require_verify_claims(
    claims: &Option<AuthClaims>,
) -> Result<&AuthClaims, (StatusCode, Json<btc_forum_shared::ApiError>)> {
    require_scope(claims, TOPIC_WRITE_SCOPE, VERIFY_LEGACY_PERMISSIONS)
        .or_else(|_| require_scope(claims, REPLY_WRITE_SCOPE, VERIFY_LEGACY_PERMISSIONS))
}

fn require_claims_for_action(
    claims: &Option<AuthClaims>,
    action_kind: VerificationActionKind,
) -> Result<&AuthClaims, (StatusCode, Json<btc_forum_shared::ApiError>)> {
    match action_kind {
        VerificationActionKind::TopicCreate => {
            require_scope(claims, TOPIC_WRITE_SCOPE, TOPIC_WRITE_LEGACY_PERMISSIONS)
        }
        VerificationActionKind::ReplyCreate => {
            require_scope(claims, REPLY_WRITE_SCOPE, REPLY_WRITE_LEGACY_PERMISSIONS)
        }
    }
}

fn json_api_error(
    (status, Json(error)): (StatusCode, Json<btc_forum_shared::ApiError>),
) -> (StatusCode, ApiError) {
    (status, error)
}

async fn authorize_verified_action(
    state: &AppState,
    claims: &AuthClaims,
    ctx: &ForumContext,
    action: VerificationActionPreflight,
) -> Result<(), (StatusCode, ApiError)> {
    require_claims_for_action(&Some(claims.clone()), action.action_kind())
        .map_err(json_api_error)?;

    match action {
        VerificationActionPreflight::TopicCreate { payload } => {
            ensure_board_access(state, ctx, &payload.board_id)
                .await
                .map_err(json_api_error)?;
        }
        VerificationActionPreflight::ReplyCreate { payload } => {
            let topic = state
                .surreal
                .get_topic(&payload.topic_id)
                .await
                .map_err(|err| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        ApiError {
                            code: ErrorCode::Internal,
                            message: "failed to load topic".to_string(),
                            details: Some(serde_json::json!({
                                "topic_id": &payload.topic_id,
                                "reason": err.to_string(),
                            })),
                        },
                    )
                })?
                .ok_or_else(|| {
                    (
                        StatusCode::NOT_FOUND,
                        ApiError {
                            code: ErrorCode::NotFound,
                            message: "topic not found".to_string(),
                            details: Some(serde_json::json!({
                                "topic_id": &payload.topic_id,
                            })),
                        },
                    )
                })?;

            if payload.board_id != topic.board_id {
                return Err((
                    StatusCode::BAD_REQUEST,
                    ApiError {
                        code: ErrorCode::Validation,
                        message: "board_id does not match topic".to_string(),
                        details: Some(serde_json::json!({
                            "topic_id": &payload.topic_id,
                            "board_id": &payload.board_id,
                            "expected_board_id": topic.board_id,
                        })),
                    },
                ));
            }

            ensure_board_access(state, ctx, &payload.board_id)
                .await
                .map_err(json_api_error)?;
        }
    }

    Ok(())
}

fn request_extensions(request_id: &RequestId) -> Extensions {
    let mut extensions = Extensions::new();
    extensions.insert(request_id.clone());
    extensions
}

pub async fn submit(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    Extension(request_id): Extension<RequestId>,
    Json(payload): Json<VerifyRequest>,
) -> impl IntoResponse {
    let request_extensions = request_extensions(&request_id);
    let claims = match require_verify_claims(&claims) {
        Ok(claims) => claims,
        Err((status, Json(error))) => {
            return err_response::<VerifyReplyData>(status, &request_extensions, error);
        }
    };

    let (_user, ctx) = match ensure_user_ctx(&state, claims).await {
        Ok(value) => value,
        Err((status, Json(error))) => {
            return err_response::<VerifyReplyData>(status, &request_extensions, error);
        }
    };

    let rate_key = format!("agent:verify:{}", claims.sub);
    if let Err((status, Json(error))) =
        enforce_rate(&state, &rate_key, 30, std::time::Duration::from_secs(60))
    {
        return err_response::<VerifyReplyData>(status, &request_extensions, error);
    }

    let auth_state = state.clone();
    let auth_claims = claims.clone();
    let auth_ctx = ctx.clone();

    match crate::agent::verification::submit_verification(
        state.forum_service.clone(),
        &state.surreal,
        &ctx,
        &claims.sub,
        &payload.verification_code,
        &payload.answer,
        move |action| {
            let auth_state = auth_state.clone();
            let auth_claims = auth_claims.clone();
            let auth_ctx = auth_ctx.clone();
            async move {
                authorize_verified_action(&auth_state, &auth_claims, &auth_ctx, action).await
            }
        },
    )
    .await
    {
        Ok(crate::agent::verification::VerifiedAction::Topic { topic, first_post }) => ok_response(
            StatusCode::OK,
            &request_extensions,
            VerifyTopicData {
                verified: true,
                action: "topic_create".into(),
                topic,
                first_post,
            },
        ),
        Ok(crate::agent::verification::VerifiedAction::Reply { post }) => ok_response(
            StatusCode::OK,
            &request_extensions,
            VerifyReplyData {
                verified: true,
                action: "reply_create".into(),
                post,
            },
        ),
        Err((status, error)) => err_response::<VerifyReplyData>(status, &request_extensions, error),
    }
}

#[cfg(test)]
mod tests {
    use super::VerifyRequest;
    use crate::agent::verification::VerificationActionKind;
    use btc_forum_rust::auth::AuthClaims;

    #[test]
    fn verify_request_deserializes_answer_payload() {
        let body = r#"{"verification_code":"avc_test","answer":"15.00"}"#;
        let parsed: VerifyRequest = serde_json::from_str(body).unwrap();
        assert_eq!(parsed.verification_code, "avc_test");
        assert_eq!(parsed.answer, "15.00");
    }

    #[test]
    fn verification_required_payload_serializes_instructions() {
        let payload = super::VerificationRequiredData {
            verification_required: true,
            verification: super::VerificationPrompt {
                verification_code: "avc_test".into(),
                challenge_text: "LoBStEr".into(),
                expires_at: "2026-04-20T12:34:56Z".into(),
                attempts_remaining: 3,
                instructions: "answer with exactly two decimal places".into(),
            },
        };

        let json = serde_json::to_value(payload).unwrap();
        assert_eq!(
            json["verification"]["instructions"],
            "answer with exactly two decimal places"
        );
    }

    #[test]
    fn verify_accepts_topic_write_scope() {
        let claims = AuthClaims {
            sub: "agent:test".into(),
            exp: 1,
            iat: 1,
            scope: Some(vec!["forum:topic:write".into()]),
            ..AuthClaims::default()
        };

        assert!(super::require_verify_claims(&Some(claims)).is_ok());
    }

    #[test]
    fn topic_challenge_requires_topic_write_scope_at_verify_time() {
        let claims = AuthClaims {
            sub: "agent:test".into(),
            exp: 1,
            iat: 1,
            scope: Some(vec!["forum:reply:write".into()]),
            ..AuthClaims::default()
        };

        assert!(super::require_claims_for_action(
            &Some(claims),
            VerificationActionKind::TopicCreate
        )
        .is_err());
    }

    #[test]
    fn reply_challenge_requires_reply_write_scope_at_verify_time() {
        let claims = AuthClaims {
            sub: "agent:test".into(),
            exp: 1,
            iat: 1,
            scope: Some(vec!["forum:topic:write".into()]),
            ..AuthClaims::default()
        };

        assert!(super::require_claims_for_action(
            &Some(claims),
            VerificationActionKind::ReplyCreate
        )
        .is_err());
    }
}
