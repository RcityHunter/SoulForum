use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::{ConnectInfo, Multipart, Path, Query, State},
    http::{HeaderMap, HeaderValue, Method, Request, StatusCode, header::{HeaderName, AUTHORIZATION}},
    middleware::Next,
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use chrono::Utc;
use dotenvy::dotenv;
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    env,
    net::SocketAddr,
    path::PathBuf,
    sync::Arc,
    sync::OnceLock,
    time::{Duration, Instant},
};
use tokio::fs;
use tower_http::trace::TraceLayer;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use btc_forum_rust::{
    auth::AuthClaims,
    services::{BoardAccessEntry, ForumError},
    rainbow_auth::{RainbowAuthClient, RainbowAuthError, RainbowUser},
    security::load_permissions,
    services::{
        BanAffects, BanCondition, BanRule, ForumContext, ForumService, PersonalMessageFolder,
        SendPersonalMessage, surreal::SurrealService,
    },
    surreal::{SurrealClient, SurrealForumService, SurrealPost, SurrealTopic, SurrealUser, connect_from_env},
};
use tower_http::cors::CorsLayer;

#[derive(Clone)]
struct AppState {
    surreal: SurrealForumService,
    forum_service: SurrealService,
    rainbow_auth: RainbowAuthClient,
    rate_limiter: Arc<RateLimiter>,
    start_time: Instant,
}

#[derive(Default)]
struct RateLimiter {
    // key -> (count, window_start)
    limits: std::sync::Mutex<std::collections::HashMap<String, (u32, Instant)>>,
}

impl RateLimiter {
    fn new() -> Self {
        Self {
            limits: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }

    fn allow(&self, key: &str, max: u32, window: Duration) -> bool {
        let mut guard = match self.limits.lock() {
            Ok(g) => g,
            Err(poison) => poison.into_inner(),
        };
        let now = Instant::now();
        let entry = guard.entry(key.to_string()).or_insert((0, now));
        let elapsed = now.duration_since(entry.1);
        if elapsed >= window {
            *entry = (1, now);
            true
        } else if entry.0 < max {
            entry.0 += 1;
            true
        } else {
            false
        }
    }

    fn snapshot(&self) -> std::collections::HashMap<String, u32> {
        let guard = match self.limits.lock() {
            Ok(g) => g,
            Err(poison) => poison.into_inner(),
        };
        guard
            .iter()
            .map(|(k, (count, _))| (k.clone(), *count))
            .collect()
    }
}

#[derive(Deserialize)]
struct RegisterRequest {
    #[serde(alias = "username")]
    email: String,
    password: String,
    role: Option<String>,
    permissions: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct LoginRequest {
    #[serde(alias = "username")]
    email: String,
    password: String,
}

static ENFORCE_CSRF: OnceLock<bool> = OnceLock::new();
static MAX_UPLOAD_MB: OnceLock<i64> = OnceLock::new();
static ALLOWED_MIME: OnceLock<Option<Vec<String>>> = OnceLock::new();

fn csrf_enabled() -> bool {
    *ENFORCE_CSRF.get_or_init(|| {
        env::var("ENFORCE_CSRF")
            .map(|v| !matches!(v.to_lowercase().as_str(), "0" | "false" | "off"))
            .unwrap_or(true)
    })
}

fn upload_dir() -> PathBuf {
    PathBuf::from(env::var("UPLOAD_DIR").unwrap_or_else(|_| "uploads".into()))
}

fn upload_base_url() -> String {
    env::var("UPLOAD_BASE_URL").unwrap_or_else(|_| "/uploads".into())
}

fn rainbow_auth_base_url() -> String {
    env::var("RAINBOW_AUTH_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".into())
}

fn max_upload_bytes() -> i64 {
    *MAX_UPLOAD_MB.get_or_init(|| {
        env::var("MAX_UPLOAD_MB")
            .ok()
            .and_then(|v| v.parse().ok())
            .filter(|v| *v > 0)
            .unwrap_or(50)
    }) * 1024 * 1024
}

fn allowed_mime() -> Option<&'static [String]> {
    ALLOWED_MIME
        .get_or_init(|| {
            env::var("ALLOWED_MIME")
                .ok()
                .map(|v| {
                    v.split(',')
                        .map(|s| s.trim().to_lowercase())
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                })
        })
        .as_deref()
}

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
    if env::var("SURREAL_ENDPOINT").ok().map(|v| v.is_empty()).unwrap_or(false) {
        panic!("SURREAL_ENDPOINT cannot be empty");
    }
    if rainbow_auth_base_url().trim().is_empty() {
        panic!("RAINBOW_AUTH_BASE_URL cannot be empty");
    }
}

fn require_auth(claims: &Option<AuthClaims>) -> Result<&AuthClaims, impl IntoResponse> {
    if let Some(claims) = claims {
        Ok(claims)
    } else {
        Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({"status": "error", "message": "authorization required"})),
        ))
    }
}

fn bearer_from_headers(headers: &HeaderMap) -> Option<String> {
    let header = headers.get(AUTHORIZATION)?;
    let value = header.to_str().ok()?.trim();
    let token = value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))?;
    let token = token.trim();
    if token.is_empty() { None } else { Some(token.to_string()) }
}

fn build_ctx_from_user(user: &SurrealUser, claims: &AuthClaims) -> ForumContext {
    let mut ctx = ForumContext::default();
    ctx.user_info.is_guest = false;
    ctx.user_info.name = user.name.clone();
    ctx.user_info.id = user.legacy_id();

    if let Some(role) = user.role.as_deref().or_else(|| claims.role.as_deref()) {
        match role {
            "admin" => ctx.user_info.is_admin = true,
            "mod" => ctx.user_info.is_mod = true,
            _ => {}
        }
    }

    if let Some(perms) = user
        .permissions
        .clone()
        .or_else(|| claims.permissions.clone())
    {
        ctx.user_info.permissions.extend(perms);
    }

    if ctx.user_info.permissions.is_empty() && !ctx.user_info.is_admin && !ctx.user_info.is_mod {
        ctx.user_info.permissions.insert("post_new".into());
        ctx.user_info.permissions.insert("post_reply_any".into());
    }

    if ctx.user_info.groups.is_empty() {
        if ctx.user_info.is_admin {
            ctx.user_info.groups.push(0);
        } else if ctx.user_info.is_mod {
            ctx.user_info.groups.extend([2, 1]);
        } else {
            ctx.user_info.groups.push(1);
        }
    }

    ctx
}

async fn ensure_user_ctx(
    state: &AppState,
    claims: &AuthClaims,
) -> Result<(SurrealUser, ForumContext), (StatusCode, Json<serde_json::Value>)> {
    let resolved_user = resolve_rainbow_user(state, claims).await;
    let name = resolved_user
        .as_ref()
        .map(|user| user.email.as_str())
        .unwrap_or(&claims.sub);
    match state
        .surreal
        .ensure_user(
            name,
            claims.role.as_deref(),
            claims.permissions.as_deref(),
        )
        .await
    {
        Ok(user) => {
            if let Some(user_info) = resolved_user {
                let mut user = user;
                user.name = user_info.email;
                let ctx = build_ctx_from_user(&user, claims);
                return Ok((user, ctx));
            }
            let ctx = build_ctx_from_user(&user, claims);
            Ok((user, ctx))
        }
        Err(err) => {
            error!(error = %err, "failed to ensure user");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"status": "error", "message": "failed to ensure user"})),
            ))
        }
    }
}

