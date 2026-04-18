use crate::services::{
    ForumContext, ForumError, ForumService, PmPreferenceState, RequestVars, ServiceResult,
};
use crate::subs_notify::{delete_notify_prefs, set_notify_prefs};
use crate::templates::profile_template::{
    render_alert_preferences, AlertChannel, AlertChannelPreference, AlertPreference,
    AlertPreferenceGroup, AlertPreferencePage, GroupToggleOption,
};
use serde_json::json;

pub struct ProfileNotificationController<S: ForumService> {
    service: S,
}

impl<S: ForumService> ProfileNotificationController<S> {
    pub fn new(service: S) -> Self {
        Self { service }
    }

    pub fn show(&self, ctx: &mut ForumContext, member_id: i64) -> ServiceResult<()> {
        ensure_can_edit(ctx, member_id)?;
        let prefs = self.service.get_pm_preferences(member_id)?;
        ctx.context.set("pm_receive_from", prefs.receive_from);
        ctx.context.set("pm_notify_level", prefs.notify_level);

        let html = render_alert_preferences(&build_page(&prefs));
        ctx.context.set("alert_preferences_html", html);
        Ok(())
    }

    pub fn save(&self, ctx: &mut ForumContext, member_id: i64) -> ServiceResult<()> {
        ensure_can_edit(ctx, member_id)?;
        let receive_from = ctx.post_vars.int("pm_receive_from").unwrap_or(0) as i32;
        let site_enabled = ctx.post_vars.contains("mention_alert");
        let email_enabled = ctx.post_vars.contains("mention_email");
        let notify_level =
            (if site_enabled { 0x01 } else { 0 }) | (if email_enabled { 0x02 } else { 0 });

        let prefs = PmPreferenceState {
            receive_from,
            notify_level,
        };
        self.service.save_pm_preferences(member_id, &prefs)?;
        ctx.context.set("saved_alert_preferences", true);
        Ok(())
    }

    pub fn board_subscriptions(&self, ctx: &mut ForumContext, member_id: i64) -> ServiceResult<()> {
        ensure_can_edit(ctx, member_id)?;
        self.process_board_changes(ctx, member_id)?;
        let boards = self.service.list_board_notifications(member_id)?;
        ctx.context.set("board_notifications", json!(boards));
        Ok(())
    }

    pub fn topic_subscriptions(&self, ctx: &mut ForumContext, member_id: i64) -> ServiceResult<()> {
        ensure_can_edit(ctx, member_id)?;
        self.process_topic_changes(ctx, member_id)?;
        let topics = self.service.list_topic_notifications(member_id)?;
        ctx.context.set("topic_notifications", json!(topics));
        Ok(())
    }

    fn process_board_changes(&self, ctx: &mut ForumContext, member_id: i64) -> ServiceResult<()> {
        if ctx.post_vars.bool("edit_notify_boards") {
            let ids = parse_id_list(&ctx.post_vars, "notify_boards");
            if !ids.is_empty() {
                self.service.remove_board_notifications(member_id, &ids)?;
                ctx.context.set("profile_updated", true);
            }
        } else if ctx.post_vars.bool("remove_notify_boards") {
            let ids = parse_id_list(&ctx.post_vars, "notify_boards");
            if !ids.is_empty() {
                let prefs: Vec<String> = ids
                    .iter()
                    .map(|id| format!("board_notify_{}", id))
                    .collect();
                delete_notify_prefs(&self.service, member_id, &prefs)?;
                ctx.context.set("profile_updated", true);
            }
        }
        Ok(())
    }

    fn process_topic_changes(&self, ctx: &mut ForumContext, member_id: i64) -> ServiceResult<()> {
        if ctx.post_vars.bool("edit_notify_topics") {
            let ids = parse_id_list(&ctx.post_vars, "notify_topics");
            if !ids.is_empty() {
                self.service.remove_topic_notifications(member_id, &ids)?;
                let prefs: Vec<(String, i32)> = ids
                    .iter()
                    .map(|id| (format!("topic_notify_{}", id), 0))
                    .collect();
                set_notify_prefs(&self.service, member_id, &prefs)?;
                ctx.context.set("profile_updated", true);
            }
        } else if ctx.post_vars.bool("remove_notify_topics") {
            let ids = parse_id_list(&ctx.post_vars, "notify_topics");
            if !ids.is_empty() {
                let prefs: Vec<String> = ids
                    .iter()
                    .map(|id| format!("topic_notify_{}", id))
                    .collect();
                delete_notify_prefs(&self.service, member_id, &prefs)?;
                ctx.context.set("profile_updated", true);
            }
        }
        Ok(())
    }
}

