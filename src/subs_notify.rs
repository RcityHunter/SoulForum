use crate::services::{ForumError, ForumService, ServiceResult};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::collections::HashMap;

type HmacSha256 = Hmac<Sha256>;

pub fn get_notify_prefs<S: ForumService>(
    service: &S,
    members: &[i64],
    prefs: Option<&[String]>,
    process_default: bool,
) -> ServiceResult<HashMap<i64, HashMap<String, i32>>> {
    let mut ids = members.to_vec();
    if !ids.contains(&0) {
        ids.push(0);
    }
    let raw = service.fetch_alert_prefs(&ids, prefs)?;
    if process_default {
        if let Some(defaults) = raw.get(&0) {
            let mut merged = HashMap::new();
            for id in members {
                let mut map = defaults.clone();
                if let Some(user) = raw.get(id) {
                    map.extend(user.clone());
                }
                merged.insert(*id, map);
            }
            return Ok(merged);
        }
    }
    Ok(raw)
}

pub fn set_notify_prefs<S: ForumService>(
    service: &S,
    member_id: i64,
    prefs: &[(String, i32)],
) -> ServiceResult<()> {
    if prefs.is_empty() {
        return Ok(());
    }
    let clamped: Vec<_> = prefs
        .iter()
        .map(|(key, value)| {
            let mut val = *value;
            val = val.clamp(-128, 127);
            (key.clone(), val)
        })
        .collect();
    service.set_alert_prefs(member_id, &clamped)
}

pub fn delete_notify_prefs<S: ForumService>(
    service: &S,
    member_id: i64,
    prefs: &[String],
) -> ServiceResult<()> {
    if prefs.is_empty() {
        return Ok(());
    }
    service.delete_alert_prefs(member_id, prefs)
}

pub fn get_member_with_token<S: ForumService>(
    service: &S,
    member_id: i64,
    email: Option<String>,
    token: &str,
    notif_type: &str,
    item_id: i64,
) -> ServiceResult<(i64, String)> {
    if member_id <= 0 || token.is_empty() {
        return Err(ForumError::Validation("unsubscribe_invalid".into()));
    }
    let email = if let Some(mail) = email {
        mail
    } else {
        service
            .get_member_email(member_id)?
            .ok_or_else(|| ForumError::Validation("member_not_found".into()))?
    };
    let secret = service.notification_secret()?;
    let expected = create_unsubscribe_token(&secret, member_id, &email, notif_type, item_id);
    if token != expected {
        return Err(ForumError::Validation("unsubscribe_invalid".into()));
    }
    Ok((member_id, email))
}

pub fn create_unsubscribe_token(
    secret: &str,
    member_id: i64,
    email: &str,
    notif_type: &str,
    item_id: i64,
) -> String {
    let message = format!("{} {} {} {}", member_id, email, notif_type, item_id);
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC valid");
    mac.update(message.as_bytes());
    let mut bytes = mac.finalize().into_bytes().to_vec();
    bytes.truncate(10);
    URL_SAFE_NO_PAD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::InMemoryService;

    #[test]
    fn notify_pref_inheritance() {
        let service = InMemoryService::default();
        let prefs = get_notify_prefs(&service, &[1], None, true).unwrap();
        assert!(prefs.get(&1).unwrap().contains_key("notify_board"));
    }

    #[test]
    fn token_roundtrip() {
        let service = InMemoryService::default();
        let secret = service.notification_secret().unwrap();
        let token = create_unsubscribe_token(&secret, 1, "alice@example.com", "board", 1);
        get_member_with_token(
            &service,
            1,
            Some("alice@example.com".into()),
            &token,
            "board",
            1,
        )
        .unwrap();
    }

    #[test]
    fn set_prefs_clamps_values() {
        let service = InMemoryService::default();
        set_notify_prefs(&service, 1, &[("custom_pref".into(), 500)]).unwrap();
        let prefs = get_notify_prefs(&service, &[1], Some(&["custom_pref".into()]), true).unwrap();
        assert_eq!(prefs.get(&1).unwrap().get("custom_pref"), Some(&127));
    }

    #[test]
    fn invalid_token_rejected() {
        let service = InMemoryService::default();
        let result = get_member_with_token(
            &service,
            1,
            Some("alice@example.com".into()),
            "invalid",
            "board",
            1,
        );
        assert!(matches!(result, Err(ForumError::Validation(_))));
    }
}
