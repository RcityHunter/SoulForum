use axum::{
    body::Body,
    http::{header::HeaderName, HeaderValue, Method, Request},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use dotenvy::dotenv;
use std::{env, net::SocketAddr, sync::Arc, time::Instant};
use tower_http::trace::TraceLayer;
use tracing::info;
use tracing_subscriber::EnvFilter;

use btc_forum_rust::{
    rainbow_auth::RainbowAuthClient,
    services::surreal::SurrealService,
    surreal::{connect_from_env, SurrealForumService},
};
use tower_http::cors::CorsLayer;

#[path = "../api/mod.rs"]
mod api;

use api::admin_routes::{
    admin_notify, apply_ban, get_board_access, get_board_permissions, list_action_logs,
    list_admins, list_bans, list_groups, list_users, revoke_ban, update_board_access,
    update_board_permissions,
};
use api::attachment_routes::{
    create_attachment_meta, delete_attachment_api, list_attachments, serve_upload,
    upload_attachment,
};
use api::auth_routes::{auth_me, login, register};
use api::demo_routes::{demo_post, demo_surreal, health, metrics, surreal_post, ui};
use api::forum_routes::{
    create_surreal_board, create_surreal_topic, create_surreal_topic_post,
    list_surreal_posts_for_topic, list_surreal_topics, surreal_boards, surreal_posts,
};
use api::guards::verify_csrf;
use api::notification_routes::{create_notification, list_notifications, mark_notification_read};
use api::personal_message_routes::{
    delete_personal_messages_api, list_personal_messages, mark_personal_messages_read,
    send_personal_message_api,
};
use api::state::{
    csrf_enabled, find_csrf_cookie, generate_csrf_token, rainbow_auth_base_url, AppState,
    RateLimiter,
};

fn validate_config() {
    let has_secret = env::var("JWT_SECRET").is_ok();
    let has_pub = env::var("JWT_PUBLIC_KEY_PEM").is_ok();
    if !has_secret && !has_pub {
        panic!("either JWT_SECRET or JWT_PUBLIC_KEY_PEM must be set for JWT validation");
    }
    if has_pub {
        tracing::warn!("JWT_PUBLIC_KEY_PEM is set; Rainbow-Auth uses HS256, so prefer JWT_SECRET");
    }
    if !csrf_enabled() {
        tracing::warn!("ENFORCE_CSRF=0 (CSRF protection disabled)");
    }
    if env::var("SURREAL_ENDPOINT")
        .ok()
        .map(|v| v.is_empty())
        .unwrap_or(false)
    {
        panic!("SURREAL_ENDPOINT cannot be empty");
    }
    if rainbow_auth_base_url().trim().is_empty() {
        panic!("RAINBOW_AUTH_BASE_URL cannot be empty");
    }
}

async fn csrf_layer(mut req: Request<Body>, next: Next) -> Response {
    let csrf_on = csrf_enabled();
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let mut set_cookie: Option<String> = None;

    if csrf_on {
        // Issue a token cookie for safe methods to reduce friction on first load.
        if matches!(method, Method::GET | Method::OPTIONS)
            && find_csrf_cookie(req.headers()).is_none()
        {
            set_cookie = Some(generate_csrf_token());
        }

        if !matches!(method, Method::GET | Method::OPTIONS) && !path.starts_with("/auth/") {
            let headers = req.headers().clone();
            if let Err(err) = verify_csrf(&headers) {
                return err.into_response();
            }
            req.extensions_mut().insert(headers);
        }
    }

    let mut response = next.run(req).await;
    if let Some(token) = set_cookie {
        if let Ok(value) =
            HeaderValue::from_str(&format!("XSRF-TOKEN={}; Path=/; SameSite=Lax", token))
        {
            response
                .headers_mut()
                .append(axum::http::header::SET_COOKIE, value);
        }
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{HeaderMap, HeaderValue, Method, Request, StatusCode},
        middleware::from_fn,
        routing::post,
        Router,
    };
    use btc_forum_rust::auth::AuthClaims;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::time::Duration;
    use tower::ServiceExt;

    #[test]
    fn validate_content_ok() {
        assert!(api::guards::validate_content("hello", "body").is_ok());
    }

    #[test]
    fn validate_content_empty_subject_err() {
        assert!(api::guards::validate_content("", "body").is_err());
    }

    #[test]
    fn validate_content_empty_body_err() {
        assert!(api::guards::validate_content("hello", " ").is_err());
    }

    #[test]
    fn require_auth_rejects_missing_claims() {
        let result = api::auth::require_auth(&None);
        assert!(result.is_err());
    }

    #[test]
    fn csrf_mismatch_rejected() {
        let mut headers = HeaderMap::new();
        headers.insert("x-csrf-token", HeaderValue::from_static("abc"));
        headers.insert(
            axum::http::header::COOKIE,
            HeaderValue::from_static("XSRF-TOKEN=def"),
        );
        assert!(api::guards::verify_csrf(&headers).is_err());
    }

    #[test]
    fn rate_limiter_hits_limit() {
        let limiter = api::state::RateLimiter::new();
        let key = "user1";
        assert!(limiter.allow(key, 2, Duration::from_secs(60)));
        assert!(limiter.allow(key, 2, Duration::from_secs(60)));
        assert!(!limiter.allow(key, 2, Duration::from_secs(60)));
    }

    #[test]
    fn rate_key_with_ip() {
        let claims = AuthClaims {
            sub: "alice".into(),
            ..Default::default()
        };
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
        let key = rate_key(&claims, Some(&addr));
        assert!(key.contains("alice"));
        assert!(key.contains("127.0.0.1"));
    }

    #[tokio::test]
    async fn csrf_layer_blocks_missing_token() {
        let app = Router::new()
            .route("/test", post(|| async { StatusCode::OK }))
            .layer(from_fn(csrf_layer));
        let req = Request::builder()
            .method(Method::POST)
            .uri("/test")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn csrf_layer_allows_with_token() {
        let app = Router::new()
            .route("/test", post(|| async { StatusCode::OK }))
            .layer(from_fn(csrf_layer));
        let req = Request::builder()
            .method(Method::POST)
            .uri("/test")
            .header("x-csrf-token", "abc")
            .header(axum::http::header::COOKIE, "XSRF-TOKEN=abc")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(req).await.unwrap();
        assert_ne!(response.status(), StatusCode::FORBIDDEN);
    }

    fn rate_key(claims: &AuthClaims, addr: Option<&SocketAddr>) -> String {
        if let Some(ip) = addr {
            format!("{}:{}", claims.sub, ip.ip())
        } else {
            claims.sub.clone()
        }
    }
}

#[tokio::main]
async fn main() {
    dotenv().ok();
    validate_config();
    init_tracing();

    let surreal = connect_from_env()
        .await
        .expect("failed to connect to SurrealDB");
    let surreal = SurrealForumService::new(surreal);
    let forum_service = SurrealService::new(surreal.client().clone());
    let rainbow_auth = RainbowAuthClient::new(rainbow_auth_base_url());
    let cors_origin = env::var("CORS_ORIGIN")
        .unwrap_or_else(|_| "http://127.0.0.1:8081,http://forum.local".to_string());
    let cors_origins: Vec<HeaderValue> = cors_origin
        .split(',')
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(|value| value.parse::<HeaderValue>().expect("invalid CORS_ORIGIN"))
        .collect();
    let state = AppState {
        surreal,
        forum_service,
        rainbow_auth,
        rate_limiter: Arc::new(RateLimiter::new()),
        start_time: Instant::now(),
    };
    let app = Router::new()
        .route("/health", get(health))
        .route("/metrics", get(metrics))
        .route("/auth/register", post(register))
        .route("/auth/login", post(login))
        .route("/auth/me", get(auth_me))
        .route("/ui", get(ui))
        .route("/demo/post", post(demo_post))
        .route("/demo/surreal", post(demo_surreal))
        .route("/surreal/post", post(surreal_post))
        .route("/surreal/posts", get(surreal_posts))
        .route(
            "/surreal/notifications",
            get(list_notifications).post(create_notification),
        )
        .route(
            "/surreal/notifications/mark_read",
            post(mark_notification_read),
        )
        .route(
            "/surreal/attachments",
            get(list_attachments).post(create_attachment_meta),
        )
        .route("/surreal/attachments/delete", post(delete_attachment_api))
        .route("/surreal/attachments/upload", post(upload_attachment))
        .route("/uploads/*path", get(serve_upload))
        .route("/surreal/personal_messages", get(list_personal_messages))
        .route(
            "/surreal/personal_messages/send",
            post(send_personal_message_api),
        )
        .route(
            "/surreal/personal_messages/read",
            post(mark_personal_messages_read),
        )
        .route(
            "/surreal/personal_messages/delete",
            post(delete_personal_messages_api),
        )
        .route(
            "/surreal/boards",
            get(surreal_boards).post(create_surreal_board),
        )
        .route(
            "/surreal/topics",
            get(list_surreal_topics).post(create_surreal_topic),
        )
        .route(
            "/surreal/topic/posts",
            get(list_surreal_posts_for_topic).post(create_surreal_topic_post),
        )
        .route("/admin/users", get(list_users))
        .route("/admin/admins", get(list_admins))
        .route("/admin/groups", get(list_groups))
        .route("/admin/bans", get(list_bans))
        .route("/admin/action_logs", get(list_action_logs))
        .route("/admin/bans/apply", post(apply_ban))
        .route("/admin/bans/revoke", post(revoke_ban))
        .route("/admin/notify", post(admin_notify))
        .route(
            "/admin/board_access",
            get(get_board_access).post(update_board_access),
        )
        .route(
            "/admin/board_permissions",
            get(get_board_permissions).post(update_board_permissions),
        )
        .layer(axum::middleware::from_fn(csrf_layer))
        .layer({
            let origins = if cors_origins.is_empty() {
                vec!["http://127.0.0.1:8081"
                    .parse::<HeaderValue>()
                    .expect("invalid default CORS origin")]
            } else {
                cors_origins.clone()
            };
            CorsLayer::new()
                .allow_origin(origins)
                .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
                .allow_headers([
                    axum::http::header::AUTHORIZATION,
                    axum::http::header::CONTENT_TYPE,
                    HeaderName::from_static("x-csrf-token"),
                ])
                .allow_credentials(true)
        })
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr: SocketAddr = env::var("BIND_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:3000".into())
        .parse()
        .expect("invalid BIND_ADDR, expected host:port");
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind HTTP listener");
    info!("API listening on http://{addr}");

    let app = app.into_make_service_with_connect_info::<SocketAddr>();
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("server crashed");
}

fn init_tracing() {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(env_filter).init();
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut terminate =
            signal(SignalKind::terminate()).expect("failed to install SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = terminate.recv() => {},
        }
    }

    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    }
}
