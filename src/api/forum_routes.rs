use axum::{
    extract::{ConnectInfo, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use std::{net::SocketAddr, time::Duration};
use tracing::{error, warn};

use btc_forum_rust::{
    auth::AuthClaims,
    points,
    services::{BoardAccessEntry, ForumContext},
    surreal::{
        connect_from_env, create_board as create_board_with_client, SurrealPost, SurrealTopic,
    },
};
use btc_forum_shared::{
    Board, BoardsResponse, CreateBoardPayload, CreateBoardResponse, CreatePostPayload,
    CreateTopicPayload, ErrorCode, PostResponse, PostsResponse, TopicCreateResponse,
    TopicsResponse,
};

use super::{
    auth::{ensure_user_ctx, require_auth, user_groups},
    error::api_error,
    guards::{
        enforce_rate, ensure_board_access, ensure_permission, ensure_permission_for_board,
        fetch_topic_board_id, load_board_access, validate_content, verify_csrf,
    },
    state::AppState,
    utils::sanitize_input,
};

fn to_board(board: btc_forum_rust::surreal::SurrealBoard) -> Board {
    Board {
        id: board.id,
        name: board.name,
        description: board.description,
        created_at: board.created_at,
        updated_at: None,
    }
}

fn to_topic(topic: SurrealTopic) -> btc_forum_shared::Topic {
    btc_forum_shared::Topic {
        id: topic.id,
        board_id: Some(topic.board_id),
        subject: topic.subject,
        author: topic.author,
        created_at: topic.created_at,
        updated_at: topic.updated_at,
    }
}

fn to_post(post: SurrealPost) -> btc_forum_shared::Post {
    btc_forum_shared::Post {
        id: post.id,
        topic_id: post.topic_id,
        board_id: post.board_id,
        subject: post.subject,
        body: post.body,
        author: post.author,
        created_at: post.created_at,
    }
}

pub(crate) async fn surreal_posts(
    State(state): State<AppState>,
    _claims: Option<AuthClaims>,
) -> impl IntoResponse {
    match state.surreal.list_posts().await {
        Ok(posts) => (
            StatusCode::OK,
            Json(PostsResponse {
                status: "ok".to_string(),
                posts: posts.into_iter().map(to_post).collect(),
            }),
        )
            .into_response(),
        Err(err) => {
            error!(error = %err, "failed to list surreal posts");
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                ErrorCode::Internal,
                err.to_string(),
            )
            .into_response()
        }
    }
}

pub(crate) async fn create_surreal_board(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(payload): Json<CreateBoardPayload>,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c,
        Err(resp) => return resp.into_response(),
    };
    let (_user, ctx) = match ensure_user_ctx(&state, claims).await {
        Ok(value) => value,
        Err(resp) => return resp.into_response(),
    };
    let key = format!("{}:{}", claims.sub, addr.ip());
    if let Err(resp) = enforce_rate(&state, &key, 10, Duration::from_secs(60)) {
        return resp.into_response();
    }
    if payload.name.trim().is_empty() || payload.name.trim().len() > 100 {
        return api_error(
            StatusCode::BAD_REQUEST,
            ErrorCode::Validation,
            "name must be 1..100 chars",
        )
        .into_response();
    }
    if let Err(resp) = ensure_permission(&state, &ctx, "manage_boards") {
        return resp.into_response();
    }
    let board_result = async {
        let fresh = connect_from_env().await?;
        create_board_with_client(&fresh, &payload.name, payload.description.as_deref()).await
    }
    .await;

    match board_result {
        Ok(board) => (
            StatusCode::CREATED,
            Json(CreateBoardResponse {
                status: "ok".to_string(),
                board: to_board(board),
            }),
        )
            .into_response(),
        Err(err) => {
            error!(error = %err, "failed to create board");
            api_error(
                StatusCode::BAD_REQUEST,
                ErrorCode::Validation,
                err.to_string(),
            )
            .into_response()
        }
    }
}

