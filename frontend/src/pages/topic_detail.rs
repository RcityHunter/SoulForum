use btc_forum_shared::{Post, Topic};
use dioxus::prelude::*;

#[derive(Props, Clone, PartialEq)]
pub struct TopicDetailPageProps {
    pub status: Signal<String>,

    pub show_topic_detail: Signal<bool>,
    pub focused_post_id: Signal<String>,

    pub selected_board: Signal<String>,
    pub selected_topic: Signal<String>,
    pub selected_board_name: String,

    pub topics: Signal<Vec<Topic>>,
    pub posts: Signal<Vec<Post>>,

    pub new_post_body: Signal<String>,

    pub on_back: EventHandler<()>,
    pub on_select_topic: EventHandler<String>,
    pub on_cancel_comment: EventHandler<()>,
    pub on_submit_comment: EventHandler<()>,
    pub on_open_points_help: EventHandler<String>,
    pub on_reward_author: EventHandler<String>,
}

pub fn TopicDetailPage(mut props: TopicDetailPageProps) -> Element {
    if !*props.show_topic_detail.read() {
        return rsx! {};
    }

    let comments: Vec<Post> = props.posts.read().iter().skip(1).cloned().rev().collect();
    let comment_count = comments.len();
    let topic_count = props.topics.read().len();
    let board_name = if props.selected_board_name.trim().is_empty() {
        "板块讨论".to_string()
    } else {
        props.selected_board_name.clone()
    };
    let board_title = props
        .posts
        .read()
        .first()
        .map(|p| {
            if p.subject.trim().is_empty() {
                "Untitled".to_string()
            } else {
                p.subject.clone()
            }
        })
        .unwrap_or_else(|| props.selected_board.read().clone());
    let current_topic_title = props
        .posts
        .read()
        .first()
        .map(|p| {
            if p.subject.trim().is_empty() {
                "主题讨论".to_string()
            } else {
                p.subject.clone()
            }
        })
        .unwrap_or_else(|| "主题讨论".to_string());

    rsx! {
        section { class: "post-detail",
            button { class: "ghost-btn", onclick: move |_| props.on_back.call(()), "← 返回列表" }
            div { class: "board-header",
                p { class: "board-header__eyebrow", "{board_name}" }
                h2 { "{board_title}" }
                p { class: "meta", "查看主题正文，并在下方继续回复讨论。" }
                {
                    if topic_count > 1 {
                        rsx! {
                            div { class: "topic-chips",
                                { props.topics.read().iter().cloned().map(|topic| {
                                    let topic_id = topic.id.clone().unwrap_or_default();
                                    let is_active = props.selected_topic.read().clone() == topic_id;
                                    rsx! {
                                        button {
                                            class: if is_active { "topic-chip active" } else { "topic-chip" },
                                            onclick: move |_| props.on_select_topic.call(topic_id.clone()),
                                            "{topic.subject}"
                                        }
                                    }
                                })}
                            }
                        }
                    } else {
                        rsx! {}
                    }
                }
            }

            div { class: "detail-main",
                div { class: "detail-left",
                    {props.posts.read().first().map(|main| {
                        let title = if main.subject.trim().is_empty() {
                            "Untitled".to_string()
                        } else {
                            main.subject.clone()
                        };
                        let points = crate::components::points::PointsSnapshot::seeded(&main.author);
                        let author_name = main.author.clone();
                        let reward_name = main.author.clone();
                        rsx! {
                            article { class: "post-card",
                                div { class: "post-header",
                                    span { class: "pill", "{current_topic_title}" }
                                    span { class: "meta", "{board_name}" }
                                }
                                h2 { "{title}" }
                                div { class: "meta", "作者: {main.author} | 时间: {main.created_at.clone().unwrap_or_default()}" }
                                crate::components::points::PointsEntry {
                                    title: format!("{} · Reputation", main.author),
                                    snapshot: points,
                                    hint: "主帖作者积分位已预留；当前为 preview，后续可接后端真实 Karma / Merit。".to_string(),
                                    action_label: "积分规则".to_string(),
                                    compact: Some(true),
                                    on_action: move |_| props.on_open_points_help.call(author_name.clone()),
                                }
                                p { "{main.body}" }
                                div { class: "post-actions",
                                    button { class: "ghost-btn", onclick: move |_| props.status.set("已复制主帖链接（占位）".into()), "复制链接" }
                                    button { class: "ghost-btn", onclick: move |_| props.status.set("已收藏（占位）".into()), "收藏" }
                                    button { class: "ghost-btn", onclick: move |_| props.on_reward_author.call(reward_name.clone()), "+1 Merit" }
                                }
                            }
                        }
                    }).unwrap_or_else(|| rsx! {})}

                    div { class: "comment-block" ,
                        div { class: "comment-compose",
                            p { class: "meta", "Join the conversation" }
                            textarea {
                                value: "{props.new_post_body.read()}",
                                oninput: move |evt| props.new_post_body.set(evt.value()),
                                rows: "4",
                                placeholder: "Write your comment..."
                            }
                            div { class: "compose-tools",
                                span { "😀" }
                                span { "GIF" }
                                span { "Aa" }
                            }
                            div { class: "actions compose-actions",
                                button { class: "ghost-btn", onclick: move |_| props.on_cancel_comment.call(()), "Cancel" }
                                button { onclick: move |_| props.on_submit_comment.call(()), "Comment" }
                            }
                        }
                        h3 { class: "comment-title", "Comments ({comment_count})" }
                    }

                    ul { class: "comment-list",
                        { comments.into_iter().map(|post| {
                            let is_focused = props.focused_post_id.read().clone() == post.id.clone().unwrap_or_default();
                            let points = crate::components::points::PointsSnapshot::seeded(&post.author);
                            let author_name = post.author.clone();
                            let reward_name = post.author.clone();
                            rsx! {
                                li { class: if is_focused { "comment-card focused" } else { "comment-card" },
                                    div { class: "comment-card__avatar", "{post.author.chars().next().unwrap_or('U')}" }
                                    div { class: "comment-card__content",
                                        div { class: "comment-meta comment-meta--stack",
                                            div { class: "comment-meta__main",
                                                strong { "{post.author}" }
                                                span { "· {post.created_at.clone().unwrap_or_default()}" }
                                            }
                                            crate::components::points::PointsBadge { snapshot: points, compact: Some(true) }
                                        }
                                        p { "{post.body}" }
                                        div { class: "comment-card__actions",
                                            button { class: "ghost-btn", onclick: move |_| props.on_reward_author.call(reward_name.clone()), "+1 Merit" }
                                            button { class: "ghost-btn", onclick: move |_| props.on_open_points_help.call(author_name.clone()), "查看积分" }
                                        }
                                    }
                                }
                            }
                        })}
                    }
                    if comment_count == 0 {
                        p { class: "meta", "还没有回复，直接在上方输入评论即可开始讨论。" }
                    }
                }

            }
        }
    }
}