async fn resolve_rainbow_user(
    state: &AppState,
    claims: &AuthClaims,
) -> Option<RainbowUser> {
    let token = claims.token.as_deref()?;
    match state.rainbow_auth.me(token).await {
        Ok(user) => Some(user),
        Err(err) => {
            error!(error = %err, "rainbow-auth user lookup failed");
            None
        }
    }
}

fn ensure_permission(
    state: &AppState,
    ctx: &ForumContext,
    permission: &str,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if state.forum_service.allowed_to(ctx, permission, None, false) {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            Json(json!({
                "status": "error",
                "message": format!("missing permission: {permission}")
            })),
        ))
    }
}

fn rainbow_auth_error_response(
    err: RainbowAuthError,
) -> (StatusCode, Json<serde_json::Value>) {
    match err {
        RainbowAuthError::Http { status, message } => (
            status,
            Json(json!({"status": "error", "message": message})),
        ),
        RainbowAuthError::Transport(message) | RainbowAuthError::Parse(message) => (
            StatusCode::BAD_GATEWAY,
            Json(json!({"status": "error", "message": message})),
        ),
    }
}

fn ensure_permission_for_board(
    state: &AppState,
    ctx: &ForumContext,
    permission: &str,
    board_id: Option<&str>,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    let mut working = ctx.clone();
    if let Some(board) = board_id {
        if let Err(err) = load_permissions(&state.forum_service, &mut working, Some(board.to_string())) {
            error!(error = %err, "failed to load permissions");
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"status": "error", "message": "failed to load permissions"})),
            ));
        }
    }
    ensure_permission(state, &working, permission)
}

fn user_groups(ctx: &ForumContext) -> Vec<i64> {
    if !ctx.user_info.groups.is_empty() {
        return ctx.user_info.groups.clone();
    }
    if ctx.user_info.is_admin {
        return vec![0];
    }
    if ctx.user_info.is_mod {
        return vec![2, 1];
    }
    vec![1]
}

async fn load_board_access(state: &AppState) -> Result<Vec<BoardAccessEntry>, ForumError> {
    let forum_service = state.forum_service.clone();
    tokio::task::spawn_blocking(move || forum_service.list_board_access())
        .await
        .map_err(|e| ForumError::Internal(format!("board access task failed: {e}")))?
}

async fn run_forum_blocking<R, F>(state: &AppState, job: F) -> Result<R, ForumError>
where
    R: Send + 'static,
    F: FnOnce(SurrealService) -> Result<R, ForumError> + Send + 'static,
{
    let forum_service = state.forum_service.clone();
    tokio::task::spawn_blocking(move || job(forum_service))
        .await
        .map_err(|e| ForumError::Internal(format!("forum task failed: {e}")))?
}

async fn ensure_board_access(
    state: &AppState,
    ctx: &ForumContext,
    board_id: &str,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if ctx.user_info.is_admin {
        return Ok(());
    }
    let entries: Vec<BoardAccessEntry> = match load_board_access(state).await {
        Ok(entries) => entries,
        Err(err) => {
            error!(error = %err, "failed to load board access");
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"status": "error", "message": "failed to load board access"})),
            ));
        }
    };
    let Some(entry) = entries.iter().find(|e| e.id == board_id) else {
        return Ok(()); // no explicit rule: allow
    };
    if entry.allowed_groups.is_empty() {
        return Ok(());
    }
    let groups = user_groups(ctx);
    if entry
        .allowed_groups
        .iter()
        .any(|gid| groups.iter().any(|g| g == gid))
    {
        return Ok(());
    }
    Err((
        StatusCode::FORBIDDEN,
        Json(json!({"status": "error", "message": "board access denied"})),
    ))
}

async fn fetch_topic_board_id(client: &SurrealClient, topic_id: &str) -> Option<String> {
    let topic_id_owned = topic_id.to_string();
    let mut response = client
        .query(
            r#"
            SELECT board_id FROM type::thing("topics", $id) LIMIT 1;
            "#,
        )
        .bind(("id", topic_id_owned))
        .await
        .ok()?;
    #[derive(Deserialize)]
    struct Row {
        board_id: Option<String>,
    }
    let rows: Vec<Row> = response.take(0).ok()?;
    rows.into_iter().find_map(|r| r.board_id)
}

fn ensure_admin(ctx: &ForumContext) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if ctx.user_info.is_admin || ctx.user_info.permissions.contains("admin") {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            Json(json!({"status": "error", "message": "admin permission required"})),
        ))
    }
}

fn enforce_rate(
    state: &AppState,
    key: &str,
    limit: u32,
    window: Duration,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if state.rate_limiter.allow(key, limit, window) {
        Ok(())
    } else {
        Err((
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({
                "status": "error",
                "message": "rate limit exceeded"
            })),
        ))
    }
}

fn rate_key(claims: &AuthClaims, addr: Option<&std::net::SocketAddr>) -> String {
    if let Some(ip) = addr {
        format!("{}:{}", claims.sub, ip.ip())
    } else {
        claims.sub.clone()
    }
}

fn generate_csrf_token() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn find_csrf_cookie(headers: &HeaderMap) -> Option<String> {
    headers
        .get(axum::http::header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|cookie| {
            cookie
                .split(';')
                .find_map(|part| part.trim().strip_prefix("XSRF-TOKEN="))
                .map(|v| v.to_string())
        })
}

fn verify_csrf(headers: &HeaderMap) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    // Simple double-submit style check: X-CSRF-TOKEN must equal Cookie XSRF-TOKEN.
    let header_token = headers
        .get("x-csrf-token")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    if header_token.is_empty() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"status": "error", "message": "missing csrf token"})),
        ));
    }
    if let (Some(header_token), Some(cookie_header)) = (
        headers.get("x-csrf-token"),
        headers.get(axum::http::header::COOKIE),
    ) {
        let header_val = header_token.to_str().unwrap_or_default();
        let cookie_val = cookie_header.to_str().unwrap_or_default();
        let mut ok = false;
        for part in cookie_val.split(';') {
            let trimmed = part.trim();
            if let Some(rest) = trimmed.strip_prefix("XSRF-TOKEN=") {
                if rest == header_val {
                    ok = true;
                    break;
                }
            }
        }
        if !ok {
            return Err((
                StatusCode::FORBIDDEN,
                Json(json!({"status": "error", "message": "csrf token mismatch"})),
            ));
        }
    }
    Ok(())
}
fn validate_content(
    subject: &str,
    body: &str,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    let s = subject.trim();
    let b = body.trim();
    if s.is_empty() || s.len() > 200 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"status": "error", "message": "subject must be 1..200 chars"})),
        ));
    }
    if b.is_empty() || b.len() > 10_000 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"status": "error", "message": "body must be 1..10000 chars"})),
        ));
    }
    Ok(())
}

