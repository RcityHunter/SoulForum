use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::env;
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use surrealdb::{
    engine::remote::http::{Client, Http},
    opt::auth::Root,
    Surreal,
};
use surrealdb_types::SurrealValue;
use tracing::{info, warn};

pub type SurrealClient = Surreal<Client>;

fn is_surreal_unauthorized(err: &surrealdb::Error) -> bool {
    let msg = err.to_string();
    msg.contains("401 Unauthorized") || msg.contains("status client error (401")
}

fn is_surreal_connection_uninitialised(err: &surrealdb::Error) -> bool {
    err.to_string().contains("Connection uninitialised")
}

fn normalize_endpoint(raw: String) -> String {
    // Surreal's HTTP client expects a host:port string. Strip any scheme and trailing slash to
    // avoid building URLs like `http://http://host:port/health`.
    let ep = raw.trim().trim_end_matches('/').to_string();
    ep.strip_prefix("http://")
        .or_else(|| ep.strip_prefix("https://"))
        .unwrap_or(&ep)
        .to_string()
}

fn endpoint_with_scheme(raw: String) -> String {
    let ep = raw.trim().trim_end_matches('/').to_string();
    if ep.starts_with("http://") || ep.starts_with("https://") {
        ep
    } else {
        format!("http://{}", ep)
    }
}

fn env_non_empty(key: &str, default: &str) -> String {
    env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| default.to_string())
}

async fn rpc_signin_token(
    endpoint: &str,
    user: &str,
    pass: &str,
) -> Result<String, surrealdb::Error> {
    let rpc_url = format!("{}/rpc", endpoint.trim_end_matches('/'));
    let payload = serde_json::json!({
        "id": 1,
        "method": "signin",
        "params": [
            { "user": user, "pass": pass }
        ]
    });

    let response = reqwest::Client::new()
        .post(&rpc_url)
        .header("Content-Type", "application/json")
        .body(payload.to_string())
        .send()
        .await
        .map_err(|e| surrealdb::Error::thrown(format!("surreal rpc signin request failed: {e}")))?;

    let text = response.text().await.map_err(|e| {
        surrealdb::Error::thrown(format!("surreal rpc signin response read failed: {e}"))
    })?;

    let value: serde_json::Value = serde_json::from_str(&text).map_err(|e| {
        surrealdb::Error::thrown(format!("surreal rpc signin invalid json: {e}; body={text}"))
    })?;

    if let Some(token) = value.get("result").and_then(|v| v.as_str()) {
        return Ok(token.to_string());
    }

    if let Some(access) = value
        .get("result")
        .and_then(|v| v.get("access"))
        .and_then(|v| v.as_str())
    {
        return Ok(access.to_string());
    }

    Err(surrealdb::Error::thrown(format!(
        "surreal rpc signin failed: {}",
        value
            .get("error")
            .cloned()
            .unwrap_or(serde_json::Value::String(text))
    )))
}

/// (Re)authenticate the given client using environment variables.
///
/// Why this exists:
/// - SurrealDB issues an auth token on `signin`.
/// - In some setups, that token can expire (e.g. ~1h), causing `/rpc` to start returning 401.
/// - Calling `signin` again refreshes the token.
pub async fn reauth_from_env(client: &SurrealClient) -> Result<(), surrealdb::Error> {
    let endpoint = endpoint_with_scheme(env_non_empty("SURREAL_ENDPOINT", "http://127.0.0.1:8000"));
    let env_ns = env_non_empty("SURREAL_NAMESPACE", "forum");
    let env_db = env_non_empty("SURREAL_DATABASE", "main");
    let env_user = env_non_empty("SURREAL_USER", "root");
    let env_pass = env_non_empty("SURREAL_PASS", "root");

    // Try a few credential/context combinations to survive env drift.
    let mut tried = HashSet::new();
    let candidates = vec![
        (
            env_user.clone(),
            env_pass.clone(),
            env_ns.clone(),
            env_db.clone(),
        ),
        (
            "root".to_string(),
            "root".to_string(),
            env_ns.clone(),
            env_db.clone(),
        ),
        (
            "root".to_string(),
            "root".to_string(),
            "forum".to_string(),
            "main".to_string(),
        ),
        (
            "root".to_string(),
            "root".to_string(),
            "auth".to_string(),
            "main".to_string(),
        ),
    ];

    let mut last_err: Option<surrealdb::Error> = None;
    for (user, pass, ns, db) in candidates {
        let key = format!("{user}|{pass}|{ns}|{db}");
        if !tried.insert(key) {
            continue;
        }

        // SurrealDB 3.x is more reliable with native signin than authenticate(token) refresh.
        match client
            .signin(Root {
                username: user.clone(),
                password: pass.clone(),
            })
            .await
        {
            Ok(_) => match client.use_ns(&ns).use_db(&db).await {
                Ok(_) => return Ok(()),
                Err(use_err) => {
                    warn!(error = %use_err, namespace = %ns, database = %db, "surreal native signin succeeded but use_ns/use_db failed");
                    last_err = Some(use_err);
                }
            },
            Err(signin_err) => {
                warn!(error = %signin_err, user = %user, namespace = %ns, database = %db, "surreal native signin failed");
                // Fallback: try rpc signin + authenticate(token)
                match rpc_signin_token(&endpoint, &user, &pass).await {
                    Ok(token) => match client.authenticate(token).await {
                        Ok(_) => match client.use_ns(&ns).use_db(&db).await {
                            Ok(_) => return Ok(()),
                            Err(err) => {
                                warn!(error = %err, namespace = %ns, database = %db, "surreal rpc-token auth succeeded but use_ns/use_db failed");
                                last_err = Some(err);
                            }
                        },
                        Err(err) => {
                            warn!(error = %err, user = %user, namespace = %ns, database = %db, "surreal rpc-token authenticate failed");
                            last_err = Some(err);
                        }
                    },
                    Err(err) => {
                        warn!(error = %err, user = %user, namespace = %ns, database = %db, "surreal rpc signin failed");
                        last_err = Some(err);
                    }
                }
            }
        };
    }

    if let Some(err) = last_err {
        Err(err)
    } else {
        let token = rpc_signin_token(&endpoint, &env_user, &env_pass).await?;
        client.authenticate(token).await?;
        client.use_ns(&env_ns).use_db(&env_db).await?;
        Ok(())
    }
}

