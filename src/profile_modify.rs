use crate::services::{
    ensure, ForumContext, ForumError, ForumService, MembergroupData, ServiceResult,
};
use serde_json::json;
use std::collections::HashSet;

pub struct ProfileController<S: ForumService + Clone> {
    service: S,
}

impl<S: ForumService + Clone> ProfileController<S> {
    pub fn new(service: S) -> Self {
        Self { service }
    }

    pub fn group_membership(&self, ctx: &mut ForumContext, member_id: i64) -> ServiceResult<()> {
        ensure(
            self.service
                .allowed_to(ctx, "manage_membergroups", None, false),
            ForumError::PermissionDenied("manage_membergroups".into()),
        )?;
        let member = self
            .service
            .get_member_record(member_id)?
            .ok_or_else(|| ForumError::Validation("member_not_found".into()))?;
        let groups = self.service.list_all_membergroups()?;
        let mut member_ids: HashSet<i64> = member.additional_groups.iter().copied().collect();
        if let Some(primary) = member.primary_group {
            member_ids.insert(primary);
        }
        ctx.context.set("primary_group", member.primary_group);

        let mut owned = Vec::new();
        let mut available = Vec::new();
        for group in groups {
            let group_id = group.id.unwrap_or(0);
            let is_member = member_ids.contains(&group_id);
            let entry = build_group_entry(&group, member.primary_group, is_member);
            if is_member {
                owned.push(entry);
            } else {
                available.push(entry);
            }
        }

        owned.push(json!({
            "id": 0,
            "name": "Regular Members",
            "description": "Default group",
            "is_primary": member.primary_group.is_none(),
            "can_be_primary": true,
            "can_leave": false,
        }));

        ctx.context.set("groups_member", owned);
        ctx.context.set("groups_available", available);
        Ok(())
    }

    pub fn update_group_membership(
        &self,
        ctx: &mut ForumContext,
        member_id: i64,
    ) -> ServiceResult<()> {
        ensure(
            self.service
                .allowed_to(ctx, "manage_membergroups", None, false),
            ForumError::PermissionDenied("manage_membergroups".into()),
        )?;
        let mut record = self
            .service
            .get_member_record(member_id)?
            .ok_or_else(|| ForumError::Validation("member_not_found".into()))?;
        let group_id = ctx
            .request
            .int("group_id")
            .ok_or_else(|| ForumError::Validation("missing_group".into()))?;
        let action = ctx.request.string("action").unwrap_or_else(|| "add".into());
        let groups = self.service.list_all_membergroups()?;
        let target = groups
            .into_iter()
            .find(|group| group.id == Some(group_id))
            .ok_or_else(|| ForumError::Validation("group_not_found".into()))?;

        if target.is_protected && !self.service.allowed_to(ctx, "admin_forum", None, false) {
            return Err(ForumError::PermissionDenied("admin_forum".into()));
        }

        match action.as_str() {
            "set_primary" => {
                record.primary_group = Some(group_id);
                record.additional_groups.retain(|gid| gid != &group_id);
            }
            "clear_primary" => {
                if record.primary_group == Some(group_id) {
                    record.primary_group = None;
                }
            }
            "remove" => {
                if record.primary_group == Some(group_id) {
                    record.primary_group = None;
                }
                record.additional_groups.retain(|gid| gid != &group_id);
            }
            _ => {
                if record.primary_group.is_none() {
                    record.primary_group = Some(group_id);
                } else if !record.additional_groups.contains(&group_id) {
                    record.additional_groups.push(group_id);
                }
            }
        }

        self.service.update_member_groups(
            member_id,
            record.primary_group,
            &record.additional_groups,
        )
    }
}

fn build_group_entry(
    group: &MembergroupData,
    primary_group: Option<i64>,
    is_member: bool,
) -> serde_json::Value {
    let id = group.id.unwrap_or(0);
    json!({
        "id": id,
        "name": group.name.clone(),
        "description": group.description.clone(),
        "color": group.color.clone(),
        "type": group.group_type,
        "is_primary": Some(id) == primary_group,
        "can_be_primary": !group.hidden,
        "can_leave": group.id != Some(1) && group.group_type > 1,
        "is_member": is_member,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{ForumContext, InMemoryService};

    #[test]
    fn membership_lists_groups() {
        let service = InMemoryService::default();
        let controller = ProfileController::new(service);
        let mut ctx = ForumContext::default();
        ctx.user_info
            .permissions
            .insert("manage_membergroups".into());
        controller.group_membership(&mut ctx, 2).unwrap();
        assert!(ctx.context.get("groups_member").is_some());
    }

    #[test]
    fn update_membership_changes_primary() {
        let service = InMemoryService::default();
        let controller = ProfileController::new(service.clone());
        let mut ctx = ForumContext::default();
        ctx.user_info
            .permissions
            .insert("manage_membergroups".into());
        ctx.request.set("group_id", 3);
        ctx.request.set("action", "set_primary");
        controller.update_group_membership(&mut ctx, 3).unwrap();
        let member = service.get_member_record(3).unwrap().unwrap();
        assert_eq!(member.primary_group, Some(3));
    }
}
