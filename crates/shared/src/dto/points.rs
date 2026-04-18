use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PointsMetric {
    Karma,
    Merit,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PointsEventKind {
    PostCreated,
    PostLiked,
    PostDeleted,
    MeritGranted,
    MeritRevoked,
    ManualAdjustment,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct PointsBalance {
    pub member_id: i64,
    pub karma: i64,
    pub merit: i64,
    pub last_event_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct PointsEvent {
    pub id: Option<String>,
    pub kind: PointsEventKind,
    pub target_member_id: i64,
    pub actor_member_id: Option<i64>,
    pub karma_delta: i64,
    pub merit_delta: i64,
    pub reason: Option<String>,
    pub reference_type: Option<String>,
    pub reference_id: Option<String>,
    pub idempotency_key: Option<String>,
    pub created_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct PointsUserSummary {
    pub member_id: i64,
    pub name: Option<String>,
    pub karma: i64,
    pub merit: i64,
    pub updated_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct CreatePointsEventPayload {
    pub kind: PointsEventKind,
    pub target_member_id: i64,
    pub actor_member_id: Option<i64>,
    #[serde(default)]
    pub karma_delta: i64,
    #[serde(default)]
    pub merit_delta: i64,
    pub reason: Option<String>,
    pub reference_type: Option<String>,
    pub reference_id: Option<String>,
    pub idempotency_key: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct PointsBalanceResponse {
    pub status: String,
    pub balance: PointsBalance,
    pub recent_events: Vec<PointsEvent>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct PointsEventCreateResponse {
    pub status: String,
    pub event: PointsEvent,
    pub balance: PointsBalance,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct PointsLeaderboardResponse {
    pub status: String,
    pub metric: PointsMetric,
    pub leaderboard: Vec<PointsUserSummary>,
}
