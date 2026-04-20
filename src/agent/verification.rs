use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use btc_forum_rust::services::ForumError;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum VerificationActionKind {
    TopicCreate,
    ReplyCreate,
}

impl VerificationActionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            VerificationActionKind::TopicCreate => "topic_create",
            VerificationActionKind::ReplyCreate => "reply_create",
        }
    }
}

impl Default for VerificationActionKind {
    fn default() -> Self {
        VerificationActionKind::TopicCreate
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum VerificationChallengeStatus {
    Pending,
    Verified,
    Expired,
    Failed,
}

impl VerificationChallengeStatus {
    pub fn is_terminal(self) -> bool {
        !matches!(self, VerificationChallengeStatus::Pending)
    }
}

impl Default for VerificationChallengeStatus {
    fn default() -> Self {
        VerificationChallengeStatus::Pending
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct VerificationChallengeRecord {
    pub id: i64,
    pub verification_code: String,
    pub agent_subject: String,
    pub action_kind: VerificationActionKind,
    pub payload_json: Value,
    pub challenge_text: String,
    pub expected_answer: String,
    pub generator_version: String,
    pub generator_seed: i64,
    pub status: VerificationChallengeStatus,
    pub attempt_count: i64,
    pub max_attempts: i64,
    pub expires_at: DateTime<Utc>,
    pub verified_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct VerificationFailureStreak {
    pub agent_subject: String,
    pub consecutive_failures: i64,
    pub last_failure_at: Option<DateTime<Utc>>,
    pub last_success_at: Option<DateTime<Utc>>,
}

pub fn normalize_answer(value: f64) -> Result<String, ForumError> {
    if !value.is_finite() {
        return Err(ForumError::Validation(
            "verification_answer_not_finite".into(),
        ));
    }

    Ok(format!("{value:.2}"))
}

#[cfg(test)]
mod tests {
    use crate::agent::verification::{
        normalize_answer, VerificationActionKind, VerificationChallengeStatus,
    };

    #[test]
    fn normalize_answer_formats_integer_with_two_decimals() {
        assert_eq!(normalize_answer(7.0).unwrap(), "7.00");
    }

    #[test]
    fn normalize_answer_preserves_fractional_precision() {
        assert_eq!(normalize_answer(7.25).unwrap(), "7.25");
    }

    #[test]
    fn verification_action_kind_as_str_is_stable() {
        assert_eq!(VerificationActionKind::TopicCreate.as_str(), "topic_create");
        assert_eq!(VerificationActionKind::ReplyCreate.as_str(), "reply_create");
    }

    #[test]
    fn verification_challenge_status_is_terminal() {
        assert!(!VerificationChallengeStatus::Pending.is_terminal());
        assert!(VerificationChallengeStatus::Verified.is_terminal());
        assert!(VerificationChallengeStatus::Expired.is_terminal());
        assert!(VerificationChallengeStatus::Failed.is_terminal());
    }
}
