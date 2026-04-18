use crate::personal_messages::pm_link;
use crate::services::{ForumContext, ForumService, ServiceResult};

pub fn notify_new_post<S: ForumService>(
    service: &S,
    ctx: &ForumContext,
    recipients: &[i64],
) -> ServiceResult<Vec<i64>> {
    if !ctx.mod_settings.bool("enable_mentions") {
        return Ok(Vec::new());
    }
    let mut notified = Vec::new();
    for member_id in recipients {
        if service
            .get_pm_ignore_list(*member_id)?
            .contains(&ctx.user_info.id)
        {
            continue;
        }
        let prefs = service.get_pm_preferences(*member_id)?;
        let wants_alert = (prefs.notify_level & 0x01) != 0;
        let wants_email = (prefs.notify_level & 0x02) != 0;
        if !wants_alert && !wants_email {
            continue;
        }
        if let Some(link) = pm_link(ctx, *member_id) {
            if !link.is_empty() && wants_alert {
                notified.push(*member_id);
            }
        }
    }
    Ok(notified)
}

pub fn notify_like<S: ForumService>(
    service: &S,
    actor_id: i64,
    target_member: i64,
) -> ServiceResult<bool> {
    let ignore = service.get_pm_ignore_list(target_member)?;
    if ignore.contains(&actor_id) {
        return Ok(false);
    }
    let prefs = service.get_pm_preferences(target_member)?;
    Ok((prefs.notify_level & 0x01) != 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{ForumContext, InMemoryService, PmPreferenceState};

    #[test]
    fn ignores_are_respected_for_posts() {
        let service = InMemoryService::default();
        let mut ctx = ForumContext::default();
        ctx.user_info.id = 1;
        ctx.user_info.is_guest = false;
        ctx.user_info.permissions.insert("pm_send".into());
        ctx.scripturl = "https://forum.local".into();
        ctx.mod_settings.set("enable_mentions", true);
        service.set_pm_ignore_list(2, &[1]).unwrap();
        let notified = notify_new_post(&service, &ctx, &[2, 3]).unwrap();
        assert_eq!(notified, vec![3]);
    }

    #[test]
    fn likes_respect_ignore() {
        let service = InMemoryService::default();
        service
            .save_pm_preferences(
                2,
                &PmPreferenceState {
                    receive_from: 0,
                    notify_level: 0x01,
                },
            )
            .unwrap();
        service.set_pm_ignore_list(2, &[1]).unwrap();
        assert!(!notify_like(&service, 1, 2).unwrap());
    }

    #[test]
    fn respects_member_preferences() {
        let service = InMemoryService::default();
        let mut ctx = ForumContext::default();
        ctx.user_info.id = 1;
        ctx.user_info.permissions.insert("pm_send".into());
        ctx.scripturl = "https://forum.local".into();
        ctx.mod_settings.set("enable_mentions", true);
        service
            .save_pm_preferences(
                3,
                &PmPreferenceState {
                    receive_from: 0,
                    notify_level: 0,
                },
            )
            .unwrap();
        let notified = notify_new_post(&service, &ctx, &[3]).unwrap();
        assert!(notified.is_empty());
    }
}