/// Connect to SurrealDB using environment variables, defaults to local root account.
pub async fn connect_from_env() -> Result<SurrealClient, surrealdb::Error> {
    let endpoint_raw = env_non_empty("SURREAL_ENDPOINT", "http://127.0.0.1:8000");
    let ns = env_non_empty("SURREAL_NAMESPACE", "forum");
    let db = env_non_empty("SURREAL_DATABASE", "main");
    // SurrealDB's Rust HTTP client expects host:port here. Passing a full URL like
    // `http://127.0.0.1:8000` can be misparsed by some client/runtime combinations,
    // producing misleading DNS errors such as resolving `http:80`.
    // Always connect with the normalized host:port form and reserve the schemeful URL
    // only for raw reqwest RPC fallback paths.
    let endpoints = vec![normalize_endpoint(endpoint_raw)];

    let mut last_err: Option<surrealdb::Error> = None;
    for endpoint in endpoints {
        info!(endpoint, namespace = %ns, database = %db, "connecting to SurrealDB (HTTP)");
        match Surreal::new::<Http>(&endpoint).await {
            Ok(client) => {
                if let Err(err) = reauth_from_env(&client).await {
                    warn!(error = %err, endpoint = %endpoint, "surreal reauth failed after connect");
                    last_err = Some(err);
                    continue;
                }
                match client.query("RETURN true;").await {
                    Ok(_) => return Ok(client),
                    Err(err) => {
                        warn!(error = %err, endpoint = %endpoint, "surreal post-connect health check failed");
                        last_err = Some(err);
                        continue;
                    }
                }
            }
            Err(err) => {
                warn!(error = %err, endpoint = %endpoint, "surreal connect failed");
                last_err = Some(err);
                continue;
            }
        }
    }

    Err(last_err.unwrap_or_else(|| {
        surrealdb::Error::thrown(
            "failed to connect to SurrealDB: no endpoint candidate succeeded".to_string(),
        )
    }))
}

/// Create a demo post record in SurrealDB.
pub async fn create_demo_post(
    client: &SurrealClient,
    subject: &str,
    body: &str,
    user: &str,
) -> Result<Value, surrealdb::Error> {
    let subject = subject.to_owned();
    let body = body.to_owned();
    let user = user.to_owned();
    let mut response = client
        .query(
            r#"
            CREATE demo_posts CONTENT {
                subject: $subject,
                body: $body,
                user: $user,
                created_at: time::now()
            } RETURN *;
            "#,
        )
        .bind(("subject", subject))
        .bind(("body", body))
        .bind(("user", user))
        .await?;

    let created: Option<Value> = response.take(0)?;
    Ok(created.unwrap_or(Value::Null))
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, SurrealValue)]
pub struct SurrealPost {
    pub id: Option<String>,
    pub topic_id: Option<String>,
    pub board_id: Option<String>,
    pub subject: String,
    pub body: String,
    pub author: String,
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, SurrealValue)]
pub struct SurrealTopic {
    pub id: Option<String>,
    pub board_id: String,
    pub subject: String,
    pub author: String,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, SurrealValue)]
pub struct SurrealBoard {
    pub id: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub created_at: Option<String>,
}

pub async fn create_post(
    client: &SurrealClient,
    subject: &str,
    body: &str,
    user: &str,
) -> Result<SurrealPost, surrealdb::Error> {
    let subject = subject.to_owned();
    let body = body.to_owned();
    let user = user.to_owned();
    let mut response = client
        .query(
            r#"
            CREATE posts CONTENT {
                topic_id: null,
                board_id: null,
                subject: $subject,
                body: $body,
                author: $user,
                created_at: time::now()
            } RETURN meta::id(id) as id, topic_id, board_id, subject, body, author,
                     <string>created_at as created_at;
            "#,
        )
        .bind(("subject", subject.clone()))
        .bind(("body", body.clone()))
        .bind(("user", user.clone()))
        .await?;

    let created: Option<SurrealPost> = response.take(0)?;
    Ok(created.unwrap_or_else(|| SurrealPost {
        id: None,
        topic_id: None,
        board_id: None,
        subject,
        body,
        author: user,
        created_at: None,
    }))
}

