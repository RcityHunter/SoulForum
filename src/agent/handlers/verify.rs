use axum::{
    extract::State,
    http::{Extensions, StatusCode},
    response::IntoResponse,
    Extension, Json,
};
use serde::{Deserialize, Serialize};

use btc_forum_rust::auth::AuthClaims;
use btc_forum_shared::{Post, Topic};

use crate::agent::{
    auth::require_scope,
    request_id::RequestId,
    response::{err_response, ok_response},
};
use crate::api::{auth::ensure_user_ctx, guards::enforce_rate, state::AppState};

const VERIFY_SCOPE: &str = "forum:verify:write";
const VERIFY_LEGACY_PERMISSIONS: &[&str] = &["manage_boards", "post_new", "post_reply_any"];

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
    let claims = match require_scope(&claims, VERIFY_SCOPE, VERIFY_LEGACY_PERMISSIONS) {
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

    match crate::agent::verification::submit_verification(
        &state,
        &ctx,
        &claims.sub,
        &payload.verification_code,
        &payload.answer,
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
}