async fn csrf_layer(mut req: Request<Body>, next: Next) -> Response {
    let csrf_on = csrf_enabled();
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let mut set_cookie: Option<String> = None;

    if csrf_on {
        // Issue a token cookie for safe methods to reduce friction on first load.
        if matches!(method, Method::GET | Method::OPTIONS) && find_csrf_cookie(req.headers()).is_none()
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

fn sanitize_input(input: &str) -> String {
    ammonia::Builder::default()
        .url_schemes(["http", "https"].into())
        .clean(input)
        .to_string()
}

fn sanitize_filename(name: &str) -> String {
    let base = name
        .rsplit(|c| c == '/' || c == '\\')
        .next()
        .unwrap_or(name);
    let mut cleaned: String = base
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '-' || *c == '_')
        .collect();
    if cleaned.is_empty() {
        cleaned = format!("upload-{}.bin", Utc::now().timestamp_millis());
    }
    cleaned
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, http::Request, middleware::from_fn, routing::post};
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use tower::ServiceExt;

    #[test]
    fn validate_content_ok() {
        assert!(validate_content("hello", "body").is_ok());
    }

    #[test]
    fn validate_content_empty_subject_err() {
        assert!(validate_content("", "body").is_err());
    }

    #[test]
    fn validate_content_empty_body_err() {
        assert!(validate_content("hello", " ").is_err());
    }

    #[test]
    fn require_auth_rejects_missing_claims() {
        let result = require_auth(&None);
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
        assert!(verify_csrf(&headers).is_err());
    }

    #[test]
    fn rate_limiter_hits_limit() {
        let limiter = RateLimiter::new();
        let key = "user1";
        assert!(limiter.allow(key, 2, Duration::from_secs(60)));
        assert!(limiter.allow(key, 2, Duration::from_secs(60)));
        assert!(!limiter.allow(key, 2, Duration::from_secs(60)));
    }

    #[test]
    fn rate_key_with_ip() {
        let claims = AuthClaims {
            sub: "alice".into(),
            exp: 0,
            iat: 0,
            session_id: None,
            role: None,
            permissions: None,
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
    let cors_origin =
        env::var("CORS_ORIGIN").unwrap_or_else(|_| "http://127.0.0.1:8080".to_string());
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
        .route("/surreal/notifications", get(list_notifications).post(create_notification))
        .route(
            "/surreal/notifications/mark_read",
            post(mark_notification_read),
        )
        .route(
            "/surreal/attachments",
            get(list_attachments).post(create_attachment_meta),
        )
        .route(
            "/surreal/attachments/delete",
            post(delete_attachment_api),
        )
        .route(
            "/surreal/attachments/upload",
            post(upload_attachment),
        )
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
        .route("/admin/bans", get(list_bans))
        .route("/admin/action_logs", get(list_action_logs))
        .route("/admin/bans/apply", post(apply_ban))
        .route("/admin/bans/revoke", post(revoke_ban))
        .route("/admin/notify", post(admin_notify))
        .route("/admin/board_access", get(get_board_access).post(update_board_access))
        .route(
            "/admin/board_permissions",
            get(get_board_permissions).post(update_board_permissions),
        )
        .layer(axum::middleware::from_fn(csrf_layer))
        .layer({
            let origin = cors_origin
                .parse::<HeaderValue>()
                .expect("invalid CORS_ORIGIN");
            CorsLayer::new()
                .allow_origin(origin)
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

async fn health(State(state): State<AppState>) -> impl IntoResponse {
    let surreal_status = match state.surreal.health().await {
        Ok(_) => json!({"status": "ok"}),
        Err(err) => {
            error!(error = %err, "surreal connectivity check failed");
            json!({"status": "error", "message": err.to_string()})
        }
    };

    (
        StatusCode::OK,
        Json(json!({
            "service": "ok (surreal-only)",
            "surreal": surreal_status,
            "timestamp": Utc::now()
        })),
    )
}

async fn metrics(State(state): State<AppState>) -> impl IntoResponse {
    let uptime = state.start_time.elapsed().as_secs();
    let rates = state.rate_limiter.snapshot();
    (
        StatusCode::OK,
        Json(json!({
            "status": "ok",
            "uptime_secs": uptime,
            "rate_limiter_keys": rates,
        })),
    )
}

async fn register(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(payload): Json<RegisterRequest>,
) -> impl IntoResponse {
    let key = format!("register:{}", addr.ip());
    if let Err(resp) = enforce_rate(&state, &key, 5, Duration::from_secs(60)) {
        return resp;
    }
    let email = payload.email.trim();
    if email.is_empty() || !email.contains('@') {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"status": "error", "message": "valid email required"})),
        );
    }
    if payload.password.len() < 6 || payload.password.len() > 128 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"status": "error", "message": "password must be 6-128 chars"})),
        );
    }

    match state
        .rainbow_auth
        .register(email, &payload.password)
        .await
    {
        Ok(message) => (
            StatusCode::OK,
            Json(json!({
                "status": "ok",
                "message": message
            })),
        ),
        Err(err) => rainbow_auth_error_response(err),
    }
}

async fn login(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(payload): Json<LoginRequest>,
) -> impl IntoResponse {
    let key = format!("login:{}", addr.ip());
    if let Err(resp) = enforce_rate(&state, &key, 10, Duration::from_secs(60)) {
        return resp;
    }
    let email = payload.email.trim();
    if email.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"status": "error", "message": "email required"})),
        );
    }

    match state
        .rainbow_auth
        .login(email, &payload.password)
        .await
    {
        Ok(login) => {
            let forum_user = match state
                .surreal
                .ensure_user(&login.user.email, None, None)
                .await
            {
                Ok(user) => user,
                Err(err) => {
                    error!(error = %err, "failed to ensure user after login");
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({"status": "error", "message": "failed to sync user"})),
                    );
                }
            };
            let member_id = forum_user.legacy_id();
            (
                StatusCode::OK,
                Json(json!({
                    "status": "ok",
                    "token": login.token,
                    "user": {
                        "name": login.user.email,
                        "role": null,
                        "permissions": Vec::<String>::new(),
                        "member_id": member_id,
                    }
                })),
            )
        }
        Err(err) => rainbow_auth_error_response(err),
    }
}

async fn auth_me(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    let Some(token) = bearer_from_headers(&headers) else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"status": "error", "message": "authorization required"})),
        );
    };

    match state.rainbow_auth.me(&token).await {
        Ok(user) => {
            let member_id = match state
                .surreal
                .ensure_user(&user.email, None, None)
                .await
            {
                Ok(forum_user) => forum_user.legacy_id(),
                Err(err) => {
                    error!(error = %err, "failed to sync user for auth/me");
                    0
                }
            };
            (
                StatusCode::OK,
                Json(json!({
                    "status": "ok",
                    "user": {
                        "name": user.email,
                        "role": null,
                        "permissions": Vec::<String>::new(),
                        "member_id": member_id,
                    }
                })),
            )
        }
        Err(err) => rainbow_auth_error_response(err),
    }
}

/// CSRF 占位：如需 CSRF 防护，可在前端携带 token（双提交或表单隐藏字段），
/// 后端通过中间件校验自定义 Header 与 Cookie 一致性，再放行路由。

async fn ui() -> Html<&'static str> {
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

async fn demo_surreal(
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
            Json(json!({
                "status": "ok",
                "record": record
            })),
        )
            .into_response(),
        Err(err) => {
            error!(error = %err, "surreal demo failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"status": "error", "message": err.to_string()})),
            )
                .into_response()
        }
    }
}

