use axum::{middleware::from_fn, routing::get, Router};

use crate::api::state::AppState;

use super::{
    handlers::{system, topic},
    request_id::inject_request_id,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/system/health", get(system::health))
        .route("/topics", get(topic::list))
        .layer(from_fn(inject_request_id))
}
