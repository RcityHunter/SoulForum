use dioxus::prelude::*;

const APP_BASE_PATH: &str = "/forum";

#[derive(Props, Clone, PartialEq)]
pub struct HomePageProps {
    pub api_base: Signal<String>,
    pub token: Signal<String>,
    pub status: Signal<String>,
    pub csrf_token: Signal<String>,

    pub boards_len: usize,
    pub topics_len: usize,
    pub posts_len: usize,

    pub current_member_label: String,
    pub points_snapshot: crate::components::points::PointsSnapshot,
    pub points_hint: String,

    pub on_load_boards: EventHandler<()>,
    pub on_check_health: EventHandler<()>,
    pub on_clear_token: EventHandler<()>,
    pub on_sync_csrf: EventHandler<()>,
    pub on_open_points: EventHandler<()>,
}

pub fn HomePage(mut props: HomePageProps) -> Element {
    rsx! {
        section { class: "hero",
            div { class: "hero__copy",
                span { class: "pill", "Suol · Testnet" }
                h1 { "灵魂论坛" }
                p { "直连 SurrealDB 的论坛 Demo：注册、发帖、回帖与权限全部在这里自测。" }
                div { class: "hero__actions",
                    button { onclick: move |_| props.on_load_boards.call(()), "加载版块/主题" }
                    a { class: "ghost-btn", href: "{APP_BASE_PATH}/admin", "管理后台 (/admin)" }
                }
            }
            div { class: "hero__panel",
                div { class: "stat", span { "当前 API" } strong { "{props.api_base.read()}" } }
                div { class: "stat-row",
                    div { class: "stat-box", strong { "{props.boards_len}" } span { "版块" } }
                    div { class: "stat-box", strong { "{props.topics_len}" } span { "主题" } }
                    div { class: "stat-box", strong { "{props.posts_len}" } span { "帖子" } }
                }
                crate::components::points::PointsEntry {
                    title: format!("{} 的积分", props.current_member_label),
                    snapshot: props.points_snapshot.clone(),
                    hint: props.points_hint.clone(),
                    action_label: "查看 Karma / Merit 说明".to_string(),
                    compact: Some(true),
                    on_action: move |_| props.on_open_points.call(()),
                }
            }
        }

        section { class: "panel",
            h2 { "连接配置" }
            div { class: "grid two",
                div {
                    label { "API 基址" }
                    input {
                        value: "{props.api_base.read()}",
                        oninput: move |evt| props.api_base.set(evt.value())
                    }
                    div { class: "actions",
                        button { onclick: move |_| props.status.set("已更新 API 基址".into()), "更新" }
                        button { onclick: move |_| props.on_load_boards.call(()), "加载数据" }
                        button { onclick: move |_| props.on_check_health.call(()), "健康检查" }
                    }
                }
                div {
                    label { "JWT Token" }
                    textarea {
                        value: "{props.token.read()}",
                        rows: "3",
                        oninput: move |evt| props.token.set(evt.value())
                    }
                    div { class: "actions",
                        button { onclick: move |_| props.on_clear_token.call(()), "清空 Token" }
                        button { onclick: move |_| props.on_sync_csrf.call(()), "同步 CSRF" }
                    }
                }
            }
        }
    }
}
