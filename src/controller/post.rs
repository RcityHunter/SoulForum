use crate::drafts::{save_draft, DraftOptions};
use crate::editor::{
    create_control_richedit, create_control_verification, RichEditOptions, VerificationOptions,
};
use crate::post_ops::{create_post, modify_post, MessageOptions, PosterOptions, TopicOptions};
use crate::services::{
    BoardListOptions, ForumContext, ForumError, ForumService, ServiceResult, SessionCheckMode,
    TopicPostingContext,
};
use serde_json::json;

pub struct PostController<S: ForumService> {
    service: S,
}

impl<S: ForumService> PostController<S> {
    pub fn new(service: S) -> Self {
        Self { service }
    }

    pub fn post(&self, ctx: &mut ForumContext) -> ServiceResult<()> {
        self.service.load_language(ctx, "Post")?;
        if ctx.mod_settings.bool("drafts_post_enabled") {
            self.service.load_language(ctx, "Drafts")?;
        }

        if ctx.request.bool("poll") && ctx.topic_id.is_some() && !ctx.request.contains("msg") {
            ctx.request.remove("poll");
        }

        let make_event = ctx.request.bool("calendar");
        ctx.context.set("make_event", make_event);
        ctx.context.set("robot_no_index", true);

        self.service.call_hook(ctx, "integrate_post_start")?;

        let notify_prefs = self.service.get_notify_prefs(ctx.user_info.id)?;
        ctx.context.set(
            "notify_prefs",
            json!({ "msg_auto_notify": notify_prefs.msg_auto_notify }),
        );
        ctx.context.set("auto_notify", notify_prefs.msg_auto_notify);

        let mut allowed_boards = Vec::new();
        if ctx.board_id.is_none() || make_event {
            let mut perms = vec!["post_new".to_string()];
            if ctx.mod_settings.bool("postmod_active") {
                perms.push("post_unapproved_topics".to_string());
            }
            allowed_boards = self.service.boards_allowed_to(ctx, &perms)?;
            if allowed_boards.is_empty() {
                return Err(ForumError::PermissionDenied("cannot_post_new".into()));
            }

            let mut board_options = BoardListOptions::default();
            if !allowed_boards.contains(&0) {
                board_options.included_boards = Some(allowed_boards.clone());
            }
            board_options.not_redirection = true;
            board_options.use_permissions = true;
            board_options.selected_board = ctx.board_id.or_else(|| allowed_boards.first().copied());
            let board_list = self.service.get_board_list(ctx, &board_options)?;
            ctx.context.set(
                "board_list",
                board_list
                    .iter()
                    .map(|board| json!({"id": board.id, "name": board.name}))
                    .collect::<Vec<_>>(),
            );
        } else if let Some(board) = ctx.board_id {
            allowed_boards.push(board);
        }

        self.ensure_topic_from_msg(ctx)?;

        let topic_context = ctx.topic_id.and_then(|topic_id| {
            self.service
                .fetch_topic_posting_context(topic_id)
                .ok()
                .flatten()
        });

        if let Some(topic_ctx) = &topic_context {
            ctx.context.set("topic_locked", topic_ctx.locked);
            ctx.context.set("topic_approved", topic_ctx.approved);
            ctx.context.set("notify", topic_ctx.notify);
            if let Some(subject) = &topic_ctx.subject {
                ctx.context.set("subject", subject);
            }
        }

        self.configure_approval_state(ctx, topic_context.as_ref(), &allowed_boards)?;
        self.configure_locking(ctx, topic_context.as_ref(), &allowed_boards);

        ctx.context.set("can_notify", !ctx.user_info.is_guest);
        ctx.context.set("move", ctx.request.bool("move"));
        ctx.context.set("announce", ctx.request.bool("announce"));

        let locked = ctx.context.bool("topic_locked");
        ctx.context
            .set("locked", locked || ctx.request.bool("lock"));
        ctx.context
            .set("can_quote", !ctx.mod_settings.contains("disabledBBC"));

        if !ctx.request.contains("message")
            && !ctx.request.contains("preview")
            && ctx.session.contains("already_attached")
        {
            ctx.session.remove("already_attached");
        }

        if locked && !self.service.allowed_to(ctx, "moderate_board", None, false) {
            return Err(ForumError::PermissionDenied("topic_locked".into()));
        }

        self.handle_poll_state(ctx, topic_context.as_ref(), &allowed_boards)?;

        if ctx.context.bool("make_event") {
            self.prepare_calendar_event(ctx, &allowed_boards)?;
        }

        if let (Some(topic_id), Some(msg_id)) = (ctx.topic_id, ctx.request.int("msg")) {
            self.handle_editing_state(ctx, topic_id, msg_id, &allowed_boards)?;
        }

        let editor_value = ctx.context.string("form_message").unwrap_or_default();
        create_control_richedit(
            ctx,
            RichEditOptions {
                id: "post_box".into(),
                value: editor_value,
                width: Some("100%".into()),
                height: Some("200px".into()),
                allow_bbc: true,
                allow_smileys: true,
                preview: true,
            },
        )?;

        let require_captcha = ctx.mod_settings.bool("posts_require_captcha");
        create_control_verification(
            ctx,
            VerificationOptions {
                id: "post".into(),
                require_captcha,
            },
            false,
        )?;

        Ok(())
    }

