use btc_forum_shared::{
    CreatePointsEventPayload, PointsBalance, PointsEvent, PointsEventKind, PointsMetric,
    PointsUserSummary,
};
use serde_json::{json, Value};
use surrealdb::types::SurrealValue;

use crate::services::ForumError;
use crate::surreal::SurrealClient;

pub const TOPIC_CREATE_KARMA: i64 = 3;
pub const REPLY_CREATE_KARMA: i64 = 1;

#[derive(Debug, Clone, SurrealValue)]
struct PointsAggregateRow {
    member_id: Option<i64>,
    karma: Option<i64>,
    merit: Option<i64>,
    updated_at: Option<String>,
    last_event_at: Option<String>,
}

#[derive(Debug, Clone, SurrealValue)]
struct PointsEventRow {
    id: Option<String>,
    kind: Option<String>,
    target_member_id: Option<i64>,
    actor_member_id: Option<i64>,
    karma_delta: Option<i64>,
    merit_delta: Option<i64>,
    reason: Option<String>,
    reference_type: Option<String>,
    reference_id: Option<String>,
    idempotency_key: Option<String>,
    created_at: Option<String>,
}

#[derive(Debug, Clone, SurrealValue)]
struct LeaderboardRow {
    member_id: Option<i64>,
    karma: Option<i64>,
    merit: Option<i64>,
    updated_at: Option<String>,
    name: Option<String>,
}

pub fn validate_points_event(payload: &CreatePointsEventPayload) -> Result<(), ForumError> {
    if payload.target_member_id <= 0 {
        return Err(ForumError::Validation(
            "target_member_id must be positive".into(),
        ));
    }
    if payload.actor_member_id.is_some_and(|id| id <= 0) {
        return Err(ForumError::Validation(
            "actor_member_id must be positive".into(),
        ));
    }
    if payload.karma_delta == 0 && payload.merit_delta == 0 {
        return Err(ForumError::Validation(
            "at least one of karma_delta or merit_delta must be non-zero".into(),
        ));
    }
    if payload.karma_delta.abs() > 10_000 || payload.merit_delta.abs() > 10_000 {
        return Err(ForumError::Validation(
            "delta too large for v1 points skeleton".into(),
        ));
    }
    if matches!(
        payload.kind,
        PointsEventKind::MeritGranted | PointsEventKind::MeritRevoked
    ) && payload.merit_delta == 0
    {
        return Err(ForumError::Validation(
            "merit event requires non-zero merit_delta".into(),
        ));
    }
    if matches!(
        payload.kind,
        PointsEventKind::PostCreated | PointsEventKind::PostLiked | PointsEventKind::PostDeleted
    ) && payload.merit_delta != 0
    {
        return Err(ForumError::Validation(
            "post-driven events should not mutate merit in v1".into(),
        ));
    }
    if let Some(reason) = &payload.reason {
        if reason.len() > 280 {
            return Err(ForumError::Validation("reason too long".into()));
        }
    }
    Ok(())
}

pub fn parse_metric(raw: Option<&str>) -> PointsMetric {
    match raw.unwrap_or("karma").trim().to_ascii_lowercase().as_str() {
        "merit" => PointsMetric::Merit,
        _ => PointsMetric::Karma,
    }
}

pub fn topic_created_payload(member_id: i64, topic_id: &str) -> CreatePointsEventPayload {
    CreatePointsEventPayload {
        kind: PointsEventKind::PostCreated,
        target_member_id: member_id,
        actor_member_id: Some(member_id),
        karma_delta: TOPIC_CREATE_KARMA,
        merit_delta: 0,
        reason: Some("topic_created".into()),
        reference_type: Some("topic".into()),
        reference_id: Some(topic_id.to_string()),
        idempotency_key: Some(format!("points:topic_created:{member_id}:{topic_id}")),
    }
}

pub fn reply_created_payload(member_id: i64, post_id: &str) -> CreatePointsEventPayload {
    CreatePointsEventPayload {
        kind: PointsEventKind::PostCreated,
        target_member_id: member_id,
        actor_member_id: Some(member_id),
        karma_delta: REPLY_CREATE_KARMA,
        merit_delta: 0,
        reason: Some("reply_created".into()),
        reference_type: Some("post".into()),
        reference_id: Some(post_id.to_string()),
        idempotency_key: Some(format!("points:reply_created:{member_id}:{post_id}")),
    }
}