pub async fn list_posts(client: &SurrealClient) -> Result<Vec<SurrealPost>, surrealdb::Error> {
    let mut response = client
        .query(
            r#"
            SELECT meta::id(id) as id, topic_id, board_id, subject, body, author,
                   <string>created_at as created_at
            FROM posts
            ORDER BY created_at DESC
            LIMIT 50;
            "#,
        )
        .await?;

    let posts: Vec<SurrealPost> = response.take(0)?;
    Ok(posts)
}

#[derive(Debug, Serialize, Deserialize, Clone, SurrealValue)]
pub struct SurrealAttachment {
    pub id: Option<String>,
    pub owner: String,
    pub filename: String,
    pub size_bytes: i64,
    pub mime_type: Option<String>,
    pub board_id: Option<String>,
    pub topic_id: Option<String>,
    pub message_id: Option<String>,
    pub created_at: Option<String>,
}

pub async fn create_attachment_meta(
    client: &SurrealClient,
    owner: &str,
    filename: &str,
    size_bytes: i64,
    mime_type: Option<&str>,
    board_id: Option<&str>,
    topic_id: Option<&str>,
) -> Result<SurrealAttachment, surrealdb::Error> {
    let mut response = client
        .query(
            r#"
            CREATE attachments CONTENT {
                owner: $owner,
                filename: $filename,
                size_bytes: $size_bytes,
                mime_type: $mime,
                board_id: $board_id,
                topic_id: $topic_id,
                created_at: time::now(),
                created_at_ms: time::unix(time::now())
            } RETURN meta::id(id) as id, owner, filename, size_bytes, mime_type, board_id, topic_id, created_at;
            "#,
        )
        .bind(("owner", owner.to_string()))
        .bind(("filename", filename.to_string()))
        .bind(("size_bytes", size_bytes))
        .bind(("mime", mime_type.map(|s| s.to_string())))
        .bind(("board_id", board_id.map(|s| s.to_string())))
        .bind(("topic_id", topic_id.map(|s| s.to_string())))
        .await?;
    let att: Option<SurrealAttachment> = response.take(0)?;
    Ok(att.unwrap_or_else(|| SurrealAttachment {
        id: None,
        owner: owner.to_string(),
        filename: filename.to_string(),
        size_bytes,
        mime_type: mime_type.map(|s| s.to_string()),
        board_id: board_id.map(|s| s.to_string()),
        topic_id: topic_id.map(|s| s.to_string()),
        message_id: None,
        created_at: None,
    }))
}

pub async fn list_attachments_for_user(
    client: &SurrealClient,
    owner: &str,
) -> Result<Vec<SurrealAttachment>, surrealdb::Error> {
    let mut response = client
        .query(
            r#"
            SELECT meta::id(id) as id, owner, filename, size_bytes, mime_type, board_id, topic_id, message_id, created_at
            FROM attachments
            WHERE owner = $owner
            ORDER BY created_at DESC
            LIMIT 200;
            "#,
        )
        .bind(("owner", owner.to_string()))
        .await?;
    let items: Vec<SurrealAttachment> = response.take(0)?;
    Ok(items)
}

#[derive(Debug, Serialize, Deserialize, Clone, SurrealValue)]
pub struct SurrealNotification {
    pub id: Option<String>,
    pub user: String,
    pub subject: String,
    pub body: String,
    pub is_read: Option<bool>,
    pub created_at: Option<String>,
}

pub async fn create_notification(
    client: &SurrealClient,
    user: &str,
    subject: &str,
    body: &str,
) -> Result<SurrealNotification, surrealdb::Error> {
    let mut response = client
        .query(
            r#"
            CREATE notifications CONTENT {
                user: $user,
                subject: $subject,
                body: $body,
                is_read: false,
                created_at: time::now(),
                created_at_ms: time::unix(time::now())
            } RETURN meta::id(id) as id, user, subject, body, is_read, created_at;
            "#,
        )
        .bind(("user", user.to_string()))
        .bind(("subject", subject.to_string()))
        .bind(("body", body.to_string()))
        .await?;
    let note: Option<SurrealNotification> = response.take(0)?;
    Ok(note.unwrap_or_else(|| SurrealNotification {
        id: None,
        user: user.to_string(),
        subject: subject.to_string(),
        body: body.to_string(),
        is_read: Some(false),
        created_at: None,
    }))
}

pub async fn list_notifications(
    client: &SurrealClient,
    user: &str,
) -> Result<Vec<SurrealNotification>, surrealdb::Error> {
    let mut response = client
        .query(
            r#"
            SELECT meta::id(id) as id, user, subject, body, is_read, created_at
            FROM notifications
            WHERE user = $user
            ORDER BY created_at DESC
            LIMIT 100;
            "#,
        )
        .bind(("user", user.to_string()))
        .await?;
    let notes: Vec<SurrealNotification> = response.take(0)?;
    Ok(notes)
}

