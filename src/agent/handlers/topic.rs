use axum::{
    extract::{Path, Query, State},
    http::{Extensions, StatusCode},
    response::IntoResponse,
    Extension, Json,
};
use serde::{Deserialize, Serialize};
use tracing::error;

use btc_forum_rust::auth::AuthClaims;
use btc_forum_rust::surreal::{SurrealPost, SurrealTopic};
use btc_forum_shared::{ApiError, CreatePostPayload, CreateTopicPayload, ErrorCode, Post, Topic};

use crate::agent::{
    auth::require_scope,
    request_id::RequestId,
    response::{err_response, ok_response},
};
use crate::api::{
    auth::ensure_user_ctx,
    guards::{enforce_rate, ensure_board_access, validate_content},
    state::AppState,
    utils::sanitize_input,
};

const TOPIC_READ_SCOPE: &str = "forum:topic:read";
const TOPIC_WRITE_SCOPE: &str = "forum:topic:write";
const REPLY_WRITE_SCOPE: &str = "forum:reply:write";
const TOPIC_READ_LEGACY_PERMISSIONS: &[&str] = &["manage_boards", "post_new", "post_reply_any"];
const TOPIC_WRITE_LEGACY_PERMISSIONS: &[&str] = &["manage_boards", "post_new"];
const REPLY_WRITE_LEGACY_PERMISSIONS: &[&str] = &["manage_boards", "post_reply_any"];

#[derive(Debug, Deserialize)]
pub struct TopicListParams {
    pub board_id: String,
}

