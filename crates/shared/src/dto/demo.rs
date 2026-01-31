use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct HealthSurrealStatus {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct HealthResponse {
    pub service: String,
    pub surreal: HealthSurrealStatus,
    pub timestamp: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct MetricsResponse {
    pub status: String,
    pub uptime_secs: u64,
    pub rate_limiter_keys: std::collections::HashMap<String, u32>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct DemoSurrealResponse {
    pub status: String,
    pub record: serde_json::Value,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct DemoPostResponse {
    pub status: String,
    pub topic_id: i64,
    pub post_id: i64,
    pub author: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct CreateSurrealPostPayload {
    pub board_id: String,
    pub subject: String,
    pub body: String,
}