pub(crate) async fn surreal_boards(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
) -> impl IntoResponse {
    let mut ctx = ForumContext::default();
    if let Some(claims) = claims {
        if let Ok((_user, c)) = ensure_user_ctx(&state, &claims).await {
            ctx = c;
        }
    }
    let access_rules: Option<Vec<BoardAccessEntry>> = load_board_access(&state).await.ok();
    match state.surreal.list_boards().await {
        Ok(boards) => {
            let filtered = match access_rules {
                Some(rules) => boards
                    .into_iter()
                    .filter(|b| {
                        if ctx.user_info.is_admin {
                            return true;
                        }
                        if let Some(rule) = rules
                            .iter()
                            .find(|r| r.id == b.id.clone().unwrap_or_default())
                        {
                            if rule.allowed_groups.is_empty() {
                                return true;
                            }
                            let groups = user_groups(&ctx);
                            rule.allowed_groups
                                .iter()
                                .any(|gid| groups.iter().any(|g| g == gid))
                        } else {
                            true
                        }
                    })
                    .map(|b| Board {
                        id: b.id,
                        name: b.name,
                        description: b.description,
                        created_at: b.created_at,
                        updated_at: None,
                    })
                    .collect(),
                None => boards
                    .into_iter()
                    .map(|b| Board {
                        id: b.id,
                        name: b.name,
                        description: b.description,
                        created_at: b.created_at,
                        updated_at: None,
                    })
                    .collect(),
            };
            (
                StatusCode::OK,
                Json(BoardsResponse {
                    status: "ok".to_string(),
                    boards: filtered,
                }),
            )
                .into_response()
        }
        Err(err) => {
            error!(error = %err, "failed to list boards");
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                ErrorCode::Internal,
                err.to_string(),
            )
            .into_response()
        }
    }
}

