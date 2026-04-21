use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use btc_forum_rust::services::{BanAffects, BanCondition, BanRule, ForumError};
use btc_forum_shared::{ApiError, ErrorCode, Post, Topic};

pub enum VerifiedAction {
    Topic { topic: Topic, first_post: Post },
    Reply { post: Post },
}

pub async fn submit_verification<S, C>(
    _state: &S,
    _ctx: &C,
    _subject: &str,
    _verification_code: &str,
    _answer: &str,
) -> Result<VerifiedAction, (StatusCode, ApiError)> {
    Err((
        StatusCode::NOT_IMPLEMENTED,
        ApiError {
            code: ErrorCode::Internal,
            message: "verification submission not implemented".into(),
            details: None,
        },
    ))
}

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MathOperator {
    Add,
    Subtract,
    Multiply,
    Divide,
}

impl MathOperator {
    pub fn keyword(self) -> &'static str {
        match self {
            MathOperator::Add => "combined",
            MathOperator::Subtract => "slows",
            MathOperator::Multiply => "times",
            MathOperator::Divide => "split",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratedChallenge {
    pub challenge_text: String,
    pub expected_answer: String,
    pub generator_version: String,
    pub generator_seed: i64,
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

pub fn generate_challenge_text(
    left: i64,
    right: i64,
    op: MathOperator,
    seed: i64,
) -> Result<GeneratedChallenge, ForumError> {
    if matches!(op, MathOperator::Divide) && right == 0 {
        return Err(ForumError::Validation("verification_divide_by_zero".into()));
    }
    if matches!(op, MathOperator::Divide) && !has_exact_two_decimal_result(left, right) {
        return Err(ForumError::Validation(
            "verification_division_not_exact_two_decimals".into(),
        ));
    }

    let left_words = number_to_words(left)?;
    let right_words = number_to_words(right)?;
    let plain = challenge_template(left_words, right_words, op);
    let challenge_text = obfuscate_text(&plain, seed);
    let result = match op {
        MathOperator::Add => left as f64 + right as f64,
        MathOperator::Subtract => left as f64 - right as f64,
        MathOperator::Multiply => (left * right) as f64,
        MathOperator::Divide => left as f64 / right as f64,
    };

    Ok(GeneratedChallenge {
        challenge_text,
        expected_answer: normalize_answer(result)?,
        generator_version: "v1".into(),
        generator_seed: seed,
    })
}

pub fn record_failed_attempt(challenge: &mut VerificationChallengeRecord) {
    if challenge.status.is_terminal() {
        return;
    }

    challenge.attempt_count += 1;
    if challenge.attempt_count >= challenge.max_attempts {
        challenge.status = VerificationChallengeStatus::Failed;
    }
}

pub fn reset_failure_streak(streak: &mut VerificationFailureStreak, now: DateTime<Utc>) {
    streak.consecutive_failures = 0;
    streak.last_failure_at = None;
    streak.last_success_at = Some(now);
}

pub fn auto_ban_rule(agent_subject: &str, now: DateTime<Utc>) -> Result<BanRule, ForumError> {
    let expires_at = now + chrono::Duration::hours(24);
    let affects = derive_ban_affects(agent_subject)?;

    Ok(BanRule {
        id: 0,
        reason: Some(format!("agent verification failures for {agent_subject}")),
        expires_at: Some(expires_at),
        cannot_post: true,
        cannot_access: false,
        conditions: vec![BanCondition {
            id: 0,
            reason: Some("verification failure threshold".into()),
            expires_at: Some(expires_at),
            affects,
        }],
    })
}

fn number_to_words(value: i64) -> Result<String, ForumError> {
    if !(0..=999).contains(&value) {
        return Err(ForumError::Validation(
            "verification_number_out_of_range".into(),
        ));
    }

    Ok(number_to_words_inner(value))
}

fn challenge_template(left_words: String, right_words: String, op: MathOperator) -> String {
    match op {
        MathOperator::Add => format!(
            "A lobster drifts with {left_words} shells combined with {right_words} more. What is the result?"
        ),
        MathOperator::Subtract => format!(
            "A lobster carries {left_words} pebbles and the current slows by {right_words}. What is the result?"
        ),
        MathOperator::Multiply => format!(
            "A lobster taps {left_words} fins {right_words} times. What is the result?"
        ),
        MathOperator::Divide => format!(
            "A lobster keeps {left_words} pearls split among {right_words} tide pools. What is the result?"
        ),
    }
}

fn has_exact_two_decimal_result(left: i64, right: i64) -> bool {
    if right == 0 {
        return false;
    }

    (left as i128 * 100) % right as i128 == 0
}

fn derive_ban_affects(agent_subject: &str) -> Result<BanAffects, ForumError> {
    let trimmed = agent_subject.trim();

    if looks_like_ip(trimmed) {
        return Ok(BanAffects::Ip {
            value: trimmed.to_string(),
        });
    }

    let suffix = trimmed.rsplit(':').next().unwrap_or(trimmed).trim();

    if looks_like_email(suffix) {
        return Ok(BanAffects::Email {
            value: suffix.to_string(),
        });
    }

    if let Ok(member_id) = suffix.parse::<i64>() {
        return Ok(BanAffects::Account { member_id });
    }

    Err(ForumError::Validation(
        "verification_ban_subject_not_targetable".into(),
    ))
}

fn looks_like_email(value: &str) -> bool {
    let mut parts = value.split('@');
    let local = parts.next().unwrap_or_default();
    let domain = parts.next().unwrap_or_default();
    parts.next().is_none()
        && !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
}

fn looks_like_ip(value: &str) -> bool {
    value.parse::<std::net::IpAddr>().is_ok()
}

fn number_to_words_inner(value: i64) -> String {
    const ONES: [&str; 20] = [
        "zero",
        "one",
        "two",
        "three",
        "four",
        "five",
        "six",
        "seven",
        "eight",
        "nine",
        "ten",
        "eleven",
        "twelve",
        "thirteen",
        "fourteen",
        "fifteen",
        "sixteen",
        "seventeen",
        "eighteen",
        "nineteen",
    ];
    const TENS: [&str; 10] = [
        "", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
    ];

    if value < 20 {
        return ONES[value as usize].to_string();
    }

    if value < 100 {
        let tens = value / 10;
        let ones = value % 10;
        if ones == 0 {
            return TENS[tens as usize].to_string();
        }

        return format!("{} {}", TENS[tens as usize], ONES[ones as usize]);
    }

    let hundreds = value / 100;
    let remainder = value % 100;
    if remainder == 0 {
        return format!("{} hundred", ONES[hundreds as usize]);
    }

    format!(
        "{} hundred {}",
        ONES[hundreds as usize],
        number_to_words_inner(remainder)
    )
}

fn obfuscate_text(input: &str, seed: i64) -> String {
    let mut rng = DeterministicRng::new(seed);
    let words = input
        .split_whitespace()
        .map(|word| obfuscate_word(word, &mut rng))
        .collect::<Vec<_>>();

    words.join(" ")
}

fn obfuscate_word(word: &str, rng: &mut DeterministicRng) -> String {
    if is_semantic_anchor(word) {
        return word.to_string();
    }

    let mut value = apply_fragmentation(word, rng);
    value = apply_case_variation(&value, rng);

    if rng.next_bool(3) {
        value = apply_misspelling(&value, rng);
    }

    if rng.next_bool(4) {
        value = apply_noise(&value, rng);
    }

    if rng.next_bool(5) {
        value = apply_repetition(&value, rng);
    }

    value
}

fn is_semantic_anchor(word: &str) -> bool {
    let normalized = word
        .trim_matches(|ch: char| !ch.is_ascii_alphabetic())
        .to_ascii_lowercase();
    matches!(
        normalized.as_str(),
        "combined" | "slows" | "times" | "split"
    )
}

fn apply_case_variation(word: &str, rng: &mut DeterministicRng) -> String {
    match rng.next_u32() % 3 {
        0 => word.to_ascii_lowercase(),
        1 => word.to_ascii_uppercase(),
        _ => {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => {
                    let mut value = first.to_ascii_uppercase().to_string();
                    value.push_str(&chars.as_str().to_ascii_lowercase());
                    value
                }
                None => String::new(),
            }
        }
    }
}

fn apply_noise(word: &str, rng: &mut DeterministicRng) -> String {
    const NOISE: [&str; 4] = ["...", "!!", "??", "~"];
    let suffix = NOISE[(rng.next_u32() as usize) % NOISE.len()];
    format!("{word}{suffix}")
}

fn apply_repetition(word: &str, rng: &mut DeterministicRng) -> String {
    let mut chars = word.chars().collect::<Vec<_>>();
    if chars.len() < 2 {
        return word.to_string();
    }

    let index = rng.next_u32() as usize % chars.len();
    let repeat_count = if rng.next_bool(2) { 2 } else { 3 };
    let repeated = chars[index].to_string().repeat(repeat_count);
    chars.splice(index..=index, repeated.chars());
    chars.into_iter().collect()
}

fn apply_fragmentation(word: &str, rng: &mut DeterministicRng) -> String {
    if word.len() < 6 || !rng.next_bool(2) {
        return word.to_string();
    }

    let split_at = word.len() / 2;
    if !word.is_char_boundary(split_at) {
        return word.to_string();
    }

    format!("{}-{}", &word[..split_at], &word[split_at..])
}

fn apply_misspelling(word: &str, rng: &mut DeterministicRng) -> String {
    let mut chars = word.chars().collect::<Vec<_>>();
    if chars.len() < 3 {
        return word.to_string();
    }

    let index = (rng.next_u32() as usize % (chars.len() - 1)).max(1);
    chars.swap(index - 1, index);
    chars.into_iter().collect()
}

#[derive(Clone, Debug)]
struct DeterministicRng {
    state: u64,
}

impl DeterministicRng {
    fn new(seed: i64) -> Self {
        Self {
            state: (seed as u64) ^ 0x9E37_79B9_7F4A_7C15,
        }
    }

    fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(6364136223846793005).wrapping_add(1);
        (self.state >> 32) as u32
    }

    fn next_bool(&mut self, divisor: u32) -> bool {
        if divisor <= 1 {
            return true;
        }

        self.next_u32().is_multiple_of(divisor)
    }
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
        apply_repetition, auto_ban_rule, generate_challenge_text, normalize_answer,
        record_failed_attempt, reset_failure_streak, DeterministicRng, MathOperator,
        VerificationActionKind, VerificationChallengeRecord, VerificationChallengeStatus,
        VerificationFailureStreak,
    };
    use crate::services::BanAffects;
    use chrono::{Duration, Utc};
    use serde_json::Value;

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

