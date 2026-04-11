use btc_forum_shared::{
    AdminAccount, AdminAccountsResponse, AdminTransferPayload, AdminTransferResponse, AdminUser,
    AdminUsersResponse, ApiError, AttachmentCreateResponse, AttachmentDeletePayload,
    AttachmentDeleteResponse, AttachmentListResponse, AttachmentMeta, AttachmentUploadResponse,
    AuthMeResponse, AuthResponse, BanApplyResponse, BanListResponse, BanPayload, BanRevokeResponse,
    BanRuleView, Board, BoardAccessEntry, BoardAccessPayload, BoardAccessResponse,
    BoardPermissionEntry, BoardPermissionPayload, BoardPermissionResponse, BoardsResponse,
    CreateAttachmentPayload, CreateBoardPayload, CreateBoardResponse, CreateNotificationPayload,
    CreatePostPayload, CreateTopicPayload, DocsPermissionGrantByRecordPayload,
    DocsPermissionGrantResponse, DocsPermissionRevokeByRecordPayload, DocsPermissionRevokeResponse,
    HealthResponse, LoginRequest, MarkNotificationPayload, MarkNotificationResponse,
    ModeratorUpdateByRecordPayload, ModeratorUpdateResponse, Notification,
    PointsBalanceResponse,
    NotificationCreateResponse, NotificationListResponse, PersonalMessage,
    PersonalMessageIdsPayload, PersonalMessageIdsResponse, PersonalMessageListResponse,
    PersonalMessageSendPayload, PersonalMessageSendResponse, Post, PostResponse, PostsResponse,
    RegisterRequest, RegisterResponse, Topic, TopicCreateResponse, TopicsResponse,
    UpdateBoardAccessResponse, UpdateBoardPermissionResponse,
};
use dioxus::prelude::*;
use gloo_timers::future::TimeoutFuture;
use reqwasm::http::{Request, RequestCredentials};
use std::{collections::HashMap, rc::Rc};
use web_sys::wasm_bindgen::JsCast;
use web_sys::{File, FormData, HtmlDocument, HtmlInputElement};

use js_sys;

use crate::api::client::ApiClient;
use crate::style::STYLE;

const BUILD_TAG: &str = "ban-click-v2";
const APP_BASE_PATH: &str = "/forum";
const API_BASE_PATH: &str = "/forum-api";

// ---------- Types ----------
#[derive(Clone, Copy, PartialEq, Eq)]
enum BoardFeedMode {
    Realtime,
    New,
    Top,
    Discussed,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct BoardFeedMetric {
    topic_count: usize,
    last_activity_at: Option<String>,
}

// ---------- Utilities ----------
fn window() -> Option<web_sys::Window> {
    web_sys::window()
}

fn is_page_visible() -> bool {
    let Some(win) = window() else { return true };
    let Some(doc) = win.document() else {
        return true;
    };
    // If the API isn't available, default to "visible" to avoid breaking data loading.
    !doc.hidden()
}

fn jitter_ms(base: u32, pct: f64) -> u32 {
    // Add +-pct jitter to avoid synchronized polling across tabs.
    let r = js_sys::Math::random(); // 0..1
    let jitter = ((r * 2.0 - 1.0) * pct).clamp(-pct, pct);
    let v = (base as f64) * (1.0 + jitter);
    v.max(250.0) as u32
}

fn board_created_at(board: &Board) -> String {
    board.created_at.clone().unwrap_or_default()
}

fn board_last_activity_at(board: &Board, metrics: &HashMap<String, BoardFeedMetric>) -> String {
    board
        .id
        .as_ref()
        .and_then(|id| metrics.get(id))
        .and_then(|metric| metric.last_activity_at.clone())
        .or_else(|| board.created_at.clone())
        .unwrap_or_default()
}

fn is_forbidden_error(err: &str) -> bool {
    err.contains("HTTP 403")
        || err.contains("403 (Forbidden)")
        || err.contains("403 Forbidden")
        || err.contains("权限不足")
        || err.to_lowercase().contains("forbidden")
}

fn is_server_error(err: &str) -> bool {
    err.contains("HTTP 500")
        || err.contains("500 (Internal Server Error)")
        || err.contains("500 Internal Server Error")
        || err.to_lowercase().contains("internal server error")
}

fn pm_error_message(err: &str) -> Option<&'static str> {
    if is_forbidden_error(err) {
        Some("该账号已被限制使用私信功能")
    } else if is_server_error(err) {
        Some("私信功能暂时不可用")
    } else {
        None
    }
}

fn save_token_to_storage(token: &str) {
    if let Some(win) = window() {
        if let Ok(Some(storage)) = win.session_storage() {
            let _ = storage.set_item("jwt_token", token);
        }
        if let Ok(Some(storage)) = win.local_storage() {
            let _ = storage.set_item("jwt_token", token);
        }
    }
}
fn load_token_from_storage() -> Option<String> {
    window()
        .and_then(|win| win.session_storage().ok().flatten())
        .and_then(|s| s.get_item("jwt_token").ok().flatten())
        .or_else(|| {
            window()
                .and_then(|win| win.local_storage().ok().flatten())
                .and_then(|s| s.get_item("jwt_token").ok().flatten())
        })
}
fn save_user_to_storage(name: &str) {
    if let Some(win) = window() {
        if let Ok(Some(storage)) = win.session_storage() {
            let _ = storage.set_item("user_name", name);
        }
        if let Ok(Some(storage)) = win.local_storage() {
            let _ = storage.set_item("user_name", name);
        }
    }
}
fn load_user_from_storage() -> Option<String> {
    window()
        .and_then(|win| win.session_storage().ok().flatten())
        .and_then(|s| s.get_item("user_name").ok().flatten())
        .or_else(|| {
            window()
                .and_then(|win| win.local_storage().ok().flatten())
                .and_then(|s| s.get_item("user_name").ok().flatten())
        })
}
fn clear_auth_storage() {
    // Best-effort clear both storages for backward compatibility with older builds.
    if let Some(win) = window() {
        if let Ok(Some(storage)) = win.session_storage() {
            let _ = storage.remove_item("jwt_token");
            let _ = storage.remove_item("user_name");
        }
        if let Ok(Some(storage)) = win.local_storage() {
            let _ = storage.remove_item("jwt_token");
            let _ = storage.remove_item("user_name");
        }
    }
}

fn should_use_explicit_sso(protocol: Option<&str>) -> bool {
    matches!(protocol, Some("https:"))
}

fn current_protocol() -> Option<String> {
    window().and_then(|win| win.location().protocol().ok())
}

fn build_cross_app_link_for_protocol(
    app_base_path: &str,
    token: Option<&str>,
    next_path: &str,
    protocol: Option<&str>,
) -> String {
    let clean_base = app_base_path.trim_end_matches('/');
    let clean_next = if next_path.trim().is_empty() {
        "/"
    } else {
        next_path
    };

    match token
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .filter(|_| should_use_explicit_sso(protocol))
    {
        Some(jwt) => format!(
            "{clean_base}/sso?token={}&next={}",
            urlencoding::encode(jwt),
            urlencoding::encode(clean_next)
        ),
        None => format!("{clean_base}/"),
    }
}

fn build_cross_app_link(app_base_path: &str, token: Option<&str>, next_path: &str) -> String {
    build_cross_app_link_for_protocol(
        app_base_path,
        token,
        next_path,
        current_protocol().as_deref(),
    )
}

#[cfg(test)]
mod tests {
    use super::build_cross_app_link_for_protocol;

    #[test]
    fn build_cross_app_link_uses_plain_home_when_token_missing() {
        assert_eq!(
            build_cross_app_link_for_protocol("/blog", None, "/", Some("http:")),
            "/blog/"
        );
        assert_eq!(
            build_cross_app_link_for_protocol("/docs/", Some("   "), "/dashboard", Some("https:")),
            "/docs/"
        );
    }

    #[test]
    fn build_cross_app_link_encodes_token_and_next_path_on_https() {
        let link = build_cross_app_link_for_protocol(
            "/docs",
            Some("abc.def+/="),
            "/spaces/demo?tab=all",
            Some("https:"),
        );
        assert_eq!(
            link,
            "/docs/sso?token=abc.def%2B%2F%3D&next=%2Fspaces%2Fdemo%3Ftab%3Dall"
        );
    }

    #[test]
    fn build_cross_app_link_skips_sso_on_http() {
        let link = build_cross_app_link_for_protocol(
            "/blog",
            Some("abc.def+/="),
            "/dashboard",
            Some("http:"),
        );
        assert_eq!(link, "/blog/");
    }
}

fn read_csrf_cookie() -> Option<String> {
    ApiClient::read_csrf_cookie()
}

fn build_client(base: &str, token: &str, csrf: &str) -> ApiClient {
    ApiClient::new(base)
        .with_token(token.to_string())
        .with_csrf(csrf.to_string())
}

fn normalize_app_path(path: &str) -> String {
    if path == APP_BASE_PATH {
        return "/".to_string();
    }
    if let Some(rest) = path.strip_prefix(APP_BASE_PATH) {
        return if rest.is_empty() {
            "/".to_string()
        } else {
            rest.to_string()
        };
    }
    path.to_string()
}

fn app_href(path: &str) -> String {
    let normalized = if path.is_empty() { "/" } else { path };
    if normalized == "/" {
        format!("{}/", APP_BASE_PATH)
    } else {
        format!("{}{}", APP_BASE_PATH, normalized)
    }
}

fn parse_forum_path(path: &str) -> (Option<String>, Option<String>) {
    let trimmed = path.trim_matches('/');
    if trimmed.is_empty() {
        return (None, None);
    }
    let parts: Vec<&str> = trimmed.split('/').collect();
    match parts.as_slice() {
        ["board", board_id] => (Some((*board_id).to_string()), None),
        ["board", board_id, "topic", topic_id] => {
            (Some((*board_id).to_string()), Some((*topic_id).to_string()))
        }
        _ => (None, None),
    }
}

fn forum_path(board_id: &str, topic_id: &str) -> String {
    if board_id.trim().is_empty() {
        "/".to_string()
    } else if topic_id.trim().is_empty() {
        format!("/board/{}", board_id)
    } else {
        format!("/board/{}/topic/{}", board_id, topic_id)
    }
}

