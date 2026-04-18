use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct PersonalMessagePeer {
    pub member_id: i64,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct PersonalMessage {
    pub id: i64,
    pub subject: String,
    #[serde(rename = "body_preview")]
    pub body: String,
    pub sender_id: i64,
    pub sender_name: String,
    pub sent_at: String,
    pub is_read: bool,
    pub recipients: Vec<PersonalMessagePeer>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct PersonalMessageListResponse {
    pub status: String,
    pub messages: Vec<PersonalMessage>,
    pub total: usize,
    pub unread: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct PersonalMessageSendPayload {
    pub to: Vec<String>,
    pub subject: String,
    pub body: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct PersonalMessageIdsPayload {
    pub ids: Vec<i64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct PersonalMessageIdsResponse {
    pub status: String,
    pub ids: Vec<i64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct PersonalMessageSendResponse {
    pub status: String,
    pub sent_to: Vec<i64>,
}