#[derive(Deserialize)]
struct CreateSurrealPost {
    board_id: String,
    subject: String,
    body: String,
}

async fn surreal_post(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(payload): Json<CreateSurrealPost>,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c,
        Err(resp) => return resp.into_response(),
    };
    let (user, ctx) = match ensure_user_ctx(&state, claims).await {
        Ok(value) => value,
        Err(resp) => return resp.into_response(),
    };
    let key = rate_key(claims, Some(&addr));
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
    if let Err(resp) = ensure_permission_for_board(&state, &ctx, "post_new", Some(&payload.board_id))
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
            Json(json!({
                "status": "ok",
                "post": post
            })),
        )
            .into_response(),
        Err(err) => {
            error!(error = %err, "failed to create surreal post");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"status": "error", "message": err.to_string()})),
            )
                .into_response()
        }
    }
}

async fn surreal_posts(
    State(state): State<AppState>,
    _claims: Option<AuthClaims>,
) -> impl IntoResponse {
    match state.surreal.list_posts().await {
        Ok(posts) => (
            StatusCode::OK,
            Json(json!({
                "status": "ok",
                "posts": posts
            })),
        )
            .into_response(),
        Err(err) => {
            error!(error = %err, "failed to list surreal posts");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"status": "error", "message": err.to_string()})),
            )
                .into_response()
        }
    }
}

#[derive(Deserialize)]
struct CreateBoardPayload {
    name: String,
    description: Option<String>,
}

async fn create_surreal_board(
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
    let key = rate_key(claims, Some(&addr));
    if let Err(resp) = enforce_rate(&state, &key, 10, Duration::from_secs(60)) {
        return resp.into_response();
    }
    if payload.name.trim().is_empty() || payload.name.trim().len() > 100 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"status": "error", "message": "name must be 1..100 chars"})),
        )
            .into_response();
    }
    if let Err(resp) = ensure_permission(&state, &ctx, "manage_boards") {
        return resp.into_response();
    }
    match state
        .surreal
        .create_board(&payload.name, payload.description.as_deref())
        .await
    {
        Ok(board) => (
            StatusCode::CREATED,
            Json(json!({"status": "ok", "board": board})),
        )
            .into_response(),
        Err(err) => {
            error!(error = %err, "failed to create board");
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"status": "error", "message": err.to_string()})),
            )
                .into_response()
        }
    }
}

async fn surreal_boards(
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
                        if let Some(rule) = rules.iter().find(|r| r.id == b.id.clone().unwrap_or_default()) {
                            if rule.allowed_groups.is_empty() {
                                return true;
                            }
                            let groups = user_groups(&ctx);
                            rule.allowed_groups.iter().any(|gid| groups.iter().any(|g| g == gid))
                        } else {
                            true
                        }
                    })
                    .collect(),
                None => boards,
            };
            (
                StatusCode::OK,
                Json(json!({"status": "ok", "boards": filtered})),
            )
                .into_response()
        }
        Err(err) => {
            error!(error = %err, "failed to list boards");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"status": "error", "message": err.to_string()})),
            )
                .into_response()
        }
    }
}

#[derive(Deserialize)]
struct CreateTopicPayload {
    board_id: String,
    subject: String,
    body: String,
}

async fn create_surreal_topic(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(payload): Json<CreateTopicPayload>,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c,
        Err(resp) => return resp.into_response(),
    };
    let (user, ctx) = match ensure_user_ctx(&state, claims).await {
        Ok(value) => value,
        Err(resp) => return resp.into_response(),
    };
    let key = rate_key(claims, Some(&addr));
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
    if let Err(resp) = ensure_permission_for_board(&state, &ctx, "post_new", Some(&payload.board_id))
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
        // create initial post inside the topic
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
        Ok((topic, post)) => (
            StatusCode::CREATED,
            Json(json!({"status": "ok", "topic": topic, "first_post": post})),
        )
            .into_response(),
        Err(err) => {
            error!(error = %err, "failed to create topic");
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"status": "error", "message": err.to_string()})),
            )
                .into_response()
        }
    }
}

#[derive(Deserialize)]
struct ListTopicsParams {
    board_id: String,
}

async fn list_surreal_topics(
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
            Json(json!({"status": "ok", "topics": topics})),
        )
            .into_response(),
        Err(err) => {
            error!(error = %err, "failed to list topics");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"status": "error", "message": err.to_string()})),
            )
                .into_response()
        }
    }
}

#[derive(Deserialize)]
struct CreatePostPayload {
    topic_id: String,
    board_id: String,
    subject: Option<String>,
    body: String,
}

async fn create_surreal_topic_post(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
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
    let key = rate_key(claims, Some(&addr));
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
    // Basic XSS mitigation by sanitizing HTML content
    if let Err(resp) =
        ensure_permission_for_board(&state, &ctx, "post_reply_any", Some(&payload.board_id))
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
        Ok(post) => (
            StatusCode::CREATED,
            Json(json!({"status": "ok", "post": post})),
        )
            .into_response(),
        Err(err) => {
            error!(error = %err, "failed to create post");
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"status": "error", "message": err.to_string()})),
            )
                .into_response()
        }
    }
}

#[derive(Deserialize)]
struct ListPostsParams {
    topic_id: String,
}

async fn list_surreal_posts_for_topic(
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
            Json(json!({"status": "ok", "posts": posts})),
        )
            .into_response(),
        Err(err) => {
            error!(error = %err, "failed to list posts");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"status": "error", "message": err.to_string()})),
            )
                .into_response()
        }
    }
}

#[derive(Deserialize)]
struct CreateNotificationPayload {
    user: Option<String>,
    subject: String,
    body: String,
}

#[derive(Deserialize)]
struct MarkNotificationPayload {
    id: String,
}

#[derive(Deserialize)]
struct CreateAttachmentPayload {
    filename: String,
    size_bytes: i64,
    mime_type: Option<String>,
    board_id: Option<String>,
    topic_id: Option<String>,
}

#[derive(Deserialize)]
struct PersonalMessageSendPayload {
    to: Vec<String>,
    subject: String,
    body: String,
}

#[derive(Deserialize)]
struct PersonalMessageIdsPayload {
    ids: Vec<i64>,
}

#[derive(Deserialize)]
struct PersonalMessageListQuery {
    folder: Option<String>,
    label: Option<i64>,
    offset: Option<usize>,
    limit: Option<usize>,
}

async fn create_notification(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(payload): Json<CreateNotificationPayload>,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c,
        Err(resp) => return resp.into_response(),
    };
    let (_user, ctx) = match ensure_user_ctx(&state, &claims).await {
        Ok(value) => value,
        Err(resp) => return resp.into_response(),
    };
    if let Err(resp) = verify_csrf(&headers) {
        return resp.into_response();
    }
    let key = format!("notify:{}", addr.ip());
    if let Err(resp) = enforce_rate(&state, &key, 20, Duration::from_secs(60)) {
        return resp.into_response();
    }
    let target_user = if ctx.user_info.is_admin {
        payload.user.unwrap_or_else(|| claims.sub.clone())
    } else {
        claims.sub.clone()
    };
    if payload.subject.trim().is_empty() || payload.subject.len() > 200 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"status": "error", "message": "subject must be 1-200 chars"})),
        )
            .into_response();
    }
    if payload.body.trim().is_empty() || payload.body.len() > 4000 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"status": "error", "message": "body must be 1-4000 chars"})),
        )
            .into_response();
    }
    match state
        .surreal
        .create_notification(&target_user, &sanitize_input(&payload.subject), &sanitize_input(&payload.body))
        .await
    {
        Ok(note) => (
            StatusCode::CREATED,
            Json(json!({"status": "ok", "notification": note})),
        )
            .into_response(),
        Err(err) => {
            error!(error = %err, "failed to create notification");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"status": "error", "message": err.to_string()})),
            )
                .into_response()
        }
    }
}

