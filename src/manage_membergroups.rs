use crate::services::{
    ensure, BoardListOptions, ForumContext, ForumError, ForumService, MembergroupData,
    MembergroupSettings, ServiceResult, SessionCheckMode,
};
use serde_json::json;

pub struct MembergroupController<S: ForumService> {
    service: S,
}

impl<S: ForumService> MembergroupController<S> {
    pub fn new(service: S) -> Self {
        Self { service }
    }

    pub fn modify_membergroups(&self, ctx: &mut ForumContext) -> ServiceResult<()> {
        self.service.load_language(ctx, "ManageMembers")?;
        self.service.load_template(ctx, "ManageMembergroups")?;
        self.service
            .call_hook(ctx, "integrate_manage_membergroups")?;

        let can_manage = self
            .service
            .allowed_to(ctx, "manage_membergroups", None, false);
        let can_admin = self.service.allowed_to(ctx, "admin_forum", None, false);
        ensure(
            can_manage || can_admin,
            ForumError::PermissionDenied("manage_membergroups".into()),
        )?;

        let subaction = self.resolve_subaction(ctx, can_manage);
        match subaction.as_str() {
            "add" => {
                ensure(
                    can_manage,
                    ForumError::PermissionDenied("manage_membergroups".into()),
                )?;
                self.add_group(ctx)
            }
            "edit" => {
                ensure(
                    can_manage,
                    ForumError::PermissionDenied("manage_membergroups".into()),
                )?;
                self.edit_group(ctx)
            }
            "members" => {
                ensure(
                    can_manage,
                    ForumError::PermissionDenied("manage_membergroups".into()),
                )?;
                self.manage_members(ctx)
            }
            "settings" => {
                ensure(
                    can_admin,
                    ForumError::PermissionDenied("admin_forum".into()),
                )?;
                self.modify_settings(ctx)
            }
            _ => {
                ensure(
                    can_manage,
                    ForumError::PermissionDenied("manage_membergroups".into()),
                )?;
                self.index(ctx)
            }
        }
    }

    fn index(&self, ctx: &mut ForumContext) -> ServiceResult<()> {
        let groups = self.service.list_membergroups()?;
        let group_data: Vec<_> = groups
            .iter()
            .map(|group| {
                json!({
                    "id": group.id,
                    "name": group.name,
                    "num_members": group.num_members,
                    "color": group.color,
                    "is_post_group": group.is_post_group,
                })
            })
            .collect();
        ctx.context.set("membergroups", group_data);
        Ok(())
    }

    fn add_group(&self, ctx: &mut ForumContext) -> ServiceResult<()> {
        let mut current = if let Some(group_id) = ctx.request.int("group") {
            self.service.get_membergroup(group_id)?
        } else {
            None
        }
        .unwrap_or_default();
        if ctx.request.contains("save") {
            self.service.check_session(ctx, SessionCheckMode::Post)?;
            let saved = self
                .service
                .save_membergroup(self.parse_group_form(ctx, None)?)?;
            ctx.context.set("saved_group_id", saved);
            if let Some(latest) = self.service.get_membergroup(saved)? {
                current = latest;
            }
        }
        self.render_group_form(ctx, current, "add")
    }

    fn edit_group(&self, ctx: &mut ForumContext) -> ServiceResult<()> {
        let group_id = ctx
            .request
            .int("group")
            .ok_or_else(|| ForumError::Validation("missing_group".into()))?;
        if ctx.request.contains("save") {
            self.service.check_session(ctx, SessionCheckMode::Post)?;
            let payload = self.parse_group_form(ctx, Some(group_id))?;
            self.service.save_membergroup(payload)?;
            ctx.context.set("saved_group_id", group_id);
        }
        let details = self
            .service
            .get_membergroup(group_id)?
            .ok_or_else(|| ForumError::Validation("group_not_found".into()))?;
        self.render_group_form(ctx, details, "edit")
    }

    fn manage_members(&self, ctx: &mut ForumContext) -> ServiceResult<()> {
        let group_id = ctx
            .request
            .int("group")
            .ok_or_else(|| ForumError::Validation("missing_group".into()))?;
        if ctx.request.contains("remove") {
            if let Some(raw_list) = ctx.post_vars.string("remove_members") {
                let members = Self::parse_id_list(Some(raw_list));
                if !members.is_empty() {
                    self.service.check_session(ctx, SessionCheckMode::Post)?;
                    self.service.remove_members_from_group(group_id, &members)?;
                    ctx.context.set("removed_members", members.len() as i64);
                }
            }
        }
        let group = self
            .service
            .get_membergroup(group_id)?
            .ok_or_else(|| ForumError::Validation("group_not_found".into()))?;
        let members = self.service.list_group_members(group_id)?;
        ctx.context.set("membergroup", &group);
        ctx.context.set("group_members", members);
        ctx.context.set("group_id", group_id);
        Ok(())
    }

    fn modify_settings(&self, ctx: &mut ForumContext) -> ServiceResult<()> {
        if ctx.request.contains("save") {
            self.service.check_session(ctx, SessionCheckMode::Post)?;
            let show = ctx.post_vars.bool("show_group_key");
            let settings = MembergroupSettings {
                show_group_key: show,
            };
            self.service.save_membergroup_settings(settings)?;
            ctx.settings.set("show_group_key", show);
        }
        let current = self.service.get_membergroup_settings()?;
        ctx.context.set("show_group_key", current.show_group_key);
        Ok(())
    }