pub(crate) async fn create_surreal_topic(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<CreateTopicPayload>,
) -> Response {
    let claims = match require_auth(&claims) {
        Ok(c) => c,
        Err(resp) => return resp.into_response(),
    };
    let (user, ctx) = match ensure_user_ctx(&state, claims).await {
        Ok(value) => value,
        Err(resp) => return resp.into_response(),
    };
    let key = format!("{}:{}", claims.sub, addr.ip());
    if let Err(resp) = enforce_rate(&state, &key, 20, Duration::from_secs(60)) {
        return resp.into_response();
    }
    if let Err(resp) = validate_content(&payload.subject, &payload.body) {
        return resp.into_response();
    }
    if let Err(resp) = verify_csrf(&headers) {
        return resp.into_response();
    }
    if let Err(resp) = ensure_board_access(&state, &ctx, &payload.board_id).await {
        return resp.into_response();
    }
    if let Err(resp) =
        ensure_permission_for_board(&state, &ctx, "post_new", Some(&payload.board_id)).await
    {
        return resp.into_response();
    }
    let author = user.name.clone();
    let topic_result: Result<(SurrealTopic, SurrealPost), surrealdb::Error> = async {
        let topic = state
            .surreal
            .create_topic(
                &payload.board_id,
                &sanitize_input(&payload.subject),
                &author,
            )
            .await?;
        let topic_id = topic.id.clone().unwrap_or_default();
        let post = state
            .surreal
            .create_post_in_topic(
                &topic_id,
                &payload.board_id,
                &sanitize_input(&payload.subject),
                &sanitize_input(&payload.body),
                &author,
            )
            .await?;
        Ok((topic, post))
    }
    .await;

    match topic_result {
        Ok((topic, post)) => {
            if let Some(topic_id) = topic.id.as_deref() {
                let payload = points::topic_created_payload(user.legacy_id(), topic_id);
                if let Err(err) = points::create_points_event(state.surreal.client(), payload).await {
                    warn!(
                        error = %err,
                        topic_id = %topic_id,
                        user = %user.name,
                        "topic created but failed to write points ledger event"
                    );
                }
            }
            (
                StatusCode::CREATED,
                Json(TopicCreateResponse {
                    status: "ok".to_string(),
                    topic: to_topic(topic),
                    first_post: to_post(post),
                }),
            )
                .into_response()
        }
        Err(err) => {
            error!(error = %err, "failed to create topic");
            api_error(
                StatusCode::BAD_REQUEST,
                ErrorCode::Validation,
                err.to_string(),
            )
            .into_response()
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct ListTopicsParams {
    pub(crate) board_id: String,
}

pub(crate) async fn list_surreal_topics(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    Query(params): Query<ListTopicsParams>,
) -> impl IntoResponse {
    let mut ctx = ForumContext::default();
    if let Some(claims) = claims {
        if let Ok((_user, c)) = ensure_user_ctx(&state, &claims).await {
            ctx = c;
        }
    }
    if let Err(resp) = ensure_board_access(&state, &ctx, &params.board_id).await {
        return resp.into_response();
    }
    match state.surreal.list_topics(&params.board_id).await {
        Ok(topics) => (
            StatusCode::OK,
            Json(TopicsResponse {
                status: "ok".to_string(),
                topics: topics.into_iter().map(to_topic).collect(),
            }),
        )
            .into_response(),
        Err(err) => {
            error!(error = %err, "failed to list topics");
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                ErrorCode::Internal,
                err.to_string(),
            )
            .into_response()
        }
    }
}

pub(crate) async fn create_surreal_topic_post(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<CreatePostPayload>,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c,
        Err(resp) => return resp.into_response(),
    };
    let (user, ctx) = match ensure_user_ctx(&state, claims).await {
        Ok(value) => value,
        Err(resp) => return resp.into_response(),
    };
    let key = format!("{}:{}", claims.sub, addr.ip());
    if let Err(resp) = enforce_rate(&state, &key, 40, Duration::from_secs(60)) {
        return resp.into_response();
    }
    let subject = payload
        .subject
        .clone()
        .unwrap_or_else(|| "Re: topic".into());
    if let Err(resp) = validate_content(&subject, &payload.body) {
        return resp.into_response();
    }
    if let Err(resp) = verify_csrf(&headers) {
        return resp.into_response();
    }
    if let Err(resp) = ensure_board_access(&state, &ctx, &payload.board_id).await {
        return resp.into_response();
    }
    if let Err(resp) =
        ensure_permission_for_board(&state, &ctx, "post_reply_any", Some(&payload.board_id)).await
    {
        return resp.into_response();
    }
    let author = user.name.clone();
    match state
        .surreal
        .create_post_in_topic(
            &payload.topic_id,
            &payload.board_id,
            &sanitize_input(&subject),
            &sanitize_input(&payload.body),
            &author,
        )
        .await
    {
        Ok(post) => {
            if let Some(post_id) = post.id.as_deref() {
                let points_payload = points::reply_created_payload(user.legacy_id(), post_id);
                if let Err(err) = points::create_points_event(state.surreal.client(), points_payload).await {
                    warn!(
                        error = %err,
                        post_id = %post_id,
                        topic_id = %payload.topic_id,
                        user = %user.name,
                        "reply created but failed to write points ledger event"
                    );
                }
            }
            (
                StatusCode::CREATED,
                Json(PostResponse {
                    status: "ok".to_string(),
                    post: to_post(post),
                }),
            )
                .into_response()
        }
        Err(err) => {
            error!(error = %err, "failed to create post");
            api_error(
                StatusCode::BAD_REQUEST,
                ErrorCode::Validation,
                err.to_string(),
            )
            .into_response()
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct ListPostsParams {
    pub(crate) topic_id: String,
}

pub(crate) async fn list_surreal_posts_for_topic(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    Query(params): Query<ListPostsParams>,
) -> impl IntoResponse {
    let mut ctx = ForumContext::default();
    if let Some(claims) = claims {
        if let Ok((_user, c)) = ensure_user_ctx(&state, &claims).await {
            ctx = c;
        }
    }
    if let Some(board_id) = fetch_topic_board_id(state.surreal.client(), &params.topic_id).await {
        if let Err(resp) = ensure_board_access(&state, &ctx, &board_id).await {
            return resp.into_response();
        }
    }
    match state.surreal.list_posts_for_topic(&params.topic_id).await {
        Ok(posts) => (
            StatusCode::OK,
            Json(PostsResponse {
                status: "ok".to_string(),
                posts: posts.into_iter().map(to_post).collect(),
            }),
        )
            .into_response(),
        Err(err) => {
            error!(error = %err, "failed to list posts");
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                ErrorCode::Internal,
                err.to_string(),
            )
            .into_response()
        }
    }
}
