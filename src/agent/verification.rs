use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use lazy_static::lazy_static;
use rand::{rngs::OsRng, RngCore};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    future::Future,
    sync::{Arc, Mutex as StdMutex},
};
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};

use btc_forum_rust::{
    services::{
        BanAffects, BanCondition, BanRule, ForumContext, ForumError, ForumService,
        VerificationChallengeUpsert,
    },
    surreal::{SurrealForumService, SurrealPost, SurrealTopic},
};
use btc_forum_shared::{ApiError, CreatePostPayload, CreateTopicPayload, ErrorCode, Post, Topic};

pub enum VerifiedAction {
    Topic { topic: Topic, first_post: Post },
    Reply { post: Post },
}

#[derive(Clone, Debug, PartialEq)]
pub enum VerificationActionPreflight {
    TopicCreate { payload: CreateTopicPayload },
    ReplyCreate { payload: CreatePostPayload },
}

impl VerificationActionPreflight {
    pub fn action_kind(&self) -> VerificationActionKind {
        match self {
            VerificationActionPreflight::TopicCreate { .. } => VerificationActionKind::TopicCreate,
            VerificationActionPreflight::ReplyCreate { .. } => VerificationActionKind::ReplyCreate,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingVerificationChallenge {
    pub verification_code: String,
    pub challenge_text: String,
    pub expires_at: DateTime<Utc>,
    pub attempt_count: i64,
    pub max_attempts: i64,
}

const VERIFICATION_INSTRUCTIONS: &str = "answer with exactly two decimal places";
const VERIFICATION_MAX_ATTEMPTS: i64 = 3;
const VERIFICATION_TTL_MINUTES: i64 = 5;

lazy_static! {
    static ref VERIFICATION_CHALLENGE_LOCKS: StdMutex<HashMap<String, Arc<AsyncMutex<()>>>> =
        StdMutex::new(HashMap::new());
}

pub async fn submit_verification<S, F, Fut>(
    forum_service: S,
    surreal: &SurrealForumService,
    ctx: &ForumContext,
    subject: &str,
    verification_code: &str,
    answer: &str,
    preflight: F,
) -> Result<VerifiedAction, (StatusCode, ApiError)>
where
    S: ForumService + Clone + Send + 'static,
    F: FnOnce(VerificationActionPreflight) -> Fut,
    Fut: Future<Output = Result<(), (StatusCode, ApiError)>>,
{
    let challenge_lock = acquire_challenge_lock(verification_code).await;
    let mut challenge = load_challenge(forum_service.clone(), verification_code).await?;
    ensure_challenge_owner(&challenge, subject)?;
    let expired_now = expire_challenge_if_needed(forum_service.clone(), &mut challenge).await?;
    if expired_now {
        record_failure_streak_and_maybe_ban(forum_service.clone(), subject, Utc::now()).await?;
    }
    ensure_challenge_is_active(&challenge)?;

    if answer.trim() != challenge.expected_answer {
        handle_failed_verification(forum_service.clone(), subject, &mut challenge).await?;
        return Err(agent_error(
            StatusCode::BAD_REQUEST,
            ErrorCode::Validation,
            "incorrect verification answer",
        ));
    }

    preflight(action_preflight_from_challenge(&challenge)?).await?;
    let now = Utc::now();
    let challenge =
        consume_pending_challenge(forum_service.clone(), verification_code, now).await?;
    drop(challenge_lock);

    let action = execute_verified_action(surreal, ctx, &challenge).await?;
    if let Err((status, error)) = clear_failure_streak(forum_service, subject, now).await {
        tracing::warn!(
            status = %status,
            code = ?error.code,
            message = %error.message,
            "failed to clear verification failure streak after successful publish"
        );
    }
    Ok(action)
}

struct ChallengeLockGuard {
    verification_code: String,
    lock: Arc<AsyncMutex<()>>,
    guard: Option<OwnedMutexGuard<()>>,
}

impl Drop for ChallengeLockGuard {
    fn drop(&mut self) {
        self.guard.take();

        let mut locks = VERIFICATION_CHALLENGE_LOCKS
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let should_remove = locks
            .get(&self.verification_code)
            .map(|current| Arc::ptr_eq(current, &self.lock) && Arc::strong_count(&self.lock) == 2)
            .unwrap_or(false);
        if should_remove {
            locks.remove(&self.verification_code);
        }
    }
}

async fn acquire_challenge_lock(verification_code: &str) -> ChallengeLockGuard {
    let lock = {
        let mut locks = VERIFICATION_CHALLENGE_LOCKS
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        locks
            .entry(verification_code.to_string())
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone()
    };
    let guard = lock.clone().lock_owned().await;
    ChallengeLockGuard {
        verification_code: verification_code.to_string(),
        lock,
        guard: Some(guard),
    }
}

fn action_preflight_from_challenge(
    challenge: &VerificationChallengeRecord,
) -> Result<VerificationActionPreflight, (StatusCode, ApiError)> {
    match challenge.action_kind {
        VerificationActionKind::TopicCreate => {
            let payload = deserialize_payload(&challenge.payload_json, challenge.action_kind)?;
            Ok(VerificationActionPreflight::TopicCreate { payload })
        }
        VerificationActionKind::ReplyCreate => {
            let payload = deserialize_payload(&challenge.payload_json, challenge.action_kind)?;
            Ok(VerificationActionPreflight::ReplyCreate { payload })
        }
    }
}

fn ensure_challenge_owner(
    challenge: &VerificationChallengeRecord,
    subject: &str,
) -> Result<(), (StatusCode, ApiError)> {
    if challenge.agent_subject == subject {
        Ok(())
    } else {
        Err(agent_error(
            StatusCode::FORBIDDEN,
            ErrorCode::Forbidden,
            "verification challenge belongs to a different subject",
        ))
    }
}

fn ensure_challenge_is_active(
    challenge: &VerificationChallengeRecord,
) -> Result<(), (StatusCode, ApiError)> {
    if challenge.expires_at <= Utc::now()
        || challenge.status == VerificationChallengeStatus::Expired
    {
        return Err(agent_error(
            StatusCode::GONE,
            ErrorCode::Conflict,
            "verification challenge expired",
        ));
    }

    if challenge.status.is_terminal() {
        return Err(agent_error(
            StatusCode::CONFLICT,
            ErrorCode::Conflict,
            "verification challenge is no longer pending",
        ));
    }

    Ok(())
}

async fn load_challenge<S>(
    forum_service: S,
    verification_code: &str,
) -> Result<VerificationChallengeRecord, (StatusCode, ApiError)>
where
    S: ForumService + Send + 'static,
{
    let verification_code = verification_code.to_string();
    let challenge = run_forum_job(forum_service, move |forum| {
        forum.get_verification_challenge(&verification_code)
    })
    .await
    .map_err(|err| forum_internal("failed to load verification challenge", err))?;

    challenge.ok_or_else(|| {
        agent_error(
            StatusCode::NOT_FOUND,
            ErrorCode::NotFound,
            "verification challenge not found",
        )
    })
}

async fn save_challenge<S>(
    forum_service: S,
    challenge: VerificationChallengeRecord,
) -> Result<(), (StatusCode, ApiError)>
where
    S: ForumService + Send + 'static,
{
    run_forum_job(forum_service, move |forum| {
        forum.save_verification_challenge(challenge)
    })
    .await
    .map_err(|err| forum_internal("failed to save verification challenge", err))
}

async fn consume_pending_challenge<S>(
    forum_service: S,
    verification_code: &str,
    verified_at: DateTime<Utc>,
) -> Result<VerificationChallengeRecord, (StatusCode, ApiError)>
where
    S: ForumService + Send + 'static,
{
    let verification_code = verification_code.to_string();
    let challenge = run_forum_job(forum_service, move |forum| {
        forum.consume_pending_verification_challenge(&verification_code, verified_at)
    })
    .await
    .map_err(|err| forum_internal("failed to consume verification challenge", err))?;

    challenge.ok_or_else(|| {
        agent_error(
            StatusCode::CONFLICT,
            ErrorCode::Conflict,
            "verification challenge is no longer pending",
        )
    })
}

async fn expire_challenge_if_needed<S>(
    forum_service: S,
    challenge: &mut VerificationChallengeRecord,
) -> Result<bool, (StatusCode, ApiError)>
where
    S: ForumService + Send + 'static,
{
    if challenge.status == VerificationChallengeStatus::Pending
        && challenge.expires_at <= Utc::now()
    {
        challenge.status = VerificationChallengeStatus::Expired;
        save_challenge(forum_service, challenge.clone()).await?;
        return Ok(true);
    }

    Ok(false)
}

async fn handle_failed_verification<S>(
    forum_service: S,
    subject: &str,
    challenge: &mut VerificationChallengeRecord,
) -> Result<(), (StatusCode, ApiError)>
where
    S: ForumService + Clone + Send + 'static,
{
    record_failed_attempt(challenge);
    save_challenge(forum_service.clone(), challenge.clone()).await?;

    record_failure_streak_and_maybe_ban(forum_service, subject, Utc::now()).await
}

async fn record_failure_streak_and_maybe_ban<S>(
    forum_service: S,
    subject: &str,
    now: DateTime<Utc>,
) -> Result<(), (StatusCode, ApiError)>
where
    S: ForumService + Clone + Send + 'static,
{
    let mut streak = load_failure_streak(forum_service.clone(), subject).await?;
    streak.consecutive_failures += 1;
    streak.last_failure_at = Some(now);
    save_failure_streak(forum_service.clone(), streak.clone()).await?;

    if streak.consecutive_failures >= 10 {
        let rule = auto_ban_rule(subject, now).map_err(|err| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                ApiError {
                    code: ErrorCode::Internal,
                    message: "failed to enforce verification auto-ban".into(),
                    details: Some(serde_json::json!({
                        "reason": err.to_string(),
                    })),
                },
            )
        })?;
        save_ban_rule(forum_service, rule).await?;
    }

    Ok(())
}

async fn load_failure_streak<S>(
    forum_service: S,
    subject: &str,
) -> Result<VerificationFailureStreak, (StatusCode, ApiError)>
where
    S: ForumService + Send + 'static,
{
    let subject = subject.to_string();
    run_forum_job(forum_service, move |forum| {
        forum.get_verification_failure_streak(&subject)
    })
    .await
    .map_err(|err| forum_internal("failed to load verification failure streak", err))
}

async fn save_failure_streak<S>(
    forum_service: S,
    streak: VerificationFailureStreak,
) -> Result<(), (StatusCode, ApiError)>
where
    S: ForumService + Send + 'static,
{
    run_forum_job(forum_service, move |forum| {
        forum.save_verification_failure_streak(streak)
    })
    .await
    .map_err(|err| forum_internal("failed to save verification failure streak", err))
}

async fn clear_failure_streak<S>(
    forum_service: S,
    subject: &str,
    now: DateTime<Utc>,
) -> Result<(), (StatusCode, ApiError)>
where
    S: ForumService + Clone + Send + 'static,
{
    let mut streak = load_failure_streak(forum_service.clone(), subject).await?;
    reset_failure_streak(&mut streak, now);
    save_failure_streak(forum_service, streak).await
}

async fn save_ban_rule<S>(forum_service: S, rule: BanRule) -> Result<(), (StatusCode, ApiError)>
where
    S: ForumService + Send + 'static,
{
    run_forum_job(forum_service, move |forum| {
        forum.save_ban_rule(rule).map(|_| ())
    })
    .await
    .map_err(|err| forum_internal("failed to save verification auto-ban", err))
}

async fn run_forum_job<S, T>(
    forum_service: S,
    job: impl FnOnce(S) -> Result<T, ForumError> + Send + 'static,
) -> Result<T, ForumError>
where
    S: ForumService + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(move || job(forum_service))
        .await
        .map_err(|err| ForumError::Internal(format!("forum task failed: {err}")))?
}