    fn render_group_form(
        &self,
        ctx: &mut ForumContext,
        group: MembergroupData,
        mode: &str,
    ) -> ServiceResult<()> {
        let MembergroupData {
            id,
            name,
            description,
            inherits_from,
            allowed_boards,
            color,
            is_post_group,
            ..
        } = group;
        let color_value = color.unwrap_or_default();
        ctx.context.set("group_mode", mode);
        ctx.context.set(
            "group_form",
            json!({
                "id": id.unwrap_or(0),
                "name": name,
                "description": description,
                "inherits_from": inherits_from,
                "color": color_value,
                "is_post_group": is_post_group,
            }),
        );
        ctx.context.set("selected_boards", allowed_boards.clone());
        let boards = self
            .service
            .get_board_list(ctx, &BoardListOptions::default())?;
        let board_list: Vec<_> = boards
            .iter()
            .map(|board| {
                json!({
                    "id": board.id,
                    "name": board.name,
                })
            })
            .collect();
        ctx.context.set("available_boards", board_list);
        Ok(())
    }

    fn parse_group_form(
        &self,
        ctx: &ForumContext,
        group_id: Option<i64>,
    ) -> ServiceResult<MembergroupData> {
        let raw_name = ctx.post_vars.string("group_name").unwrap_or_default();
        let name = raw_name.trim().to_string();
        ensure(
            !name.is_empty(),
            ForumError::Validation("group_name".into()),
        )?;
        let description = ctx
            .post_vars
            .string("group_description")
            .unwrap_or_default();
        let inherits_from = ctx.post_vars.int("inherit_id");
        let allowed_boards = Self::parse_id_list(ctx.post_vars.string("board_access"));
        let color = ctx.post_vars.string("color").and_then(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        });
        let is_post_group = ctx.post_vars.bool("is_post_group");
        let min_posts = ctx.post_vars.int("min_posts").unwrap_or(-1);
        let group_type = ctx.post_vars.int("group_type").unwrap_or(0) as i32;
        let hidden = ctx.post_vars.bool("hidden");
        let icons = ctx.post_vars.string("icons");
        let is_protected = ctx.post_vars.bool("is_protected");
        Ok(MembergroupData {
            id: group_id,
            name,
            description,
            inherits_from,
            allowed_boards,
            color,
            is_post_group,
            min_posts,
            group_type,
            hidden,
            icons,
            is_protected,
        })
    }

    fn parse_id_list(raw: Option<String>) -> Vec<i64> {
        raw.unwrap_or_default()
            .split([',', ';'])
            .filter_map(|part| part.trim().parse::<i64>().ok())
            .collect()
    }

    fn resolve_subaction(&self, ctx: &ForumContext, can_manage: bool) -> String {
        let requested = ctx.request.string("sa");
        if let Some(sub) = requested {
            match sub.as_str() {
                "index" | "add" | "edit" | "members" | "settings" => return sub,
                _ => {}
            }
        }
        if can_manage {
            "index".into()
        } else {
            "settings".into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{ForumContext, InMemoryService};

    fn build_controller() -> (MembergroupController<InMemoryService>, InMemoryService) {
        let service = InMemoryService::default();
        let controller = MembergroupController::new(service.clone());
        (controller, service)
    }

    #[test]
    fn index_lists_groups() {
        let (controller, _service) = build_controller();
        let mut ctx = ForumContext::default();
        ctx.user_info
            .permissions
            .insert("manage_membergroups".into());
        controller.modify_membergroups(&mut ctx).unwrap();
        assert!(ctx.context.get("membergroups").is_some());
    }

    #[test]
    fn add_group_creates_record() {
        let (controller, service) = build_controller();
        let mut ctx = ForumContext::default();
        ctx.user_info
            .permissions
            .insert("manage_membergroups".into());
        ctx.request.set("sa", "add");
        ctx.request.set("save", true);
        ctx.post_vars.set("group_name", "Testers");
        ctx.post_vars.set("group_description", "QA team");
        ctx.post_vars.set("inherit_id", 0);
        ctx.post_vars.set("board_access", "1");
        ctx.post_vars.set("color", "#00ff00");
        controller.modify_membergroups(&mut ctx).unwrap();
        let groups = service.list_membergroups().unwrap();
        assert!(groups.iter().any(|group| group.name == "Testers"));
    }

    #[test]
    fn edit_group_updates_record() {
        let (controller, service) = build_controller();
        let mut ctx = ForumContext::default();
        ctx.user_info
            .permissions
            .insert("manage_membergroups".into());
        ctx.request.set("sa", "edit");
        ctx.request.set("group", 1);
        ctx.request.set("save", true);
        ctx.post_vars.set("group_name", "Core Admins");
        ctx.post_vars.set("group_description", "Updated");
        ctx.post_vars.set("inherit_id", 0);
        ctx.post_vars.set("board_access", "1");
        controller.modify_membergroups(&mut ctx).unwrap();
        let updated = service.get_membergroup(1).unwrap().unwrap();
        assert_eq!(updated.name, "Core Admins");
    }

    #[test]
    fn members_removal_flow() {
        let (controller, service) = build_controller();
        let mut ctx = ForumContext::default();
        ctx.user_info
            .permissions
            .insert("manage_membergroups".into());
        ctx.request.set("sa", "members");
        ctx.request.set("group", 1);
        ctx.request.set("remove", true);
        ctx.post_vars.set("remove_members", "1");
        controller.modify_membergroups(&mut ctx).unwrap();
        assert!(service.list_group_members(1).unwrap().is_empty());
    }

    #[test]
    fn settings_toggle_updates_state() {
        let (controller, service) = build_controller();
        let mut ctx = ForumContext::default();
        ctx.user_info.permissions.insert("admin_forum".into());
        ctx.request.set("sa", "settings");
        ctx.request.set("save", true);
        ctx.post_vars.set("show_group_key", true);
        controller.modify_membergroups(&mut ctx).unwrap();
        let settings = service.get_membergroup_settings().unwrap();
        assert!(settings.show_group_key);
    }
}