pub async fn mark_notification_read(
    client: &SurrealClient,
    id: &str,
) -> Result<(), surrealdb::Error> {
    let owned = id.to_string();
    client
        .query(
            r#"
            UPDATE type::thing("notifications", $id) SET is_read = true;
            "#,
        )
        .bind(("id", owned))
        .await?;
    Ok(())
}

pub async fn create_board(
    client: &SurrealClient,
    name: &str,
    description: Option<&str>,
) -> Result<SurrealBoard, surrealdb::Error> {
    // Re-select namespace/database before writes. After reconnects or auth refreshes, some
    // SurrealDB client states can lose the active ns/db context and surface it as
    // "Connection uninitialised" on the next write query.
    let ns = env_non_empty("SURREAL_NAMESPACE", "forum");
    let db = env_non_empty("SURREAL_DATABASE", "main");
    client.use_ns(&ns).use_db(&db).await?;

    let name = name.to_owned();
    let description_owned = description.map(|d| d.to_owned());
    let mut response = client
        .query(
            r#"
            CREATE boards CONTENT {
                name: $name,
                description: $description,
                created_at: time::now()
            } RETURN meta::id(id) as id, name, description, <string>created_at as created_at;
            "#,
        )
        .bind(("name", name.clone()))
        .bind(("description", description_owned.clone()))
        .await?;

    let board: Option<SurrealBoard> = response.take(0)?;
    Ok(board.unwrap_or_else(|| SurrealBoard {
        id: None,
        name,
        description: description_owned,
        created_at: None,
    }))
}

pub async fn list_boards(client: &SurrealClient) -> Result<Vec<SurrealBoard>, surrealdb::Error> {
    let mut response = client
        .query(
            r#"
            SELECT meta::id(id) as id, name, description, <string>created_at as created_at
            FROM boards
            ORDER BY created_at DESC;
            "#,
        )
        .await?;
    let boards: Vec<SurrealBoard> = response.take(0)?;
    Ok(boards)
}

pub async fn create_topic(
    client: &SurrealClient,
    board_id: &str,
    subject: &str,
    author: &str,
) -> Result<SurrealTopic, surrealdb::Error> {
    let board_id = board_id.to_owned();
    let subject = subject.to_owned();
    let author = author.to_owned();
    let mut response = client
        .query(
            r#"
            CREATE topics CONTENT {
                board_id: $board_id,
                subject: $subject,
                author: $author,
                created_at: time::now(),
                updated_at: time::now()
            } RETURN meta::id(id) as id, board_id, subject, author,
                     <string>created_at as created_at,
                     <string>updated_at as updated_at;
            "#,
        )
        .bind(("board_id", board_id.clone()))
        .bind(("subject", subject.clone()))
        .bind(("author", author.clone()))
        .await?;

    let topic: Option<SurrealTopic> = response.take(0)?;
    Ok(topic.unwrap_or_else(|| SurrealTopic {
        id: None,
        board_id,
        subject,
        author,
        created_at: None,
        updated_at: None,
    }))
}

pub async fn list_topics(
    client: &SurrealClient,
    board_id: &str,
) -> Result<Vec<SurrealTopic>, surrealdb::Error> {
    let board_id = board_id.to_owned();
    let mut response = client
        .query(
            r#"
            SELECT meta::id(id) as id, board_id, subject, author,
                   <string>created_at as created_at,
                   <string>updated_at as updated_at
            FROM topics
            WHERE board_id = $board_id
            ORDER BY created_at DESC
            LIMIT 50;
            "#,
        )
        .bind(("board_id", board_id))
        .await?;
    let topics: Vec<SurrealTopic> = response.take(0)?;
    Ok(topics)
}

pub async fn get_topic(
    client: &SurrealClient,
    topic_id: &str,
) -> Result<Option<SurrealTopic>, surrealdb::Error> {
    let topic_id = topic_id.to_owned();
    let mut response = client
        .query(
            r#"
            SELECT meta::id(id) as id, board_id, subject, author,
                   <string>created_at as created_at,
                   <string>updated_at as updated_at
            FROM topics
            WHERE meta::id(id) = $topic_id
            LIMIT 1;
            "#,
        )
        .bind(("topic_id", topic_id))
        .await?;
    let topic: Option<SurrealTopic> = response.take(0)?;
    Ok(topic)
}

