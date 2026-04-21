# Agent Verification Challenge Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a Moltbook-style verification challenge flow for Agent API topic and reply creation, with admin bypass, two-phase publish, verification submission, failure streak tracking, and auto-ban enforcement.

**Architecture:** Introduce a shared agent verification module plus durable service-layer persistence for challenges and failure streaks. Wire Agent API write handlers so non-admin creates return a challenge first, then create content only after `POST /agent/v1/verify` succeeds.

**Tech Stack:** Rust, Axum, SurrealDB, serde, chrono, existing SoulForum Agent API envelope and rate limiter.

---

### Task 1: Add Verification Domain Types And Service Contracts

**Files:**
- Create: `src/agent/verification.rs`
- Modify: `src/agent/mod.rs`
- Modify: `src/services/mod.rs`
- Test: `src/agent/verification.rs`

- [ ] **Step 1: Write the failing tests for verification domain behavior**

Add these tests to the new `src/agent/verification.rs` test module:

```rust
#[cfg(test)]
mod tests {
    use super::{normalize_answer, VerificationActionKind, VerificationChallengeStatus};

    #[test]
    fn normalize_answer_formats_integer_with_two_decimals() {
        assert_eq!(normalize_answer(15.0).unwrap(), "15.00");
    }

    #[test]
    fn normalize_answer_preserves_fractional_precision() {
        assert_eq!(normalize_answer(12.5).unwrap(), "12.50");
    }

    #[test]
    fn verification_action_kind_has_stable_storage_names() {
        assert_eq!(VerificationActionKind::TopicCreate.as_str(), "topic_create");
        assert_eq!(VerificationActionKind::ReplyCreate.as_str(), "reply_create");
    }

    #[test]
    fn verification_status_has_terminal_helper() {
        assert!(!VerificationChallengeStatus::Pending.is_terminal());
        assert!(VerificationChallengeStatus::Verified.is_terminal());
        assert!(VerificationChallengeStatus::Expired.is_terminal());
        assert!(VerificationChallengeStatus::Failed.is_terminal());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test normalize_answer_formats_integer_with_two_decimals --lib
```

Expected: FAIL with unresolved import or missing `src/agent/verification.rs`.

- [ ] **Step 3: Write the domain module and exports**

Create `src/agent/verification.rs` with the initial types and helpers:

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use btc_forum_rust::services::ForumError;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum VerificationActionKind {
    TopicCreate,
    ReplyCreate,
}

impl VerificationActionKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::TopicCreate => "topic_create",
            Self::ReplyCreate => "reply_create",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum VerificationChallengeStatus {
    Pending,
    Verified,
    Expired,
    Failed,
}

