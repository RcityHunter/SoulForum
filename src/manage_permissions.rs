use crate::services::{
    ensure, ForumContext, ForumError, ForumService, PermissionScope, ServiceResult,
};
use serde_json::json;
use std::collections::{BTreeMap, HashMap};

const MEMBERGROUP_SECTIONS: &[&str] = &[
    "general",
    "pm",
    "calendar",
    "maintenance",
    "member_admin",
    "profile",
    "likes",
    "mentions",
    "bbc",
    "profile_account",
];

const BOARD_SECTIONS: &[&str] = &[
    "general_board",
    "topic",
    "post",
    "poll",
    "notification",
    "attachment",
];

const LEFT_PERMISSION_GROUPS: &[&str] = &[
    "general",
    "calendar",
    "maintenance",
    "member_admin",
    "topic",
    "post",
];

pub struct ManagePermissionsController<S: ForumService> {
    service: S,
}

impl<S: ForumService> ManagePermissionsController<S> {
    pub fn new(service: S) -> Self {
        Self { service }
    }

    pub fn modify_permissions(&self, ctx: &mut ForumContext) -> ServiceResult<()> {
        self.service.load_language(ctx, "ManagePermissions")?;
        self.service.load_language(ctx, "ManageMembers")?;
        self.service.load_template(ctx, "ManagePermissions")?;

        ensure(
            self.service
                .allowed_to(ctx, "manage_permissions", None, false),
            ForumError::PermissionDenied("manage_permissions".into()),
        )?;

        let subaction = ctx.request.string("sa").unwrap_or_else(|| "index".into());
        match subaction.as_str() {
            "index" => self.permission_index(ctx),
            _ => self.permission_index(ctx),
        }
    }

    fn permission_index(&self, ctx: &mut ForumContext) -> ServiceResult<()> {
        ctx.context.set("page_title", "Permissions");
        load_all_permissions(ctx)?;
        let profiles = self.service.permission_profiles()?;
        ctx.context.set("permission_profiles", profiles);
        let show_advanced = !ctx.context.bool("admin_hide_advanced");
        ctx.context.set("show_advanced_options", show_advanced);

        let groups = self.service.permission_groups()?;
        let scripturl = ctx.scripturl.clone();
        let group_json: Vec<_> = groups
            .into_iter()
            .map(|group| {
                json!({
                    "id": group.id,
                    "name": group.name,
                    "num_members": group.num_members,
                    "allow_delete": group.allow_delete,
                    "allow_modify": group.allow_modify,
                    "can_search": group.can_search,
                    "help": group.help,
                    "is_post_group": group.is_post_group,
                    "color": group.color,
                    "icons": group.icons,
                    "children": group
                        .children
                        .iter()
                        .map(|child| json!({"id": child.id, "name": child.name}))
                        .collect::<Vec<_>>(),
                    "num_permissions": {
                        "allowed": group.allowed,
                        "denied": group.denied,
                    },
                    "access": group.access,
                    "href": group.link.map(|link| format!("{}{}", scripturl, link)),
                })
            })
            .collect();
        ctx.context.set("groups", group_json);
        Ok(())
    }
}

struct PermissionSpec {
    id: String,
    scope: PermissionScope,
    section: &'static str,
    has_options: bool,
}

fn base_permission_specs() -> Vec<PermissionSpec> {
    let mut specs = Vec::new();
    let member_specs = [
        ("view_stats", false, "general"),
        ("view_mlist", false, "general"),
        ("who_view", false, "general"),
        ("search_posts", false, "general"),
        ("pm_read", false, "pm"),
        ("pm_send", false, "pm"),
        ("pm_draft", false, "pm"),
        ("calendar_view", false, "calendar"),
        ("calendar_post", false, "calendar"),
        ("calendar_edit", true, "calendar"),
        ("admin_forum", false, "maintenance"),
        ("manage_boards", false, "maintenance"),
        ("manage_attachments", false, "maintenance"),
        ("manage_smileys", false, "maintenance"),
        ("edit_news", false, "maintenance"),
        ("access_mod_center", false, "maintenance"),
        ("moderate_forum", false, "member_admin"),
        ("manage_membergroups", false, "member_admin"),
        ("manage_permissions", false, "member_admin"),
        ("manage_bans", false, "member_admin"),
        ("send_mail", false, "member_admin"),
        ("issue_warning", false, "member_admin"),
        ("profile_view", false, "profile"),
        ("profile_forum", true, "profile"),
        ("profile_extra", true, "profile"),
        ("profile_signature", true, "profile"),
        ("profile_website", true, "profile"),
        ("profile_title", true, "profile"),
        ("profile_blurb", true, "profile"),
        ("profile_server_avatar", false, "profile"),
        ("profile_upload_avatar", false, "profile"),
        ("profile_remote_avatar", false, "profile"),
        ("report_user", false, "profile"),
        ("profile_identity", true, "profile_account"),
        ("profile_displayed_name", true, "profile_account"),
        ("profile_password", true, "profile_account"),
        ("profile_remove", true, "profile_account"),
        ("view_warning", true, "profile_account"),
        ("likes_like", false, "likes"),
        ("mention", false, "mentions"),
    ];
    for (id, has_options, section) in member_specs {
        specs.push(PermissionSpec {
            id: id.to_string(),
            scope: PermissionScope::Membergroup,
            section,
            has_options,
        });
    }
    let board_specs = [
        ("moderate_board", false, "general_board"),
        ("approve_posts", false, "general_board"),
        ("post_new", false, "topic"),
        ("post_unapproved_topics", false, "topic"),
        ("post_reply", true, "topic"),
        ("post_unapproved_replies", true, "topic"),
        ("post_draft", false, "topic"),
        ("merge_any", false, "topic"),
        ("split_any", false, "topic"),
        ("make_sticky", false, "topic"),
        ("move", true, "topic"),
        ("lock", true, "topic"),
        ("remove", true, "topic"),
        ("modify_replies", false, "topic"),
        ("delete_replies", false, "topic"),
        ("announce_topic", false, "topic"),
        ("delete", true, "post"),
        ("modify", true, "post"),
        ("report_any", false, "post"),
        ("poll_view", false, "poll"),
        ("poll_vote", false, "poll"),
        ("poll_post", false, "poll"),
        ("poll_add", true, "poll"),
        ("poll_edit", true, "poll"),
        ("poll_lock", true, "poll"),
        ("poll_remove", true, "poll"),
        ("view_attachments", false, "attachment"),
        ("post_unapproved_attachments", false, "attachment"),
        ("post_attachment", false, "attachment"),
    ];
    for (id, has_options, section) in board_specs {
        specs.push(PermissionSpec {
            id: id.to_string(),
            scope: PermissionScope::Board,
            section,
            has_options,
        });
    }
    specs
}