fn build_page(prefs: &PmPreferenceState) -> AlertPreferencePage {
    let site_enabled = (prefs.notify_level & 0x01) != 0;
    let email_enabled = (prefs.notify_level & 0x02) != 0;
    let group = AlertPreferenceGroup {
        id: "mentions".into(),
        label: "Mentions".into(),
        options: vec![GroupToggleOption {
            id: "mentions_notify".into(),
            label: "Notify me when mentioned".into(),
            description: None,
            enabled: site_enabled || email_enabled,
        }],
        alerts: vec![AlertPreference {
            id: "mention".into(),
            label: "When someone mentions me".into(),
            help_link: None,
            channels: vec![
                AlertChannelPreference {
                    channel: AlertChannel::Alert,
                    enabled: site_enabled,
                    allowed: true,
                },
                AlertChannelPreference {
                    channel: AlertChannel::Email,
                    enabled: email_enabled,
                    allowed: true,
                },
                AlertChannelPreference {
                    channel: AlertChannel::Push,
                    enabled: false,
                    allowed: false,
                },
            ],
        }],
    };
    AlertPreferencePage {
        description: "Control how you receive mention alerts.".into(),
        show_notify_once: true,
        notify_once_enabled: false,
        alert_timeout: Some(30),
        groups: vec![group],
    }
}

fn ensure_can_edit(ctx: &ForumContext, member_id: i64) -> ServiceResult<()> {
    if ctx.user_info.id == member_id || ctx.user_info.permissions.contains("profile_extra_any") {
        Ok(())
    } else {
        Err(ForumError::PermissionDenied("profile_extra".into()))
    }
}

fn parse_id_list(vars: &RequestVars, key: &str) -> Vec<i64> {
    if let Some(raw) = vars.string(key) {
        if raw.trim().is_empty() {
            return Vec::new();
        }
        if raw.trim_start().starts_with('[') {
            serde_json::from_str(&raw).unwrap_or_default()
        } else {
            raw.split(',')
                .filter_map(|part| part.trim().parse::<i64>().ok())
                .collect()
        }
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{ForumContext, InMemoryService};

    #[test]
    fn renders_alert_preferences_html() {
        let service = InMemoryService::default();
        let controller = ProfileNotificationController::new(service.clone());
        let mut ctx = ForumContext::default();
        ctx.user_info.id = 2;
        ctx.user_info.permissions.insert("profile_extra_any".into());
        controller.show(&mut ctx, 2).unwrap();
        let html = ctx.context.string("alert_preferences_html").unwrap();
        assert!(html.contains("Mentions"));
    }

    #[test]
    fn saves_alert_preferences() {
        let service = InMemoryService::default();
        let controller = ProfileNotificationController::new(service.clone());
        let mut ctx = ForumContext::default();
        ctx.user_info.id = 1;
        ctx.user_info.permissions.insert("profile_extra_any".into());
        ctx.post_vars.set("pm_receive_from", 1);
        ctx.post_vars.set("mention_alert", true);
        controller.save(&mut ctx, 1).unwrap();
        assert!(ctx.context.bool("saved_alert_preferences"));
    }

    #[test]
    fn board_notifications_listed_and_removed() {
        let service = InMemoryService::default();
        service.add_board_notification(1, 1).unwrap();
        let controller = ProfileNotificationController::new(service.clone());
        let mut ctx = ForumContext::default();
        ctx.user_info.id = 1;
        controller.board_subscriptions(&mut ctx, 1).unwrap();
        assert!(ctx.context.get("board_notifications").is_some());

        ctx.post_vars.set("edit_notify_boards", true);
        ctx.post_vars.set("notify_boards", "[1]");
        controller.board_subscriptions(&mut ctx, 1).unwrap();
        let entries = ctx
            .context
            .get("board_notifications")
            .and_then(|value| value.as_array())
            .unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn topic_notifications_remove_prefs() {
        let service = InMemoryService::default();
        service.add_topic_notification(1, 1).unwrap();
        let controller = ProfileNotificationController::new(service.clone());
        let mut ctx = ForumContext::default();
        ctx.user_info.id = 1;
        controller.topic_subscriptions(&mut ctx, 1).unwrap();
        assert!(ctx.context.get("topic_notifications").is_some());

        ctx.post_vars.set("remove_notify_topics", true);
        ctx.post_vars.set("notify_topics", "[1]");
        controller.topic_subscriptions(&mut ctx, 1).unwrap();
        assert!(ctx.context.bool("profile_updated"));
    }
}
