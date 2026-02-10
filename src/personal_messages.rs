use crate::drafts::{list_pm_drafts, load_pm_draft, save_pm_draft, PmDraftOptions, PmDraftSummary};
use crate::editor::{
    create_control_richedit, create_control_verification, RichEditOptions, VerificationOptions,
};
use crate::pm_context::load_pm_state;
use crate::pm_ops::{self, RecipientInput};
use crate::services::{
    ForumContext, ForumError, ForumService, PersonalMessageFolder, PersonalMessageSearchQuery,
    ServiceResult, SessionCheckMode,
};
use serde_json::json;

pub struct PersonalMessageController<S: ForumService> {
    service: S,
}

impl<S: ForumService> PersonalMessageController<S> {
    pub fn new(service: S) -> Self {
        Self { service }
    }

    pub fn dispatch(&self, ctx: &mut ForumContext) -> ServiceResult<()> {
        if ctx.user_info.is_guest {
            return Err(ForumError::PermissionDenied("pm_read".into()));
        }
        if !self.service.allowed_to(ctx, "pm_read", None, false) {
            return Err(ForumError::PermissionDenied("pm_read".into()));
        }

        load_pm_state(&self.service, ctx)?;

        self.service.load_language(ctx, "PersonalMessage")?;
        if ctx.mod_settings.bool("drafts_pm_enabled") {
            self.service.load_language(ctx, "Drafts")?;
        }
        if !ctx.request.bool("xml") {
            self.service.load_template(ctx, "PersonalMessage")?;
        }

        let overview = self.service.personal_message_overview(ctx.user_info.id)?;
        ctx.user_info.messages = overview.total as i64;
        ctx.user_info.unread_messages = overview.unread as i64;
        ctx.context.set(
            "pm_overview",
            json!({
                "limit": overview.limit,
                "total": overview.total,
                "unread": overview.unread,
            }),
        );
        ctx.context.set(
            "pm_labels",
            self.service.personal_message_labels(ctx.user_info.id)?,
        );
        ctx.context.set(
            "can_send_pm",
            self.service.allowed_to(ctx, "pm_send", None, false),
        );
        ctx.context
            .set("current_folder", folder_from_request(ctx).to_string());
        ctx.context.set("current_label", ctx.request.int("l"));
        update_pm_popup_state(ctx);

        let subaction = ctx.request.string("sa").unwrap_or_else(|| "folder".into());
        ctx.context.set("pm_subaction", &subaction);

        match subaction.as_str() {
            "popup" => self.popup(ctx),
            "send" => self.compose(ctx),
            "send2" => self.send(ctx),
            "pmactions" => self.apply_actions(ctx),
            "manlabels" => self.manage_labels(ctx),
            "search" => self.search(ctx),
            "search2" => self.search_execute(ctx),
            "prune" => self.prune(ctx),
            "removeall2" => self.remove_all(ctx),
            "showpmdrafts" => self.list_drafts(ctx),
            _ => self.folder(ctx),
        }
    }

    fn folder(&self, ctx: &mut ForumContext) -> ServiceResult<()> {
        let folder = folder_from_request(ctx);
        let start = ctx.request.int("start").unwrap_or(0).max(0) as usize;
        let label = ctx.request.int("l");
        let limit = ctx.mod_settings.int("pm_per_page").unwrap_or(20).max(1) as usize;
        let page =
            self.service
                .personal_message_page(ctx.user_info.id, folder, label, start, limit)?;
        ctx.context.set("pm_page", page);
        Ok(())
    }

    fn popup(&self, ctx: &mut ForumContext) -> ServiceResult<()> {
        let messages = self.service.personal_message_popup(ctx.user_info.id, 5)?;
        ctx.context.set("pm_popup", messages);
        Ok(())
    }

    fn compose(&self, ctx: &mut ForumContext) -> ServiceResult<()> {
        let state = self.collect_form_state(ctx);
        self.apply_draft_context(ctx)?;
        self.apply_quote_context(ctx)?;
        self.prepare_editor(ctx, &state)?;
        Ok(())
    }