pub async fn create_post_in_topic(
    client: &SurrealClient,
    topic_id: &str,
    board_id: &str,
    subject: &str,
    body: &str,
    author: &str,
) -> Result<SurrealPost, surrealdb::Error> {
    let topic_id = topic_id.to_owned();
    let board_id = board_id.to_owned();
    let subject = subject.to_owned();
    let body = body.to_owned();
    let author = author.to_owned();
    let mut response = client
        .query(
            r#"
            CREATE posts CONTENT {
                topic_id: $topic_id,
                board_id: $board_id,
                subject: $subject,
                body: $body,
                author: $author,
                created_at: time::now()
            } RETURN meta::id(id) as id, topic_id, board_id, subject, body, author,
                     <string>created_at as created_at;
            "#,
        )
        .bind(("topic_id", topic_id.clone()))
        .bind(("board_id", board_id.clone()))
        .bind(("subject", subject.clone()))
        .bind(("body", body.clone()))
        .bind(("author", author.clone()))
        .await?;

    let post: Option<SurrealPost> = response.take(0)?;
    Ok(post.unwrap_or_else(|| SurrealPost {
        id: None,
        topic_id: Some(topic_id),
        board_id: Some(board_id),
        subject,
        body,
        author,
        created_at: None,
    }))
}

pub async fn list_posts_for_topic(
    client: &SurrealClient,
    topic_id: &str,
) -> Result<Vec<SurrealPost>, surrealdb::Error> {
    let topic_id = topic_id.to_owned();
    let mut response = client
        .query(
            r#"
            SELECT meta::id(id) as id, topic_id, board_id, subject, body, author,
                   <string>created_at as created_at
            FROM posts
            WHERE topic_id = $topic_id
            ORDER BY created_at ASC
            LIMIT 200;
            "#,
        )
        .bind(("topic_id", topic_id))
        .await?;

    let posts: Vec<SurrealPost> = response.take(0)?;
    Ok(posts)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, SurrealValue)]
pub struct SurrealUser {
    pub id: Option<String>,
    pub name: String,
    pub role: Option<String>,
    pub permissions: Option<Vec<String>>,
    pub password_hash: Option<String>,
    pub created_at: Option<String>,
}

impl SurrealUser {
    /// Provide a stable numeric identifier for legacy code paths that still expect i64 ids.
    /// Prefer numeric record ids when present; otherwise hash the username.
    pub fn legacy_id(&self) -> i64 {
        if let Some(id) = self
            .id
            .as_deref()
            .and_then(|rid| rid.split(':').last())
            .and_then(|s| s.parse().ok())
        {
            if id != 0 {
                return id;
            }
        }
        let mut hasher = DefaultHasher::new();
        self.name.hash(&mut hasher);
        let hashed = (hasher.finish() & 0x7FFF_FFFF_FFFF_FFFF) as i64;
        if hashed == 0 {
            1
        } else {
            hashed
        }
    }
}

pub async fn get_user_by_name(
    client: &SurrealClient,
    name: &str,
) -> Result<Option<SurrealUser>, surrealdb::Error> {
    let name = name.to_owned();
    let mut response = client
        .query(
            r#"
            SELECT meta::id(id) as id, name, role, permissions, password_hash, type::string(created_at) as created_at
            FROM users
            WHERE name = $name
            LIMIT 1;
            "#,
        )
        .bind(("name", name))
        .await?;
    let user: Option<SurrealUser> = response.take(0)?;
    Ok(user)
}

pub async fn create_user(
    client: &SurrealClient,
    name: &str,
    role: Option<&str>,
    permissions: Option<&[String]>,
    password_hash: Option<&str>,
) -> Result<SurrealUser, surrealdb::Error> {
    let name = name.to_owned();
    let role = role
        .map(|r| r.to_owned())
        .unwrap_or_else(|| "member".into());
    let perms = permissions.map(|p| p.to_owned()).unwrap_or_default();
    let mut response = client
        .query(
            r#"
            CREATE users CONTENT {
                name: $name,
                role: $role,
                permissions: $permissions,
                password_hash: $password_hash,
                created_at: time::now()
            } RETURN meta::id(id) as id, name, role, permissions, password_hash, type::string(created_at) as created_at;
            "#,
        )
        .bind(("name", name.clone()))
        .bind(("role", role.clone()))
        .bind(("permissions", perms.clone()))
        .bind(("password_hash", password_hash.map(|s| s.to_string())))
        .await?;
    let user: Option<SurrealUser> = response.take(0)?;
    Ok(user.unwrap_or_else(|| SurrealUser {
        id: None,
        name,
        role: Some(role),
        permissions: Some(perms),
        password_hash: password_hash.map(|s| s.to_string()),
        created_at: None,
    }))
}

