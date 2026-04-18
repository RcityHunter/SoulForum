use btc_forum_rust::services::{ForumContext, ForumService, InMemoryService, PostSubmission};

#[test]
fn post_submission_requires_subject_and_body() {
    let service = InMemoryService::new_with_sample();
    let mut ctx = ForumContext::default();
    ctx.user_info.is_guest = false;
    ctx.user_info.permissions.insert("post_new".into());

    let submission = PostSubmission {
        topic_id: None,
        board_id: 0,
        message_id: None,
        subject: String::new(),
        body: String::new(),
        icon: "xx".into(),
        approved: true,
        send_notifications: false,
    };

    let result = service.persist_post(&ctx, submission);
    assert!(result.is_err(), "empty subject/body should error");
}

#[test]
fn allowed_to_respects_permissions() {
    let service = InMemoryService::new_with_sample();
    let mut ctx = ForumContext::default();
    ctx.user_info.is_guest = false;
    ctx.user_info.permissions.insert("post_new".into());
    assert!(service.allowed_to(&ctx, "post_new", None, false));
    assert!(!service.allowed_to(&ctx, "admin_only", None, false));
}
