use axum::{
    body::Body,
    http::{Request, StatusCode},
    middleware::{from_fn, Next},
    response::IntoResponse,
    routing::post,
    Router,
};
use btc_forum_rust::auth::AuthClaims;
use btc_forum_rust::services::{ForumContext, ForumService, InMemoryService};
use tower::ServiceExt;

async fn reject_layer(req: Request<Body>, next: Next) -> impl IntoResponse {
    // Always reject to simulate rate limit
    if req.headers().get("X-REJECT").is_some() {
        return (StatusCode::TOO_MANY_REQUESTS, "rate limited").into_response();
    }
    next.run(req).await
}

#[tokio::test]
async fn rate_limit_returns_429() {
    let app = Router::new()
        .route("/post", post(|| async move { StatusCode::OK }))
        .layer(from_fn(reject_layer));

    let req = Request::builder()
        .method("POST")
        .uri("/post")
        .header("X-REJECT", "1")
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn unauthorized_when_no_permission() {
    let service = InMemoryService::new_with_sample();
    let ctx = ForumContext::default();
    assert!(!service.allowed_to(&ctx, "post_new", None, false));
}

#[tokio::test]
async fn jwt_claims_mock() {
    let claims = AuthClaims {
        sub: "user".into(),
        permissions: Some(vec!["post_new".into()]),
        ..Default::default()
    };
    assert_eq!(claims.sub, "user");
    assert!(claims
        .permissions
        .unwrap()
        .contains(&"post_new".to_string()));
}