pub async fn ensure_user(
    client: &SurrealClient,
    name: &str,
    role: Option<&str>,
    permissions: Option<&[String]>,
) -> Result<SurrealUser, surrealdb::Error> {
    if let Some(user) = get_user_by_name(client, name).await? {
        let is_admin = user.role.as_deref() == Some("admin");
        let has_manage = user
            .permissions
            .as_ref()
            .map(|p| p.iter().any(|perm| perm == "manage_boards"))
            .unwrap_or(false);
        if is_admin || has_manage {
            return Ok(user);
        }

        #[derive(Debug, Clone, SurrealValue)]
        struct CountRow {
            count: i64,
        }
        let mut result = client
            .query("SELECT count() AS count FROM users WHERE role = 'admin' GROUP ALL;")
            .await?;
        let rows: Vec<CountRow> = result.take(0).unwrap_or_default();
        let has_admin = rows.first().map(|row| row.count > 0).unwrap_or(false);
        if !has_admin {
            let perms = vec![
                "manage_boards".to_string(),
                "post_new".to_string(),
                "post_reply_any".to_string(),
            ];
            let mut response = client
                .query(
                    r#"
                    UPDATE users SET role = $role, permissions = $permissions
                    WHERE name = $name
                    RETURN meta::id(id) as id, name, role, permissions, password_hash, type::string(created_at) as created_at;
                    "#,
                )
                .bind(("role", "admin".to_string()))
                .bind(("permissions", perms.clone()))
                .bind(("name", name.to_string()))
                .await?;
            let updated: Option<SurrealUser> = response.take(0)?;
            return Ok(updated.unwrap_or_else(|| SurrealUser {
                id: user.id,
                name: user.name,
                role: Some("admin".to_string()),
                permissions: Some(perms),
                password_hash: user.password_hash,
                created_at: user.created_at,
            }));
        }
        return Ok(user);
    }
    let mut seed_role = role;
    let mut seed_permissions_ref = permissions;

    if role.is_none() && permissions.is_none() {
        #[derive(Debug, Clone, SurrealValue)]
        struct CountRow {
            count: i64,
        }
        let mut result = client
            .query("SELECT count() AS count FROM users GROUP ALL;")
            .await?;
        let rows: Vec<CountRow> = result.take(0).unwrap_or_default();
        let is_first_user = rows.first().map(|row| row.count == 0).unwrap_or(true);
        if is_first_user {
            seed_role = Some("admin");
            let seed_permissions = vec![
                "manage_boards".to_string(),
                "post_new".to_string(),
                "post_reply_any".to_string(),
            ];
            seed_permissions_ref = Some(seed_permissions.as_slice());
            return create_user(client, name, seed_role, seed_permissions_ref, None).await;
        }
    }

    create_user(client, name, seed_role, seed_permissions_ref, None).await
}

/// Thin service wrapper to encapsulate SurrealDB forum operations.
#[derive(Clone)]
pub struct SurrealForumService {
    client: SurrealClient,
}

impl SurrealForumService {
    pub fn new(client: SurrealClient) -> Self {
        Self { client }
    }

    pub fn client(&self) -> &SurrealClient {
        &self.client
    }

    /// Lightweight connectivity check.
    pub async fn health(&self) -> Result<(), surrealdb::Error> {
        self.client.query("RETURN true;").await?;
        Ok(())
    }

    pub async fn create_demo_post(
        &self,
        subject: &str,
        body: &str,
        user: &str,
    ) -> Result<Value, surrealdb::Error> {
        create_demo_post(&self.client, subject, body, user).await
    }

    pub async fn create_board(
        &self,
        name: &str,
        description: Option<&str>,
    ) -> Result<SurrealBoard, surrealdb::Error> {
        match create_board(&self.client, name, description).await {
            Ok(board) => Ok(board),
            Err(err)
                if is_surreal_unauthorized(&err) || is_surreal_connection_uninitialised(&err) =>
            {
                warn!(
                    error = %err,
                    "create_board failed due to auth/connection state, trying reauth"
                );
                if let Err(reauth_err) = reauth_from_env(&self.client).await {
                    warn!(error = %reauth_err, "create_board reauth failed, trying reconnect");
                }
                match create_board(&self.client, name, description).await {
                    Ok(board) => Ok(board),
                    Err(retry_err)
                        if is_surreal_unauthorized(&retry_err)
                            || is_surreal_connection_uninitialised(&retry_err) =>
                    {
                        warn!(
                            error = %retry_err,
                            "create_board still failing after reauth, rebuilding surreal client"
                        );
                        let fresh = connect_from_env().await?;
                        create_board(&fresh, name, description).await
                    }
                    Err(retry_err) => Err(retry_err),
                }
            }
            Err(err) => Err(err),
        }
    }

    pub async fn list_boards(&self) -> Result<Vec<SurrealBoard>, surrealdb::Error> {
        match list_boards(&self.client).await {
            Ok(boards) => Ok(boards),
            Err(err) if is_surreal_unauthorized(&err) => {
                warn!(error = %err, "list_boards unauthorized, trying reauth");
                if let Err(reauth_err) = reauth_from_env(&self.client).await {
                    warn!(error = %reauth_err, "list_boards reauth failed, trying reconnect");
                }
                match list_boards(&self.client).await {
                    Ok(boards) => Ok(boards),
                    Err(retry_err) if is_surreal_unauthorized(&retry_err) => {
                        warn!(error = %retry_err, "list_boards still unauthorized after reauth, rebuilding surreal client");
                        let fresh = connect_from_env().await?;
                        list_boards(&fresh).await
                    }
                    Err(retry_err) => Err(retry_err),
                }
            }
            Err(err) => Err(err),
        }
    }

