use crate::services::{
    BanCondition, BanRule, ForumContext, ForumError, ForumService, PermissionChange, ServiceResult,
};
use chrono::Utc;
use std::collections::HashSet;

pub fn load_permissions<S: ForumService>(
    service: &S,
    ctx: &mut ForumContext,
    board_id: Option<String>,
) -> ServiceResult<()> {
    if ctx.user_info.is_admin {
        ban_permissions(ctx);
        return Ok(());
    }
    is_not_banned(service, ctx, false)?;
    let mut groups = ctx.user_info.groups.clone();
    if groups.is_empty() {
        groups.push(4);
    }
    let mut grants = Vec::new();
    let mut removals = Vec::new();
    apply_changes(
        &service.general_permissions(&groups)?,
        &mut grants,
        &mut removals,
    );
    if let Some(board) = board_id {
        apply_changes(
            &service.board_permissions(&board, &groups)?,
            &mut grants,
            &mut removals,
        );
    }
    let mut permission_set: HashSet<String> = grants.into_iter().collect();
    if ctx.mod_settings.bool("permission_enable_deny") {
        for remove in removals {
            permission_set.remove(&remove);
        }
    }
    ctx.user_info.permissions = permission_set;
    if !ctx.user_info.is_guest {
        ctx.user_info.permissions.insert("is_not_guest".into());
        if ctx.user_info.permissions.contains("profile_view") {
            ctx.user_info.permissions.insert("profile_view_any".into());
        }
    }
    ban_permissions(ctx);
    Ok(())
}

fn condition_matches(ctx: &ForumContext, condition: &BanCondition) -> bool {
    if let Some(expire) = condition.expires_at {
        if expire < Utc::now() {
            return false;
        }
    }
    match &condition.affects {
        crate::services::BanAffects::Account { member_id } => ctx.user_info.id == *member_id,
        crate::services::BanAffects::Email { value } => {
            ctx.user_info.email.eq_ignore_ascii_case(value)
        }
        crate::services::BanAffects::Ip { value } => ctx.user_info.ip == *value,
    }
}

fn apply_ban_context(ctx: &mut ForumContext, rule: &BanRule) {
    ctx.session.set("ban_active", true);
    ctx.session.set("ban_rule", rule.id);
    ctx.session.set("ban_cannot_post", rule.cannot_post);
    ctx.session.set("ban_cannot_access", rule.cannot_access);
    if let Some(reason) = &rule.reason {
        ctx.session.set("ban_reason", reason);
    }
}

pub fn is_not_banned<S: ForumService>(
    service: &S,
    ctx: &mut ForumContext,
    force: bool,
) -> ServiceResult<()> {
    if ctx.user_info.is_admin {
        return Ok(());
    }
    let now = Utc::now().timestamp();
    if !force {
        if let Some(last) = ctx.session.int("ban_last_checked") {
            if now - last < 60 {
                if ctx.session.bool("ban_active") {
                    return Err(ForumError::PermissionDenied("banned".into()));
                }
                return Ok(());
            }
        }
    }
    ctx.session.set("ban_last_checked", now);
    let rules = service.list_ban_rules()?;
    for rule in rules {
        if let Some(expire) = rule.expires_at {
            if expire < Utc::now() {
                continue;
            }
        }
        if rule
            .conditions
            .iter()
            .any(|cond| condition_matches(ctx, cond))
        {
            apply_ban_context(ctx, &rule);
            service
                .record_ban_hit(&[rule.id], Some(&ctx.user_info.email))
                .ok();
            if rule.cannot_access {
                return Err(ForumError::PermissionDenied("banned".into()));
            }
            return Ok(());
        }
    }
    ctx.session.remove("ban_active");
    ctx.session.remove("ban_rule");
    ctx.session.remove("ban_reason");
    ctx.session.remove("ban_cannot_post");
    ctx.session.remove("ban_cannot_access");
    Ok(())
}

fn apply_changes(
    changes: &[PermissionChange],
    grants: &mut Vec<String>,
    removals: &mut Vec<String>,
) {
    for change in changes {
        if change.allow {
            grants.push(change.permission.clone());
        } else {
            removals.push(change.permission.clone());
        }
    }
}

pub fn ban_permissions(ctx: &mut ForumContext) {
    if ctx.session.bool("ban_cannot_access") {
        ctx.user_info.permissions.clear();
        return;
    }
    if ctx.session.bool("ban_cannot_post")
        || ctx
            .mod_settings
            .int("warning_mute")
            .map(|limit| ctx.user_info.warning as i64 >= limit)
            .unwrap_or(false)
    {
        remove_permissions(
            ctx,
            &[
                "pm_send",
                "calendar_post",
                "poll_post",
                "post_new",
                "post_reply_any",
            ],
        );
        return;
    }
    if let Some(limit) = ctx.mod_settings.int("warning_moderate") {
        if ctx.user_info.warning as i64 >= limit {
            if ctx.user_info.permissions.remove("post_new") {
                ctx.user_info
                    .permissions
                    .insert("post_unapproved_topics".into());
            }
            if ctx.user_info.permissions.remove("post_reply_any") {
                ctx.user_info
                    .permissions
                    .insert("post_unapproved_replies_any".into());
            }
            if ctx.user_info.permissions.remove("post_reply_own") {
                ctx.user_info
                    .permissions
                    .insert("post_unapproved_replies_own".into());
            }
        }
    }
}

fn remove_permissions(ctx: &mut ForumContext, perms: &[&str]) {
    for perm in perms {
        ctx.user_info.permissions.remove(*perm);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{ForumContext, InMemoryService};

    #[test]
    fn load_permissions_populates_set() {
        let service = InMemoryService::new_with_sample();
        let mut ctx = ForumContext::default();
        ctx.user_info.groups = vec![0];
        load_permissions(&service, &mut ctx, Some("1".into())).unwrap();
        assert!(ctx.user_info.permissions.contains("post_reply_any"));
    }

    #[test]
    fn ban_removes_permissions() {
        let service = InMemoryService::new_with_sample();
        let mut ctx = ForumContext::default();
        ctx.user_info.groups = vec![0];
        load_permissions(&service, &mut ctx, None).unwrap();
        ctx.session.set("ban_cannot_post", true);
        ban_permissions(&mut ctx);
        assert!(!ctx.user_info.permissions.contains("post_new"));
    }

    #[test]
    fn banned_user_is_blocked() {
        let service = InMemoryService::new_with_sample();
        let _ = service.save_ban_rule(BanRule {
            id: 0,
            reason: Some("test ban".into()),
            expires_at: None,
            cannot_post: true,
            cannot_access: true,
            conditions: vec![BanCondition {
                id: 0,
                reason: Some("banned email".into()),
                expires_at: None,
                affects: crate::services::BanAffects::Email {
                    value: "blocked@example.com".into(),
                },
            }],
        });
        let mut ctx = ForumContext::default();
        ctx.user_info.email = "blocked@example.com".into();
        let result = is_not_banned(&service, &mut ctx, true);
        assert!(result.is_err());
    }
}