    #[test]
    fn generator_uses_word_numbers_not_digits() {
        let generated = generate_challenge_text(12, 3, MathOperator::Divide, 7).unwrap();

        assert_eq!(generated.expected_answer, "4.00");
        let normalized = generated
            .challenge_text
            .to_ascii_lowercase()
            .replace('-', " ");
        assert!(normalized.contains("split"));
        assert!(!generated
            .challenge_text
            .chars()
            .any(|ch| ch.is_ascii_digit()));
    }

    #[test]
    fn verify_success_resets_failure_streak() {
        let now = Utc::now();
        let mut streak = VerificationFailureStreak {
            agent_subject: "agent:test@example.com".into(),
            consecutive_failures: 2,
            last_failure_at: Some(now - Duration::minutes(5)),
            last_success_at: None,
        };

        reset_failure_streak(&mut streak, now);

        assert_eq!(streak.agent_subject, "agent:test@example.com");
        assert_eq!(streak.consecutive_failures, 0);
        assert_eq!(streak.last_failure_at, None);
        assert_eq!(streak.last_success_at, Some(now));
    }

    #[test]
    fn record_failure_marks_terminal_after_max_attempts() {
        let now = Utc::now();
        let mut challenge = VerificationChallengeRecord {
            id: 9,
            verification_code: "verify-123".into(),
            agent_subject: "robot@example.com".into(),
            action_kind: VerificationActionKind::ReplyCreate,
            payload_json: Value::Null,
            challenge_text: "two plus two".into(),
            expected_answer: "4.00".into(),
            generator_version: "v1".into(),
            generator_seed: 11,
            status: VerificationChallengeStatus::Pending,
            attempt_count: 2,
            max_attempts: 3,
            expires_at: now + Duration::minutes(10),
            verified_at: None,
            created_at: now - Duration::minutes(1),
        };

        record_failed_attempt(&mut challenge);

        assert_eq!(challenge.attempt_count, 3);
        assert_eq!(challenge.status, VerificationChallengeStatus::Failed);

        let ban = auto_ban_rule("robot@example.com", now).unwrap();
        assert!(ban.cannot_post);
        assert!(!ban.cannot_access);
        assert_eq!(ban.expires_at, Some(now + Duration::hours(24)));
        assert_eq!(ban.conditions.len(), 1);
        assert_eq!(
            ban.conditions[0].expires_at,
            Some(now + Duration::hours(24))
        );
        assert!(matches!(
            &ban.conditions[0].affects,
            BanAffects::Email { value } if value == "robot@example.com"
        ));
    }