async fn mark_notification_read(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    headers: HeaderMap,
    Json(payload): Json<MarkNotificationPayload>,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c,
        Err(resp) => return resp.into_response(),
    };
    if let Err(resp) = verify_csrf(&headers) {
        return resp.into_response();
    }
    if payload.id.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"status": "error", "message": "id required"})),
        )
            .into_response();
    }
    match state
        .surreal
        .mark_notification_read(&payload.id)
        .await
    {
        Ok(_) => (
            StatusCode::OK,
            Json(json!({"status": "ok", "id": payload.id, "user": claims.sub})),
        )
            .into_response(),
        Err(err) => {
            error!(error = %err, "failed to mark notification read");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"status": "error", "message": err.to_string()})),
            )
                .into_response()
        }
    }
}

async fn list_attachments(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c,
        Err(resp) => return resp.into_response(),
    };
    let base_url = upload_base_url();
    match state.surreal.list_attachments_for_user(&claims.sub).await {
        Ok(items) => (
            StatusCode::OK,
            Json(json!({"status": "ok", "attachments": items, "base_url": base_url})),
        )
            .into_response(),
        Err(err) => {
            error!(error = %err, "failed to list attachments");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"status": "error", "message": err.to_string()})),
            )
                .into_response()
        }
    }
}

async fn create_attachment_meta(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(payload): Json<CreateAttachmentPayload>,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c,
        Err(resp) => return resp.into_response(),
    };
    if let Err(resp) = verify_csrf(&headers) {
        return resp.into_response();
    }
    let key = format!("attach:{}", addr.ip());
    if let Err(resp) = enforce_rate(&state, &key, 30, Duration::from_secs(60)) {
        return resp.into_response();
    }
    if payload.filename.trim().is_empty() || payload.filename.len() > 255 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"status": "error", "message": "filename must be 1-255 chars"})),
        )
            .into_response();
    }
    if payload.size_bytes < 0 || payload.size_bytes > max_upload_bytes() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"status": "error", "message": "size_bytes invalid"})),
        )
            .into_response();
    }
    if let Some(list) = allowed_mime() {
        if let Some(mt) = payload.mime_type.as_deref() {
            let mt_lower = mt.to_lowercase();
            if !list.iter().any(|allowed| mt_lower.starts_with(allowed)) {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"status": "error", "message": "mime_type not allowed"})),
                )
                    .into_response();
            }
        }
    }
    let base_url = upload_base_url();
    match state
        .surreal
        .create_attachment_meta(
            &claims.sub,
            &payload.filename,
            payload.size_bytes,
            payload.mime_type.as_deref(),
            payload.board_id.as_deref(),
            payload.topic_id.as_deref(),
        )
        .await
    {
        Ok(att) => (
            StatusCode::CREATED,
            Json(json!({"status": "ok", "attachment": att, "base_url": base_url})),
        )
            .into_response(),
        Err(err) => {
            error!(error = %err, "failed to create attachment meta");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"status": "error", "message": err.to_string()})),
            )
                .into_response()
        }
    }
}

async fn upload_attachment(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c,
        Err(resp) => return resp.into_response(),
    };
    if let Err(resp) = verify_csrf(&headers) {
        return resp.into_response();
    }
    let key = format!("attach_upload:{}", addr.ip());
    if let Err(resp) = enforce_rate(&state, &key, 10, Duration::from_secs(60)) {
        return resp.into_response();
    }

    let mut file_bytes: Option<Bytes> = None;
    let mut file_name: Option<String> = None;
    let mut mime: Option<String> = None;
    let mut board_id: Option<String> = None;
    let mut topic_id: Option<String> = None;

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().map(|s| s.to_string());
        match name.as_deref() {
            Some("file") => {
                file_name = field.file_name().map(|s| s.to_string());
                mime = field.content_type().map(|s| s.to_string());
                match field.bytes().await {
                    Ok(bytes) => file_bytes = Some(bytes),
                    Err(err) => {
                        error!(error = %err, "failed to read upload");
                        return (
                            StatusCode::BAD_REQUEST,
                            Json(json!({"status": "error", "message": "failed to read upload"})),
                        )
                            .into_response();
                    }
                }
            }
            Some("board_id") => {
                if let Ok(text) = field.text().await {
                    board_id = Some(text);
                }
            }
            Some("topic_id") => {
                if let Ok(text) = field.text().await {
                    topic_id = Some(text);
                }
            }
            _ => {}
        }
    }

    let Some(bytes) = file_bytes else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"status": "error", "message": "missing file field"})),
        )
            .into_response();
    };
    let raw_name = file_name.unwrap_or_else(|| "upload.bin".into());
    let safe_name = sanitize_filename(&raw_name);
    let size_bytes = bytes.len() as i64;
    if size_bytes == 0 || size_bytes > max_upload_bytes() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"status": "error", "message": "file size must be 1..max"})),
        )
            .into_response();
    }
    if let Some(list) = allowed_mime() {
        if let Some(mt) = mime.clone() {
            let mt_lower = mt.to_lowercase();
            if !list.iter().any(|allowed| mt_lower.starts_with(allowed)) {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"status": "error", "message": "mime_type not allowed"})),
                )
                    .into_response();
            }
        }
    }

    let dir = upload_dir();
    if let Err(err) = fs::create_dir_all(&dir).await {
        error!(error = %err, "failed to create upload dir");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"status": "error", "message": "server cannot create upload dir"})),
        )
            .into_response();
    }

    let mut path = dir.join(&safe_name);
    let (stem, ext) = match path.file_stem().and_then(|s| s.to_str()) {
        Some(stem) => (stem.to_string(), path.extension().and_then(|e| e.to_str()).map(|e| e.to_string())),
        None => (safe_name.clone(), None),
    };
    let mut counter = 1;
    while fs::try_exists(&path).await.unwrap_or(false) {
        let new_name = if let Some(ext) = &ext {
            format!("{stem}-{counter}.{ext}")
        } else {
            format!("{stem}-{counter}")
        };
        path = dir.join(&new_name);
        counter += 1;
    }
    let final_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(&safe_name)
        .to_string();

    if let Err(err) = fs::write(&path, &bytes).await {
        error!(error = %err, "failed to write upload");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"status": "error", "message": "failed to write upload"})),
        )
            .into_response();
    }

    let base_url = upload_base_url();
    let public_url = format!(
        "{}/{}",
        base_url.trim_end_matches('/'),
        &final_name
    );

    match state
        .surreal
        .create_attachment_meta(
            &claims.sub,
            &final_name,
            size_bytes,
            mime.as_deref(),
            board_id.as_deref(),
            topic_id.as_deref(),
        )
        .await
    {
        Ok(att) => (
            StatusCode::CREATED,
            Json(json!({"status": "ok", "attachment": att, "url": public_url, "base_url": base_url})),
        )
            .into_response(),
        Err(err) => {
            error!(error = %err, "failed to store attachment meta");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"status": "error", "message": err.to_string()})),
            )
                .into_response()
        }
    }
}