async fn execute_verified_action(
    surreal: &SurrealForumService,
    ctx: &ForumContext,
    challenge: &VerificationChallengeRecord,
) -> Result<VerifiedAction, (StatusCode, ApiError)> {
    match challenge.action_kind {
        VerificationActionKind::TopicCreate => {
            execute_verified_topic_create(surreal, ctx, challenge).await
        }
        VerificationActionKind::ReplyCreate => {
            execute_verified_reply_create(surreal, ctx, challenge).await
        }
    }
}

async fn execute_verified_topic_create(
    surreal: &SurrealForumService,
    ctx: &ForumContext,
    challenge: &VerificationChallengeRecord,
) -> Result<VerifiedAction, (StatusCode, ApiError)> {
    let payload: CreateTopicPayload =
        deserialize_payload(&challenge.payload_json, challenge.action_kind)?;
    let (topic, first_post) = create_topic_now(surreal, &payload, &ctx.user_info.name)
        .await
        .map_err(|err| {
            agent_error_with_details(
                StatusCode::BAD_REQUEST,
                ErrorCode::Validation,
                "failed to create topic",
                serde_json::json!({
                    "board_id": &payload.board_id,
                    "reason": err.to_string(),
                }),
            )
        })?;

    Ok(VerifiedAction::Topic {
        topic: to_topic(topic),
        first_post: to_post(first_post),
    })
}

