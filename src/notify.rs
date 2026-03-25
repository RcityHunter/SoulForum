use crate::load::prepare_action_context;
use crate::services::{
    ForumContext, ForumError, ForumService, NotifyPrefs, ServiceResult, SessionCheckMode,
    TopicLogEntry,
};
use crate::subs_notify::{
    get_member_with_token, get_notify_prefs as load_alert_prefs, set_notify_prefs,
};
use serde_json::json;
use std::cmp::min;

pub fn get_notify_prefs<S: ForumService>(
    ctx: &mut ForumContext,
    service: &S,
) -> ServiceResult<NotifyPrefs> {
    let prefs = service.get_notify_prefs(ctx.user_info.id)?;
    ctx.context.set(
        "notify_prefs",
        json!({ "msg_auto_notify": prefs.msg_auto_notify }),
    );
    Ok(prefs)
}

pub struct NotifyController<S: ForumService> {
    service: S,
}

impl<S: ForumService> NotifyController<S> {
    pub fn new(service: S) -> Self {
        Self { service }
    }

    pub fn board_notify(&self, ctx: &mut ForumContext) -> ServiceResult<()> {
        prepare_action_context(ctx, "notifyboard");
        let board_id = ctx
            .board_id
            .ok_or_else(|| ForumError::Validation("no_board".into()))?;
        let start = ctx.request.int("start").unwrap_or(0);
        let mut mode = ctx.request.int("mode");
        if mode.is_none() {
            if let Some(sa) = ctx.request.string("sa") {
                mode = match sa.as_str() {
                    "on" => Some(3),
                    "off" => Some(-1),
                    _ => None,
                };
            }
        }

        let resolution = self.resolve_member(ctx, "board", board_id)?;

        if mode.is_none() && !ctx.request.contains("xml") {
            self.service.load_template(ctx, "Notify")?;
            ctx.context.set(
                "notification_set",
                self.service
                    .is_board_notification_set(resolution.member_id, board_id)?,
            );
            ctx.context.set(
                "board_href",
                format!("{}?board={}.{}", ctx.scripturl, board_id, start),
            );
            ctx.context.set("start", start);
            ctx.context.set("page_title", Self::notification_title(ctx));
            ctx.context.set("sub_template", "notify_board");
            if resolution.member_id != ctx.user_info.id {
                if let Some(token) = resolution.token.clone() {
                    ctx.context.set(
                        "notify_info",
                        json!({
                            "u": resolution.member_id,
                            "token": token,
                        }),
                    );
                }
            }
            return Ok(());
        }

        let mut final_mode = None;
        if let Some(requested_mode) = mode {
            if !resolution.via_token {
                self.service.check_session(ctx, SessionCheckMode::Get)?;
            }
            let pref_key = format!("board_notify_{}", board_id);
            let resolved_mode =
                self.normalize_mode(resolution.member_id, &pref_key, requested_mode)?;
            let alert_pref = Self::alert_value(resolved_mode);
            set_notify_prefs(
                &self.service,
                resolution.member_id,
                &[(pref_key, alert_pref)],
            )?;
            if resolved_mode > 1 {
                self.service
                    .add_board_notification(resolution.member_id, board_id)?;
            } else {
                self.service
                    .remove_board_notification(resolution.member_id, board_id)?;
            }
            final_mode = Some(resolved_mode);
        }

        if ctx.request.contains("xml") {
            ctx.context.set(
                "xml_data",
                json!({
                    "errors": {
                        "identifier": "error",
                        "children": [{"value": 0}],
                    }
                }),
            );
            ctx.context.set("sub_template", "generic_xml");
            return Ok(());
        }

        if resolution.via_token {
            if let Some(active_mode) = final_mode {
                self.service.load_template(ctx, "Notify")?;
                ctx.context.set("page_title", Self::notification_title(ctx));
                ctx.context.set("sub_template", "notify_pref_changed");
                let key = if active_mode == 3 {
                    "notify_board_subscribed"
                } else {
                    "notify_board_unsubscribed"
                };
                ctx.context.set(
                    "notify_success_msg",
                    Self::format_notify_message(
                        ctx,
                        key,
                        &resolution.email,
                        if active_mode == 3 {
                            "Board notifications enabled for %1$s"
                        } else {
                            "Board notifications disabled for %1$s"
                        },
                    ),
                );
                return Ok(());
            }
        }

        self.service
            .redirect_exit(&format!("board={}.{}", board_id, start))
    }

