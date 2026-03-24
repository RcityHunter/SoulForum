use crate::services::{ForumContext, ServiceResult};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct PmSpamSettings {
    pub max_recipients: i64,
    pub verification_posts: i64,
    pub per_hour: i64,
}

pub fn save_pm_spam_settings(
    ctx: &mut ForumContext,
    settings: PmSpamSettings,
) -> ServiceResult<()> {
    ctx.mod_settings
        .set("max_pm_recipients", settings.max_recipients);
    ctx.mod_settings
        .set("pm_posts_verification", settings.verification_posts);
    ctx.mod_settings.set("pm_posts_per_hour", settings.per_hour);
    ctx.mod_settings.set(
        "pm_spam_settings",
        format!(
            "{},{},{}",
            settings.max_recipients, settings.verification_posts, settings.per_hour
        ),
    );
    Ok(())
}

pub fn load_pm_spam_settings(ctx: &ForumContext) -> PmSpamSettings {
    if let Some(stored) = ctx.mod_settings.string("pm_spam_settings") {
        let parts: Vec<_> = stored.split(',').collect();
        if parts.len() == 3 {
            return PmSpamSettings {
                max_recipients: parts[0].parse().unwrap_or(0),
                verification_posts: parts[1].parse().unwrap_or(0),
                per_hour: parts[2].parse().unwrap_or(0),
            };
        }
    }
    PmSpamSettings {
        max_recipients: ctx.mod_settings.int("max_pm_recipients").unwrap_or(0),
        verification_posts: ctx.mod_settings.int("pm_posts_verification").unwrap_or(0),
        per_hour: ctx.mod_settings.int("pm_posts_per_hour").unwrap_or(0),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MentionSettings {
    pub enable_mentions: bool,
    pub email_notify_by_default: bool,
}

impl Default for MentionSettings {
    fn default() -> Self {
        Self {
            enable_mentions: true,
            email_notify_by_default: false,
        }
    }
}

pub fn save_mention_settings(
    ctx: &mut ForumContext,
    settings: MentionSettings,
) -> ServiceResult<()> {
    ctx.mod_settings
        .set("enable_mentions", settings.enable_mentions);
    ctx.mod_settings
        .set("mention_email_notify", settings.email_notify_by_default);
    Ok(())
}

pub fn load_mention_settings(ctx: &ForumContext) -> MentionSettings {
    MentionSettings {
        enable_mentions: ctx.mod_settings.bool("enable_mentions"),
        email_notify_by_default: ctx.mod_settings.bool("mention_email_notify"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::ForumContext;

    #[test]
    fn save_and_load_pm_settings() {
        let mut ctx = ForumContext::default();
        let settings = PmSpamSettings {
            max_recipients: 5,
            verification_posts: 10,
            per_hour: 20,
        };
        save_pm_spam_settings(&mut ctx, settings).unwrap();
        let loaded = load_pm_spam_settings(&ctx);
        assert_eq!(loaded, settings);
    }

    #[test]
    fn mention_settings_roundtrip() {
        let mut ctx = ForumContext::default();
        let settings = MentionSettings {
            enable_mentions: false,
            email_notify_by_default: true,
        };
        save_mention_settings(&mut ctx, settings).unwrap();
        assert_eq!(load_mention_settings(&ctx), settings);
    }
}