async fn execute_verified_reply_create(
    surreal: &SurrealForumService,
    ctx: &ForumContext,
    challenge: &VerificationChallengeRecord,
) -> Result<VerifiedAction, (StatusCode, ApiError)> {
    let payload: CreatePostPayload =
        deserialize_payload(&challenge.payload_json, challenge.action_kind)?;
    let topic = surreal
        .get_topic(&payload.topic_id)
        .await
        .map_err(|err| {
            agent_error_with_details(
                StatusCode::INTERNAL_SERVER_ERROR,
                ErrorCode::Internal,
                "failed to load topic",
                serde_json::json!({
                    "topic_id": &payload.topic_id,
                    "reason": err.to_string(),
                }),
            )
        })?
        .ok_or_else(|| {
            agent_error_with_details(
                StatusCode::NOT_FOUND,
                ErrorCode::NotFound,
                "topic not found",
                serde_json::json!({
                    "topic_id": &payload.topic_id,
                }),
            )
        })?;

    if topic.board_id != payload.board_id {
        return Err(agent_error_with_details(
            StatusCode::BAD_REQUEST,
            ErrorCode::Validation,
            "board_id does not match topic",
            serde_json::json!({
                "topic_id": &payload.topic_id,
                "board_id": &payload.board_id,
                "expected_board_id": topic.board_id,
            }),
        ));
    }

    let subject = reply_subject(&topic.subject, payload.subject.as_deref());
    let post = create_reply_now(
        surreal,
        &payload.topic_id,
        &payload.board_id,
        &subject,
        &payload.body,
        &ctx.user_info.name,
    )
    .await
    .map_err(|err| {
        agent_error_with_details(
            StatusCode::BAD_REQUEST,
            ErrorCode::Validation,
            "failed to create reply",
            serde_json::json!({
                "topic_id": &payload.topic_id,
                "reason": err.to_string(),
            }),
        )
    })?;

    Ok(VerifiedAction::Reply {
        post: to_post(post),
    })
}