pub async fn get_points_balance(
    client: &SurrealClient,
    member_id: i64,
) -> Result<PointsBalance, ForumError> {
    let mut response = client
        .query(
            r#"
            SELECT member_id, karma, merit, updated_at, last_event_at
            FROM user_points
            WHERE member_id = $member_id
            LIMIT 1;
            "#,
        )
        .bind(("member_id", member_id))
        .await
        .map_err(|e| ForumError::Internal(e.to_string()))?;

    let row: Option<PointsAggregateRow> = response
        .take(0)
        .ok()
        .and_then(|mut rows: Vec<PointsAggregateRow>| rows.pop());

    Ok(row
        .map(|row| PointsBalance {
            member_id: row.member_id.unwrap_or(member_id),
            karma: row.karma.unwrap_or(0),
            merit: row.merit.unwrap_or(0),
            last_event_at: row.last_event_at,
            updated_at: row.updated_at,
        })
        .unwrap_or(PointsBalance {
            member_id,
            karma: 0,
            merit: 0,
            last_event_at: None,
            updated_at: None,
        }))
}

pub async fn list_recent_points_events(
    client: &SurrealClient,
    member_id: i64,
    limit: usize,
) -> Result<Vec<PointsEvent>, ForumError> {
    let mut response = client
        .query(
            r#"
            SELECT meta::id(id) as id, kind, target_member_id, actor_member_id, karma_delta, merit_delta,
                   reason, reference_type, reference_id, idempotency_key, created_at
            FROM points_events
            WHERE target_member_id = $member_id
            ORDER BY created_at DESC
            LIMIT $limit;
            "#,
        )
        .bind(("member_id", member_id))
        .bind(("limit", limit as i64))
        .await
        .map_err(|e| ForumError::Internal(e.to_string()))?;
    let rows: Vec<PointsEventRow> = response.take(0).unwrap_or_default();
    Ok(rows.into_iter().map(map_event_row).collect())
}

