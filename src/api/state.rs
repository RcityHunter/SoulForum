use std::{
    collections::HashMap,
    env,
    path::PathBuf,
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};

use btc_forum_rust::{
    rainbow_auth::RainbowAuthClient,
    services::{surreal::SurrealService, ForumError},
    surreal::SurrealForumService,
};

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) surreal: SurrealForumService,
    pub(crate) forum_service: SurrealService,
    pub(crate) rainbow_auth: RainbowAuthClient,
    pub(crate) rate_limiter: Arc<RateLimiter>,
    pub(crate) start_time: Instant,
}

#[derive(Default)]
pub(crate) struct RateLimiter {
    // key -> (count, window_start)
    limits: std::sync::Mutex<HashMap<String, (u32, Instant)>>,
}

impl RateLimiter {
    pub(crate) fn new() -> Self {
        Self {
            limits: std::sync::Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn allow(&self, key: &str, max: u32, window: Duration) -> bool {
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

    pub(crate) fn snapshot(&self) -> HashMap<String, u32> {
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

static ENFORCE_CSRF: OnceLock<bool> = OnceLock::new();
static MAX_UPLOAD_MB: OnceLock<i64> = OnceLock::new();
static ALLOWED_MIME: OnceLock<Option<Vec<String>>> = OnceLock::new();

pub(crate) fn csrf_enabled() -> bool {
    *ENFORCE_CSRF.get_or_init(|| {
        env::var("ENFORCE_CSRF")
            .map(|v| !matches!(v.to_lowercase().as_str(), "0" | "false" | "off"))
            .unwrap_or(true)
    })
}

pub(crate) fn upload_dir() -> PathBuf {
    PathBuf::from(env::var("UPLOAD_DIR").unwrap_or_else(|_| "uploads".into()))
}

pub(crate) fn upload_base_url() -> String {
    env::var("UPLOAD_BASE_URL").unwrap_or_else(|_| "/uploads".into())
}

pub(crate) fn rainbow_auth_base_url() -> String {
    env::var("RAINBOW_AUTH_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".into())
}

pub(crate) fn max_upload_bytes() -> i64 {
    *MAX_UPLOAD_MB.get_or_init(|| {
        env::var("MAX_UPLOAD_MB")
            .ok()
            .and_then(|v| v.parse().ok())
            .filter(|v| *v > 0)
            .unwrap_or(10)
    }) * 1024
        * 1024
}

pub(crate) fn allowed_mime() -> Option<&'static [String]> {
    ALLOWED_MIME
        .get_or_init(|| {
            env::var("ALLOWED_MIME")
                .ok()
                .map(|v| {
                    v.split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                })
                .filter(|list: &Vec<String>| !list.is_empty())
        })
        .as_deref()
}

pub(crate) fn generate_csrf_token() -> String {
    use rand::{rngs::OsRng, RngCore};
    let mut buf = [0u8; 32];
    OsRng.fill_bytes(&mut buf);
    buf.iter().map(|b| format!("{:02x}", b)).collect()
}

pub(crate) fn find_csrf_cookie(headers: &axum::http::HeaderMap) -> Option<String> {
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

pub(crate) async fn run_forum_blocking<T>(
    state: &AppState,
    job: impl FnOnce(&SurrealService) -> Result<T, ForumError> + Send + 'static,
) -> Result<T, ForumError>
where
    T: Send + 'static,
{
    let forum_service = state.forum_service.clone();
    tokio::task::spawn_blocking(move || job(&forum_service))
        .await
        .map_err(|e| ForumError::Internal(format!("forum task failed: {e}")))?
}