#[derive(Deserialize)]
struct DeleteAttachmentPayload {
    id: String,
}

async fn delete_attachment_api(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    headers: HeaderMap,
    Json(payload): Json<DeleteAttachmentPayload>,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c,
        Err(resp) => return resp.into_response(),
    };
    if let Err(resp) = verify_csrf(&headers) {
        return resp.into_response();
    }
    let (_user, ctx) = match ensure_user_ctx(&state, &claims).await {
        Ok(value) => value,
        Err(resp) => return resp.into_response(),
    };
    let id_num = payload
        .id
        .split(':')
        .last()
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0);
    if id_num <= 0 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"status": "error", "message": "invalid id"})),
        )
            .into_response();
    }
    // Only owner or admin can delete.
    if !ctx.user_info.is_admin {
        match state.surreal.list_attachments_for_user(&claims.sub).await {
            Ok(items) => {
                if !items.iter().any(|a| a.id.as_deref() == Some(&payload.id)) {
                    return (
                        StatusCode::FORBIDDEN,
                        Json(json!({"status": "error", "message": "not allowed to delete"})),
                    )
                        .into_response();
                }
            }
            Err(err) => {
                error!(error = %err, "failed to load attachments for delete");
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"status": "error", "message": "cannot verify ownership"})),
                )
                    .into_response();
            }
        }
    }
    match state.forum_service.delete_attachment(id_num) {
        Ok(_) => (
            StatusCode::OK,
            Json(json!({"status": "ok", "id": payload.id})),
        )
            .into_response(),
        Err(err) => {
            error!(error = %err, user = %claims.sub, "failed to delete attachment");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"status": "error", "message": err.to_string()})),
            )
                .into_response()
        }
    }
}

async fn serve_upload(Path(path): axum::extract::Path<String>) -> impl IntoResponse {
    let mut full = upload_dir();
    // basic traversal guard
    for segment in path.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            continue;
        }
        full.push(segment);
    }
    match fs::read(&full).await {
        Ok(bytes) => (
            StatusCode::OK,
            [(axum::http::header::CONTENT_TYPE, "application/octet-stream")],
            bytes,
        )
            .into_response(),
        Err(err) => {
            if err.kind() == std::io::ErrorKind::NotFound {
                StatusCode::NOT_FOUND.into_response()
            } else {
                error!(error = %err, ?full, "failed to read uploaded file");
                StatusCode::INTERNAL_SERVER_ERROR.into_response()
            }
        }
    }
}

async fn list_personal_messages(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    Query(query): Query<PersonalMessageListQuery>,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c,
        Err(resp) => return resp.into_response(),
    };
    let (_user, ctx) = match ensure_user_ctx(&state, &claims).await {
        Ok(value) => value,
        Err(resp) => return resp.into_response(),
    };
    let folder = match query.folder.as_deref().unwrap_or("inbox").to_lowercase().as_str() {
        "sent" => PersonalMessageFolder::Sent,
        _ => PersonalMessageFolder::Inbox,
    };
    let limit = query.limit.unwrap_or(50).min(200);
    let offset = query.offset.unwrap_or(0);
    let label = query.label;
    match run_forum_blocking(&state, move |forum| {
        forum.personal_message_page(ctx.user_info.id, folder.clone(), label, offset, limit)
    }).await {
        Ok(page) => (
            StatusCode::OK,
            Json(json!({"status": "ok", "messages": page.messages, "folder": folder, "total": page.total, "unread": page.unread})),
        )
            .into_response(),
        Err(err) => {
            error!(error = %err, "failed to list personal messages");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"status": "error", "message": err.to_string()})),
            )
                .into_response()
        }
    }
}

async fn send_personal_message_api(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    headers: HeaderMap,
    Json(payload): Json<PersonalMessageSendPayload>,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c,
        Err(resp) => return resp.into_response(),
    };
    if let Err(resp) = verify_csrf(&headers) {
        return resp.into_response();
    }
    if payload.to.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"status": "error", "message": "recipient required"})),
        )
            .into_response();
    }
    if payload.subject.trim().is_empty() || payload.subject.len() > 200 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"status": "error", "message": "subject must be 1-200 chars"})),
        )
            .into_response();
    }
    if payload.body.trim().is_empty() || payload.body.len() > 4000 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"status": "error", "message": "body must be 1-4000 chars"})),
        )
            .into_response();
    }
    let (user, ctx) = match ensure_user_ctx(&state, &claims).await {
        Ok(value) => value,
        Err(resp) => return resp.into_response(),
    };

    // resolve recipient ids by name
    let recipients = payload.to.clone();
    let recipient_ids = match run_forum_blocking(&state, move |forum| {
        let mut ids = Vec::new();
        for name in recipients {
            match forum.find_member_by_name(&name) {
                Ok(Some(member)) => ids.push(member.id),
                _ => return Err(ForumError::Validation(format!("unknown recipient: {name}"))),
            }
        }
        Ok(ids)
    })
    .await {
        Ok(ids) => ids,
        Err(err) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"status": "error", "message": err.to_string()})),
            )
                .into_response();
        }
    };

    let message = SendPersonalMessage {
        sender_id: ctx.user_info.id,
        sender_name: user.name.clone(),
        to: recipient_ids,
        bcc: Vec::new(),
        subject: sanitize_input(&payload.subject),
        body: sanitize_input(&payload.body),
    };
    match run_forum_blocking(&state, move |forum| forum.send_personal_message(message)).await {
        Ok(result) => (
            StatusCode::CREATED,
            Json(json!({"status": "ok", "sent_to": result.recipient_ids})),
        )
            .into_response(),
        Err(err) => {
            error!(error = %err, "failed to send personal message");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"status": "error", "message": err.to_string()})),
            )
                .into_response()
        }
    }
}

async fn mark_personal_messages_read(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    headers: HeaderMap,
    Json(payload): Json<PersonalMessageIdsPayload>,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c,
        Err(resp) => return resp.into_response(),
    };
    if let Err(resp) = verify_csrf(&headers) {
        return resp.into_response();
    }
    if payload.ids.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"status": "error", "message": "ids required"})),
        )
            .into_response();
    }
    let (_user, ctx) = match ensure_user_ctx(&state, &claims).await {
        Ok(value) => value,
        Err(resp) => return resp.into_response(),
    };
    let ids = payload.ids.clone();
    match run_forum_blocking(&state, move |forum| forum.mark_personal_messages(ctx.user_info.id, &ids, true)).await {
        Ok(_) => (
            StatusCode::OK,
            Json(json!({"status": "ok", "ids": payload.ids})),
        )
            .into_response(),
        Err(err) => {
            error!(error = %err, "failed to mark personal messages read");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"status": "error", "message": err.to_string()})),
            )
                .into_response()
        }
    }
}