    fn ensure_topic_from_msg(&self, ctx: &mut ForumContext) -> ServiceResult<()> {
        if ctx.topic_id.is_none() {
            if let Some(msg_id) = ctx.request.int("msg") {
                if let Some(topic_id) = self.service.find_topic_id_by_msg(msg_id)? {
                    ctx.topic_id = Some(topic_id);
                } else {
                    ctx.request.remove("msg");
                }
            }
        }
        Ok(())
    }

    fn configure_approval_state(
        &self,
        ctx: &mut ForumContext,
        topic_ctx: Option<&TopicPostingContext>,
        boards: &[i64],
    ) -> ServiceResult<()> {
        let mut becomes_approved = true;
        if ctx.topic_id.is_none() {
            if ctx.mod_settings.bool("postmod_active")
                && !self.service.allowed_to(ctx, "post_new", Some(boards), true)
                && self
                    .service
                    .allowed_to(ctx, "post_unapproved_topics", Some(boards), true)
            {
                becomes_approved = false;
            } else {
                self.service.allowed_to(ctx, "post_new", Some(boards), true);
            }
        } else if ctx.mod_settings.bool("postmod_active")
            && !self
                .service
                .allowed_to(ctx, "post_reply_any", Some(boards), true)
            && self
                .service
                .allowed_to(ctx, "post_unapproved_replies", Some(boards), true)
        {
            becomes_approved = false;
        }

        if let Some(topic_ctx) = topic_ctx {
            if !topic_ctx.approved
                && !self
                    .service
                    .allowed_to(ctx, "approve_posts", Some(boards), true)
            {
                becomes_approved = false;
            }
        }

        ctx.context.set("becomes_approved", becomes_approved);
        Ok(())
    }

    fn configure_locking(
        &self,
        ctx: &mut ForumContext,
        topic_ctx: Option<&TopicPostingContext>,
        boards: &[i64],
    ) {
        let can_lock = self.service.allowed_to(ctx, "lock_any", Some(boards), true)
            || self.service.allowed_to(ctx, "lock_own", Some(boards), true);
        ctx.context.set("can_lock", can_lock);
        let can_sticky = self
            .service
            .allowed_to(ctx, "make_sticky", Some(boards), true);
        ctx.context.set("can_sticky", can_sticky);

        if let Some(topic_ctx) = topic_ctx {
            ctx.context.set("already_locked", topic_ctx.locked);
            ctx.context.set("already_sticky", topic_ctx.sticky);
        }
    }

    fn handle_poll_state(
        &self,
        ctx: &mut ForumContext,
        topic_ctx: Option<&TopicPostingContext>,
        boards: &[i64],
    ) -> ServiceResult<()> {
        if !ctx.request.bool("poll") {
            return Ok(());
        }

        if ctx.mod_settings.string("pollMode").unwrap_or_default() != "1" {
            return Ok(());
        }

        if ctx.topic_id.is_none() {
            self.service
                .allowed_to(ctx, "poll_post", Some(boards), true);
        } else if let Some(topic_ctx) = topic_ctx {
            if topic_ctx.id_member_started == ctx.user_info.id
                && !self
                    .service
                    .allowed_to(ctx, "poll_add_any", Some(boards), true)
            {
                self.service
                    .allowed_to(ctx, "poll_add_own", Some(boards), true);
            } else {
                self.service
                    .allowed_to(ctx, "poll_add_any", Some(boards), true);
            }
        }

        ctx.context.set(
            "poll_options",
            json!({
                "max_votes": ctx.post_vars.int("poll_max_votes").unwrap_or(1).max(1),
                "hide": ctx.post_vars.int("poll_hide").unwrap_or(0),
                "expire": ctx.post_vars.int("poll_expire"),
                "change_vote": ctx.post_vars.bool("poll_change_vote"),
                "guest_vote": ctx.post_vars.bool("poll_guest_vote"),
            }),
        );
        Ok(())
    }