    pub fn topic_notify(&self, ctx: &mut ForumContext) -> ServiceResult<()> {
        prepare_action_context(ctx, "notifytopic");
        let topic_id = ctx
            .topic_id
            .ok_or_else(|| ForumError::Validation("no_topic".into()))?;
        let start = ctx.request.int("start").unwrap_or(0);
        let mut mode = ctx.request.int("mode");
        if mode.is_none() {
            if let Some(sa) = ctx.request.string("sa") {
                mode = match sa.as_str() {
                    "on" => Some(3),
                    "off" => Some(-1),
                    _ => None,
                };
            }
        }

        let resolution = self.resolve_member(ctx, "topic", topic_id)?;

        if mode.is_none() && !ctx.request.contains("xml") {
            self.service.load_template(ctx, "Notify")?;
            ctx.context.set(
                "notification_set",
                self.service
                    .is_topic_notification_set(resolution.member_id, topic_id)?,
            );
            ctx.context.set(
                "topic_href",
                format!("{}?topic={}.{}", ctx.scripturl, topic_id, start),
            );
            ctx.context.set("start", start);
            ctx.context.set("page_title", Self::notification_title(ctx));
            ctx.context.set("sub_template", "notify_topic");
            if resolution.member_id != ctx.user_info.id {
                if let Some(token) = resolution.token.clone() {
                    ctx.context.set(
                        "notify_info",
                        json!({
                            "u": resolution.member_id,
                            "token": token,
                        }),
                    );
                }
            }
            return Ok(());
        }

        let mut final_mode = None;
        if let Some(requested_mode) = mode {
            if !resolution.via_token {
                self.service.check_session(ctx, SessionCheckMode::Get)?;
            }
            let pref_key = format!("topic_notify_{}", topic_id);
            let resolved_mode =
                self.normalize_mode(resolution.member_id, &pref_key, requested_mode)?;
            let alert_pref = Self::alert_value(resolved_mode);
            let mut log_entry = self
                .service
                .load_topic_log(resolution.member_id, topic_id)?
                .unwrap_or(TopicLogEntry {
                    member_id: resolution.member_id,
                    topic_id,
                    last_msg_id: 0,
                    unwatched: resolved_mode == 0,
                });
            log_entry.unwatched = resolved_mode == 0;
            self.service.save_topic_log(log_entry)?;
            set_notify_prefs(
                &self.service,
                resolution.member_id,
                &[(pref_key, alert_pref)],
            )?;
            if resolved_mode > 1 {
                self.service
                    .add_topic_notification(resolution.member_id, topic_id)?;
            } else {
                self.service
                    .remove_topic_notification(resolution.member_id, topic_id)?;
            }
            final_mode = Some(resolved_mode);
        }

        if ctx.request.contains("xml") {
            ctx.context.set(
                "xml_data",
                json!({
                    "errors": {
                        "identifier": "error",
                        "children": [{"value": 0}],
                    }
                }),
            );
            ctx.context.set("sub_template", "generic_xml");
            return Ok(());
        }

        if resolution.via_token {
            if let Some(active_mode) = final_mode {
                self.service.load_template(ctx, "Notify")?;
                ctx.context.set("page_title", Self::notification_title(ctx));
                ctx.context.set("sub_template", "notify_pref_changed");
                let key = if active_mode == 3 {
                    "notify_topic_subscribed"
                } else {
                    "notify_topic_unsubscribed"
                };
                ctx.context.set(
                    "notify_success_msg",
                    Self::format_notify_message(
                        ctx,
                        key,
                        &resolution.email,
                        if active_mode == 3 {
                            "Topic notifications enabled for %1$s"
                        } else {
                            "Topic notifications disabled for %1$s"
                        },
                    ),
                );
                return Ok(());
            }
        }

        self.service
            .redirect_exit(&format!("topic={}.{}", topic_id, start))
    }

    fn resolve_member(
        &self,
        ctx: &mut ForumContext,
        notif_type: &str,
        item_id: i64,
    ) -> ServiceResult<MemberResolution> {
        if let (Some(member_id), Some(token)) = (ctx.request.int("u"), ctx.request.string("token"))
        {
            let (id, email) =
                get_member_with_token(&self.service, member_id, None, &token, notif_type, item_id)?;
            Ok(MemberResolution {
                member_id: id,
                email,
                via_token: true,
                token: Some(token),
            })
        } else {
            if ctx.user_info.is_guest {
                return Err(ForumError::PermissionDenied("not_logged_in".into()));
            }
            Ok(MemberResolution {
                member_id: ctx.user_info.id,
                email: ctx.user_info.email.clone(),
                via_token: false,
                token: None,
            })
        }
    }

