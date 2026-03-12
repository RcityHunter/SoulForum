use btc_forum_shared::{AdminAccount, AdminUser, BanRuleView, Board, BoardAccessEntry, BoardPermissionEntry};
use dioxus::prelude::*;

#[derive(Props, Clone, PartialEq)]
pub struct AdminPageProps {
    pub api_base: Signal<String>,
    pub token: Signal<String>,
    pub csrf_token: Signal<String>,
    pub status: Signal<String>,

    pub boards: Signal<Vec<Board>>,
    pub board_access: Signal<Vec<BoardAccessEntry>>,
    pub board_permissions: Signal<Vec<BoardPermissionEntry>>,

    pub admin_users: Signal<Vec<AdminUser>>,
    pub admin_accounts: Signal<Vec<AdminAccount>>,
    pub admin_user_query: Signal<String>,

    pub selected_record_id: Signal<String>,
    pub transfer_target_record_id: Signal<String>,
    pub transfer_demote_self: Signal<bool>,

    pub new_board_name: Signal<String>,
    pub new_board_desc: Signal<String>,
    pub new_board_topic_subject: Signal<String>,
    pub new_board_topic_body: Signal<String>,

    pub access_board_id: Signal<String>,
    pub access_groups: Signal<String>,

    pub perm_board_id: Signal<String>,
    pub perm_group_id: Signal<String>,
    pub perm_allow: Signal<String>,
    pub perm_deny: Signal<String>,

    pub ban_member_id: Signal<String>,
    pub ban_hours: Signal<String>,
    pub ban_reason: Signal<String>,
    pub ban_cannot_post: Signal<bool>,
    pub ban_cannot_access: Signal<bool>,
    pub bans: Signal<Vec<BanRuleView>>,

    pub on_refresh_all: EventHandler<()>,
    pub on_load_access: EventHandler<()>,
    pub on_load_permissions: EventHandler<()>,
    pub on_check_health: EventHandler<()>,

    pub on_load_admin_users: EventHandler<()>,
    pub on_load_admin_accounts: EventHandler<()>,

    pub on_assign_moderator: EventHandler<String>,
    pub on_revoke_moderator: EventHandler<String>,
    pub on_grant_docs_space: EventHandler<String>,
    pub on_revoke_docs_space: EventHandler<String>,

    pub on_transfer_admin: EventHandler<()>,

    pub on_create_board: EventHandler<()>,
    pub on_update_access: EventHandler<()>,
    pub on_update_permissions: EventHandler<()>,

    pub on_apply_ban: EventHandler<()>,
    pub on_load_bans: EventHandler<()>,
    pub on_revoke_ban: EventHandler<i64>,

    pub on_clear_token: EventHandler<()>,
    pub on_sync_csrf: EventHandler<()>,
}

