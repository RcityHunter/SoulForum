use axum::{
    middleware::from_fn,
    routing::{get, post},
    Router,
};

use crate::api::state::AppState;

use super::{
    handlers::{board, notification, pm, system, topic},
    request_id::inject_request_id,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/system/health", get(system::health))
        .route("/boards", get(board::list))
        .route("/notifications", get(notification::list))
        .route("/pm/send", post(pm::send))
        .route("/topics", get(topic::list).post(topic::create))
        .route("/topics/:topic_id", get(topic::get))
        .route("/replies", post(topic::create_reply))
        .layer(from_fn(inject_request_id))
}
