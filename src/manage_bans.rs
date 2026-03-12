use crate::services::{
    BanAffects, BanCondition, BanLogEntry, BanRule, ForumContext, ForumError, ForumService,
    ServiceResult,
};
use chrono::{DateTime, Utc};
use serde_json::json;

pub struct BanController<S: ForumService> {
    service: S,
}

impl<S: ForumService> BanController<S> {
    pub fn new(service: S) -> Self {
        Self { service }
    }

    pub fn list_bans(&self, ctx: &mut ForumContext) -> ServiceResult<()> {
        let bans = self.service.list_ban_rules()?;
        ctx.context.set(
            "ban_list",
            bans.into_iter().map(rule_to_json).collect::<Vec<_>>(),
        );
        Ok(())
    }

    pub fn save_ban(&self, ctx: &mut ForumContext) -> ServiceResult<i64> {
        let id = ctx.request.int("id");
        let reason = ctx.post_vars.string("reason");
        let expires = ctx.post_vars.string("expires").and_then(parse_datetime);
        let emails = split_list(ctx.post_vars.string("emails"));
        let members = split_list(ctx.post_vars.string("members"));
        let ips = split_list(ctx.post_vars.string("ips"));

        let mut conditions = Vec::new();
        for email in emails {
            conditions.push(BanCondition {
                id: 0,
                reason: reason.clone(),
                expires_at: expires,
                affects: BanAffects::Email { value: email },
            });
        }
        for member in members {
            if let Ok(member_id) = member.parse::<i64>() {
                conditions.push(BanCondition {
                    id: 0,
                    reason: reason.clone(),
                    expires_at: expires,
                    affects: BanAffects::Account { member_id },
                });
            }
        }
        for ip in ips {
            conditions.push(BanCondition {
                id: 0,
                reason: reason.clone(),
                expires_at: expires,
                affects: BanAffects::Ip { value: ip },
            });
        }

        if conditions.is_empty() {
            return Err(ForumError::Validation("no_conditions".into()));
        }

        let rule = BanRule {
            id: id.unwrap_or(0),
            reason,
            expires_at: expires,
            cannot_post: true,
            cannot_access: false,
            conditions,
        };
        self.service.save_ban_rule(rule)
    }

    pub fn delete_ban(&self, rule_id: i64) -> ServiceResult<()> {
        self.service.delete_ban_rule(rule_id)
    }

    pub fn list_logs(&self, ctx: &mut ForumContext) -> ServiceResult<()> {
        let logs = self.service.ban_logs()?;
        ctx.context.set(
            "ban_logs",
            logs.into_iter().map(log_to_json).collect::<Vec<_>>(),
        );
        Ok(())
    }
}

fn rule_to_json(rule: BanRule) -> serde_json::Value {
    json!({
        "id": rule.id,
        "reason": rule.reason,
        "expires_at": rule.expires_at,
        "conditions": rule.conditions
            .into_iter()
            .map(|cond| {
                json!({
                    "id": cond.id,
                    "reason": cond.reason,
                    "expires_at": cond.expires_at,
                    "affects": cond.affects,
                })
            })
            .collect::<Vec<_>>(),
    })
}

fn log_to_json(log: BanLogEntry) -> serde_json::Value {
    json!({
        "id": log.id,
        "rule_id": log.rule_id,
        "member_id": log.member_id,
        "email": log.email,
        "timestamp": log.timestamp,
    })
}

fn split_list(value: Option<String>) -> Vec<String> {
    value
        .unwrap_or_default()
        .split(',')
        .map(|part| part.trim())
        .filter(|part| !part.is_empty())
        .map(|part| part.to_string())
        .collect()
}

fn parse_datetime(value: String) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(&value)
        .map(|dt| dt.with_timezone(&Utc))
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{ForumContext, InMemoryService};

    #[test]
    fn create_and_list_bans() {
        let service = InMemoryService::default();
        let controller = BanController::new(service.clone());
        let mut ctx = ForumContext::default();
        ctx.post_vars.set("emails", "bad@example.com");
        controller.save_ban(&mut ctx).unwrap();
        controller.list_bans(&mut ctx).unwrap();
        assert!(ctx.context.get("ban_list").is_some());
    }

    #[test]
    fn logs_are_listed() {
        let service = InMemoryService::default();
        let controller = BanController::new(service.clone());
        controller.delete_ban(999).unwrap();
        let mut ctx = ForumContext::default();
        controller.list_logs(&mut ctx).unwrap();
    }
}
