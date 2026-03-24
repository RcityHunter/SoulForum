use crate::profile_modify::ProfileController;
use crate::services::{
    ensure, ForumContext, ForumError, ForumService, PmPreferenceState, ServiceResult,
};

pub struct ProfileActions<S: ForumService + Clone> {
    service: S,
}

impl<S: ForumService + Clone> ProfileActions<S> {
    pub fn new(service: S) -> Self {
        Self { service }
    }

    pub fn show_group_membership(
        &self,
        ctx: &mut ForumContext,
        member_id: i64,
    ) -> ServiceResult<()> {
        let controller = ProfileController::new(self.service.clone());
        controller.group_membership(ctx, member_id)
    }

    pub fn handle_group_membership(
        &self,
        ctx: &mut ForumContext,
        member_id: i64,
    ) -> ServiceResult<()> {
        let controller = ProfileController::new(self.service.clone());
        controller.update_group_membership(ctx, member_id)
    }

    pub fn show_pm_settings(&self, ctx: &mut ForumContext, member_id: i64) -> ServiceResult<()> {
        ensure_can_edit(ctx, member_id)?;
        let prefs = self.service.get_pm_preferences(member_id)?;
        let ignore = self.service.get_pm_ignore_list(member_id)?;
        ctx.context.set("pm_receive_from", prefs.receive_from);
        ctx.context.set("pm_notify_level", prefs.notify_level);
        ctx.context.set("pm_ignore_list", ignore);
        Ok(())
    }

    pub fn save_pm_settings(&self, ctx: &mut ForumContext, member_id: i64) -> ServiceResult<()> {
        ensure_can_edit(ctx, member_id)?;
        let receive_from = ctx.post_vars.int("pm_receive_from").unwrap_or(0) as i32;
        let notify_level = ctx.post_vars.int("pm_notify").unwrap_or(0) as i32;
        let ignore_list = ctx
            .post_vars
            .string("pm_ignore_list")
            .unwrap_or_default()
            .split(',')
            .filter_map(|part| part.trim().parse::<i64>().ok())
            .collect::<Vec<_>>();
        self.service.save_pm_preferences(
            member_id,
            &PmPreferenceState {
                receive_from,
                notify_level,
            },
        )?;
        self.service.set_pm_ignore_list(member_id, &ignore_list)?;
        Ok(())
    }

    pub fn delete_account(&self, ctx: &mut ForumContext, member_id: i64) -> ServiceResult<()> {
        let is_owner = ctx.user_info.id == member_id;
        if !is_owner {
            ensure(
                self.service
                    .allowed_to(ctx, "profile_remove_any", None, false),
                ForumError::PermissionDenied("profile_remove_any".into()),
            )?;
        } else if !self
            .service
            .allowed_to(ctx, "profile_remove_any", None, false)
        {
            ensure(
                self.service
                    .allowed_to(ctx, "profile_remove_own", None, false),
                ForumError::PermissionDenied("profile_remove_own".into()),
            )?;
        }

        let can_delete_posts =
            !is_owner && self.service.allowed_to(ctx, "moderate_forum", None, false);
        ctx.context.set("can_delete_posts", can_delete_posts);
        let show_perma = ctx.mod_settings.bool("recycle_enable")
            && ctx.mod_settings.int("recycle_board").unwrap_or(0) > 0;
        ctx.context.set("show_perma_delete", show_perma);
        let needs_approval = is_owner
            && ctx.mod_settings.bool("approveAccountDeletion")
            && !self.service.allowed_to(ctx, "moderate_forum", None, false);
        ctx.context.set("needs_approval", needs_approval);
        ctx.context.set(
            "page_title",
            format!(
                "Delete Account: {}",
                ctx.context.string("member_name").unwrap_or_default()
            ),
        );
        Ok(())
    }

    pub fn delete_account_confirm(
        &self,
        ctx: &mut ForumContext,
        member_id: i64,
    ) -> ServiceResult<()> {
        let is_owner = ctx.user_info.id == member_id;
        if !is_owner {
            ensure(
                self.service
                    .allowed_to(ctx, "profile_remove_any", None, false),
                ForumError::PermissionDenied("profile_remove_any".into()),
            )?;
        } else if !self
            .service
            .allowed_to(ctx, "profile_remove_any", None, false)
        {
            ensure(
                self.service
                    .allowed_to(ctx, "profile_remove_own", None, false),
                ForumError::PermissionDenied("profile_remove_own".into()),
            )?;
        }

        let record = self
            .service
            .get_member_record(member_id)?
            .ok_or_else(|| ForumError::Validation("member_not_found".into()))?;

        let is_admin = record.primary_group == Some(1) || record.additional_groups.contains(&1);
        if is_admin {
            ensure(
                self.service.allowed_to(ctx, "admin_forum", None, false),
                ForumError::PermissionDenied("admin_forum".into()),
            )?;
            let others = self
                .service
                .list_members()?
                .into_iter()
                .filter(|member| member.id != member_id)
                .any(|member| {
                    member.primary_group == Some(1) || member.additional_groups.contains(&1)
                });
            ensure(others, ForumError::Validation("at_least_one_admin".into()))?;
        }

        self.service.delete_member(member_id)
    }
}

fn ensure_can_edit(ctx: &ForumContext, member_id: i64) -> ServiceResult<()> {
    if ctx.user_info.id == member_id || ctx.user_info.permissions.contains("profile_extra_any") {
        Ok(())
    } else {
        Err(ForumError::PermissionDenied("profile_extra".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{ForumContext, InMemoryService};

    #[test]
    fn delete_requires_permissions() {
        let service = InMemoryService::default();
        let controller = ProfileActions::new(service.clone());
        let mut ctx = ForumContext::default();
        ctx.user_info.id = 99;
        ctx.user_info
            .permissions
            .insert("profile_remove_any".into());
        controller.delete_account(&mut ctx, 2).unwrap();
        controller.delete_account_confirm(&mut ctx, 2).unwrap();
        assert!(service.get_member_record(2).unwrap().is_none());
    }

    #[test]
    fn cannot_remove_last_admin() {
        let service = InMemoryService::default();
        let controller = ProfileActions::new(service);
        let mut ctx = ForumContext::default();
        ctx.user_info.id = 1;
        ctx.user_info
            .permissions
            .insert("profile_remove_any".into());
        ctx.user_info.permissions.insert("admin_forum".into());
        let result = controller.delete_account_confirm(&mut ctx, 1);
        assert!(result.is_err());
    }
}
