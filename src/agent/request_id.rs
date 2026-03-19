use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    body::Body,
    http::{HeaderValue, Request},
    middleware::Next,
    response::Response,
};
use tracing::debug;

static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug)]
pub struct RequestId(pub String);

pub fn generate_request_id() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    let counter = REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed);

    format!("agv1-{millis}-{counter}")
}

pub async fn inject_request_id(mut req: Request<Body>, next: Next) -> Response {
    let request_id = req
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
        .unwrap_or_else(generate_request_id);

    let method = req.method().clone();
    let path = req.uri().path().to_string();

    req.extensions_mut().insert(RequestId(request_id.clone()));

    debug!(request_id = %request_id, %method, %path, "agent request started");
    let mut response = next.run(req).await;
    debug!(
        request_id = %request_id,
        %method,
        %path,
        status = %response.status(),
        "agent request completed"
    );
    if let Ok(value) = HeaderValue::from_str(&request_id) {
        response.headers_mut().insert("x-request-id", value);
    }
    response
}