#[derive(Debug, Serialize)]
pub struct TopicListData {
    pub topics: Vec<Topic>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TopicGetData {
    pub topic: Topic,
    pub posts: Vec<Post>,
}

#[derive(Debug, Serialize)]
pub struct TopicCreateData {
    pub topic: Topic,
    pub first_post: Post,
}

#[derive(Debug, Serialize)]
pub struct ReplyCreateData {
    pub post: Post,
}

fn request_extensions(request_id: &RequestId) -> Extensions {
    let mut extensions = Extensions::new();
    extensions.insert(request_id.clone());
    extensions
}

fn to_topic(topic: SurrealTopic) -> Topic {
    Topic {
        id: topic.id,
        board_id: Some(topic.board_id),
        subject: topic.subject,
        author: topic.author,
        created_at: topic.created_at,
        updated_at: topic.updated_at,
    }
}

fn to_post(post: SurrealPost) -> Post {
    Post {
        id: post.id,
        topic_id: post.topic_id,
        board_id: post.board_id,
        subject: post.subject,
        body: post.body,
        author: post.author,
        created_at: post.created_at,
    }
}

fn reply_subject(topic_subject: &str, requested_subject: Option<&str>) -> String {
    requested_subject
        .map(str::trim)
        .filter(|subject| !subject.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("Re: {topic_subject}"))
}

pub async fn list(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    Extension(request_id): Extension<RequestId>,
    Query(params): Query<TopicListParams>,
) -> impl IntoResponse {
    let request_extensions = request_extensions(&request_id);

    let claims = match require_scope(&claims, TOPIC_READ_SCOPE, TOPIC_READ_LEGACY_PERMISSIONS) {
        Ok(claims) => claims,
        Err((status, Json(error))) => {
            return err_response::<TopicListData>(status, &request_extensions, error)
        }
    };

    let (_user, ctx) = match ensure_user_ctx(&state, claims).await {
        Ok(value) => value,
        Err((status, Json(error))) => {
            return err_response::<TopicListData>(status, &request_extensions, error)
        }
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
        topics: topics.into_iter().map(to_topic).collect(),
        next_cursor: None,
    };

    ok_response(StatusCode::OK, &request_extensions, data)
}

pub async fn get(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    Extension(request_id): Extension<RequestId>,
    Path(topic_id): Path<String>,
) -> impl IntoResponse {
    let request_extensions = request_extensions(&request_id);

    let claims = match require_scope(&claims, TOPIC_READ_SCOPE, TOPIC_READ_LEGACY_PERMISSIONS) {
        Ok(claims) => claims,
        Err((status, Json(error))) => {
            return err_response::<TopicGetData>(status, &request_extensions, error)
        }
    };

    let (_user, ctx) = match ensure_user_ctx(&state, claims).await {
        Ok(value) => value,
        Err((status, Json(error))) => {
            return err_response::<TopicGetData>(status, &request_extensions, error)
        }
    };

    let topic = match state.surreal.get_topic(&topic_id).await {
        Ok(Some(topic)) => topic,
        Ok(None) => {
            return err_response::<TopicGetData>(
                StatusCode::NOT_FOUND,
                &request_extensions,
                ApiError {
                    code: ErrorCode::NotFound,
                    message: "topic not found".to_string(),
                    details: Some(serde_json::json!({
                        "topic_id": topic_id,
                    })),
                },
            )
        }
        Err(err) => {
            error!(
                error = %err,
                request_id = %request_id.0,
                topic_id = %topic_id,
                "agent v1 topic get failed"
            );
            return err_response::<TopicGetData>(
                StatusCode::INTERNAL_SERVER_ERROR,
                &request_extensions,
                ApiError {
                    code: ErrorCode::Internal,
                    message: "failed to load topic".to_string(),
                    details: Some(serde_json::json!({
                        "topic_id": topic_id,
                    })),
                },
            );
        }
    };

    if let Err((status, Json(error))) = ensure_board_access(&state, &ctx, &topic.board_id).await {
        return err_response::<TopicGetData>(status, &request_extensions, error);
    }

    let posts = match state.surreal.list_posts_for_topic(&topic_id).await {
        Ok(posts) => posts,
        Err(err) => {
            error!(
                error = %err,
                request_id = %request_id.0,
                topic_id = %topic_id,
                "agent v1 topic posts load failed"
            );
            return err_response::<TopicGetData>(
                StatusCode::INTERNAL_SERVER_ERROR,
                &request_extensions,
                ApiError {
                    code: ErrorCode::Internal,
                    message: "failed to load topic posts".to_string(),
                    details: Some(serde_json::json!({
                        "topic_id": topic_id,
                    })),
                },
            );
        }
    };

    ok_response(
        StatusCode::OK,
        &request_extensions,
        TopicGetData {
            topic: to_topic(topic),
            posts: posts.into_iter().map(to_post).collect(),
        },
    )
}

pub async fn create(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    Extension(request_id): Extension<RequestId>,
    Json(payload): Json<CreateTopicPayload>,
) -> impl IntoResponse {
    let request_extensions = request_extensions(&request_id);

    let claims = match require_scope(&claims, TOPIC_WRITE_SCOPE, TOPIC_WRITE_LEGACY_PERMISSIONS) {
        Ok(claims) => claims,
        Err((status, Json(error))) => {
            return err_response::<TopicCreateData>(status, &request_extensions, error)
        }
    };

    let (user, ctx) = match ensure_user_ctx(&state, claims).await {
        Ok(value) => value,
        Err((status, Json(error))) => {
            return err_response::<TopicCreateData>(status, &request_extensions, error)
        }
    };

    if let Err((status, Json(error))) = ensure_board_access(&state, &ctx, &payload.board_id).await {
        return err_response::<TopicCreateData>(status, &request_extensions, error);
    }

    if let Err((status, Json(error))) = validate_content(&payload.subject, &payload.body) {
        return err_response::<TopicCreateData>(status, &request_extensions, error);
    }

    let rate_key = format!("agent:topic:create:{}", claims.sub);
    if let Err((status, Json(error))) =
        enforce_rate(&state, &rate_key, 20, std::time::Duration::from_secs(60))
    {
        return err_response::<TopicCreateData>(status, &request_extensions, error);
    }

    let author = user.name.clone();
    let subject = sanitize_input(&payload.subject);
    let body = sanitize_input(&payload.body);

    let topic_result: Result<(SurrealTopic, SurrealPost), surrealdb::Error> = async {
        let topic = state
            .surreal
            .create_topic(&payload.board_id, &subject, &author)
            .await?;
        let topic_id = topic.id.clone().unwrap_or_default();
        let post = state
            .surreal
            .create_post_in_topic(&topic_id, &payload.board_id, &subject, &body, &author)
            .await?;
        Ok((topic, post))
    }
    .await;

    match topic_result {
        Ok((topic, first_post)) => ok_response(
            StatusCode::CREATED,
            &request_extensions,
            TopicCreateData {
                topic: to_topic(topic),
                first_post: to_post(first_post),
            },
        ),
        Err(err) => {
            error!(
                error = %err,
                request_id = %request_id.0,
                board_id = %payload.board_id,
                "agent v1 topic create failed"
            );
            err_response::<TopicCreateData>(
                StatusCode::BAD_REQUEST,
                &request_extensions,
                ApiError {
                    code: ErrorCode::Validation,
                    message: "failed to create topic".to_string(),
                    details: Some(serde_json::json!({
                        "board_id": payload.board_id,
                        "reason": err.to_string(),
                    })),
                },
            )
        }
    }
}

pub async fn create_reply(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    Extension(request_id): Extension<RequestId>,
    Json(payload): Json<CreatePostPayload>,
) -> impl IntoResponse {
    let request_extensions = request_extensions(&request_id);

    let claims = match require_scope(&claims, REPLY_WRITE_SCOPE, REPLY_WRITE_LEGACY_PERMISSIONS) {
        Ok(claims) => claims,
        Err((status, Json(error))) => {
            return err_response::<ReplyCreateData>(status, &request_extensions, error)
        }
    };

    let (user, ctx) = match ensure_user_ctx(&state, claims).await {
        Ok(value) => value,
        Err((status, Json(error))) => {
            return err_response::<ReplyCreateData>(status, &request_extensions, error)
        }
    };

    let topic = match state.surreal.get_topic(&payload.topic_id).await {
        Ok(Some(topic)) => topic,
        Ok(None) => {
            return err_response::<ReplyCreateData>(
                StatusCode::NOT_FOUND,
                &request_extensions,
                ApiError {
                    code: ErrorCode::NotFound,
                    message: "topic not found".to_string(),
                    details: Some(serde_json::json!({
                        "topic_id": payload.topic_id,
                    })),
                },
            )
        }
        Err(err) => {
            error!(
                error = %err,
                request_id = %request_id.0,
                topic_id = %payload.topic_id,
                "agent v1 reply topic lookup failed"
            );
            return err_response::<ReplyCreateData>(
                StatusCode::INTERNAL_SERVER_ERROR,
                &request_extensions,
                ApiError {
                    code: ErrorCode::Internal,
                    message: "failed to load topic".to_string(),
                    details: Some(serde_json::json!({
                        "topic_id": payload.topic_id,
                    })),
                },
            );
        }
    };

    if payload.board_id != topic.board_id {
        return err_response::<ReplyCreateData>(
            StatusCode::BAD_REQUEST,
            &request_extensions,
            ApiError {
                code: ErrorCode::Validation,
                message: "board_id does not match topic".to_string(),
                details: Some(serde_json::json!({
                    "topic_id": payload.topic_id,
                    "board_id": payload.board_id,
                    "expected_board_id": topic.board_id,
                })),
            },
        );
    }

    if let Err((status, Json(error))) = ensure_board_access(&state, &ctx, &topic.board_id).await {
        return err_response::<ReplyCreateData>(status, &request_extensions, error);
    }

    let subject = reply_subject(&topic.subject, payload.subject.as_deref());
    if let Err((status, Json(error))) = validate_content(&subject, &payload.body) {
        return err_response::<ReplyCreateData>(status, &request_extensions, error);
    }

    let rate_key = format!("agent:reply:create:{}", claims.sub);
    if let Err((status, Json(error))) =
        enforce_rate(&state, &rate_key, 40, std::time::Duration::from_secs(60))
    {
        return err_response::<ReplyCreateData>(status, &request_extensions, error);
    }

    let author = user.name.clone();
    let subject = sanitize_input(&subject);
    let body = sanitize_input(&payload.body);

    match state
        .surreal
        .create_post_in_topic(&payload.topic_id, &topic.board_id, &subject, &body, &author)
        .await
    {
        Ok(post) => ok_response(
            StatusCode::CREATED,
            &request_extensions,
            ReplyCreateData {
                post: to_post(post),
            },
        ),
        Err(err) => {
            error!(
                error = %err,
                request_id = %request_id.0,
                topic_id = %payload.topic_id,
                "agent v1 reply create failed"
            );
            err_response::<ReplyCreateData>(
                StatusCode::BAD_REQUEST,
                &request_extensions,
                ApiError {
                    code: ErrorCode::Validation,
                    message: "failed to create reply".to_string(),
                    details: Some(serde_json::json!({
                        "topic_id": payload.topic_id,
                        "reason": err.to_string(),
                    })),
                },
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{reply_subject, to_post, to_topic};
    use btc_forum_rust::surreal::{SurrealPost, SurrealTopic};

    #[test]
    fn reply_subject_defaults_to_topic_subject() {
        assert_eq!(reply_subject("Welcome", None), "Re: Welcome");
        assert_eq!(reply_subject("Welcome", Some("   ")), "Re: Welcome");
    }

    #[test]
    fn reply_subject_prefers_explicit_non_empty_subject() {
        assert_eq!(reply_subject("Welcome", Some("Custom")), "Custom");
    }

    #[test]
    fn maps_surreal_topic_into_shared_topic() {
        let topic = to_topic(SurrealTopic {
            id: Some("topics:1".into()),
            board_id: "boards:1".into(),
            subject: "Hello".into(),
            author: "alice".into(),
            created_at: Some("2026-03-19T00:00:00Z".into()),
            updated_at: Some("2026-03-19T00:01:00Z".into()),
        });

        assert_eq!(topic.id.as_deref(), Some("topics:1"));
        assert_eq!(topic.board_id.as_deref(), Some("boards:1"));
        assert_eq!(topic.subject, "Hello");
        assert_eq!(topic.author, "alice");
    }

    #[test]
    fn maps_surreal_post_into_shared_post() {
        let post = to_post(SurrealPost {
            id: Some("posts:1".into()),
            topic_id: Some("topics:1".into()),
            board_id: Some("boards:1".into()),
            subject: "Re: Hello".into(),
            body: "Body".into(),
            author: "bob".into(),
            created_at: Some("2026-03-19T00:00:00Z".into()),
        });

        assert_eq!(post.id.as_deref(), Some("posts:1"));
        assert_eq!(post.topic_id.as_deref(), Some("topics:1"));
        assert_eq!(post.board_id.as_deref(), Some("boards:1"));
        assert_eq!(post.subject, "Re: Hello");
        assert_eq!(post.body, "Body");
        assert_eq!(post.author, "bob");
    }
}
