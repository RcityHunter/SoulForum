use crate::services::{
    BanAffects, BanCondition, BanRule, ForumContext, ForumError, ForumService, ServiceResult,
};
use serde_json::json;

pub fn list_members<S: ForumService>(service: &S, ctx: &mut ForumContext) -> ServiceResult<()> {
    ensure_permission(ctx, "moderate_forum")?;
    let members = service.list_members()?;
    ctx.context.set(
        "member_list",
        members
            .into_iter()
            .map(|member| {
                json!({
                    "id": member.id,
                    "name": member.name,
                    "primary_group": member.primary_group,
                    "warning": member.warning,
                    "send_pm": crate::personal_messages::pm_link(ctx, member.id),
                })
            })
            .collect::<Vec<_>>(),
    );
    Ok(())
}

pub fn ban_members<S: ForumService>(
    service: &S,
    ctx: &ForumContext,
    member_ids: &[i64],
    reason: Option<String>,
) -> ServiceResult<i64> {
    ensure_permission(ctx, "manage_bans")?;
    if member_ids.is_empty() {
        return Err(ForumError::Validation("no_members".into()));
    }
    let conditions = member_ids
        .iter()
        .map(|member_id| BanCondition {
            id: 0,
            reason: reason.clone(),
            expires_at: None,
            affects: BanAffects::Account {
                member_id: *member_id,
            },
        })
        .collect();
    let rule_id = service.save_ban_rule(BanRule {
        id: 0,
        reason,
        expires_at: None,
        cannot_post: true,
        cannot_access: false,
        conditions,
    })?;
    service.log_action(
        "ban_members",
        Some(ctx.user_info.id),
        &json!({ "rule_id": rule_id, "members": member_ids }),
    )?;
    Ok(rule_id)
}

pub fn unban_rule<S: ForumService>(
    service: &S,
    ctx: &ForumContext,
    rule_id: i64,
) -> ServiceResult<()> {
    ensure_permission(ctx, "manage_bans")?;
    service.delete_ban_rule(rule_id)
}

fn ensure_permission(ctx: &ForumContext, permission: &str) -> ServiceResult<()> {
    if ctx.user_info.permissions.contains(permission) {
        Ok(())
    } else {
        Err(ForumError::PermissionDenied(permission.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{ForumContext, InMemoryService};

    #[test]
    fn members_listed() {
        let service = InMemoryService::default();
        let mut ctx = ForumContext::default();
        ctx.user_info.permissions.insert("moderate_forum".into());
        list_members(&service, &mut ctx).unwrap();
        assert!(ctx.context.get("member_list").is_some());
    }

    #[test]
    fn bulk_ban_creates_rule() {
        let service = InMemoryService::default();
        let mut ctx = ForumContext::default();
        ctx.user_info.permissions.insert("manage_bans".into());
        let rule_id = ban_members(&service, &ctx, &[2, 3], Some("Test".into())).unwrap();
        assert!(rule_id > 0);
    }
}
