use crate::services::{
    ensure, ForumContext, ForumError, ForumService, GroupAssignType, MembergroupListEntry,
    MembergroupListType, PermissionSnapshot, ServiceResult,
};
use std::collections::HashMap;

const PROTECTED_GROUPS: [i64; 5] = [-1, 0, 1, 3, 4];

pub fn delete_membergroups<S: ForumService>(
    ctx: &ForumContext,
    service: &S,
    groups: &[i64],
) -> ServiceResult<()> {
    ensure(
        service.allowed_to(ctx, "manage_membergroups", None, false),
        ForumError::PermissionDenied("manage_membergroups".into()),
    )?;
    ensure(
        !groups.is_empty(),
        ForumError::Validation("no_group_found".into()),
    )?;
    let mut deletable: Vec<i64> = groups
        .iter()
        .copied()
        .filter(|gid| !PROTECTED_GROUPS.contains(gid))
        .collect();
    deletable.sort_unstable();
    deletable.dedup();
    ensure(
        !deletable.is_empty(),
        ForumError::Validation("no_group_found".into()),
    )?;
    service.delete_membergroups(&deletable)
}

pub fn remove_members_from_groups<S: ForumService>(
    ctx: &ForumContext,
    service: &S,
    members: &[i64],
    groups: Option<&[i64]>,
    permission_checked: bool,
    ignore_protected: bool,
) -> ServiceResult<()> {
    if !permission_checked {
        ensure(
            service.allowed_to(ctx, "manage_membergroups", None, false),
            ForumError::PermissionDenied("manage_membergroups".into()),
        )?;
    }
    ensure(
        !members.is_empty(),
        ForumError::Validation("no_members".into()),
    )?;
    if let Some(list) = groups {
        let normalized = normalize_group_list(
            list,
            ignore_protected,
            service.allowed_to(ctx, "admin_forum", None, false),
        );
        ensure(
            !normalized.is_empty(),
            ForumError::Validation("no_group_found".into()),
        )?;
        service.remove_members_from_groups(members, Some(normalized.as_slice()))
    } else {
        service.remove_members_from_groups(members, None)
    }
}

pub fn add_members_to_group<S: ForumService>(
    ctx: &ForumContext,
    service: &S,
    members: &[i64],
    group: i64,
    assign_type: GroupAssignType,
    permission_checked: bool,
    ignore_protected: bool,
) -> ServiceResult<()> {
    if !permission_checked {
        ensure(
            service.allowed_to(ctx, "manage_membergroups", None, false),
            ForumError::PermissionDenied("manage_membergroups".into()),
        )?;
    }
    ensure(
        !members.is_empty(),
        ForumError::Validation("no_members".into()),
    )?;
    if !ignore_protected && !service.allowed_to(ctx, "admin_forum", None, false) {
        ensure(
            group != 1,
            ForumError::PermissionDenied("admin_forum".into()),
        )?;
    }
    service.add_members_to_group(members, group, assign_type)
}

pub fn list_get_membergroups<S: ForumService>(
    service: &S,
    membergroup_type: &str,
) -> ServiceResult<Vec<MembergroupListEntry>> {
    let kind = if membergroup_type == "post_count" {
        MembergroupListType::PostCount
    } else {
        MembergroupListType::Regular
    };
    service.list_membergroups_detailed(kind)
}

pub fn get_groups_with_permissions<S: ForumService>(
    service: &S,
    group_permissions: &[String],
    board_permissions: &[String],
    profile_id: i64,
) -> ServiceResult<HashMap<String, PermissionSnapshot>> {
    service.groups_with_permissions(group_permissions, board_permissions, profile_id)
}

fn normalize_group_list(groups: &[i64], ignore_protected: bool, is_admin: bool) -> Vec<i64> {
    let mut cleaned: Vec<i64> = groups.iter().copied().collect();
    cleaned.sort_unstable();
    cleaned.dedup();
    if ignore_protected || is_admin {
        cleaned
    } else {
        cleaned
            .into_iter()
            .filter(|gid| !PROTECTED_GROUPS.contains(gid))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{ForumContext, GroupAssignType, InMemoryService};

    #[test]
    fn delete_rejects_protected() {
        let service = InMemoryService::default();
        let ctx = ForumContext::default();
        let result = delete_membergroups(&ctx, &service, &[1, 2]);
        assert!(result.is_err());
    }

    #[test]
    fn remove_members_clears_groups() {
        let service = InMemoryService::default();
        let mut ctx = ForumContext::default();
        ctx.user_info
            .permissions
            .insert("manage_membergroups".into());
        ctx.user_info.permissions.insert("admin_forum".into());
        service
            .add_members_to_group(&[2], 1, GroupAssignType::ForcePrimary)
            .unwrap();
        remove_members_from_groups(&ctx, &service, &[2], Some(&[1]), true, false).unwrap();
        let members = service.list_group_members(1).unwrap();
        assert!(members.iter().all(|member| member.id != 2));
    }

    #[test]
    fn add_members_checks_permissions() {
        let service = InMemoryService::default();
        let mut ctx = ForumContext::default();
        ctx.user_info
            .permissions
            .insert("manage_membergroups".into());
        ctx.user_info.permissions.insert("admin_forum".into());
        add_members_to_group(&ctx, &service, &[3], 1, GroupAssignType::Auto, true, false).unwrap();
        let members = service.list_group_members(1).unwrap();
        assert!(members.iter().any(|member| member.id == 3));
    }

    #[test]
    fn list_returns_entries() {
        let service = InMemoryService::default();
        let entries = list_get_membergroups(&service, "regular").unwrap();
        assert!(!entries.is_empty());
    }

    #[test]
    fn permissions_map_populates() {
        let service = InMemoryService::default();
        let result =
            get_groups_with_permissions(&service, &["post_new".into()], &["poll_vote".into()], 1)
                .unwrap();
        assert!(result.contains_key("post_new"));
    }
}