pub async fn create_points_event(
    client: &SurrealClient,
    payload: CreatePointsEventPayload,
) -> Result<(PointsEvent, PointsBalance), ForumError> {
    validate_points_event(&payload)?;

    if let Some(key) = payload.idempotency_key.as_deref() {
        let mut existing = client
            .query(
                r#"
                SELECT meta::id(id) as id, kind, target_member_id, actor_member_id, karma_delta, merit_delta,
                       reason, reference_type, reference_id, idempotency_key, created_at
                FROM points_events
                WHERE idempotency_key = $key
                LIMIT 1;
                "#,
            )
            .bind(("key", key.to_string()))
            .await
            .map_err(|e| ForumError::Internal(e.to_string()))?;
        let row: Option<PointsEventRow> = existing
            .take(0)
            .ok()
            .and_then(|mut rows: Vec<PointsEventRow>| rows.pop());
        if let Some(row) = row {
            let balance = get_points_balance(client, payload.target_member_id).await?;
            return Ok((map_event_row(row), balance));
        }
    }

    let event_id = format!(
        "{}:{}:{}:{}",
        payload.target_member_id,
        payload.actor_member_id.unwrap_or(0),
        payload.karma_delta,
        payload.merit_delta
    );
    let event_id = if let Some(key) = payload.idempotency_key.as_ref() {
        format!("event:{}", key)
    } else {
        format!(
            "event:{}:{}",
            event_id,
            chrono::Utc::now().timestamp_millis()
        )
    };

    let metadata = build_metadata(&payload);
    client
        .query(
            r#"
            BEGIN TRANSACTION;

            LET $existing = (SELECT VALUE meta::id(id) FROM points_events WHERE idempotency_key = $idempotency_key LIMIT 1);

            IF $idempotency_key = NONE OR count($existing) = 0 THEN {
                CREATE type::thing("points_events", $event_id) CONTENT {
                    kind: $kind,
                    target_member_id: $target_member_id,
                    actor_member_id: $actor_member_id,
                    karma_delta: $karma_delta,
                    merit_delta: $merit_delta,
                    reason: $reason,
                    reference_type: $reference_type,
                    reference_id: $reference_id,
                    idempotency_key: $idempotency_key,
                    metadata: $metadata,
                    created_at: time::now()
                };

                UPSERT type::thing("user_points", <string>$target_member_id) SET
                    member_id = $target_member_id,
                    karma = math::max([0, <int>(karma ?? 0) + $karma_delta]),
                    merit = math::max([0, <int>(merit ?? 0) + $merit_delta]),
                    last_event_at = time::now(),
                    updated_at = time::now();
            };

            COMMIT TRANSACTION;
            "#,
        )
        .bind(("event_id", event_id.clone()))
        .bind(("kind", event_kind_string(&payload.kind).to_string()))
        .bind(("target_member_id", payload.target_member_id))
        .bind(("actor_member_id", payload.actor_member_id))
        .bind(("karma_delta", payload.karma_delta))
        .bind(("merit_delta", payload.merit_delta))
        .bind(("reason", payload.reason.clone()))
        .bind(("reference_type", payload.reference_type.clone()))
        .bind(("reference_id", payload.reference_id.clone()))
        .bind(("idempotency_key", payload.idempotency_key.clone()))
        .bind(("metadata", metadata))
        .await
        .map_err(|e| ForumError::Internal(e.to_string()))?;

    let mut response = client
        .query(
            r#"
            SELECT meta::id(id) as id, kind, target_member_id, actor_member_id, karma_delta, merit_delta,
                   reason, reference_type, reference_id, idempotency_key, created_at
            FROM points_events
            WHERE (idempotency_key = $idempotency_key AND $idempotency_key != NONE)
               OR meta::id(id) = type::thing("points_events", $event_id)
            ORDER BY created_at DESC
            LIMIT 1;
            "#,
        )
        .bind(("idempotency_key", payload.idempotency_key.clone()))
        .bind(("event_id", event_id))
        .await
        .map_err(|e| ForumError::Internal(e.to_string()))?;
    let row: Option<PointsEventRow> = response
        .take(0)
        .ok()
        .and_then(|mut rows: Vec<PointsEventRow>| rows.pop());
    let event = row.map(map_event_row).ok_or_else(|| {
        ForumError::Internal("points event persisted but could not be reloaded".into())
    })?;
    let balance = get_points_balance(client, payload.target_member_id).await?;
    Ok((event, balance))
}

pub async fn leaderboard(
    client: &SurrealClient,
    metric: PointsMetric,
    limit: usize,
) -> Result<Vec<PointsUserSummary>, ForumError> {
    let order_field = match metric {
        PointsMetric::Karma => "karma",
        PointsMetric::Merit => "merit",
    };
    let query = format!(
        r#"
        SELECT p.member_id as member_id, p.karma as karma, p.merit as merit, p.updated_at as updated_at,
               (SELECT VALUE name FROM users WHERE meta::id(id) = type::thing("users", <string>p.member_id) LIMIT 1)[0] as name
        FROM user_points p
        ORDER BY {order_field} DESC, updated_at ASC
        LIMIT $limit;
        "#
    );
    let mut response = client
        .query(query)
        .bind(("limit", limit as i64))
        .await
        .map_err(|e| ForumError::Internal(e.to_string()))?;
    let rows: Vec<LeaderboardRow> = response.take(0).unwrap_or_default();
    Ok(rows
        .into_iter()
        .map(|row| PointsUserSummary {
            member_id: row.member_id.unwrap_or_default(),
            name: row.name,
            karma: row.karma.unwrap_or(0),
            merit: row.merit.unwrap_or(0),
            updated_at: row.updated_at,
        })
        .collect())
}

fn map_event_row(row: PointsEventRow) -> PointsEvent {
    PointsEvent {
        id: row.id,
        kind: parse_event_kind(row.kind.as_deref()),
        target_member_id: row.target_member_id.unwrap_or_default(),
        actor_member_id: row.actor_member_id,
        karma_delta: row.karma_delta.unwrap_or(0),
        merit_delta: row.merit_delta.unwrap_or(0),
        reason: row.reason,
        reference_type: row.reference_type,
        reference_id: row.reference_id,
        idempotency_key: row.idempotency_key,
        created_at: row.created_at,
    }
}

