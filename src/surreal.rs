use std::collections::hash_map::DefaultHasher;
use std::env;
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use surrealdb::{
    Surreal,
    engine::remote::http::{Client, Http},
    opt::auth::Root,
};
use tracing::info;

pub type SurrealClient = Surreal<Client>;

fn normalize_endpoint(raw: String) -> String {
    // Surreal's HTTP client expects a host:port string. Strip any scheme and trailing slash to
    // avoid building URLs like `http://http://host:port/health`.
    let ep = raw.trim().trim_end_matches('/').to_string();
    ep.strip_prefix("http://")
        .or_else(|| ep.strip_prefix("https://"))
        .unwrap_or(&ep)
        .to_string()
}

/// Connect to SurrealDB using environment variables, defaults to local root account.
pub async fn connect_from_env() -> Result<SurrealClient, surrealdb::Error> {
    let endpoint_raw =
        env::var("SURREAL_ENDPOINT").unwrap_or_else(|_| "http://127.0.0.1:8000".into());
    let endpoint = normalize_endpoint(endpoint_raw);
    let ns = env::var("SURREAL_NAMESPACE").unwrap_or_else(|_| "auth".into());
    let db = env::var("SURREAL_DATABASE").unwrap_or_else(|_| "main".into());
    let user = env::var("SURREAL_USER").unwrap_or_else(|_| "root".into());
    let pass = env::var("SURREAL_PASS").unwrap_or_else(|_| "root".into());

    info!(endpoint, namespace = %ns, database = %db, "connecting to SurrealDB (HTTP)");
    let client = Surreal::new::<Http>(&endpoint).await?;
    client
        .signin(Root {
            username: &user,
            password: &pass,
        })
        .await?;
    client.use_ns(&ns).use_db(&db).await?;
    Ok(client)
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SurrealPost {
    pub id: Option<String>,
    pub topic_id: Option<String>,
    pub board_id: Option<String>,
    pub subject: String,
    pub body: String,
    pub author: String,
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SurrealTopic {
    pub id: Option<String>,
    pub board_id: String,
    pub subject: String,
    pub author: String,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
            } RETURN meta::id(id) as id, topic_id, board_id, subject, body, author, created_at;
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
            SELECT meta::id(id) as id, topic_id, board_id, subject, body, author, created_at
            FROM posts
            ORDER BY created_at DESC
            LIMIT 50;
            "#,
        )
        .await?;

    let posts: Vec<SurrealPost> = response.take(0)?;
    Ok(posts)
}

#[derive(Debug, Serialize, Deserialize, Clone)]
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

#[derive(Debug, Serialize, Deserialize, Clone)]
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
    let name = name.to_owned();
    let description_owned = description.map(|d| d.to_owned());
    let mut response = client
        .query(
            r#"
            CREATE boards CONTENT {
                name: $name,
                description: $description,
                created_at: time::now()
            } RETURN meta::id(id) as id, name, description, created_at;
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
            SELECT meta::id(id) as id, name, description, created_at
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
            } RETURN meta::id(id) as id, board_id, subject, author, created_at, updated_at;
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
            SELECT meta::id(id) as id, board_id, subject, author, created_at, updated_at
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
            } RETURN meta::id(id) as id, topic_id, board_id, subject, body, author, created_at;
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
            SELECT meta::id(id) as id, topic_id, board_id, subject, body, author, created_at
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
        if hashed == 0 { 1 } else { hashed }
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
            SELECT meta::id(id) as id, name, role, permissions, password_hash, created_at
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
            } RETURN meta::id(id) as id, name, role, permissions, password_hash, created_at;
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

        #[derive(Deserialize)]
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
                    RETURN meta::id(id) as id, name, role, permissions, password_hash, created_at;
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
    let mut seed_permissions: Vec<String> = Vec::new();
    let mut seed_role = role;
    let mut seed_permissions_ref = permissions;

    if role.is_none() && permissions.is_none() {
        #[derive(Deserialize)]
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
            seed_permissions = vec![
                "manage_boards".to_string(),
                "post_new".to_string(),
                "post_reply_any".to_string(),
            ];
            seed_permissions_ref = Some(seed_permissions.as_slice());
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
        create_board(&self.client, name, description).await
    }

    pub async fn list_boards(&self) -> Result<Vec<SurrealBoard>, surrealdb::Error> {
        list_boards(&self.client).await
    }

    pub async fn create_topic(
        &self,
        board_id: &str,
        subject: &str,
        author: &str,
    ) -> Result<SurrealTopic, surrealdb::Error> {
        create_topic(&self.client, board_id, subject, author).await
    }

    pub async fn list_topics(&self, board_id: &str) -> Result<Vec<SurrealTopic>, surrealdb::Error> {
        list_topics(&self.client, board_id).await
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
        create_post_in_topic(&self.client, topic_id, board_id, subject, body, author).await
    }

    pub async fn list_posts_for_topic(
        &self,
        topic_id: &str,
    ) -> Result<Vec<SurrealPost>, surrealdb::Error> {
        list_posts_for_topic(&self.client, topic_id).await
    }

    pub async fn list_posts(&self) -> Result<Vec<SurrealPost>, surrealdb::Error> {
        list_posts(&self.client).await
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
        ensure_user(&self.client, name, role, permissions).await
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
