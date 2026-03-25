use crate::services::ForumContext;

pub fn apply_language(ctx: &mut ForumContext, lang: &str) {
    let pack = ActiveLanguage::detect(ctx);
    match lang {
        "ManageSettings" => load_manage_settings(ctx, pack),
        "ManagePermissions" => load_manage_permissions(ctx, pack),
        "index" => load_index_strings(ctx, pack),
        _ => {}
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ActiveLanguage {
    English,
    ChineseSimplified,
}

impl ActiveLanguage {
    fn detect(ctx: &ForumContext) -> Self {
        Self::from_code(ctx.user_info.language.as_str())
    }

    fn from_code(code: &str) -> Self {
        let normalized = code.trim().to_ascii_lowercase();
        if normalized.starts_with("zh") || normalized.contains("chinese") {
            Self::ChineseSimplified
        } else {
            Self::English
        }
    }

    fn pick<'a>(&self, english: &'a str, chinese: &'a str) -> &'a str {
        match self {
            ActiveLanguage::English => english,
            ActiveLanguage::ChineseSimplified => chinese,
        }
    }
}

fn load_manage_settings(ctx: &mut ForumContext, lang: ActiveLanguage) {
    ctx.txt.set(
        "enable_mentions",
        lang.pick("Enable Mentions", "Enable Mentions"),
    );
    ctx.txt.set(
        "mention_email_notify",
        lang.pick("Send email when I am mentioned", "当有人提及时给我发送邮件"),
    );
}

fn load_manage_permissions(ctx: &mut ForumContext, lang: ActiveLanguage) {
    ctx.txt.set(
        "permissiongroup_mentions",
        lang.pick("Mentions", "提及内容"),
    );
    ctx.txt.set(
        "permissionname_mention",
        lang.pick("Mention others via @name", "通过 @name 提及其他"),
    );
    ctx.txt.set(
        "permissionhelp_mention",
        lang.pick(
            "This permission allows a user to mention other users by @name. For example, user Jack could be mentioned using @Jack by a user when given this permission.",
            "此权限允许用户通过@name提及其他用户。 例如，用户在授予此权限时可以使用 @Jack 来提及用户 Jack 。",
        ),
    );
}

fn load_index_strings(ctx: &mut ForumContext, lang: ActiveLanguage) {
    ctx.txt
        .set("notification", lang.pick("Notifications", "通知"));
    ctx.txt.set("mentions", lang.pick("Mentions", "提及内容"));
    ctx.txt.set(
        "notify_topic_0_desc",
        lang.pick(
            "You will not receive any emails or alerts for this topic and it will also not show up in your unread replies and topics list. You will still receive @mentions for this topic.",
            "You will not receive any emails or alerts for this topic and it will also not show up in your unread replies and topics list. You will still receive @mentions for this topic.",
        ),
    );
    ctx.txt.set(
        "notify_topic_1_desc",
        lang.pick(
            "You will not receive any emails or alerts but only @mentions by other members.",
            "您将不会收到任何电子邮件或警报，只会收到其他成员提到的 @mention。",
        ),
    );
    ctx.txt.set(
        "notify_board_subscribed",
        lang.pick(
            "%1$s has been subscribed to new topic notifications for this board.",
            "%1$s has been subscribed to new topic notifications for this board.",
        ),
    );
    ctx.txt.set(
        "notify_board_unsubscribed",
        lang.pick(
            "%1$s has been unsubscribed from new topic notifications for this board.",
            "%1$s has been unsubscribed from new topic notifications for this board.",
        ),
    );
    ctx.txt.set(
        "notify_topic_subscribed",
        lang.pick(
            "%1$s has been subscribed to new reply notifications for this topic.",
            "%1$s has been subscribed to new reply notifications for this topic.",
        ),
    );
    ctx.txt.set(
        "notify_topic_unsubscribed",
        lang.pick(
            "%1$s has been unsubscribed from new reply notifications for this topic.",
            "%1$s has been unsubscribed from new reply notifications for this topic.",
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::ForumContext;

    #[test]
    fn sets_manage_settings_strings() {
        let mut ctx = ForumContext::default();
        apply_language(&mut ctx, "ManageSettings");
        assert_eq!(
            ctx.txt.string("enable_mentions").as_deref(),
            Some("Enable Mentions")
        );
    }

    #[test]
    fn respects_user_language_preference() {
        let mut ctx = ForumContext::default();
        ctx.user_info.language = "chinese_simplified".into();
        apply_language(&mut ctx, "index");
        assert_eq!(ctx.txt.string("mentions").as_deref(), Some("提及内容"));
        assert!(ctx
            .txt
            .string("notify_topic_0_desc")
            .unwrap()
            .contains("@mentions"));
    }

    #[test]
    fn language_detection_handles_codes() {
        assert_eq!(
            ActiveLanguage::from_code("zh_CN"),
            ActiveLanguage::ChineseSimplified
        );
        assert_eq!(ActiveLanguage::from_code("en"), ActiveLanguage::English);
    }
}