    fn normalize_mode(
        &self,
        member_id: i64,
        pref_key: &str,
        requested_mode: i64,
    ) -> ServiceResult<i32> {
        if requested_mode != -1 {
            return Ok(requested_mode as i32);
        }
        let prefs = load_alert_prefs(
            &self.service,
            &[member_id],
            Some(&[pref_key.to_string()]),
            true,
        )?;
        let current = prefs
            .get(&member_id)
            .and_then(|map| map.get(pref_key))
            .copied()
            .unwrap_or(0);
        Ok(min(2, current))
    }

    fn alert_value(mode: i32) -> i32 {
        if mode <= 1 {
            0
        } else if mode == 2 {
            1
        } else {
            3
        }
    }

    fn notification_title(ctx: &ForumContext) -> String {
        ctx.txt
            .string("notification")
            .unwrap_or_else(|| "Notifications".into())
    }

    fn format_notify_message(ctx: &ForumContext, key: &str, email: &str, fallback: &str) -> String {
        let template = ctx.txt.string(key).unwrap_or_else(|| fallback.to_string());
        template.replace("%1$s", email)
    }
}

struct MemberResolution {
    member_id: i64,
    email: String,
    via_token: bool,
    token: Option<String>,
}

pub fn set_auto_notify<S: ForumService>(
    ctx: &mut ForumContext,
    service: &S,
    enabled: bool,
) -> ServiceResult<NotifyPrefs> {
    let prefs = service.update_notify_pref(ctx.user_info.id, enabled)?;
    ctx.context.set("auto_notify", prefs.msg_auto_notify);
    Ok(prefs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{ForumContext, InMemoryService};

    #[test]
    fn prefs_roundtrip() {
        let service = InMemoryService::default();
        let mut ctx = ForumContext::default();
        let prefs = set_auto_notify(&mut ctx, &service, true).unwrap();
        assert!(prefs.msg_auto_notify);
        let refreshed = get_notify_prefs(&mut ctx, &service).unwrap();
        assert!(refreshed.msg_auto_notify);
    }

    #[test]
    fn board_notify_configures_template() {
        let service = InMemoryService::default();
        let controller = NotifyController::new(service.clone());
        let mut ctx = ForumContext {
            board_id: Some(1),
            scripturl: "https://forum.local".into(),
            ..Default::default()
        };
        ctx.request.set("start", 0);
        ctx.user_info.id = 1;
        ctx.user_info.is_guest = false;
        ctx.user_info.email = "alice@example.com".into();
        controller.board_notify(&mut ctx).unwrap();
        assert_eq!(
            ctx.context.string("sub_template").as_deref(),
            Some("notify_board")
        );
    }

    #[test]
    fn board_notify_subscribes_member() {
        let service = InMemoryService::default();
        let controller = NotifyController::new(service.clone());
        let mut ctx = ForumContext {
            board_id: Some(1),
            scripturl: "https://forum.local".into(),
            ..Default::default()
        };
        ctx.request.set("start", 0);
        ctx.request.set("mode", 3);
        ctx.user_info.id = 1;
        ctx.user_info.is_guest = false;
        ctx.user_info.email = "alice@example.com".into();
        controller.board_notify(&mut ctx).unwrap();
        assert!(service
            .is_board_notification_set(1, 1)
            .expect("board notify state"));
    }

    #[test]
    fn topic_notify_sets_unwatched_flag() {
        let service = InMemoryService::default();
        let controller = NotifyController::new(service.clone());
        let mut ctx = ForumContext {
            topic_id: Some(1),
            scripturl: "https://forum.local".into(),
            ..Default::default()
        };
        ctx.request.set("start", 0);
        ctx.request.set("mode", 0);
        ctx.user_info.id = 1;
        ctx.user_info.is_guest = false;
        ctx.user_info.email = "alice@example.com".into();
        controller.topic_notify(&mut ctx).unwrap();
        let record = service
            .load_topic_log(1, 1)
            .expect("log load")
            .expect("entry exists");
        assert!(record.unwatched);
    }
}
