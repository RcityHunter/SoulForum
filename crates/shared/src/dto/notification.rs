use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Notification {
    pub id: String,
    pub user: String,
    pub subject: String,
    pub body: String,
    pub created_at: Option<String>,
    pub is_read: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct NotificationListResponse {
    pub status: String,
    pub notifications: Vec<Notification>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct NotificationCreateResponse {
    pub status: String,
    pub notification: Notification,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct CreateNotificationPayload {
    pub user: Option<String>,
    pub subject: String,
    pub body: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct MarkNotificationPayload {
    pub id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct MarkNotificationResponse {
    pub status: String,
    pub id: String,
    pub user: String,
}
