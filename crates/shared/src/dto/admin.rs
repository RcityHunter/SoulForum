use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct AdminUser {
    pub id: i64,
    pub name: String,
    pub primary_group: Option<i64>,
    pub additional_groups: Vec<i64>,
    pub warning: i32,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct AdminUsersResponse {
    pub status: String,
    pub members: Vec<AdminUser>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct AdminAccount {
    pub id: i64,
    pub name: String,
    pub role: Option<String>,
    pub permissions: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct AdminAccountsResponse {
    pub status: String,
    pub admins: Vec<AdminAccount>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct AdminGroup {
    pub id: i64,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct AdminGroupsResponse {
    pub status: String,
    pub groups: Vec<AdminGroup>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct BoardAccessEntry {
    pub id: String,
    pub name: String,
    pub allowed_groups: Vec<i64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct BoardAccessPayload {
    pub board_id: String,
    pub allowed_groups: Vec<i64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct BoardAccessResponse {
    pub status: String,
    pub entries: Vec<BoardAccessEntry>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct UpdateBoardAccessResponse {
    pub status: String,
    pub board_id: String,
    pub allowed_groups: Vec<i64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct BoardPermissionEntry {
    pub board_id: String,
    pub group_id: i64,
    pub allow: Vec<String>,
    pub deny: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct BoardPermissionPayload {
    pub board_id: String,
    pub group_id: i64,
    pub allow: Vec<String>,
    pub deny: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct BoardPermissionResponse {
    pub status: String,
    pub entries: Vec<BoardPermissionEntry>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct UpdateBoardPermissionResponse {
    pub status: String,
    pub board_id: String,
    pub group_id: i64,
    pub allow: Vec<String>,
    pub deny: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct BanMemberView {
    pub member_id: i64,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct BanRuleView {
    pub id: i64,
    pub expires_at: Option<String>,
    pub reason: Option<String>,
    #[serde(default)]
    pub members: Vec<BanMemberView>,
    #[serde(default)]
    pub emails: Vec<String>,
    #[serde(default)]
    pub ips: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct BanListResponse {
    pub status: String,
    pub bans: Vec<BanRuleView>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct AdminNotifyPayload {
    pub user_ids: Vec<i64>,
    pub subject: String,
    pub body: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct AdminNotifyResponse {
    pub status: String,
    pub sent_to: Vec<i64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct BanPayload {
    #[serde(default)]
    pub member_id: Option<i64>,
    #[serde(default)]
    pub ban_id: Option<i64>,
    pub reason: Option<String>,
    pub hours: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct BanApplyResponse {
    pub status: String,
    pub ban_id: i64,
    pub member_id: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct BanRevokeResponse {
    pub status: String,
    pub ban_id: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct ActionLogEntry {
    pub id: i64,
    pub action: String,
    pub member_id: Option<i64>,
    pub details: serde_json::Value,
    pub timestamp: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct ActionLogsResponse {
    pub status: String,
    pub logs: Vec<ActionLogEntry>,
}