impl VerificationChallengeStatus {
    pub fn is_terminal(&self) -> bool {
        !matches!(self, Self::Pending)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
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

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct VerificationFailureStreak {
    pub agent_subject: String,
    pub consecutive_failures: i64,
    pub last_failure_at: Option<DateTime<Utc>>,
    pub last_success_at: Option<DateTime<Utc>>,
}

pub fn normalize_answer(value: f64) -> Result<String, ForumError> {
    if !value.is_finite() {
        return Err(ForumError::Validation("answer must be finite".into()));
    }
    Ok(format!("{value:.2}"))
}
```

Update `src/agent/mod.rs` to export the module:

```rust
pub mod auth;
pub mod capability;
pub mod handlers;
pub mod request_id;
pub mod response;
pub mod router;
pub mod verification;
```

Extend `src/services/mod.rs` with verification records and service methods:

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VerificationChallengeUpsert {
    pub verification_code: String,
    pub agent_subject: String,
    pub action_kind: String,
    pub payload_json: Value,
    pub challenge_text: String,
    pub expected_answer: String,
    pub generator_version: String,
    pub generator_seed: i64,
    pub max_attempts: i64,
    pub expires_at: DateTime<Utc>,
}

pub trait ForumService {
    // existing methods...
    fn create_verification_challenge(
        &self,
        input: VerificationChallengeUpsert,
    ) -> ServiceResult<VerificationChallengeRecord>;
    fn get_verification_challenge(
        &self,
        verification_code: &str,
    ) -> ServiceResult<Option<VerificationChallengeRecord>>;
    fn save_verification_challenge(
        &self,
        challenge: VerificationChallengeRecord,
    ) -> ServiceResult<()>;
    fn get_verification_failure_streak(
        &self,
        agent_subject: &str,
    ) -> ServiceResult<VerificationFailureStreak>;
    fn save_verification_failure_streak(
        &self,
        streak: VerificationFailureStreak,
    ) -> ServiceResult<()>;
}
```

- [ ] **Step 4: Run test to verify it passes**

Run:

```bash
cargo test normalize_answer_formats_integer_with_two_decimals --lib
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/agent/mod.rs src/agent/verification.rs src/services/mod.rs
git commit -m "Add verification challenge domain types"
```

### Task 2: Add In-Memory And Surreal Persistence For Challenges And Failure Streaks

**Files:**
- Modify: `src/services/mod.rs`
- Modify: `src/services/surreal.rs`
- Modify: `migrations/surreal/0001_init.surql`
- Modify: `docs/surreal_schema.md`
- Test: `src/services/mod.rs`

- [ ] **Step 1: Write the failing service tests**

Add tests in `src/services/mod.rs` for the in-memory implementation:

```rust
#[test]
fn create_and_fetch_verification_challenge_round_trips() {
    let service = InMemoryService::default();
    let record = service
        .create_verification_challenge(VerificationChallengeUpsert {
            verification_code: "avc_test".into(),
            agent_subject: "agent@example.com".into(),
            action_kind: "topic_create".into(),
            payload_json: serde_json::json!({"board_id":"boards:1","subject":"Hello","body":"Body"}),
            challenge_text: "lObStEr".into(),
            expected_answer: "15.00".into(),
            generator_version: "v1".into(),
            generator_seed: 42,
            max_attempts: 3,
            expires_at: chrono::Utc::now(),
        })
        .unwrap();

    let fetched = service
        .get_verification_challenge("avc_test")
        .unwrap()
        .unwrap();

    assert_eq!(record.verification_code, fetched.verification_code);
    assert_eq!(fetched.agent_subject, "agent@example.com");
}

#[test]
fn verification_failure_streak_persists_updates() {
    let service = InMemoryService::default();
    let mut streak = service
        .get_verification_failure_streak("agent@example.com")
        .unwrap();
    streak.consecutive_failures = 4;
    service.save_verification_failure_streak(streak.clone()).unwrap();

    let fetched = service
        .get_verification_failure_streak("agent@example.com")
        .unwrap();

    assert_eq!(fetched.consecutive_failures, 4);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test create_and_fetch_verification_challenge_round_trips --lib
```

Expected: FAIL because the new trait methods are not implemented in `InMemoryService`.

- [ ] **Step 3: Implement persistence in the in-memory and Surreal services**

Extend the in-memory state in `src/services/mod.rs`:

```rust
#[derive(Default)]
struct ServiceState {
    // existing fields...
    verification_challenges: HashMap<String, VerificationChallengeRecord>,
    verification_failure_streaks: HashMap<String, VerificationFailureStreak>,
    next_verification_id: i64,
}
```

Implement the new trait methods in the in-memory service:

```rust
fn create_verification_challenge(
    &self,
    input: VerificationChallengeUpsert,
) -> ServiceResult<VerificationChallengeRecord> {
    let mut state = self.state.lock().unwrap();
    let id = state.next_verification_id;
    state.next_verification_id += 1;

    let record = VerificationChallengeRecord {
        id,
        verification_code: input.verification_code,
        agent_subject: input.agent_subject,
        action_kind: match input.action_kind.as_str() {
            "topic_create" => VerificationActionKind::TopicCreate,
            "reply_create" => VerificationActionKind::ReplyCreate,
            other => return Err(ForumError::Validation(format!("unknown action_kind: {other}"))),
        },
        payload_json: input.payload_json,
        challenge_text: input.challenge_text,
        expected_answer: input.expected_answer,
        generator_version: input.generator_version,
        generator_seed: input.generator_seed,
        status: VerificationChallengeStatus::Pending,
        attempt_count: 0,
        max_attempts: input.max_attempts,
        expires_at: input.expires_at,
        verified_at: None,
        created_at: Utc::now(),
    };

    state
        .verification_challenges
        .insert(record.verification_code.clone(), record.clone());
    Ok(record)
}
```

Add Surreal schema objects to `migrations/surreal/0001_init.surql`:

```sql
DEFINE TABLE agent_verification_challenges SCHEMALESS;
DEFINE INDEX idx_agent_verification_code ON TABLE agent_verification_challenges COLUMNS verification_code UNIQUE;
DEFINE INDEX idx_agent_verification_subject ON TABLE agent_verification_challenges COLUMNS agent_subject, created_at_ms;
DEFINE INDEX idx_agent_verification_status ON TABLE agent_verification_challenges COLUMNS status, expires_at_ms;

DEFINE TABLE agent_verification_failures SCHEMALESS;
DEFINE INDEX idx_agent_verification_failures_subject ON TABLE agent_verification_failures COLUMNS agent_subject UNIQUE;
```

Implement the Surreal service methods in `src/services/surreal.rs` with the same field names used by the new schema:

```rust
fn get_verification_challenge(
    &self,
    verification_code: &str,
) -> Result<Option<VerificationChallengeRecord>, ForumError> {
    #[derive(Deserialize)]
    struct Row {
        id: Option<i64>,
        verification_code: String,
        agent_subject: String,
        action_kind: String,
        payload_json: Value,
        challenge_text: String,
        expected_answer: String,
        generator_version: String,
        generator_seed: i64,
        status: String,
        attempt_count: i64,
        max_attempts: i64,
        expires_at_ms: i64,
        verified_at_ms: Option<i64>,
        created_at_ms: i64,
    }

    let rows: Vec<Row> = self.block_on(async {
        let mut response = self.client.query(
            "SELECT * FROM agent_verification_challenges WHERE verification_code = $code LIMIT 1;"
        )
        .bind(("code", verification_code.to_string()))
        .await?;
        response.take(0)
    })?;

    Ok(rows.into_iter().next().map(map_verification_row))
}
```

Update `docs/surreal_schema.md` with a short section describing the two new tables and indexes.

- [ ] **Step 4: Run test to verify it passes**

Run:

```bash
cargo test create_and_fetch_verification_challenge_round_trips --lib
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/services/mod.rs src/services/surreal.rs migrations/surreal/0001_init.surql docs/surreal_schema.md
git commit -m "Add verification challenge persistence"
```

### Task 3: Implement Challenge Generator, State Machine, And Auto-Ban Enforcement

**Files:**
- Modify: `src/agent/verification.rs`
- Modify: `src/services/mod.rs`
- Test: `src/agent/verification.rs`

- [ ] **Step 1: Write the failing verification logic tests**

Add these tests to `src/agent/verification.rs`:

```rust
#[test]
fn generator_uses_word_numbers_not_digits() {
    let generated = generate_challenge_text(23, 45, MathOperator::Add, 7);
    assert!(!generated.challenge_text.chars().any(|ch| ch.is_ascii_digit()));
    assert_eq!(generated.expected_answer, "68.00");
}

#[test]
fn verify_success_resets_failure_streak() {
    let mut streak = VerificationFailureStreak {
        agent_subject: "agent@example.com".into(),
        consecutive_failures: 3,
        last_failure_at: None,
        last_success_at: None,
    };
    reset_failure_streak(&mut streak, chrono::Utc::now());
    assert_eq!(streak.consecutive_failures, 0);
    assert!(streak.last_success_at.is_some());
}

#[test]
fn record_failure_marks_terminal_after_max_attempts() {
    let mut challenge = sample_pending_challenge();
    challenge.attempt_count = 2;
    challenge.max_attempts = 3;
    record_failed_attempt(&mut challenge);
    assert_eq!(challenge.attempt_count, 3);
    assert_eq!(challenge.status, VerificationChallengeStatus::Failed);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test generator_uses_word_numbers_not_digits --lib
```

Expected: FAIL because the generator and state transition helpers do not exist yet.

- [ ] **Step 3: Implement the generator and verification state transitions**

Add these core pieces to `src/agent/verification.rs`:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MathOperator {
    Add,
    Subtract,
    Multiply,
    Divide,
}

impl MathOperator {
    pub fn keyword(&self) -> &'static str {
        match self {
            Self::Add => "combined",
            Self::Subtract => "slows",
            Self::Multiply => "times",
            Self::Divide => "split",
        }
    }
}

#[derive(Clone, Debug)]
pub struct GeneratedChallenge {
    pub challenge_text: String,
    pub expected_answer: String,
    pub generator_version: String,
    pub generator_seed: i64,
}

pub fn generate_challenge_text(
    left: i64,
    right: i64,
    op: MathOperator,
    seed: i64,
) -> GeneratedChallenge {
    let plain = format!(
        "A lobster starts with {} units and {} by {}, what is the result?",
        number_to_words(left),
        op.keyword(),
        number_to_words(right),
    );
    let challenge_text = obfuscate_text(&plain, seed);
    let result = match op {
        MathOperator::Add => left as f64 + right as f64,
        MathOperator::Subtract => left as f64 - right as f64,
        MathOperator::Multiply => (left * right) as f64,
        MathOperator::Divide => left as f64 / right as f64,
    };

    GeneratedChallenge {
        challenge_text,
        expected_answer: normalize_answer(result).unwrap(),
        generator_version: "v1".into(),
        generator_seed: seed,
    }
}

pub fn record_failed_attempt(challenge: &mut VerificationChallengeRecord) {
    challenge.attempt_count += 1;
    if challenge.attempt_count >= challenge.max_attempts {
        challenge.status = VerificationChallengeStatus::Failed;
    }
}

pub fn reset_failure_streak(streak: &mut VerificationFailureStreak, now: DateTime<Utc>) {
    streak.consecutive_failures = 0;
    streak.last_success_at = Some(now);
}
```

Also add a helper that converts a 10-failure streak into a `BanRule` payload:

```rust
pub fn auto_ban_rule(agent_subject: &str, now: DateTime<Utc>) -> BanRule {
    BanRule {
        id: 0,
        reason: Some(format!("agent verification failures for {agent_subject}")),
        cannot_post: true,
        cannot_access: false,
        expires_at_ms: Some((now + chrono::Duration::hours(24)).timestamp_millis()),
        conditions: vec![BanCondition {
            id: 0,
            reason: Some("verification failure threshold".into()),
            affects: BanAffects::Email {
                value: agent_subject.to_string(),
            },
        }],
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run:

```bash
cargo test generator_uses_word_numbers_not_digits --lib
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/agent/verification.rs src/services/mod.rs
git commit -m "Add verification challenge generator and enforcement"
```

### Task 4: Add Agent Verify Endpoint And Verification Response Payloads

**Files:**
- Modify: `src/agent/router.rs`
- Modify: `src/agent/handlers.rs`
- Create: `src/agent/handlers/verify.rs`
- Modify: `src/agent/response.rs`
- Test: `src/agent/handlers/verify.rs`

- [ ] **Step 1: Write the failing verify handler tests**

Create `src/agent/handlers/verify.rs` with these tests first:

```rust
#[cfg(test)]
mod tests {
    use super::VerifyRequest;

    #[test]
    fn verify_request_deserializes_answer_payload() {
        let body = r#"{"verification_code":"avc_test","answer":"15.00"}"#;
        let parsed: VerifyRequest = serde_json::from_str(body).unwrap();
        assert_eq!(parsed.verification_code, "avc_test");
        assert_eq!(parsed.answer, "15.00");
    }

    #[test]
    fn verification_required_payload_serializes_instructions() {
        let payload = super::VerificationRequiredData {
            verification_required: true,
            verification: super::VerificationPrompt {
                verification_code: "avc_test".into(),
                challenge_text: "LoBStEr".into(),
                expires_at: "2026-04-20T12:34:56Z".into(),
                attempts_remaining: 3,
                instructions: "answer with exactly two decimal places".into(),
            },
        };

        let json = serde_json::to_value(payload).unwrap();
        assert_eq!(json["verification"]["instructions"], "answer with exactly two decimal places");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test verify_request_deserializes_answer_payload --lib
```

Expected: FAIL because `verify.rs` and its types do not exist.

- [ ] **Step 3: Implement the verify handler and register the route**

Create `src/agent/handlers/verify.rs`:

```rust
use axum::{
    http::{Extensions, StatusCode},
    response::IntoResponse,
    Extension, Json,
    extract::State,
};
use serde::{Deserialize, Serialize};

use btc_forum_rust::auth::AuthClaims;
use btc_forum_shared::{ApiError, ErrorCode, Post, Topic};

use crate::agent::{
    auth::require_scope,
    request_id::RequestId,
    response::{err_response, ok_response},
};
use crate::api::{auth::ensure_user_ctx, guards::enforce_rate, state::AppState};

const VERIFY_SCOPE: &str = "forum:verify:write";
const VERIFY_LEGACY_PERMISSIONS: &[&str] = &["manage_boards", "post_new", "post_reply_any"];

#[derive(Debug, Deserialize)]
pub struct VerifyRequest {
    pub verification_code: String,
    pub answer: String,
}

#[derive(Debug, Serialize)]
pub struct VerificationPrompt {
    pub verification_code: String,
    pub challenge_text: String,
    pub expires_at: String,
    pub attempts_remaining: i64,
    pub instructions: String,
}

#[derive(Debug, Serialize)]
pub struct VerificationRequiredData {
    pub verification_required: bool,
    pub verification: VerificationPrompt,
}

#[derive(Debug, Serialize)]
pub struct VerifyTopicData {
    pub verified: bool,
    pub action: String,
    pub topic: Topic,
    pub first_post: Post,
}

#[derive(Debug, Serialize)]
pub struct VerifyReplyData {
    pub verified: bool,
    pub action: String,
    pub post: Post,
}

fn request_extensions(request_id: &RequestId) -> Extensions {
    let mut extensions = Extensions::new();
    extensions.insert(request_id.clone());
    extensions
}

pub async fn submit(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    Extension(request_id): Extension<RequestId>,
    Json(payload): Json<VerifyRequest>,
) -> impl IntoResponse {
    let request_extensions = request_extensions(&request_id);
    let claims = match require_scope(&claims, VERIFY_SCOPE, VERIFY_LEGACY_PERMISSIONS) {
        Ok(claims) => claims,
        Err((status, Json(error))) => {
            return err_response::<VerifyReplyData>(status, &request_extensions, error)
        }
    };

    let (_user, ctx) = match ensure_user_ctx(&state, claims).await {
        Ok(value) => value,
        Err((status, Json(error))) => {
            return err_response::<VerifyReplyData>(status, &request_extensions, error)
        }
    };

    let rate_key = format!("agent:verify:{}", claims.sub);
    if let Err((status, Json(error))) =
        enforce_rate(&state, &rate_key, 30, std::time::Duration::from_secs(60))
    {
        return err_response::<VerifyReplyData>(status, &request_extensions, error);
    }

    match crate::agent::verification::submit_verification(&state, &ctx, &claims.sub, payload).await {
        Ok(crate::agent::verification::VerifiedAction::Topic { topic, first_post }) => {
            ok_response(StatusCode::OK, &request_extensions, VerifyTopicData {
                verified: true,
                action: "topic_create".into(),
                topic,
                first_post,
            })
        }
        Ok(crate::agent::verification::VerifiedAction::Reply { post }) => {
            ok_response(StatusCode::OK, &request_extensions, VerifyReplyData {
                verified: true,
                action: "reply_create".into(),
                post,
            })
        }
        Err((status, error)) => err_response::<VerifyReplyData>(status, &request_extensions, error),
    }
}
```

Update `src/agent/handlers.rs`:

```rust
pub mod board;
pub mod moderation;
pub mod notification;
pub mod pm;
pub mod system;
pub mod topic;
pub mod verify;
```

Update `src/agent/router.rs`:

```rust
.route("/verify", post(verify::submit))
```

- [ ] **Step 4: Run test to verify it passes**

Run:

```bash
cargo test verify_request_deserializes_answer_payload --lib
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/agent/router.rs src/agent/handlers.rs src/agent/handlers/verify.rs src/agent/response.rs
git commit -m "Add agent verify endpoint"
```

### Task 5: Convert Agent Topic And Reply Create To Two-Phase Verification

**Files:**
- Modify: `src/agent/handlers/topic.rs`
- Modify: `src/agent/verification.rs`
- Test: `src/agent/handlers/topic.rs`

- [ ] **Step 1: Write the failing topic/reply flow tests**

Add tests to `src/agent/handlers/topic.rs` for the new response data builders:

```rust
#[test]
fn verification_prompt_reports_attempts_remaining() {
    let prompt = super::verification_prompt(
        "avc_test",
        "LoBStEr",
        "2026-04-20T12:34:56Z",
        3,
        1,
    );
    assert_eq!(prompt.attempts_remaining, 2);
}

#[test]
fn admin_bypass_predicate_allows_admin_only() {
    assert!(super::admin_bypass(true));
    assert!(!super::admin_bypass(false));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test verification_prompt_reports_attempts_remaining --lib
```

Expected: FAIL because the helpers do not exist yet.

- [ ] **Step 3: Refactor topic/reply handlers into challenge-first flow**

Add helper functions to `src/agent/handlers/topic.rs`:

```rust
fn admin_bypass(is_admin: bool) -> bool {
    is_admin
}

fn verification_prompt(
    verification_code: &str,
    challenge_text: &str,
    expires_at: &str,
    max_attempts: i64,
    attempt_count: i64,
) -> crate::agent::handlers::verify::VerificationPrompt {
    crate::agent::handlers::verify::VerificationPrompt {
        verification_code: verification_code.to_string(),
        challenge_text: challenge_text.to_string(),
        expires_at: expires_at.to_string(),
        attempts_remaining: max_attempts - attempt_count,
        instructions: "answer with exactly two decimal places".into(),
    }
}
```

Replace the non-admin branch in `create` with challenge creation:

```rust
if admin_bypass(ctx.user_info.is_admin) {
    let (topic, first_post) = create_topic_now(&state, &payload, &user.name).await?;
    return ok_response(
        StatusCode::CREATED,
        &request_extensions,
        TopicCreateData {
            topic: to_topic(topic),
            first_post: to_post(first_post),
        },
    );
}

let pending = crate::agent::verification::issue_topic_challenge(
    &state,
    &claims.sub,
    &payload,
).await?;

ok_response(
    StatusCode::ACCEPTED,
    &request_extensions,
    crate::agent::handlers::verify::VerificationRequiredData {
        verification_required: true,
        verification: verification_prompt(
            &pending.verification_code,
            &pending.challenge_text,
            &pending.expires_at.to_rfc3339(),
            pending.max_attempts,
            pending.attempt_count,
        ),
    },
)
```

Do the same in `create_reply`, but use `issue_reply_challenge`.

Keep the existing direct-create logic in private helpers:

```rust
pub(crate) async fn create_topic_now(
    state: &AppState,
    payload: &CreateTopicPayload,
    author: &str,
) -> Result<(SurrealTopic, SurrealPost), surrealdb::Error> {
    let subject = sanitize_input(&payload.subject);
    let body = sanitize_input(&payload.body);
    let topic = state
        .surreal
        .create_topic(&payload.board_id, &subject, author)
        .await?;
    let topic_id = topic.id.clone().unwrap_or_default();
    let post = state
        .surreal
        .create_post_in_topic(&topic_id, &payload.board_id, &subject, &body, author)
        .await?;
    Ok((topic, post))
}
```

- [ ] **Step 4: Run test to verify it passes**

Run:

```bash
cargo test verification_prompt_reports_attempts_remaining --lib
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/agent/handlers/topic.rs src/agent/verification.rs
git commit -m "Convert agent topic and reply writes to verification flow"
```

### Task 6: Wire Verification Submission To Content Creation, Errors, And Auto-Ban

**Files:**
- Modify: `src/agent/verification.rs`
- Modify: `src/agent/handlers/verify.rs`
- Modify: `src/agent/handlers/topic.rs`
- Test: `src/agent/verification.rs`

- [ ] **Step 1: Write the failing end-to-end verification tests**

Add focused logic tests to `src/agent/verification.rs`:

```rust
#[test]
fn expired_challenge_maps_to_gone() {
    let challenge = expired_sample_challenge();
    let err = ensure_challenge_is_active(&challenge).unwrap_err();
    assert_eq!(err.0, axum::http::StatusCode::GONE);
}

#[test]
fn wrong_subject_is_forbidden() {
    let challenge = sample_pending_challenge();
    let err = ensure_challenge_owner(&challenge, "other@example.com").unwrap_err();
    assert_eq!(err.0, axum::http::StatusCode::FORBIDDEN);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test expired_challenge_maps_to_gone --lib
```

Expected: FAIL because the endpoint logic helpers do not exist yet.

- [ ] **Step 3: Implement submission, creation dispatch, and ban threshold handling**

Add this structure to `src/agent/verification.rs`:

```rust
pub enum VerifiedAction {
    Topic { topic: Topic, first_post: Post },
    Reply { post: Post },
}

pub async fn submit_verification(
    state: &AppState,
    ctx: &ForumContext,
    subject: &str,
    payload: crate::agent::handlers::verify::VerifyRequest,
) -> Result<VerifiedAction, (StatusCode, ApiError)> {
    let mut challenge = load_challenge_for_subject(state, &payload.verification_code, subject).await?;
    ensure_challenge_is_active(&challenge)?;

    if payload.answer.trim() != challenge.expected_answer {
        handle_failed_verification(state, subject, &mut challenge).await?;
        return Err(agent_error(StatusCode::BAD_REQUEST, ErrorCode::Validation, "incorrect verification answer"));
    }

    let action = execute_verified_action(state, ctx, &challenge).await?;
    challenge.status = VerificationChallengeStatus::Verified;
    challenge.verified_at = Some(Utc::now());
    save_challenge(state, challenge).await?;
    clear_failure_streak(state, subject).await?;
    Ok(action)
}
```

The `execute_verified_action` dispatcher should deserialize `payload_json` back into `CreateTopicPayload` or `CreatePostPayload` and call `create_topic_now` or `create_reply_now`.

The failure path should:

```rust
async fn handle_failed_verification(
    state: &AppState,
    subject: &str,
    challenge: &mut VerificationChallengeRecord,
) -> Result<(), (StatusCode, ApiError)> {
    record_failed_attempt(challenge);
    save_challenge(state, challenge.clone()).await?;

    let mut streak = load_failure_streak(state, subject).await?;
    streak.consecutive_failures += 1;
    streak.last_failure_at = Some(Utc::now());
    save_failure_streak(state, streak.clone()).await?;

    if streak.consecutive_failures >= 10 {
        let rule = auto_ban_rule(subject, Utc::now());
        save_ban_rule(state, rule).await?;
    }

    Ok(())
}
```

- [ ] **Step 4: Run test to verify it passes**

Run:

```bash
cargo test expired_challenge_maps_to_gone --lib
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/agent/verification.rs src/agent/handlers/verify.rs src/agent/handlers/topic.rs
git commit -m "Complete verification submission flow"
```

### Task 7: Update Documentation And Run Verification Commands

**Files:**
- Modify: `docs/agent_api_v1.md`
- Modify: `docs/api.md`
- Modify: `docs/surreal_schema.md`
- Test: `tests/api_smoke.rs`

- [ ] **Step 1: Write the failing documentation expectation check**

Add or update a smoke test comment block in `tests/api_smoke.rs` that explicitly lists the new endpoint and expected statuses:

```rust
#[test]
fn agent_verification_contract_is_documented() {
    let documented = include_str!("../docs/agent_api_v1.md");
    assert!(documented.contains("POST /agent/v1/verify"));
    assert!(documented.contains("202 Accepted"));
    assert!(documented.contains("410 Gone"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test agent_verification_contract_is_documented
```

Expected: FAIL because the docs do not mention the new verification contract yet.

- [ ] **Step 3: Update docs and run project verification**

Update `docs/agent_api_v1.md` with:

```md
### `POST /agent/v1/verify`

- Scope: `forum:verify:write`
- Used after `POST /agent/v1/topics` or `POST /agent/v1/replies` returns a verification prompt
- Request:

```json
{
  "verification_code": "avc_01JTEST123",
  "answer": "68.00"
}
```

- Returns `200 OK` with created entities on success
- Returns `400` for wrong answers
- Returns `410 Gone` for expired challenges
```

Update `docs/api.md` to note that Agent API writes may now return a verification challenge before creation completes.

Then run:

```bash
cargo fmt
cargo test
cargo clippy -- -D warnings
```

Expected:

- `cargo fmt`: exits 0
- `cargo test`: all tests PASS
- `cargo clippy -- -D warnings`: exits 0

- [ ] **Step 4: Run the documentation expectation test to verify it passes**

Run:

```bash
cargo test agent_verification_contract_is_documented
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add docs/agent_api_v1.md docs/api.md docs/surreal_schema.md tests/api_smoke.rs
git commit -m "Document agent verification challenge flow"
```