fn load_all_permissions(ctx: &mut ForumContext) -> ServiceResult<()> {
    let mut specs = base_permission_specs();
    let restricted = ctx
        .context
        .get("restricted_bbc")
        .and_then(|value| value.as_array().cloned())
        .map(|vals| {
            vals.into_iter()
                .filter_map(|val| val.as_str().map(|s| s.to_string()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| vec!["html".into()]);
    for tag in restricted {
        specs.push(PermissionSpec {
            id: format!("bbc_{}", tag),
            scope: PermissionScope::Membergroup,
            section: "bbc",
            has_options: false,
        });
    }

    let hidden = determine_hidden_permissions(ctx);
    let relabels = determine_relabels(ctx);

    let mut grouped: HashMap<PermissionScope, BTreeMap<String, Vec<_>>> = HashMap::new();
    for spec in specs
        .into_iter()
        .filter(|spec| !hidden.contains(&spec.id.as_str()))
    {
        let label = relabels
            .get(spec.id.as_str())
            .cloned()
            .unwrap_or_else(|| spec.id.clone());
        grouped
            .entry(spec.scope)
            .or_default()
            .entry(spec.section.to_string())
            .or_default()
            .push(json!({
                "id": spec.id,
                "label": label,
                "has_options": spec.has_options,
            }));
    }

    ctx.context.set(
        "permissions",
        json!({
            "membergroup": build_section_list(PermissionScope::Membergroup, &grouped),
            "board": build_section_list(PermissionScope::Board, &grouped),
        }),
    );
    ctx.context
        .set("left_permission_groups", LEFT_PERMISSION_GROUPS);
    Ok(())
}

fn determine_hidden_permissions(ctx: &ForumContext) -> Vec<&'static str> {
    let mut hidden = Vec::new();
    if !ctx.mod_settings.bool("cal_enabled") {
        hidden.extend(["calendar_view", "calendar_post", "calendar_edit"]);
    }
    if !ctx.mod_settings.bool("warning_enabled") {
        hidden.extend(["issue_warning", "view_warning"]);
    }
    if !ctx.mod_settings.bool("postmod_active") {
        hidden.extend([
            "approve_posts",
            "post_unapproved_topics",
            "post_unapproved_replies",
            "post_unapproved_attachments",
        ]);
    }
    if !ctx.mod_settings.bool("attachmentEnable") {
        hidden.extend([
            "manage_attachments",
            "view_attachments",
            "post_unapproved_attachments",
            "post_attachment",
        ]);
    }
    hidden
}

fn determine_relabels(ctx: &ForumContext) -> HashMap<&'static str, String> {
    let mut relabels = HashMap::new();
    if ctx.mod_settings.bool("postmod_active") {
        relabels.insert("post_new", "auto_approve_topics".into());
        relabels.insert("post_reply", "auto_approve_replies".into());
        relabels.insert("post_attachment", "auto_approve_attachments".into());
    }
    relabels
}

fn build_section_list(
    scope: PermissionScope,
    grouped: &HashMap<PermissionScope, BTreeMap<String, Vec<serde_json::Value>>>,
) -> Vec<serde_json::Value> {
    let sections = match scope {
        PermissionScope::Membergroup => MEMBERGROUP_SECTIONS,
        PermissionScope::Board => BOARD_SECTIONS,
    };
    let mut list = Vec::new();
    for section in sections {
        let permissions = grouped
            .get(&scope)
            .and_then(|map| map.get(*section))
            .cloned()
            .unwrap_or_default();
        list.push(json!({
            "id": section,
            "permissions": permissions,
        }));
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{ForumContext, InMemoryService};

    #[test]
    fn index_populates_groups_and_permissions() {
        let service = InMemoryService::default();
        let controller = ManagePermissionsController::new(service);
        let mut ctx = ForumContext::default();
        ctx.user_info
            .permissions
            .insert("manage_permissions".into());
        controller.modify_permissions(&mut ctx).unwrap();
        assert!(ctx.context.get("permissions").is_some());
        assert!(ctx.context.get("groups").is_some());
    }

    #[test]
    fn hidden_permissions_respect_mod_settings() {
        let mut ctx = ForumContext::default();
        ctx.mod_settings.set("cal_enabled", false);
        load_all_permissions(&mut ctx).unwrap();
        let permissions = ctx.context.get("permissions").unwrap();
        let member_sections = permissions["membergroup"].as_array().unwrap();
        let calendar_section = member_sections
            .iter()
            .find(|section| section["id"] == "calendar")
            .unwrap();
        assert!(calendar_section["permissions"]
            .as_array()
            .unwrap()
            .is_empty());
    }
}