async fn delete_personal_messages_api(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    headers: HeaderMap,
    Json(payload): Json<PersonalMessageIdsPayload>,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c,
        Err(resp) => return resp.into_response(),
    };
    if let Err(resp) = verify_csrf(&headers) {
        return resp.into_response();
    }
    if payload.ids.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"status": "error", "message": "ids required"})),
        )
            .into_response();
    }
    let (_user, ctx) = match ensure_user_ctx(&state, &claims).await {
        Ok(value) => value,
        Err(resp) => return resp.into_response(),
    };
    let ids = payload.ids.clone();
    match run_forum_blocking(&state, move |forum| {
        forum.delete_personal_messages(ctx.user_info.id, PersonalMessageFolder::Inbox, &ids)
    })
    .await {
        Ok(_) => (
            StatusCode::OK,
            Json(json!({"status": "ok", "ids": payload.ids})),
        )
            .into_response(),
        Err(err) => {
            error!(error = %err, "failed to delete personal messages");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"status": "error", "message": err.to_string()})),
            )
                .into_response()
        }
    }
}

async fn list_notifications(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c,
        Err(resp) => return resp.into_response(),
    };
    let target = claims.sub.clone();
    match state.surreal.list_notifications(&target).await {
        Ok(items) => (
            StatusCode::OK,
            Json(json!({"status": "ok", "notifications": items})),
        )
            .into_response(),
        Err(err) => {
            error!(error = %err, "failed to list notifications");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"status": "error", "message": err.to_string()})),
            )
                .into_response()
        }
    }
}

async fn demo_post(State(state): State<AppState>, claims: Option<AuthClaims>) -> impl IntoResponse {
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

    match state.forum_service.persist_post(
        &ctx,
        btc_forum_rust::services::PostSubmission {
            topic_id: None,
            board_id: 0,
            message_id: None,
            subject: "API example".into(),
            body: "Hello from Axum demo endpoint".into(),
            icon: "xx".into(),
            approved: true,
            send_notifications: false,
        },
    ) {
        Ok(posted) => (
            StatusCode::OK,
            Json(json!({
                "status": "ok",
                "topic_id": posted.topic_id,
                "post_id": posted.message_id,
                "author": author
            })),
        )
            .into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"status": "error", "message": err.to_string()})),
        )
            .into_response(),
    }
}

async fn list_users(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    Query(params): Query<AdminUsersQuery>,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c.clone(),
        Err(resp) => return resp.into_response(),
    };
    let (_user, ctx) = match ensure_user_ctx(&state, &claims).await {
        Ok(value) => value,
        Err(resp) => return resp.into_response(),
    };
    if let Err(resp) = ensure_admin(&ctx) {
        return resp.into_response();
    }
    match run_forum_blocking(&state, move |forum| forum.list_members()).await {
        Ok(members) => {
            let filtered: Vec<_> = members
                .into_iter()
                .filter(|m| {
                    if let Some(ref q) = params.q {
                        m.name.to_lowercase().contains(&q.to_lowercase())
                    } else {
                        true
                    }
                })
                .take(params.limit.unwrap_or(200))
                .collect();
            (
                StatusCode::OK,
                Json(json!({ "status": "ok", "members": filtered })),
            )
                .into_response()
        }
        Err(err) => {
            error!(error = %err, "failed to list members");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"status": "error", "message": err.to_string()})),
            )
                .into_response()
        }
    }
}

#[derive(Deserialize)]
struct AdminNotifyPayload {
    user_ids: Vec<i64>,
    subject: String,
    body: String,
}

#[derive(Deserialize)]
struct BanPayload {
    member_id: i64,
    reason: Option<String>,
    hours: Option<i64>,
}

#[derive(Deserialize)]
struct AdminUsersQuery {
    q: Option<String>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct AdminPageQuery {
    _limit: Option<usize>,
    _offset: Option<usize>,
}

#[derive(Deserialize)]
struct BoardAccessPayload {
    board_id: String,
    allowed_groups: Vec<i64>,
}

#[derive(Deserialize)]
struct BoardPermissionPayload {
    board_id: String,
    group_id: i64,
    allow: Vec<String>,
    deny: Vec<String>,
}

async fn admin_notify(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(payload): Json<AdminNotifyPayload>,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c.clone(),
        Err(resp) => return resp.into_response(),
    };
    let (_user, ctx) = match ensure_user_ctx(&state, &claims).await {
        Ok(value) => value,
        Err(resp) => return resp.into_response(),
    };
    if let Err(resp) = ensure_admin(&ctx) {
        return resp.into_response();
    }
    let key = rate_key(&claims, Some(&addr));
    if let Err(resp) = enforce_rate(&state, &key, 5, Duration::from_secs(60)) {
        return resp.into_response();
    }
    if let Err(resp) = verify_csrf(&headers) {
        return resp.into_response();
    }
    if payload.user_ids.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"status": "error", "message": "user_ids required"})),
        )
            .into_response();
    }
    if payload.user_ids.len() > 50 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"status": "error", "message": "user_ids too many (max 50)"})),
        )
            .into_response();
    }
    if let Err(resp) = validate_content(&payload.subject, &payload.body) {
        return resp.into_response();
    }
    let message = SendPersonalMessage {
        sender_id: 0,
        sender_name: "admin".into(),
        to: payload.user_ids.clone(),
        bcc: Vec::new(),
        subject: sanitize_input(&payload.subject),
        body: sanitize_input(&payload.body),
    };
    match state.forum_service.send_personal_message(message) {
        Ok(result) => (
            StatusCode::OK,
            Json(json!({"status": "ok", "sent_to": result.recipient_ids })),
        )
            .into_response(),
        Err(err) => {
            error!(error = %err, "failed to send admin notification");
            let _ = state.forum_service.log_action(
                "admin_notify_error",
                None,
                &json!({
                    "error": err.to_string(),
                    "subject": payload.subject,
                    "user_ids": payload.user_ids,
                }),
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"status": "error", "message": err.to_string()})),
            )
                .into_response()
        }
    }
}

async fn list_bans(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    Query(_): Query<AdminPageQuery>,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c.clone(),
        Err(resp) => return resp.into_response(),
    };
    let (_user, ctx) = match ensure_user_ctx(&state, &claims).await {
        Ok(value) => value,
        Err(resp) => return resp.into_response(),
    };
    if let Err(resp) = ensure_admin(&ctx) {
        return resp.into_response();
    }
    match state.forum_service.list_ban_rules() {
        Ok(bans) => (StatusCode::OK, Json(json!({"status": "ok", "bans": bans}))).into_response(),
        Err(err) => {
            error!(error = %err, "failed to list bans");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"status": "error", "message": err.to_string()})),
            )
                .into_response()
        }
    }
}

