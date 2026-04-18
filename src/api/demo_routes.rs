use axum::{
    extract::{ConnectInfo, State},
    http::StatusCode,
    response::{Html, IntoResponse},
    Json,
};
use std::{net::SocketAddr, time::Duration};
use tracing::error;

use btc_forum_rust::{auth::AuthClaims, services::ForumService};
use btc_forum_shared::{
    CreateSurrealPostPayload, DemoPostResponse, DemoSurrealResponse, HealthResponse,
    HealthSurrealStatus, MetricsResponse, PostResponse,
};

use super::{
    auth::{ensure_user_ctx, require_auth},
    error::api_error_from_status,
    guards::{
        enforce_rate, ensure_board_access, ensure_permission, ensure_permission_for_board,
        validate_content, verify_csrf,
    },
    state::{run_forum_blocking, AppState},
    utils::sanitize_input,
};

pub(crate) async fn health(State(state): State<AppState>) -> impl IntoResponse {
    let surreal_status = match state.surreal.health().await {
        Ok(_) => HealthSurrealStatus {
            status: "ok".to_string(),
            message: None,
        },
        Err(err) => {
            error!(error = %err, "surreal connectivity check failed");
            HealthSurrealStatus {
                status: "error".to_string(),
                message: Some(err.to_string()),
            }
        }
    };

    (
        StatusCode::OK,
        Json(HealthResponse {
            service: "ok (surreal-only)".to_string(),
            surreal: surreal_status,
            timestamp: chrono::Utc::now().to_rfc3339(),
        }),
    )
}

pub(crate) async fn metrics(State(state): State<AppState>) -> impl IntoResponse {
    let uptime = state.start_time.elapsed().as_secs();
    let rates = state.rate_limiter.snapshot();
    (
        StatusCode::OK,
        Json(MetricsResponse {
            status: "ok".to_string(),
            uptime_secs: uptime,
            rate_limiter_keys: rates,
        }),
    )
}

pub(crate) async fn ui() -> Html<&'static str> {
    Html(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <title>Forum Demo</title>
  <style>
    body { font-family: Arial, sans-serif; max-width: 720px; margin: 40px auto; line-height: 1.6; }
    label { display: block; margin-top: 12px; }
    textarea, input { width: 100%; padding: 8px; }
    button { margin-top: 12px; padding: 10px 16px; cursor: pointer; }
    pre { background: #f4f4f4; padding: 12px; border-radius: 6px; }
  </style>
</head>
<body>
  <h1>Forum Demo</h1>
  <p>Paste a JWT (from Rainbow-Auth) and post a sample message via the demo endpoint.</p>
  <label>JWT Bearer Token</label>
  <textarea id="token" rows="3" placeholder="eyJhbGciOi..."></textarea>
  <button id="send">Send Demo Post</button>
  <pre id="output">Waiting...</pre>
  <script>
    const btn = document.getElementById('send');
    const out = document.getElementById('output');
    btn.onclick = async () => {
      const token = document.getElementById('token').value.trim();
      if (!token) {
        out.textContent = 'Please provide a JWT token.';
        return;
      }
      out.textContent = 'Sending...';
      try {
        const res = await fetch('/demo/post', {
          method: 'POST',
          headers: {
            'Authorization': 'Bearer ' + token
          }
        });
        const text = await res.text();
        out.textContent = text;
      } catch (err) {
        out.textContent = 'Error: ' + err;
      }
    };
  </script>
</body>
</html>"#,
    )
}

pub(crate) async fn demo_surreal(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c,
        Err(resp) => return resp.into_response(),
    };
    let (user, _) = match ensure_user_ctx(&state, claims).await {
        Ok(value) => value,
        Err(resp) => return resp.into_response(),
    };
    let author = user.name.clone();
    match state
        .surreal
        .create_demo_post(
            "Surreal demo",
            "Hello from SurrealDB demo endpoint",
            &author,
        )
        .await
    {
        Ok(record) => (
            StatusCode::OK,
            Json(DemoSurrealResponse {
                status: "ok".to_string(),
                record,
            }),
        )
            .into_response(),
        Err(err) => {
            error!(error = %err, "surreal demo failed");
            api_error_from_status(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
                .into_response()
        }
    }
}

fn to_post(post: btc_forum_rust::surreal::SurrealPost) -> btc_forum_shared::Post {
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

pub(crate) async fn surreal_post(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<CreateSurrealPostPayload>,
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
    match state
        .surreal
        .create_post(
            &sanitize_input(&payload.subject),
            &sanitize_input(&payload.body),
            &author,
        )
        .await
    {
        Ok(post) => (
            StatusCode::CREATED,
            Json(PostResponse {
                status: "ok".to_string(),
                post: to_post(post),
            }),
        )
            .into_response(),
        Err(err) => {
            error!(error = %err, "failed to create surreal post");
            api_error_from_status(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
                .into_response()
        }
    }
}

pub(crate) async fn demo_post(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c.clone(),
        Err(resp) => return resp.into_response(),
    };
    let (user, ctx) = match ensure_user_ctx(&state, &claims).await {
        Ok(value) => value,
        Err(resp) => return resp.into_response(),
    };
    if let Err(resp) = enforce_rate(&state, &claims.sub, 20, Duration::from_secs(60)) {
        return resp.into_response();
    }
    if let Err(resp) = enforce_rate(&state, &claims.sub, 30, Duration::from_secs(60)) {
        return resp.into_response();
    }
    let author = user.name.clone();
    if let Err(resp) = ensure_permission(&state, &ctx, "post_new") {
        return resp.into_response();
    }

    let submission = btc_forum_rust::services::PostSubmission {
        topic_id: None,
        board_id: 0,
        message_id: None,
        subject: "API example".into(),
        body: "Hello from Axum demo endpoint".into(),
        icon: "xx".into(),
        approved: true,
        send_notifications: false,
    };
    match run_forum_blocking(&state, move |forum| forum.persist_post(&ctx, submission)).await {
        Ok(posted) => (
            StatusCode::OK,
            Json(DemoPostResponse {
                status: "ok".to_string(),
                topic_id: posted.topic_id,
                post_id: posted.message_id,
                author,
            }),
        )
            .into_response(),
        Err(err) => api_error_from_status(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
            .into_response(),
    }
}