    fn send(&self, ctx: &mut ForumContext) -> ServiceResult<()> {
        self.service
            .check_session(ctx, SessionCheckMode::Post)
            .map_err(|_| ForumError::SessionTimeout)?;
        if !self.service.allowed_to(ctx, "pm_send", None, false) {
            return Err(ForumError::PermissionDenied("pm_send".into()));
        }

        let state = self.collect_form_state(ctx);
        if ctx.post_vars.bool("save_draft") {
            self.persist_pm_draft(ctx, &state)?;
            return Ok(());
        }

        let log = pm_ops::send_pm(
            &self.service,
            ctx,
            state.recipients,
            &state.subject,
            &state.body,
        )?;
        ctx.context.set(
            "pm_send_log",
            json!({
                "message_id": log.message_id,
                "sent": log.sent,
                "failed": log
                    .failed
                    .iter()
                    .map(|entry| json!({"target": entry.target, "reason": entry.reason}))
                    .collect::<Vec<_>>(),
            }),
        );
        Ok(())
    }

    fn persist_pm_draft(&self, ctx: &mut ForumContext, state: &PmFormState) -> ServiceResult<()> {
        let resolved = pm_ops::resolve_recipients(&self.service, &state.recipients)?;
        let saved = save_pm_draft(
            ctx,
            &self.service,
            PmDraftOptions {
                id: ctx.post_vars.int("id_pm_draft"),
                subject: state.subject.clone(),
                body: state.body.clone(),
                to: resolved.to,
                bcc: resolved.bcc,
            },
        )?;
        ctx.context.set(
            "pm_draft_saved",
            json!({"id": saved.id, "saved_at": saved.saved_at}),
        );
        Ok(())
    }

    fn list_drafts(&self, ctx: &mut ForumContext) -> ServiceResult<()> {
        let start = ctx.request.int("start").unwrap_or(0).max(0) as usize;
        let drafts = list_pm_drafts(ctx, &self.service, start, 20)?;
        ctx.context.set(
            "pm_drafts",
            drafts
                .iter()
                .map(|draft| self.draft_to_json(draft))
                .collect::<Vec<_>>(),
        );
        Ok(())
    }

    fn draft_to_json(&self, draft: &PmDraftSummary) -> serde_json::Value {
        json!({
            "id": draft.id,
            "subject": draft.subject,
            "to": draft.to,
            "bcc": draft.bcc,
            "saved_at": draft.saved_at,
        })
    }

    fn apply_draft_context(&self, ctx: &mut ForumContext) -> ServiceResult<()> {
        if let Some(draft_id) = ctx.request.int("id_draft") {
            if let Some(draft) = load_pm_draft(ctx, &self.service, draft_id)? {
                ctx.context.set("pm_draft", self.draft_to_json(&draft));
                ctx.post_vars.set("subject", draft.subject);
                ctx.post_vars.set("message", draft.body);
                ctx.post_vars.set(
                    "recipient_to",
                    draft
                        .to
                        .iter()
                        .map(|id| id.to_string())
                        .collect::<Vec<_>>()
                        .join(","),
                );
                ctx.post_vars.set(
                    "recipient_bcc",
                    draft
                        .bcc
                        .iter()
                        .map(|id| id.to_string())
                        .collect::<Vec<_>>()
                        .join(","),
                );
            }
        }
        Ok(())
    }

    fn apply_quote_context(&self, ctx: &mut ForumContext) -> ServiceResult<()> {
        if !ctx.request.bool("quote") {
            return Ok(());
        }
        if let Some(pm_id) = ctx.request.int("pmsg") {
            if let Some(detail) = self
                .service
                .personal_message_detail(ctx.user_info.id, pm_id)?
            {
                let quote = pm_ops::load_pm_quote(&detail);
                ctx.context.set("pm_quote", quote);
            }
        }
        Ok(())
    }

