use dioxus::prelude::*;
use reqwasm::http::{Request, RequestCredentials};
use serde::{Deserialize, Serialize};
use serde::de::DeserializeOwned;
use web_sys::wasm_bindgen::JsCast;
use web_sys::HtmlDocument;

fn main() {
    launch(App);
}

// ---------- Types ----------
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
struct Board {
    id: Option<String>,
    name: String,
    description: Option<String>,
    created_at: Option<String>,
    updated_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
struct Topic {
    id: Option<String>,
    board_id: Option<String>,
    subject: String,
    author: String,
    created_at: Option<String>,
    updated_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
struct Post {
    id: Option<String>,
    topic_id: Option<String>,
    board_id: Option<String>,
    subject: String,
    body: String,
    author: String,
    created_at: Option<String>,
}

#[derive(Deserialize)]
struct BoardsResponse { status: String, boards: Vec<Board> }
#[derive(Deserialize)]
struct TopicsResponse { status: String, topics: Vec<Topic> }
#[derive(Deserialize)]
struct PostsResponse { status: String, posts: Vec<Post> }
#[derive(Deserialize)]
struct TopicCreateResponse { status: String, topic: Topic, first_post: Post }
#[derive(Deserialize)]
struct PostResponse { status: String, post: Post }

#[derive(Deserialize)]
struct AuthResponse { status: String, token: String, user: AuthUser }
#[derive(Deserialize)]
struct AuthUser { name: String, role: Option<String>, permissions: Option<Vec<String>> }
#[derive(Deserialize)]
struct RegisterResponse { status: String, message: String }

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
struct BoardAccessEntry { id: String, name: String, allowed_groups: Vec<i64> }
#[derive(Serialize)]
struct BoardAccessPayload { board_id: String, allowed_groups: Vec<i64> }
#[derive(Deserialize)]
struct BoardAccessResponse { status: String, entries: Vec<BoardAccessEntry> }
#[derive(Deserialize)]
struct UpdateBoardAccessResponse { status: String, board_id: String, allowed_groups: Vec<i64> }

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
struct BoardPermissionEntry { board_id: String, group_id: i64, allow: Vec<String>, deny: Vec<String> }
#[derive(Serialize)]
struct BoardPermissionPayload { board_id: String, group_id: i64, allow: Vec<String>, deny: Vec<String> }
#[derive(Deserialize)]
struct BoardPermissionResponse { status: String, entries: Vec<BoardPermissionEntry> }
#[derive(Deserialize)]
struct UpdateBoardPermissionResponse { status: String, board_id: String, group_id: i64, allow: Vec<String>, deny: Vec<String> }

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
struct Notification { id: String, user: String, subject: String, body: String, created_at: Option<String>, is_read: Option<bool> }
#[derive(Deserialize)]
struct NotificationListResponse { status: String, notifications: Vec<Notification> }
#[derive(Deserialize)]
struct NotificationCreateResponse { status: String, notification: Notification }

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
struct AttachmentMeta { id: Option<String>, filename: String, size_bytes: i64, mime_type: Option<String>, created_at: Option<String> }
#[derive(Deserialize)]
struct AttachmentListResponse { status: String, attachments: Vec<AttachmentMeta>, base_url: Option<String> }
#[derive(Deserialize)]
struct AttachmentCreateResponse { status: String, attachment: AttachmentMeta, base_url: Option<String>, url: Option<String> }
#[derive(Serialize)]
struct AttachmentDeletePayload { id: String }

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
struct PersonalMessagePeer { member_id: i64, name: String }
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
struct PersonalMessage { id: i64, subject: String, body: String, sender_id: i64, sender_name: String, sent_at: String, is_read: bool, recipients: Vec<PersonalMessagePeer> }
#[derive(Deserialize)]
struct PersonalMessageListResponse { status: String, messages: Vec<PersonalMessage>, total: usize, unread: usize }

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
struct BanRuleView { id: i64, expires_at: Option<String>, reason: Option<String> }
#[derive(Deserialize)]
struct BanListResponse { status: String, bans: Vec<BanRuleView> }

#[derive(Serialize)]
struct LoginPayload { email: String, password: String }
#[derive(Serialize)]
struct RegisterPayload { email: String, password: String }
#[derive(Serialize)]
struct CreateBoardPayload { name: String, description: Option<String> }
#[derive(Serialize)]
struct CreateTopicPayload { board_id: String, subject: String, body: String }
#[derive(Serialize)]
struct CreatePostPayload { topic_id: String, board_id: String, subject: Option<String>, body: String }

// ---------- Utilities ----------
fn window() -> Option<web_sys::Window> { web_sys::window() }
fn save_token_to_storage(token: &str) { if let Some(win) = window() { if let Ok(Some(storage)) = win.local_storage() { let _ = storage.set_item("jwt_token", token); } } }
fn load_token_from_storage() -> Option<String> { window().and_then(|win| win.local_storage().ok().flatten()).and_then(|s| s.get_item("jwt_token").ok().flatten()) }
fn set_csrf_cookie(token: &str) { if token.trim().is_empty() { return; } if let Some(win) = window() { if let Some(doc) = win.document() { if let Ok(html) = doc.dyn_into::<HtmlDocument>() { let _ = html.set_cookie(&format!("XSRF-TOKEN={}; Path=/", token)); } } } }

async fn get_json<T: DeserializeOwned>(base: &str, path: &str, token: &str, csrf: &str) -> Result<T, String> {
    let url = format!("{}/{}", base.trim_end_matches('/'), path.trim_start_matches('/'));
    let mut req = Request::get(&url);
    if !token.trim().is_empty() { req = req.header("Authorization", &format!("Bearer {}", token)); }
    if !csrf.trim().is_empty() {
        req = req.header("X-CSRF-TOKEN", csrf).header("Cookie", &format!("XSRF-TOKEN={}", csrf)).credentials(RequestCredentials::Include);
    }
    let resp = req.send().await.map_err(|e| format!("网络错误: {e}"))?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| format!("读取响应失败: {e}"))?;
    if !resp.ok() { return Err(format!("HTTP {status}: {text}")); }
    serde_json::from_str(&text).map_err(|e| format!("解析失败: {e}，原始响应: {text}"))
}

async fn post_json<T: DeserializeOwned, B: Serialize>(base: &str, path: &str, token: &str, csrf: &str, body: &B) -> Result<T, String> {
    let url = format!("{}/{}", base.trim_end_matches('/'), path.trim_start_matches('/'));
    let mut req = Request::post(&url);
    if !token.trim().is_empty() { req = req.header("Authorization", &format!("Bearer {}", token)); }
    if !csrf.trim().is_empty() {
        req = req.header("X-CSRF-TOKEN", csrf).header("Cookie", &format!("XSRF-TOKEN={}", csrf)).credentials(RequestCredentials::Include);
    }
    let resp = req.header("Content-Type", "application/json").body(serde_json::to_string(body).unwrap()).send().await.map_err(|e| format!("网络错误: {e}"))?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| format!("读取响应失败: {e}"))?;
    if !resp.ok() { return Err(format!("HTTP {status}: {text}")); }
    serde_json::from_str(&text).map_err(|e| format!("解析失败: {e}，原始响应: {text}"))
}

