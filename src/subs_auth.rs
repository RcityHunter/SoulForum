use crate::security::is_not_banned;
use crate::services::{ForumContext, ForumError, ForumService, MemberRecord, ServiceResult};
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use serde_json::json;

pub fn rebuild_mod_cache<S: ForumService>(
    service: &S,
    ctx: &mut ForumContext,
) -> ServiceResult<()> {
    let boards = service.list_board_access()?;
    let memberships = &ctx.user_info.groups;
    let visible: Vec<_> = boards
        .iter()
        .filter(|board| {
            board
                .allowed_groups
                .iter()
                .any(|group| memberships.contains(group))
        })
        .map(|board| board.id.clone())
        .collect();
    ctx.session.set(
        "mod_cache",
        json!({
            "time": service.settings_last_updated(),
            "id": ctx.user_info.id,
            "boards": visible,
        }),
    );
    Ok(())
}

/// Hash a password for storage (Argon2id).
pub fn hash_password(password: &str) -> ServiceResult<String> {
    if password.trim().len() < 6 {
        return Err(ForumError::Validation("password_too_short".into()));
    }
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| ForumError::Internal(format!("hash_password failed: {e}")))?
        .to_string();
    Ok(hash)
}

fn verify_password_hash(password: &str, stored: &str) -> bool {
    if stored.is_empty() {
        return false;
    }
    if stored.starts_with("$argon2") {
        if let Ok(parsed) = PasswordHash::new(stored) {
            return Argon2::default()
                .verify_password(password.as_bytes(), &parsed)
                .is_ok();
        }
    }
    password == stored
}

pub fn validate_login_password<S: ForumService>(
    service: &S,
    ctx: &mut ForumContext,
    username: &str,
    password: &str,
) -> ServiceResult<()> {
    let member = service
        .find_member_by_name(username)?
        .ok_or_else(|| ForumError::PermissionDenied("unknown_user".into()))?;
    if !verify_password_hash(password, &member.password) {
        return Err(ForumError::PermissionDenied("bad_password".into()));
    }
    load_member_into_context(ctx, &member);
    is_not_banned(service, ctx, true)?;
    Ok(())
}

pub fn load_user_settings<S: ForumService>(
    service: &S,
    ctx: &mut ForumContext,
    username: &str,
) -> ServiceResult<()> {
    let member = service
        .find_member_by_name(username)?
        .ok_or_else(|| ForumError::PermissionDenied("unknown_user".into()))?;
    load_member_into_context(ctx, &member);
    is_not_banned(service, ctx, false)?;
    Ok(())
}

fn load_member_into_context(ctx: &mut ForumContext, member: &MemberRecord) {
    ctx.user_info.id = member.id;
    ctx.user_info.name = member.name.clone();
    ctx.user_info.groups = member
        .primary_group
        .into_iter()
        .chain(member.additional_groups.iter().copied())
        .collect();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{ForumContext, InMemoryService};

    #[test]
    fn validate_login_success() {
        let service = InMemoryService::default();
        let mut ctx = ForumContext::default();
        validate_login_password(&service, &mut ctx, "Alice", "password1").unwrap();
        assert_eq!(ctx.user_info.id, 1);
    }

    #[test]
    fn validate_login_failure() {
        let service = InMemoryService::default();
        let mut ctx = ForumContext::default();
        let result = validate_login_password(&service, &mut ctx, "Alice", "wrong");
        assert!(result.is_err());
    }
}
