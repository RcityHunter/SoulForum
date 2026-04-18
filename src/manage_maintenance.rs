use crate::services::{ForumContext, ForumService, ServiceResult};

pub fn cleanup_personal_messages<S: ForumService>(
    service: &S,
    ctx: &mut ForumContext,
) -> ServiceResult<()> {
    let repaired = service.repair_pm_data()?;
    ctx.context.set("pm_repaired", repaired as i64);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pm_ops::{self, RecipientInput};
    use crate::services::{ForumContext, InMemoryService};

    #[test]
    fn cleanup_reports_count() {
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
        service.cleanup_pm_recipients(&[2]).unwrap();
        cleanup_personal_messages(&service, &mut ctx).unwrap();
        assert!(ctx.context.get("pm_repaired").is_some());
    }
}