// ---------- App ----------
fn App() -> Element {
    // signals
    let mut api_base = use_signal(|| "http://127.0.0.1:3000".to_string());
    let mut token = use_signal(|| load_token_from_storage().unwrap_or_default());
    let mut status = use_signal(|| "等待操作...".to_string());
    let mut csrf_token = use_signal(|| "".to_string());
    let start_path = window().and_then(|win| win.location().pathname().ok()).unwrap_or_else(|| "/".to_string());
    let mut is_admin_page = use_signal(move || start_path.starts_with("/admin"));
    let mut login_username = use_signal(|| "".to_string());
    let mut login_password = use_signal(|| "".to_string());
    let mut register_username = use_signal(|| "".to_string());
    let mut register_password = use_signal(|| "".to_string());

    let boards = use_signal(Vec::<Board>::new);
    let mut topics = use_signal(Vec::<Topic>::new);
    let mut posts = use_signal(Vec::<Post>::new);
    let board_access = use_signal(Vec::<BoardAccessEntry>::new);
    let board_permissions = use_signal(Vec::<BoardPermissionEntry>::new);
    let notifications = use_signal(Vec::<Notification>::new);
    let attachments = use_signal(Vec::<AttachmentMeta>::new);
    let attachment_base_url = use_signal(|| "/uploads".to_string());
    let mut pm_folder = use_signal(|| "inbox".to_string());
    let personal_messages = use_signal(Vec::<PersonalMessage>::new);
    let pm_to = use_signal(|| "".to_string());
    let pm_subject = use_signal(|| "".to_string());
    let pm_body = use_signal(|| "".to_string());
    let selected_board = use_signal(|| "".to_string());
    let selected_topic = use_signal(|| "".to_string());
    let new_topic_subject = use_signal(|| "".to_string());
    let new_topic_body = use_signal(|| "".to_string());
    let new_post_subject = use_signal(|| "".to_string());
    let new_post_body = use_signal(|| "".to_string());
    let access_board_id = use_signal(|| "".to_string());
    let access_groups = use_signal(|| "".to_string());
    let perm_board_id = use_signal(|| "".to_string());
    let perm_group_id = use_signal(|| "".to_string());
    let perm_allow = use_signal(|| "".to_string());
    let perm_deny = use_signal(|| "".to_string());
    let ban_member_id = use_signal(|| "".to_string());
    let ban_hours = use_signal(|| "24".to_string());
    let ban_reason = use_signal(|| "".to_string());
    let bans = use_signal(Vec::<BanRuleView>::new);

    // actions (login/register etc.)
    let login = move || {
        let base = api_base.read().clone();
        let user = login_username.read().clone();
        let pass = login_password.read().clone();
        let mut status = status.clone();
        let mut token_sig = token.clone();
        if user.is_empty() || pass.is_empty() {
            status.set("请输入邮箱和密码".into());
            return;
        }
        spawn(async move {
            status.set("登录中...".into());
            let payload = LoginPayload { email: user.clone(), password: pass.clone() };
            match post_json::<AuthResponse, _>(&base, "/auth/login", "", "", &payload).await {
                Ok(resp) => {
                    save_token_to_storage(&resp.token);
                    token_sig.set(resp.token);
                    status.set(format!("已登录：{}", resp.user.name));
                }
                Err(err) => status.set(format!("登录失败：{err}")),
            }
        });
    };

    let register = move || {
        let base = api_base.read().clone();
        let user = register_username.read().clone();
        let pass = register_password.read().clone();
        let mut status = status.clone();
        if user.is_empty() || pass.is_empty() {
            status.set("请输入邮箱和密码".into());
            return;
        }
        spawn(async move {
            status.set("注册中...".into());
            let payload = RegisterPayload { email: user.clone(), password: pass.clone() };
            match post_json::<RegisterResponse, _>(&base, "/auth/register", "", "", &payload).await {
                Ok(resp) => {
                    status.set(resp.message);
                }
                Err(err) => status.set(format!("注册失败：{err}")),
            }
        });
    };

    // data loaders
    let load_boards = move || {
        let base = api_base.read().clone();
        let jwt = token.read().clone();
        let csrf = csrf_token.read().clone();
        let mut status = status.clone();
        let mut boards = boards.clone();
        let mut selected_board = selected_board.clone();
        spawn(async move {
            status.set("加载版块中...".into());
            match get_json::<BoardsResponse>(&base, "/surreal/boards", &jwt, &csrf).await {
                Ok(resp) => {
                    selected_board.set(resp.boards.get(0).and_then(|b| b.id.clone()).unwrap_or_default());
                    boards.set(resp.boards);
                    status.set("版块加载完成".into());
                }
                Err(err) => status.set(format!("加载版块失败：{err}")),
            }
        });
    };

    let check_health = move || {
        let base = api_base.read().clone();
        let mut status = status.clone();
        spawn(async move {
            status.set("健康检查中...".into());
            match get_json::<serde_json::Value>(&base, "/health", "", "").await {
                Ok(resp) => {
                    let service = resp.get("service").and_then(|v| v.as_str()).unwrap_or("unknown");
                    let surreal = resp
                        .get("surreal")
                        .and_then(|v| v.get("status"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    status.set(format!("健康检查: {service} / surreal: {surreal}"));
                }
                Err(err) => status.set(format!("健康检查失败：{err}")),
            }
        });
    };

    let load_topics = move || {
        let base = api_base.read().clone();
        let jwt = token.read().clone();
        let csrf = csrf_token.read().clone();
        let mut status = status.clone();
        let mut topics = topics.clone();
        let selected_board_id = selected_board.read().clone();
        let mut selected_topic = selected_topic.clone();
        if selected_board_id.is_empty() {
            status.set("请先选择版块".into());
            return;
        }
        spawn(async move {
            status.set("加载主题中...".into());
            let path = format!("/surreal/topics?board_id={}", selected_board_id);
            match get_json::<TopicsResponse>(&base, &path, &jwt, &csrf).await {
                Ok(resp) => {
                    if let Some(first) = resp.topics.get(0).and_then(|t| t.id.clone()) {
                        selected_topic.set(first);
                    }
                    topics.set(resp.topics);
                    status.set("主题加载完成".into());
                }
                Err(err) => status.set(format!("加载主题失败：{err}")),
            }
        });
    };

    let load_posts = move || {
        let base = api_base.read().clone();
        let jwt = token.read().clone();
        let csrf = csrf_token.read().clone();
        let mut status = status.clone();
        let mut posts = posts.clone();
        let topic_id = selected_topic.read().clone();
        if topic_id.is_empty() {
            status.set("请先选择主题".into());
            return;
        }
        spawn(async move {
            status.set("加载帖子中...".into());
            let path = format!("/surreal/topic/posts?topic_id={}", topic_id);
            match get_json::<PostsResponse>(&base, &path, &jwt, &csrf).await {
                Ok(resp) => {
                    posts.set(resp.posts);
                    status.set("帖子加载完成".into());
                }
                Err(err) => status.set(format!("加载帖子失败：{err}")),
            }
        });
    };

    let load_notifications = move || {
        let base = api_base.read().clone();
        let jwt = token.read().clone();
        let csrf = csrf_token.read().clone();
        let mut status = status.clone();
        let mut list = notifications.clone();
        if jwt.trim().is_empty() {
            status.set("请先登录再查看通知".into());
            return;
        }
        spawn(async move {
            status.set("加载通知中...".into());
            match get_json::<NotificationListResponse>(&base, "/surreal/notifications", &jwt, &csrf).await {
                Ok(resp) => {
                    list.set(resp.notifications);
                    status.set("通知加载完成".into());
                }
                Err(err) => status.set(format!("加载通知失败：{err}")),
            }
        });
    };

    let send_notification = move || {
        let base = api_base.read().clone();
        let jwt = token.read().clone();
        let csrf = csrf_token.read().clone();
        let mut status = status.clone();
        if jwt.trim().is_empty() {
            status.set("请先登录再发送通知".into());
            return;
        }
        spawn(async move {
            status.set("发送通知占位...".into());
            let payload = serde_json::json!({ "subject": "Hello", "body": "这是一条占位通知", "user": null });
            match post_json::<NotificationCreateResponse, _>(&base, "/surreal/notifications", &jwt, &csrf, &payload).await {
                Ok(resp) => status.set(format!("已创建通知 {}", resp.notification.subject)),
                Err(err) => status.set(format!("发送失败：{err}")),
            }
        });
    };

    let load_attachments = move || {
        let base = api_base.read().clone();
        let jwt = token.read().clone();
        let csrf = csrf_token.read().clone();
        let mut status = status.clone();
        let mut list = attachments.clone();
        let mut base_url_sig = attachment_base_url.clone();
        if jwt.trim().is_empty() {
            status.set("请先登录再查看附件".into());
            return;
        }
        spawn(async move {
            status.set("加载附件中...".into());
            match get_json::<AttachmentListResponse>(&base, "/surreal/attachments", &jwt, &csrf).await {
                Ok(resp) => {
                    if let Some(url) = resp.base_url { base_url_sig.set(url); }
                    list.set(resp.attachments);
                    status.set("附件加载完成".into());
                }
                Err(err) => status.set(format!("加载附件失败：{err}")),
            }
        });
    };

    let create_attachment = move || {
        let base = api_base.read().clone();
        let jwt = token.read().clone();
        let csrf = csrf_token.read().clone();
        let mut status = status.clone();
        let mut list = attachments.clone();
        let mut base_url_sig = attachment_base_url.clone();
        if jwt.trim().is_empty() {
            status.set("请先登录再创建附件".into());
            return;
        }
        spawn(async move {
            status.set("创建附件占位...".into());
            let payload = serde_json::json!({ "filename": "demo.txt", "size_bytes": 1234, "mime_type": "text/plain", "board_id": null, "topic_id": null });
            match post_json::<AttachmentCreateResponse, _>(&base, "/surreal/attachments", &jwt, &csrf, &payload).await {
                Ok(resp) => {
                    if let Some(url) = resp.base_url { base_url_sig.set(url); }
                    let mut current = list.read().clone();
                    current.insert(0, resp.attachment);
                    list.set(current);
                    status.set("附件元数据已创建（占位，不含文件）".into());
                }
                Err(err) => status.set(format!("创建失败：{err}")),
            }
        });
    };

    let load_pms = move || {
        let base = api_base.read().clone();
        let jwt = token.read().clone();
        let csrf = csrf_token.read().clone();
        let mut status = status.clone();
        let mut list = personal_messages.clone();
        let folder = pm_folder.read().clone();
        if jwt.trim().is_empty() { status.set("请先登录再查看私信".into()); return; }
        spawn(async move {
            status.set("加载私信中...".into());
            let path = format!("/surreal/personal_messages?folder={}", folder);
            match get_json::<PersonalMessageListResponse>(&base, &path, &jwt, &csrf).await {
                Ok(resp) => {
                    list.set(resp.messages);
                    status.set("私信加载完成".into());
                }
                Err(err) => status.set(format!("加载私信失败：{err}")),
            }
        });
    };

    let mark_pm_read = move |ids: Vec<i64>| {
        let base = api_base.read().clone();
        let jwt = token.read().clone();
        let csrf = csrf_token.read().clone();
        let mut status = status.clone();
        let mut list = personal_messages.clone();
        if jwt.trim().is_empty() || ids.is_empty() { return; }
        spawn(async move {
            let payload = serde_json::json!({ "ids": ids });
            match post_json::<serde_json::Value, _>(&base, "/surreal/personal_messages/read", &jwt, &csrf, &payload).await {
                Ok(_) => {
                    let mut current = list.read().clone();
                    for pm in current.iter_mut() {
                        if payload["ids"].as_array().unwrap().iter().any(|v| v.as_i64() == Some(pm.id)) {
                            pm.is_read = true;
                        }
                    }
                    list.set(current);
                    status.set("已标记已读".into());
                }
                Err(err) => status.set(format!("标记失败：{err}")),
            }
        });
    };

    let delete_pms = move |ids: Vec<i64>| {
        let base = api_base.read().clone();
        let jwt = token.read().clone();
        let csrf = csrf_token.read().clone();
        let mut status = status.clone();
        let mut list = personal_messages.clone();
        if jwt.trim().is_empty() || ids.is_empty() { return; }
        spawn(async move {
            let payload = serde_json::json!({ "ids": ids.clone() });
            match post_json::<serde_json::Value, _>(&base, "/surreal/personal_messages/delete", &jwt, &csrf, &payload).await {
                Ok(_) => {
                    let filtered: Vec<_> = list.read().iter().cloned().filter(|pm| !ids.contains(&pm.id)).collect();
                    list.set(filtered);
                    status.set("已删除所选私信".into());
                }
                Err(err) => status.set(format!("删除失败：{err}")),
            }
        });
    };

    let send_pm = move || {
        let base = api_base.read().clone();
        let jwt = token.read().clone();
        let csrf = csrf_token.read().clone();
        let mut status = status.clone();
        let to_raw = pm_to.read().clone();
        let subj = pm_subject.read().clone();
        let body = pm_body.read().clone();
        if jwt.trim().is_empty() { status.set("请先登录".into()); return; }
        if to_raw.trim().is_empty() || body.trim().is_empty() { status.set("请填写收件人和内容".into()); return; }
        let recipients: Vec<String> = to_raw.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        spawn(async move {
            status.set("发送私信中...".into());
            let payload = serde_json::json!({ "to": recipients, "subject": subj, "body": body });
            match post_json::<serde_json::Value, _>(&base, "/surreal/personal_messages/send", &jwt, &csrf, &payload).await {
                Ok(_) => status.set("私信已发送".into()),
                Err(err) => status.set(format!("发送失败：{err}")),
            }
        });
    };

    let load_access = move || {
        let base = api_base.read().clone();
        let jwt = token.read().clone();
        let csrf = csrf_token.read().clone();
        let mut status = status.clone();
        let mut access = board_access.clone();
        if jwt.trim().is_empty() { status.set("请先登录/粘贴管理员 JWT".into()); return; }
        spawn(async move {
            status.set("加载版块访问控制...".into());
            match get_json::<BoardAccessResponse>(&base, "/admin/board_access", &jwt, &csrf).await {
                Ok(resp) => {
                    access.set(resp.entries);
                    status.set("版块访问控制已加载".into());
                }
                Err(err) => status.set(format!("加载失败：{err}")),
            }
        });
    };

    let update_access = move || {
        let base = api_base.read().clone();
        let jwt = token.read().clone();
        let csrf = csrf_token.read().clone();
        let mut status = status.clone();
        let mut access = board_access.clone();
        let board_id = access_board_id.read().trim().to_string();
        let groups_raw = access_groups.read().clone();
        if jwt.trim().is_empty() { status.set("请先登录/粘贴管理员 JWT".into()); return; }
        if board_id.is_empty() { status.set("请输入有效的版块 ID".into()); return; }
        let mut groups = Vec::new();
        if !groups_raw.trim().is_empty() {
            for part in groups_raw.split(',') { if let Ok(id) = part.trim().parse::<i64>() { groups.push(id); } }
        }
        spawn(async move {
            status.set("更新版块访问控制...".into());
            let payload = BoardAccessPayload { board_id: board_id.clone(), allowed_groups: groups.clone() };
            match post_json::<UpdateBoardAccessResponse, _>(&base, "/admin/board_access", &jwt, &csrf, &payload).await {
                Ok(resp) => {
                    let mut current = access.read().clone();
                    if let Some(entry) = current.iter_mut().find(|e| e.id == resp.board_id) { entry.allowed_groups = resp.allowed_groups.clone(); }
                    access.set(current);
                    status.set("已更新".into());
                }
                Err(err) => status.set(format!("更新失败：{err}")),
            }
        });
    };

    let load_permissions = move || {
        let base = api_base.read().clone();
        let jwt = token.read().clone();
        let csrf = csrf_token.read().clone();
        let mut status = status.clone();
        let mut perms = board_permissions.clone();
        if jwt.trim().is_empty() { status.set("请先登录/粘贴管理员 JWT".into()); return; }
        spawn(async move {
            status.set("加载版块权限中...".into());
            match get_json::<BoardPermissionResponse>(&base, "/admin/board_permissions", &jwt, &csrf).await {
                Ok(resp) => { perms.set(resp.entries); status.set("版块权限已加载".into()); }
                Err(err) => status.set(format!("加载失败：{err}")),
            }
        });
    };

    let update_permissions = move || {
        let base = api_base.read().clone();
        let jwt = token.read().clone();
        let csrf = csrf_token.read().clone();
        let mut status = status.clone();
        let mut perms = board_permissions.clone();
        let board_id = perm_board_id.read().trim().to_string();
        let group_id = perm_group_id.read().trim().parse::<i64>().unwrap_or(0);
        let allow: Vec<String> = perm_allow.read().split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        let deny: Vec<String> = perm_deny.read().split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        if jwt.trim().is_empty() { status.set("请先登录/粘贴管理员 JWT".into()); return; }
        if board_id.is_empty() || group_id == 0 { status.set("请输入有效的 board_id 与 group_id".into()); return; }
        spawn(async move {
            status.set("更新版块权限...".into());
            let payload = BoardPermissionPayload { board_id: board_id.clone(), group_id, allow: allow.clone(), deny: deny.clone() };
            match post_json::<UpdateBoardPermissionResponse, _>(&base, "/admin/board_permissions", &jwt, &csrf, &payload).await {
                Ok(resp) => {
                    let mut current = perms.read().clone();
                    if let Some(entry) = current.iter_mut().find(|e| e.board_id == resp.board_id && e.group_id == resp.group_id) {
                        entry.allow = resp.allow.clone(); entry.deny = resp.deny.clone();
                    }
                    perms.set(current);
                    status.set("版块权限已更新".into());
                }
                Err(err) => status.set(format!("更新失败：{err}")),
            }
        });
    };

    let apply_ban = move || {
        let base = api_base.read().clone();
        let jwt = token.read().clone();
        let csrf = csrf_token.read().clone();
        let mut status = status.clone();
        let bans_sig = bans.clone();
        let member_id = ban_member_id.read().trim().parse::<i64>().unwrap_or(0);
        let hours = ban_hours.read().trim().parse::<i64>().unwrap_or(0);
        let reason = ban_reason.read().clone();
        if jwt.trim().is_empty() { status.set("请先登录/粘贴管理员 JWT".into()); return; }
        if member_id == 0 || hours <= 0 { status.set("请输入有效的 member_id 与时长".into()); return; }
        spawn(async move {
            status.set("封禁中...".into());
            let payload = serde_json::json!({ "member_id": member_id, "hours": hours, "reason": reason });
            match post_json::<serde_json::Value, _>(&base, "/admin/bans/apply", &jwt, &csrf, &payload).await {
                Ok(_) => { status.set("已封禁".into()); load_bans_inner(base, jwt, csrf, bans_sig.clone(), status.clone()).await; }
                Err(err) => status.set(format!("封禁失败：{err}")),
            }
        });
    };

    let revoke_ban = move |ban_id: i64| {
        let base = api_base.read().clone();
        let jwt = token.read().clone();
        let csrf = csrf_token.read().clone();
        let mut status = status.clone();
        let bans_sig = bans.clone();
        if jwt.trim().is_empty() { status.set("请先登录/粘贴管理员 JWT".into()); return; }
        spawn(async move {
            status.set("解除封禁中...".into());
            let payload = serde_json::json!({ "member_id": ban_id });
            match post_json::<serde_json::Value, _>(&base, "/admin/bans/revoke", &jwt, &csrf, &payload).await {
                Ok(_) => { status.set("已解除封禁".into()); load_bans_inner(base, jwt, csrf, bans_sig.clone(), status.clone()).await; }
                Err(err) => status.set(format!("解除失败：{err}")),
            }
        });
    };

    // helper to reload bans
    fn load_bans_inner(base: String, jwt: String, csrf: String, mut bans_sig: Signal<Vec<BanRuleView>>, mut status: Signal<String>) -> impl std::future::Future<Output=()> {
        async move {
            match get_json::<BanListResponse>(&base, "/admin/bans", &jwt, &csrf).await {
                Ok(resp) => { bans_sig.set(resp.bans); status.set("封禁列表已刷新".into()); }
                Err(err) => status.set(format!("刷新封禁失败：{err}")),
            }
        }
    }

    let load_bans = move || {
        let base = api_base.read().clone();
        let jwt = token.read().clone();
        let csrf = csrf_token.read().clone();
        let bans_sig = bans.clone();
        let status = status.clone();
        spawn(load_bans_inner(base, jwt, csrf, bans_sig, status.clone()));
    };

    let is_admin = *is_admin_page.read();

    rsx! {
        style { {STYLE} }
        div { class: "app-shell",
            nav { class: "top-nav",
                div { class: "brand",
                    span { class: "brand__dot" }
                    span { "BTC Forum" }
                    span { class: "brand__tag", "alpha" }
                }
                div { class: "nav-links",
                    a { class: if !is_admin { "nav-link active" } else { "nav-link" }, href: "/", onclick: move |_| { is_admin_page.set(false); }, "论坛" }
                    a { class: if is_admin { "nav-link active" } else { "nav-link" }, href: "/admin", onclick: move |_| { is_admin_page.set(true); }, "管理后台" }
                }
            }

            div { class: "status-bar", "状态：{status.read()}" }

            {if !is_admin { rsx! {
                section { class: "hero",
                    div { class: "hero__copy",
                        span { class: "pill", "Bitcoin Forum · Testnet" }
                        h1 { "比特币技术 & 社区实验室" }
                        p { "直连 SurrealDB 的论坛 Demo：注册、发帖、回帖与权限全部在这里自测。" }
                        div { class: "hero__actions",
                            button { onclick: move |_| load_boards(), "加载版块/主题" }
                            a { class: "ghost-btn", href: "/admin", "管理后台 (/admin)" }
                        }
                    }
                    div { class: "hero__panel",
                        div { class: "stat", span { "当前 API" } strong { "{api_base.read()}" } }
                        div { class: "stat-row",
                            div { class: "stat-box", strong { "{boards.read().len()}" } span { "版块" } }
                            div { class: "stat-box", strong { "{topics.read().len()}" } span { "主题" } }
                            div { class: "stat-box", strong { "{posts.read().len()}" } span { "帖子" } }
                        }
                    }
                }

                section { class: "panel",
                    h2 { "连接配置" }
                    div { class: "grid two",
                        div {
                            label { "API 基址" }
                            input { value: "{api_base.read()}", oninput: move |evt| api_base.set(evt.value()) }
                            div { class: "actions",
                                button { onclick: move |_| status.set("已更新 API 基址".into()), "更新" }
                                button { onclick: move |_| load_boards(), "加载数据" }
                                button { onclick: move |_| check_health(), "健康检查" }
                            }
                        }
                        div {
                            label { "JWT Token" }
                            textarea { value: "{token.read()}", rows: "3", oninput: move |evt| { token.set(evt.value()); save_token_to_storage(&evt.value()); } }
                            div { class: "actions",
                                button { onclick: move |_| { token.set("".into()); save_token_to_storage(""); status.set("已清空本地 token".into()); }, "清空 Token" }
                                button { onclick: move |_| { let csrf = format!("csrf-{}", js_sys::Date::now() as i64); csrf_token.set(csrf.clone()); set_csrf_cookie(&csrf); status.set("已刷新 CSRF".into()); }, "生成 CSRF" }
                            }
                        }
                    }
                    div { class: "grid two gap",
                        div { class: "card-ghost",
                            h4 { "登录" }
                            input { value: "{login_username.read()}", oninput: move |evt| login_username.set(evt.value()), placeholder: "邮箱" }
                            input { value: "{login_password.read()}", oninput: move |evt| login_password.set(evt.value()), placeholder: "密码", r#type: "password" }
                            div { class: "actions", button { onclick: move |_| login(), "登录" } }
                        }
                        div { class: "card-ghost",
                            h4 { "注册" }
                            p { class: "muted", "注册后需前往 Rainbow-Auth 邮箱验证完成激活。" }
                            input { value: "{register_username.read()}", oninput: move |evt| register_username.set(evt.value()), placeholder: "邮箱" }
                            input { value: "{register_password.read()}", oninput: move |evt| register_password.set(evt.value()), placeholder: "密码", r#type: "password" }
                            div { class: "actions", button { onclick: move |_| register(), "注册" } }
                        }
                    }
                }

                section { class: "panel grid two",
                    div {
                        h3 { "版块 / 主题" }
                        label { "选中版块 ID" }
                        input { value: "{selected_board.read()}", oninput: move |evt| selected_board.set(evt.value()) }
                        label { "选中主题 ID" }
                        input { value: "{selected_topic.read()}", oninput: move |evt| selected_topic.set(evt.value()) }
                        div { class: "actions",
                            button { onclick: move |_| load_boards(), "刷新版块" }
                            button { onclick: move |_| load_topics(), "刷新主题" }
                            button { onclick: move |_| {
                                let board_id = selected_board.read().clone();
                                if board_id.is_empty() { status.set("请选择版块".into()); return; }
                                let new_subject = new_topic_subject.read().clone();
                                let new_body = new_topic_body.read().clone();
                                if new_subject.trim().is_empty() || new_body.trim().is_empty() { status.set("请输入主题标题和内容".into()); return; }
                                let base = api_base.read().clone();
                                let jwt = token.read().clone();
                                let csrf = csrf_token.read().clone();
                                let mut topics = topics.clone();
                                let mut posts = posts.clone();
                                let mut status = status.clone();
                                spawn(async move {
                                    status.set("创建主题中...".into());
                                    let payload = CreateTopicPayload { board_id: board_id.clone(), subject: new_subject.clone(), body: new_body.clone() };
                                    match post_json::<TopicCreateResponse, _>(&base, "/surreal/topics", &jwt, &csrf, &payload).await {
                                        Ok(resp) => { topics.set({ let mut next = topics.read().clone(); next.insert(0, resp.topic.clone()); next }); posts.set(vec![resp.first_post]); status.set("主题已创建".into()); }
                                        Err(err) => status.set(format!("创建失败：{err}")),
                                    }
                                });
                            }, "创建主题" }
                        }
                        h4 { "版块" }
                        ul { class: "list board-list",
                            { boards.read().iter().cloned().map(|b| {
                                let selected_id = selected_board.read().clone();
                                rsx! {
                                    li {
                                        class: if selected_id == b.id.clone().unwrap_or_default() { "item selected" } else { "item" },
                                        onclick: move |_| { selected_board.set(b.id.clone().unwrap_or_default()); selected_topic.set("".into()); topics.set(Vec::new()); posts.set(Vec::new()); load_topics(); },
                                        strong { "{b.name}" }
                                        div { class: "meta", "{b.description.clone().unwrap_or_default()}" }
                                    }
                                }
                            })}
                        }
                    }
                    div {
                        h3 { "主题 / 帖子" }
                        label { "主题标题" }
                        input { value: "{new_topic_subject.read()}", oninput: move |evt| new_topic_subject.set(evt.value()), placeholder: "新主题标题" }
                        label { "主题内容" }
                        textarea { value: "{new_topic_body.read()}", oninput: move |evt| new_topic_body.set(evt.value()), rows: "3", placeholder: "新主题内容" }
                        div { class: "actions", button { onclick: move |_| load_topics(), "刷新主题" } }
                        h4 { "主题" }
                        ul { class: "list topic-list",
                            { topics.read().iter().cloned().map(|topic| {
                                let selected_topic_id = selected_topic.read().clone();
                                rsx! {
                                    li {
                                        class: if selected_topic_id == topic.id.clone().unwrap_or_default() { "item selected" } else { "item" },
                                        onclick: move |_| { selected_topic.set(topic.id.clone().unwrap_or_default()); load_posts(); },
                                        strong { "{topic.subject}" }
                                        div { class: "meta", "作者: {topic.author} | 时间: {topic.created_at.clone().unwrap_or_default()}" }
                                    }
                                }
                            })}
                        }

                        h4 { "新回帖" }
                        label { "主题 ID" }
                        input { value: "{selected_topic.read()}", oninput: move |evt| selected_topic.set(evt.value()) }
                        label { "回帖标题（可选）" }
                        input { value: "{new_post_subject.read()}", oninput: move |evt| new_post_subject.set(evt.value()), placeholder: "标题" }
                        label { "回帖内容" }
                        textarea { value: "{new_post_body.read()}", oninput: move |evt| new_post_body.set(evt.value()), rows: "3", placeholder: "内容" }
                        div { class: "actions",
                            button { onclick: move |_| {
                                let board_id = selected_board.read().clone();
                                let topic_id = selected_topic.read().clone();
                                let subject = new_post_subject.read().clone();
                                let body = new_post_body.read().clone();
                                let base = api_base.read().clone();
                                let jwt = token.read().clone();
                                let csrf = csrf_token.read().clone();
                                let mut status = status.clone();
                                let mut posts = posts.clone();
                                if board_id.is_empty() || topic_id.is_empty() { status.set("请先选择版块和主题".into()); return; }
                                if body.trim().is_empty() { status.set("回复内容不能为空".into()); return; }
                                spawn(async move {
                                    status.set("发送帖子中...".into());
                                    let payload = CreatePostPayload { topic_id: topic_id.clone(), board_id: board_id.clone(), subject: if subject.trim().is_empty() { None } else { Some(subject.clone()) }, body: body.clone() };
                                    match post_json::<PostResponse, _>(&base, "/surreal/topic/posts", &jwt, &csrf, &payload).await {
                                        Ok(resp) => { posts.set({ let mut next = posts.read().clone(); next.push(resp.post); next }); status.set("帖子已发送".into()); }
                                        Err(err) => status.set(format!("发送失败：{err}")),
                                    }
                                });
                            }, "发送" }
                        }

                        ul { class: "list post-list",
                            { posts.read().iter().cloned().map(|post| { rsx! {
                                li { class: "item",
                                    strong { "{post.subject}" }
                                    div { class: "meta", "作者: {post.author} | 时间: {post.created_at.clone().unwrap_or_default()}" }
                                    p { "{post.body}" }
                                }
                            }})}
                        }
                    }
                }

                section { class: "panel",
                    div { class: "panel__header",
                        h3 { "通知 / 附件占位" }
                        span { class: "muted", "仅元数据操作，不含真实文件" }
                    }
                    div { class: "actions",
                        button { onclick: move |_| load_notifications(), "刷新通知" }
                        button { onclick: move |_| send_notification(), "发送占位通知" }
                        button { onclick: move |_| load_attachments(), "刷新附件" }
                        button { onclick: move |_| create_attachment(), "创建占位附件" }
                    }
                    h4 { "通知" }
                    ul { class: "list",
                        { notifications.read().iter().cloned().map(|n| { rsx! {
                            li { class: "item",
                                strong { "{n.subject}" }
                                div { class: "meta", "用户: {n.user} | 时间: {n.created_at.clone().unwrap_or_default()} | 已读: {n.is_read.unwrap_or(false)}" }
                                p { "{n.body}" }
                                if !n.is_read.unwrap_or(false) {
                                    button { class: "ghost-btn", onclick: move |_| {
                                        let base = api_base.read().clone();
                                        let jwt = token.read().clone();
                                        let csrf = csrf_token.read().clone();
                                        let mut status = status.clone();
                                        let mut list = notifications.clone();
                                        let note_id = n.id.clone();
                                        spawn(async move {
                                            let payload = serde_json::json!({ "id": note_id.clone() });
                                            match post_json::<serde_json::Value, _>(&base, "/surreal/notifications/mark_read", &jwt, &csrf, &payload).await {
                                                Ok(_) => {
                                                    let mut current = list.read().clone();
                                                    if let Some(item) = current.iter_mut().find(|item| item.id == note_id) {
                                                        item.is_read = Some(true);
                                                    }
                                                    list.set(current);
                                                    status.set("通知已标记为已读".into());
                                                }
                                                Err(err) => status.set(format!("标记失败：{err}")),
                                            }
                                        });
                                    }, "标记已读" }
                                }
                            }
                        }})}
                    }
                    h4 { "附件元数据" }
                    ul { class: "list",
                        { let base_url = attachment_base_url.read().clone(); attachments.read().iter().cloned().map(move |a| { rsx! {
                            li { class: "item",
                                strong { "{a.filename} ({a.size_bytes} bytes)" }
                                div { class: "meta", "类型: {a.mime_type.clone().unwrap_or_default()} | 时间: {a.created_at.clone().unwrap_or_default()}" }
                                a { href: "{base_url.trim_end_matches('/')}/{a.filename}", target: "_blank", rel: "noopener", "下载" }
                                button { class: "link danger", onclick: move |_| {
                                    let Some(id) = a.id.clone() else { return; };
                                    let base = api_base.read().clone();
                                    let jwt = token.read().clone();
                                    let csrf = csrf_token.read().clone();
                                    let mut status = status.clone();
                                    let mut list = attachments.clone();
                                    spawn(async move {
                                        let payload = AttachmentDeletePayload { id: id.clone() };
                                        match post_json::<serde_json::Value, _>(&base, "/surreal/attachments/delete", &jwt, &csrf, &payload).await {
                                            Ok(_) => {
                                                let filtered: Vec<_> = list.read().iter().cloned().filter(|item| item.id.as_ref() != Some(&payload.id)).collect();
                                                list.set(filtered);
                                                status.set("附件已删除".into());
                                            }
                                            Err(err) => status.set(format!("删除失败：{err}")),
                                        }
                                    });
                                }, "删除" }
                            }
                        }}) }
                    }
                }

                section { class: "panel",
                    div { class: "panel__header",
                        h3 { "私信 (Inbox/Sent)" }
                        span { class: "muted", "简单收件箱/发件箱占位" }
                    }
                    div { class: "actions",
                        select { value: "{pm_folder.read()}", onchange: move |evt| pm_folder.set(evt.value()), option { value: "inbox", "收件箱" } option { value: "sent", "发件箱" } }
                        button { onclick: move |_| load_pms(), "刷新" }
                        button { onclick: move |_| {
                            let ids: Vec<i64> = personal_messages.read().iter().filter(|pm| !pm.is_read).map(|pm| pm.id).collect();
                            mark_pm_read(ids);
                        }, "全部标记已读" }
                        button { onclick: move |_| {
                            let ids: Vec<i64> = personal_messages.read().iter().map(|pm| pm.id).collect();
                            delete_pms(ids);
                        }, "删除全部" }
                    }
                    h4 { "发送私信" }
                    div { class: "stack",
                        input { value: "{pm_to.read()}", oninput: move |evt| pm_to.set(evt.value()), placeholder: "收件人用户名，逗号分隔" }
                        input { value: "{pm_subject.read()}", oninput: move |evt| pm_subject.set(evt.value()), placeholder: "标题" }
                        textarea { value: "{pm_body.read()}", oninput: move |evt| pm_body.set(evt.value()), rows: "3", placeholder: "内容" }
                        button { onclick: move |_| send_pm(), "发送私信" }
                    }
                    h4 { "当前私信" }
                    ul { class: "list",
                        { personal_messages.read().iter().cloned().map(|pm| { rsx! {
                            li { class: "item",
                                strong { "{pm.subject}" }
                                div { class: "meta", "来自: {pm.sender_name} | 时间: {pm.sent_at} | 已读: {pm.is_read}" }
                                p { "{pm.body}" }
                                div { class: "actions",
                                    button { class: "ghost-btn", onclick: move |_| mark_pm_read(vec![pm.id]), "标记已读" }
                                    button { class: "ghost-btn", onclick: move |_| delete_pms(vec![pm.id]), "删除" }
                                }
                            }
                        }}) }
                    }
                }
            }} else { rsx! {
                section { class: "hero hero--admin",
                    div { class: "hero__copy",
                        span { class: "pill", "Admin" }
                        h1 { "论坛管理后台" }
                        p { "管理 SurrealDB 中的 board_access 与 board_permissions，适合站点配置与灰度测试。" }
                        div { class: "hero__actions",
                            button { onclick: move |_| load_access(), "加载访问控制" }
                            button { onclick: move |_| load_permissions(), "加载版块权限" }
                        }
                    }
                    div { class: "hero__panel",
                        div { class: "stat", span { "API" } strong { "{api_base.read()}" } }
                        div { class: "stat-row",
                            div { class: "stat-box", strong { "{board_access.read().len()}" } span { "访问规则" } }
                            div { class: "stat-box", strong { "{board_permissions.read().len()}" } span { "权限规则" } }
                        }
                    }
                }

                section { class: "panel",
                    h2 { "连接 / 凭证" }
                    div { class: "grid two",
                        div {
                            label { "API 基址" }
                            input { value: "{api_base.read()}", oninput: move |evt| api_base.set(evt.value()) }
                            div { class: "actions",
                                button { onclick: move |_| status.set("已更新 API 基址".into()), "更新" }
                                button { onclick: move |_| load_access(), "加载数据" }
                                button { onclick: move |_| check_health(), "健康检查" }
                            }
                        }
                        div {
                            label { "JWT Token" }
                            textarea { value: "{token.read()}", rows: "3", oninput: move |evt| { token.set(evt.value()); save_token_to_storage(&evt.value()); } }
                            div { class: "actions",
                                button { onclick: move |_| { token.set("".into()); save_token_to_storage(""); status.set("已清空本地 token".into()); }, "清空 Token" }
                                button { onclick: move |_| { let csrf = format!("csrf-{}", js_sys::Date::now() as i64); csrf_token.set(csrf.clone()); set_csrf_cookie(&csrf); status.set("已刷新 CSRF".into()); }, "生成 CSRF" }
                            }
                        }
                    }
                }

                section { class: "panel",
                    h3 { "版块访问控制" }
                    div { class: "grid two",
                        div {
                            label { "board_id" }
                            input { value: "{access_board_id.read()}", oninput: move |evt| access_board_id.set(evt.value()), placeholder: "board_id" }
                            label { "允许的组 (逗号分隔)" }
                            input { value: "{access_groups.read()}", oninput: move |evt| access_groups.set(evt.value()), placeholder: "1,2,3" }
                            div { class: "actions", button { onclick: move |_| update_access(), "保存" } }
                        }
                        div {
                            h4 { "当前访问控制" }
                            ul { class: "list",
                                { board_access.read().iter().cloned().map(|entry| {
                                    let groups = if entry.allowed_groups.is_empty() { "(空)".into() } else { entry.allowed_groups.iter().map(|g| g.to_string()).collect::<Vec<_>>().join(", ") };
                                    rsx! {
                                        li { class: "item",
                                            strong { "Board #{entry.id}" }
                                            div { class: "meta", "允许组: {groups}" }
                                        }
                                    }
                                })}
                            }
                        }
                    }
                }

                section { class: "panel",
                    h3 { "版块权限" }
                    div { class: "grid two",
                        div {
                            label { "board_id" }
                            input { value: "{perm_board_id.read()}", oninput: move |evt| perm_board_id.set(evt.value()), placeholder: "board_id" }
                            label { "group_id" }
                            input { value: "{perm_group_id.read()}", oninput: move |evt| perm_group_id.set(evt.value()), placeholder: "group_id" }
                            label { "Allow (逗号分隔)" }
                            input { value: "{perm_allow.read()}", oninput: move |evt| perm_allow.set(evt.value()), placeholder: "post_new,post_reply_any" }
                            label { "Deny (逗号分隔)" }
                            input { value: "{perm_deny.read()}", oninput: move |evt| perm_deny.set(evt.value()), placeholder: "manage_boards" }
                            div { class: "actions", button { onclick: move |_| update_permissions(), "更新权限" } }
                        }
                        div {
                            h4 { "当前权限规则" }
                            ul { class: "list",
                                { board_permissions.read().iter().cloned().map(|entry| {
                                    let allow = if entry.allow.is_empty() { "无".into() } else { entry.allow.join(", ") };
                                    let deny = if entry.deny.is_empty() { "无".into() } else { entry.deny.join(", ") };
                                    rsx! {
                                        li { class: "item",
                                            strong { "Board #{entry.board_id} / Group #{entry.group_id}" }
                                            div { class: "meta", "Allow: {allow}" }
                                            div { class: "meta", "Deny: {deny}" }
                                        }
                                    }
                                })}
                            }
                        }
                    }
                }

                section { class: "panel",
                    h3 { "封禁（管理员）" }
                    p { "快速封禁/解封用户（member_id）。" }
                    div { class: "grid two",
                        div {
                            label { "member_id" }
                            input { value: "{ban_member_id.read()}", oninput: move |evt| ban_member_id.set(evt.value()), placeholder: "用户 ID（数字）" }
                            label { "封禁时长（小时）" }
                            input { value: "{ban_hours.read()}", oninput: move |evt| ban_hours.set(evt.value()), placeholder: "例如 24" }
                            label { "原因（可选）" }
                            input { value: "{ban_reason.read()}", oninput: move |evt| ban_reason.set(evt.value()), placeholder: "原因" }
                            div { class: "actions", button { onclick: move |_| apply_ban(), "封禁" } button { onclick: move |_| load_bans(), "刷新封禁列表" } }
                        }
                        div {
                            h4 { "当前封禁" }
                            ul { class: "list",
                            { bans.read().iter().cloned().map(|b| {
                                let expires = b.expires_at.clone().unwrap_or_default();
                                let reason = b.reason.clone().unwrap_or_default();
                                rsx! {
                                    li { class: "item",
                                        strong { "Ban #{b.id}" }
                                        div { class: "meta", "过期时间: {expires} | 原因: {reason}" }
                                        button { class: "ghost-btn", onclick: move |_| revoke_ban(b.id), "解除" }
                                    }
                                }
                            }) }
                            }
                        }
                    }
                }
            }} }
        }
    }
}

// ---------- Styles ----------
const STYLE: &str = r#"
:root { --bg: #0d1118; --panel: #131a26; --muted: #9da9c0; --text: #e8edf5; --accent: #f7931a; --accent2: #3d8bfd; --border: rgba(255,255,255,0.08); --radius: 14px; }
* { box-sizing: border-box; }
body { margin: 0; background: radial-gradient(circle at 18% 20%, rgba(247,147,26,0.08), transparent 26%), radial-gradient(circle at 82% 12%, rgba(61,139,253,0.12), transparent 24%), #0f141e; color: var(--text); font-family: "Inter", "Noto Sans SC", system-ui, -apple-system, sans-serif; }
a { color: inherit; text-decoration: none; }
.app-shell { max-width: 1200px; margin: 0 auto; padding: 18px 18px 36px; display: flex; flex-direction: column; gap: 14px; }
.top-nav { position: sticky; top: 0; z-index: 10; display: flex; align-items: center; justify-content: space-between; padding: 10px 14px; border: 1px solid var(--border); background: rgba(13,17,24,0.9); backdrop-filter: blur(8px); border-radius: 14px; box-shadow: 0 10px 40px rgba(0,0,0,0.35); }
.brand { display: flex; align-items: center; gap: 10px; font-weight: 800; letter-spacing: 0.4px; text-transform: uppercase; }
.brand__dot { width: 10px; height: 10px; border-radius: 50%; background: var(--accent); box-shadow: 0 0 12px rgba(247,147,26,0.8); }
.brand__tag { padding: 2px 8px; border-radius: 999px; background: rgba(61,139,253,0.15); color: #9dc3ff; font-size: 12px; }
.nav-links { display: flex; gap: 8px; align-items: center; }
.nav-link { padding: 8px 12px; border-radius: 10px; border: 1px solid var(--border); background: rgba(255,255,255,0.03); color: var(--text); font-weight: 600; cursor: pointer; }
.nav-link.active { background: linear-gradient(120deg, var(--accent), var(--accent2)); color: #0b0e15; box-shadow: 0 8px 28px rgba(61,139,253,0.3); }
.status-bar { border: 1 dashed var(--border); border-radius: 12px; padding: 10px 12px; color: var(--muted); background: rgba(255,255,255,0.02); }
.hero { display: grid; grid-template-columns: 1.3fr 1fr; gap: 18px; padding: 20px; border-radius: 16px; border: 1px solid var(--border); background: radial-gradient(circle at 15% 20%, rgba(247,147,26,0.15), transparent 40%), radial-gradient(circle at 85% 15%, rgba(61,139,253,0.16), transparent 35%), #0f141e; box-shadow: 0 16px 50px rgba(0,0,0,0.5); }
.hero__copy h1 { margin: 6px 0 8px; font-size: 28px; letter-spacing: 0.3px; }
.hero__copy p { margin: 0 0 12px; color: var(--muted); }
.hero__actions { display: flex; gap: 10px; flex-wrap: wrap; }
.hero__panel { background: rgba(255,255,255,0.04); border: 1px solid var(--border); border-radius: 12px; padding: 14px; display: flex; flex-direction: column; gap: 10px; }
.stat { display: flex; flex-direction: column; gap: 4px; color: var(--muted); }
.stat strong { color: var(--text); font-size: 15px; }
.stat-row { display: grid; grid-template-columns: repeat(auto-fit, minmax(110px, 1fr)); gap: 8px; }
.stat-box { background: rgba(0,0,0,0.25); border: 1px solid var(--border); border-radius: 10px; padding: 10px; text-align: center; }
.stat-box strong { font-size: 20px; display: block; color: #fbc27a; }
.pill { display: inline-block; padding: 4px 10px; border-radius: 999px; background: rgba(247,147,26,0.15); color: #ffbd71; font-weight: 700; letter-spacing: 0.6px; text-transform: uppercase; font-size: 12px; }
.ghost-btn { padding: 9px 12px; border-radius: 10px; border: 1px solid var(--border); background: transparent; color: var(--text); cursor: pointer; }
.panel { background: var(--panel); border: 1px solid var(--border); border-radius: var(--radius); padding: 16px; box-shadow: 0 12px 36px rgba(0,0,0,0.35); }
.panel h2, .panel h3, .panel h4 { margin: 0 0 10px; }
.panel__header { display: flex; align-items: baseline; justify-content: space-between; gap: 10px; }
.muted { color: var(--muted); font-size: 13px; }
.grid { display: grid; gap: 14px; }
.grid.two { grid-template-columns: repeat(auto-fit, minmax(320px, 1fr)); }
.grid.two.gap { gap: 16px; }
label { display: block; margin-top: 6px; font-weight: 700; color: var(--text); }
input, textarea { width: 100%; margin-top: 6px; padding: 10px 12px; border-radius: 10px; border: 1px solid var(--border); background: rgba(255,255,255,0.04); color: var(--text); }
input:focus, textarea:focus { outline: 1px solid var(--accent); border-color: var(--accent); }
textarea { resize: vertical; }
.actions { display: flex; gap: 10px; flex-wrap: wrap; margin-top: 12px; }
button { padding: 10px 14px; border: none; border-radius: 10px; background: linear-gradient(120deg, var(--accent), var(--accent2)); color: #0b0e13; font-weight: 800; cursor: pointer; }
.card-ghost { background: rgba(255,255,255,0.02); border: 1px dashed var(--border); border-radius: 12px; padding: 12px; }
.checkbox { display: flex; align-items: center; gap: 8px; margin-top: 8px; }
.stack { display: flex; flex-direction: column; gap: 8px; }
.list { list-style: none; padding: 0; margin: 12px 0 0 0; display: flex; flex-direction: column; gap: 10px; }
.item { background: rgba(255,255,255,0.03); border: 1px solid var(--border); padding: 10px 12px; border-radius: 12px; }
.item.selected { border-color: var(--accent2); background: rgba(61,139,253,0.09); }
.meta { color: var(--muted); font-size: 13px; margin-top: 4px; }
.hero--admin { background: radial-gradient(circle at 18% 20%, rgba(61,139,253,0.2), transparent 38%), radial-gradient(circle at 80% 10%, rgba(247,147,26,0.16), transparent 30%), #0e1521; }
@media (max-width: 900px) { .hero { grid-template-columns: 1fr; } }
@media (max-width: 640px) { .top-nav { flex-direction: column; align-items: flex-start; gap: 10px; } .nav-links { width: 100%; flex-wrap: wrap; } }
"#;