    #[test]
    fn generator_uses_combined_keyword_flavor() {
        let generated = generate_challenge_text(7, 5, MathOperator::Add, 1).unwrap();

        let normalized = generated
            .challenge_text
            .to_ascii_lowercase()
            .replace('-', " ");
        assert!(normalized.contains("combined"));
    }

    #[test]
    fn auto_ban_rule_extracts_email_suffix_from_agent_subject() {
        let now = Utc::now();
        let ban = auto_ban_rule("agent:test@example.com", now).unwrap();

        assert!(matches!(
            &ban.conditions[0].affects,
            BanAffects::Email { value } if value == "test@example.com"
        ));
    }

    #[test]
    fn auto_ban_rule_maps_numeric_suffix_to_account_target() {
        let now = Utc::now();
        let ban = auto_ban_rule("user:42", now).unwrap();

        assert!(matches!(
            &ban.conditions[0].affects,
            BanAffects::Account { member_id } if member_id == &42
        ));
    }

    #[test]
    fn auto_ban_rule_maps_ip_subject_to_ip_target() {
        let now = Utc::now();
        let ban = auto_ban_rule("192.168.1.24", now).unwrap();

        assert!(matches!(
            &ban.conditions[0].affects,
            BanAffects::Ip { value } if value == "192.168.1.24"
        ));
    }

