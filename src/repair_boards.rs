use crate::services::{ForumContext, ForumService, ServiceResult};

pub fn repair_personal_messages<S: ForumService>(
    service: &S,
    ctx: &mut ForumContext,
) -> ServiceResult<()> {
    let repaired = service.repair_pm_data()?;
    ctx.context.set("pm_repair_count", repaired as i64);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pm_ops::{self, RecipientInput};
    use crate::services::{ForumContext, InMemoryService};

    #[test]
    fn repair_counts_orphans() {
        let service = InMemoryService::default();
        let mut ctx = ForumContext::default();
        ctx.user_info.id = 1;
        ctx.user_info.name = "Alice".into();
        ctx.user_info.permissions.insert("pm_send".into());
        pm_ops::send_pm(
            &service,
            &ctx,
            RecipientInput {
                to: vec!["2".into()],
                bcc: vec![],
            },
            "Hi",
            "Body",
        )
        .unwrap();
        service.cleanup_pm_ignore_lists(&[2]).unwrap();
        repair_personal_messages(&service, &mut ctx).unwrap();
        assert!(ctx.context.get("pm_repair_count").is_some());
    }
}
