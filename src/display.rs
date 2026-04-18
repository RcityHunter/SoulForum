use crate::personal_messages::{pm_link, update_pm_popup_state};
use crate::security::is_not_banned;
use crate::services::{ForumContext, ForumError, ForumService, MessageData, ServiceResult};
use crate::subs_notify::get_notify_prefs;
use crate::templates::alerts_template::render_alert_menu;
use serde_json::json;

pub struct DisplayController<S: ForumService> {
    service: S,
}

impl<S: ForumService> DisplayController<S> {
    pub fn new(service: S) -> Self {
        Self { service }
    }

    pub fn display(&self, ctx: &mut ForumContext) -> ServiceResult<()> {
        is_not_banned(&self.service, ctx, false)?;
        let topic_id = ctx
            .topic_id
            .ok_or_else(|| ForumError::Validation("no_topic".into()))?;

        self.service.load_template(ctx, "Display")?;
        self.service.increment_topic_views(topic_id)?;

        let per_page = ctx.mod_settings.int("defaultMaxMessages").unwrap_or(20);
        let start = ctx.request.int("start").unwrap_or(0);

        let messages = self
            .service
            .fetch_topic_messages(topic_id, start, per_page)?;

        let rendered: Vec<_> = messages
            .iter()
            .map(|msg| render_message(ctx, msg))
            .collect();
        ctx.context.set("messages", rendered);
        ctx.context.set("page_start", start);
        ctx.context.set("messages_per_page", per_page);
        update_pm_popup_state(ctx);
        let alerts = ctx
            .context
            .get("alerts")
            .and_then(|value| value.as_array().cloned())
            .unwrap_or_default();
        ctx.context.set(
            "alerts_menu",
            render_alert_menu(&alerts, ctx.user_info.unread_messages as usize),
        );

        self.configure_topic_notification(ctx, topic_id)?;
        self.build_normal_buttons(ctx, topic_id);

        Ok(())
    }

    fn configure_topic_notification(
        &self,
        ctx: &mut ForumContext,
        topic_id: i64,
    ) -> ServiceResult<()> {
        let can_notify = !ctx.user_info.is_guest;
        ctx.context.set("can_set_notify", can_notify);
        ctx.context.set("can_unwatch", can_notify);
        if !can_notify {
            ctx.context.set("topic_notification_mode", 0);
            return Ok(());
        }

        let member_id = ctx.user_info.id;
        let pref_key = format!("topic_notify_{}", topic_id);
        let pref_names = vec!["topic_notify".to_string(), pref_key.clone()];
        let prefs = get_notify_prefs(
            &self.service,
            &[member_id],
            Some(pref_names.as_slice()),
            true,
        )?;
        let member_prefs = prefs.get(&member_id).cloned().unwrap_or_default();
        let pref_value = member_prefs
            .get(&pref_key)
            .copied()
            .or_else(|| member_prefs.get("topic_notify").copied())
            .unwrap_or(0);

        let unwatched = self
            .service
            .load_topic_log(member_id, topic_id)?
            .map(|log| log.unwatched)
            .unwrap_or(false);
        ctx.context.set("topic_unwatched", unwatched);

        let mode = if unwatched {
            0
        } else if pref_value & 0x02 != 0 {
            3
        } else if pref_value & 0x01 != 0 {
            2
        } else {
            1
        };
        ctx.context.set("topic_notification_mode", mode);
        Ok(())
    }

    fn build_normal_buttons(&self, ctx: &mut ForumContext, topic_id: i64) {
        if !ctx.context.bool("can_set_notify") {
            return;
        }

        let base_url = format!("{}?action=notifytopic;topic={}", ctx.scripturl, topic_id);
        let session_var = ctx
            .session
            .string("session_var")
            .unwrap_or_else(|| "session".into());
        let session_id = ctx
            .session
            .string("session_id")
            .unwrap_or_else(|| "token".into());
        let session_suffix = format!(";{}={}", session_var, session_id);

        let mode = ctx.context.int("topic_notification_mode").unwrap_or(0);
        let text_key = format!("notify_topic_{}", mode);

        let mut sub_buttons = Vec::new();
        sub_buttons.push(json!({
            "test": "can_unwatch",
            "text": "notify_topic_0",
            "url": format!("{};mode=0{}", base_url, session_suffix),
        }));
        for option in 1..=3 {
            sub_buttons.push(json!({
                "text": format!("notify_topic_{}", option),
                "url": format!("{};mode={}{}", base_url, option, session_suffix),
            }));
        }

        let notify_button = json!({
            "text": text_key,
            "sub_buttons": sub_buttons,
        });

        ctx.context
            .set("normal_buttons", json!({ "notify": notify_button }));
    }
}

fn render_message(ctx: &ForumContext, msg: &MessageData) -> serde_json::Value {
    let pm_url = pm_link(ctx, msg.member_id);
    json!({
        "id": msg.id,
        "subject": msg.subject,
        "body": msg.body,
        "author": msg.member_id,
        "send_pm": pm_url,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{ForumContext, InMemoryService};

    #[test]
    fn display_loads_messages() {
        let service = InMemoryService::default();
        let mut ctx = ForumContext {
            topic_id: Some(1),
            ..Default::default()
        };
        ctx.mod_settings.set("defaultMaxMessages", 10);
        DisplayController::new(service)
            .display(&mut ctx)
            .expect("display should load messages");
    }

    #[test]
    fn display_sets_notify_button() {
        let service = InMemoryService::default();
        let mut ctx = ForumContext {
            topic_id: Some(1),
            ..Default::default()
        };
        ctx.user_info.id = 1;
        ctx.user_info.is_guest = false;
        ctx.user_info.email = "alice@example.com".into();
        ctx.scripturl = "https://forum.local".into();
        ctx.session.set("session_var", "sess");
        ctx.session.set("session_id", "123");
        ctx.mod_settings.set("defaultMaxMessages", 10);

        DisplayController::new(service)
            .display(&mut ctx)
            .expect("display should prepare buttons");

        let buttons = ctx
            .context
            .get("normal_buttons")
            .and_then(|value| value.as_object())
            .expect("normal buttons");
        let notify = buttons
            .get("notify")
            .and_then(|value| value.as_object())
            .expect("notify button");
        assert_eq!(
            notify.get("text").and_then(|value| value.as_str()),
            Some("notify_topic_1")
        );
        let subs = notify
            .get("sub_buttons")
            .and_then(|value| value.as_array())
            .expect("sub buttons");
        assert_eq!(subs.len(), 4);
        assert_eq!(
            subs[0].get("test").and_then(|value| value.as_str()),
            Some("can_unwatch")
        );
    }
}