    fn prepare_calendar_event(&self, ctx: &mut ForumContext, boards: &[i64]) -> ServiceResult<()> {
        self.service
            .allowed_to(ctx, "calendar_post", Some(boards), true);
        ctx.context.set(
            "event_title",
            ctx.request.string("evtitle").unwrap_or_default(),
        );
        Ok(())
    }

    fn handle_editing_state(
        &self,
        ctx: &mut ForumContext,
        topic_id: i64,
        msg_id: i64,
        boards: &[i64],
    ) -> ServiceResult<()> {
        let data = self
            .service
            .fetch_message_edit_data(topic_id, msg_id)?
            .ok_or_else(|| ForumError::Validation("no_message".into()))?;

        ctx.context.set("editing", true);
        ctx.context.set("form_subject", data.subject.clone());
        ctx.context.set("form_message", data.body.clone());
        ctx.context.set("use_smileys", data.smileys_enabled);
        ctx.context.set("icon", data.icon.clone());

        if data.id_member == ctx.user_info.id {
            self.ensure_permission(ctx, "modify_own", Some(boards), true)?;
        } else {
            self.ensure_permission(ctx, "modify_any", Some(boards), true)?;
        }

        let attachments: Vec<_> = data
            .attachments
            .iter()
            .map(|att| {
                json!({
                    "attachID": att.id,
                    "name": att.filename,
                    "size": att.size,
                    "approved": att.approved,
                })
            })
            .collect();
        ctx.context.set("current_attachments", attachments);
        ctx.context
            .set("destination", format!("post2;msg={msg_id}"));
        Ok(())
    }

    fn ensure_permission(
        &self,
        ctx: &ForumContext,
        permission: &str,
        boards: Option<&[i64]>,
        any: bool,
    ) -> ServiceResult<()> {
        if self.service.allowed_to(ctx, permission, boards, any) {
            Ok(())
        } else {
            Err(ForumError::PermissionDenied(permission.into()))
        }
    }

    fn determine_board_id(&self, ctx: &mut ForumContext) -> ServiceResult<i64> {
        if let Some(board) = ctx.board_id {
            return Ok(board);
        }
        if let Some(board) = ctx.request.int("board") {
            ctx.board_id = Some(board);
            return Ok(board);
        }
        if let Some(topic_id) = ctx.topic_id {
            if let Some(topic) = self.service.fetch_topic_posting_context(topic_id)? {
                ctx.board_id = Some(topic.board_id);
                return Ok(topic.board_id);
            }
        }
        Err(ForumError::Validation("missing_board".into()))
    }