fn deserialize_payload<T: DeserializeOwned>(
    payload_json: &Value,
    action_kind: VerificationActionKind,
) -> Result<T, (StatusCode, ApiError)> {
    serde_json::from_value(payload_json.clone()).map_err(|err| {
        agent_error_with_details(
            StatusCode::INTERNAL_SERVER_ERROR,
            ErrorCode::Internal,
            "failed to decode verification payload",
            serde_json::json!({
                "action": action_kind.as_str(),
                "reason": err.to_string(),
            }),
        )
    })
}

pub async fn create_topic_now(
    surreal: &SurrealForumService,
    payload: &CreateTopicPayload,
    author: &str,
) -> Result<(SurrealTopic, SurrealPost), surrealdb::Error> {
    let subject = sanitize_input(&payload.subject);
    let body = sanitize_input(&payload.body);
    let topic = surreal
        .create_topic(&payload.board_id, &subject, author)
        .await?;
    let topic_id = topic.id.clone().unwrap_or_default();
    let post = surreal
        .create_post_in_topic(&topic_id, &payload.board_id, &subject, &body, author)
        .await?;
    Ok((topic, post))
}

pub async fn create_reply_now(
    surreal: &SurrealForumService,
    topic_id: &str,
    board_id: &str,
    subject: &str,
    body: &str,
    author: &str,
) -> Result<SurrealPost, surrealdb::Error> {
    let subject = sanitize_input(subject);
    let body = sanitize_input(body);
    surreal
        .create_post_in_topic(topic_id, board_id, &subject, &body, author)
        .await
}