pub fn AdminPage(mut props: AdminPageProps) -> Element {
    let mut ban_cannot_post = props.ban_cannot_post;
    let mut ban_cannot_access = props.ban_cannot_access;
    rsx! {
        section { class: "hero hero--admin",
            div { class: "hero__copy",
                span { class: "pill", "Admin" }
                h1 { "论坛管理后台" }
                p { "管理 SurrealDB 中的 board_access 与 board_permissions，适合站点配置与灰度测试。" }
                div { class: "hero__actions",
                    button { onclick: move |_| props.on_refresh_all.call(()), "一键刷新全部" }
                    button { onclick: move |_| props.on_load_access.call(()), "加载访问控制" }
                    button { onclick: move |_| props.on_load_permissions.call(()), "加载版块权限" }
                }
            }
            div { class: "hero__panel",
                div { class: "stat", span { "API" } strong { "{props.api_base.read()}" } }
                div { class: "stat-row",
                    div { class: "stat-box", strong { "{props.board_access.read().len()}" } span { "访问规则" } }
                    div { class: "stat-box", strong { "{props.board_permissions.read().len()}" } span { "权限规则" } }
                }
            }
        }

        section { class: "panel",
            h2 { "连接 / 凭证" }
            div { class: "grid two",
                div {
                    label { "API 基址" }
                    input { value: "{props.api_base.read()}", oninput: move |evt| props.api_base.set(evt.value()) }
                    div { class: "actions",
                        button { onclick: move |_| props.on_refresh_all.call(()), "一键刷新全部" }
                        button { onclick: move |_| props.status.set("已更新 API 基址".into()), "更新" }
                        button { onclick: move |_| props.on_load_access.call(()), "加载数据" }
                        button { onclick: move |_| props.on_check_health.call(()), "健康检查" }
                    }
                }
                div {
                    label { "JWT Token" }
                    textarea { value: "{props.token.read()}", rows: "3", oninput: move |evt| props.token.set(evt.value()) }
                    div { class: "actions",
                        button { onclick: move |_| props.on_clear_token.call(()), "清空 Token" }
                        button { onclick: move |_| props.on_sync_csrf.call(()), "同步 CSRF" }
                    }
                }
            }
        }

        section { class: "panel",
            h3 { "用户列表" }
            div { class: "grid two",
                div {
                    label { "搜索用户名" }
                    input { value: "{props.admin_user_query.read()}", oninput: move |evt| props.admin_user_query.set(evt.value()), placeholder: "输入邮箱或用户名" }
                    div { class: "actions",
                        button { onclick: move |_| props.on_load_admin_users.call(()), "刷新用户" }
                        button { onclick: move |_| props.on_load_admin_accounts.call(()), "刷新管理员" }
                    }
                }
                div {
                    h4 { "成员" }
                    ul { class: "list list--limit4",
                        { props.admin_users.read().iter().cloned().map(|member| {
                            let display_name = if member.name.trim().is_empty() {
                                format!("(unnamed #{})", member.id)
                            } else {
                                member.name.clone()
                            };
                            let record_id = member.record_id.clone().unwrap_or_default();
                            let rid_select = record_id.clone();
                            let rid_assign = record_id.clone();
                            let rid_revoke = record_id.clone();
                            let rid_grant_docs = record_id.clone();
                            let rid_revoke_docs = record_id.clone();
                            let rid_transfer = record_id.clone();
                            let auth_user_id = member.auth_user_id.clone().unwrap_or_default();
                            let groups = if member.additional_groups.is_empty() {
                                "(无)".into()
                            } else {
                                member.additional_groups.iter().map(|g| g.to_string()).collect::<Vec<_>>().join(", ")
                            };

                            rsx! {
                                li { class: "item",
                                    strong { "{display_name}" }
                                    div { class: "meta", "Legacy ID: {member.id} | Auth ID: {auth_user_id} | Record ID: {record_id}" }
                                    div { class: "meta", "主组: {member.primary_group.unwrap_or(0)} | 附加组: {groups} | 警告: {member.warning}" }
                                    div { class: "actions",
                                        button { class: "ghost-btn", onclick: move |_| {
                                            props.selected_record_id.set(rid_select.clone());
                                            props.status.set(format!("已选择用户: record={}", rid_select));
                                        }, "选择" }
                                        button { class: "ghost-btn", onclick: move |_| props.on_assign_moderator.call(rid_assign.clone()), "设为版主" }
                                        button { class: "ghost-btn", onclick: move |_| props.on_revoke_moderator.call(rid_revoke.clone()), "取消版主" }
                                        button { class: "ghost-btn", onclick: move |_| props.on_grant_docs_space.call(rid_grant_docs.clone()), "授权 Docs 建空间" }
                                        button { class: "ghost-btn", onclick: move |_| props.on_revoke_docs_space.call(rid_revoke_docs.clone()), "撤销 Docs 建空间" }
                                        button { class: "ghost-btn", onclick: move |_| {
                                            props.transfer_target_record_id.set(rid_transfer.clone());
                                            props.status.set(format!("已填入管理员转让目标: {}", rid_transfer));
                                        }, "作为转让目标" }
                                        button { class: "ghost-btn", onclick: move |_| {
                                            props.ban_member_id.set(member.id.to_string());
                                            props.status.set(format!("已填入封禁对象: {}", member.id));
                                        }, "封禁此用户" }
                                    }
                                }
                            }
                        })}
                    }

                    h4 { "管理员" }
                    ul { class: "list",
                        { props.admin_accounts.read().iter().cloned().map(|admin| {
                            let display_name = if admin.name.trim().is_empty() {
                                format!("(unnamed #{})", admin.id)
                            } else {
                                admin.name.clone()
                            };
                            let role = admin.role.clone().unwrap_or_else(|| "unknown".into());
                            let record_id = admin.record_id.clone().unwrap_or_default();
                            let auth_user_id = admin.auth_user_id.clone().unwrap_or_default();
                            let perms = if admin.permissions.is_empty() { "(无)".into() } else { admin.permissions.join(", ") };
                            rsx! {
                                li { class: "item",
                                    strong { "{display_name}" }
                                    div { class: "meta", "Legacy ID: {admin.id} | Auth ID: {auth_user_id} | Record ID: {record_id}" }
                                    div { class: "meta", "角色: {role} | 权限: {perms}" }
                                }
                            }
                        })}
                    }
                }
            }
        }

        section { class: "panel",
            h3 { "管理员 / 版主操作" }
            div { class: "stack",
                div {
                    h4 { "版主管理" }
                    label { "record_id" }
                    input { value: "{props.selected_record_id.read()}", oninput: move |evt| props.selected_record_id.set(evt.value()), placeholder: "users:xxxx 或 uuid" }
                    div { class: "actions",
                        button { onclick: move |_| props.on_assign_moderator.call(props.selected_record_id.read().trim().to_string()), "设为版主" }
                        button { onclick: move |_| props.on_revoke_moderator.call(props.selected_record_id.read().trim().to_string()), "取消版主" }
                    }
                }
                div {
                    h4 { "管理员转让" }
                    label { "目标 record_id" }
                    input { value: "{props.transfer_target_record_id.read()}", oninput: move |evt| props.transfer_target_record_id.set(evt.value()), placeholder: "users:xxxx 或 uuid" }
                    div { class: "actions",
                        button { class: "ghost-btn", onclick: move |_| {
                            let next = !*props.transfer_demote_self.read();
                            props.transfer_demote_self.set(next);
                        }, if *props.transfer_demote_self.read() { "当前：转让后降级自己" } else { "当前：转让后保留自己管理员" } }
                        button { onclick: move |_| props.on_transfer_admin.call(()), "执行管理员转让" }
                    }
                }
            }
        }

        section { class: "panel",
            h3 { "创建版块" }
            div { class: "grid two",
                div {
                    label { "版块名称" }
                    input { value: "{props.new_board_name.read()}", oninput: move |evt| props.new_board_name.set(evt.value()), placeholder: "例如: General" }
                    label { "描述 (可选)" }
                    input { value: "{props.new_board_desc.read()}", oninput: move |evt| props.new_board_desc.set(evt.value()), placeholder: "板块简介" }
                    label { "首个主题标题（必填）" }
                    input { value: "{props.new_board_topic_subject.read()}", oninput: move |evt| props.new_board_topic_subject.set(evt.value()), placeholder: "例如：欢迎来到本版块" }
                    label { "首个主题内容（必填）" }
                    textarea { value: "{props.new_board_topic_body.read()}", oninput: move |evt| props.new_board_topic_body.set(evt.value()), rows: "4", placeholder: "写下第一条帖子，用户进入后可直接回复交流" }
                    div { class: "actions",
                        button { onclick: move |_| props.on_create_board.call(()), "创建版块并发布首个主题" }
                    }
                }
                div {
                    h4 { "当前版块" }
                    ul { class: "list list--limit5",
                        { props.boards.read().iter().cloned().map(|b| {
                            let bid = b.id.clone().unwrap_or_default();
                            let bid_for_access = bid.clone();
                            let bid_for_perm = bid.clone();
                            let desc = b.description.clone().unwrap_or_default();
                            rsx! {
                                li { class: "item",
                                    strong { "{b.name}" }
                                    div { class: "meta", "{desc}" }
                                    div { class: "actions",
                                        button { class: "ghost-btn", onclick: move |_| {
                                            props.access_board_id.set(bid_for_access.clone());
                                            props.status.set(format!("已选择访问控制版块: {}", bid_for_access));
                                        }, "用于访问控制" }
                                        button { class: "ghost-btn", onclick: move |_| {
                                            props.perm_board_id.set(bid_for_perm.clone());
                                            props.status.set(format!("已选择权限版块: {}", bid_for_perm));
                                        }, "用于权限规则" }
                                    }
                                }
                            }
                        })}
                    }
                }
            }
        }

        section { class: "panel",
            h3 { "版块访问控制" }
            div { class: "grid two",
                div {
                    label { "board_id" }
                    input { value: "{props.access_board_id.read()}", oninput: move |evt| props.access_board_id.set(evt.value()), placeholder: "输入 board_id 或版块名称自动匹配" }
                    label { "允许的组 (逗号分隔)" }
                    input { value: "{props.access_groups.read()}", oninput: move |evt| props.access_groups.set(evt.value()), placeholder: "0,2,4" }
                    div { class: "meta", "组对照：0=管理员，2=版主，4=普通登录用户" }
                    div { class: "actions", button { onclick: move |_| props.on_update_access.call(()), "保存" } }
                }
                div {
                    h4 { "当前访问控制" }
                    ul { class: "list",
                        { props.board_access.read().iter().cloned().map(|entry| {
                            let groups = if entry.allowed_groups.is_empty() { "(空)".into() } else { entry.allowed_groups.iter().map(|g| g.to_string()).collect::<Vec<_>>().join(", ") };
                            let groups_text = entry.allowed_groups.iter().map(|g| g.to_string()).collect::<Vec<_>>().join(",");
                            let id = entry.id.clone();
                            rsx! {
                                li { class: "item",
                                    strong { "Board #{entry.id}" }
                                    div { class: "meta", "允许组: {groups}" }
                                    button { class: "ghost-btn", onclick: move |_| {
                                        props.access_board_id.set(id.clone());
                                        props.access_groups.set(groups_text.clone());
                                        props.status.set(format!("已载入 Board #{} 的访问控制", id));
                                    }, "编辑此规则" }
                                }
                            }
                        })}
                    }
                }
            }
        }

        section { class: "panel",
            h3 { "版块权限（高级，可选）" }
            details {
                summary { "展开高级权限设置（默认可不配置）" }
                div { class: "grid two",
                    div {
                        div { class: "meta", "用法：按“版块 + 组”设置细粒度权限。Allow=放行，Deny=禁止（逗号分隔）" }
                        div { class: "meta", "组对照：0=管理员，2=版主，4=普通登录用户" }
                        label { "board_id" }
                        input { value: "{props.perm_board_id.read()}", oninput: move |evt| props.perm_board_id.set(evt.value()), placeholder: "board_id" }
                        label { "group_id" }
                        input { value: "{props.perm_group_id.read()}", oninput: move |evt| props.perm_group_id.set(evt.value()), placeholder: "group_id" }
                        label { "Allow (逗号分隔)" }
                        input { value: "{props.perm_allow.read()}", oninput: move |evt| props.perm_allow.set(evt.value()), placeholder: "post_new,post_reply_any" }
                        label { "Deny (逗号分隔)" }
                        input { value: "{props.perm_deny.read()}", oninput: move |evt| props.perm_deny.set(evt.value()), placeholder: "manage_boards" }
                        div { class: "actions", button { onclick: move |_| props.on_update_permissions.call(()), "更新权限" } }
                    }
                    div {
                        h4 { "当前权限规则" }
                        ul { class: "list",
                            { props.board_permissions.read().iter().cloned().map(|entry| {
                                let allow = if entry.allow.is_empty() { "无".into() } else { entry.allow.join(", ") };
                                let deny = if entry.deny.is_empty() { "无".into() } else { entry.deny.join(", ") };
                                let board_id = entry.board_id.clone();
                                let group_id = entry.group_id;
                                let allow_raw = entry.allow.join(",");
                                let deny_raw = entry.deny.join(",");
                                rsx! {
                                    li { class: "item",
                                        strong { "Board #{entry.board_id} / Group #{entry.group_id}" }
                                        div { class: "meta", "Allow: {allow}" }
                                        div { class: "meta", "Deny: {deny}" }
                                        button { class: "ghost-btn", onclick: move |_| {
                                            props.perm_board_id.set(board_id.clone());
                                            props.perm_group_id.set(group_id.to_string());
                                            props.perm_allow.set(allow_raw.clone());
                                            props.perm_deny.set(deny_raw.clone());
                                            props.status.set(format!("已载入 Board #{} Group #{} 权限规则", board_id, group_id));
                                        }, "编辑此规则" }
                                    }
                                }
                            })}
                        }
                    }
                }
            }
        }

        section { class: "panel",
            h3 { "封禁（管理员）" }
            p { "快速封禁/解封用户（Legacy ID / member_id）。" }
            div { class: "grid two",
                div {
                    label { "Legacy ID（member_id）" }
                    input { value: "{props.ban_member_id.read()}", oninput: move |evt| props.ban_member_id.set(evt.value()), placeholder: "Legacy ID（数字）" }
                    label { "封禁时长（小时）" }
                    input { value: "{props.ban_hours.read()}", oninput: move |evt| props.ban_hours.set(evt.value()), placeholder: "例如 24" }
                    label { "原因（可选）" }
                    input { value: "{props.ban_reason.read()}", oninput: move |evt| props.ban_reason.set(evt.value()), placeholder: "原因" }
                    div { class: "actions",
                        label {
                            input {
                                r#type: "checkbox",
                                checked: *ban_cannot_post.read(),
                                onchange: move |_| {
                                    let next = !*ban_cannot_post.read();
                                    ban_cannot_post.set(next);
                                }
                            }
                            " 禁止发言/私信"
                        }
                        label {
                            input {
                                r#type: "checkbox",
                                checked: *ban_cannot_access.read(),
                                onchange: move |_| {
                                    let next = !*ban_cannot_access.read();
                                    ban_cannot_access.set(next);
                                }
                            }
                            " 禁止访问论坛"
                        }
                    }
                    div { class: "actions",
                        button { onclick: move |_| props.on_apply_ban.call(()), "封禁" }
                        button { onclick: move |_| props.on_load_bans.call(()), "刷新封禁列表" }
                    }
                }
                div {
                    h4 { "当前封禁" }
                    ul { class: "list",
                        { props.bans.read().iter().cloned().map(|b| {
                            let expires = b.expires_at.clone().unwrap_or_default();
                            let reason = b.reason.clone().unwrap_or_default();
                            let members = if b.members.is_empty() {
                                "无".to_string()
                            } else {
                                b.members
                                    .iter()
                                    .map(|m| if m.name.is_empty() { format!("{}", m.member_id) } else { format!("{}({})", m.name, m.member_id) })
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            };
                            rsx! {
                                li { class: "item",
                                    strong { "Ban #{b.id}" }
                                    div { class: "meta", "过期时间: {expires} | 原因: {reason}" }
                                    div { class: "meta",
                                        if b.cannot_access {
                                            "效果: 禁止访问论坛"
                                        } else if b.cannot_post {
                                            "效果: 禁止发言/私信"
                                        } else {
                                            "效果: 未指定"
                                        }
                                    }
                                    div { class: "meta", "成员: {members}" }
                                    button { class: "ghost-btn", r#type: "button", onclick: move |_| props.on_revoke_ban.call(b.id), "解除" }
                                }
                            }
                        })}
                    }
                }
            }
        }
    }
}