fn replace_browser_path(path: &str) {
    if let Some(win) = window() {
        if let Ok(history) = win.history() {
            let _ = history.replace_state_with_url(
                &web_sys::wasm_bindgen::JsValue::NULL,
                "",
                Some(&app_href(path)),
            );
        }
    }
}

fn preview_points_snapshot(seed: &str) -> crate::components::points::PointsSnapshot {
    crate::components::points::PointsSnapshot::seeded(seed)
}

fn points_help_text(snapshot: &crate::components::points::PointsSnapshot) -> String {
    if snapshot.backend_ready {
        format!(
            "当前展示真实积分：Karma {} / Merit {}。",
            snapshot.karma.unwrap_or_default(),
            snapshot.merit.unwrap_or_default()
        )
    } else {
        "当前展示的是 preview 占位；登录后会自动读取真实 Karma / Merit。".to_string()
    }
}

// Legacy wrappers kept during refactor; services should call ApiClient directly.
async fn get_json<T: serde::de::DeserializeOwned>(
    base: &str,
    path: &str,
    token: &str,
    csrf: &str,
) -> Result<T, String> {
    build_client(base, token, csrf).get_json(path).await
}

async fn post_json<T: serde::de::DeserializeOwned, B: serde::Serialize>(
    base: &str,
    path: &str,
    token: &str,
    csrf: &str,
    body: &B,
) -> Result<T, String> {
    build_client(base, token, csrf).post_json(path, body).await
}