    #[test]
    fn auto_ban_rule_maps_ipv6_subject_to_ip_target() {
        let now = Utc::now();
        let ban = auto_ban_rule("2001:db8::1", now).unwrap();

        assert!(matches!(
            &ban.conditions[0].affects,
            BanAffects::Ip { value } if value == "2001:db8::1"
        ));
    }

    #[test]
    fn auto_ban_rule_rejects_generic_subject() {
        let now = Utc::now();
        let error = auto_ban_rule("agent:demo", now).unwrap_err();

        assert!(matches!(
            error,
            crate::services::ForumError::Validation(message)
                if message == "verification_ban_subject_not_targetable"
        ));
    }

    #[test]
    fn repetition_stays_within_single_token() {
        let mut rng = DeterministicRng::new(9);
        let repeated = apply_repetition("shells", &mut rng);

        assert!(!repeated.contains(' '));
        assert!(!repeated.contains('-'));
        assert!(repeated.len() > "shells".len());
    }

    #[test]
    fn division_rejects_non_terminating_two_decimal_answers() {
        let error = generate_challenge_text(1, 3, MathOperator::Divide, 4).unwrap_err();

        assert!(matches!(
            error,
            crate::services::ForumError::Validation(message)
                if message == "verification_division_not_exact_two_decimals"
        ));
    }
}