    pub async fn create_topic(
        &self,
        board_id: &str,
        subject: &str,
        author: &str,
    ) -> Result<SurrealTopic, surrealdb::Error> {
        match create_topic(&self.client, board_id, subject, author).await {
            Ok(topic) => Ok(topic),
            Err(err)
                if is_surreal_unauthorized(&err) || is_surreal_connection_uninitialised(&err) =>
            {
                warn!(
                    error = %err,
                    board_id = %board_id,
                    "create_topic failed due to auth/connection state, trying reauth"
                );
                if let Err(reauth_err) = reauth_from_env(&self.client).await {
                    warn!(error = %reauth_err, board_id = %board_id, "create_topic reauth failed, trying reconnect");
                }
                match create_topic(&self.client, board_id, subject, author).await {
                    Ok(topic) => Ok(topic),
                    Err(retry_err)
                        if is_surreal_unauthorized(&retry_err)
                            || is_surreal_connection_uninitialised(&retry_err) =>
                    {
                        warn!(
                            error = %retry_err,
                            board_id = %board_id,
                            "create_topic still failing after reauth, rebuilding surreal client"
                        );
                        let fresh = connect_from_env().await?;
                        create_topic(&fresh, board_id, subject, author).await
                    }
                    Err(retry_err) => Err(retry_err),
                }
            }
            Err(err) => Err(err),
        }
    }

    pub async fn list_topics(&self, board_id: &str) -> Result<Vec<SurrealTopic>, surrealdb::Error> {
        match list_topics(&self.client, board_id).await {
            Ok(topics) => Ok(topics),
            Err(err) if is_surreal_unauthorized(&err) => {
                warn!(error = %err, board_id = %board_id, "list_topics unauthorized, trying reauth");
                if let Err(reauth_err) = reauth_from_env(&self.client).await {
                    warn!(error = %reauth_err, board_id = %board_id, "list_topics reauth failed, trying reconnect");
                }
                match list_topics(&self.client, board_id).await {
                    Ok(topics) => Ok(topics),
                    Err(retry_err) if is_surreal_unauthorized(&retry_err) => {
                        warn!(error = %retry_err, board_id = %board_id, "list_topics still unauthorized after reauth, rebuilding surreal client");
                        let fresh = connect_from_env().await?;
                        list_topics(&fresh, board_id).await
                    }
                    Err(retry_err) => Err(retry_err),
                }
            }
            Err(err) => Err(err),
        }
    }

    pub async fn get_topic(
        &self,
        topic_id: &str,
    ) -> Result<Option<SurrealTopic>, surrealdb::Error> {
        match get_topic(&self.client, topic_id).await {
            Ok(topic) => Ok(topic),
            Err(err) if is_surreal_unauthorized(&err) => {
                warn!(error = %err, topic_id = %topic_id, "get_topic unauthorized, trying reauth");
                if let Err(reauth_err) = reauth_from_env(&self.client).await {
                    warn!(error = %reauth_err, topic_id = %topic_id, "get_topic reauth failed, trying reconnect");
                }
                match get_topic(&self.client, topic_id).await {
                    Ok(topic) => Ok(topic),
                    Err(retry_err) if is_surreal_unauthorized(&retry_err) => {
                        warn!(error = %retry_err, topic_id = %topic_id, "get_topic still unauthorized after reauth, rebuilding surreal client");
                        let fresh = connect_from_env().await?;
                        get_topic(&fresh, topic_id).await
                    }
                    Err(retry_err) => Err(retry_err),
                }
            }
            Err(err) => Err(err),
        }
    }

    pub async fn create_post(
        &self,
        subject: &str,
        body: &str,
        user: &str,
    ) -> Result<SurrealPost, surrealdb::Error> {
        create_post(&self.client, subject, body, user).await
    }

    pub async fn create_post_in_topic(
        &self,
        topic_id: &str,
        board_id: &str,
        subject: &str,
        body: &str,
        author: &str,
    ) -> Result<SurrealPost, surrealdb::Error> {
        match create_post_in_topic(&self.client, topic_id, board_id, subject, body, author).await {
            Ok(post) => Ok(post),
            Err(err)
                if is_surreal_unauthorized(&err) || is_surreal_connection_uninitialised(&err) =>
            {
                warn!(
                    error = %err,
                    topic_id = %topic_id,
                    board_id = %board_id,
                    "create_post_in_topic failed due to auth/connection state, trying reauth"
                );
                if let Err(reauth_err) = reauth_from_env(&self.client).await {
                    warn!(
                        error = %reauth_err,
                        topic_id = %topic_id,
                        board_id = %board_id,
                        "create_post_in_topic reauth failed, trying reconnect"
                    );
                }
                match create_post_in_topic(&self.client, topic_id, board_id, subject, body, author)
                    .await
                {
                    Ok(post) => Ok(post),
                    Err(retry_err)
                        if is_surreal_unauthorized(&retry_err)
                            || is_surreal_connection_uninitialised(&retry_err) =>
                    {
                        warn!(
                            error = %retry_err,
                            topic_id = %topic_id,
                            board_id = %board_id,
                            "create_post_in_topic still failing after reauth, rebuilding surreal client"
                        );
                        let fresh = connect_from_env().await?;
                        create_post_in_topic(&fresh, topic_id, board_id, subject, body, author)
                            .await
                    }
                    Err(retry_err) => Err(retry_err),
                }
            }
            Err(err) => Err(err),
        }
    }

