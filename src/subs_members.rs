use crate::services::{ForumContext, ForumService, ServiceResult};

pub fn cleanup_after_member_removal<S: ForumService>(
    service: &S,
    _ctx: &ForumContext,
    member_ids: &[i64],
) -> ServiceResult<()> {
    if member_ids.is_empty() {
        return Ok(());
    }
    service.cleanup_pm_recipients(member_ids)?;
    service.cleanup_pm_ignore_lists(member_ids)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pm_ops::{self, RecipientInput};
    use crate::services::{ForumContext, InMemoryService};

    #[test]
    fn cleanup_removes_recipients() {
        let service = InMemoryService::default();
        let mut ctx = ForumContext::default();
        ctx.user_info.id = 1;
        ctx.user_info.name = "Alice".into();
        ctx.user_info.permissions.insert("pm_send".into());
        let log = pm_ops::send_pm(
            &service,
            &ctx,
            RecipientInput {
                to: vec!["2".into()],
                bcc: vec![],
            },
            "Hello",
            "Body",
        )
        .unwrap();
        let cleanup_ctx = ForumContext::default();
        cleanup_after_member_removal(&service, &cleanup_ctx, &[2]).unwrap();
        assert!(service
            .personal_message_detail(2, log.message_id.unwrap())
            .unwrap()
            .is_none());
    }

    #[test]
    fn cleanup_updates_ignore_list() {
        let service = InMemoryService::default();
        let ctx = ForumContext::default();
        service.set_pm_ignore_list(1, &[2, 3]).unwrap();
        cleanup_after_member_removal(&service, &ctx, &[3]).unwrap();
        let ignore = service.get_pm_ignore_list(1).unwrap();
        assert_eq!(ignore, vec![2]);
    }
}
