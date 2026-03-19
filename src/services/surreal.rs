use crate::services::{
    ActionLogEntry, AttachmentInfo, AttachmentRecord, AttachmentUpload, BanLogEntry, BanRule,
    BoardAccessEntry, BoardListOptions, BoardNotificationSummary, BoardSummary, CalendarEvent,
    DraftStorage, ForumContext, ForumError, ForumService, GroupAssignType, GroupMember,
    MemberRecord, MembergroupData, MembergroupListEntry, MembergroupListType, MembergroupSettings,
    MembergroupSummary, MentionRecord, MessageData, MessageEditData, NotifyPrefs, PermissionChange,
    PermissionGroupContext, PermissionProfile, PermissionSnapshot, PersonalMessageDetail,
    PersonalMessageFolder, PersonalMessageLabel, PersonalMessageOverview, PersonalMessagePage,
    PersonalMessagePeer, PersonalMessageSearchQuery, PersonalMessageSendResult,
    PersonalMessageSummary, PmDraftRecord, PmPreferenceState, PollData, PostedMessage,
    QuoteContent, SendPersonalMessage, SessionCheckMode, TopicLogEntry, TopicNotificationSummary,
    TopicPostingContext,
};
use crate::surreal::{
    connect_from_env, create_board as surreal_create_board,
    create_post_in_topic as surreal_create_post_in_topic, create_topic as surreal_create_topic,
    get_user_by_name, list_boards as surreal_list_boards,
    list_posts_for_topic as surreal_list_posts_for_topic, reauth_from_env, SurrealClient,
};
use chrono::{TimeZone, Utc};
use serde_json::Value;
use std::hash::{DefaultHasher, Hash, Hasher};
use surrealdb::types::SurrealValue;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, SurrealValue)]
struct PostRow {
    id: Option<String>,
    topic_id: String,
    board_id: String,
    subject: String,
    body: String,
    author: String,
    created_at: Option<String>,
}

impl PostRow {
    fn topic_id_numeric(&self) -> Option<i64> {
        self.topic_id.split(':').last().and_then(|s| s.parse().ok())
    }
}

/// Minimal Surreal-backed ForumService implementation.
///
/// Only a subset of methods are implemented to support posting workflows used by controllers.
#[derive(Clone)]
pub struct SurrealService {
    client: SurrealClient,
}

impl SurrealService {
    pub fn new(client: SurrealClient) -> Self {
        Self { client }
    }

    fn fetch_post_row(&self, msg_id: i64) -> Result<Option<PostRow>, ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let rid = format!("posts:{msg_id}");
        let mut response = rt
            .block_on(async {
                self.client
                    .query(
                        r#"
                        SELECT meta::id(id) as id, topic_id, board_id, subject, body, author, created_at
                        FROM posts
                        WHERE meta::id(id) = $rid
                        LIMIT 1;
                        "#,
                    )
                    .bind(("rid", rid))
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        let row: Option<PostRow> = response
            .take(0)
            .ok()
            .and_then(|mut v: Vec<PostRow>| v.pop());
        Ok(row)
    }

    fn ensure_board(&self, ctx: &ForumContext) -> Result<String, ForumError> {
        // Use ctx.board_id if provided; otherwise fetch or create a default board.
        if let Some(id) = ctx
            .board_info
            .string("surreal_id")
            .or_else(|| ctx.context.string("surreal_board_id"))
        {
            return Ok(id);
        }
        if let Some(board_id) = ctx.board_id {
            // Caller provided a numeric id; convert to string for Surreal queries.
            return Ok(board_id.to_string());
        }

        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let boards = rt
            .block_on(surreal_list_boards(&self.client))
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        if let Some(id) = boards.first().and_then(|b| b.id.clone()) {
            return Ok(id);
        }
        let created = rt
            .block_on(surreal_create_board(
                &self.client,
                "General",
                Some("Default board"),
            ))
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        Ok(created.id.unwrap_or_else(|| "board:default".into()))
    }

    fn parse_ts(ms: i64) -> chrono::DateTime<Utc> {
        Utc.timestamp_millis_opt(ms)
            .single()
            .unwrap_or_else(Utc::now)
    }

    fn is_surreal_unauthorized(err: &surrealdb::Error) -> bool {
        let msg = err.to_string();
        msg.contains("401 Unauthorized") || msg.contains("status client error (401")
    }
}

impl ForumService for SurrealService {
    fn load_language(&self, _ctx: &mut ForumContext, _lang: &str) -> Result<(), ForumError> {
        Ok(())
    }

    fn load_template(&self, _ctx: &mut ForumContext, _template: &str) -> Result<(), ForumError> {
        Ok(())
    }

    fn call_hook(&self, _ctx: &mut ForumContext, _hook: &str) -> Result<(), ForumError> {
        Ok(())
    }

    fn get_notify_prefs(&self, _user_id: i64) -> Result<NotifyPrefs, ForumError> {
        Ok(NotifyPrefs {
            msg_auto_notify: false,
        })
    }

    fn boards_allowed_to(
        &self,
        _ctx: &ForumContext,
        _permissions: &[String],
    ) -> Result<Vec<i64>, ForumError> {
        Ok(vec![0])
    }

    fn get_board_list(
        &self,
        _ctx: &ForumContext,
        _options: &BoardListOptions,
    ) -> Result<Vec<BoardSummary>, ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let boards = rt
            .block_on(surreal_list_boards(&self.client))
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        Ok(boards
            .into_iter()
            .enumerate()
            .map(|(idx, b)| BoardSummary {
                id: idx as i64 + 1,
                name: b.name,
            })
            .collect())
    }

    fn check_session(
        &self,
        _ctx: &ForumContext,
        _mode: SessionCheckMode,
    ) -> Result<(), ForumError> {
        Ok(())
    }

    fn allowed_to(
        &self,
        ctx: &ForumContext,
        permission: &str,
        _boards: Option<&[i64]>,
        _any: bool,
    ) -> bool {
        if ctx.user_info.is_admin {
            return true;
        }
        if ctx.user_info.is_mod && permission.starts_with("post_") {
            return true;
        }
        ctx.user_info.permissions.contains(permission)
    }

    fn redirect_exit(&self, _url: &str) -> Result<(), ForumError> {
        Ok(())
    }

    fn find_topic_id_by_msg(&self, _msg_id: i64) -> Result<Option<i64>, ForumError> {
        Ok(self
            .fetch_post_row(_msg_id)?
            .and_then(|p| p.topic_id_numeric()))
    }