    pub async fn list_posts_for_topic(
        &self,
        topic_id: &str,
    ) -> Result<Vec<SurrealPost>, surrealdb::Error> {
        match list_posts_for_topic(&self.client, topic_id).await {
            Ok(posts) => Ok(posts),
            Err(err) if is_surreal_unauthorized(&err) => {
                warn!(error = %err, topic_id = %topic_id, "list_posts_for_topic unauthorized, trying reauth");
                if let Err(reauth_err) = reauth_from_env(&self.client).await {
                    warn!(error = %reauth_err, topic_id = %topic_id, "list_posts_for_topic reauth failed, trying reconnect");
                }
                match list_posts_for_topic(&self.client, topic_id).await {
                    Ok(posts) => Ok(posts),
                    Err(retry_err) if is_surreal_unauthorized(&retry_err) => {
                        warn!(error = %retry_err, topic_id = %topic_id, "list_posts_for_topic still unauthorized after reauth, rebuilding surreal client");
                        let fresh = connect_from_env().await?;
                        list_posts_for_topic(&fresh, topic_id).await
                    }
                    Err(retry_err) => Err(retry_err),
                }
            }
            Err(err) => Err(err),
        }
    }

    pub async fn list_posts(&self) -> Result<Vec<SurrealPost>, surrealdb::Error> {
        match list_posts(&self.client).await {
            Ok(posts) => Ok(posts),
            Err(err) if is_surreal_unauthorized(&err) => {
                warn!(error = %err, "list_posts unauthorized, trying reauth");
                if let Err(reauth_err) = reauth_from_env(&self.client).await {
                    warn!(error = %reauth_err, "list_posts reauth failed, trying reconnect");
                }
                match list_posts(&self.client).await {
                    Ok(posts) => Ok(posts),
                    Err(retry_err) if is_surreal_unauthorized(&retry_err) => {
                        warn!(error = %retry_err, "list_posts still unauthorized after reauth, rebuilding surreal client");
                        let fresh = connect_from_env().await?;
                        list_posts(&fresh).await
                    }
                    Err(retry_err) => Err(retry_err),
                }
            }
            Err(err) => Err(err),
        }
    }

    pub async fn create_attachment_meta(
        &self,
        owner: &str,
        filename: &str,
        size_bytes: i64,
        mime_type: Option<&str>,
        board_id: Option<&str>,
        topic_id: Option<&str>,
    ) -> Result<SurrealAttachment, surrealdb::Error> {
        create_attachment_meta(
            &self.client,
            owner,
            filename,
            size_bytes,
            mime_type,
            board_id,
            topic_id,
        )
        .await
    }

    pub async fn list_attachments_for_user(
        &self,
        owner: &str,
    ) -> Result<Vec<SurrealAttachment>, surrealdb::Error> {
        list_attachments_for_user(&self.client, owner).await
    }

    pub async fn create_notification(
        &self,
        user: &str,
        subject: &str,
        body: &str,
    ) -> Result<SurrealNotification, surrealdb::Error> {
        create_notification(&self.client, user, subject, body).await
    }

    pub async fn list_notifications(
        &self,
        user: &str,
    ) -> Result<Vec<SurrealNotification>, surrealdb::Error> {
        list_notifications(&self.client, user).await
    }

    pub async fn mark_notification_read(&self, id: &str) -> Result<(), surrealdb::Error> {
        mark_notification_read(&self.client, id).await
    }

    pub async fn ensure_user(
        &self,
        name: &str,
        role: Option<&str>,
        permissions: Option<&[String]>,
    ) -> Result<SurrealUser, surrealdb::Error> {
        match ensure_user(&self.client, name, role, permissions).await {
            Ok(user) => Ok(user),
            Err(err) if is_surreal_unauthorized(&err) => {
                warn!(error = %err, user = %name, "ensure_user unauthorized, trying reauth");
                if let Err(reauth_err) = reauth_from_env(&self.client).await {
                    warn!(error = %reauth_err, user = %name, "ensure_user reauth failed, trying reconnect");
                }
                match ensure_user(&self.client, name, role, permissions).await {
                    Ok(user) => Ok(user),
                    Err(retry_err) if is_surreal_unauthorized(&retry_err) => {
                        warn!(error = %retry_err, user = %name, "ensure_user still unauthorized after reauth, rebuilding surreal client");
                        let fresh = connect_from_env().await?;
                        ensure_user(&fresh, name, role, permissions).await
                    }
                    Err(retry_err) => Err(retry_err),
                }
            }
            Err(err) => Err(err),
        }
    }

    pub async fn user_by_name(&self, name: &str) -> Result<Option<SurrealUser>, surrealdb::Error> {
        get_user_by_name(&self.client, name).await
    }

    pub async fn create_user_with_password(
        &self,
        name: &str,
        role: Option<&str>,
        permissions: Option<&[String]>,
        password_hash: Option<&str>,
    ) -> Result<SurrealUser, surrealdb::Error> {
        create_user(&self.client, name, role, permissions, password_hash).await
    }
}
