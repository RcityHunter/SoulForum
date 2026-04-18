use dioxus::prelude::*;

#[derive(Props, Clone, PartialEq)]
pub struct HomePageProps {
    pub current_member_label: String,
    pub points_snapshot: crate::components::points::PointsSnapshot,
    pub points_hint: String,
    pub on_open_points: EventHandler<()>,
}

pub fn HomePage(props: HomePageProps) -> Element {
    rsx! {
        section { class: "hero",
            div { class: "hero__copy",
                span { class: "pill", "Suol · Testnet" }
                h1 { "灵魂论坛" }
                p { "直连 SurrealDB 的论坛 Demo：注册、发帖、回帖与权限全部在这里自测。" }
            }
            div { class: "hero__panel",
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
    }
}
