use axum::{
    extract::{Query, State},
    http::{Extensions, StatusCode},
    response::IntoResponse,
    Extension, Json,
};
use serde::{Deserialize, Serialize};
use tracing::error;

use btc_forum_rust::auth::AuthClaims;
use btc_forum_shared::{ApiError, ErrorCode, Topic};

use crate::agent::{
    auth::require_scope,
    request_id::RequestId,
    response::{err_response, ok_response},
};
use crate::api::{auth::ensure_user_ctx, guards::ensure_board_access, state::AppState};

#[derive(Debug, Deserialize)]
pub struct TopicListParams {
    pub board_id: String,
}

#[derive(Debug, Serialize)]
pub struct TopicListData {
    pub topics: Vec<Topic>,
    pub next_cursor: Option<String>,
}

pub async fn list(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    Extension(request_id): Extension<RequestId>,
    Query(params): Query<TopicListParams>,
) -> impl IntoResponse {
    let request_extensions = {
        let mut extensions = Extensions::new();
        extensions.insert(request_id.clone());
        extensions
    };

    let claims = match require_scope(&claims, "forum:topic:read", &["manage_boards", "post_new", "post_reply_any"]) {
        Ok(claims) => claims,
        Err((status, Json(error))) => return err_response::<TopicListData>(status, &request_extensions, error),
    };

    let (_user, ctx) = match ensure_user_ctx(&state, claims).await {
        Ok(value) => value,
        Err((status, Json(error))) => return err_response::<TopicListData>(status, &request_extensions, error),
    };

    if let Err((status, Json(error))) = ensure_board_access(&state, &ctx, &params.board_id).await {
        return err_response::<TopicListData>(status, &request_extensions, error);
    }

    let topics = match state.surreal.list_topics(&params.board_id).await {
        Ok(topics) => topics,
        Err(err) => {
            error!(
                error = %err,
                request_id = %request_id.0,
                board_id = %params.board_id,
                "agent v1 topic list failed"
            );
            return err_response::<TopicListData>(
                StatusCode::INTERNAL_SERVER_ERROR,
                &request_extensions,
                ApiError {
                    code: ErrorCode::Internal,
                    message: "failed to list topics".to_string(),
                    details: Some(serde_json::json!({
                        "board_id": params.board_id,
                    })),
                },
            );
        }
    };

    let data = TopicListData {
        topics: topics
            .into_iter()
            .map(|topic| Topic {
                id: topic.id,
                board_id: Some(topic.board_id),
                subject: topic.subject,
                author: topic.author,
                created_at: topic.created_at,
                updated_at: topic.updated_at,
            })
            .collect(),
        next_cursor: None,
    };

    ok_response(StatusCode::OK, &request_extensions, data)
}