    fn fetch_topic_posting_context(
        &self,
        _topic_id: i64,
    ) -> Result<Option<TopicPostingContext>, ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let rid = format!("topics:{_topic_id}");
        let mut response = rt
            .block_on(async {
                self.client
                    .query(
                        r#"
                        SELECT board_id, subject, created_at, updated_at
                        FROM topics
                        WHERE meta::id(id) = $rid
                        LIMIT 1;
                        "#,
                    )
                    .bind(("rid", rid))
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        #[derive(Debug, Clone, SurrealValue)]
        struct TopicRow {
            board_id: Option<String>,
            subject: Option<String>,
        }
        let topic: Option<TopicRow> = response
            .take(0)
            .ok()
            .and_then(|mut v: Vec<TopicRow>| v.pop());
        let Some(topic) = topic else {
            return Ok(None);
        };

        // Last/first post ids
        let mut last_resp = rt
            .block_on(async {
                self.client
                    .query(
                        r#"
                        SELECT meta::id(id) as id, created_at
                        FROM posts
                        WHERE topic_id = $tid
                        ORDER BY created_at DESC
                        LIMIT 1;
                        "#,
                    )
                    .bind(("tid", format!("topics:{_topic_id}")))
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        let mut first_resp = rt
            .block_on(async {
                self.client
                    .query(
                        r#"
                        SELECT meta::id(id) as id, created_at
                        FROM posts
                        WHERE topic_id = $tid
                        ORDER BY created_at ASC
                        LIMIT 1;
                        "#,
                    )
                    .bind(("tid", format!("topics:{_topic_id}")))
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        #[derive(Debug, Clone, SurrealValue)]
        struct MsgRow {
            id: Option<String>,
            created_at: Option<String>,
        }
        let last: Option<MsgRow> = last_resp
            .take(0)
            .ok()
            .and_then(|mut v: Vec<MsgRow>| v.pop());
        let first: Option<MsgRow> = first_resp
            .take(0)
            .ok()
            .and_then(|mut v: Vec<MsgRow>| v.pop());

        let parse_dt = |s: Option<String>| {
            s.and_then(|v| chrono::DateTime::parse_from_rfc3339(&v).ok())
                .map(|dt| dt.with_timezone(&Utc))
        };

        Ok(Some(TopicPostingContext {
            locked: false,
            approved: true,
            notify: false,
            sticky: false,
            board_id: topic
                .board_id
                .as_deref()
                .and_then(|id| id.split(':').last())
                .and_then(|s| s.parse().ok())
                .unwrap_or_default(),
            poll_id: None,
            last_msg_id: last
                .as_ref()
                .and_then(|m| m.id.as_deref())
                .and_then(|id| id.split(':').last())
                .and_then(|s| s.parse().ok()),
            first_msg_id: first
                .as_ref()
                .and_then(|m| m.id.as_deref())
                .and_then(|id| id.split(':').last())
                .and_then(|s| s.parse().ok()),
            id_member_started: 0,
            subject: topic.subject,
            last_post_time: last.and_then(|m| parse_dt(m.created_at)),
        }))
    }

    fn fetch_message_edit_data(
        &self,
        _topic_id: i64,
        _msg_id: i64,
    ) -> Result<Option<MessageEditData>, ForumError> {
        let Some(post) = self.fetch_post_row(_msg_id)? else {
            return Ok(None);
        };
        let attachments: Vec<AttachmentInfo> = self
            .list_message_attachments(_msg_id)?
            .into_iter()
            .map(|a| AttachmentInfo {
                id: a.id,
                filename: a.name,
                size: a.size,
                approved: a.approved,
            })
            .collect();
        Ok(Some(MessageEditData {
            id_member: 0,
            topic_id: post.topic_id_numeric().unwrap_or(_topic_id),
            subject: post.subject,
            body: post.body,
            icon: "xx".into(),
            smileys_enabled: true,
            approved: true,
            attachments,
        }))
    }

    fn persist_post(
        &self,
        ctx: &ForumContext,
        submission: crate::services::PostSubmission,
    ) -> Result<PostedMessage, ForumError> {
        let board_id = self.ensure_board(ctx)?;
        let subject = if submission.subject.is_empty() {
            "(no subject)".to_string()
        } else {
            submission.subject
        };
        let body = submission.body;
        let author = ctx.user_info.name.clone();

        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;

        // If no topic_id, create a topic first.
        let topic_id = if let Some(tid) = submission.topic_id {
            tid.to_string()
        } else {
            let topic = rt
                .block_on(surreal_create_topic(
                    &self.client,
                    &board_id,
                    &subject,
                    &author,
                ))
                .map_err(|e| ForumError::Internal(e.to_string()))?;
            topic.id.unwrap_or_else(|| "topic:new".into())
        };

        let post = rt
            .block_on(surreal_create_post_in_topic(
                &self.client,
                &topic_id,
                &board_id,
                &subject,
                &body,
                &author,
            ))
            .map_err(|e| ForumError::Internal(e.to_string()))?;

        let message_id = post
            .id
            .and_then(|id| id.split(':').last().and_then(|s| s.parse().ok()))
            .unwrap_or(0);
        let topic_numeric = topic_id
            .split(':')
            .last()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        Ok(PostedMessage {
            topic_id: topic_numeric,
            message_id,
        })
    }

    fn fetch_quote_content(&self, _msg_id: i64) -> Result<Option<QuoteContent>, ForumError> {
        Ok(self.fetch_post_row(_msg_id)?.map(|p| QuoteContent {
            subject: p.subject,
            body: p.body,
        }))
    }

    fn send_announcement(
        &self,
        _topic_id: i64,
    ) -> Result<crate::services::AnnouncementResult, ForumError> {
        Ok(crate::services::AnnouncementResult { recipients: 0 })
    }

    fn store_attachment(&self, _upload: AttachmentUpload) -> Result<AttachmentRecord, ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let id = Utc::now().timestamp_millis();
        let mut response = rt
            .block_on(async {
                self.client
                    .query(
                        r#"
                        CREATE attachments CONTENT {
                            id: $id,
                            name: $name,
                            tmp_path: $tmp_path,
                            size: $size,
                            mime_type: $mime_type,
                            width: $width,
                            height: $height,
                            message_id: null,
                            approved: true,
                            created_at_ms: time::now()
                        } RETURN id, name, size, mime_type, width, height, message_id, approved;
                        "#,
                    )
                    .bind(("id", id))
                    .bind(("name", _upload.name.clone()))
                    .bind(("tmp_path", _upload.tmp_path.clone()))
                    .bind(("size", _upload.size))
                    .bind(("mime_type", _upload.mime_type.clone()))
                    .bind(("width", _upload.width))
                    .bind(("height", _upload.height))
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        #[derive(Debug, Clone, SurrealValue)]
        struct Row {
            id: Option<i64>,
            name: String,
            size: i64,
            mime_type: String,
            width: Option<i32>,
            height: Option<i32>,
            message_id: Option<i64>,
            approved: bool,
        }
        let row: Option<Row> = response.take(0).ok().and_then(|mut v: Vec<Row>| v.pop());
        row.map(|r| AttachmentRecord {
            id: r.id.unwrap_or(id),
            name: r.name,
            size: r.size,
            mime_type: r.mime_type,
            approved: r.approved,
            width: r.width,
            height: r.height,
            message_id: r.message_id,
        })
        .ok_or_else(|| ForumError::Internal("failed to store attachment".into()))
    }

    fn delete_attachment(&self, _id: i64) -> Result<(), ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        rt.block_on(async {
            self.client
                .query("DELETE attachments WHERE id = $id")
                .bind(("id", _id))
                .await
        })
        .map_err(|e| ForumError::Internal(e.to_string()))?;
        Ok(())
    }

    fn list_message_attachments(&self, _msg_id: i64) -> Result<Vec<AttachmentRecord>, ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let mut response = rt
            .block_on(async {
                self.client
                    .query(
                        r#"
                        SELECT id, name, size, mime_type, width, height, message_id, approved
                        FROM attachments WHERE message_id = $msg_id;
                        "#,
                    )
                    .bind(("msg_id", _msg_id))
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        #[derive(Debug, Clone, SurrealValue)]
        struct Row {
            id: Option<i64>,
            name: String,
            size: i64,
            mime_type: String,
            width: Option<i32>,
            height: Option<i32>,
            message_id: Option<i64>,
            approved: bool,
        }
        let rows: Vec<Row> = response.take(0).unwrap_or_default();
        Ok(rows
            .into_iter()
            .map(|r| AttachmentRecord {
                id: r.id.unwrap_or(0),
                name: r.name,
                size: r.size,
                mime_type: r.mime_type,
                approved: r.approved,
                width: r.width,
                height: r.height,
                message_id: r.message_id,
            })
            .collect())
    }

    fn link_attachment_to_message(
        &self,
        _attachment_id: i64,
        _msg_id: i64,
    ) -> Result<(), ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        rt.block_on(async {
            self.client
                .query("UPDATE attachments SET message_id = $msg_id WHERE id = $id")
                .bind(("msg_id", _msg_id))
                .bind(("id", _attachment_id))
                .await
        })
        .map_err(|e| ForumError::Internal(e.to_string()))?;
        Ok(())
    }

    fn current_attachment_dir(&self) -> Result<i64, ForumError> {
        Ok(0)
    }

    fn attachment_dir_usage(&self, _dir_id: i64) -> Result<(i64, i64), ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let mut response = rt
            .block_on(async {
                self.client
                    .query("SELECT count() as files, sum(size) as total FROM attachments;")
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        #[derive(Debug, Clone, SurrealValue)]
        struct Row {
            files: Option<i64>,
            total: Option<i64>,
        }
        let row: Option<Row> = response.take(0).ok().and_then(|mut v: Vec<Row>| v.pop());
        Ok((
            row.as_ref().and_then(|r| r.files).unwrap_or(0),
            row.and_then(|r| r.total).unwrap_or(0),
        ))
    }

    fn update_attachment_dir_usage(
        &self,
        _dir_id: i64,
        _size_delta: i64,
        _file_delta: i64,
    ) -> Result<(), ForumError> {
        // Attachment directories are derived from the attachments table; nothing to persist.
        Ok(())
    }

    fn save_draft_record(&self, mut _record: DraftStorage) -> Result<i64, ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let id = if _record.id == 0 {
            Utc::now().timestamp_millis()
        } else {
            _record.id
        };
        let poster_time_ms = _record.poster_time.timestamp_millis();

        rt.block_on(async {
            self.client
                .query(
                    r#"
                    UPSERT type::thing("drafts", $id) SET
                        board_id = $board_id,
                        topic_id = $topic_id,
                        subject = $subject,
                        body = $body,
                        icon = $icon,
                        smileys_enabled = $smileys_enabled,
                        locked = $locked,
                        sticky = $sticky,
                        poster_time_ms = $poster_time_ms;
                    "#,
                )
                .bind(("id", id))
                .bind(("board_id", _record.board_id))
                .bind(("topic_id", _record.topic_id))
                .bind(("subject", _record.subject.clone()))
                .bind(("body", _record.body.clone()))
                .bind(("icon", _record.icon.clone()))
                .bind(("smileys_enabled", _record.smileys_enabled))
                .bind(("locked", _record.locked))
                .bind(("sticky", _record.sticky))
                .bind(("poster_time_ms", poster_time_ms))
                .await
        })
        .map_err(|e| ForumError::Internal(e.to_string()))?;

        Ok(id)
    }

    fn delete_draft(&self, _draft_id: i64) -> Result<(), ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        rt.block_on(async {
            self.client
                .query("DELETE drafts WHERE id = $id;")
                .bind(("id", _draft_id))
                .await
        })
        .map_err(|e| ForumError::Internal(e.to_string()))?;
        Ok(())
    }

    fn read_draft(&self, _draft_id: i64) -> Result<Option<DraftStorage>, ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let mut response = rt
            .block_on(async {
                self.client
                    .query(
                        r#"
                        SELECT id, board_id, topic_id, subject, body, icon, smileys_enabled, locked, sticky, poster_time_ms
                        FROM drafts
                        WHERE id = $id
                        LIMIT 1;
                        "#,
                    )
                    .bind(("id", _draft_id))
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;

        #[derive(Debug, Clone, SurrealValue)]
        struct Row {
            id: Option<i64>,
            board_id: Option<i64>,
            topic_id: Option<i64>,
            subject: Option<String>,
            body: Option<String>,
            icon: Option<String>,
            smileys_enabled: Option<bool>,
            locked: Option<bool>,
            sticky: Option<bool>,
            poster_time_ms: Option<i64>,
        }

        let row: Option<Row> = response.take(0).ok().and_then(|mut v: Vec<Row>| v.pop());
        Ok(row.map(|r| DraftStorage {
            id: r.id.unwrap_or(_draft_id),
            board_id: r.board_id.unwrap_or_default(),
            topic_id: r.topic_id.unwrap_or_default(),
            subject: r.subject.unwrap_or_default(),
            body: r.body.unwrap_or_default(),
            icon: r.icon.unwrap_or_else(|| "xx".into()),
            smileys_enabled: r.smileys_enabled.unwrap_or(true),
            locked: r.locked.unwrap_or(false),
            sticky: r.sticky.unwrap_or(false),
            poster_time: r
                .poster_time_ms
                .and_then(|ms| Utc.timestamp_millis_opt(ms).single())
                .unwrap_or_else(Utc::now),
        }))
    }

    fn update_notify_pref(&self, _user_id: i64, _auto: bool) -> Result<NotifyPrefs, ForumError> {
        Ok(NotifyPrefs {
            msg_auto_notify: false,
        })
    }

    fn is_board_notification_set(&self, member_id: i64, board_id: i64) -> Result<bool, ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let mut response = rt
            .block_on(async {
                self.client
                    .query(
                        r#"
                        SELECT VALUE count()
                        FROM board_notifications
                        WHERE member_id = $member_id AND board_id = $board_id
                        GROUP ALL;
                        "#,
                    )
                    .bind(("member_id", member_id))
                    .bind(("board_id", board_id))
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        let count: Option<i64> = response.take(0).ok().and_then(|mut v: Vec<i64>| v.pop());
        Ok(count.unwrap_or(0) > 0)
    }

    fn add_board_notification(&self, member_id: i64, board_id: i64) -> Result<(), ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let id = format!("{member_id}:{board_id}");
        rt.block_on(async {
            self.client
                .query(
                    r#"
                    UPSERT type::thing("board_notifications", $id) SET
                        member_id = $member_id,
                        board_id = $board_id,
                        created_at = time::now();
                    "#,
                )
                .bind(("id", id))
                .bind(("member_id", member_id))
                .bind(("board_id", board_id))
                .await
        })
        .map_err(|e| ForumError::Internal(e.to_string()))?;
        Ok(())
    }

    fn remove_board_notification(&self, member_id: i64, board_id: i64) -> Result<(), ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let id = format!("{member_id}:{board_id}");
        rt.block_on(async {
            self.client
                .query("DELETE type::thing(\"board_notifications\", $id);")
                .bind(("id", id))
                .await
        })
        .map_err(|e| ForumError::Internal(e.to_string()))?;
        Ok(())
    }

    fn is_topic_notification_set(&self, member_id: i64, topic_id: i64) -> Result<bool, ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let mut response = rt
            .block_on(async {
                self.client
                    .query(
                        r#"
                        SELECT VALUE count()
                        FROM topic_notifications
                        WHERE member_id = $member_id AND topic_id = $topic_id
                        GROUP ALL;
                        "#,
                    )
                    .bind(("member_id", member_id))
                    .bind(("topic_id", topic_id))
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        let count: Option<i64> = response.take(0).ok().and_then(|mut v: Vec<i64>| v.pop());
        Ok(count.unwrap_or(0) > 0)
    }

    fn add_topic_notification(&self, member_id: i64, topic_id: i64) -> Result<(), ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let id = format!("{member_id}:{topic_id}");
        rt.block_on(async {
            self.client
                .query(
                    r#"
                    UPSERT type::thing("topic_notifications", $id) SET
                        member_id = $member_id,
                        topic_id = $topic_id,
                        created_at = time::now();
                    "#,
                )
                .bind(("id", id))
                .bind(("member_id", member_id))
                .bind(("topic_id", topic_id))
                .await
        })
        .map_err(|e| ForumError::Internal(e.to_string()))?;
        Ok(())
    }

    fn remove_topic_notification(&self, member_id: i64, topic_id: i64) -> Result<(), ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let id = format!("{member_id}:{topic_id}");
        rt.block_on(async {
            self.client
                .query(
                    r#"
                    DELETE type::thing("topic_notifications", $id);
                    DELETE type::thing("topic_logs", $id);
                    "#,
                )
                .bind(("id", id))
                .await
        })
        .map_err(|e| ForumError::Internal(e.to_string()))?;
        Ok(())
    }

    fn load_topic_log(
        &self,
        member_id: i64,
        topic_id: i64,
    ) -> Result<Option<TopicLogEntry>, ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        #[derive(Debug, Clone, SurrealValue)]
        struct TopicLogRow {
            member_id: Option<i64>,
            topic_id: Option<i64>,
            last_msg_id: Option<i64>,
            unwatched: Option<bool>,
        }
        let id = format!("{member_id}:{topic_id}");
        let mut response = rt
            .block_on(async {
                self.client
                    .query(
                        r#"
                        SELECT member_id, topic_id, last_msg_id, unwatched
                        FROM topic_logs
                        WHERE meta::id(id) = type::thing("topic_logs", $id)
                        LIMIT 1;
                        "#,
                    )
                    .bind(("id", id))
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        let row: Option<TopicLogRow> = response
            .take(0)
            .ok()
            .and_then(|mut v: Vec<TopicLogRow>| v.pop());
        Ok(row.map(|row| TopicLogEntry {
            member_id: row.member_id.unwrap_or(member_id),
            topic_id: row.topic_id.unwrap_or(topic_id),
            last_msg_id: row.last_msg_id.unwrap_or_default(),
            unwatched: row.unwatched.unwrap_or(false),
        }))
    }

    fn save_topic_log(&self, entry: TopicLogEntry) -> Result<(), ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let id = format!("{}:{}", entry.member_id, entry.topic_id);
        rt.block_on(async {
            self.client
                .query(
                    r#"
                    UPSERT type::thing("topic_logs", $id) SET
                        member_id = $member_id,
                        topic_id = $topic_id,
                        last_msg_id = $last_msg_id,
                        unwatched = $unwatched,
                        updated_at = time::now();
                    "#,
                )
                .bind(("id", id))
                .bind(("member_id", entry.member_id))
                .bind(("topic_id", entry.topic_id))
                .bind(("last_msg_id", entry.last_msg_id))
                .bind(("unwatched", entry.unwatched))
                .await
        })
        .map_err(|e| ForumError::Internal(e.to_string()))?;
        Ok(())
    }

    fn list_board_notifications(
        &self,
        member_id: i64,
    ) -> Result<Vec<BoardNotificationSummary>, ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        #[derive(Debug, Clone, SurrealValue)]
        struct Row {
            board_id: Option<i64>,
            board_name: Option<String>,
        }
        let mut response = rt
            .block_on(async {
                self.client
                    .query(
                        r#"
                        SELECT board_id,
                               (SELECT VALUE name FROM boards WHERE meta::id(id) = type::thing("boards", string::from(board_id)) LIMIT 1)[0] AS board_name
                        FROM board_notifications
                        WHERE member_id = $member_id
                        ORDER BY board_id ASC;
                        "#,
                    )
                    .bind(("member_id", member_id))
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        let rows: Vec<Row> = response.take(0).unwrap_or_default();
        Ok(rows
            .into_iter()
            .map(|row| BoardNotificationSummary {
                id: row.board_id.unwrap_or_default(),
                name: row.board_name.unwrap_or_else(|| "Unknown".into()),
            })
            .collect())
    }

    fn list_topic_notifications(
        &self,
        member_id: i64,
    ) -> Result<Vec<TopicNotificationSummary>, ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        #[derive(Debug, Clone, SurrealValue)]
        struct Row {
            topic_id: Option<i64>,
            subject: Option<String>,
            board_id: Option<i64>,
            board_name: Option<String>,
        }
        let mut response = rt
            .block_on(async {
                self.client
                    .query(
                        r#"
                        SELECT topic_id,
                               (SELECT VALUE subject FROM topics WHERE meta::id(id) = type::thing("topics", string::from(topic_id)) LIMIT 1)[0] AS subject,
                               <int>(SELECT VALUE math::sum([board_id]) FROM topics WHERE meta::id(id) = type::thing("topics", string::from(topic_id)) LIMIT 1)[0] AS board_id,
                               (SELECT VALUE name FROM boards WHERE meta::id(id) = type::thing("boards", string::from((SELECT VALUE math::sum([board_id]) FROM topics WHERE meta::id(id) = type::thing("topics", string::from(topic_id)) LIMIT 1)[0])) LIMIT 1)[0] AS board_name
                        FROM topic_notifications
                        WHERE member_id = $member_id
                        ORDER BY topic_id ASC;
                        "#,
                    )
                    .bind(("member_id", member_id))
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        let rows: Vec<Row> = response.take(0).unwrap_or_default();
        Ok(rows
            .into_iter()
            .map(|row| TopicNotificationSummary {
                id: row.topic_id.unwrap_or_default(),
                subject: row.subject.unwrap_or_else(|| "(no subject)".into()),
                board_id: row.board_id.unwrap_or_default(),
                board_name: row.board_name.unwrap_or_else(|| "Unknown".into()),
            })
            .collect())
    }

    fn remove_board_notifications(&self, member_id: i64, boards: &[i64]) -> Result<(), ForumError> {
        for board_id in boards {
            self.remove_board_notification(member_id, *board_id)?;
        }
        Ok(())
    }

    fn remove_topic_notifications(&self, member_id: i64, topics: &[i64]) -> Result<(), ForumError> {
        for topic_id in topics {
            self.remove_topic_notification(member_id, *topic_id)?;
        }
        Ok(())
    }

    fn can_link_event(&self, _user_id: i64) -> Result<bool, ForumError> {
        Ok(true)
    }

    fn insert_event(&self, _event: CalendarEvent) -> Result<i64, ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let id = _event.id.unwrap_or_else(|| Utc::now().timestamp_millis());
        rt.block_on(async {
            self.client
                .query(
                    r#"
                    UPSERT type::thing("calendar_events", $id) SET
                        board_id = $board_id,
                        topic_id = $topic_id,
                        title = $title,
                        location = $location,
                        member_id = $member_id,
                        created_at_ms = time::now();
                    "#,
                )
                .bind(("id", id))
                .bind(("board_id", _event.board_id))
                .bind(("topic_id", _event.topic_id))
                .bind(("title", _event.title.clone()))
                .bind(("location", _event.location.clone()))
                .bind(("member_id", _event.member_id))
                .await
        })
        .map_err(|e| ForumError::Internal(e.to_string()))?;
        Ok(id)
    }

    fn modify_event(&self, _event_id: i64, _event: CalendarEvent) -> Result<(), ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        rt.block_on(async {
            self.client
                .query(
                    r#"
                    UPDATE type::thing("calendar_events", $id) SET
                        board_id = $board_id,
                        topic_id = $topic_id,
                        title = $title,
                        location = $location,
                        member_id = $member_id;
                    "#,
                )
                .bind(("id", _event_id))
                .bind(("board_id", _event.board_id))
                .bind(("topic_id", _event.topic_id))
                .bind(("title", _event.title.clone()))
                .bind(("location", _event.location.clone()))
                .bind(("member_id", _event.member_id))
                .await
        })
        .map_err(|e| ForumError::Internal(e.to_string()))?;
        Ok(())
    }

    fn create_poll(&self, _poll: PollData) -> Result<i64, ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let poll_id = Utc::now().timestamp_millis();
        rt.block_on(async {
            self.client
                .query(
                    r#"
                    CREATE polls CONTENT {
                        id: $id,
                        topic_id: $topic_id,
                        question: $question,
                        max_votes: $max_votes,
                        change_vote: $change_vote,
                        guest_vote: $guest_vote,
                        created_at_ms: time::now()
                    };
                    "#,
                )
                .bind(("id", poll_id))
                .bind(("topic_id", _poll.topic_id))
                .bind(("question", _poll.question.clone()))
                .bind(("max_votes", _poll.max_votes))
                .bind(("change_vote", _poll.change_vote))
                .bind(("guest_vote", _poll.guest_vote))
                .await
        })
        .map_err(|e| ForumError::Internal(e.to_string()))?;

        for option in &_poll.options {
            rt.block_on(async {
                self.client
                    .query(
                        r#"
                        CREATE poll_options CONTENT {
                            poll_id: $poll_id,
                            option_id: $option_id,
                            label: $label,
                            votes: $votes
                        };
                        "#,
                    )
                    .bind(("poll_id", poll_id))
                    .bind(("option_id", option.id))
                    .bind(("label", option.label.clone()))
                    .bind(("votes", option.votes))
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        }

        Ok(poll_id)
    }

    fn remove_poll(&self, _poll_id: i64) -> Result<(), ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        rt.block_on(async {
            self.client
                .query("DELETE poll_options WHERE poll_id = $id; DELETE polls WHERE id = $id;")
                .bind(("id", _poll_id))
                .await
        })
        .map_err(|e| ForumError::Internal(e.to_string()))?;
        Ok(())
    }

    fn lock_poll(&self, _poll_id: i64, _lock: bool) -> Result<(), ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        rt.block_on(async {
            self.client
                .query("UPDATE polls SET locked = $locked WHERE id = $id;")
                .bind(("locked", _lock))
                .bind(("id", _poll_id))
                .await
        })
        .map_err(|e| ForumError::Internal(e.to_string()))?;
        Ok(())
    }

    fn cast_vote(
        &self,
        _poll_id: i64,
        _member_id: i64,
        _options: &[i64],
    ) -> Result<(), ForumError> {
        if _options.is_empty() {
            return Ok(());
        }
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        for opt in _options {
            rt.block_on(async {
                self.client
                    .query(
                        r#"
                        UPDATE poll_options SET votes += 1
                        WHERE poll_id = $poll_id AND option_id = $option_id;
                        "#,
                    )
                    .bind(("poll_id", _poll_id))
                    .bind(("option_id", *opt))
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        }
        Ok(())
    }

    fn fetch_topic_messages(
        &self,
        topic_id: i64,
        _start: i64,
        _limit: i64,
    ) -> Result<Vec<MessageData>, ForumError> {
        let tid = topic_id.to_string();
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let posts = rt
            .block_on(surreal_list_posts_for_topic(&self.client, &tid))
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        Ok(posts
            .into_iter()
            .map(|p| MessageData {
                id: p
                    .id
                    .as_deref()
                    .and_then(|id| id.split(':').last())
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0),
                topic_id,
                body: p.body,
                subject: p.subject,
                member_id: 0,
                approved: true,
            })
            .collect())
    }

    fn increment_topic_views(&self, _topic_id: i64) -> Result<(), ForumError> {
        Ok(())
    }

    fn list_membergroups(&self) -> Result<Vec<MembergroupSummary>, ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let mut response = rt
            .block_on(async {
                self.client
                    .query(
                        r#"
                        SELECT meta::id(id) as id, name, description
                        FROM membergroups;
                        "#,
                    )
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        #[derive(Debug, Clone, SurrealValue)]
        struct Row {
            id: Option<i64>,
            name: Option<String>,
            _description: Option<String>,
        }
        let rows: Vec<Row> = response.take(0).unwrap_or_default();
        Ok(rows
            .into_iter()
            .map(|r| MembergroupSummary {
                id: r.id.unwrap_or(0),
                name: r.name.unwrap_or_default(),
                num_members: 0,
                color: None,
                is_post_group: false,
            })
            .collect())
    }

    fn get_membergroup(&self, _group_id: i64) -> Result<Option<MembergroupData>, ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let mut response = rt
            .block_on(async {
                self.client
                    .query(
                        r#"
                        SELECT id, name, description, type
                        FROM membergroups WHERE id = $id LIMIT 1;
                        "#,
                    )
                    .bind(("id", _group_id))
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        #[derive(Debug, Clone, SurrealValue)]
        struct Row {
            id: Option<i64>,
            name: Option<String>,
            description: Option<String>,
            r#type: Option<i32>,
            color: Option<String>,
            hidden: Option<bool>,
            min_posts: Option<i64>,
        }
        let row: Option<Row> = response.take(0).ok().and_then(|mut v: Vec<Row>| v.pop());
        Ok(row.map(|r| MembergroupData {
            id: r.id,
            name: r.name.unwrap_or_default(),
            description: r.description.unwrap_or_default(),
            inherits_from: None,
            allowed_boards: Vec::new(),
            color: r.color,
            is_post_group: false,
            min_posts: r.min_posts.unwrap_or(0),
            group_type: r.r#type.unwrap_or(0),
            hidden: r.hidden.unwrap_or(false),
            icons: None,
            is_protected: false,
        }))
    }

    fn save_membergroup(&self, _group: MembergroupData) -> Result<i64, ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let id = _group.id.unwrap_or_else(|| Utc::now().timestamp_millis());
        rt.block_on(async {
            self.client
                .query(
                    r#"
                    UPDATE membergroups SET
                        name = $name,
                        description = $description,
                        type = $type
                    WHERE id = $id;
                    "#,
                )
                .bind(("name", _group.name.clone()))
                .bind(("description", _group.description.clone()))
                .bind(("type", _group.group_type.clone()))
                .bind(("id", id))
                .await
        })
        .map_err(|e| ForumError::Internal(e.to_string()))?;
        Ok(id)
    }

    fn list_group_members(&self, _group_id: i64) -> Result<Vec<GroupMember>, ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let mut response = rt
            .block_on(async {
                self.client
                    .query(
                        r#"
                        SELECT member_id, is_primary, name
                        FROM group_members
                        WHERE group_id = $group_id;
                        "#,
                    )
                    .bind(("group_id", _group_id))
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        #[derive(Debug, Clone, SurrealValue)]
        struct Row {
            member_id: Option<i64>,
            is_primary: Option<bool>,
            name: Option<String>,
        }
        let rows: Vec<Row> = response.take(0).unwrap_or_default();
        Ok(rows
            .into_iter()
            .filter_map(|r| {
                r.member_id.map(|mid| GroupMember {
                    id: mid,
                    name: r.name.unwrap_or_default(),
                    primary: r.is_primary.unwrap_or(false),
                })
            })
            .collect())
    }

    fn remove_members_from_group(
        &self,
        _group_id: i64,
        _members: &[i64],
    ) -> Result<(), ForumError> {
        if _members.is_empty() {
            return Ok(());
        }
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        rt.block_on(async {
            self.client
                .query(
                    r#"
                    DELETE group_members
                    WHERE group_id = $group_id AND member_id IN $members;
                    "#,
                )
                .bind(("group_id", _group_id))
                .bind(("members", _members.to_vec()))
                .await
        })
        .map_err(|e| ForumError::Internal(e.to_string()))?;
        Ok(())
    }

    fn get_membergroup_settings(&self) -> Result<MembergroupSettings, ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let mut response = rt
            .block_on(async {
                self.client
                    .query(
                        r#"
                        SELECT show_group_key
                        FROM membergroup_settings
                        LIMIT 1;
                        "#,
                    )
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        #[derive(Debug, Clone, SurrealValue)]
        struct Row {
            show_group_key: Option<bool>,
        }
        let row: Option<Row> = response.take(0).ok().and_then(|mut v: Vec<Row>| v.pop());
        Ok(MembergroupSettings {
            show_group_key: row.and_then(|r| r.show_group_key).unwrap_or(false),
        })
    }

    fn save_membergroup_settings(&self, _settings: MembergroupSettings) -> Result<(), ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        rt.block_on(async {
            self.client
                .query(
                    r#"
                    UPDATE membergroup_settings SET
                        show_group_key = $show_group_key;
                    "#,
                )
                .bind(("show_group_key", _settings.show_group_key))
                .await
        })
        .map_err(|e| ForumError::Internal(e.to_string()))?;
        Ok(())
    }

    fn delete_membergroups(&self, _group_ids: &[i64]) -> Result<(), ForumError> {
        if _group_ids.is_empty() {
            return Ok(());
        }
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        rt.block_on(async {
            self.client
                .query(
                    r#"
                    DELETE membergroups WHERE id IN $ids;
                    DELETE group_members WHERE group_id IN $ids;
                    "#,
                )
                .bind(("ids", _group_ids.to_vec()))
                .await
        })
        .map_err(|e| ForumError::Internal(e.to_string()))?;
        Ok(())
    }

    fn remove_members_from_groups(
        &self,
        _member_ids: &[i64],
        _group_ids: Option<&[i64]>,
    ) -> Result<(), ForumError> {
        if _member_ids.is_empty() {
            return Ok(());
        }
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let ids = _member_ids.to_vec();
        let mut query = String::from("DELETE group_members WHERE member_id IN $members");
        if let Some(groups) = _group_ids {
            query.push_str(" AND group_id IN $groups");
            rt.block_on(async {
                self.client
                    .query(query)
                    .bind(("members", ids))
                    .bind(("groups", groups.to_vec()))
                    .await
            })
        } else {
            rt.block_on(async { self.client.query(query).bind(("members", ids)).await })
        }
        .map_err(|e| ForumError::Internal(e.to_string()))?;
        Ok(())
    }

    fn add_members_to_group(
        &self,
        _member_ids: &[i64],
        _group_id: i64,
        _assign_type: GroupAssignType,
    ) -> Result<(), ForumError> {
        if _member_ids.is_empty() {
            return Ok(());
        }
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        for member in _member_ids {
            rt.block_on(async {
                self.client
                    .query(
                        r#"
                        CREATE group_members CONTENT {
                            group_id: $group_id,
                            member_id: $member_id,
                            is_primary: false
                        };
                        "#,
                    )
                    .bind(("group_id", _group_id))
                    .bind(("member_id", *member))
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        }
        Ok(())
    }

    fn list_membergroups_detailed(
        &self,
        _group_type: MembergroupListType,
    ) -> Result<Vec<MembergroupListEntry>, ForumError> {
        let summaries = self.list_membergroups()?;
        Ok(summaries
            .into_iter()
            .filter(|s| match _group_type {
                MembergroupListType::Regular => !s.is_post_group,
                MembergroupListType::PostCount => s.is_post_group,
            })
            .map(|s| MembergroupListEntry {
                id: s.id,
                name: s.name,
                description: String::new(),
                min_posts: 0,
                color: s.color.clone(),
                group_type: if s.is_post_group {
                    MembergroupListType::PostCount as i32
                } else {
                    MembergroupListType::Regular as i32
                },
                num_members: s.num_members,
                moderators: Vec::new(),
                icons: None,
                can_moderate: false,
                hidden: s.is_post_group,
            })
            .collect())
    }

    fn groups_with_permissions(
        &self,
        _group_permissions: &[String],
        _board_permissions: &[String],
        _profile_id: i64,
    ) -> Result<std::collections::HashMap<String, PermissionSnapshot>, ForumError> {
        let mut map = std::collections::HashMap::new();
        for permission in _group_permissions {
            map.insert(
                permission.clone(),
                PermissionSnapshot {
                    allowed: vec![1],
                    denied: Vec::new(),
                },
            );
        }
        for permission in _board_permissions {
            map.insert(
                permission.clone(),
                PermissionSnapshot {
                    allowed: vec![1],
                    denied: Vec::new(),
                },
            );
        }
        Ok(map)
    }

    fn permission_groups(&self) -> Result<Vec<PermissionGroupContext>, ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let mut response = rt
            .block_on(async {
                self.client
                    .query(
                        r#"
                        SELECT id, name, color
                        FROM permission_groups;
                        "#,
                    )
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        #[derive(Debug, Clone, SurrealValue)]
        struct Row {
            id: Option<i64>,
            name: Option<String>,
            color: Option<String>,
        }
        let rows: Vec<Row> = response.take(0).unwrap_or_default();
        Ok(rows
            .into_iter()
            .map(|r| PermissionGroupContext {
                id: r.id.unwrap_or(0),
                name: r.name.unwrap_or_default(),
                access: false,
                num_members: 0,
                allow_delete: false,
                allow_modify: false,
                can_search: false,
                help: None,
                is_post_group: false,
                color: r.color,
                icons: None,
                children: Vec::new(),
                allowed: 0,
                denied: 0,
                link: None,
            })
            .collect())
    }

    fn permission_profiles(&self) -> Result<Vec<PermissionProfile>, ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let mut response = rt
            .block_on(async {
                self.client
                    .query(
                        r#"
                        SELECT id, name
                        FROM permission_profiles;
                        "#,
                    )
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        #[derive(Debug, Clone, SurrealValue)]
        struct Row {
            id: Option<i64>,
            name: Option<String>,
        }
        let rows: Vec<Row> = response.take(0).unwrap_or_default();
        Ok(rows
            .into_iter()
            .map(|r| PermissionProfile {
                id: r.id.unwrap_or(0),
                name: r.name.unwrap_or_default(),
            })
            .collect())
    }

    fn ungrouped_member_count(&self) -> Result<i64, ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let mut response = rt
            .block_on(async {
                self.client
                    .query(
                        r#"
                        SELECT count() as total FROM users;
                        "#,
                    )
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        #[derive(Debug, Clone, SurrealValue)]
        struct Row {
            total: Option<i64>,
        }
        let row: Option<Row> = response.take(0).ok().and_then(|mut v: Vec<Row>| v.pop());
        Ok(row.and_then(|r| r.total).unwrap_or(0))
    }

    fn get_member_record(&self, _member_id: i64) -> Result<Option<MemberRecord>, ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let id_str = format!("users:{}", _member_id);
        let mut response = rt
            .block_on(async {
                self.client
                    .query(
                        r#"
                        SELECT meta::id(id) as id, name, role, permissions, password_hash
                        FROM $id
                        LIMIT 1;
                        "#,
                    )
                    .bind(("id", id_str))
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        #[derive(Debug, Clone, SurrealValue)]
        struct Row {
            id: Option<String>,
            name: String,
            _role: Option<String>,
            _permissions: Option<Vec<String>>,
        }
        let row: Option<Row> = response.take(0).ok().and_then(|mut v: Vec<Row>| v.pop());
        Ok(row.map(|row| MemberRecord {
            id: row
                .id
                .as_deref()
                .and_then(|id| id.split(':').last())
                .and_then(|s| s.parse().ok())
                .unwrap_or(_member_id),
            name: row.name,
            primary_group: None,
            additional_groups: Vec::new(),
            password: String::new(),
            warning: 0,
        }))
    }

    fn update_member_groups(
        &self,
        _member_id: i64,
        _primary_group: Option<i64>,
        _additional_groups: &[i64],
    ) -> Result<(), ForumError> {
        Ok(())
    }

    fn list_all_membergroups(&self) -> Result<Vec<MembergroupData>, ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let mut response = rt
            .block_on(async {
                self.client
                    .query(
                        r#"
                        SELECT id, name, description, type, min_posts, color, hidden
                        FROM membergroups;
                        "#,
                    )
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        #[derive(Debug, Clone, SurrealValue)]
        struct Row {
            id: Option<i64>,
            name: Option<String>,
            description: Option<String>,
            r#type: Option<i32>,
            min_posts: Option<i64>,
            color: Option<String>,
            hidden: Option<bool>,
        }
        let rows: Vec<Row> = response.take(0).unwrap_or_default();
        Ok(rows
            .into_iter()
            .map(|r| MembergroupData {
                id: r.id,
                name: r.name.unwrap_or_default(),
                description: r.description.unwrap_or_default(),
                inherits_from: None,
                allowed_boards: Vec::new(),
                color: r.color,
                is_post_group: r.r#type.unwrap_or(0) == MembergroupListType::PostCount as i32,
                min_posts: r.min_posts.unwrap_or(0),
                group_type: r.r#type.unwrap_or(0),
                hidden: r.hidden.unwrap_or(false),
                icons: None,
                is_protected: false,
            })
            .collect())
    }

    fn list_members(&self) -> Result<Vec<MemberRecord>, ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let mut response = rt
            .block_on(async {
                self.client
                    .query(
                        r#"
                        SELECT meta::id(id) as id, name, role, permissions, primary_group, additional_groups
                        FROM users;
                        "#,
                    )
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        #[derive(Debug, Clone, SurrealValue)]
        struct Row {
            id: Option<String>,
            name: String,
            _role: Option<String>,
            _permissions: Option<Vec<String>>,
            primary_group: Option<i64>,
            additional_groups: Option<Vec<i64>>,
        }
        let rows: Vec<Row> = response.take(0).unwrap_or_default();
        Ok(rows
            .into_iter()
            .map(|row| {
                let mut id = row
                    .id
                    .as_deref()
                    .and_then(|id| id.split(':').last())
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                if id == 0 {
                    let mut hasher = DefaultHasher::new();
                    row.name.hash(&mut hasher);
                    id = (hasher.finish() & 0x7FFF_FFFF_FFFF_FFFF) as i64;
                    if id == 0 {
                        id = 1;
                    }
                }
                MemberRecord {
                    id,
                    name: row.name,
                    primary_group: row.primary_group,
                    additional_groups: row.additional_groups.unwrap_or_default(),
                    password: String::new(),
                    warning: 0,
                }
            })
            .collect())
    }

    fn delete_member(&self, _member_id: i64) -> Result<(), ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let id_str = format!("users:{}", _member_id);
        rt.block_on(async { self.client.query("DELETE $id").bind(("id", id_str)).await })
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        Ok(())
    }

    fn list_board_access(&self) -> Result<Vec<BoardAccessEntry>, ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let query = r#"
            SELECT board_id, allowed_groups,
                (SELECT name FROM boards WHERE id = board_id)[0].name AS name
            FROM board_access;
            "#;
        let mut response = match rt.block_on(async { self.client.query(query).await }) {
            Ok(response) => response,
            Err(err) if Self::is_surreal_unauthorized(&err) => {
                if let Err(reauth_err) = rt.block_on(async { reauth_from_env(&self.client).await })
                {
                    tracing::warn!(error = %reauth_err, "list_board_access reauth failed, trying reconnect");
                }
                match rt.block_on(async { self.client.query(query).await }) {
                    Ok(response) => response,
                    Err(retry_err) if Self::is_surreal_unauthorized(&retry_err) => {
                        tracing::warn!(error = %retry_err, "list_board_access still unauthorized after reauth, rebuilding surreal client");
                        let fresh = rt
                            .block_on(async { connect_from_env().await })
                            .map_err(|e| ForumError::Internal(e.to_string()))?;
                        rt.block_on(async { fresh.query(query).await })
                            .map_err(|e| ForumError::Internal(e.to_string()))?
                    }
                    Err(retry_err) => return Err(ForumError::Internal(retry_err.to_string())),
                }
            }
            Err(err) => return Err(ForumError::Internal(err.to_string())),
        };
        #[derive(Debug, Clone, SurrealValue)]
        struct Row {
            board_id: Option<String>,
            allowed_groups: Option<Vec<i64>>,
            name: Option<String>,
        }
        let rows: Vec<Row> = response.take(0).unwrap_or_default();
        Ok(rows
            .into_iter()
            .filter_map(|r| {
                r.board_id.map(|bid| BoardAccessEntry {
                    id: bid,
                    name: r.name.unwrap_or_default(),
                    allowed_groups: r.allowed_groups.unwrap_or_default(),
                })
            })
            .collect())
    }

    fn set_board_access(&self, _board_id: &str, _groups: &[i64]) -> Result<(), ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let groups = _groups.to_vec();
        let board_id = _board_id.to_string();
        rt.block_on(async {
            self.client
                .query(
                    r#"
                    UPSERT type::record("board_access", $board_id) SET
                        board_id = $board_id,
                        allowed_groups = $groups;
                    "#,
                )
                .bind(("board_id", board_id))
                .bind(("groups", groups))
                .await
        })
        .map_err(|e| ForumError::Internal(e.to_string()))?;
        Ok(())
    }

    fn fetch_alert_prefs(
        &self,
        _members: &[i64],
        _prefs: Option<&[String]>,
    ) -> Result<std::collections::HashMap<i64, std::collections::HashMap<String, i32>>, ForumError>
    {
        if _members.is_empty() {
            return Ok(std::collections::HashMap::new());
        }
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let mut response = rt
            .block_on(async {
                self.client
                    .query(
                        r#"
                        SELECT member_id, prefs FROM alert_prefs
                        WHERE member_id IN $member_ids;
                        "#,
                    )
                    .bind(("member_ids", _members.to_vec()))
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        #[derive(Debug, Clone, SurrealValue)]
        struct Row {
            member_id: Option<i64>,
            prefs: Option<std::collections::HashMap<String, i32>>,
        }
        let rows: Vec<Row> = response.take(0).unwrap_or_default();
        let mut map = std::collections::HashMap::new();
        for row in rows {
            if let Some(member_id) = row.member_id {
                if let Some(prefs_map) = row.prefs {
                    let filtered = if let Some(only) = _prefs {
                        prefs_map
                            .into_iter()
                            .filter(|(k, _)| only.contains(k))
                            .collect()
                    } else {
                        prefs_map
                    };
                    map.insert(member_id, filtered);
                }
            }
        }
        Ok(map)
    }

    fn set_alert_prefs(&self, _member_id: i64, _prefs: &[(String, i32)]) -> Result<(), ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let prefs_map: std::collections::HashMap<String, i32> = _prefs.iter().cloned().collect();
        rt.block_on(async {
            self.client
                .query(
                    r#"
                    UPDATE alert_prefs SET prefs = $prefs WHERE member_id = $member_id;
                    IF none THEN (
                        CREATE alert_prefs CONTENT { member_id: $member_id, prefs: $prefs };
                    );
                    "#,
                )
                .bind(("member_id", _member_id))
                .bind(("prefs", prefs_map))
                .await
        })
        .map_err(|e| ForumError::Internal(e.to_string()))?;
        Ok(())
    }

    fn delete_alert_prefs(&self, _member_id: i64, _prefs: &[String]) -> Result<(), ForumError> {
        if _prefs.is_empty() {
            return Ok(());
        }
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let prefs = _prefs.to_vec();
        rt.block_on(async {
            self.client
                .query(
                    r#"
                    LET existing = (SELECT prefs FROM alert_prefs WHERE member_id = $member_id LIMIT 1);
                    IF count(existing) > 0 THEN (
                        UPDATE alert_prefs SET prefs = object::remove(prefs, $remove_keys)
                        WHERE member_id = $member_id;
                    );
                    "#,
                )
                .bind(("member_id", _member_id))
                .bind(("remove_keys", prefs))
                .await
        })
        .map_err(|e| ForumError::Internal(e.to_string()))?;
        Ok(())
    }

    fn get_member_email(&self, _member_id: i64) -> Result<Option<String>, ForumError> {
        Ok(None)
    }

    fn notification_secret(&self) -> Result<String, ForumError> {
        Ok(String::new())
    }

    fn general_permissions(&self, _group_ids: &[i64]) -> Result<Vec<PermissionChange>, ForumError> {
        if _group_ids.is_empty() {
            return Ok(Vec::new());
        }
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let mut response = rt
            .block_on(async {
                self.client
                    .query(
                        r#"
                        SELECT permissions
                        FROM membergroups
                        WHERE id IN $group_ids;
                        "#,
                    )
                    .bind(("group_ids", _group_ids.to_vec()))
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        #[derive(Debug, Clone, SurrealValue)]
        struct Row {
            permissions: Option<Vec<String>>,
        }
        let rows: Vec<Row> = response.take(0).unwrap_or_default();
        let mut changes = Vec::new();
        for row in rows {
            if let Some(perms) = row.permissions {
                for perm in perms {
                    changes.push(PermissionChange {
                        permission: perm,
                        allow: true,
                    });
                }
            }
        }
        Ok(changes)
    }

    fn board_permissions(
        &self,
        board_id: &str,
        _group_ids: &[i64],
    ) -> Result<Vec<PermissionChange>, ForumError> {
        if _group_ids.is_empty() {
            return Ok(Vec::new());
        }
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let board_id_owned = board_id.to_string();
        let mut response = rt
            .block_on(async {
                self.client
                    .query(
                        r#"
                        SELECT allow, deny
                        FROM board_permissions
                        WHERE board_id = $board_id AND group_id IN $group_ids;
                        "#,
                    )
                    .bind(("board_id", board_id_owned))
                    .bind(("group_ids", _group_ids.to_vec()))
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        #[derive(Debug, Clone, SurrealValue)]
        struct Row {
            allow: Option<Vec<String>>,
            deny: Option<Vec<String>>,
        }
        let rows: Vec<Row> = response.take(0).unwrap_or_default();
        let mut changes = Vec::new();
        for row in rows {
            if let Some(perms) = row.allow {
                changes.extend(perms.into_iter().map(|p| PermissionChange {
                    permission: p,
                    allow: true,
                }));
            }
            if let Some(perms) = row.deny {
                changes.extend(perms.into_iter().map(|p| PermissionChange {
                    permission: p,
                    allow: false,
                }));
            }
        }
        Ok(changes)
    }

    fn spider_group_id(&self) -> Option<i64> {
        None
    }

    fn settings_last_updated(&self) -> i64 {
        Utc::now().timestamp()
    }

    fn list_ban_rules(&self) -> Result<Vec<BanRule>, ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let mut response = rt
            .block_on(async {
                self.client
                    .query(
                        r#"
                        SELECT meta::id(id) as id, reason, expires_at_ms, cannot_post, cannot_access, conditions
                        FROM ban_rules;
                        "#,
                    )
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        #[derive(Debug, Clone, SurrealValue)]
        struct Row {
            id: Option<i64>,
            reason: Option<String>,
            expires_at_ms: Option<i64>,
            cannot_post: Option<bool>,
            cannot_access: Option<bool>,
            conditions: Option<serde_json::Value>,
        }
        let rows: Vec<Row> = response.take(0).unwrap_or_default();
        Ok(rows
            .into_iter()
            .map(|r| BanRule {
                id: r.id.unwrap_or(0),
                reason: r.reason,
                expires_at: r
                    .expires_at_ms
                    .and_then(|ms| Utc.timestamp_millis_opt(ms).single()),
                cannot_post: r.cannot_post.unwrap_or(false),
                cannot_access: r.cannot_access.unwrap_or(false),
                conditions: r
                    .conditions
                    .and_then(|value| serde_json::from_value(value).ok())
                    .unwrap_or_default(),
            })
            .collect())
    }

    fn save_ban_rule(&self, _rule: BanRule) -> Result<i64, ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let rule_id = if _rule.id == 0 {
            Utc::now().timestamp_millis()
        } else {
            _rule.id
        };
        let conditions = serde_json::to_value(&_rule.conditions).unwrap_or(serde_json::Value::Null);
        rt.block_on(async {
            self.client
                .query(
                    r#"
                    UPSERT type::record("ban_rules", $id) SET
                        id = $id,
                        reason = $reason,
                        expires_at_ms = $expires,
                        cannot_post = $cannot_post,
                        cannot_access = $cannot_access,
                        conditions = $conditions;
                    "#,
                )
                .bind(("reason", _rule.reason.clone()))
                .bind(("expires", _rule.expires_at.map(|dt| dt.timestamp_millis())))
                .bind(("cannot_post", _rule.cannot_post))
                .bind(("cannot_access", _rule.cannot_access))
                .bind(("conditions", conditions))
                .bind(("id", rule_id))
                .await
        })
        .map_err(|e| ForumError::Internal(e.to_string()))?;
        Ok(rule_id)
    }

    fn delete_ban_rule(&self, _rule_id: i64) -> Result<(), ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        rt.block_on(async {
            self.client
                .query("DELETE ban_rules WHERE meta::id(id) = $id;")
                .bind(("id", _rule_id))
                .await
        })
        .map_err(|e| ForumError::Internal(e.to_string()))?;
        Ok(())
    }

    fn ban_logs(&self) -> Result<Vec<BanLogEntry>, ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let mut response = rt
            .block_on(async {
                self.client
                    .query(
                        r#"
                        SELECT id, ban_id, email, hit_at_ms
                        FROM ban_logs;
                        "#,
                    )
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        #[derive(Debug, Clone, SurrealValue)]
        struct Row {
            id: Option<i64>,
            ban_id: Option<i64>,
            email: Option<String>,
            hit_at_ms: Option<i64>,
        }
        let rows: Vec<Row> = response.take(0).unwrap_or_default();
        Ok(rows
            .into_iter()
            .map(|r| BanLogEntry {
                id: r.id.unwrap_or(0),
                rule_id: r.ban_id.unwrap_or(0),
                email: r.email,
                timestamp: r
                    .hit_at_ms
                    .and_then(|ms| Utc.timestamp_millis_opt(ms).single())
                    .unwrap_or_else(Utc::now),
                member_id: None,
            })
            .collect())
    }

    fn record_ban_hit(&self, _bans: &[i64], _email: Option<&str>) -> Result<(), ForumError> {
        if _bans.is_empty() {
            return Ok(());
        }
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        for ban_id in _bans {
            rt.block_on(async {
                self.client
                    .query(
                        r#"
                        CREATE ban_logs CONTENT {
                            ban_id: $ban_id,
                            email: $email,
                            hit_at_ms: time::now()
                        };
                        "#,
                    )
                    .bind(("ban_id", *ban_id))
                    .bind(("email", _email.unwrap_or_default().to_string()))
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        }
        Ok(())
    }

    fn find_member_by_name(&self, _name: &str) -> Result<Option<MemberRecord>, ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let name = _name.to_owned();
        let user = rt
            .block_on(get_user_by_name(&self.client, &name))
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        Ok(user.map(|u| MemberRecord {
            id: u.legacy_id(),
            name: u.name,
            primary_group: None,
            additional_groups: Vec::new(),
            password: u.password_hash.unwrap_or_default(),
            warning: 0,
        }))
    }

    fn find_members_by_name(&self, _names: &[String]) -> Result<Vec<MemberRecord>, ForumError> {
        let mut members = Vec::new();
        for name in _names {
            if let Ok(Some(rec)) = self.find_member_by_name(name) {
                members.push(rec);
            }
        }
        Ok(members)
    }

    fn cleanup_pm_recipients(&self, _member_ids: &[i64]) -> Result<(), ForumError> {
        if _member_ids.is_empty() {
            return Ok(());
        }
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let ids = _member_ids.to_vec();
        rt.block_on(async {
            self.client
                .query("DELETE personal_messages WHERE owner_id IN $ids;")
                .bind(("ids", ids))
                .await
        })
        .map_err(|e| ForumError::Internal(e.to_string()))?;
        Ok(())
    }

    fn cleanup_pm_ignore_lists(&self, _member_ids: &[i64]) -> Result<(), ForumError> {
        if _member_ids.is_empty() {
            return Ok(());
        }
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        rt.block_on(async {
            self.client
                .query(
                    r#"
                    DELETE pm_ignore_lists WHERE owner_id IN $ids OR ignored_id IN $ids;
                    "#,
                )
                .bind(("ids", _member_ids.to_vec()))
                .await
        })
        .map_err(|e| ForumError::Internal(e.to_string()))?;
        Ok(())
    }

    fn get_pm_ignore_list(&self, _member_id: i64) -> Result<Vec<i64>, ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let mut response = rt
            .block_on(async {
                self.client
                    .query(
                        r#"
                        SELECT ignored_id
                        FROM pm_ignore_lists
                        WHERE owner_id = $owner_id;
                        "#,
                    )
                    .bind(("owner_id", _member_id))
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        #[derive(Debug, Clone, SurrealValue)]
        struct Row {
            ignored_id: Option<i64>,
        }
        let rows: Vec<Row> = response.take(0).unwrap_or_default();
        Ok(rows.into_iter().filter_map(|r| r.ignored_id).collect())
    }

    fn set_pm_ignore_list(&self, _member_id: i64, _members: &[i64]) -> Result<(), ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let members = _members.to_vec();
        rt.block_on(async {
            self.client
                .query(
                    r#"
                    DELETE pm_ignore_lists WHERE owner_id = $owner_id;
                    "#,
                )
                .bind(("owner_id", _member_id))
                .await
        })
        .map_err(|e| ForumError::Internal(e.to_string()))?;

        if members.is_empty() {
            return Ok(());
        }

        for ignored in members {
            rt.block_on(async {
                self.client
                    .query(
                        r#"
                        CREATE pm_ignore_lists CONTENT {
                            owner_id: $owner_id,
                            ignored_id: $ignored_id
                        };
                        "#,
                    )
                    .bind(("owner_id", _member_id))
                    .bind(("ignored_id", ignored))
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        }
        Ok(())
    }

    fn get_buddy_list(&self, _member_id: i64) -> Result<Vec<i64>, ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let mut response = rt
            .block_on(async {
                self.client
                    .query(
                        r#"
                        SELECT buddy_id
                        FROM buddy_lists
                        WHERE owner_id = $owner_id;
                        "#,
                    )
                    .bind(("owner_id", _member_id))
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        #[derive(Debug, Clone, SurrealValue)]
        struct Row {
            buddy_id: Option<i64>,
        }
        let rows: Vec<Row> = response.take(0).unwrap_or_default();
        Ok(rows.into_iter().filter_map(|r| r.buddy_id).collect())
    }

    fn get_pm_preferences(&self, _member_id: i64) -> Result<PmPreferenceState, ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let mut response = rt
            .block_on(async {
                self.client
                    .query(
                        r#"
                        SELECT receive_from, notify_level
                        FROM pm_preferences
                        WHERE owner_id = $owner_id
                        LIMIT 1;
                        "#,
                    )
                    .bind(("owner_id", _member_id))
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        #[derive(Debug, Clone, SurrealValue)]
        struct Row {
            receive_from: Option<i32>,
            notify_level: Option<i32>,
        }
        let row: Option<Row> = response.take(0).ok().and_then(|mut v: Vec<Row>| v.pop());
        Ok(PmPreferenceState {
            receive_from: row.as_ref().and_then(|r| r.receive_from).unwrap_or(0),
            notify_level: row.and_then(|r| r.notify_level).unwrap_or(0),
        })
    }

    fn save_pm_preferences(
        &self,
        _member_id: i64,
        _prefs: &PmPreferenceState,
    ) -> Result<(), ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        rt.block_on(async {
            self.client
                .query(
                    r#"
                    UPSERT type::thing("pm_preferences", $owner_id) SET
                        owner_id = $owner_id,
                        receive_from = $receive_from,
                        notify_level = $notify_level;
                    "#,
                )
                .bind(("owner_id", _member_id))
                .bind(("receive_from", _prefs.receive_from))
                .bind(("notify_level", _prefs.notify_level))
                .await
        })
        .map_err(|e| ForumError::Internal(e.to_string()))?;
        Ok(())
    }

    fn record_pm_sent(
        &self,
        _sender_id: i64,
        _timestamp: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), ForumError> {
        // Stored implicitly via send_personal_message (Sent folder), nothing extra required.
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        rt.block_on(async {
            self.client
                .query(
                    r#"
                    CREATE pm_send_log CONTENT {
                        sender_id: $sender_id,
                        created_at_ms: $created_at
                    };
                    "#,
                )
                .bind(("sender_id", _sender_id))
                .bind(("created_at", _timestamp.timestamp_millis()))
                .await
        })
        .map_err(|e| ForumError::Internal(e.to_string()))?;
        Ok(())
    }

    fn count_pm_sent_since(
        &self,
        _sender_id: i64,
        _since: chrono::DateTime<chrono::Utc>,
    ) -> Result<usize, ForumError> {
        let since_ms = _since.timestamp_millis();
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let mut response = rt
            .block_on(async {
                self.client
                    .query(
                        r#"
                        SELECT count() as total
                        FROM personal_messages
                        WHERE owner_id = $owner_id
                          AND folder = "Sent"
                          AND created_at_ms >= $since_ms;
                        "#,
                    )
                    .bind(("owner_id", _sender_id))
                    .bind(("since_ms", since_ms))
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        #[derive(Debug, Clone, SurrealValue)]
        struct CountRow {
            total: Option<usize>,
        }
        let row: Option<CountRow> = response
            .take(0)
            .ok()
            .and_then(|mut v: Vec<CountRow>| v.pop());
        Ok(row.and_then(|c| c.total).unwrap_or(0))
    }

    fn repair_pm_data(&self) -> Result<usize, ForumError> {
        // Surreal-backed PM data is append-only enough for current develop baseline;
        // keep this as a safe no-op until a dedicated repair flow is implemented.
        Ok(0)
    }

    fn log_action(
        &self,
        _action: &str,
        _member_id: Option<i64>,
        _details: &Value,
    ) -> Result<(), ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let action = _action.to_string();
        let details = _details.clone();
        rt.block_on(async {
            self.client
                .query(
                    r#"
                    CREATE action_logs CONTENT {
                        action: $action,
                        member_id: $member_id,
                        details: $details,
                        created_at_ms: time::now()
                    };
                    "#,
                )
                .bind(("action", action))
                .bind(("member_id", _member_id))
                .bind(("details", details))
                .await
        })
        .map_err(|e| ForumError::Internal(e.to_string()))?;
        Ok(())
    }

    fn list_action_logs(&self) -> Result<Vec<ActionLogEntry>, ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let mut response = rt
            .block_on(async {
                self.client
                    .query(
                        r#"
                        SELECT action, member_id, details, created_at_ms
                        FROM action_logs
                        ORDER BY created_at_ms DESC
                        LIMIT 200;
                        "#,
                    )
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        #[derive(Debug, Clone, SurrealValue)]
        struct Row {
            id: Option<String>,
            action: Option<String>,
            member_id: Option<i64>,
            details: Option<Value>,
            created_at_ms: Option<i64>,
        }
        let rows: Vec<Row> = response.take(0).unwrap_or_default();
        Ok(rows
            .into_iter()
            .map(|r| ActionLogEntry {
                id: r
                    .id
                    .as_deref()
                    .and_then(|id| id.split(':').last())
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0),
                action: r.action.unwrap_or_default(),
                member_id: r.member_id,
                details: r.details.unwrap_or(Value::Null),
                timestamp: r
                    .created_at_ms
                    .and_then(|ms| Utc.timestamp_millis_opt(ms).single())
                    .unwrap_or_else(Utc::now),
            })
            .collect())
    }

    fn clean_expired_bans(&self) -> Result<usize, ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let mut response = rt
            .block_on(async {
                self.client
                    .query(
                        r#"
                        DELETE ban_rules
                        WHERE expires_at_ms IS NOT NONE AND expires_at_ms < time::now()
                        RETURN count();
                        "#,
                    )
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        #[derive(Debug, Clone, SurrealValue)]
        struct Row {
            count: Option<usize>,
        }
        let row: Option<Row> = response.take(0).ok().and_then(|mut v: Vec<Row>| v.pop());
        Ok(row.and_then(|r| r.count).unwrap_or(0))
    }

    fn save_pm_draft(&self, _draft: PmDraftRecord) -> Result<i64, ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let id = if _draft.id == 0 {
            Utc::now().timestamp_millis()
        } else {
            _draft.id
        };
        let saved_at_ms = _draft.saved_at.timestamp_millis();
        rt.block_on(async {
            self.client
                .query(
                    r#"
                    UPSERT type::thing("pm_drafts", $id) SET
                        id = $id,
                        owner_id = $owner_id,
                        subject = $subject,
                        body = $body,
                        to_members = $to_members,
                        bcc_members = $bcc_members,
                        saved_at_ms = $saved_at_ms;
                    "#,
                )
                .bind(("id", id))
                .bind(("owner_id", _draft.owner_id))
                .bind(("subject", _draft.subject.clone()))
                .bind(("body", _draft.body.clone()))
                .bind(("to_members", _draft.to.clone()))
                .bind(("bcc_members", _draft.bcc.clone()))
                .bind(("saved_at_ms", saved_at_ms))
                .await
        })
        .map_err(|e| ForumError::Internal(e.to_string()))?;
        Ok(id)
    }

    fn delete_pm_draft(&self, _owner_id: i64, _draft_id: i64) -> Result<(), ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        rt.block_on(async {
            self.client
                .query(
                    r#"
                    DELETE pm_drafts
                    WHERE id = $id AND owner_id = $owner_id;
                    "#,
                )
                .bind(("id", _draft_id))
                .bind(("owner_id", _owner_id))
                .await
        })
        .map_err(|e| ForumError::Internal(e.to_string()))?;
        Ok(())
    }

    fn list_pm_drafts(
        &self,
        _owner_id: i64,
        _start: usize,
        _limit: usize,
    ) -> Result<Vec<PmDraftRecord>, ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let mut response = rt
            .block_on(async {
                self.client
                    .query(
                        r#"
                        SELECT id, owner_id, subject, body, to_members, bcc_members, saved_at_ms
                        FROM pm_drafts
                        WHERE owner_id = $owner_id
                        ORDER BY saved_at_ms DESC
                        LIMIT $limit START $start;
                        "#,
                    )
                    .bind(("owner_id", _owner_id))
                    .bind(("limit", _limit as i64))
                    .bind(("start", _start as i64))
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        #[derive(Debug, Clone, SurrealValue)]
        struct Row {
            id: Option<i64>,
            owner_id: Option<i64>,
            subject: Option<String>,
            body: Option<String>,
            to_members: Option<Vec<i64>>,
            bcc_members: Option<Vec<i64>>,
            saved_at_ms: Option<i64>,
        }
        let rows: Vec<Row> = response.take(0).unwrap_or_default();
        Ok(rows
            .into_iter()
            .map(|row| PmDraftRecord {
                id: row.id.unwrap_or(0),
                owner_id: row.owner_id.unwrap_or(_owner_id),
                subject: row.subject.unwrap_or_default(),
                body: row.body.unwrap_or_default(),
                to: row.to_members.unwrap_or_default(),
                bcc: row.bcc_members.unwrap_or_default(),
                saved_at: row
                    .saved_at_ms
                    .and_then(|ms| Utc.timestamp_millis_opt(ms).single())
                    .unwrap_or_else(Utc::now),
            })
            .collect())
    }

    fn read_pm_draft(
        &self,
        _owner_id: i64,
        _draft_id: i64,
    ) -> Result<Option<PmDraftRecord>, ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let mut response = rt
            .block_on(async {
                self.client
                    .query(
                        r#"
                        SELECT id, owner_id, subject, body, to_members, bcc_members, saved_at_ms
                        FROM pm_drafts
                        WHERE owner_id = $owner_id AND id = $id
                        LIMIT 1;
                        "#,
                    )
                    .bind(("owner_id", _owner_id))
                    .bind(("id", _draft_id))
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        #[derive(Debug, Clone, SurrealValue)]
        struct Row {
            id: Option<i64>,
            owner_id: Option<i64>,
            subject: Option<String>,
            body: Option<String>,
            to_members: Option<Vec<i64>>,
            bcc_members: Option<Vec<i64>>,
            saved_at_ms: Option<i64>,
        }
        let row: Option<Row> = response.take(0).ok().and_then(|mut v: Vec<Row>| v.pop());
        Ok(row.map(|r| PmDraftRecord {
            id: r.id.unwrap_or(_draft_id),
            owner_id: r.owner_id.unwrap_or(_owner_id),
            subject: r.subject.unwrap_or_default(),
            body: r.body.unwrap_or_default(),
            to: r.to_members.unwrap_or_default(),
            bcc: r.bcc_members.unwrap_or_default(),
            saved_at: r
                .saved_at_ms
                .and_then(|ms| Utc.timestamp_millis_opt(ms).single())
                .unwrap_or_else(Utc::now),
        }))
    }

    fn personal_message_overview(
        &self,
        _user_id: i64,
    ) -> Result<PersonalMessageOverview, ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let mut response = rt
            .block_on(async {
                self.client
                    .query(
                        r#"
                        SELECT
                            count() as total,
                            count(is_read = false) as unread
                        FROM personal_messages
                        WHERE owner_id = $owner_id;
                        "#,
                    )
                    .bind(("owner_id", _user_id))
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        #[derive(Debug, Clone, SurrealValue)]
        struct CountRow {
            total: Option<usize>,
            unread: Option<usize>,
        }
        let row: Option<CountRow> = response
            .take(0)
            .ok()
            .and_then(|mut v: Vec<CountRow>| v.pop());
        Ok(PersonalMessageOverview {
            limit: None,
            total: row.as_ref().and_then(|c| c.total).unwrap_or(0),
            unread: row.and_then(|c| c.unread).unwrap_or(0),
        })
    }

    fn personal_message_labels(
        &self,
        _user_id: i64,
    ) -> Result<Vec<PersonalMessageLabel>, ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;

        // Fetch label definitions.
        let mut labels_resp = rt
            .block_on(async {
                self.client
                    .query(
                        r#"
                        SELECT label_id, name
                        FROM pm_labels
                        WHERE owner_id = $owner_id
                        ORDER BY created_at_ms DESC;
                        "#,
                    )
                    .bind(("owner_id", _user_id))
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;

        #[derive(Debug, Clone, SurrealValue)]
        struct LabelRow {
            label_id: Option<i64>,
            name: String,
        }
        let rows: Vec<LabelRow> = labels_resp.take(0).unwrap_or_default();
        let mut labels: std::collections::HashMap<i64, PersonalMessageLabel> = rows
            .into_iter()
            .map(|row| {
                let id = row
                    .label_id
                    .unwrap_or_else(|| Utc::now().timestamp_millis());
                (
                    id,
                    PersonalMessageLabel {
                        id,
                        name: row.name,
                        messages: 0,
                        unread: 0,
                    },
                )
            })
            .collect();

        // Compute counts from messages.
        let mut msg_resp = match rt.block_on(async {
            self.client
                .query(
                    r#"
                    SELECT labels, is_read
                    FROM personal_messages
                    WHERE owner_id = $owner_id;
                    "#,
                )
                .bind(("owner_id", _user_id))
                .await
        }) {
            Ok(resp) => resp,
            Err(err) if Self::is_surreal_unauthorized(&err) => {
                if let Err(reauth_err) = rt.block_on(reauth_from_env(&self.client)) {
                    let fresh = rt.block_on(connect_from_env()).map_err(|connect_err| {
                        ForumError::Internal(format!(
                            "query failed with 401; reauth failed: {}; reconnect failed: {}",
                            reauth_err, connect_err
                        ))
                    })?;
                    let mut retry_resp = rt
                        .block_on(async {
                            fresh
                                .query(
                                    r#"
                                    SELECT labels, is_read
                                    FROM personal_messages
                                    WHERE owner_id = $owner_id;
                                    "#,
                                )
                                .bind(("owner_id", _user_id))
                                .await
                        })
                        .map_err(|e| ForumError::Internal(e.to_string()))?;
                    retry_resp
                } else {
                    rt.block_on(async {
                        self.client
                            .query(
                                r#"
                            SELECT labels, is_read
                            FROM personal_messages
                            WHERE owner_id = $owner_id;
                            "#,
                            )
                            .bind(("owner_id", _user_id))
                            .await
                    })
                    .map_err(|e| ForumError::Internal(e.to_string()))?
                }
            }
            Err(err) => return Err(ForumError::Internal(err.to_string())),
        };

        #[derive(Debug, Clone, SurrealValue)]
        struct MsgRow {
            labels: Option<Vec<i64>>,
            is_read: Option<bool>,
        }
        let msg_rows: Vec<MsgRow> = msg_resp.take(0).unwrap_or_default();
        for row in msg_rows {
            let lbls = row.labels.unwrap_or_default();
            let unread = row.is_read.map(|r| !r).unwrap_or(false);
            for lbl in lbls {
                let entry = labels.entry(lbl).or_insert(PersonalMessageLabel {
                    id: lbl,
                    name: format!("Label {}", lbl),
                    messages: 0,
                    unread: 0,
                });
                entry.messages += 1;
                if unread {
                    entry.unread += 1;
                }
            }
        }

        let mut result: Vec<PersonalMessageLabel> = labels.into_values().collect();
        result.sort_by_key(|l| l.id);
        Ok(result)
    }

    fn personal_message_page(
        &self,
        _user_id: i64,
        _folder: PersonalMessageFolder,
        _label: Option<i64>,
        _start: usize,
        _limit: usize,
    ) -> Result<PersonalMessagePage, ForumError> {
        #[derive(Debug, Clone, SurrealValue)]
        struct PmRow {
            pm_id: i64,
            sender_id: i64,
            sender_name: String,
            subject: String,
            body: String,
            is_read: bool,
            created_at_ms: i64,
            recipients: Option<Vec<i64>>,
            labels: Option<Vec<i64>>,
        }

        let folder = match _folder {
            PersonalMessageFolder::Inbox => "Inbox",
            PersonalMessageFolder::Sent => "Sent",
        };

        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;

        let mut base_query = String::from(
            r#"
            SELECT pm_id, sender_id, sender_name, subject, body, is_read, created_at_ms, recipients, labels
            FROM personal_messages
            WHERE owner_id = $owner_id AND folder = $folder
            "#,
        );
        if _label.unwrap_or(-1) >= 0 {
            base_query.push_str(" AND array::contains(labels ?? [], $label)");
        }
        base_query.push_str(" ORDER BY created_at_ms DESC LIMIT $limit START $start;");

        let mut response = match rt.block_on(async {
            let mut query = self.client.query(base_query.clone());
            query = query
                .bind(("owner_id", _user_id))
                .bind(("folder", folder))
                .bind(("limit", _limit as i64))
                .bind(("start", _start as i64));
            if let Some(label) = _label {
                if label >= 0 {
                    query = query.bind(("label", label));
                }
            }
            query.await
        }) {
            Ok(resp) => resp,
            Err(err) if Self::is_surreal_unauthorized(&err) => {
                if let Err(reauth_err) = rt.block_on(reauth_from_env(&self.client)) {
                    let fresh = rt.block_on(connect_from_env()).map_err(|connect_err| {
                        ForumError::Internal(format!(
                            "query failed with 401; reauth failed: {}; reconnect failed: {}",
                            reauth_err, connect_err
                        ))
                    })?;
                    rt.block_on(async {
                        let mut query = fresh.query(base_query.clone());
                        query = query
                            .bind(("owner_id", _user_id))
                            .bind(("folder", folder))
                            .bind(("limit", _limit as i64))
                            .bind(("start", _start as i64));
                        if let Some(label) = _label {
                            if label >= 0 {
                                query = query.bind(("label", label));
                            }
                        }
                        query.await
                    })
                    .map_err(|e| ForumError::Internal(e.to_string()))?
                } else {
                    rt.block_on(async {
                        let mut query = self.client.query(base_query.clone());
                        query = query
                            .bind(("owner_id", _user_id))
                            .bind(("folder", folder))
                            .bind(("limit", _limit as i64))
                            .bind(("start", _start as i64));
                        if let Some(label) = _label {
                            if label >= 0 {
                                query = query.bind(("label", label));
                            }
                        }
                        query.await
                    })
                    .map_err(|e| ForumError::Internal(e.to_string()))?
                }
            }
            Err(err) => return Err(ForumError::Internal(err.to_string())),
        };

        let rows: Vec<PmRow> = response.take(0).unwrap_or_default();

        let messages = rows
            .into_iter()
            .map(|row| {
                let sent_at = Self::parse_ts(row.created_at_ms);
                PersonalMessageSummary {
                    id: row.pm_id,
                    subject: row.subject,
                    body_preview: row.body.chars().take(120).collect(),
                    sender_id: row.sender_id,
                    sender_name: row.sender_name,
                    sent_at,
                    is_read: row.is_read,
                    recipients: row
                        .recipients
                        .unwrap_or_default()
                        .into_iter()
                        .map(|id| PersonalMessagePeer {
                            member_id: id,
                            name: String::new(),
                        })
                        .collect(),
                    labels: row.labels.unwrap_or_default(),
                }
            })
            .collect();

        // counts
        let mut count_query = String::from(
            r#"
            SELECT
                count() as total,
                count(is_read = false) as unread
            FROM personal_messages
            WHERE owner_id = $owner_id AND folder = $folder
            "#,
        );
        if _label.unwrap_or(-1) >= 0 {
            count_query.push_str(" AND array::contains(labels ?? [], $label)");
        }
        count_query.push(';');

        let mut counts = match rt.block_on(async {
            let mut query = self.client.query(count_query.clone());
            query = query.bind(("owner_id", _user_id)).bind(("folder", folder));
            if let Some(label) = _label {
                if label >= 0 {
                    query = query.bind(("label", label));
                }
            }
            query.await
        }) {
            Ok(resp) => resp,
            Err(err) if Self::is_surreal_unauthorized(&err) => {
                if let Err(reauth_err) = rt.block_on(reauth_from_env(&self.client)) {
                    let fresh = rt.block_on(connect_from_env()).map_err(|connect_err| {
                        ForumError::Internal(format!(
                            "count query failed with 401; reauth failed: {}; reconnect failed: {}",
                            reauth_err, connect_err
                        ))
                    })?;
                    rt.block_on(async {
                        let mut query = fresh.query(count_query.clone());
                        query = query.bind(("owner_id", _user_id)).bind(("folder", folder));
                        if let Some(label) = _label {
                            if label >= 0 {
                                query = query.bind(("label", label));
                            }
                        }
                        query.await
                    })
                    .map_err(|e| ForumError::Internal(e.to_string()))?
                } else {
                    rt.block_on(async {
                        let mut query = self.client.query(count_query.clone());
                        query = query.bind(("owner_id", _user_id)).bind(("folder", folder));
                        if let Some(label) = _label {
                            if label >= 0 {
                                query = query.bind(("label", label));
                            }
                        }
                        query.await
                    })
                    .map_err(|e| ForumError::Internal(e.to_string()))?
                }
            }
            Err(err) => return Err(ForumError::Internal(err.to_string())),
        };

        #[derive(Debug, Clone, SurrealValue)]
        struct CountRow {
            total: Option<usize>,
            unread: Option<usize>,
        }
        let count_row: Option<CountRow> =
            counts.take(0).ok().and_then(|mut v: Vec<CountRow>| v.pop());

        Ok(PersonalMessagePage {
            start: _start,
            total: count_row.as_ref().and_then(|c| c.total).unwrap_or(0),
            unread: count_row.as_ref().and_then(|c| c.unread).unwrap_or(0),
            messages,
        })
    }

    fn personal_message_popup(
        &self,
        _user_id: i64,
        _limit: usize,
    ) -> Result<Vec<PersonalMessageSummary>, ForumError> {
        let page =
            self.personal_message_page(_user_id, PersonalMessageFolder::Inbox, None, 0, _limit)?;
        Ok(page.messages)
    }

    fn personal_message_detail(
        &self,
        _user_id: i64,
        _pm_id: i64,
    ) -> Result<Option<PersonalMessageDetail>, ForumError> {
        #[derive(Debug, Clone, SurrealValue)]
        struct PmRow {
            pm_id: i64,
            _owner_id: i64,
            sender_id: i64,
            sender_name: String,
            subject: String,
            body: String,
            is_read: bool,
            created_at_ms: i64,
            recipients: Option<Vec<i64>>,
            labels: Option<Vec<i64>>,
        }

        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;

        let mut response = rt
            .block_on(async {
                self.client
                    .query(
                        r#"
                        SELECT pm_id, owner_id, sender_id, sender_name, subject, body, is_read, created_at_ms, recipients, labels
                        FROM personal_messages
                        WHERE owner_id = $owner_id AND pm_id = $pm_id
                        LIMIT 1;
                        "#,
                    )
                    .bind(("owner_id", _user_id))
                    .bind(("pm_id", _pm_id))
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;

        let row: Option<PmRow> = response.take(0).ok().and_then(|mut v: Vec<PmRow>| v.pop());
        Ok(row.map(|row| PersonalMessageDetail {
            id: row.pm_id,
            subject: row.subject,
            body: row.body,
            sender_id: row.sender_id,
            sender_name: row.sender_name,
            sent_at: Self::parse_ts(row.created_at_ms),
            recipients: row
                .recipients
                .unwrap_or_default()
                .into_iter()
                .map(|id| PersonalMessagePeer {
                    member_id: id,
                    name: String::new(),
                })
                .collect(),
            labels: row.labels.unwrap_or_default(),
            is_read: row.is_read,
        }))
    }

    fn send_personal_message(
        &self,
        request: SendPersonalMessage,
    ) -> Result<PersonalMessageSendResult, ForumError> {
        if request.to.is_empty() && request.bcc.is_empty() {
            return Err(ForumError::Internal("no recipients provided".into()));
        }
        let recipients: Vec<i64> = request
            .to
            .iter()
            .chain(request.bcc.iter())
            .cloned()
            .collect();
        let pm_id = Utc::now().timestamp_millis();
        let created_at_ms = pm_id;

        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;

        for recipient in &recipients {
            rt.block_on(async {
                self.client
                    .query(
                        r#"
                        CREATE personal_messages CONTENT {
                            pm_id: $pm_id,
                            owner_id: $owner_id,
                            sender_id: $sender_id,
                            sender_name: $sender_name,
                            subject: $subject,
                            body: $body,
                            is_read: false,
                            folder: "Inbox",
                            created_at_ms: $created_at_ms,
                            recipients: $recipients,
                            labels: []
                        };
                        "#,
                    )
                    .bind(("pm_id", pm_id))
                    .bind(("owner_id", *recipient))
                    .bind(("sender_id", request.sender_id))
                    .bind(("sender_name", request.sender_name.clone()))
                    .bind(("subject", request.subject.clone()))
                    .bind(("body", request.body.clone()))
                    .bind(("created_at_ms", created_at_ms))
                    .bind(("recipients", recipients.clone()))
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        }

        rt.block_on(async {
            self.client
                .query(
                    r#"
                    CREATE personal_messages CONTENT {
                        pm_id: $pm_id,
                        owner_id: $owner_id,
                        sender_id: $sender_id,
                        sender_name: $sender_name,
                        subject: $subject,
                        body: $body,
                        is_read: true,
                        folder: "Sent",
                        created_at_ms: $created_at_ms,
                        recipients: $recipients,
                        labels: []
                    };
                    "#,
                )
                .bind(("pm_id", pm_id))
                .bind(("owner_id", request.sender_id))
                .bind(("sender_id", request.sender_id))
                .bind(("sender_name", request.sender_name.clone()))
                .bind(("subject", request.subject.clone()))
                .bind(("body", request.body.clone()))
                .bind(("created_at_ms", created_at_ms))
                .bind(("recipients", recipients.clone()))
                .await
        })
        .map_err(|e| ForumError::Internal(e.to_string()))?;

        Ok(PersonalMessageSendResult {
            id: pm_id,
            recipient_ids: recipients,
        })
    }

    fn delete_personal_messages(
        &self,
        _user_id: i64,
        _folder: PersonalMessageFolder,
        _ids: &[i64],
    ) -> Result<(), ForumError> {
        if _ids.is_empty() {
            return Ok(());
        }
        let folder = match _folder {
            PersonalMessageFolder::Inbox => "Inbox",
            PersonalMessageFolder::Sent => "Sent",
        };
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        rt.block_on(async {
            self.client
                .query(
                    r#"
                    DELETE personal_messages
                    WHERE owner_id = $owner_id
                      AND folder = $folder
                      AND pm_id IN $ids;
                    "#,
                )
                .bind(("owner_id", _user_id))
                .bind(("folder", folder))
                .bind(("ids", _ids.to_vec()))
                .await
        })
        .map_err(|e| ForumError::Internal(e.to_string()))?;
        Ok(())
    }

    fn mark_personal_messages(
        &self,
        _user_id: i64,
        _ids: &[i64],
        _read: bool,
    ) -> Result<(), ForumError> {
        if _ids.is_empty() {
            return Ok(());
        }
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        rt.block_on(async {
            self.client
                .query(
                    r#"
                    UPDATE personal_messages SET is_read = $read
                    WHERE owner_id = $owner_id
                      AND pm_id IN $ids;
                    "#,
                )
                .bind(("owner_id", _user_id))
                .bind(("ids", _ids.to_vec()))
                .bind(("read", _read))
                .await
        })
        .map_err(|e| ForumError::Internal(e.to_string()))?;
        Ok(())
    }

    fn prune_personal_messages(&self, _user_id: i64, _days: i64) -> Result<usize, ForumError> {
        let cutoff = Utc::now() - chrono::Duration::days(_days);
        let cutoff_ms = cutoff.timestamp_millis();
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let mut response = rt
            .block_on(async {
                self.client
                    .query(
                        r#"
                        DELETE personal_messages
                        WHERE owner_id = $owner_id
                          AND created_at_ms < $cutoff_ms
                        RETURN count();
                        "#,
                    )
                    .bind(("owner_id", _user_id))
                    .bind(("cutoff_ms", cutoff_ms))
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        #[derive(Debug, Clone, SurrealValue)]
        struct CountRow {
            count: Option<usize>,
        }
        let count: Option<CountRow> = response
            .take(0)
            .ok()
            .and_then(|mut v: Vec<CountRow>| v.pop());
        Ok(count.and_then(|c| c.count).unwrap_or(0))
    }

    fn clear_personal_messages(
        &self,
        _user_id: i64,
        _folder: PersonalMessageFolder,
    ) -> Result<usize, ForumError> {
        let folder = match _folder {
            PersonalMessageFolder::Inbox => "Inbox",
            PersonalMessageFolder::Sent => "Sent",
        };
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let mut response = rt
            .block_on(async {
                self.client
                    .query(
                        r#"
                        DELETE personal_messages
                        WHERE owner_id = $owner_id
                          AND folder = $folder
                        RETURN count();
                        "#,
                    )
                    .bind(("owner_id", _user_id))
                    .bind(("folder", folder))
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        #[derive(Debug, Clone, SurrealValue)]
        struct CountRow {
            count: Option<usize>,
        }
        let count: Option<CountRow> = response
            .take(0)
            .ok()
            .and_then(|mut v: Vec<CountRow>| v.pop());
        Ok(count.and_then(|c| c.count).unwrap_or(0))
    }

    fn create_pm_label(&self, _user_id: i64, _name: &str) -> Result<i64, ForumError> {
        let name = _name.trim();
        if name.is_empty() {
            return Err(ForumError::Validation("label name required".into()));
        }
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let id = Utc::now().timestamp_millis();
        rt.block_on(async {
            self.client
                .query(
                    r#"
                    CREATE pm_labels CONTENT {
                        label_id: $label_id,
                        owner_id: $owner_id,
                        name: $name,
                        created_at_ms: time::now()
                    };
                    "#,
                )
                .bind(("label_id", id))
                .bind(("owner_id", _user_id))
                .bind(("name", name.to_string()))
                .await
        })
        .map_err(|e| ForumError::Internal(e.to_string()))?;
        Ok(id)
    }

    fn rename_pm_label(
        &self,
        _user_id: i64,
        _label_id: i64,
        _name: &str,
    ) -> Result<(), ForumError> {
        let name = _name.trim();
        if name.is_empty() {
            return Err(ForumError::Validation("label name required".into()));
        }
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        rt.block_on(async {
            self.client
                .query(
                    r#"
                    UPDATE pm_labels SET name = $name
                    WHERE owner_id = $owner_id AND label_id = $label_id;
                    "#,
                )
                .bind(("name", name.to_string()))
                .bind(("owner_id", _user_id))
                .bind(("label_id", _label_id))
                .await
        })
        .map_err(|e| ForumError::Internal(e.to_string()))?;
        Ok(())
    }

    fn delete_pm_labels(&self, _user_id: i64, _labels: &[i64]) -> Result<(), ForumError> {
        if _labels.is_empty() {
            return Ok(());
        }
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let labels = _labels.to_vec();
        // Remove label definitions.
        rt.block_on(async {
            self.client
                .query(
                    r#"
                    DELETE pm_labels
                    WHERE owner_id = $owner_id
                      AND label_id IN $labels;
                    "#,
                )
                .bind(("owner_id", _user_id))
                .bind(("labels", labels.clone()))
                .await
        })
        .map_err(|e| ForumError::Internal(e.to_string()))?;

        // Strip labels from existing messages.
        let mut response = rt
            .block_on(async {
                self.client
                    .query(
                        r#"
                        SELECT pm_id, labels
                        FROM personal_messages
                        WHERE owner_id = $owner_id;
                        "#,
                    )
                    .bind(("owner_id", _user_id))
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        #[derive(Debug, Clone, SurrealValue)]
        struct Row {
            pm_id: i64,
            labels: Option<Vec<i64>>,
        }
        let rows: Vec<Row> = response.take(0).unwrap_or_default();
        let label_set: std::collections::HashSet<i64> = _labels.iter().copied().collect();
        for row in rows {
            let mut updated = row.labels.unwrap_or_default();
            let len_before = updated.len();
            updated.retain(|l| !label_set.contains(l));
            if updated.len() != len_before {
                rt.block_on(async {
                    self.client
                        .query(
                            r#"
                            UPDATE personal_messages SET labels = $labels
                            WHERE owner_id = $owner_id AND pm_id = $pm_id;
                            "#,
                        )
                        .bind(("labels", updated.clone()))
                        .bind(("owner_id", _user_id))
                        .bind(("pm_id", row.pm_id))
                        .await
                })
                .map_err(|e| ForumError::Internal(e.to_string()))?;
            }
        }
        Ok(())
    }

    fn label_personal_messages(
        &self,
        _user_id: i64,
        _ids: &[i64],
        _label_id: i64,
        _add: bool,
    ) -> Result<(), ForumError> {
        if _ids.is_empty() {
            return Ok(());
        }
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        let ids = _ids.to_vec();
        let mut response = rt
            .block_on(async {
                self.client
                    .query(
                        r#"
                        SELECT pm_id, labels
                        FROM personal_messages
                        WHERE owner_id = $owner_id AND pm_id IN $ids;
                        "#,
                    )
                    .bind(("owner_id", _user_id))
                    .bind(("ids", ids))
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        #[derive(Debug, Clone, SurrealValue)]
        struct Row {
            pm_id: i64,
            labels: Option<Vec<i64>>,
        }
        let rows: Vec<Row> = response.take(0).unwrap_or_default();
        for row in rows {
            let mut labels = row.labels.unwrap_or_default();
            if _add {
                if !labels.contains(&_label_id) {
                    labels.push(_label_id);
                }
            } else {
                labels.retain(|l| l != &_label_id);
            }
            rt.block_on(async {
                self.client
                    .query(
                        r#"
                        UPDATE personal_messages SET labels = $labels
                        WHERE owner_id = $owner_id AND pm_id = $pm_id;
                        "#,
                    )
                    .bind(("labels", labels.clone()))
                    .bind(("owner_id", _user_id))
                    .bind(("pm_id", row.pm_id))
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        }
        Ok(())
    }

    fn search_personal_messages(
        &self,
        _user_id: i64,
        _query: &PersonalMessageSearchQuery,
    ) -> Result<Vec<PersonalMessageSummary>, ForumError> {
        let page =
            self.personal_message_page(_user_id, PersonalMessageFolder::Inbox, None, 0, 200)?;
        let text = _query.text.to_lowercase();
        let filtered = page
            .messages
            .into_iter()
            .filter(|msg| {
                msg.subject.to_lowercase().contains(&text)
                    || msg.body_preview.to_lowercase().contains(&text)
            })
            .collect();
        Ok(filtered)
    }

    fn list_mentions(
        &self,
        content_type: &str,
        content_id: i64,
    ) -> Result<Vec<MentionRecord>, ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        #[derive(Debug, Clone, SurrealValue)]
        struct Row {
            id: Option<String>,
            content_type: Option<String>,
            content_id: Option<i64>,
            author_id: Option<i64>,
            mentioned_id: Option<i64>,
            time_ms: Option<i64>,
        }
        let mut response = rt
            .block_on(async {
                self.client
                    .query(
                        r#"
                        SELECT meta::id(id) AS id, content_type, content_id, author_id, mentioned_id, time_ms
                        FROM mentions
                        WHERE content_type = $content_type AND content_id = $content_id
                        ORDER BY time_ms ASC;
                        "#,
                    )
                    .bind(("content_type", content_type.to_string()))
                    .bind(("content_id", content_id))
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        let rows: Vec<Row> = response.take(0).unwrap_or_default();
        Ok(rows
            .into_iter()
            .map(|row| MentionRecord {
                id: row
                    .id
                    .as_deref()
                    .and_then(|id| id.split(':').last())
                    .and_then(|id| id.parse().ok())
                    .unwrap_or_default(),
                content_type: row.content_type.unwrap_or_else(|| content_type.to_string()),
                content_id: row.content_id.unwrap_or(content_id),
                author_id: row.author_id.unwrap_or_default(),
                mentioned_id: row.mentioned_id.unwrap_or_default(),
                time: row
                    .time_ms
                    .and_then(|ms| Utc.timestamp_millis_opt(ms).single())
                    .unwrap_or_else(Utc::now),
            })
            .collect())
    }

    fn insert_mentions(&self, records: &[MentionRecord]) -> Result<(), ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        for record in records {
            let mention_id = if record.id > 0 {
                record.id
            } else {
                Utc::now().timestamp_millis()
            };
            rt.block_on(async {
                self.client
                    .query(
                        r#"
                        UPSERT type::thing("mentions", $id) SET
                            content_type = $content_type,
                            content_id = $content_id,
                            author_id = $author_id,
                            mentioned_id = $mentioned_id,
                            time_ms = $time_ms;
                        "#,
                    )
                    .bind(("id", mention_id))
                    .bind(("content_type", record.content_type.clone()))
                    .bind(("content_id", record.content_id))
                    .bind(("author_id", record.author_id))
                    .bind(("mentioned_id", record.mentioned_id))
                    .bind(("time_ms", record.time.timestamp_millis()))
                    .await
            })
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        }
        Ok(())
    }

    fn delete_mentions(
        &self,
        content_type: &str,
        content_id: i64,
        member_ids: &[i64],
    ) -> Result<(), ForumError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ForumError::Internal(format!("runtime init failed: {e}")))?;
        rt.block_on(async {
            self.client
                .query(
                    r#"
                    DELETE mentions
                    WHERE content_type = $content_type
                      AND content_id = $content_id
                      AND mentioned_id INSIDE $member_ids;
                    "#,
                )
                .bind(("content_type", content_type.to_string()))
                .bind(("content_id", content_id))
                .bind(("member_ids", member_ids.to_vec()))
                .await
        })
        .map_err(|e| ForumError::Internal(e.to_string()))?;
        Ok(())
    }
}