fn parse_event_kind(raw: Option<&str>) -> PointsEventKind {
    match raw.unwrap_or("manual_adjustment") {
        "post_created" => PointsEventKind::PostCreated,
        "post_liked" => PointsEventKind::PostLiked,
        "post_deleted" => PointsEventKind::PostDeleted,
        "merit_granted" => PointsEventKind::MeritGranted,
        "merit_revoked" => PointsEventKind::MeritRevoked,
        _ => PointsEventKind::ManualAdjustment,
    }
}

fn event_kind_string(kind: &PointsEventKind) -> &'static str {
    match kind {
        PointsEventKind::PostCreated => "post_created",
        PointsEventKind::PostLiked => "post_liked",
        PointsEventKind::PostDeleted => "post_deleted",
        PointsEventKind::MeritGranted => "merit_granted",
        PointsEventKind::MeritRevoked => "merit_revoked",
        PointsEventKind::ManualAdjustment => "manual_adjustment",
    }
}

fn build_metadata(payload: &CreatePointsEventPayload) -> Value {
    json!({
        "v": 1,
        "aggregate": "user_points",
        "axes": ["karma", "merit"],
        "reference_type": payload.reference_type,
        "reference_id": payload.reference_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_zero_delta() {
        let payload = CreatePointsEventPayload {
            kind: PointsEventKind::ManualAdjustment,
            target_member_id: 1,
            actor_member_id: Some(2),
            karma_delta: 0,
            merit_delta: 0,
            reason: None,
            reference_type: None,
            reference_id: None,
            idempotency_key: None,
        };
        assert!(validate_points_event(&payload).is_err());
    }

    #[test]
    fn rejects_merit_on_post_event() {
        let payload = CreatePointsEventPayload {
            kind: PointsEventKind::PostCreated,
            target_member_id: 1,
            actor_member_id: Some(2),
            karma_delta: 1,
            merit_delta: 1,
            reason: None,
            reference_type: Some("post".into()),
            reference_id: Some("posts:1".into()),
            idempotency_key: None,
        };
        assert!(validate_points_event(&payload).is_err());
    }

    #[test]
    fn parses_metric_default_to_karma() {
        assert_eq!(parse_metric(Some("merit")), PointsMetric::Merit);
        assert_eq!(parse_metric(Some("wat")), PointsMetric::Karma);
        assert_eq!(parse_metric(None), PointsMetric::Karma);
    }

    #[test]
    fn builds_topic_created_payload() {
        let payload = topic_created_payload(42, "topics:abc");
        assert_eq!(payload.kind, PointsEventKind::PostCreated);
        assert_eq!(payload.target_member_id, 42);
        assert_eq!(payload.actor_member_id, Some(42));
        assert_eq!(payload.karma_delta, TOPIC_CREATE_KARMA);
        assert_eq!(payload.reference_type.as_deref(), Some("topic"));
        assert_eq!(payload.reference_id.as_deref(), Some("topics:abc"));
        assert_eq!(payload.reason.as_deref(), Some("topic_created"));
        assert_eq!(
            payload.idempotency_key.as_deref(),
            Some("points:topic_created:42:topics:abc")
        );
    }

    #[test]
    fn builds_reply_created_payload() {
        let payload = reply_created_payload(7, "posts:def");
        assert_eq!(payload.kind, PointsEventKind::PostCreated);
        assert_eq!(payload.target_member_id, 7);
        assert_eq!(payload.actor_member_id, Some(7));
        assert_eq!(payload.karma_delta, REPLY_CREATE_KARMA);
        assert_eq!(payload.reference_type.as_deref(), Some("post"));
        assert_eq!(payload.reference_id.as_deref(), Some("posts:def"));
        assert_eq!(payload.reason.as_deref(), Some("reply_created"));
        assert_eq!(
            payload.idempotency_key.as_deref(),
            Some("points:reply_created:7:posts:def")
        );
    }
}