    pub fn post2(&self, ctx: &mut ForumContext) -> ServiceResult<()> {
        self.service.check_session(ctx, SessionCheckMode::Post)?;

        let subject = ctx
            .post_vars
            .string("subject")
            .or_else(|| ctx.post_vars.string("quick_subject"))
            .ok_or_else(|| ForumError::Validation("no_subject".into()))?;
        let message = ctx
            .post_vars
            .string("message")
            .or_else(|| ctx.post_vars.string("quickReply"))
            .ok_or_else(|| ForumError::Validation("no_message".into()))?;

        if subject.trim().is_empty() {
            return Err(ForumError::Validation("no_subject".into()));
        }
        if message.trim().is_empty() {
            return Err(ForumError::Validation("no_message".into()));
        }

        let board_id = self.determine_board_id(ctx)?;

        if ctx.post_vars.bool("save_draft") {
            let draft = save_draft(
                ctx,
                &self.service,
                DraftOptions {
                    id: ctx.post_vars.int("id_draft"),
                    topic_id: ctx.topic_id,
                    board_id: Some(board_id),
                    subject: subject.clone(),
                    body: message.clone(),
                    icon: ctx.post_vars.string("icon").unwrap_or_else(|| "xx".into()),
                    smileys_enabled: true,
                    locked: ctx.post_vars.bool("lock"),
                    sticky: ctx.post_vars.bool("sticky"),
                },
            )?;
            ctx.context.set("draft_saved", true);
            ctx.context.set("id_draft", draft.id);
            return Ok(());
        }

        let mut msg_opts = MessageOptions {
            id: ctx.request.int("msg"),
            subject: subject.trim().to_string(),
            body: message,
            icon: ctx.post_vars.string("icon").unwrap_or_else(|| "xx".into()),
            smileys_enabled: true,
            attachments: Vec::new(),
            approved: ctx.context.bool("becomes_approved"),
            poster_time: None,
            send_notifications: true,
        };
        let mut topic_opts = TopicOptions {
            id: ctx.topic_id,
            board: board_id,
            poll: None,
            lock_mode: None,
            sticky_mode: None,
            mark_as_read: true,
            is_approved: ctx.context.bool("becomes_approved"),
        };
        let poster_opts = PosterOptions {
            id: ctx.user_info.id,
            name: ctx.user_info.name.clone(),
            email: ctx.user_info.email.clone(),
            ip: ctx.user_info.ip.clone(),
            update_post_count: !ctx.user_info.is_guest,
        };

        let result = if msg_opts.id.is_some() {
            modify_post(&self.service, ctx, &msg_opts, &topic_opts, &poster_opts)?
        } else {
            let posted = create_post(
                &self.service,
                ctx,
                &mut msg_opts,
                &mut topic_opts,
                &poster_opts,
            )?;
            ctx.topic_id = topic_opts.id;
            posted
        };

        ctx.context.set("last_post_id", result.message_id);
        Ok(())
    }

    pub fn quote_fast(&self, ctx: &mut ForumContext) -> ServiceResult<()> {
        let msg_id = ctx
            .request
            .int("quote")
            .ok_or_else(|| ForumError::Validation("no_quote".into()))?;
        let data = self
            .service
            .fetch_quote_content(msg_id)?
            .ok_or_else(|| ForumError::Validation("no_message".into()))?;
        ctx.context.set("quote_subject", data.subject);
        ctx.context.set("quote_body", data.body);
        Ok(())
    }

    pub fn announce_topic(&self, ctx: &mut ForumContext) -> ServiceResult<()> {
        let topic_id = ctx
            .topic_id
            .ok_or_else(|| ForumError::Validation("no_topic".into()))?;
        self.ensure_permission(ctx, "announce_topic", None, true)?;
        let result = self.service.send_announcement(topic_id)?;
        ctx.context
            .set("announcement_recipients", result.recipients as i64);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::InMemoryService;
    use std::collections::HashSet;

    fn base_context() -> ForumContext {
        let mut ctx = ForumContext::default();
        ctx.user_info.name = "Tester".into();
        ctx.user_info.email = "tester@example.com".into();
        ctx
    }

    #[test]
    fn post_populates_editing_context() {
        let service = InMemoryService::default();
        let controller = PostController::new(service);
        let mut ctx = base_context();
        ctx.board_id = Some(1);
        ctx.topic_id = Some(1);
        ctx.request.set("msg", 1);
        ctx.user_info.id = 1;
        ctx.user_info.permissions = HashSet::from(["modify_own".into()]);

        controller.post(&mut ctx).unwrap();
        assert!(ctx.context.bool("editing"));
        assert_eq!(ctx.context.string("form_subject"), Some("Welcome".into()));
    }

    #[test]
    fn post2_creates_message() {
        let service = InMemoryService::default();
        let controller = PostController::new(service);
        let mut ctx = base_context();
        ctx.board_id = Some(1);
        ctx.user_info.permissions = HashSet::from(["post_new".into()]);
        ctx.context.set("becomes_approved", true);
        ctx.post_vars.set("subject", "Rust rewrite");
        ctx.post_vars.set("message", "Content body");

        controller.post2(&mut ctx).unwrap();
        assert!(ctx.context.int("last_post_id").is_some());
    }

    #[test]
    fn quote_fast_sets_context() {
        let service = InMemoryService::default();
        let controller = PostController::new(service);
        let mut ctx = base_context();
        ctx.request.set("quote", 1);

        controller.quote_fast(&mut ctx).unwrap();
        assert_eq!(
            ctx.context.string("quote_body"),
            Some("Hello from PHP".into())
        );
    }
}