fn sanitize_input(input: &str) -> String {
    ammonia::Builder::default()
        .url_schemes(["http", "https"].into())
        .clean(input)
        .to_string()
}

fn reply_subject(topic_subject: &str, requested_subject: Option<&str>) -> String {
    requested_subject
        .map(str::trim)
        .filter(|subject| !subject.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("Re: {topic_subject}"))
}

fn to_topic(topic: SurrealTopic) -> Topic {
    Topic {
        id: topic.id,
        board_id: Some(topic.board_id),
        subject: topic.subject,
        author: topic.author,
        created_at: topic.created_at,
        updated_at: topic.updated_at,
    }
}

fn to_post(post: SurrealPost) -> Post {
    Post {
        id: post.id,
        topic_id: post.topic_id,
        board_id: post.board_id,
        subject: post.subject,
        body: post.body,
        author: post.author,
        created_at: post.created_at,
    }
}

fn agent_error(
    status: StatusCode,
    code: ErrorCode,
    message: impl Into<String>,
) -> (StatusCode, ApiError) {
    (
        status,
        ApiError {
            code,
            message: message.into(),
            details: None,
        },
    )
}

fn agent_error_with_details(
    status: StatusCode,
    code: ErrorCode,
    message: impl Into<String>,
    details: serde_json::Value,
) -> (StatusCode, ApiError) {
    (
        status,
        ApiError {
            code,
            message: message.into(),
            details: Some(details),
        },
    )
}

fn forum_internal(message: impl Into<String>, err: ForumError) -> (StatusCode, ApiError) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        ApiError {
            code: ErrorCode::Internal,
            message: message.into(),
            details: Some(serde_json::json!({
                "reason": err.to_string(),
            })),
        },
    )
}

pub async fn issue_topic_challenge<S>(
    forum_service: S,
    agent_subject: &str,
    payload: &CreateTopicPayload,
) -> Result<PendingVerificationChallenge, ApiError>
where
    S: ForumService + Clone + Send + 'static,
{
    issue_challenge(
        forum_service,
        agent_subject,
        VerificationActionKind::TopicCreate,
        payload,
    )
    .await
}