    fn apply_actions(&self, ctx: &mut ForumContext) -> ServiceResult<()> {
        self.service.check_session(ctx, SessionCheckMode::Post)?;
        let ids = parse_id_list(ctx, "pm_ids");
        if ids.is_empty() {
            return Ok(());
        }
        let folder = folder_from_request(ctx);
        match ctx
            .post_vars
            .string("pm_action")
            .unwrap_or_else(|| "markread".into())
            .as_str()
        {
            "delete" => self
                .service
                .delete_personal_messages(ctx.user_info.id, folder, &ids)?,
            "markread" => self
                .service
                .mark_personal_messages(ctx.user_info.id, &ids, true)?,
            "markunread" => self
                .service
                .mark_personal_messages(ctx.user_info.id, &ids, false)?,
            "addlabel" => {
                if let Some(label) = ctx.post_vars.int("label_id") {
                    self.service
                        .label_personal_messages(ctx.user_info.id, &ids, label, true)?;
                }
            }
            "removelabel" => {
                if let Some(label) = ctx.post_vars.int("label_id") {
                    self.service
                        .label_personal_messages(ctx.user_info.id, &ids, label, false)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn manage_labels(&self, ctx: &mut ForumContext) -> ServiceResult<()> {
        if ctx.post_vars.contains("label_action") {
            self.service.check_session(ctx, SessionCheckMode::Post)?;
            match ctx.post_vars.string("label_action").as_deref() {
                Some("add") => {
                    if let Some(name) = ctx.post_vars.string("label_name") {
                        if !name.trim().is_empty() {
                            self.service
                                .create_pm_label(ctx.user_info.id, name.trim())?;
                        }
                    }
                }
                Some("rename") => {
                    if let (Some(id), Some(name)) = (
                        ctx.post_vars.int("label_id"),
                        ctx.post_vars.string("label_name"),
                    ) {
                        self.service
                            .rename_pm_label(ctx.user_info.id, id, name.trim())?;
                    }
                }
                Some("delete") => {
                    let labels = parse_id_list(ctx, "label_ids");
                    self.service.delete_pm_labels(ctx.user_info.id, &labels)?;
                }
                _ => {}
            }
        }

        ctx.context.set(
            "pm_labels",
            self.service.personal_message_labels(ctx.user_info.id)?,
        );
        Ok(())
    }

    fn search(&self, ctx: &mut ForumContext) -> ServiceResult<()> {
        ctx.context.set("pm_search", true);
        Ok(())
    }

    fn search_execute(&self, ctx: &mut ForumContext) -> ServiceResult<()> {
        let text = ctx
            .post_vars
            .string("search")
            .or_else(|| ctx.request.string("search"))
            .unwrap_or_default();
        if text.trim().is_empty() {
            ctx.context.set("pm_search", true);
            return Ok(());
        }
        let query = PersonalMessageSearchQuery {
            text,
            member_filter: ctx
                .post_vars
                .int("member")
                .or_else(|| ctx.request.int("member")),
        };
        let results = self
            .service
            .search_personal_messages(ctx.user_info.id, &query)?;
        ctx.context.set("pm_search_results", results);
        Ok(())
    }

    fn prune(&self, ctx: &mut ForumContext) -> ServiceResult<()> {
        self.service.check_session(ctx, SessionCheckMode::Post)?;
        let days = ctx.post_vars.int("days").unwrap_or(30);
        let removed = self
            .service
            .prune_personal_messages(ctx.user_info.id, days)?;
        ctx.context.set("pm_pruned", removed as i64);
        Ok(())
    }

    fn remove_all(&self, ctx: &mut ForumContext) -> ServiceResult<()> {
        self.service.check_session(ctx, SessionCheckMode::Post)?;
        let folder = folder_from_request(ctx);
        let removed = self
            .service
            .clear_personal_messages(ctx.user_info.id, folder)?;
        ctx.context.set("pm_removed", removed as i64);
        Ok(())
    }

    fn collect_form_state(&self, ctx: &mut ForumContext) -> PmFormState {
        let recipients = RecipientInput {
            to: parse_name_list(
                ctx.post_vars
                    .string("recipient_to")
                    .or_else(|| ctx.request.string("recipient_to"))
                    .or_else(|| ctx.post_vars.string("to"))
                    .or_else(|| ctx.request.string("to")),
            ),
            bcc: parse_name_list(
                ctx.post_vars
                    .string("recipient_bcc")
                    .or_else(|| ctx.request.string("recipient_bcc"))
                    .or_else(|| ctx.post_vars.string("bcc"))
                    .or_else(|| ctx.request.string("bcc")),
            ),
        };
        let subject = ctx
            .post_vars
            .string("subject")
            .or_else(|| ctx.request.string("subject"))
            .unwrap_or_default();
        let body = ctx
            .post_vars
            .string("message")
            .or_else(|| ctx.request.string("message"))
            .unwrap_or_default();
        PmFormState {
            recipients,
            subject,
            body,
            _store_outbox: ctx.post_vars.bool("outbox"),
        }
    }

    fn prepare_editor(&self, ctx: &mut ForumContext, state: &PmFormState) -> ServiceResult<()> {
        ctx.context.set(
            "pm_compose",
            json!({
                "subject": state.subject,
                "body": state.body,
                "to": state.recipients.to,
                "bcc": state.recipients.bcc,
            }),
        );

        let drafts_enabled = ctx.mod_settings.bool("drafts_pm_enabled")
            && self.service.allowed_to(ctx, "pm_draft", None, false);
        let autosave = drafts_enabled
            && ctx.mod_settings.bool("drafts_autosave_enabled")
            && ctx.options.bool("drafts_autosave_enabled");
        ctx.context.set("drafts_pm_save", drafts_enabled);
        ctx.context.set("drafts_pm_autosave", autosave);

        create_control_richedit(
            ctx,
            RichEditOptions {
                id: "pm_message".into(),
                value: state.body.clone(),
                width: Some("100%".into()),
                height: Some("250px".into()),
                allow_bbc: true,
                allow_smileys: true,
                preview: true,
            },
        )?;

        let verification_limit = ctx.mod_settings.int("pm_posts_verification").unwrap_or(0);
        let require_captcha = verification_limit > 0 && ctx.user_info.posts < verification_limit;
        create_control_verification(
            ctx,
            VerificationOptions {
                id: "pm_send".into(),
                require_captcha,
            },
            false,
        )?;

        Ok(())
    }
}

pub fn update_pm_popup_state(ctx: &mut ForumContext) {
    let last = ctx.session.int("pm_unread_last").unwrap_or(0);
    let current = ctx.user_info.unread_messages;
    let popup = current > last;
    ctx.context.set("pm_popup_alert", popup);
    ctx.session.set("pm_unread_last", current);
}

pub fn pm_link(ctx: &ForumContext, member_id: i64) -> Option<String> {
    if ctx.user_info.is_guest || ctx.user_info.id == member_id {
        return None;
    }
    if ctx.user_info.permissions.contains("pm_send") {
        Some(format!(
            "{}?action=pm;sa=send;u={}",
            ctx.scripturl, member_id
        ))
    } else {
        None
    }
}

pub fn call_pm_menu_hook<S: ForumService>(
    service: &S,
    ctx: &mut ForumContext,
) -> ServiceResult<()> {
    service.call_hook(ctx, "integrate_pm_areas")
}

pub fn ssi_welcome<S: ForumService>(
    service: &S,
    ctx: &mut ForumContext,
    can_register: bool,
) -> ServiceResult<String> {
    if ctx.user_info.is_guest {
        if can_register {
            Ok(format!(
                "Welcome guest! Please register at {}/signup.",
                ctx.scripturl
            ))
        } else {
            Ok(format!(
                "Welcome guest! Please log in at {}/login.",
                ctx.scripturl
            ))
        }
    } else {
        let overview = service.personal_message_overview(ctx.user_info.id)?;
        ctx.context.set("user_messages", overview.total as i64);
        ctx.context
            .set("user_unread_messages", overview.unread as i64);
        if service.allowed_to(ctx, "pm_read", None, false) {
            Ok(format!(
                "Hello {}, you have {} messages ({} new).",
                ctx.user_info.name, overview.total, overview.unread
            ))
        } else {
            Ok(format!("Hello {}!", ctx.user_info.name))
        }
    }
}

fn folder_from_request(ctx: &ForumContext) -> PersonalMessageFolder {
    match ctx.request.string("f").as_deref() {
        Some("sent") => PersonalMessageFolder::Sent,
        _ => PersonalMessageFolder::Inbox,
    }
}

fn parse_name_list(value: Option<String>) -> Vec<String> {
    value
        .unwrap_or_default()
        .split(|c: char| c == ',' || c == ';')
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
        .collect()
}

fn parse_id_list(ctx: &ForumContext, key: &str) -> Vec<i64> {
    let value = ctx
        .post_vars
        .string(key)
        .or_else(|| ctx.request.string(key));
    if value.is_none() {
        if let Some(id) = ctx.post_vars.int(key).or_else(|| ctx.request.int(key)) {
            return vec![id];
        }
    }
    parse_list(value)
}

fn parse_list(value: Option<String>) -> Vec<i64> {
    value
        .unwrap_or_default()
        .split(|c: char| c == ',' || c == ';' || c.is_whitespace())
        .filter_map(|part| part.trim().parse::<i64>().ok())
        .collect()
}

struct PmFormState {
    recipients: RecipientInput,
    subject: String,
    body: String,
    _store_outbox: bool,
}

impl ToString for PersonalMessageFolder {
    fn to_string(&self) -> String {
        match self {
            PersonalMessageFolder::Inbox => "inbox".into(),
            PersonalMessageFolder::Sent => "sent".into(),
        }
    }
}
