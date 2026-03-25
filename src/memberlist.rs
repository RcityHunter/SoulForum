use crate::personal_messages::pm_link;
use crate::services::{ForumContext, ForumService, ServiceResult};
use serde_json::json;

pub fn show_memberlist<S: ForumService>(service: &S, ctx: &mut ForumContext) -> ServiceResult<()> {
    let members = service.list_members()?;
    let list: Vec<_> = members
        .into_iter()
        .map(|member| {
            json!({
                "id": member.id,
                "name": member.name,
                "send_pm": pm_link(ctx, member.id),
            })
        })
        .collect();
    ctx.context.set("memberlist", list);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{ForumContext, InMemoryService};

    #[test]
    fn memberlist_includes_pm_link() {
        let service = InMemoryService::default();
        let mut ctx = ForumContext {
            scripturl: "https://forum.local".into(),
            ..Default::default()
        };
        ctx.user_info.id = 99;
        ctx.user_info.is_guest = false;
        ctx.user_info.permissions.insert("pm_send".into());
        show_memberlist(&service, &mut ctx).unwrap();
        assert!(ctx.context.get("memberlist").is_some());
    }
}
