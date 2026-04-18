use crate::services::{ForumContext, ForumError, ForumService, ServiceResult};
use crate::subs_notify::get_notify_prefs;
use serde_json::json;

pub fn prepare_board_notify<S: ForumService>(
    ctx: &mut ForumContext,
    service: &S,
) -> ServiceResult<()> {
    let board_id = ctx
        .board_id
        .ok_or_else(|| ForumError::Validation("no_board".into()))?;

    let can_mark = !ctx.user_info.is_guest;
    ctx.context.set("can_mark_notify", can_mark);
    if !can_mark {
        ctx.context.set("board_notification_mode", 1);
        return Ok(());
    }

    let member_id = ctx.user_info.id;
    let is_marked = service.is_board_notification_set(member_id, board_id)?;
    ctx.context.set("is_marked_notify", is_marked);

    let pref_key = format!("board_notify_{}", board_id);
    let pref_names = vec!["board_notify".to_string(), pref_key.clone()];
    let prefs = get_notify_prefs(service, &[member_id], Some(pref_names.as_slice()), true)?;
    let member_prefs = prefs.get(&member_id).cloned().unwrap_or_default();
    let pref_value = member_prefs
        .get(&pref_key)
        .copied()
        .or_else(|| member_prefs.get("board_notify").copied())
        .unwrap_or(0);

    let mode = if !is_marked {
        1
    } else if pref_value & 0x02 != 0 {
        3
    } else if pref_value & 0x01 != 0 {
        2
    } else {
        1
    };
    ctx.context.set("board_notification_mode", mode);

    build_notify_button(ctx, board_id, mode);
    Ok(())
}

fn build_notify_button(ctx: &mut ForumContext, board_id: i64, mode: i64) {
    let base_url = format!("{}?action=notifyboard;board={}", ctx.scripturl, board_id);
    let session_var = ctx
        .session
        .string("session_var")
        .unwrap_or_else(|| "session".into());
    let session_id = ctx
        .session
        .string("session_id")
        .unwrap_or_else(|| "token".into());
    let suffix = format!(";{}={}", session_var, session_id);

    let mut sub_buttons = Vec::new();
    for option in 1..=3 {
        sub_buttons.push(json!({
            "text": format!("notify_board_{}", option),
            "url": format!("{};mode={}{}", base_url, option, suffix),
        }));
    }

    let notify_button = json!({
        "text": format!("notify_board_{}", mode),
        "sub_buttons": sub_buttons,
    });

    let mut buttons = ctx
        .context
        .get("normal_buttons")
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    buttons.insert("notify".into(), notify_button);
    ctx.context.set("normal_buttons", json!(buttons));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{ForumContext, InMemoryService};

    #[test]
    fn guest_defaults_to_mode_one() {
        let service = InMemoryService::default();
        let mut ctx = ForumContext {
            board_id: Some(1),
            scripturl: "https://forum.local".into(),
            ..Default::default()
        };
        ctx.user_info.is_guest = true;
        prepare_board_notify(&mut ctx, &service).unwrap();
        assert_eq!(ctx.context.int("board_notification_mode"), Some(1));
    }

    #[test]
    fn logged_in_member_gets_buttons() {
        let service = InMemoryService::default();
        let mut ctx = ForumContext {
            board_id: Some(1),
            scripturl: "https://forum.local".into(),
            ..Default::default()
        };
        ctx.user_info.id = 1;
        ctx.user_info.is_guest = false;
        ctx.session.set("session_var", "sess");
        ctx.session.set("session_id", "123");
        prepare_board_notify(&mut ctx, &service).unwrap();
        let buttons = ctx
            .context
            .get("normal_buttons")
            .and_then(|value| value.as_object())
            .expect("buttons");
        assert!(buttons.contains_key("notify"));
    }
}