pub async fn issue_reply_challenge<S>(
    forum_service: S,
    agent_subject: &str,
    payload: &CreatePostPayload,
) -> Result<PendingVerificationChallenge, ApiError>
where
    S: ForumService + Clone + Send + 'static,
{
    issue_challenge(
        forum_service,
        agent_subject,
        VerificationActionKind::ReplyCreate,
        payload,
    )
    .await
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum VerificationActionKind {
    #[default]
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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum VerificationChallengeStatus {
    #[default]
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

fn challenge_seed() -> i64 {
    Utc::now().timestamp_micros()
}

fn verification_code(_seed: i64) -> String {
    let mut entropy = [0_u8; 12];
    OsRng.fill_bytes(&mut entropy);
    encode_verification_code(entropy)
}

fn encode_verification_code(entropy: [u8; 12]) -> String {
    let mut code = String::from("avc_");
    for byte in entropy {
        use std::fmt::Write as _;
        let _ = write!(&mut code, "{byte:02x}");
    }
    code
}

fn challenge_operands(seed: i64) -> (i64, i64, MathOperator) {
    let normalized = seed.unsigned_abs();
    match normalized % 4 {
        0 => (
            10 + (normalized % 40) as i64,
            1 + ((normalized / 3) % 20) as i64,
            MathOperator::Add,
        ),
        1 => {
            let right = 1 + ((normalized / 5) % 15) as i64;
            let left = right + 5 + ((normalized / 7) % 25) as i64;
            (left, right, MathOperator::Subtract)
        }
        2 => (
            2 + (normalized % 10) as i64,
            2 + ((normalized / 11) % 10) as i64,
            MathOperator::Multiply,
        ),
        _ => {
            let right = 1 + ((normalized / 13) % 10) as i64;
            let quotient = 1 + ((normalized / 17) % 12) as i64;
            (right * quotient, right, MathOperator::Divide)
        }
    }
}

async fn issue_challenge<S, P>(
    forum_service: S,
    agent_subject: &str,
    action_kind: VerificationActionKind,
    payload: &P,
) -> Result<PendingVerificationChallenge, ApiError>
where
    S: ForumService + Clone + Send + 'static,
    P: Serialize,
{
    let seed = challenge_seed();
    let (left, right, operator) = challenge_operands(seed);
    let generated =
        generate_challenge_text(left, right, operator, seed).map_err(validation_error)?;
    let payload_json = serde_json::to_value(payload).map_err(|err| ApiError {
        code: ErrorCode::Internal,
        message: "failed to serialize verification payload".into(),
        details: Some(serde_json::json!({
            "reason": err.to_string(),
        })),
    })?;

    let now = Utc::now();
    let expires_at = now + chrono::Duration::minutes(VERIFICATION_TTL_MINUTES);
    let verification_code = verification_code(seed);
    let agent_subject = agent_subject.to_string();
    let challenge_text = generated.challenge_text;
    let expected_answer = generated.expected_answer;
    let generator_version = generated.generator_version;
    let generator_seed = generated.generator_seed;

    let record = tokio::task::spawn_blocking(move || {
        forum_service.create_verification_challenge(VerificationChallengeUpsert {
            verification_code,
            agent_subject,
            action_kind,
            payload_json,
            challenge_text,
            expected_answer,
            generator_version,
            generator_seed,
            status: VerificationChallengeStatus::Pending,
            attempt_count: 0,
            max_attempts: VERIFICATION_MAX_ATTEMPTS,
            expires_at,
            verified_at: None,
        })
    })
    .await
    .map_err(|err| ApiError {
        code: ErrorCode::Internal,
        message: "failed to store verification challenge".into(),
        details: Some(serde_json::json!({
            "reason": err.to_string(),
        })),
    })?
    .map_err(service_error)?;

    Ok(PendingVerificationChallenge {
        verification_code: record.verification_code,
        challenge_text: record.challenge_text,
        expires_at: record.expires_at,
        attempt_count: record.attempt_count,
        max_attempts: record.max_attempts,
    })
}

fn validation_error(err: ForumError) -> ApiError {
    match err {
        ForumError::Validation(message) => ApiError {
            code: ErrorCode::Validation,
            message,
            details: Some(serde_json::json!({
                "instructions": VERIFICATION_INSTRUCTIONS,
            })),
        },
        other => service_error(other),
    }
}

fn service_error(err: ForumError) -> ApiError {
    match err {
        ForumError::Validation(message) => ApiError {
            code: ErrorCode::Validation,
            message,
            details: None,
        },
        other => ApiError {
            code: ErrorCode::Internal,
            message: "verification challenge unavailable".into(),
            details: Some(serde_json::json!({
                "reason": other.to_string(),
            })),
        },
    }
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
        MathOperator::Multiply => {
            format!("A lobster taps {left_words} fins {right_words} times. What is the result?")
        }
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
        apply_repetition, auto_ban_rule, encode_verification_code, ensure_challenge_is_active,
        ensure_challenge_owner, expire_challenge_if_needed, generate_challenge_text,
        normalize_answer, record_failed_attempt, record_failure_streak_and_maybe_ban,
        reset_failure_streak, verification_code, DeterministicRng, MathOperator,
        VerificationActionKind, VerificationChallengeRecord, VerificationChallengeStatus,
        VerificationFailureStreak,
    };
    use crate::services::{BanAffects, ForumService, InMemoryService};
    use axum::http::StatusCode;
    use chrono::{Duration, Utc};
    use serde_json::Value;

    fn sample_pending_challenge() -> VerificationChallengeRecord {
        let now = Utc::now();
        VerificationChallengeRecord {
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
        }
    }

    fn expired_sample_challenge() -> VerificationChallengeRecord {
        let mut challenge = sample_pending_challenge();
        challenge.expires_at = Utc::now() - Duration::seconds(1);
        challenge
    }

    #[test]
    fn normalize_answer_formats_integer_with_two_decimals() {
        assert_eq!(normalize_answer(7.0).unwrap(), "7.00");
    }

    #[test]
    fn normalize_answer_preserves_fractional_precision() {
        assert_eq!(normalize_answer(7.25).unwrap(), "7.25");
    }

    #[test]
    fn verification_ttl_matches_five_minute_window() {
        assert_eq!(super::VERIFICATION_TTL_MINUTES, 5);
    }

    #[test]
    fn verification_code_does_not_expose_raw_seed() {
        let seed = 0x12345_i64;
        let code = verification_code(seed);

        assert_ne!(code, format!("avc_{:x}", seed.unsigned_abs()));
    }

    #[test]
    fn encoded_verification_code_uses_supplied_entropy_not_seed_format() {
        let code = encode_verification_code([0x12; 12]);

        assert_eq!(code, "avc_121212121212121212121212");
        assert_ne!(code, "avc_12");
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
    fn expired_challenge_maps_to_gone() {
        let challenge = expired_sample_challenge();
        let err = ensure_challenge_is_active(&challenge).unwrap_err();

        assert_eq!(err.0, StatusCode::GONE);
    }

    #[tokio::test]
    async fn expiring_pending_challenge_records_one_failure() {
        let service = InMemoryService::default();
        let mut challenge = expired_sample_challenge();
        let subject = challenge.agent_subject.clone();

        let expired = expire_challenge_if_needed(service.clone(), &mut challenge)
            .await
            .unwrap();
        assert!(expired);
        record_failure_streak_and_maybe_ban(service.clone(), &subject, Utc::now())
            .await
            .unwrap();

        let stored = service
            .get_verification_challenge(&challenge.verification_code)
            .unwrap()
            .unwrap();
        let streak = service.get_verification_failure_streak(&subject).unwrap();
        assert_eq!(stored.status, VerificationChallengeStatus::Expired);
        assert_eq!(streak.consecutive_failures, 1);
    }

    #[test]
    fn wrong_subject_is_forbidden() {
        let challenge = sample_pending_challenge();
        let err = ensure_challenge_owner(&challenge, "other@example.com").unwrap_err();

        assert_eq!(err.0, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn challenge_lock_entry_is_removed_after_guard_drops() {
        let verification_code = format!(
            "verify-lock-cleanup-{}",
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        );

        {
            let _guard = super::acquire_challenge_lock(&verification_code).await;
            let locks = super::VERIFICATION_CHALLENGE_LOCKS
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            assert!(locks.contains_key(&verification_code));
        }

        let locks = super::VERIFICATION_CHALLENGE_LOCKS
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        assert!(!locks.contains_key(&verification_code));
    }

    #[test]
    fn record_failure_marks_terminal_after_max_attempts() {
        let now = Utc::now();
        let mut challenge = sample_pending_challenge();

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