async fn apply_ban(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    headers: HeaderMap,
    Json(payload): Json<BanPayload>,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c.clone(),
        Err(resp) => return resp.into_response(),
    };
    let (_user, ctx) = match ensure_user_ctx(&state, &claims).await {
        Ok(value) => value,
        Err(resp) => return resp.into_response(),
    };
    if let Err(resp) = ensure_admin(&ctx) {
        return resp.into_response();
    }
    if let Err(resp) = verify_csrf(&headers) {
        return resp.into_response();
    }
    let hours = payload.hours.unwrap_or(24).clamp(1, 24 * 365);
    let until = Utc::now()
        .checked_add_signed(chrono::Duration::hours(hours))
        .map(|dt| dt.timestamp())
        .unwrap_or_else(|| Utc::now().timestamp());
    let rule = BanRule {
        id: 0,
        reason: payload.reason.clone(),
        expires_at: Some(chrono::DateTime::from_timestamp(until, 0).unwrap()),
        conditions: vec![BanCondition {
            id: 0,
            reason: payload.reason.clone(),
            affects: BanAffects::Account {
                member_id: payload.member_id,
            },
            expires_at: Some(chrono::DateTime::from_timestamp(until, 0).unwrap()),
        }],
    };
    match state.forum_service.save_ban_rule(rule) {
        Ok(id) => (
            StatusCode::OK,
            Json(json!({"status": "ok", "ban_id": id, "member_id": payload.member_id})),
        )
            .into_response(),
        Err(err) => {
            error!(error = %err, "failed to apply ban");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"status": "error", "message": err.to_string()})),
            )
                .into_response()
        }
    }
}

async fn revoke_ban(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    headers: HeaderMap,
    Json(payload): Json<BanPayload>,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c.clone(),
        Err(resp) => return resp.into_response(),
    };
    let (_user, ctx) = match ensure_user_ctx(&state, &claims).await {
        Ok(value) => value,
        Err(resp) => return resp.into_response(),
    };
    if let Err(resp) = ensure_admin(&ctx) {
        return resp.into_response();
    }
    if let Err(resp) = verify_csrf(&headers) {
        return resp.into_response();
    }
    match state.forum_service.delete_ban_rule(payload.member_id) {
        Ok(_) => (
            StatusCode::OK,
            Json(json!({"status": "ok", "ban_id": payload.member_id})),
        )
            .into_response(),
        Err(err) => {
            error!(error = %err, "failed to revoke ban");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"status": "error", "message": err.to_string()})),
            )
                .into_response()
        }
    }
}

async fn list_action_logs(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    Query(_): Query<AdminPageQuery>,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c.clone(),
        Err(resp) => return resp.into_response(),
    };
    let (_user, ctx) = match ensure_user_ctx(&state, &claims).await {
        Ok(value) => value,
        Err(resp) => return resp.into_response(),
    };
    if let Err(resp) = ensure_admin(&ctx) {
        return resp.into_response();
    }
    match state.forum_service.list_action_logs() {
        Ok(logs) => (StatusCode::OK, Json(json!({"status": "ok", "logs": logs}))).into_response(),
        Err(err) => {
            error!(error = %err, "failed to list action logs");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"status": "error", "message": err.to_string()})),
            )
                .into_response()
        }
    }
}

async fn get_board_access(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c.clone(),
        Err(resp) => return resp.into_response(),
    };
    let (_user, ctx) = match ensure_user_ctx(&state, &claims).await {
        Ok(value) => value,
        Err(resp) => return resp.into_response(),
    };
    if let Err(resp) = ensure_admin(&ctx) {
        return resp.into_response();
    }
    match load_board_access(&state).await {
        Ok(entries) => (
            StatusCode::OK,
            Json(json!({"status": "ok", "entries": entries})),
        )
            .into_response(),
        Err(err) => {
            error!(error = %err, "failed to list board access");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"status": "error", "message": err.to_string()})),
            )
                .into_response()
        }
    }
}

async fn update_board_access(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    headers: HeaderMap,
    Json(payload): Json<BoardAccessPayload>,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c.clone(),
        Err(resp) => return resp.into_response(),
    };
    let (_user, ctx) = match ensure_user_ctx(&state, &claims).await {
        Ok(value) => value,
        Err(resp) => return resp.into_response(),
    };
    if let Err(resp) = ensure_admin(&ctx) {
        return resp.into_response();
    }
    if let Err(resp) = verify_csrf(&headers) {
        return resp.into_response();
    }
    if payload.allowed_groups.len() > 1000 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"status": "error", "message": "too many groups"})),
        )
            .into_response();
    }
    match state
        .forum_service
        .set_board_access(&payload.board_id, &payload.allowed_groups)
    {
        Ok(_) => (
            StatusCode::OK,
            Json(json!({"status": "ok", "board_id": payload.board_id, "allowed_groups": payload.allowed_groups})),
        )
            .into_response(),
        Err(err) => {
            error!(error = %err, "failed to update board access");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"status": "error", "message": err.to_string()})),
            )
                .into_response()
        }
    }
}

#[derive(Serialize, Deserialize)]
struct BoardPermissionEntry {
    board_id: String,
    group_id: i64,
    allow: Vec<String>,
    deny: Vec<String>,
}

async fn get_board_permissions(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c.clone(),
        Err(resp) => return resp.into_response(),
    };
    let (_user, ctx) = match ensure_user_ctx(&state, &claims).await {
        Ok(value) => value,
        Err(resp) => return resp.into_response(),
    };
    if let Err(resp) = ensure_admin(&ctx) {
        return resp.into_response();
    }
    let mut response = match state
        .surreal
        .client()
        .query(
            r#"
            SELECT board_id, group_id, allow, deny
            FROM board_permissions;
            "#,
        )
        .await
    {
        Ok(resp) => resp,
        Err(err) => {
            error!(error = %err, "failed to list board permissions");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"status": "error", "message": err.to_string()})),
            )
                .into_response();
        }
    };
    let entries: Vec<BoardPermissionEntry> = response.take(0).unwrap_or_default();
    (
        StatusCode::OK,
        Json(json!({"status": "ok", "entries": entries})),
    )
        .into_response()
}

async fn update_board_permissions(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    headers: HeaderMap,
    Json(payload): Json<BoardPermissionPayload>,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c.clone(),
        Err(resp) => return resp.into_response(),
    };
    let (_user, ctx) = match ensure_user_ctx(&state, &claims).await {
        Ok(value) => value,
        Err(resp) => return resp.into_response(),
    };
    if let Err(resp) = ensure_admin(&ctx) {
        return resp.into_response();
    }
    if let Err(resp) = verify_csrf(&headers) {
        return resp.into_response();
    }
    if payload.allow.len() + payload.deny.len() > 500 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"status": "error", "message": "too many permissions"})),
        )
            .into_response();
    }

    let result = state
        .surreal
        .client()
        .query(
            r#"
            UPDATE type::thing("board_permissions", string::concat("bp:", $board_id, ":", $group_id)) SET
                board_id = $board_id,
                group_id = $group_id,
                allow = $allow,
                deny = $deny;
            "#,
        )
        .bind(("board_id", payload.board_id.clone()))
        .bind(("group_id", payload.group_id))
        .bind(("allow", payload.allow.clone()))
        .bind(("deny", payload.deny.clone()))
        .await;

    match result {
        Ok(_) => (
            StatusCode::OK,
            Json(json!({
                "status": "ok",
                "board_id": payload.board_id,
                "group_id": payload.group_id,
                "allow": payload.allow,
                "deny": payload.deny
            })),
        )
            .into_response(),
        Err(err) => {
            error!(error = %err, "failed to update board permissions");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"status": "error", "message": err.to_string()})),
            )
                .into_response()
        }
    }
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
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
