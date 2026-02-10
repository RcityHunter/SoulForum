use btc_forum_rust::controller::post::PostController;
use btc_forum_rust::personal_messages::{ssi_welcome, PersonalMessageController};
use btc_forum_rust::services::{surreal::SurrealService, ForumContext};
use btc_forum_rust::surreal::connect_from_env;

fn main() {
    let rt = tokio::runtime::Runtime::new().expect("failed to create runtime");
    let surreal_client = rt
        .block_on(connect_from_env())
        .expect("failed to connect to SurrealDB");
    let service = SurrealService::new(surreal_client);

    let post_controller = PostController::new(service.clone());
    let pm_controller = PersonalMessageController::new(service.clone());

    let mut ctx = ForumContext::default();
    ctx.board_id = Some(1);
    ctx.topic_id = Some(1);
    ctx.user_info.is_guest = false;
    ctx.user_info.permissions.insert("post_new".into());
    ctx.post_vars.set("subject", "CLI example");
    ctx.post_vars
        .set("message", "Hello from Rust post controller");
    ctx.context.set("becomes_approved", true);

    if let Err(error) = post_controller.post(&mut ctx) {
        eprintln!("post() -> {error}");
    }
    if let Err(error) = post_controller.post2(&mut ctx) {
        eprintln!("post2() -> {error}");
    }
    if let Err(error) = post_controller.quote_fast(&mut ctx) {
        eprintln!("quote_fast() -> {error}");
    }
    if let Err(error) = post_controller.announce_topic(&mut ctx) {
        eprintln!("announce_topic() -> {error}");
    }

    let mut pm_ctx = ForumContext::default();
    pm_ctx.user_info.id = 2;
    pm_ctx.user_info.is_guest = false;
    pm_ctx
        .user_info
        .permissions
        .extend(["pm_read".into(), "pm_send".into()]);
    pm_ctx.scripturl = "https://forum.local".into();
    if let Err(error) = pm_controller.dispatch(&mut pm_ctx) {
        eprintln!("pm dispatch -> {error}");
    }
    if let Ok(message) = ssi_welcome(&service, &mut pm_ctx, true) {
        println!("{message}");
    }
}