// ---------- App ----------
pub fn app() -> Element {
    // signals
    let mut api_base = use_signal(|| API_BASE_PATH.to_string());
    let mut token = use_signal(|| load_token_from_storage().unwrap_or_default());
    let mut current_user = use_signal(|| load_user_from_storage().unwrap_or_default());
    let mut status = use_signal(|| "等待操作...".to_string());
    let mut current_member_id = use_signal(|| None::<i64>);
    let mut csrf_token = use_signal(|| read_csrf_cookie().unwrap_or_default());
    let mut auth_checked = use_signal(|| false);
    let mut boards_checked = use_signal(|| false);
    let mut admin_bootstrapped = use_signal(|| false);
    let start_path = window()
        .and_then(|win| win.location().pathname().ok())
        .map(|path| normalize_app_path(&path))
        .unwrap_or_else(|| "/".to_string());
    let start_path_admin = start_path.clone();
    let start_path_register = start_path.clone();
    let start_path_login = start_path.clone();
    let (start_board_id, start_topic_id) = parse_forum_path(&start_path);
    let mut is_admin_page = use_signal(move || start_path_admin.starts_with("/admin"));
    let mut is_register_page = use_signal(move || start_path_register.starts_with("/register"));
    let mut is_login_page = use_signal(move || start_path_login.starts_with("/login"));
    let mut login_username = use_signal(|| "".to_string());
    let mut login_password = use_signal(|| "".to_string());
    let mut register_username = use_signal(|| "".to_string());
    let mut register_password = use_signal(|| "".to_string());
    let mut register_confirm = use_signal(|| "".to_string());
    let start_has_forum_detail = start_board_id.is_some();
    let mut show_topic_detail = use_signal(move || start_has_forum_detail);
    let mut focused_post_id = use_signal(|| "".to_string());

    let boards = use_signal(Vec::<Board>::new);
    let board_feed_metrics = use_signal(HashMap::<String, BoardFeedMetric>::new);
    let mut board_feed_mode = use_signal(|| BoardFeedMode::Realtime);
    let mut topics = use_signal(Vec::<Topic>::new);
    let mut posts = use_signal(Vec::<Post>::new);
    let board_access = use_signal(Vec::<BoardAccessEntry>::new);
    let board_permissions = use_signal(Vec::<BoardPermissionEntry>::new);
    let notifications = use_signal(Vec::<Notification>::new);
    let attachments = use_signal(Vec::<AttachmentMeta>::new);
    let attachment_base_url = use_signal(|| "/uploads".to_string());
    let mut pm_folder = use_signal(|| "inbox".to_string());
    let personal_messages = use_signal(Vec::<PersonalMessage>::new);
    let mut pm_disabled = use_signal(|| false);
    let mut pm_forbidden_notified = use_signal(|| false);
    let mut pm_to = use_signal(|| "".to_string());
    let mut pm_subject = use_signal(|| "".to_string());
    let mut pm_body = use_signal(|| "".to_string());
    let initial_board_id = start_board_id.unwrap_or_default();
    let initial_topic_id = start_topic_id.unwrap_or_default();
    let mut selected_board = use_signal(move || initial_board_id.clone());
    let mut selected_topic = use_signal(move || initial_topic_id.clone());
    let mut new_post_body = use_signal(|| "".to_string());
    let mut new_board_name = use_signal(|| "".to_string());
    let mut new_board_desc = use_signal(|| "".to_string());
    let mut new_board_topic_subject = use_signal(|| "".to_string());
    let mut new_board_topic_body = use_signal(|| "".to_string());
    let mut existing_topic_board_id = use_signal(|| "".to_string());
    let mut existing_topic_subject = use_signal(|| "".to_string());
    let mut existing_topic_body = use_signal(|| "".to_string());
    let mut access_board_id = use_signal(|| "".to_string());
    let mut access_groups = use_signal(|| "".to_string());
    let mut perm_board_id = use_signal(|| "".to_string());
    let mut perm_group_id = use_signal(|| "".to_string());
    let mut perm_allow = use_signal(|| "".to_string());
    let mut perm_deny = use_signal(|| "".to_string());
    let mut ban_member_id = use_signal(|| "".to_string());
    let mut ban_hours = use_signal(|| "24".to_string());
    let mut ban_reason = use_signal(|| "".to_string());
    let mut ban_cannot_post = use_signal(|| true);
    let mut ban_cannot_access = use_signal(|| false);
    let bans = use_signal(Vec::<BanRuleView>::new);
    let admin_users = use_signal(Vec::<AdminUser>::new);
    let admin_accounts = use_signal(Vec::<AdminAccount>::new);
    let mut admin_user_query = use_signal(|| "".to_string());
    let mut selected_record_id = use_signal(|| "".to_string());
    let mut transfer_target_record_id = use_signal(|| "".to_string());
    let mut transfer_demote_self = use_signal(|| true);
    let mut pm_auto_poll_started = use_signal(|| false);
    let mut forum_auto_poll_started = use_signal(|| false);
    let mut current_points_snapshot = use_signal(|| crate::components::points::PointsSnapshot::pending());

    // actions (login/register etc.)
    let login = move || {
        let base = api_base.read().clone();
        let user = login_username.read().clone();
        let pass = login_password.read().clone();
        let mut status = status.clone();
        let mut token_sig = token.clone();
        let mut current_user = current_user.clone();
        let mut is_login_page = is_login_page.clone();
        let mut is_register_page = is_register_page.clone();
        let mut is_admin_page = is_admin_page.clone();
        let mut boards_checked = boards_checked.clone();
        let mut auth_checked = auth_checked.clone();
        let mut pm_disabled = pm_disabled.clone();
        let mut pm_forbidden_notified = pm_forbidden_notified.clone();
        if user.is_empty() || pass.is_empty() {
            status.set("请输入邮箱和密码".into());
            return;
        }
        spawn(async move {
            status.set("登录中...".into());
            let payload = LoginRequest {
                email: user.clone(),
                password: pass.clone(),
            };
            match post_json::<AuthResponse, _>(&base, "/auth/login", "", "", &payload).await {
                Ok(resp) => {
                    save_token_to_storage(&resp.token);
                    token_sig.set(resp.token);
                    save_user_to_storage(&resp.user.name);
                    current_user.set(resp.user.name.clone());
                    current_member_id.set(resp.user.member_id);
                    let csrf = read_csrf_cookie().unwrap_or_default();
                    csrf_token.set(csrf);
                    status.set(format!("已登录：{}", resp.user.name));
                    is_login_page.set(false);
                    is_register_page.set(false);
                    is_admin_page.set(false);
                    boards_checked.set(false);
                    // Force a fresh /auth/me validation with the new token.
                    auth_checked.set(false);
                    pm_disabled.set(false);
                    pm_forbidden_notified.set(false);
                }
                Err(err) => status.set(format!("登录失败：{err}")),
            }
        });
    };

    let register = move || {
        let base = api_base.read().clone();
        let user = register_username.read().clone();
        let pass = register_password.read().clone();
        let confirm = register_confirm.read().clone();
        let mut status = status.clone();
        if user.is_empty() || pass.is_empty() {
            status.set("请输入邮箱和密码".into());
            return;
        }
        if !confirm.is_empty() && confirm != pass {
            status.set("两次密码不一致".into());
            return;
        }
        spawn(async move {
            status.set("注册中...".into());
            let payload = RegisterRequest {
                email: user.clone(),
                password: pass.clone(),
                username: None,
                role: None,
                permissions: None,
            };
            match post_json::<RegisterResponse, _>(&base, "/auth/register", "", "", &payload).await
            {
                Ok(resp) => {
                    status.set(resp.message);
                }
                Err(err) => {
                    let friendly = if err.contains("Email already registered")
                        || err.contains("邮箱")
                        || err.contains("已注册")
                    {
                        "该邮箱已注册，请直接登录或更换邮箱".to_string()
                    } else {
                        format!("注册失败：{err}")
                    };
                    status.set(friendly);
                }
            }
        });
    };

    let create_board = move || {
        let base = api_base.read().clone();
        let jwt = token.read().clone();
        let csrf = csrf_token.read().clone();
        let name = new_board_name.read().clone();
        let desc = new_board_desc.read().clone();
        let first_topic_subject = new_board_topic_subject.read().clone();
        let first_topic_body = new_board_topic_body.read().clone();
        let mut status = status.clone();
        let mut boards_sig = boards.clone();
        if name.trim().is_empty() {
            status.set("请输入版块名称".into());
            return;
        }
        if first_topic_subject.trim().is_empty() || first_topic_body.trim().is_empty() {
            status.set("请填写首个主题标题和内容".into());
            return;
        }
        spawn(async move {
            status.set("创建版块中...".into());
            let payload = CreateBoardPayload {
                name: name.clone(),
                description: if desc.trim().is_empty() {
                    None
                } else {
                    Some(desc.clone())
                },
            };
            let client = build_client(&base, &jwt, &csrf);
            match crate::services::forum::create_board(&client, &payload).await {
                Ok(resp) => {
                    status.set(format!("版块已创建：{}，正在创建首个主题...", name));
                    // Optimistically update local list so UI reflects success even if follow-up fetch fails.
                    let mut next = boards_sig.read().clone();
                    let created = resp.board.clone();
                    let exists = next.iter().any(|b| match (&b.id, &created.id) {
                        (Some(lhs), Some(rhs)) => lhs == rhs,
                        _ => b.name == created.name,
                    });
                    if !exists {
                        next.insert(0, created);
                        boards_sig.set(next);
                    }
                    let board_id = resp.board.id.clone().unwrap_or_default();
                    if !board_id.trim().is_empty() {
                        let topic_payload = CreateTopicPayload {
                            board_id: board_id.clone(),
                            subject: first_topic_subject.clone(),
                            body: first_topic_body.clone(),
                        };
                        match crate::services::forum::create_topic(&client, &topic_payload).await {
                            Ok(_) => {
                                status.set(format!("版块和首个主题已创建：{}", name));
                            }
                            Err(err) => {
                                status.set(format!("版块已创建，但首个主题创建失败：{err}"));
                            }
                        }
                    } else {
                        status.set("版块已创建，但未拿到 board_id，无法自动创建首个主题".into());
                    }
                    if let Ok(resp) = crate::services::forum::load_boards(&client).await {
                        boards_sig.set(resp.boards);
                    } else {
                        status.set(format!(
                            "版块已创建：{}（列表自动刷新失败，请稍后重试）",
                            name
                        ));
                    }
                }
                Err(err) => status.set(format!("创建版块失败：{err}")),
            }
        });
    };

    let create_topic = move || {
        let base = api_base.read().clone();
        let jwt = token.read().clone();
        let csrf = csrf_token.read().clone();
        let board_id = existing_topic_board_id.read().clone();
        let subject = existing_topic_subject.read().clone();
        let body = existing_topic_body.read().clone();
        let mut status = status.clone();
        let mut topics_sig = topics.clone();
        if board_id.trim().is_empty() {
            status.set("请先填写已有版块 ID".into());
            return;
        }
        if subject.trim().is_empty() || body.trim().is_empty() {
            status.set("请填写主题标题和内容".into());
            return;
        }
        spawn(async move {
            status.set("创建主题中...".into());
            let payload = CreateTopicPayload {
                board_id: board_id.clone(),
                subject: subject.clone(),
                body: body.clone(),
            };
            let client = build_client(&base, &jwt, &csrf);
            match crate::services::forum::create_topic(&client, &payload).await {
                Ok(resp) => {
                    let mut next = topics_sig.read().clone();
                    next.insert(0, resp.topic.clone());
                    topics_sig.set(next);
                    status.set(format!("主题已创建：{}", resp.topic.subject));
                    selected_board.set(board_id.clone());
                    selected_topic.set(resp.topic.id.clone().unwrap_or_default());
                    posts.set(vec![resp.first_post.clone()]);
                    show_topic_detail.set(true);
                    replace_browser_path(&forum_path(
                        &board_id,
                        &resp.topic.id.clone().unwrap_or_default(),
                    ));
                    existing_topic_subject.set(String::new());
                    existing_topic_body.set(String::new());
                }
                Err(err) => status.set(format!("创建主题失败：{err}")),
            }
        });
    };

    // Rule: do not mutate signals in the render path; trigger side effects from use_effect/event handlers only.
    use_effect(move || {
        if *auth_checked.read() {
            return;
        }
        auth_checked.set(true);
        let base = api_base.read().clone();
        let jwt = token.read().clone();
        let mut status = status.clone();
        let mut token_sig = token.clone();
        let mut current_user = current_user.clone();
        let mut pm_disabled = pm_disabled.clone();
        let mut pm_forbidden_notified = pm_forbidden_notified.clone();
        spawn(async move {
            if jwt.trim().is_empty() {
                return;
            }
            status.set("校验登录状态...".into());
            match get_json::<AuthMeResponse>(&base, "/auth/me", &jwt, "").await {
                Ok(resp) => {
                    let perms = resp.user.permissions.clone().unwrap_or_default();
                    let pm_blocked = perms
                        .iter()
                        .any(|perm| perm == "ban_cannot_post" || perm == "ban_cannot_access");
                    save_user_to_storage(&resp.user.name);
                    current_user.set(resp.user.name);
                    current_member_id.set(resp.user.member_id);
                    let csrf = read_csrf_cookie().unwrap_or_default();
                    csrf_token.set(csrf);
                    pm_disabled.set(pm_blocked);
                    pm_forbidden_notified.set(pm_blocked);
                    if pm_blocked {
                        status.set("该账号已被限制使用私信功能".into());
                    } else {
                        status.set("登录已验证".into());
                    }
                }
                Err(err) => {
                    let err_text = err.to_string();
                    let is_auth_error = err_text.contains("401") || err_text.contains("403");
                    if is_auth_error {
                        clear_auth_storage();
                        token_sig.set("".into());
                        current_user.set("".into());
                        pm_disabled.set(false);
                        pm_forbidden_notified.set(false);
                        status.set(format!("登录已失效：{err_text}"));
                    } else {
                        // Backend transient errors (e.g. 5xx) should not force logout.
                        status.set(format!("登录校验暂时失败：{err_text}"));
                    }
                }
            }
        });
    });

    // data loaders
    let load_boards = move || {
        let base = api_base.read().clone();
        let jwt = token.read().clone();
        let csrf = csrf_token.read().clone();
        let mut status = status.clone();
        let mut boards = boards.clone();
        let mut board_feed_metrics = board_feed_metrics.clone();
        let mut selected_board = selected_board.clone();
        spawn(async move {
            status.set("加载版块中...".into());
            let client = build_client(&base, &jwt, &csrf);
            match crate::services::forum::load_boards(&client).await {
                Ok(resp) => {
                    let boards_data = resp.boards;
                    let mut metrics = HashMap::new();
                    for board in &boards_data {
                        let Some(board_id) = board.id.clone() else {
                            continue;
                        };
                        if let Ok(topics_resp) =
                            crate::services::forum::load_topics(&client, &board_id).await
                        {
                            let last_activity_at = topics_resp
                                .topics
                                .iter()
                                .filter_map(|topic| {
                                    topic
                                        .updated_at
                                        .clone()
                                        .or_else(|| topic.created_at.clone())
                                })
                                .max();
                            metrics.insert(
                                board_id,
                                BoardFeedMetric {
                                    topic_count: topics_resp.topics.len(),
                                    last_activity_at,
                                },
                            );
                        }
                    }
                    selected_board.set(
                        boards_data
                            .get(0)
                            .and_then(|b| b.id.clone())
                            .unwrap_or_default(),
                    );
                    board_feed_metrics.set(metrics);
                    boards.set(boards_data);
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
            match get_json::<HealthResponse>(&base, "/health", "", "").await {
                Ok(resp) => {
                    let service = resp.service;
                    let surreal = resp.surreal.status;
                    status.set(format!("健康检查: {service} / surreal: {surreal}"));
                }
                Err(err) => status.set(format!("健康检查失败：{err}")),
            }
        });
    };

    let load_topics_for_board = move |board_id: String| {
        let base = api_base.read().clone();
        let jwt = token.read().clone();
        let csrf = csrf_token.read().clone();
        let mut status = status.clone();
        let mut topics = topics.clone();
        let mut posts = posts.clone();
        let mut selected_topic = selected_topic.clone();
        let selected_topic_id = selected_topic.read().clone();
        if board_id.is_empty() {
            status.set("请先选择版块".into());
            return;
        }
        spawn(async move {
            status.set("加载主题中...".into());
            let client = build_client(&base, &jwt, &csrf);
            match crate::services::forum::load_topics(&client, &board_id).await {
                Ok(resp) => {
                    let next_topic_id = if !selected_topic_id.trim().is_empty()
                        && resp
                            .topics
                            .iter()
                            .any(|t| t.id.as_deref() == Some(selected_topic_id.as_str()))
                    {
                        selected_topic_id.clone()
                    } else {
                        resp.topics
                            .get(0)
                            .and_then(|t| t.id.clone())
                            .unwrap_or_default()
                    };

                    if !next_topic_id.is_empty() {
                        selected_topic.set(next_topic_id.clone());
                        replace_browser_path(&forum_path(&board_id, &next_topic_id));
                        match crate::services::forum::load_posts(&client, &next_topic_id).await {
                            Ok(posts_resp) => {
                                posts.set(posts_resp.posts);
                                status.set("主题/帖子加载完成".into());
                            }
                            Err(err) => status.set(format!("加载帖子失败：{err}")),
                        }
                    } else {
                        posts.set(Vec::new());
                        status.set("暂无主题".into());
                    }
                    topics.set(resp.topics);
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
        let board_id = selected_board.read().clone();
        let topic_id = selected_topic.read().clone();
        if topic_id.is_empty() {
            status.set("请先选择主题".into());
            return;
        }
        spawn(async move {
            status.set("加载帖子中...".into());
            let client = build_client(&base, &jwt, &csrf);
            match crate::services::forum::load_posts(&client, &topic_id).await {
                Ok(resp) => {
                    posts.set(resp.posts);
                    replace_browser_path(&forum_path(&board_id, &topic_id));
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
            match get_json::<NotificationListResponse>(&base, "/surreal/notifications", &jwt, &csrf)
                .await
            {
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
            let payload = CreateNotificationPayload {
                subject: "Hello".to_string(),
                body: "这是一条占位通知".to_string(),
                user: None,
            };
            match post_json::<NotificationCreateResponse, _>(
                &base,
                "/surreal/notifications",
                &jwt,
                &csrf,
                &payload,
            )
            .await
            {
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
            match get_json::<AttachmentListResponse>(&base, "/surreal/attachments", &jwt, &csrf)
                .await
            {
                Ok(resp) => {
                    if let Some(url) = resp.base_url {
                        base_url_sig.set(url);
                    }
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
            let payload = CreateAttachmentPayload {
                filename: "demo.txt".to_string(),
                size_bytes: 1234,
                mime_type: Some("text/plain".to_string()),
                board_id: None,
                topic_id: None,
            };
            match post_json::<AttachmentCreateResponse, _>(
                &base,
                "/surreal/attachments",
                &jwt,
                &csrf,
                &payload,
            )
            .await
            {
                Ok(resp) => {
                    if let Some(url) = resp.base_url {
                        base_url_sig.set(url);
                    }
                    let mut current = list.read().clone();
                    current.insert(0, resp.attachment);
                    list.set(current);
                    status.set("附件元数据已创建（占位，不含文件）".into());
                }
                Err(err) => status.set(format!("创建失败：{err}")),
            }
        });
    };

    let upload_attachment = move || {
        let base = api_base.read().clone();
        let jwt = token.read().clone();
        let csrf = csrf_token.read().clone();
        let board_id = selected_board.read().clone();
        let topic_id = selected_topic.read().clone();
        let mut status = status.clone();
        let mut list = attachments.clone();
        let mut base_url_sig = attachment_base_url.clone();
        if jwt.trim().is_empty() {
            status.set("请先登录再上传附件".into());
            return;
        }
        spawn(async move {
            let Some(doc) = window().and_then(|win| win.document()) else {
                status.set("无法访问页面文档".into());
                return;
            };
            let Some(el) = doc.get_element_by_id("file-upload") else {
                status.set("未找到文件输入框".into());
                return;
            };
            let Ok(input) = el.dyn_into::<HtmlInputElement>() else {
                status.set("文件输入框类型不正确".into());
                return;
            };
            let Some(files) = input.files() else {
                status.set("请选择文件".into());
                return;
            };
            let Some(file) = files.get(0) else {
                status.set("请选择文件".into());
                return;
            };
            let file: File = file;
            let form = FormData::new().unwrap();
            let _ = form.append_with_blob_and_filename("file", &file, &file.name());
            if !board_id.trim().is_empty() {
                let _ = form.append_with_str("board_id", &board_id);
            }
            if !topic_id.trim().is_empty() {
                let _ = form.append_with_str("topic_id", &topic_id);
            }
            status.set("上传附件中...".into());
            let url = format!(
                "{}/{}",
                base.trim_end_matches('/'),
                "surreal/attachments/upload"
            );
            let resp = Request::post(&url)
                .header("Authorization", &format!("Bearer {}", jwt))
                .header("X-CSRF-TOKEN", &csrf)
                .credentials(RequestCredentials::Include)
                .body(form)
                .send()
                .await;
            let resp = match resp {
                Ok(resp) => resp,
                Err(err) => {
                    status.set(format!("上传失败：{err}"));
                    return;
                }
            };
            let status_code = resp.status();
            let text = match resp.text().await {
                Ok(text) => text,
                Err(err) => {
                    status.set(format!("读取响应失败：{err}"));
                    return;
                }
            };
            if !resp.ok() {
                status.set(format!("上传失败：HTTP {status_code}: {text}"));
                return;
            }
            match serde_json::from_str::<AttachmentUploadResponse>(&text) {
                Ok(resp) => {
                    if let Some(url) = resp.base_url {
                        base_url_sig.set(url);
                    }
                    let mut current = list.read().clone();
                    current.insert(0, resp.attachment);
                    list.set(current);
                    input.set_value("");
                    status.set("附件上传完成".into());
                }
                Err(err) => status.set(format!("解析失败：{err}")),
            }
        });
    };

    let load_pms = move || {
        let base = api_base.read().clone();
        let jwt = token.read().clone();
        let csrf = csrf_token.read().clone();
        let mut status = status.clone();
        let mut list = personal_messages.clone();
        let pm_disabled_now = *pm_disabled.read();
        let mut forbidden_notified = pm_forbidden_notified.clone();
        let folder = pm_folder.read().clone();
        if jwt.trim().is_empty() {
            status.set("请先登录再查看私信".into());
            return;
        }
        if pm_disabled_now {
            status.set("该账号已被限制使用私信功能".into());
            list.set(Vec::new());
            return;
        }
        spawn(async move {
            status.set("加载私信中...".into());
            let client = build_client(&base, &jwt, &csrf);
            match crate::services::pm::load_pms(&client, &folder).await {
                Ok(resp) => {
                    list.set(resp.messages);
                    status.set("私信加载完成".into());
                }
                Err(err) => {
                    if let Some(message) = pm_error_message(&err) {
                        list.set(Vec::new());
                        if !*forbidden_notified.read() {
                            forbidden_notified.set(true);
                            status.set(message.into());
                        }
                    } else {
                        status.set(format!("加载私信失败：{err}"));
                    }
                }
            }
        });
    };

    let mark_pm_read = move |ids: Vec<i64>| {
        let base = api_base.read().clone();
        let jwt = token.read().clone();
        let csrf = csrf_token.read().clone();
        let mut status = status.clone();
        let mut list = personal_messages.clone();
        if jwt.trim().is_empty() || ids.is_empty() {
            return;
        }
        spawn(async move {
            let payload = PersonalMessageIdsPayload { ids: ids.clone() };
            let client = build_client(&base, &jwt, &csrf);
            match crate::services::pm::mark_read(&client, &payload).await {
                Ok(_) => {
                    let mut current = list.read().clone();
                    for pm in current.iter_mut() {
                        if ids.iter().any(|id| *id == pm.id) {
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
        if jwt.trim().is_empty() || ids.is_empty() {
            return;
        }
        spawn(async move {
            let payload = PersonalMessageIdsPayload { ids: ids.clone() };
            let client = build_client(&base, &jwt, &csrf);
            match crate::services::pm::delete(&client, &payload).await {
                Ok(_) => {
                    let filtered: Vec<_> = list
                        .read()
                        .iter()
                        .cloned()
                        .filter(|pm| !ids.contains(&pm.id))
                        .collect();
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
        let mut list = personal_messages.clone();
        let folder_sig = pm_folder.clone();
        let to_raw = pm_to.read().clone();
        let subj = pm_subject.read().clone();
        let body = pm_body.read().clone();
        let pm_disabled_now = *pm_disabled.read();
        if jwt.trim().is_empty() {
            status.set("请先登录".into());
            return;
        }
        if pm_disabled_now {
            status.set("该账号已被限制使用私信功能".into());
            return;
        }
        if to_raw.trim().is_empty() || body.trim().is_empty() {
            status.set("请填写收件人和内容".into());
            return;
        }
        let recipients: Vec<String> = to_raw
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        spawn(async move {
            status.set("发送私信中...".into());
            let payload = PersonalMessageSendPayload {
                to: recipients,
                subject: subj,
                body,
            };
            let client = build_client(&base, &jwt, &csrf);
            match crate::services::pm::send(&client, &payload).await {
                Ok(_) => {
                    status.set("私信已发送".into());
                    if let Ok(resp) =
                        crate::services::pm::load_pms(&client, &folder_sig.read().clone()).await
                    {
                        list.set(resp.messages);
                    }
                }
                Err(err) => status.set(format!("发送失败：{err}")),
            }
        });
    };

    use_effect(move || {
        let mut forbidden_notified = pm_forbidden_notified.clone();
        if token.read().trim().is_empty() {
            pm_disabled.set(false);
            forbidden_notified.set(false);
        }
    });

    use_effect(move || {
        if *pm_auto_poll_started.read()
            || *pm_disabled.read()
            || *pm_forbidden_notified.read()
            || *is_admin_page.read()
            || token.read().trim().is_empty()
        {
            return;
        }
        pm_auto_poll_started.set(true);
        let base_sig = api_base.clone();
        let jwt_sig = token.clone();
        let csrf_sig = csrf_token.clone();
        let folder_sig = pm_folder.clone();
        let mut list_sig = personal_messages.clone();
        let admin_page_sig = is_admin_page.clone();
        let mut started_sig = pm_auto_poll_started.clone();
        let mut status_sig = status.clone();
        let mut forbidden_notified_sig = pm_forbidden_notified.clone();
        spawn(async move {
            // Stop conditions: logout OR navigating into admin view.
            // Backoff strategy: on errors, increase interval up to 60s with jitter.
            let mut interval_ms: u32 = 12_000;
            loop {
                let is_admin = *admin_page_sig.read();
                let jwt = jwt_sig.read().clone();
                if jwt.trim().is_empty() || is_admin {
                    started_sig.set(false);
                    break;
                }

                // Visibility gating: when tab is hidden, slow down polling.
                if !is_page_visible() {
                    TimeoutFuture::new(jitter_ms(30_000, 0.15)).await;
                    continue;
                }

                let base = base_sig.read().clone();
                let csrf = csrf_sig.read().clone();
                let folder = folder_sig.read().clone();

                let client = build_client(&base, &jwt, &csrf);
                match crate::services::pm::load_pms(&client, &folder).await {
                    Ok(resp) => {
                        list_sig.set(resp.messages);
                        interval_ms = 12_000;
                    }
                    Err(err) => {
                        if let Some(message) = pm_error_message(&err) {
                            list_sig.set(Vec::new());
                            if !*forbidden_notified_sig.read() {
                                forbidden_notified_sig.set(true);
                                status_sig.set(message.into());
                            }
                            started_sig.set(false);
                            break;
                        }
                        interval_ms = (interval_ms.saturating_mul(2)).min(60_000);
                    }
                }

                TimeoutFuture::new(jitter_ms(interval_ms, 0.10)).await;
            }
        });
    });

    use_effect(move || {
        if *forum_auto_poll_started.read()
            || *is_admin_page.read()
            || !*show_topic_detail.read()
            || token.read().trim().is_empty()
        {
            return;
        }
        forum_auto_poll_started.set(true);
        let base_sig = api_base.clone();
        let jwt_sig = token.clone();
        let csrf_sig = csrf_token.clone();
        let board_sig = selected_board.clone();
        let topic_sig = selected_topic.clone();
        let show_detail_sig = show_topic_detail.clone();
        let admin_page_sig = is_admin_page.clone();
        let mut boards_sig = boards.clone();
        let mut topics_sig = topics.clone();
        let mut posts_sig = posts.clone();
        let mut started_sig = forum_auto_poll_started.clone();
        spawn(async move {
            // Stop conditions: logout OR admin view OR leaving topic detail view.
            // Backoff strategy: on errors, increase interval up to 60s with jitter.
            let mut interval_ms: u32 = 8_000;
            loop {
                let jwt = jwt_sig.read().clone();
                let is_admin = *admin_page_sig.read();
                let show_detail = *show_detail_sig.read();
                if jwt.trim().is_empty() || is_admin || !show_detail {
                    started_sig.set(false);
                    break;
                }

                // Visibility gating: when tab is hidden, slow down polling.
                if !is_page_visible() {
                    TimeoutFuture::new(jitter_ms(30_000, 0.15)).await;
                    continue;
                }

                let base = base_sig.read().clone();
                let csrf = csrf_sig.read().clone();

                // Refresh boards/topics/posts only when user is actively viewing topic detail.
                let mut ok = true;
                match get_json::<BoardsResponse>(&base, "/surreal/boards", &jwt, &csrf).await {
                    Ok(resp) => boards_sig.set(resp.boards),
                    Err(_) => ok = false,
                }

                let board_id = board_sig.read().clone();
                if !board_id.trim().is_empty() {
                    let topics_path = format!("/surreal/topics?board_id={}", board_id);
                    match get_json::<TopicsResponse>(&base, &topics_path, &jwt, &csrf).await {
                        Ok(resp) => topics_sig.set(resp.topics),
                        Err(_) => ok = false,
                    }
                }

                let topic_id = topic_sig.read().clone();
                if !topic_id.trim().is_empty() {
                    let posts_path = format!("/surreal/topic/posts?topic_id={}", topic_id);
                    match get_json::<PostsResponse>(&base, &posts_path, &jwt, &csrf).await {
                        Ok(resp) => posts_sig.set(resp.posts),
                        Err(_) => ok = false,
                    }
                }

                if ok {
                    interval_ms = 8_000;
                } else {
                    interval_ms = (interval_ms.saturating_mul(2)).min(60_000);
                }

                TimeoutFuture::new(jitter_ms(interval_ms, 0.10)).await;
            }
        });
    });

    let load_access = move || {
        let base = api_base.read().clone();
        let jwt = token.read().clone();
        let csrf = csrf_token.read().clone();
        let mut status = status.clone();
        let mut access = board_access.clone();
        if jwt.trim().is_empty() {
            status.set("请先登录/粘贴管理员 JWT".into());
            return;
        }
        spawn(async move {
            status.set("加载版块访问控制...".into());
            let client = build_client(&base, &jwt, &csrf);
            match crate::services::admin::load_board_access(&client).await {
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
        if jwt.trim().is_empty() {
            status.set("请先登录/粘贴管理员 JWT".into());
            return;
        }
        if board_id.is_empty() {
            status.set("请输入有效的版块 ID".into());
            return;
        }
        let mut groups = Vec::new();
        if !groups_raw.trim().is_empty() {
            for part in groups_raw.split(',') {
                if let Ok(id) = part.trim().parse::<i64>() {
                    groups.push(id);
                }
            }
        }
        spawn(async move {
            status.set("更新版块访问控制...".into());
            let payload = BoardAccessPayload {
                board_id: board_id.clone(),
                allowed_groups: groups.clone(),
            };
            let client = build_client(&base, &jwt, &csrf);
            match crate::services::admin::update_board_access(&client, &payload).await {
                Ok(resp) => {
                    let mut current = access.read().clone();
                    if let Some(entry) = current.iter_mut().find(|e| e.id == resp.board_id) {
                        entry.allowed_groups = resp.allowed_groups.clone();
                    }
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
        if jwt.trim().is_empty() {
            status.set("请先登录/粘贴管理员 JWT".into());
            return;
        }
        spawn(async move {
            status.set("加载版块权限中...".into());
            let client = build_client(&base, &jwt, &csrf);
            match crate::services::admin::load_board_permissions(&client).await {
                Ok(resp) => {
                    perms.set(resp.entries);
                    status.set("版块权限已加载".into());
                }
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
        let allow: Vec<String> = perm_allow
            .read()
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let deny: Vec<String> = perm_deny
            .read()
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if jwt.trim().is_empty() {
            status.set("请先登录/粘贴管理员 JWT".into());
            return;
        }
        if board_id.is_empty() || group_id == 0 {
            status.set("请输入有效的 board_id 与 group_id".into());
            return;
        }
        spawn(async move {
            status.set("更新版块权限...".into());
            let payload = BoardPermissionPayload {
                board_id: board_id.clone(),
                group_id,
                allow: allow.clone(),
                deny: deny.clone(),
            };
            let client = build_client(&base, &jwt, &csrf);
            match crate::services::admin::update_board_permissions(&client, &payload).await {
                Ok(resp) => {
                    let mut current = perms.read().clone();
                    if let Some(entry) = current
                        .iter_mut()
                        .find(|e| e.board_id == resp.board_id && e.group_id == resp.group_id)
                    {
                        entry.allow = resp.allow.clone();
                        entry.deny = resp.deny.clone();
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
        let mut bans_sig = bans.clone();
        let member_id = ban_member_id.read().trim().parse::<i64>().unwrap_or(0);
        let hours = ban_hours.read().trim().parse::<i64>().unwrap_or(0);
        let reason = ban_reason.read().clone();
        let cannot_post = *ban_cannot_post.read();
        let cannot_access = *ban_cannot_access.read();
        if jwt.trim().is_empty() {
            status.set("请先登录/粘贴管理员 JWT".into());
            return;
        }
        if member_id == 0 || hours <= 0 {
            status.set("请输入有效的 Legacy ID（member_id）与时长".into());
            return;
        }
        spawn(async move {
            status.set("封禁中...".into());
            let payload = BanPayload {
                member_id: Some(member_id),
                ban_id: None,
                reason: Some(reason),
                hours: Some(hours),
                cannot_post,
                cannot_access,
            };
            let client = build_client(&base, &jwt, &csrf);
            match crate::services::admin::apply_ban(&client, &payload).await {
                Ok(_) => {
                    status.set("已封禁".into());
                    // refresh list
                    if let Ok(resp) = crate::services::admin::load_bans(&client).await {
                        bans_sig.set(resp.bans);
                        status.set("封禁列表已刷新".into());
                    }
                }
                Err(err) => status.set(format!("封禁失败：{err}")),
            }
        });
    };

    let revoke_ban = Rc::new(move |ban_id: i64| {
        let base = api_base.read().clone();
        let jwt = token.read().clone();
        let csrf = csrf_token.read().clone();
        let mut status = status.clone();
        let mut bans_sig = bans.clone();
        if jwt.trim().is_empty() {
            status.set("请先登录/粘贴管理员 JWT".into());
            return;
        }
        status.set("解除封禁中...".into());
        spawn(async move {
            let payload = BanPayload {
                member_id: None,
                ban_id: Some(ban_id),
                reason: None,
                hours: None,
                cannot_post: false,
                cannot_access: false,
            };
            let client = build_client(&base, &jwt, &csrf);
            match crate::services::admin::revoke_ban(&client, &payload).await {
                Ok(_) => {
                    status.set("已解除封禁".into());
                    if let Ok(resp) = crate::services::admin::load_bans(&client).await {
                        bans_sig.set(resp.bans);
                        status.set("封禁列表已刷新".into());
                    }
                }
                Err(err) => status.set(format!("解除失败：{err}")),
            }
        });
    });

    let load_bans = move || {
        let base = api_base.read().clone();
        let jwt = token.read().clone();
        let csrf = csrf_token.read().clone();
        let mut bans_sig = bans.clone();
        let mut status = status.clone();
        if jwt.trim().is_empty() {
            status.set("请先登录/粘贴管理员 JWT".into());
            return;
        }
        spawn(async move {
            let client = build_client(&base, &jwt, &csrf);
            match crate::services::admin::load_bans(&client).await {
                Ok(resp) => {
                    bans_sig.set(resp.bans);
                    status.set("封禁列表已刷新".into());
                }
                Err(err) => status.set(format!("刷新封禁失败：{err}")),
            }
        });
    };

    let load_admin_users = move || {
        let base = api_base.read().clone();
        let jwt = token.read().clone();
        let csrf = csrf_token.read().clone();
        let q = admin_user_query.read().clone();
        let mut status = status.clone();
        let mut admin_users = admin_users.clone();
        if jwt.trim().is_empty() {
            status.set("请先登录/粘贴管理员 JWT".into());
            return;
        }
        spawn(async move {
            let client = build_client(&base, &jwt, &csrf);
            match crate::services::admin::load_admin_users(&client, Some(&q)).await {
                Ok(resp) => {
                    admin_users.set(resp.members);
                    status.set("用户列表已刷新".into());
                }
                Err(err) => status.set(format!("加载用户失败：{err}")),
            }
        });
    };

    let load_admin_accounts = move || {
        let base = api_base.read().clone();
        let jwt = token.read().clone();
        let csrf = csrf_token.read().clone();
        let mut status = status.clone();
        let mut admin_accounts = admin_accounts.clone();
        if jwt.trim().is_empty() {
            status.set("请先登录/粘贴管理员 JWT".into());
            return;
        }
        spawn(async move {
            let client = build_client(&base, &jwt, &csrf);
            match crate::services::admin::load_admin_accounts(&client).await {
                Ok(resp) => {
                    admin_accounts.set(resp.admins);
                    status.set("管理员列表已刷新".into());
                }
                Err(err) => status.set(format!("加载管理员失败：{err}")),
            }
        });
    };

    let assign_moderator = Rc::new(move |record_id: String| {
        let base = api_base.read().clone();
        let jwt = token.read().clone();
        let csrf = csrf_token.read().clone();
        let q = admin_user_query.read().clone();
        let mut status = status.clone();
        let mut admin_users_sig = admin_users.clone();
        let mut admin_accounts_sig = admin_accounts.clone();
        if jwt.trim().is_empty() {
            status.set("请先登录/粘贴管理员 JWT".into());
            return;
        }
        if record_id.trim().is_empty() {
            status.set("无效 record_id".into());
            return;
        }
        status.set(format!("正在设置版主: {record_id}"));
        spawn(async move {
            let client = build_client(&base, &jwt, &csrf);
            let payload = ModeratorUpdateByRecordPayload { record_id };
            match crate::services::admin::assign_moderator_by_record(&client, &payload).await {
                Ok(resp) => {
                    status.set(format!(
                        "已设置版主: {} (legacy_id={} role={})",
                        resp.record_id.unwrap_or_default(),
                        resp.member_id,
                        resp.role
                    ));
                    if let Ok(users_resp) =
                        crate::services::admin::load_admin_users(&client, Some(&q)).await
                    {
                        admin_users_sig.set(users_resp.members);
                    }
                    if let Ok(admins_resp) =
                        crate::services::admin::load_admin_accounts(&client).await
                    {
                        admin_accounts_sig.set(admins_resp.admins);
                    }
                }
                Err(err) => status.set(format!("设置版主失败：{err}")),
            }
        });
    });

    let revoke_moderator = Rc::new(move |record_id: String| {
        let base = api_base.read().clone();
        let jwt = token.read().clone();
        let csrf = csrf_token.read().clone();
        let q = admin_user_query.read().clone();
        let mut status = status.clone();
        let mut admin_users_sig = admin_users.clone();
        let mut admin_accounts_sig = admin_accounts.clone();
        if jwt.trim().is_empty() {
            status.set("请先登录/粘贴管理员 JWT".into());
            return;
        }
        if record_id.trim().is_empty() {
            status.set("无效 record_id".into());
            return;
        }
        status.set(format!("正在取消版主: {record_id}"));
        spawn(async move {
            let client = build_client(&base, &jwt, &csrf);
            let payload = ModeratorUpdateByRecordPayload { record_id };
            match crate::services::admin::revoke_moderator_by_record(&client, &payload).await {
                Ok(resp) => {
                    status.set(format!(
                        "已取消版主: {} (legacy_id={} role={})",
                        resp.record_id.unwrap_or_default(),
                        resp.member_id,
                        resp.role
                    ));
                    if let Ok(users_resp) =
                        crate::services::admin::load_admin_users(&client, Some(&q)).await
                    {
                        admin_users_sig.set(users_resp.members);
                    }
                    if let Ok(admins_resp) =
                        crate::services::admin::load_admin_accounts(&client).await
                    {
                        admin_accounts_sig.set(admins_resp.admins);
                    }
                }
                Err(err) => status.set(format!("取消版主失败：{err}")),
            }
        });
    });

    let grant_docs_space_create = Rc::new(move |record_id: String| {
        let base = api_base.read().clone();
        let jwt = token.read().clone();
        let csrf = csrf_token.read().clone();
        let q = admin_user_query.read().clone();
        let mut status = status.clone();
        let mut admin_users_sig = admin_users.clone();
        let mut admin_accounts_sig = admin_accounts.clone();
        if jwt.trim().is_empty() {
            status.set("请先登录/粘贴管理员 JWT".into());
            return;
        }
        if record_id.trim().is_empty() {
            status.set("无效 record_id".into());
            return;
        }
        status.set(format!("正在授权 Docs 建空间: {record_id}"));
        spawn(async move {
            let client = build_client(&base, &jwt, &csrf);
            let payload = DocsPermissionGrantByRecordPayload { record_id };
            match crate::services::admin::grant_docs_space_create(&client, &payload).await {
                Ok(resp) => {
                    let suffix = if resp.already_granted {
                        "（已存在）"
                    } else {
                        "（已新增）"
                    };
                    status.set(format!(
                        "Docs 建空间权限已授予: {} -> {} {}",
                        resp.record_id, resp.auth_user_id, suffix
                    ));
                    if let Ok(users_resp) =
                        crate::services::admin::load_admin_users(&client, Some(&q)).await
                    {
                        admin_users_sig.set(users_resp.members);
                    }
                    if let Ok(admins_resp) =
                        crate::services::admin::load_admin_accounts(&client).await
                    {
                        admin_accounts_sig.set(admins_resp.admins);
                    }
                }
                Err(err) => status.set(format!("授权 Docs 建空间失败：{err}")),
            }
        });
    });

    let revoke_docs_space_create = Rc::new(move |record_id: String| {
        let base = api_base.read().clone();
        let jwt = token.read().clone();
        let csrf = csrf_token.read().clone();
        let q = admin_user_query.read().clone();
        let mut status = status.clone();
        let mut admin_users_sig = admin_users.clone();
        let mut admin_accounts_sig = admin_accounts.clone();
        if jwt.trim().is_empty() {
            status.set("请先登录/粘贴管理员 JWT".into());
            return;
        }
        if record_id.trim().is_empty() {
            status.set("无效 record_id".into());
            return;
        }
        status.set(format!("正在撤销 Docs 建空间: {record_id}"));
        spawn(async move {
            let client = build_client(&base, &jwt, &csrf);
            let payload = DocsPermissionRevokeByRecordPayload { record_id };
            match crate::services::admin::revoke_docs_space_create(&client, &payload).await {
                Ok(resp) => {
                    let suffix = if resp.already_revoked {
                        "（无需撤销）"
                    } else {
                        "（已撤销）"
                    };
                    status.set(format!(
                        "Docs 建空间权限已撤销: {} -> {} {}",
                        resp.record_id, resp.auth_user_id, suffix
                    ));
                    if let Ok(users_resp) =
                        crate::services::admin::load_admin_users(&client, Some(&q)).await
                    {
                        admin_users_sig.set(users_resp.members);
                    }
                    if let Ok(admins_resp) =
                        crate::services::admin::load_admin_accounts(&client).await
                    {
                        admin_accounts_sig.set(admins_resp.admins);
                    }
                }
                Err(err) => status.set(format!("撤销 Docs 建空间失败：{err}")),
            }
        });
    });

    let transfer_admin = move || {
        let base = api_base.read().clone();
        let jwt = token.read().clone();
        let csrf = csrf_token.read().clone();
        let target_record_raw = transfer_target_record_id.read().trim().to_string();
        let demote_self = *transfer_demote_self.read();
        let mut status = status.clone();
        let mut admin_users_sig = admin_users.clone();
        let mut admin_accounts_sig = admin_accounts.clone();
        let mut is_admin_page_sig = is_admin_page.clone();
        let mut admin_bootstrapped_sig = admin_bootstrapped.clone();
        if jwt.trim().is_empty() {
            status.set("请先登录/粘贴管理员 JWT".into());
            return;
        }
        let target_record_id = if target_record_raw.is_empty() {
            None
        } else {
            Some(target_record_raw.clone())
        };
        if target_record_id.is_none() {
            status.set("请输入目标 record_id".into());
            return;
        }
        spawn(async move {
            status.set("正在转让管理员...".into());
            let payload = AdminTransferPayload {
                target_member_id: None,
                target_record_id,
                demote_self,
            };
            let client = build_client(&base, &jwt, &csrf);
            match crate::services::admin::transfer_admin(&client, &payload).await {
                Ok(resp) => {
                    status.set(format!(
                        "管理员已转让: {} -> {}，已退出管理页",
                        resp.from_member_id, resp.to_member_id
                    ));
                    // After transfer, current account may immediately lose admin access.
                    // Exit admin view to prevent noisy 403 polling/errors.
                    admin_users_sig.set(Vec::new());
                    admin_accounts_sig.set(Vec::new());
                    admin_bootstrapped_sig.set(false);
                    is_admin_page_sig.set(false);
                }
                Err(err) => status.set(format!("管理员转让失败：{err}")),
            }
        });
    };

    let is_admin = *is_admin_page.read();
    let is_register = *is_register_page.read();
    let is_login = *is_login_page.read();
    let is_logged_in = !token.read().trim().is_empty();
    let current_jwt = token.read().trim().to_string();
    let blog_link = build_cross_app_link("/blog", Some(&current_jwt), "/");
    let docs_link = build_cross_app_link("/docs", Some(&current_jwt), "/dashboard");
    let display_name = current_user.read().trim().to_string();
    let display_name = if display_name.is_empty() {
        "Member".to_string()
    } else {
        display_name
    };
    let current_member_label = if is_logged_in {
        display_name.clone()
    } else {
        "Guest".to_string()
    };
    let welcome_text = if is_logged_in {
        format!("Welcome, {}.", display_name)
    } else {
        "Welcome, Guest. Please login or register.".to_string()
    };
    let selected_board_id_for_feed = selected_board.read().clone();
    let board_feed_mode_value = *board_feed_mode.read();
    let board_metrics_map = board_feed_metrics.read().clone();
    let mut home_feed_boards = boards.read().clone();
    home_feed_boards.sort_by(|a, b| match board_feed_mode_value {
        BoardFeedMode::Realtime => board_last_activity_at(b, &board_metrics_map)
            .cmp(&board_last_activity_at(a, &board_metrics_map))
            .then_with(|| board_created_at(b).cmp(&board_created_at(a)))
            .then_with(|| a.name.cmp(&b.name)),
        BoardFeedMode::New => board_created_at(b)
            .cmp(&board_created_at(a))
            .then_with(|| a.name.cmp(&b.name)),
        BoardFeedMode::Top => {
            let a_topics =
                a.id.as_ref()
                    .and_then(|id| board_metrics_map.get(id))
                    .map(|m| m.topic_count)
                    .unwrap_or(0);
            let b_topics =
                b.id.as_ref()
                    .and_then(|id| board_metrics_map.get(id))
                    .map(|m| m.topic_count)
                    .unwrap_or(0);
            b_topics
                .cmp(&a_topics)
                .then_with(|| {
                    board_last_activity_at(b, &board_metrics_map)
                        .cmp(&board_last_activity_at(a, &board_metrics_map))
                })
                .then_with(|| a.name.cmp(&b.name))
        }
        BoardFeedMode::Discussed => {
            let a_topics =
                a.id.as_ref()
                    .and_then(|id| board_metrics_map.get(id))
                    .map(|m| m.topic_count)
                    .unwrap_or(0);
            let b_topics =
                b.id.as_ref()
                    .and_then(|id| board_metrics_map.get(id))
                    .map(|m| m.topic_count)
                    .unwrap_or(0);
            let a_score = (a_topics as i64) * 1000
                + board_last_activity_at(a, &board_metrics_map).len() as i64;
            let b_score = (b_topics as i64) * 1000
                + board_last_activity_at(b, &board_metrics_map).len() as i64;
            b_score
                .cmp(&a_score)
                .then_with(|| b_topics.cmp(&a_topics))
                .then_with(|| a.name.cmp(&b.name))
        }
    });

    let points_member_label = current_member_label.clone();
    let selected_board_name_for_detail = boards
        .read()
        .iter()
        .find(|board| board.id.as_deref() == Some(selected_board.read().as_str()))
        .map(|board| board.name.clone())
        .unwrap_or_default();

    let mut logout = move || {
        clear_auth_storage();
        token.set("".into());
        current_user.set("".into());
        current_member_id.set(None);
        pm_disabled.set(false);
        pm_forbidden_notified.set(false);
        admin_bootstrapped.set(false);
        is_admin_page.set(false);
        is_register_page.set(false);
        is_login_page.set(false);
        status.set("已登出".into());
    };

    use_effect(move || {
        let current_member_label = points_member_label.clone();
        let jwt = token.read().clone();
        let csrf = csrf_token.read().clone();
        let base = api_base.read().clone();
        if jwt.trim().is_empty() {
            current_points_snapshot.set(preview_points_snapshot(&current_member_label));
            return;
        }

        current_points_snapshot.set(crate::components::points::PointsSnapshot::pending());
        let mut current_points_snapshot = current_points_snapshot.clone();
        let mut status = status.clone();
        spawn(async move {
            let client = build_client(&base, &jwt, &csrf);
            match crate::services::points::load_my_points(&client).await {
                Ok(PointsBalanceResponse { balance, .. }) => {
                    current_points_snapshot
                        .set(crate::components::points::PointsSnapshot::from_balance(&balance));
                }
                Err(err) => {
                    current_points_snapshot.set(preview_points_snapshot(&current_member_label));
                    status.set(format!("加载积分失败，已回退 preview：{err}"));
                }
            }
        });
    });

    use_effect(move || {
        let is_admin_now = *is_admin_page.read();
        let is_register_now = *is_register_page.read();
        let is_login_now = *is_login_page.read();

        if is_admin_now && !*admin_bootstrapped.read() {
            admin_bootstrapped.set(true);
            load_boards();
            load_admin_users();
            load_admin_accounts();
            load_access();
            load_permissions();
            load_bans();
        }

        if !*boards_checked.read() && !is_admin_now && !is_register_now && !is_login_now {
            boards_checked.set(true);
            load_boards();
        }
    });

    rsx! {
        style { {STYLE} }
        div { class: if *show_topic_detail.read() { "app-shell app-shell--detail" } else { "app-shell" },
            crate::components::nav::TopNav {
                is_admin,
                is_register,
                is_login,
                is_logged_in,
                welcome_text: welcome_text.clone(),
                blog_link: blog_link.clone(),
                docs_link: docs_link.clone(),
                on_home: move |_| {
                    admin_bootstrapped.set(false);
                    is_admin_page.set(false);
                    is_register_page.set(false);
                    is_login_page.set(false);
                    show_topic_detail.set(false);
                    replace_browser_path("/");
                },
                on_login: move |_| {
                    admin_bootstrapped.set(false);
                    is_admin_page.set(false);
                    is_register_page.set(false);
                    is_login_page.set(true);
                    replace_browser_path("/login");
                },
                on_register: move |_| {
                    admin_bootstrapped.set(false);
                    is_admin_page.set(false);
                    is_register_page.set(true);
                    is_login_page.set(false);
                    replace_browser_path("/register");
                },
                on_admin: move |_| {
                    admin_bootstrapped.set(false);
                    is_admin_page.set(true);
                    is_register_page.set(false);
                    is_login_page.set(false);
                    replace_browser_path("/admin");
                },
                on_logout: move |_| logout(),
            }

            div { class: "status-bar",
                div { "状态({BUILD_TAG})：{status.read()}" }
            }

            {if is_login && !is_admin { rsx! {
                crate::pages::login::LoginPage {
                    login_username,
                    login_password,
                    on_login: move |_| login(),
                }
            }} else if is_register && !is_admin { rsx! {
                crate::pages::register::RegisterPage {
                    register_username,
                    register_password,
                    register_confirm,
                    register_feedback: status.read().clone(),
                    on_register: move |_| register(),
                }
            }} else if !is_admin { rsx! {
                crate::pages::home::HomePage {
                    api_base,
                    token,
                    status,
                    csrf_token,
                    boards_len: boards.read().len(),
                    topics_len: topics.read().len(),
                    posts_len: posts.read().len(),
                    current_member_label: current_member_label.clone(),
                    points_snapshot: current_points_snapshot.read().clone(),
                    points_hint: if current_points_snapshot.read().backend_ready {
                        "实时读取当前用户真实 Karma / Merit。".to_string()
                    } else {
                        "当前为 preview 占位；登录后会自动读取真实 Karma / Merit。".to_string()
                    },
                    on_load_boards: move |_| load_boards(),
                    on_check_health: move |_| check_health(),
                    on_clear_token: move |_| {
                        token.set("".into());
                        save_token_to_storage("");
                        status.set("已清空本地 token".into());
                    },
                    on_sync_csrf: move |_| {
                        let csrf = read_csrf_cookie().unwrap_or_default();
                        csrf_token.set(csrf.clone());
                        status.set("已同步 CSRF".into());
                    },
                    on_open_points: move |_| {
                        status.set(points_help_text(&current_points_snapshot.read()));
                    },
                }

                {if *show_topic_detail.read() { rsx! {
                    crate::pages::topic_detail::TopicDetailPage {
                        status,
                        show_topic_detail,
                        focused_post_id,
                        selected_board,
                        selected_topic,
                        selected_board_name: selected_board_name_for_detail,
                        topics,
                        posts,
                        new_post_body,
                        on_back: move |_| {
                            show_topic_detail.set(false);
                            replace_browser_path("/");
                        },
                        on_select_topic: move |topic_id: String| {
                            selected_topic.set(topic_id.clone());
                            replace_browser_path(&forum_path(&selected_board.read(), &topic_id));
                            load_posts();
                        },
                        on_cancel_comment: move |_| {
                            new_post_body.set("".into());
                            status.set("已清空评论输入".into());
                        },
                        on_submit_comment: move |_| {
                            let board_id = selected_board.read().clone();
                            let topic_id = selected_topic.read().clone();
                            let body = new_post_body.read().clone();
                            let base = api_base.read().clone();
                            let jwt = token.read().clone();
                            let csrf = csrf_token.read().clone();
                            let mut status = status.clone();
                            let mut posts = posts.clone();
                            let mut new_post_body = new_post_body.clone();
                            if board_id.is_empty() || topic_id.is_empty() {
                                status.set("请先选择一个主题".into());
                                return;
                            }
                            if body.trim().is_empty() {
                                status.set("评论内容不能为空".into());
                                return;
                            }
                            spawn(async move {
                                status.set("评论发送中...".into());
                                let payload = CreatePostPayload {
                                    topic_id: topic_id.clone(),
                                    board_id: board_id.clone(),
                                    subject: None,
                                    body: body.clone(),
                                };
                                let client = build_client(&base, &jwt, &csrf);
                                match crate::services::forum::create_post(&client, &payload).await {
                                    Ok(resp) => {
                                        posts.set({ let mut next = posts.read().clone(); next.push(resp.post); next });
                                        new_post_body.set(String::new());
                                        status.set("评论已发布".into());
                                    }
                                    Err(err) => status.set(format!("评论失败：{err}")),
                                }
                            });
                        },
                        on_open_points_help: move |author: String| {
                            status.set(format!("{author} 的 Karma / Merit 目前为前端 preview，待后端积分接口接入后展示真实值。"));
                        },
                        on_reward_author: move |author: String| {
                            status.set(format!("已触发给 {author} 的 +1 Merit 入口（当前仅前端占位，未写入后端）。"));
                        },
                    }
                }} else { rsx! {
                    section { class: "forum-feed-layout",
                        div { class: "panel forum-feed-main",
                            div { class: "forum-feed-header",
                                div { class: "forum-feed-header__left",
                                    strong { "Posts" }
                                    span { class: "forum-feed-live", "LIVE" }
                                    span { class: "forum-feed-live-meta", "just now" }
                                }
                                div { class: "forum-feed-tabs",
                                    button {
                                        class: if board_feed_mode_value == BoardFeedMode::Realtime { "forum-feed-tab forum-feed-tab--active" } else { "forum-feed-tab" },
                                        onclick: move |_| board_feed_mode.set(BoardFeedMode::Realtime),
                                        "Realtime"
                                    }
                                    button {
                                        class: if board_feed_mode_value == BoardFeedMode::New { "forum-feed-tab forum-feed-tab--active" } else { "forum-feed-tab" },
                                        onclick: move |_| board_feed_mode.set(BoardFeedMode::New),
                                        "New"
                                    }
                                    button {
                                        class: if board_feed_mode_value == BoardFeedMode::Top { "forum-feed-tab forum-feed-tab--active" } else { "forum-feed-tab" },
                                        onclick: move |_| board_feed_mode.set(BoardFeedMode::Top),
                                        "Top"
                                    }
                                    button {
                                        class: if board_feed_mode_value == BoardFeedMode::Discussed { "forum-feed-tab forum-feed-tab--active" } else { "forum-feed-tab" },
                                        onclick: move |_| board_feed_mode.set(BoardFeedMode::Discussed),
                                        "Discussed"
                                    }
                                }
                            }
                            div { class: "forum-feed-banner",
                                span { "Hot Right Now" }
                                small { "进入版块即可查看主题并直接回复" }
                            }
                            div { class: "forum-feed-list",
                                {
                                    home_feed_boards.iter().cloned().enumerate().map(|(idx, b)| {
                                        let selected_id = selected_board_id_for_feed.clone();
                                        let board_id = b.id.clone().unwrap_or_default();
                                        let board_name = b.name.clone();
                                        let metric = board_metrics_map.get(&board_id).cloned().unwrap_or_default();
                                        let board_created_at = board_last_activity_at(&b, &board_metrics_map);
                                        let desc = b.description.clone().unwrap_or_else(|| "进入此版块查看主题流并参与讨论。".into());
                                        let accent_rank = match board_feed_mode_value {
                                            BoardFeedMode::Realtime => metric.topic_count * 11 + idx + 1,
                                            BoardFeedMode::New => idx + 1,
                                            BoardFeedMode::Top => metric.topic_count * 37 + 84,
                                            BoardFeedMode::Discussed => metric.topic_count * 19 + 42,
                                        };
                                        let topic_count = metric.topic_count;
                                        let feed_label = match board_feed_mode_value {
                                            BoardFeedMode::Realtime => "live now",
                                            BoardFeedMode::New => "new board",
                                            BoardFeedMode::Top => "top ranked",
                                            BoardFeedMode::Discussed => "most discussed",
                                        };
                                        rsx! {
                                            article {
                                                class: if selected_id == board_id { "forum-feed-card selected" } else { "forum-feed-card" },
                                                onclick: move |_| {
                                                    selected_board.set(board_id.clone());
                                                    selected_topic.set("".into());
                                                    topics.set(Vec::new());
                                                    posts.set(Vec::new());
                                                    show_topic_detail.set(true);
                                                    replace_browser_path(&forum_path(&board_id, ""));
                                                    load_topics_for_board(board_id.clone());
                                                },
                                                div { class: "forum-feed-card__votes",
                                                    span { class: "forum-feed-card__up", "▲" }
                                                    strong { "{accent_rank * 37 + 84}" }
                                                    span { class: "forum-feed-card__down", "▼" }
                                                }
                                                div { class: "forum-feed-card__body",
                                                    div { class: "forum-feed-card__meta",
                                                        span { class: "forum-feed-card__tag", "# {board_name}" }
                                                        span { class: "forum-feed-card__time", "{board_created_at}" }
                                                    }
                                                    h3 { "{b.name}" }
                                                    p { "{desc}" }
                                                    div { class: "forum-feed-card__footer",
                                                        span { class: "forum-feed-card__pill", "{topic_count} active topics" }
                                                        span { class: "forum-feed-card__pill", "{feed_label}" }
                                                        button { class: "ghost-btn", "进入讨论" }
                                                    }
                                                }
                                            }
                                        }
                                    })
                                }
                            }
                        }

                        aside { class: "forum-feed-side",
                            section { class: "panel forum-side-card forum-side-card--activity",
                                div { class: "forum-side-card__titlebar",
                                    strong { "Live Activity" }
                                    span { "auto-updating" }
                                }
                                div { class: "forum-side-activity-list",
                                    div { class: "forum-side-activity-item",
                                        strong { "系统状态" }
                                        p { "{status.read()}" }
                                    }
                                    div { class: "forum-side-activity-item",
                                        strong { "主题流" }
                                        p { "进入任意版块后自动加载主题，评论区直接回复。" }
                                    }
                                    div { class: "forum-side-activity-item",
                                        strong { "当前数据" }
                                        p { "版块 {boards.read().len()} · 主题 {topics.read().len()} · 帖子 {posts.read().len()}" }
                                    }
                                }
                            }

                            section { class: "panel forum-side-card",
                                div { class: "forum-side-card__subheader",
                                    strong { "Subboards" }
                                    a { href: "#", "View All" }
                                }
                                div { class: "forum-side-board-list" ,
                                    {
                                        boards.read().iter().take(5).cloned().map(|b| {
                                            let board_id = b.id.clone().unwrap_or_default();
                                            let sidebar_desc = b
                                                .description
                                                .clone()
                                                .unwrap_or_else(|| "点击进入查看讨论".into());
                                            rsx! {
                                                div {
                                                    class: "forum-side-board-item",
                                                    onclick: move |_| {
                                                        selected_board.set(board_id.clone());
                                                        selected_topic.set("".into());
                                                        topics.set(Vec::new());
                                                        posts.set(Vec::new());
                                                        show_topic_detail.set(true);
                                                        replace_browser_path(&forum_path(&board_id, ""));
                                                        load_topics_for_board(board_id.clone());
                                                    },
                                                    div { class: "forum-side-board-item__icon", "₿" }
                                                    div { class: "forum-side-board-item__body",
                                                        strong { "m/{b.name}" }
                                                        span { "{sidebar_desc}" }
                                                    }
                                                }
                                            }
                                        })
                                    }
                                }
                            }
                        }
                    }
                }}}

                section { class: "panel pm-panel",
                    div { class: "panel__header",
                        h3 { "私信中心" }
                        span { class: "muted", "自动刷新（约12秒）" }
                    }
                    div { class: "actions pm-toolbar",
                        select { value: "{pm_folder.read()}", onchange: move |evt| pm_folder.set(evt.value()), option { value: "inbox", "收件箱" } option { value: "sent", "发件箱" } }
                        span { class: "pm-count", "当前 {personal_messages.read().len()} 条 / 未读 {personal_messages.read().iter().filter(|pm| !pm.is_read).count()} 条" }
                        button { class: "ghost-btn", onclick: move |_| load_pms(), "立即同步" }
                        button { onclick: move |_| {
                            let ids: Vec<i64> = personal_messages.read().iter().filter(|pm| !pm.is_read).map(|pm| pm.id).collect();
                            mark_pm_read(ids);
                        }, "全部标记已读" }
                        button { onclick: move |_| {
                            let ids: Vec<i64> = personal_messages.read().iter().map(|pm| pm.id).collect();
                            delete_pms(ids);
                        }, "删除全部" }
                    }
                    div { class: "grid two pm-grid",
                        div { class: "pm-list-panel",
                            h4 { if pm_folder.read().as_str() == "sent" { "发件箱" } else { "收件箱" } }
                            if personal_messages.read().is_empty() {
                                p { class: "muted", "当前没有消息，系统会自动刷新。" }
                            } else {
                                ul { class: "list list--limit4 pm-list",
                                    { personal_messages.read().iter().cloned().map(|pm| { rsx! {
                                        li { class: if pm.is_read { "item pm-item" } else { "item pm-item unread" },
                                            strong { class: "pm-subject", "{pm.subject}" }
                                            div { class: "meta", "来自: {pm.sender_name} · 时间: {pm.sent_at}" }
                                            p { "{pm.body}" }
                                            div { class: "actions",
                                                button { class: "ghost-btn", onclick: move |_| mark_pm_read(vec![pm.id]), "标记已读" }
                                                button { class: "ghost-btn", onclick: move |_| delete_pms(vec![pm.id]), "删除" }
                                            }
                                        }
                                    }}) }
                                }
                            }
                        }
                        div { class: "pm-compose-panel",
                            h4 { "发送私信" }
                            div { class: "muted", "我的 Legacy ID: {current_member_id.read().as_ref().map(|v| v.to_string()).unwrap_or_else(|| \"-\".into())}" }
                            div { class: "stack pm-compose",
                                input { value: "{pm_to.read()}", oninput: move |evt| pm_to.set(evt.value()), placeholder: "收件人用户名（多个用逗号）" }
                                input { value: "{pm_subject.read()}", oninput: move |evt| pm_subject.set(evt.value()), placeholder: "标题" }
                                textarea { value: "{pm_body.read()}", oninput: move |evt| pm_body.set(evt.value()), rows: "5", placeholder: "内容" }
                                button { onclick: move |_| send_pm(), "发送私信" }
                            }
                        }
                    }
                }
            }} else { rsx! {
                crate::pages::admin::AdminPage {
                    api_base,
                    token,
                    csrf_token,
                    status,
                    boards,
                    board_access,
                    board_permissions,
                    admin_users,
                    admin_accounts,
                    admin_user_query,
                    selected_record_id,
                    transfer_target_record_id,
                    transfer_demote_self,
                    new_board_name,
                    new_board_desc,
                    new_board_topic_subject,
                    new_board_topic_body,
                    existing_topic_board_id,
                    existing_topic_subject,
                    existing_topic_body,
                    access_board_id,
                    access_groups,
                    perm_board_id,
                    perm_group_id,
                    perm_allow,
                    perm_deny,
                    ban_member_id,
                    ban_hours,
                    ban_reason,
                    ban_cannot_post,
                    ban_cannot_access,
                    bans,
                    on_refresh_all: move |_| { load_admin_users(); load_admin_accounts(); load_access(); load_permissions(); load_bans(); },
                    on_load_access: move |_| load_access(),
                    on_load_permissions: move |_| load_permissions(),
                    on_check_health: move |_| check_health(),
                    on_load_admin_users: move |_| load_admin_users(),
                    on_load_admin_accounts: move |_| load_admin_accounts(),
                    on_assign_moderator: move |rid| (assign_moderator)(rid),
                    on_revoke_moderator: move |rid| (revoke_moderator)(rid),
                    on_grant_docs_space: move |rid| (grant_docs_space_create)(rid),
                    on_revoke_docs_space: move |rid| (revoke_docs_space_create)(rid),
                    on_transfer_admin: move |_| transfer_admin(),
                    on_create_board: move |_| create_board(),
                    on_create_topic: move |_| create_topic(),
                    on_update_access: move |_| update_access(),
                    on_update_permissions: move |_| update_permissions(),
                    on_apply_ban: move |_| apply_ban(),
                    on_load_bans: move |_| load_bans(),
                    on_revoke_ban: move |ban_id| (revoke_ban)(ban_id),
                    on_clear_token: move |_| { token.set("".into()); save_token_to_storage(""); status.set("已清空本地 token".into()); },
                    on_sync_csrf: move |_| { let csrf = read_csrf_cookie().unwrap_or_default(); csrf_token.set(csrf.clone()); status.set("已同步 CSRF".into()); },
                }

            }} }
        }
    }
}

// ---------- Styles ----------
