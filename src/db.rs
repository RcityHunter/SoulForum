use sqlx::{postgres::PgPoolOptions, PgPool};
use std::{env, time::Duration};

/// Database configuration loaded from environment.
#[derive(Debug, Clone)]
pub struct DbConfig {
    pub url: String,
    pub max_connections: u32,
    pub acquire_timeout: Duration,
}

impl DbConfig {
    pub fn from_env() -> Self {
        let url =
            env::var("DATABASE_URL").expect("DATABASE_URL is required to start the API server");
        let max_connections = env::var("DATABASE_MAX_CONNECTIONS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(5);
        let acquire_timeout_secs = env::var("DATABASE_ACQUIRE_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(3);

        Self {
            url,
            max_connections,
            acquire_timeout: Duration::from_secs(acquire_timeout_secs),
        }
    }
}

pub fn connect_pool(cfg: &DbConfig) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(cfg.max_connections)
        .acquire_timeout(cfg.acquire_timeout)
        .connect_lazy(&cfg.url)
}

/// Ensure a user exists locally using an external subject identifier (from JWT sub).
/// Returns the user id.
pub async fn upsert_user_by_sub(pool: &PgPool, sub: &str) -> Result<i64, sqlx::Error> {
    // Use sub as username; synthesize a local email to satisfy unique constraint.
    let email = format!("{sub}@local");
    sqlx::query_scalar::<_, i64>(
        r#"
        INSERT INTO users (username, email, password_hash)
        VALUES ($1, $2, 'external')
        ON CONFLICT (username)
        DO UPDATE SET updated_at = now()
        RETURNING id
        "#,
    )
    .bind(sub)
    .bind(&email)
    .fetch_one(pool)
    .await
}
