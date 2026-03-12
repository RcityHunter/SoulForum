use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use thiserror::Error;

pub mod surreal;

pub type ServiceResult<T> = Result<T, ForumError>;

#[derive(Debug, Error)]
pub enum ForumError {
    #[error("permission denied: {0}")]
    PermissionDenied(String),
    #[error("session timeout")]
    SessionTimeout,
    #[error("language error: {0}")]
    Lang(String),
    #[error("validation error: {0}")]
    Validation(String),
    #[error("internal error: {0}")]
    Internal(String),
}

#[derive(Clone, Debug, Default)]
pub struct DataBag {
    inner: HashMap<String, Value>,
}

impl DataBag {
    pub fn new() -> Self {
        Self {
            inner: HashMap::new(),
        }
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.inner.get(key)
    }

    pub fn set<T: Serialize>(&mut self, key: &str, value: T) {
        self.inner.insert(
            key.to_string(),
            serde_json::to_value(value).unwrap_or(Value::Null),
        );
    }

    pub fn remove(&mut self, key: &str) {
        self.inner.remove(key);
    }

    pub fn bool(&self, key: &str) -> bool {
        self.inner
            .get(key)
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
    }

    pub fn int(&self, key: &str) -> Option<i64> {
        self.inner.get(key).and_then(|value| value.as_i64())
    }

    pub fn string(&self, key: &str) -> Option<String> {
        self.inner
            .get(key)
            .and_then(|value| value.as_str().map(|s| s.to_string()))
    }

    pub fn contains(&self, key: &str) -> bool {
        self.inner.contains_key(key)
    }

    pub fn increment(&mut self, key: &str, amount: i64) {
        let next = self.int(key).unwrap_or(0) + amount;
        self.set(key, next);
    }
}

#[derive(Clone, Debug, Default)]
pub struct RequestVars {
    data: DataBag,
}

impl RequestVars {
    pub fn new() -> Self {
        Self {
            data: DataBag::new(),
        }
    }

    pub fn bool(&self, key: &str) -> bool {
        self.data.bool(key)
    }

    pub fn int(&self, key: &str) -> Option<i64> {
        self.data.int(key)
    }

    pub fn string(&self, key: &str) -> Option<String> {
        self.data.string(key)
    }

    pub fn set<T: Serialize>(&mut self, key: &str, value: T) {
        self.data.set(key, value);
    }

    pub fn remove(&mut self, key: &str) {
        self.data.remove(key);
    }

    pub fn contains(&self, key: &str) -> bool {
        self.data.contains(key)
    }
}

#[derive(Clone, Debug)]
pub struct UserInfo {
    pub id: i64,
    pub is_guest: bool,
    pub is_admin: bool,
    pub is_mod: bool,
    pub posts: i64,
    pub messages: i64,
    pub unread_messages: i64,
    pub buddies: Vec<i64>,
    pub pm_ignore_list: Vec<i64>,
    pub pm_prefs: i32,
    pub pm_receive_from: i32,
    pub permissions: HashSet<String>,
    pub name: String,
    pub email: String,
    pub ip: String,
    pub query_wanna_see_board: String,
    pub language: String,
    pub groups: Vec<i64>,
    pub warning: i32,
    pub possibly_robot: bool,
}

impl Default for UserInfo {
    fn default() -> Self {
        Self {
            id: 0,
            is_guest: true,
            is_admin: false,
            is_mod: false,
            posts: 0,
            messages: 0,
            unread_messages: 0,
            buddies: Vec::new(),
            pm_ignore_list: Vec::new(),
            pm_prefs: 0,
            pm_receive_from: 0,
            permissions: HashSet::new(),
            name: String::from("Guest"),
            email: String::new(),
            ip: String::from("127.0.0.1"),
            query_wanna_see_board: String::new(),
            language: String::from("en_US"),
            groups: vec![4],
            warning: 0,
            possibly_robot: false,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct ForumContext {
    pub scripturl: String,
    pub board_id: Option<i64>,
    pub topic_id: Option<i64>,
    pub txt: DataBag,
    pub settings: DataBag,
    pub mod_settings: DataBag,
    pub context: DataBag,
    pub request: RequestVars,
    pub post_vars: RequestVars,
    pub session: DataBag,
    pub options: DataBag,
    pub user_info: UserInfo,
    pub board_info: DataBag,
    pub topic_info: DataBag,
    pub smc_func: DataBag,
}

#[derive(Clone, Debug, Default)]
pub struct NotifyPrefs {
    pub msg_auto_notify: bool,
}

#[derive(Clone, Debug, Default)]
pub struct BoardListOptions {
    pub included_boards: Option<Vec<i64>>,
    pub not_redirection: bool,
    pub use_permissions: bool,
    pub selected_board: Option<i64>,
}

#[derive(Clone, Debug, Default)]
pub struct BoardSummary {
    pub id: i64,
    pub name: String,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct BoardAccessEntry {
    pub id: String,
    pub name: String,
    pub allowed_groups: Vec<i64>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct PersonalMessageLabel {
    pub id: i64,
    pub name: String,
    pub messages: usize,
    pub unread: usize,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct PersonalMessagePeer {
    pub member_id: i64,
    pub name: String,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct PersonalMessageSummary {
    pub id: i64,
    pub subject: String,
    pub body_preview: String,
    pub sender_id: i64,
    pub sender_name: String,
    pub sent_at: DateTime<Utc>,
    pub is_read: bool,
    pub recipients: Vec<PersonalMessagePeer>,
    pub labels: Vec<i64>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct PersonalMessageDetail {
    pub id: i64,
    pub subject: String,
    pub body: String,
    pub sender_id: i64,
    pub sender_name: String,
    pub sent_at: DateTime<Utc>,
    pub recipients: Vec<PersonalMessagePeer>,
    pub labels: Vec<i64>,
    pub is_read: bool,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct PersonalMessagePage {
    pub start: usize,
    pub total: usize,
    pub unread: usize,
    pub messages: Vec<PersonalMessageSummary>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct PersonalMessageOverview {
    pub limit: Option<usize>,
    pub total: usize,
    pub unread: usize,
}

#[derive(Clone, Debug, Default)]
pub struct SendPersonalMessage {
    pub sender_id: i64,
    pub sender_name: String,
    pub to: Vec<i64>,
    pub bcc: Vec<i64>,
    pub subject: String,
    pub body: String,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct PersonalMessageSendResult {
    pub id: i64,
    pub recipient_ids: Vec<i64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PersonalMessageSearchQuery {
    pub text: String,
    pub member_filter: Option<i64>,
}

#[derive(Clone, Copy, Debug, Serialize, Eq, PartialEq)]
pub enum PersonalMessageFolder {
    Inbox,
    Sent,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct PmDraftRecord {
    pub id: i64,
    pub owner_id: i64,
    pub subject: String,
    pub body: String,
    pub to: Vec<i64>,
    pub bcc: Vec<i64>,
    pub saved_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct PmPreferenceState {
    pub receive_from: i32,
    pub notify_level: i32,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct MemberRecord {
    pub id: i64,
    pub name: String,
    pub primary_group: Option<i64>,
    pub additional_groups: Vec<i64>,
    pub password: String,
    pub warning: i32,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct PermissionChildGroup {
    pub id: i64,
    pub name: String,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct PermissionGroupContext {
    pub id: i64,
    pub name: String,
    pub num_members: i64,
    pub allow_delete: bool,
    pub allow_modify: bool,
    pub can_search: bool,
    pub help: Option<String>,
    pub is_post_group: bool,
    pub color: Option<String>,
    pub icons: Option<String>,
    pub children: Vec<PermissionChildGroup>,
    pub allowed: i64,
    pub denied: i64,
    pub access: bool,
    pub link: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct PermissionProfile {
    pub id: i64,
    pub name: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct PermissionDefinition {
    pub id: String,
    pub scope: PermissionScope,
    pub section: String,
    pub has_options: bool,
}

#[derive(Clone, Copy, Debug, Serialize, Eq, PartialEq, Hash)]
pub enum PermissionScope {
    Membergroup,
    Board,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct MembergroupSummary {
    pub id: i64,
    pub name: String,
    pub num_members: i64,
    pub color: Option<String>,
    pub is_post_group: bool,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct MembergroupData {
    pub id: Option<i64>,
    pub name: String,
    pub description: String,
    pub inherits_from: Option<i64>,
    pub allowed_boards: Vec<i64>,
    pub color: Option<String>,
    pub is_post_group: bool,
    pub min_posts: i64,
    pub group_type: i32,
    pub hidden: bool,
    pub icons: Option<String>,
    pub is_protected: bool,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct GroupMember {
    pub id: i64,
    pub name: String,
    pub primary: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GroupAssignType {
    OnlyPrimary,
    OnlyAdditional,
    ForcePrimary,
    Auto,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MembergroupListType {
    Regular,
    PostCount,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct MembergroupSettings {
    pub show_group_key: bool,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct MembergroupListEntry {
    pub id: i64,
    pub name: String,
    pub min_posts: i64,
    pub description: String,
    pub color: Option<String>,
    pub group_type: i32,
    pub num_members: i64,
    pub moderators: Vec<GroupMember>,
    pub icons: Option<String>,
    pub can_moderate: bool,
    pub hidden: bool,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct PermissionSnapshot {
    pub allowed: Vec<i64>,
    pub denied: Vec<i64>,
}

#[derive(Clone, Debug)]
pub struct PermissionChange {
    pub permission: String,
    pub allow: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BanAffects {
    Account { member_id: i64 },
    Email { value: String },
    Ip { value: String },
}

impl Default for BanAffects {
    fn default() -> Self {
        BanAffects::Account { member_id: 0 }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct BanCondition {
    pub id: i64,
    pub reason: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub affects: BanAffects,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct BanRule {
    pub id: i64,
    pub reason: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub cannot_post: bool,
    #[serde(default)]
    pub cannot_access: bool,
    pub conditions: Vec<BanCondition>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct BanLogEntry {
    pub id: i64,
    pub rule_id: i64,
    pub member_id: Option<i64>,
    pub email: Option<String>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct ActionLogEntry {
    pub id: i64,
    pub action: String,
    pub member_id: Option<i64>,
    pub details: Value,
    pub timestamp: DateTime<Utc>,
}

#[derive(Clone, Debug, Default)]
pub struct TopicPostingContext {
    pub locked: bool,
    pub approved: bool,
    pub notify: bool,
    pub sticky: bool,
    pub board_id: i64,
    pub poll_id: Option<i64>,
    pub last_msg_id: Option<i64>,
    pub first_msg_id: Option<i64>,
    pub id_member_started: i64,
    pub subject: Option<String>,
    pub last_post_time: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Default)]
pub struct AttachmentUpload {
    pub name: String,
    pub tmp_path: String,
    pub size: i64,
    pub mime_type: String,
    pub width: Option<i32>,
    pub height: Option<i32>,
}

#[derive(Clone, Debug, Default)]
pub struct AttachmentRecord {
    pub id: i64,
    pub name: String,
    pub size: i64,
    pub mime_type: String,
    pub approved: bool,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub message_id: Option<i64>,
}

#[derive(Clone, Debug)]
pub struct PollOption {
    pub id: i64,
    pub label: String,
    pub votes: i64,
}

#[derive(Clone, Debug)]
pub struct PollData {
    pub id: i64,
    pub topic_id: i64,
    pub question: String,
    pub options: Vec<PollOption>,
    pub max_votes: i32,
    pub change_vote: bool,
    pub guest_vote: bool,
}

#[derive(Clone, Debug, Default)]
pub struct CalendarEvent {
    pub id: Option<i64>,
    pub board_id: i64,
    pub topic_id: i64,
    pub title: String,
    pub location: String,
    pub member_id: i64,
}

#[derive(Clone, Debug)]
pub struct DraftStorage {
    pub id: i64,
    pub board_id: i64,
    pub topic_id: i64,
    pub subject: String,
    pub body: String,
    pub icon: String,
    pub smileys_enabled: bool,
    pub locked: bool,
    pub sticky: bool,
    pub poster_time: DateTime<Utc>,
}

impl Default for DraftStorage {
    fn default() -> Self {
        Self {
            id: 0,
            board_id: 0,
            topic_id: 0,
            subject: String::new(),
            body: String::new(),
            icon: String::from("xx"),
            smileys_enabled: true,
            locked: false,
            sticky: false,
            poster_time: Utc::now(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct AttachmentInfo {
    pub id: i64,
    pub filename: String,
    pub size: i64,
    pub approved: bool,
}

#[derive(Clone, Debug, Default)]
pub struct MessageEditData {
    pub id_member: i64,
    pub topic_id: i64,
    pub subject: String,
    pub body: String,
    pub icon: String,
    pub smileys_enabled: bool,
    pub approved: bool,
    pub attachments: Vec<AttachmentInfo>,
}

#[derive(Clone, Debug)]
pub struct MessageData {
    pub id: i64,
    pub topic_id: i64,
    pub subject: String,
    pub body: String,
    pub member_id: i64,
    pub approved: bool,
}

#[derive(Clone, Debug, Default)]
pub struct PostSubmission {
    pub topic_id: Option<i64>,
    pub board_id: i64,
    pub message_id: Option<i64>,
    pub subject: String,
    pub body: String,
    pub icon: String,
    pub approved: bool,
    pub send_notifications: bool,
}

#[derive(Clone, Debug, Default)]
pub struct PostedMessage {
    pub topic_id: i64,
    pub message_id: i64,
}

#[derive(Clone, Debug, Default)]
pub struct QuoteContent {
    pub subject: String,
    pub body: String,
}

#[derive(Clone, Debug, Default)]
pub struct AnnouncementResult {
    pub recipients: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionCheckMode {
    Get,
    Post,
    Request,
}

pub trait ForumService {
    fn load_language(&self, ctx: &mut ForumContext, lang: &str) -> ServiceResult<()>;
    fn load_template(&self, ctx: &mut ForumContext, template: &str) -> ServiceResult<()>;
    fn call_hook(&self, ctx: &mut ForumContext, hook: &str) -> ServiceResult<()>;
    fn get_notify_prefs(&self, user_id: i64) -> ServiceResult<NotifyPrefs>;
    fn boards_allowed_to(
        &self,
        ctx: &ForumContext,
        permissions: &[String],
    ) -> ServiceResult<Vec<i64>>;
    fn get_board_list(
        &self,
        ctx: &ForumContext,
        options: &BoardListOptions,
    ) -> ServiceResult<Vec<BoardSummary>>;
    fn check_session(&self, ctx: &ForumContext, mode: SessionCheckMode) -> ServiceResult<()>;
    fn allowed_to(
        &self,
        ctx: &ForumContext,
        permission: &str,
        boards: Option<&[i64]>,
        any: bool,
    ) -> bool;
    fn redirect_exit(&self, url: &str) -> ServiceResult<()>;
    fn find_topic_id_by_msg(&self, msg_id: i64) -> ServiceResult<Option<i64>>;
    fn fetch_topic_posting_context(
        &self,
        topic_id: i64,
    ) -> ServiceResult<Option<TopicPostingContext>>;
    fn fetch_message_edit_data(
        &self,
        topic_id: i64,
        msg_id: i64,
    ) -> ServiceResult<Option<MessageEditData>>;
    fn persist_post(
        &self,
        ctx: &ForumContext,
        submission: PostSubmission,
    ) -> ServiceResult<PostedMessage>;
    fn fetch_quote_content(&self, msg_id: i64) -> ServiceResult<Option<QuoteContent>>;
    fn send_announcement(&self, topic_id: i64) -> ServiceResult<AnnouncementResult>;
    fn store_attachment(&self, upload: AttachmentUpload) -> ServiceResult<AttachmentRecord>;
    fn delete_attachment(&self, id: i64) -> ServiceResult<()>;
    fn list_message_attachments(&self, msg_id: i64) -> ServiceResult<Vec<AttachmentRecord>>;
    fn link_attachment_to_message(&self, attachment_id: i64, msg_id: i64) -> ServiceResult<()>;
    fn current_attachment_dir(&self) -> ServiceResult<i64>;
    fn attachment_dir_usage(&self, dir_id: i64) -> ServiceResult<(i64, i64)>;
    fn update_attachment_dir_usage(
        &self,
        dir_id: i64,
        size_delta: i64,
        file_delta: i64,
    ) -> ServiceResult<()>;
    fn save_draft_record(&self, record: DraftStorage) -> ServiceResult<i64>;
    fn delete_draft(&self, draft_id: i64) -> ServiceResult<()>;
    fn read_draft(&self, draft_id: i64) -> ServiceResult<Option<DraftStorage>>;
    fn update_notify_pref(&self, user_id: i64, auto: bool) -> ServiceResult<NotifyPrefs>;
    fn can_link_event(&self, user_id: i64) -> ServiceResult<bool>;
    fn insert_event(&self, event: CalendarEvent) -> ServiceResult<i64>;
    fn modify_event(&self, event_id: i64, event: CalendarEvent) -> ServiceResult<()>;
    fn create_poll(&self, poll: PollData) -> ServiceResult<i64>;
    fn remove_poll(&self, poll_id: i64) -> ServiceResult<()>;
    fn lock_poll(&self, poll_id: i64, lock: bool) -> ServiceResult<()>;
    fn cast_vote(&self, poll_id: i64, member_id: i64, options: &[i64]) -> ServiceResult<()>;
    fn fetch_topic_messages(
        &self,
        topic_id: i64,
        start: i64,
        limit: i64,
    ) -> ServiceResult<Vec<MessageData>>;
    fn increment_topic_views(&self, topic_id: i64) -> ServiceResult<()>;
    fn list_membergroups(&self) -> ServiceResult<Vec<MembergroupSummary>>;
    fn get_membergroup(&self, group_id: i64) -> ServiceResult<Option<MembergroupData>>;
    fn save_membergroup(&self, group: MembergroupData) -> ServiceResult<i64>;
    fn list_group_members(&self, group_id: i64) -> ServiceResult<Vec<GroupMember>>;
    fn remove_members_from_group(&self, group_id: i64, members: &[i64]) -> ServiceResult<()>;
    fn get_membergroup_settings(&self) -> ServiceResult<MembergroupSettings>;
    fn save_membergroup_settings(&self, settings: MembergroupSettings) -> ServiceResult<()>;
    fn delete_membergroups(&self, group_ids: &[i64]) -> ServiceResult<()>;
    fn remove_members_from_groups(
        &self,
        member_ids: &[i64],
        group_ids: Option<&[i64]>,
    ) -> ServiceResult<()>;
    fn add_members_to_group(
        &self,
        member_ids: &[i64],
        group_id: i64,
        assign_type: GroupAssignType,
    ) -> ServiceResult<()>;
    fn list_membergroups_detailed(
        &self,
        group_type: MembergroupListType,
    ) -> ServiceResult<Vec<MembergroupListEntry>>;
    fn groups_with_permissions(
        &self,
        group_permissions: &[String],
        board_permissions: &[String],
        profile_id: i64,
    ) -> ServiceResult<HashMap<String, PermissionSnapshot>>;
    fn permission_groups(&self) -> ServiceResult<Vec<PermissionGroupContext>>;
    fn permission_profiles(&self) -> ServiceResult<Vec<PermissionProfile>>;
    fn ungrouped_member_count(&self) -> ServiceResult<i64>;
    fn get_member_record(&self, member_id: i64) -> ServiceResult<Option<MemberRecord>>;
    fn update_member_groups(
        &self,
        member_id: i64,
        primary_group: Option<i64>,
        additional_groups: &[i64],
    ) -> ServiceResult<()>;
    fn list_all_membergroups(&self) -> ServiceResult<Vec<MembergroupData>>;
    fn list_members(&self) -> ServiceResult<Vec<MemberRecord>>;
    fn delete_member(&self, member_id: i64) -> ServiceResult<()>;
    fn list_board_access(&self) -> ServiceResult<Vec<BoardAccessEntry>>;
    fn set_board_access(&self, board_id: &str, groups: &[i64]) -> ServiceResult<()>;
    fn fetch_alert_prefs(
        &self,
        members: &[i64],
        prefs: Option<&[String]>,
    ) -> ServiceResult<HashMap<i64, HashMap<String, i32>>>;
    fn set_alert_prefs(&self, member_id: i64, prefs: &[(String, i32)]) -> ServiceResult<()>;
    fn delete_alert_prefs(&self, member_id: i64, prefs: &[String]) -> ServiceResult<()>;
    fn get_member_email(&self, member_id: i64) -> ServiceResult<Option<String>>;
    fn notification_secret(&self) -> ServiceResult<String>;
    fn general_permissions(&self, group_ids: &[i64]) -> ServiceResult<Vec<PermissionChange>>;
    fn board_permissions(
        &self,
        board_id: &str,
        group_ids: &[i64],
    ) -> ServiceResult<Vec<PermissionChange>>;
    fn spider_group_id(&self) -> Option<i64>;
    fn settings_last_updated(&self) -> i64;
    fn list_ban_rules(&self) -> ServiceResult<Vec<BanRule>>;
    fn save_ban_rule(&self, rule: BanRule) -> ServiceResult<i64>;
    fn delete_ban_rule(&self, rule_id: i64) -> ServiceResult<()>;
    fn ban_logs(&self) -> ServiceResult<Vec<BanLogEntry>>;
    fn record_ban_hit(&self, bans: &[i64], email: Option<&str>) -> ServiceResult<()>;
    fn find_member_by_name(&self, name: &str) -> ServiceResult<Option<MemberRecord>>;
    fn find_members_by_name(&self, names: &[String]) -> ServiceResult<Vec<MemberRecord>>;
    fn cleanup_pm_recipients(&self, member_ids: &[i64]) -> ServiceResult<()>;
    fn cleanup_pm_ignore_lists(&self, member_ids: &[i64]) -> ServiceResult<()>;
    fn get_pm_ignore_list(&self, member_id: i64) -> ServiceResult<Vec<i64>>;
    fn set_pm_ignore_list(&self, member_id: i64, members: &[i64]) -> ServiceResult<()>;
    fn get_buddy_list(&self, member_id: i64) -> ServiceResult<Vec<i64>>;
    fn get_pm_preferences(&self, member_id: i64) -> ServiceResult<PmPreferenceState>;
    fn save_pm_preferences(&self, member_id: i64, prefs: &PmPreferenceState) -> ServiceResult<()>;
    fn record_pm_sent(&self, sender_id: i64, timestamp: DateTime<Utc>) -> ServiceResult<()>;
    fn count_pm_sent_since(&self, sender_id: i64, since: DateTime<Utc>) -> ServiceResult<usize>;
    fn log_action(
        &self,
        action: &str,
        member_id: Option<i64>,
        details: &Value,
    ) -> ServiceResult<()>;
    fn list_action_logs(&self) -> ServiceResult<Vec<ActionLogEntry>>;
    fn clean_expired_bans(&self) -> ServiceResult<usize>;
    fn save_pm_draft(&self, draft: PmDraftRecord) -> ServiceResult<i64>;
    fn delete_pm_draft(&self, owner_id: i64, draft_id: i64) -> ServiceResult<()>;
    fn list_pm_drafts(
        &self,
        owner_id: i64,
        start: usize,
        limit: usize,
    ) -> ServiceResult<Vec<PmDraftRecord>>;
    fn read_pm_draft(&self, owner_id: i64, draft_id: i64) -> ServiceResult<Option<PmDraftRecord>>;
    fn personal_message_overview(&self, user_id: i64) -> ServiceResult<PersonalMessageOverview>;
    fn personal_message_labels(&self, user_id: i64) -> ServiceResult<Vec<PersonalMessageLabel>>;
    fn personal_message_page(
        &self,
        user_id: i64,
        folder: PersonalMessageFolder,
        label: Option<i64>,
        start: usize,
        limit: usize,
    ) -> ServiceResult<PersonalMessagePage>;
    fn personal_message_popup(
        &self,
        user_id: i64,
        limit: usize,
    ) -> ServiceResult<Vec<PersonalMessageSummary>>;
    fn personal_message_detail(
        &self,
        user_id: i64,
        pm_id: i64,
    ) -> ServiceResult<Option<PersonalMessageDetail>>;
    fn send_personal_message(
        &self,
        request: SendPersonalMessage,
    ) -> ServiceResult<PersonalMessageSendResult>;
    fn delete_personal_messages(
        &self,
        user_id: i64,
        folder: PersonalMessageFolder,
        ids: &[i64],
    ) -> ServiceResult<()>;
    fn mark_personal_messages(&self, user_id: i64, ids: &[i64], read: bool) -> ServiceResult<()>;
    fn prune_personal_messages(&self, user_id: i64, days: i64) -> ServiceResult<usize>;
    fn clear_personal_messages(
        &self,
        user_id: i64,
        folder: PersonalMessageFolder,
    ) -> ServiceResult<usize>;
    fn create_pm_label(&self, user_id: i64, name: &str) -> ServiceResult<i64>;
    fn rename_pm_label(&self, user_id: i64, label_id: i64, name: &str) -> ServiceResult<()>;
    fn delete_pm_labels(&self, user_id: i64, labels: &[i64]) -> ServiceResult<()>;
    fn label_personal_messages(
        &self,
        user_id: i64,
        ids: &[i64],
        label_id: i64,
        add: bool,
    ) -> ServiceResult<()>;
    fn search_personal_messages(
        &self,
        user_id: i64,
        query: &PersonalMessageSearchQuery,
    ) -> ServiceResult<Vec<PersonalMessageSummary>>;
}
pub fn bool_to_value(value: bool) -> Value {
    Value::Bool(value)
}

pub fn int_to_value(value: i64) -> Value {
    Value::Number(value.into())
}

pub fn array_to_value(items: &[Value]) -> Value {
    Value::Array(items.to_vec())
}

pub fn serialize_option<T: Serialize>(value: Option<T>) -> Value {
    value
        .map(|inner| serde_json::to_value(inner).unwrap_or(Value::Null))
        .unwrap_or(Value::Null)
}

pub fn push_to_array<T: Serialize>(bag: &mut DataBag, key: &str, value: T) {
    let mut existing = bag
        .inner
        .get(key)
        .cloned()
        .and_then(|val| val.as_array().cloned())
        .unwrap_or_default();
    existing.push(serde_json::to_value(value).unwrap_or(Value::Null));
    bag.set(key, Value::Array(existing));
}

pub fn ensure(condition: bool, error: ForumError) -> ServiceResult<()> {
    if condition {
        Ok(())
    } else {
        Err(error)
    }
}

#[derive(Default)]
struct InMemoryState {
    boards: HashMap<i64, BoardSummary>,
    board_access: HashMap<String, Vec<i64>>,
    board_profiles: HashMap<i64, i64>,
    topics: HashMap<i64, TopicPostingContext>,
    messages: HashMap<i64, MessageEditData>,
    attachments: HashMap<i64, AttachmentRecord>,
    next_msg_id: i64,
    next_attach_id: i64,
    attachment_dirs: HashMap<i64, (i64, i64)>, // dir_id -> (size, files)
    current_dir: i64,
    drafts: HashMap<i64, DraftStorage>,
    next_draft_id: i64,
    notify_prefs: HashMap<i64, NotifyPrefs>,
    events: HashMap<i64, CalendarEvent>,
    next_event_id: i64,
    polls: HashMap<i64, PollData>,
    next_poll_id: i64,
    topic_views: HashMap<i64, i64>,
    membergroups: HashMap<i64, MembergroupData>,
    members: HashMap<i64, MemberRecord>,
    member_emails: HashMap<i64, String>,
    alert_prefs: HashMap<i64, HashMap<String, i32>>,
    group_permissions: HashMap<i64, Vec<PermissionChange>>,
    profile_permissions: HashMap<(i64, i64), Vec<PermissionChange>>,
    spider_group: Option<i64>,
    next_group_id: i64,
    group_settings: MembergroupSettings,
    permission_profiles: Vec<PermissionProfile>,
    auth_secret: String,
    settings_updated: i64,
    ban_rules: HashMap<i64, BanRule>,
    next_ban_rule_id: i64,
    next_ban_condition_id: i64,
    ban_logs: Vec<BanLogEntry>,
    next_ban_log_id: i64,
    action_logs: Vec<ActionLogEntry>,
    next_action_log_id: i64,
    personal_messages: HashMap<i64, StoredPersonalMessage>,
    next_pm_id: i64,
    pm_labels: HashMap<i64, HashMap<i64, String>>,
    pm_label_seq: HashMap<i64, i64>,
    pm_drafts: HashMap<i64, HashMap<i64, PmDraftRecord>>,
    next_pm_draft_id: i64,
    pm_ignore_lists: HashMap<i64, Vec<i64>>,
    buddy_lists: HashMap<i64, Vec<i64>>,
    pm_sent_log: HashMap<i64, Vec<DateTime<Utc>>>,
    pm_preferences: HashMap<i64, PmPreferenceState>,
}

#[derive(Clone, Debug)]
struct StoredPersonalMessage {
    id: i64,
    subject: String,
    body: String,
    sender_id: i64,
    sender_name: String,
    sent_at: DateTime<Utc>,
    sender_deleted: bool,
    recipients: Vec<PmRecipientState>,
}

#[derive(Clone, Debug)]
struct PmRecipientState {
    member_id: i64,
    name: String,
    is_read: bool,
    deleted: bool,
    labels: Vec<i64>,
}

#[derive(Clone, Debug)]
struct RecipientSnapshot {
    labels: Vec<i64>,
    is_read: bool,
}

type PmView<'a> = (&'a StoredPersonalMessage, Option<RecipientSnapshot>);

#[derive(Clone)]
pub struct InMemoryService {
    state: Arc<Mutex<InMemoryState>>,
}

impl InMemoryService {
    pub fn new_with_sample() -> Self {
        let mut state = InMemoryState::default();
        state.boards.insert(
            1,
            BoardSummary {
                id: 1,
                name: "General".into(),
            },
        );
        state.boards.insert(
            2,
            BoardSummary {
                id: 2,
                name: "Staff".into(),
            },
        );
        state.board_access.insert("2".into(), vec![1]);
        state.board_access.insert("1".into(), vec![0, 1]);
        state.board_profiles.insert(1, 1);
        state.board_profiles.insert(2, 2);
        state.topics.insert(
            1,
            TopicPostingContext {
                locked: false,
                approved: true,
                notify: false,
                sticky: false,
                board_id: 1,
                poll_id: None,
                last_msg_id: Some(1),
                first_msg_id: Some(1),
                id_member_started: 1,
                subject: Some("Welcome".into()),
                last_post_time: None,
            },
        );
        state.messages.insert(
            1,
            MessageEditData {
                id_member: 1,
                topic_id: 1,
                subject: "Welcome".into(),
                body: "Hello from PHP".into(),
                icon: "xx".into(),
                smileys_enabled: true,
                approved: true,
                attachments: vec![AttachmentInfo {
                    id: 10,
                    filename: "intro.txt".into(),
                    size: 128,
                    approved: true,
                }],
            },
        );
        state.next_msg_id = 2;
        state.attachments.insert(
            10,
            AttachmentRecord {
                id: 10,
                name: "intro.txt".into(),
                size: 128,
                mime_type: "text/plain".into(),
                approved: true,
                width: None,
                height: None,
                message_id: Some(1),
            },
        );
        state.next_attach_id = 11;
        state.attachment_dirs.insert(
            1,
            (
                state.attachments.values().map(|a| a.size).sum(),
                state.attachments.len() as i64,
            ),
        );
        state.current_dir = 1;
        state.next_draft_id = 1;
        state.next_event_id = 1;
        state.next_poll_id = 1;
        state.topic_views = HashMap::new();
        state.membergroups.insert(
            1,
            MembergroupData {
                id: Some(1),
                name: "Administrators".into(),
                description: "Site managers".into(),
                inherits_from: None,
                allowed_boards: vec![1],
                color: Some("#ff0000".into()),
                is_post_group: false,
                min_posts: -1,
                group_type: 0,
                hidden: false,
                icons: None,
                is_protected: true,
            },
        );
        state.membergroups.insert(
            3,
            MembergroupData {
                id: Some(3),
                name: "Global Moderators".into(),
                description: "Trusted moderators".into(),
                inherits_from: None,
                allowed_boards: vec![1],
                color: Some("#3366ff".into()),
                is_post_group: false,
                min_posts: -1,
                group_type: 0,
                hidden: false,
                icons: Some("2#star.png".into()),
                is_protected: false,
            },
        );
        state.membergroups.insert(
            4,
            MembergroupData {
                id: Some(4),
                name: "Jr. Members".into(),
                description: "New community members".into(),
                inherits_from: None,
                allowed_boards: vec![1],
                color: None,
                is_post_group: true,
                min_posts: 10,
                group_type: 1,
                hidden: false,
                icons: Some("1#dot.png".into()),
                is_protected: false,
            },
        );
        state.members.insert(
            1,
            MemberRecord {
                id: 1,
                name: "Alice".into(),
                primary_group: Some(1),
                additional_groups: Vec::new(),
                password: "password1".into(),
                warning: 0,
            },
        );
        state.member_emails.insert(1, "alice@example.com".into());
        state.members.insert(
            2,
            MemberRecord {
                id: 2,
                name: "Bob".into(),
                primary_group: Some(3),
                additional_groups: vec![4],
                password: "password2".into(),
                warning: 25,
            },
        );
        state.member_emails.insert(2, "bob@example.com".into());
        state.members.insert(
            3,
            MemberRecord {
                id: 3,
                name: "Carol".into(),
                primary_group: None,
                additional_groups: Vec::new(),
                password: "password3".into(),
                warning: 0,
            },
        );
        state.member_emails.insert(3, "carol@example.com".into());
        state.alert_prefs.insert(
            0,
            HashMap::from([("notify_board".into(), 1), ("notify_topic".into(), 1)]),
        );
        state
            .alert_prefs
            .insert(1, HashMap::from([("notify_board".into(), 0)]));
        state.alert_prefs.insert(2, HashMap::new());
        state.alert_prefs.insert(3, HashMap::new());
        state.personal_messages.insert(
            1,
            StoredPersonalMessage {
                id: 1,
                subject: "Welcome to PMs".into(),
                body: "Hi Bob, welcome to the forum!".into(),
                sender_id: 1,
                sender_name: "Alice".into(),
                sent_at: Utc::now(),
                sender_deleted: false,
                recipients: vec![
                    PmRecipientState {
                        member_id: 2,
                        name: "Bob".into(),
                        is_read: false,
                        deleted: false,
                        labels: Vec::new(),
                    },
                    PmRecipientState {
                        member_id: 3,
                        name: "Carol".into(),
                        is_read: true,
                        deleted: false,
                        labels: Vec::new(),
                    },
                ],
            },
        );
        state.next_pm_id = 2;
        state.pm_labels.insert(1, HashMap::new());
        state.pm_labels.insert(2, HashMap::new());
        state.pm_labels.insert(3, HashMap::new());
        state.pm_label_seq.insert(1, 1);
        state.pm_label_seq.insert(2, 1);
        state.pm_label_seq.insert(3, 1);
        state.pm_drafts.insert(1, HashMap::new());
        state.pm_drafts.insert(2, HashMap::new());
        state.pm_drafts.insert(3, HashMap::new());
        state.next_pm_draft_id = 1;
        state.pm_ignore_lists.insert(1, Vec::new());
        state.pm_ignore_lists.insert(2, Vec::new());
        state.pm_ignore_lists.insert(3, Vec::new());
        state.buddy_lists.insert(1, vec![2]);
        state.buddy_lists.insert(2, vec![1]);
        state.buddy_lists.insert(3, Vec::new());
        state.pm_sent_log = HashMap::new();
        state.pm_preferences.insert(1, PmPreferenceState::default());
        state.pm_preferences.insert(2, PmPreferenceState::default());
        state.pm_preferences.insert(3, PmPreferenceState::default());
        state.next_group_id = 5;
        state.permission_profiles = vec![
            PermissionProfile {
                id: 1,
                name: "Default".into(),
            },
            PermissionProfile {
                id: 2,
                name: "Read Only".into(),
            },
        ];
        state.group_permissions.insert(
            0,
            vec![
                PermissionChange {
                    permission: "profile_view".into(),
                    allow: true,
                },
                PermissionChange {
                    permission: "post_new".into(),
                    allow: true,
                },
            ],
        );
        state.group_permissions.insert(
            1,
            vec![PermissionChange {
                permission: "admin_forum".into(),
                allow: true,
            }],
        );
        state.group_permissions.insert(
            3,
            vec![PermissionChange {
                permission: "moderate_board".into(),
                allow: true,
            }],
        );
        state.profile_permissions.insert(
            (1, 0),
            vec![PermissionChange {
                permission: "post_reply_any".into(),
                allow: true,
            }],
        );
        state.profile_permissions.insert(
            (2, 1),
            vec![PermissionChange {
                permission: "view_staff_board".into(),
                allow: true,
            }],
        );
        state.spider_group = Some(4);
        state.auth_secret = "secret_key".into();
        state.settings_updated = 0;
        state.next_ban_rule_id = 2;
        state.next_ban_condition_id = 11;
        state.next_ban_log_id = 1;
        state.next_action_log_id = 1;
        state.ban_rules.insert(
            1,
            BanRule {
                id: 1,
                reason: Some("Spammer".into()),
                expires_at: None,
                cannot_post: true,
                cannot_access: false,
                conditions: vec![BanCondition {
                    id: 10,
                    reason: Some("Banned email".into()),
                    expires_at: None,
                    affects: BanAffects::Email {
                        value: "banned@example.com".into(),
                    },
                }],
            },
        );
        state.action_logs = Vec::new();
        Self {
            state: Arc::new(Mutex::new(state)),
        }
    }

    fn pm_recipient_views<'a>(
        &self,
        state: &'a InMemoryState,
        user_id: i64,
        label: Option<i64>,
    ) -> Vec<PmView<'a>> {
        let mut views = Vec::new();
        for pm in state.personal_messages.values() {
            if let Some(recipient) = pm
                .recipients
                .iter()
                .find(|rec| rec.member_id == user_id && !rec.deleted)
            {
                if let Some(label_id) = label {
                    if label_id >= 0 && !recipient.labels.contains(&label_id) {
                        continue;
                    }
                }
                views.push((
                    pm,
                    Some(RecipientSnapshot {
                        labels: recipient.labels.clone(),
                        is_read: recipient.is_read,
                    }),
                ));
            }
        }
        views
    }

    fn pm_sent_views<'a>(&self, state: &'a InMemoryState, user_id: i64) -> Vec<PmView<'a>> {
        state
            .personal_messages
            .values()
            .filter(|pm| pm.sender_id == user_id && !pm.sender_deleted)
            .map(|pm| (pm, None))
            .collect()
    }

    fn pm_summary(
        &self,
        pm: &StoredPersonalMessage,
        recipient: Option<RecipientSnapshot>,
    ) -> PersonalMessageSummary {
        PersonalMessageSummary {
            id: pm.id,
            subject: pm.subject.clone(),
            body_preview: Self::pm_snippet(&pm.body),
            sender_id: pm.sender_id,
            sender_name: pm.sender_name.clone(),
            sent_at: pm.sent_at,
            is_read: recipient.as_ref().map(|rec| rec.is_read).unwrap_or(true),
            recipients: pm
                .recipients
                .iter()
                .map(|rec| PersonalMessagePeer {
                    member_id: rec.member_id,
                    name: rec.name.clone(),
                })
                .collect(),
            labels: recipient.map(|rec| rec.labels).unwrap_or_else(Vec::new),
        }
    }

    fn pm_detail(
        &self,
        pm: &StoredPersonalMessage,
        recipient: Option<RecipientSnapshot>,
    ) -> PersonalMessageDetail {
        PersonalMessageDetail {
            id: pm.id,
            subject: pm.subject.clone(),
            body: pm.body.clone(),
            sender_id: pm.sender_id,
            sender_name: pm.sender_name.clone(),
            sent_at: pm.sent_at,
            recipients: pm
                .recipients
                .iter()
                .map(|rec| PersonalMessagePeer {
                    member_id: rec.member_id,
                    name: rec.name.clone(),
                })
                .collect(),
            labels: recipient
                .as_ref()
                .map(|rec| rec.labels.clone())
                .unwrap_or_else(Vec::new),
            is_read: recipient.as_ref().map(|rec| rec.is_read).unwrap_or(true),
        }
    }

    fn pm_counts(&self, state: &InMemoryState, user_id: i64) -> (usize, usize) {
        let views = self.pm_recipient_views(state, user_id, None);
        let total = views.len();
        let unread = views
            .iter()
            .filter(|(_, rec)| rec.as_ref().map(|view| !view.is_read).unwrap_or(false))
            .count();
        (total, unread)
    }

    fn pm_snippet(text: &str) -> String {
        let mut snippet = text.trim().chars().take(120).collect::<String>();
        if text.len() > snippet.len() {
            snippet.push_str("...");
        }
        snippet
    }

    fn maybe_remove_pm(&self, state: &mut InMemoryState, pm_id: i64) {
        if let Some(pm) = state.personal_messages.get(&pm_id) {
            let recipients_left = pm.recipients.iter().any(|rec| !rec.deleted);
            if recipients_left || !pm.sender_deleted {
                return;
            }
        }
        state.personal_messages.remove(&pm_id);
    }
}

impl Default for InMemoryService {
    fn default() -> Self {
        Self::new_with_sample()
    }
}

impl ForumService for InMemoryService {
    fn load_language(&self, _ctx: &mut ForumContext, _lang: &str) -> ServiceResult<()> {
        Ok(())
    }

    fn load_template(&self, _ctx: &mut ForumContext, _template: &str) -> ServiceResult<()> {
        Ok(())
    }

    fn call_hook(&self, _ctx: &mut ForumContext, _hook: &str) -> ServiceResult<()> {
        Ok(())
    }

    fn get_notify_prefs(&self, _user_id: i64) -> ServiceResult<NotifyPrefs> {
        let state = self.state.lock().unwrap();
        Ok(state
            .notify_prefs
            .get(&_user_id)
            .cloned()
            .unwrap_or(NotifyPrefs {
                msg_auto_notify: false,
            }))
    }

    fn boards_allowed_to(
        &self,
        _ctx: &ForumContext,
        _permissions: &[String],
    ) -> ServiceResult<Vec<i64>> {
        let state = self.state.lock().unwrap();
        Ok(state.boards.keys().copied().collect())
    }

    fn get_board_list(
        &self,
        _ctx: &ForumContext,
        options: &BoardListOptions,
    ) -> ServiceResult<Vec<BoardSummary>> {
        let state = self.state.lock().unwrap();
        let mut boards: Vec<BoardSummary> = state
            .boards
            .values()
            .cloned()
            .filter(|board| {
                options
                    .included_boards
                    .as_ref()
                    .map(|allowed| allowed.contains(&board.id))
                    .unwrap_or(true)
            })
            .collect();
        if boards.is_empty() {
            if let Some(selected) = options.selected_board {
                if let Some(board) = state.boards.get(&selected) {
                    boards.push(board.clone());
                }
            }
        }
        Ok(boards)
    }

    fn check_session(&self, ctx: &ForumContext, _mode: SessionCheckMode) -> ServiceResult<()> {
        if ctx.session.bool("force_timeout") {
            Err(ForumError::SessionTimeout)
        } else {
            Ok(())
        }
    }

    fn allowed_to(
        &self,
        ctx: &ForumContext,
        permission: &str,
        _boards: Option<&[i64]>,
        _any: bool,
    ) -> bool {
        ctx.user_info.is_admin || ctx.user_info.permissions.contains(permission)
    }

    fn redirect_exit(&self, _url: &str) -> ServiceResult<()> {
        Ok(())
    }

    fn find_topic_id_by_msg(&self, msg_id: i64) -> ServiceResult<Option<i64>> {
        let state = self.state.lock().unwrap();
        Ok(state.messages.get(&msg_id).map(|m| m.topic_id))
    }

    fn fetch_topic_posting_context(
        &self,
        topic_id: i64,
    ) -> ServiceResult<Option<TopicPostingContext>> {
        let state = self.state.lock().unwrap();
        Ok(state.topics.get(&topic_id).cloned())
    }

    fn fetch_message_edit_data(
        &self,
        _topic_id: i64,
        msg_id: i64,
    ) -> ServiceResult<Option<MessageEditData>> {
        let state = self.state.lock().unwrap();
        Ok(state.messages.get(&msg_id).cloned())
    }

    fn persist_post(
        &self,
        _ctx: &ForumContext,
        submission: PostSubmission,
    ) -> ServiceResult<PostedMessage> {
        if submission.subject.trim().is_empty() || submission.body.trim().is_empty() {
            return Err(ForumError::Validation("no_subject_or_body".into()));
        }
        let mut state = self.state.lock().unwrap();
        let _notifications = submission.send_notifications;
        let topic_id = if let Some(id) = submission.topic_id {
            id
        } else {
            let new_id = state.topics.keys().max().copied().unwrap_or(0) + 1;
            state.topics.insert(
                new_id,
                TopicPostingContext {
                    locked: false,
                    approved: submission.approved,
                    notify: false,
                    sticky: false,
                    board_id: submission.board_id,
                    poll_id: None,
                    last_msg_id: None,
                    first_msg_id: None,
                    id_member_started: 0,
                    subject: Some(submission.subject.clone()),
                    last_post_time: None,
                },
            );
            new_id
        };

        let message_id = if let Some(existing) = submission.message_id {
            if let Some(message) = state.messages.get_mut(&existing) {
                message.subject = submission.subject.clone();
                message.body = submission.body.clone();
                message.icon = submission.icon.clone();
                message.approved = submission.approved;
            }
            existing
        } else {
            let new_id = state.next_msg_id;
            state.next_msg_id += 1;
            state.messages.insert(
                new_id,
                MessageEditData {
                    id_member: 1,
                    topic_id,
                    subject: submission.subject.clone(),
                    body: submission.body.clone(),
                    icon: submission.icon.clone(),
                    smileys_enabled: true,
                    approved: submission.approved,
                    attachments: Vec::new(),
                },
            );
            new_id
        };

        if let Some(topic) = state.topics.get_mut(&topic_id) {
            topic.last_msg_id = Some(message_id);
            if topic.first_msg_id.is_none() {
                topic.first_msg_id = Some(message_id);
            }
            topic.subject = Some(submission.subject.clone());
        }

        Ok(PostedMessage {
            topic_id,
            message_id,
        })
    }

    fn fetch_quote_content(&self, msg_id: i64) -> ServiceResult<Option<QuoteContent>> {
        let state = self.state.lock().unwrap();
        Ok(state.messages.get(&msg_id).map(|msg| QuoteContent {
            subject: msg.subject.clone(),
            body: msg.body.clone(),
        }))
    }

    fn send_announcement(&self, _topic_id: i64) -> ServiceResult<AnnouncementResult> {
        Ok(AnnouncementResult { recipients: 1 })
    }

    fn store_attachment(&self, upload: AttachmentUpload) -> ServiceResult<AttachmentRecord> {
        let mut state = self.state.lock().unwrap();
        let id = state.next_attach_id;
        state.next_attach_id += 1;
        let record = AttachmentRecord {
            id,
            name: upload.name,
            size: upload.size,
            mime_type: upload.mime_type,
            approved: true,
            width: upload.width,
            height: upload.height,
            message_id: None,
        };
        let dir_id = state.current_dir;
        state.attachments.insert(id, record.clone());
        let entry = state.attachment_dirs.entry(dir_id).or_insert((0, 0));
        entry.0 += record.size;
        entry.1 += 1;
        Ok(record)
    }

    fn delete_attachment(&self, id: i64) -> ServiceResult<()> {
        let mut state = self.state.lock().unwrap();
        state.attachments.remove(&id);
        Ok(())
    }

    fn list_message_attachments(&self, msg_id: i64) -> ServiceResult<Vec<AttachmentRecord>> {
        let state = self.state.lock().unwrap();
        Ok(state
            .attachments
            .values()
            .cloned()
            .filter(|att| att.message_id == Some(msg_id))
            .collect())
    }

    fn link_attachment_to_message(&self, attachment_id: i64, msg_id: i64) -> ServiceResult<()> {
        let mut state = self.state.lock().unwrap();
        if let Some(att) = state.attachments.get_mut(&attachment_id) {
            att.message_id = Some(msg_id);
            Ok(())
        } else {
            Err(ForumError::Validation("attachment_not_found".into()))
        }
    }

    fn current_attachment_dir(&self) -> ServiceResult<i64> {
        let state = self.state.lock().unwrap();
        Ok(state.current_dir)
    }

    fn attachment_dir_usage(&self, dir_id: i64) -> ServiceResult<(i64, i64)> {
        let state = self.state.lock().unwrap();
        Ok(state
            .attachment_dirs
            .get(&dir_id)
            .cloned()
            .unwrap_or((0, 0)))
    }

    fn update_attachment_dir_usage(
        &self,
        dir_id: i64,
        size_delta: i64,
        file_delta: i64,
    ) -> ServiceResult<()> {
        let mut state = self.state.lock().unwrap();
        let entry = state.attachment_dirs.entry(dir_id).or_insert((0, 0));
        entry.0 += size_delta;
        entry.1 += file_delta;
        Ok(())
    }

    fn save_draft_record(&self, mut record: DraftStorage) -> ServiceResult<i64> {
        let mut state = self.state.lock().unwrap();
        if record.id == 0 {
            record.id = state.next_draft_id;
            state.next_draft_id += 1;
        }
        let id = record.id;
        state.drafts.insert(id, record);
        Ok(id)
    }

    fn delete_draft(&self, draft_id: i64) -> ServiceResult<()> {
        let mut state = self.state.lock().unwrap();
        state.drafts.remove(&draft_id);
        Ok(())
    }

    fn read_draft(&self, draft_id: i64) -> ServiceResult<Option<DraftStorage>> {
        let state = self.state.lock().unwrap();
        Ok(state.drafts.get(&draft_id).cloned())
    }

    fn update_notify_pref(&self, user_id: i64, auto: bool) -> ServiceResult<NotifyPrefs> {
        let mut state = self.state.lock().unwrap();
        let prefs = NotifyPrefs {
            msg_auto_notify: auto,
        };
        state.notify_prefs.insert(user_id, prefs.clone());
        Ok(prefs)
    }

    fn can_link_event(&self, _user_id: i64) -> ServiceResult<bool> {
        Ok(true)
    }

    fn insert_event(&self, event: CalendarEvent) -> ServiceResult<i64> {
        let mut state = self.state.lock().unwrap();
        let id = state.next_event_id;
        state.next_event_id += 1;
        state.events.insert(
            id,
            CalendarEvent {
                id: Some(id),
                ..event
            },
        );
        Ok(id)
    }

    fn modify_event(&self, event_id: i64, event: CalendarEvent) -> ServiceResult<()> {
        let mut state = self.state.lock().unwrap();
        if let Some(existing) = state.events.get_mut(&event_id) {
            existing.title = event.title;
            existing.location = event.location;
            existing.board_id = event.board_id;
            existing.topic_id = event.topic_id;
            existing.member_id = event.member_id;
            Ok(())
        } else {
            Err(ForumError::Validation("event_not_found".into()))
        }
    }

    fn create_poll(&self, poll: PollData) -> ServiceResult<i64> {
        let mut state = self.state.lock().unwrap();
        let id = state.next_poll_id;
        state.next_poll_id += 1;
        state.polls.insert(id, poll);
        Ok(id)
    }

    fn remove_poll(&self, poll_id: i64) -> ServiceResult<()> {
        let mut state = self.state.lock().unwrap();
        state.polls.remove(&poll_id);
        Ok(())
    }

    fn lock_poll(&self, poll_id: i64, lock: bool) -> ServiceResult<()> {
        let mut state = self.state.lock().unwrap();
        if let Some(poll) = state.polls.get_mut(&poll_id) {
            poll.change_vote = !lock;
            Ok(())
        } else {
            Err(ForumError::Validation("poll_not_found".into()))
        }
    }

    fn cast_vote(&self, poll_id: i64, _member_id: i64, options: &[i64]) -> ServiceResult<()> {
        let mut state = self.state.lock().unwrap();
        if let Some(poll) = state.polls.get_mut(&poll_id) {
            for option_id in options {
                if let Some(opt) = poll.options.iter_mut().find(|opt| &opt.id == option_id) {
                    opt.votes += 1;
                }
            }
            Ok(())
        } else {
            Err(ForumError::Validation("poll_not_found".into()))
        }
    }

    fn fetch_topic_messages(
        &self,
        topic_id: i64,
        start: i64,
        limit: i64,
    ) -> ServiceResult<Vec<MessageData>> {
        let state = self.state.lock().unwrap();
        let mut messages: Vec<MessageData> = state
            .messages
            .iter()
            .filter(|(_, msg)| msg.topic_id == topic_id)
            .map(|(id, msg)| MessageData {
                id: *id,
                topic_id: msg.topic_id,
                subject: msg.subject.clone(),
                body: msg.body.clone(),
                member_id: msg.id_member,
                approved: msg.approved,
            })
            .collect();
        messages.sort_by_key(|msg| msg.id);
        let skip = start.max(0) as usize;
        let take = limit.max(0) as usize;
        let iter = messages.into_iter().skip(skip);
        let messages = if take > 0 {
            iter.take(take).collect()
        } else {
            iter.collect()
        };
        Ok(messages)
    }

    fn increment_topic_views(&self, topic_id: i64) -> ServiceResult<()> {
        let mut state = self.state.lock().unwrap();
        let entry = state.topic_views.entry(topic_id).or_insert(0);
        *entry += 1;
        Ok(())
    }

    fn list_membergroups(&self) -> ServiceResult<Vec<MembergroupSummary>> {
        let state = self.state.lock().unwrap();
        let mut groups: Vec<MembergroupSummary> = state
            .membergroups
            .iter()
            .map(|(&id, group)| {
                let num_members = state
                    .members
                    .values()
                    .filter(|member| {
                        member.primary_group == Some(id)
                            || member.additional_groups.iter().any(|gid| gid == &id)
                    })
                    .count() as i64;
                MembergroupSummary {
                    id,
                    name: group.name.clone(),
                    num_members,
                    color: group.color.clone(),
                    is_post_group: group.is_post_group,
                }
            })
            .collect();
        groups.sort_by_key(|group| group.id);
        Ok(groups)
    }

    fn get_membergroup(&self, group_id: i64) -> ServiceResult<Option<MembergroupData>> {
        let state = self.state.lock().unwrap();
        Ok(state.membergroups.get(&group_id).cloned())
    }

    fn save_membergroup(&self, mut group: MembergroupData) -> ServiceResult<i64> {
        let mut state = self.state.lock().unwrap();
        if state.next_group_id == 0 {
            state.next_group_id = 1;
        }
        let id = group.id.unwrap_or_else(|| {
            let id = state.next_group_id;
            state.next_group_id += 1;
            id
        });
        group.id = Some(id);
        state.membergroups.insert(id, group);
        Ok(id)
    }

    fn list_group_members(&self, group_id: i64) -> ServiceResult<Vec<GroupMember>> {
        let state = self.state.lock().unwrap();
        let mut members = Vec::new();
        for record in state.members.values() {
            if record.primary_group == Some(group_id) {
                members.push(GroupMember {
                    id: record.id,
                    name: record.name.clone(),
                    primary: true,
                });
            } else if record.additional_groups.iter().any(|gid| gid == &group_id) {
                members.push(GroupMember {
                    id: record.id,
                    name: record.name.clone(),
                    primary: false,
                });
            }
        }
        Ok(members)
    }

    fn remove_members_from_group(&self, group_id: i64, members: &[i64]) -> ServiceResult<()> {
        let mut state = self.state.lock().unwrap();
        for record in state.members.values_mut() {
            if members.contains(&record.id) {
                if record.primary_group == Some(group_id) {
                    record.primary_group = None;
                }
                record.additional_groups.retain(|gid| gid != &group_id);
            }
        }
        Ok(())
    }

    fn get_membergroup_settings(&self) -> ServiceResult<MembergroupSettings> {
        let state = self.state.lock().unwrap();
        Ok(state.group_settings.clone())
    }

    fn save_membergroup_settings(&self, settings: MembergroupSettings) -> ServiceResult<()> {
        let mut state = self.state.lock().unwrap();
        state.group_settings = settings;
        Ok(())
    }

    fn delete_membergroups(&self, group_ids: &[i64]) -> ServiceResult<()> {
        let mut state = self.state.lock().unwrap();
        for group_id in group_ids {
            state.membergroups.remove(group_id);
        }
        for member in state.members.values_mut() {
            if member
                .primary_group
                .map(|gid| group_ids.contains(&gid))
                .unwrap_or(false)
            {
                member.primary_group = None;
            }
            member
                .additional_groups
                .retain(|gid| !group_ids.contains(gid));
        }
        Ok(())
    }

    fn remove_members_from_groups(
        &self,
        member_ids: &[i64],
        group_ids: Option<&[i64]>,
    ) -> ServiceResult<()> {
        let mut state = self.state.lock().unwrap();
        for member_id in member_ids {
            if let Some(record) = state.members.get_mut(member_id) {
                match group_ids {
                    Some(groups) => {
                        if record
                            .primary_group
                            .map(|gid| groups.contains(&gid))
                            .unwrap_or(false)
                        {
                            record.primary_group = None;
                        }
                        record.additional_groups.retain(|gid| !groups.contains(gid));
                    }
                    None => {
                        record.primary_group = None;
                        record.additional_groups.clear();
                    }
                }
            }
        }
        Ok(())
    }

    fn add_members_to_group(
        &self,
        member_ids: &[i64],
        group_id: i64,
        assign_type: GroupAssignType,
    ) -> ServiceResult<()> {
        let mut state = self.state.lock().unwrap();
        if !state.membergroups.contains_key(&group_id) {
            return Err(ForumError::Validation("group_not_found".into()));
        }
        for member_id in member_ids {
            let entry = state
                .members
                .entry(*member_id)
                .or_insert_with(|| MemberRecord {
                    id: *member_id,
                    name: format!("Member {}", member_id),
                    primary_group: None,
                    additional_groups: Vec::new(),
                    password: String::new(),
                    warning: 0,
                });
            match assign_type {
                GroupAssignType::OnlyPrimary => {
                    if entry.primary_group.is_none() || entry.primary_group == Some(group_id) {
                        entry.primary_group = Some(group_id);
                    }
                }
                GroupAssignType::OnlyAdditional => {
                    if !entry.additional_groups.contains(&group_id) {
                        entry.additional_groups.push(group_id);
                    }
                }
                GroupAssignType::ForcePrimary => {
                    entry.primary_group = Some(group_id);
                }
                GroupAssignType::Auto => {
                    if entry.primary_group.is_none() {
                        entry.primary_group = Some(group_id);
                    } else if !entry.additional_groups.contains(&group_id) {
                        entry.additional_groups.push(group_id);
                    }
                }
            }
        }
        Ok(())
    }

    fn list_membergroups_detailed(
        &self,
        group_type: MembergroupListType,
    ) -> ServiceResult<Vec<MembergroupListEntry>> {
        let state = self.state.lock().unwrap();
        let mut entries = Vec::new();
        for (id, group) in state.membergroups.iter() {
            let is_post_group = group.min_posts != -1;
            match group_type {
                MembergroupListType::Regular if is_post_group => continue,
                MembergroupListType::PostCount if !is_post_group => continue,
                _ => {}
            }
            let num_members = state
                .members
                .values()
                .filter(|member| {
                    member.primary_group == Some(*id)
                        || member.additional_groups.iter().any(|gid| gid == id)
                })
                .count() as i64;
            entries.push(MembergroupListEntry {
                id: *id,
                name: group.name.clone(),
                min_posts: group.min_posts,
                description: group.description.clone(),
                color: group.color.clone(),
                group_type: group.group_type,
                num_members,
                moderators: Vec::new(),
                icons: group.icons.clone(),
                can_moderate: false,
                hidden: group.hidden,
            });
        }
        entries.sort_by_key(|entry| entry.id);
        Ok(entries)
    }

    fn groups_with_permissions(
        &self,
        group_permissions: &[String],
        board_permissions: &[String],
        _profile_id: i64,
    ) -> ServiceResult<HashMap<String, PermissionSnapshot>> {
        let mut map = HashMap::new();
        for permission in group_permissions {
            map.insert(
                permission.clone(),
                PermissionSnapshot {
                    allowed: vec![1],
                    denied: Vec::new(),
                },
            );
        }
        for permission in board_permissions {
            map.insert(
                permission.clone(),
                PermissionSnapshot {
                    allowed: vec![1],
                    denied: Vec::new(),
                },
            );
        }
        Ok(map)
    }

    fn permission_groups(&self) -> ServiceResult<Vec<PermissionGroupContext>> {
        let state = self.state.lock().unwrap();
        let ungrouped = state
            .members
            .values()
            .filter(|member| member.primary_group.is_none())
            .count() as i64;
        let mut groups = Vec::new();
        groups.push(PermissionGroupContext {
            id: -1,
            name: "Guests".into(),
            num_members: 0,
            allow_delete: false,
            allow_modify: true,
            can_search: false,
            help: Some("membergroup_guests".into()),
            is_post_group: false,
            color: None,
            icons: None,
            children: Vec::new(),
            allowed: 0,
            denied: 0,
            access: false,
            link: None,
        });
        groups.push(PermissionGroupContext {
            id: 0,
            name: "Regular Members".into(),
            num_members: ungrouped,
            allow_delete: false,
            allow_modify: true,
            can_search: false,
            help: Some("membergroup_regular_members".into()),
            is_post_group: false,
            color: None,
            icons: None,
            children: Vec::new(),
            allowed: 12,
            denied: 0,
            access: false,
            link: Some("?action=moderate;area=viewgroups;group=0".into()),
        });

        let total_permissions = 40;
        let mut children: HashMap<i64, Vec<PermissionChildGroup>> = HashMap::new();
        for (id, group) in state.membergroups.iter() {
            let num_members = state
                .members
                .values()
                .filter(|member| {
                    member.primary_group == Some(*id)
                        || member.additional_groups.iter().any(|gid| gid == id)
                })
                .count() as i64;
            let summary = PermissionGroupContext {
                id: *id,
                name: group.name.clone(),
                num_members,
                allow_delete: *id > 4,
                allow_modify: *id > 1,
                can_search: *id != 3,
                help: if *id == 1 {
                    Some("membergroup_administrator".into())
                } else if *id == 3 {
                    Some("membergroup_moderator".into())
                } else {
                    None
                },
                is_post_group: group.is_post_group,
                color: group.color.clone(),
                icons: group.icons.clone(),
                children: Vec::new(),
                allowed: if *id == 1 {
                    total_permissions
                } else {
                    total_permissions / 2
                },
                denied: if *id == 1 { 0 } else { 1 },
                access: *id != 3,
                link: Some(format!("?action=moderate;area=viewgroups;group={}", id)),
            };
            if let Some(parent) = group.inherits_from {
                children
                    .entry(parent)
                    .or_default()
                    .push(PermissionChildGroup {
                        id: *id,
                        name: group.name.clone(),
                    });
                continue;
            }
            groups.push(summary);
        }

        for group in &mut groups {
            if let Some(mut kids) = children.remove(&group.id) {
                kids.sort_by_key(|child| child.id);
                group.children = kids;
            }
        }
        groups.sort_by_key(|group| group.id);
        Ok(groups)
    }

    fn permission_profiles(&self) -> ServiceResult<Vec<PermissionProfile>> {
        let state = self.state.lock().unwrap();
        Ok(state.permission_profiles.clone())
    }

    fn ungrouped_member_count(&self) -> ServiceResult<i64> {
        let state = self.state.lock().unwrap();
        Ok(state
            .members
            .values()
            .filter(|member| member.primary_group.is_none())
            .count() as i64)
    }

    fn get_member_record(&self, member_id: i64) -> ServiceResult<Option<MemberRecord>> {
        let state = self.state.lock().unwrap();
        Ok(state.members.get(&member_id).cloned())
    }

    fn update_member_groups(
        &self,
        member_id: i64,
        primary_group: Option<i64>,
        additional_groups: &[i64],
    ) -> ServiceResult<()> {
        let mut state = self.state.lock().unwrap();
        if let Some(member) = state.members.get_mut(&member_id) {
            member.primary_group = primary_group;
            member.additional_groups = additional_groups.to_vec();
            Ok(())
        } else {
            Err(ForumError::Validation("member_not_found".into()))
        }
    }

    fn list_all_membergroups(&self) -> ServiceResult<Vec<MembergroupData>> {
        let state = self.state.lock().unwrap();
        Ok(state.membergroups.values().cloned().collect())
    }

    fn list_members(&self) -> ServiceResult<Vec<MemberRecord>> {
        let state = self.state.lock().unwrap();
        Ok(state.members.values().cloned().collect())
    }

    fn delete_member(&self, member_id: i64) -> ServiceResult<()> {
        let mut state = self.state.lock().unwrap();
        state.members.remove(&member_id);
        Ok(())
    }

    fn list_board_access(&self) -> ServiceResult<Vec<BoardAccessEntry>> {
        let state = self.state.lock().unwrap();
        Ok(state
            .boards
            .iter()
            .map(|(id, board)| BoardAccessEntry {
                id: id.to_string(),
                name: board.name.clone(),
                allowed_groups: state
                    .board_access
                    .get(&id.to_string())
                    .cloned()
                    .unwrap_or_else(|| vec![0]),
            })
            .collect())
    }

    fn set_board_access(&self, board_id: &str, groups: &[i64]) -> ServiceResult<()> {
        let mut state = self.state.lock().unwrap();
        state
            .board_access
            .insert(board_id.to_string(), groups.to_vec());
        Ok(())
    }

    fn fetch_alert_prefs(
        &self,
        members: &[i64],
        prefs: Option<&[String]>,
    ) -> ServiceResult<HashMap<i64, HashMap<String, i32>>> {
        let state = self.state.lock().unwrap();
        let mut result = HashMap::new();
        for member in members {
            if let Some(existing) = state.alert_prefs.get(member) {
                let filtered = if let Some(list) = prefs {
                    existing
                        .iter()
                        .filter(|(key, _)| list.contains(key))
                        .map(|(key, value)| (key.clone(), *value))
                        .collect()
                } else {
                    existing.clone()
                };
                result.insert(*member, filtered);
            }
        }
        Ok(result)
    }

    fn set_alert_prefs(&self, member_id: i64, prefs: &[(String, i32)]) -> ServiceResult<()> {
        let mut state = self.state.lock().unwrap();
        let entry = state.alert_prefs.entry(member_id).or_default();
        for (pref, value) in prefs {
            entry.insert(pref.clone(), *value);
        }
        Ok(())
    }

    fn delete_alert_prefs(&self, member_id: i64, prefs: &[String]) -> ServiceResult<()> {
        let mut state = self.state.lock().unwrap();
        if let Some(entry) = state.alert_prefs.get_mut(&member_id) {
            for pref in prefs {
                entry.remove(pref);
            }
        }
        Ok(())
    }

    fn get_member_email(&self, member_id: i64) -> ServiceResult<Option<String>> {
        let state = self.state.lock().unwrap();
        Ok(state.member_emails.get(&member_id).cloned())
    }

    fn notification_secret(&self) -> ServiceResult<String> {
        let state = self.state.lock().unwrap();
        Ok(state.auth_secret.clone())
    }

    fn general_permissions(&self, group_ids: &[i64]) -> ServiceResult<Vec<PermissionChange>> {
        let state = self.state.lock().unwrap();
        let mut list = Vec::new();
        for group in group_ids {
            if let Some(entries) = state.group_permissions.get(group) {
                list.extend(entries.clone());
            }
        }
        Ok(list)
    }

    fn board_permissions(
        &self,
        board_id: &str,
        group_ids: &[i64],
    ) -> ServiceResult<Vec<PermissionChange>> {
        let state = self.state.lock().unwrap();
        let board_numeric = board_id.parse::<i64>().ok();
        let profile = match board_numeric.and_then(|id| state.board_profiles.get(&id)) {
            Some(profile) => *profile,
            None => return Ok(Vec::new()),
        };
        let mut list = Vec::new();
        for group in group_ids {
            if let Some(entries) = state.profile_permissions.get(&(profile, *group)) {
                list.extend(entries.clone());
            }
        }
        Ok(list)
    }

    fn spider_group_id(&self) -> Option<i64> {
        let state = self.state.lock().unwrap();
        state.spider_group
    }

    fn settings_last_updated(&self) -> i64 {
        let state = self.state.lock().unwrap();
        state.settings_updated
    }

    fn list_ban_rules(&self) -> ServiceResult<Vec<BanRule>> {
        let state = self.state.lock().unwrap();
        Ok(state.ban_rules.values().cloned().collect())
    }

    fn save_ban_rule(&self, mut rule: BanRule) -> ServiceResult<i64> {
        let mut state = self.state.lock().unwrap();
        if rule.id == 0 {
            rule.id = state.next_ban_rule_id;
            state.next_ban_rule_id += 1;
        }
        for condition in &mut rule.conditions {
            if condition.id == 0 {
                condition.id = state.next_ban_condition_id;
                state.next_ban_condition_id += 1;
            }
        }
        let id = rule.id;
        state.ban_rules.insert(id, rule);
        Ok(id)
    }

    fn delete_ban_rule(&self, rule_id: i64) -> ServiceResult<()> {
        let mut state = self.state.lock().unwrap();
        state.ban_rules.remove(&rule_id);
        Ok(())
    }

    fn ban_logs(&self) -> ServiceResult<Vec<BanLogEntry>> {
        let state = self.state.lock().unwrap();
        Ok(state.ban_logs.clone())
    }

    fn record_ban_hit(&self, bans: &[i64], email: Option<&str>) -> ServiceResult<()> {
        let mut state = self.state.lock().unwrap();
        for rule_id in bans {
            let log_id = state.next_ban_log_id;
            state.next_ban_log_id += 1;
            state.ban_logs.push(BanLogEntry {
                id: log_id,
                rule_id: *rule_id,
                member_id: None,
                email: email.map(|val| val.to_string()),
                timestamp: Utc::now(),
            });
        }
        Ok(())
    }

    fn find_member_by_name(&self, name: &str) -> ServiceResult<Option<MemberRecord>> {
        let state = self.state.lock().unwrap();
        for record in state.members.values() {
            if record.name.eq_ignore_ascii_case(name) {
                return Ok(Some(record.clone()));
            }
        }
        Ok(None)
    }

    fn find_members_by_name(&self, names: &[String]) -> ServiceResult<Vec<MemberRecord>> {
        let state = self.state.lock().unwrap();
        if names.is_empty() {
            return Ok(Vec::new());
        }
        let mut lowered: HashSet<String> = names.iter().map(|n| n.to_lowercase()).collect();
        let mut results = Vec::new();
        for record in state.members.values() {
            if lowered.contains(&record.name.to_lowercase()) {
                results.push(record.clone());
                lowered.remove(&record.name.to_lowercase());
            }
            if lowered.is_empty() {
                break;
            }
        }
        Ok(results)
    }

    fn cleanup_pm_recipients(&self, member_ids: &[i64]) -> ServiceResult<()> {
        if member_ids.is_empty() {
            return Ok(());
        }
        let remove: HashSet<i64> = member_ids.iter().copied().collect();
        let mut state = self.state.lock().unwrap();
        let pm_ids: Vec<i64> = state.personal_messages.keys().copied().collect();
        for pm_id in pm_ids {
            {
                if let Some(pm) = state.personal_messages.get_mut(&pm_id) {
                    if remove.contains(&pm.sender_id) {
                        pm.sender_id = 0;
                        pm.sender_name = "Deleted Member".into();
                        pm.sender_deleted = true;
                    }
                    for recipient in &mut pm.recipients {
                        if remove.contains(&recipient.member_id) {
                            recipient.deleted = true;
                        }
                    }
                }
            }
            self.maybe_remove_pm(&mut state, pm_id);
        }
        Ok(())
    }

    fn cleanup_pm_ignore_lists(&self, member_ids: &[i64]) -> ServiceResult<()> {
        if member_ids.is_empty() {
            return Ok(());
        }
        let remove: HashSet<i64> = member_ids.iter().copied().collect();
        let mut state = self.state.lock().unwrap();
        for list in state.pm_ignore_lists.values_mut() {
            list.retain(|entry| !remove.contains(entry));
        }
        for list in state.buddy_lists.values_mut() {
            list.retain(|entry| !remove.contains(entry));
        }
        for member in member_ids {
            state.pm_ignore_lists.remove(member);
            state.buddy_lists.remove(member);
        }
        Ok(())
    }

    fn get_pm_ignore_list(&self, member_id: i64) -> ServiceResult<Vec<i64>> {
        let state = self.state.lock().unwrap();
        Ok(state
            .pm_ignore_lists
            .get(&member_id)
            .cloned()
            .unwrap_or_default())
    }

    fn set_pm_ignore_list(&self, member_id: i64, members: &[i64]) -> ServiceResult<()> {
        let mut state = self.state.lock().unwrap();
        state.pm_ignore_lists.insert(member_id, members.to_vec());
        Ok(())
    }

    fn get_buddy_list(&self, member_id: i64) -> ServiceResult<Vec<i64>> {
        let state = self.state.lock().unwrap();
        Ok(state
            .buddy_lists
            .get(&member_id)
            .cloned()
            .unwrap_or_default())
    }

    fn get_pm_preferences(&self, member_id: i64) -> ServiceResult<PmPreferenceState> {
        let state = self.state.lock().unwrap();
        Ok(state
            .pm_preferences
            .get(&member_id)
            .cloned()
            .unwrap_or_default())
    }

    fn save_pm_preferences(&self, member_id: i64, prefs: &PmPreferenceState) -> ServiceResult<()> {
        let mut state = self.state.lock().unwrap();
        state.pm_preferences.insert(member_id, prefs.clone());
        Ok(())
    }

    fn record_pm_sent(&self, sender_id: i64, timestamp: DateTime<Utc>) -> ServiceResult<()> {
        let mut state = self.state.lock().unwrap();
        let entry = state.pm_sent_log.entry(sender_id).or_default();
        entry.push(timestamp);
        let cutoff = timestamp - Duration::hours(1);
        entry.retain(|time| *time >= cutoff);
        Ok(())
    }

    fn count_pm_sent_since(&self, sender_id: i64, since: DateTime<Utc>) -> ServiceResult<usize> {
        let mut state = self.state.lock().unwrap();
        let entry = state.pm_sent_log.entry(sender_id).or_default();
        entry.retain(|time| *time >= since);
        Ok(entry.len())
    }

    fn log_action(
        &self,
        action: &str,
        member_id: Option<i64>,
        details: &Value,
    ) -> ServiceResult<()> {
        let mut state = self.state.lock().unwrap();
        let id = state.next_action_log_id;
        state.next_action_log_id += 1;
        state.action_logs.push(ActionLogEntry {
            id,
            action: action.into(),
            member_id,
            details: details.clone(),
            timestamp: Utc::now(),
        });
        Ok(())
    }

    fn list_action_logs(&self) -> ServiceResult<Vec<ActionLogEntry>> {
        let state = self.state.lock().unwrap();
        Ok(state.action_logs.clone())
    }

    fn personal_message_overview(&self, user_id: i64) -> ServiceResult<PersonalMessageOverview> {
        let state = self.state.lock().unwrap();
        let (total, unread) = self.pm_counts(&state, user_id);
        Ok(PersonalMessageOverview {
            limit: None,
            total,
            unread,
        })
    }

    fn personal_message_labels(&self, user_id: i64) -> ServiceResult<Vec<PersonalMessageLabel>> {
        let state = self.state.lock().unwrap();
        let views = self.pm_recipient_views(&state, user_id, None);
        let mut labels = Vec::new();
        let inbox_total = views.len();
        let inbox_unread = views
            .iter()
            .filter(|(_, rec)| rec.as_ref().map(|view| !view.is_read).unwrap_or(false))
            .count();
        labels.push(PersonalMessageLabel {
            id: -1,
            name: "Inbox".into(),
            messages: inbox_total,
            unread: inbox_unread,
        });
        if let Some(custom) = state.pm_labels.get(&user_id) {
            for (label_id, name) in custom {
                let messages = views
                    .iter()
                    .filter(|(_, rec)| {
                        rec.as_ref()
                            .map(|view| view.labels.contains(label_id))
                            .unwrap_or(false)
                    })
                    .count();
                let unread = views
                    .iter()
                    .filter(|(_, rec)| {
                        rec.as_ref()
                            .map(|view| view.labels.contains(label_id) && !view.is_read)
                            .unwrap_or(false)
                    })
                    .count();
                labels.push(PersonalMessageLabel {
                    id: *label_id,
                    name: name.clone(),
                    messages,
                    unread,
                });
            }
        }
        Ok(labels)
    }

    fn personal_message_page(
        &self,
        user_id: i64,
        folder: PersonalMessageFolder,
        label: Option<i64>,
        start: usize,
        limit: usize,
    ) -> ServiceResult<PersonalMessagePage> {
        let state = self.state.lock().unwrap();
        let mut entries = match folder {
            PersonalMessageFolder::Inbox => self.pm_recipient_views(&state, user_id, label),
            PersonalMessageFolder::Sent => self.pm_sent_views(&state, user_id),
        };
        entries.sort_by_key(|(pm, _)| pm.sent_at);
        entries.reverse();
        let total = entries.len();
        let unread = entries
            .iter()
            .filter(|(_, rec)| rec.as_ref().map(|view| !view.is_read).unwrap_or(false))
            .count();
        let slice = entries
            .into_iter()
            .skip(start)
            .take(limit.max(1))
            .map(|(pm, rec)| self.pm_summary(pm, rec))
            .collect();
        Ok(PersonalMessagePage {
            start,
            total,
            unread,
            messages: slice,
        })
    }

    fn personal_message_popup(
        &self,
        user_id: i64,
        limit: usize,
    ) -> ServiceResult<Vec<PersonalMessageSummary>> {
        let page =
            self.personal_message_page(user_id, PersonalMessageFolder::Inbox, None, 0, limit)?;
        Ok(page.messages)
    }

    fn personal_message_detail(
        &self,
        user_id: i64,
        pm_id: i64,
    ) -> ServiceResult<Option<PersonalMessageDetail>> {
        let mut state = self.state.lock().unwrap();
        let Some(pm) = state.personal_messages.get_mut(&pm_id) else {
            return Ok(None);
        };
        if pm.sender_id == user_id && !pm.sender_deleted {
            let detail = self.pm_detail(pm, None);
            return Ok(Some(detail));
        }
        if let Some(snapshot) = pm
            .recipients
            .iter_mut()
            .find(|rec| rec.member_id == user_id && !rec.deleted)
            .map(|recipient| {
                recipient.is_read = true;
                RecipientSnapshot {
                    labels: recipient.labels.clone(),
                    is_read: recipient.is_read,
                }
            })
        {
            let detail = self.pm_detail(pm, Some(snapshot));
            return Ok(Some(detail));
        }
        Ok(None)
    }

    fn send_personal_message(
        &self,
        request: SendPersonalMessage,
    ) -> ServiceResult<PersonalMessageSendResult> {
        let mut state = self.state.lock().unwrap();
        let mut recipients = Vec::new();
        let mut seen = HashSet::new();
        for target in request.to.iter().chain(request.bcc.iter()) {
            if !seen.insert(*target) {
                continue;
            }
            if let Some(member) = state.members.get(target) {
                recipients.push(PmRecipientState {
                    member_id: *target,
                    name: member.name.clone(),
                    is_read: false,
                    deleted: false,
                    labels: Vec::new(),
                });
            }
        }
        if recipients.is_empty() {
            return Err(ForumError::Validation("no_recipients".into()));
        }
        let id = state.next_pm_id;
        state.next_pm_id += 1;
        let sender_name = if let Some(member) = state.members.get(&request.sender_id) {
            member.name.clone()
        } else {
            request.sender_name.clone()
        };
        let pm = StoredPersonalMessage {
            id,
            subject: request.subject,
            body: request.body,
            sender_id: request.sender_id,
            sender_name,
            sent_at: Utc::now(),
            sender_deleted: false,
            recipients,
        };
        let recipient_ids = pm.recipients.iter().map(|rec| rec.member_id).collect();
        state.personal_messages.insert(id, pm);
        Ok(PersonalMessageSendResult { id, recipient_ids })
    }

    fn delete_personal_messages(
        &self,
        user_id: i64,
        folder: PersonalMessageFolder,
        ids: &[i64],
    ) -> ServiceResult<()> {
        let mut state = self.state.lock().unwrap();
        for pm_id in ids {
            if let Some(pm) = state.personal_messages.get_mut(pm_id) {
                match folder {
                    PersonalMessageFolder::Inbox => {
                        if let Some(rec) = pm
                            .recipients
                            .iter_mut()
                            .find(|rec| rec.member_id == user_id)
                        {
                            rec.deleted = true;
                        }
                    }
                    PersonalMessageFolder::Sent => {
                        if pm.sender_id == user_id {
                            pm.sender_deleted = true;
                        }
                    }
                }
            }
            self.maybe_remove_pm(&mut state, *pm_id);
        }
        Ok(())
    }

    fn mark_personal_messages(&self, user_id: i64, ids: &[i64], read: bool) -> ServiceResult<()> {
        let mut state = self.state.lock().unwrap();
        for pm_id in ids {
            if let Some(pm) = state.personal_messages.get_mut(pm_id) {
                if let Some(rec) = pm
                    .recipients
                    .iter_mut()
                    .find(|rec| rec.member_id == user_id && !rec.deleted)
                {
                    rec.is_read = read;
                }
            }
        }
        Ok(())
    }

    fn prune_personal_messages(&self, user_id: i64, days: i64) -> ServiceResult<usize> {
        let mut state = self.state.lock().unwrap();
        let threshold = Utc::now() - Duration::days(days.max(0) as i64);
        let mut removed = 0;
        let ids: Vec<i64> = state.personal_messages.keys().copied().collect();
        for pm_id in ids {
            if let Some(pm) = state.personal_messages.get_mut(&pm_id) {
                if pm.sent_at < threshold {
                    if pm.sender_id == user_id {
                        pm.sender_deleted = true;
                        removed += 1;
                    }
                    for rec in &mut pm.recipients {
                        if rec.member_id == user_id {
                            rec.deleted = true;
                        }
                    }
                }
            }
            self.maybe_remove_pm(&mut state, pm_id);
        }
        Ok(removed)
    }

    fn clear_personal_messages(
        &self,
        user_id: i64,
        folder: PersonalMessageFolder,
    ) -> ServiceResult<usize> {
        let mut state = self.state.lock().unwrap();
        let ids: Vec<i64> = state.personal_messages.keys().copied().collect();
        let mut removed = 0;
        for pm_id in ids {
            if let Some(pm) = state.personal_messages.get_mut(&pm_id) {
                match folder {
                    PersonalMessageFolder::Inbox => {
                        for rec in &mut pm.recipients {
                            if rec.member_id == user_id && !rec.deleted {
                                rec.deleted = true;
                                removed += 1;
                            }
                        }
                    }
                    PersonalMessageFolder::Sent => {
                        if pm.sender_id == user_id && !pm.sender_deleted {
                            pm.sender_deleted = true;
                            removed += 1;
                        }
                    }
                }
            }
            self.maybe_remove_pm(&mut state, pm_id);
        }
        Ok(removed)
    }

    fn create_pm_label(&self, user_id: i64, name: &str) -> ServiceResult<i64> {
        let mut state = self.state.lock().unwrap();
        let next_id = state.pm_label_seq.entry(user_id).or_insert(1);
        let label_id = *next_id;
        *next_id += 1;
        state
            .pm_labels
            .entry(user_id)
            .or_default()
            .insert(label_id, name.to_string());
        Ok(label_id)
    }

    fn rename_pm_label(&self, user_id: i64, label_id: i64, name: &str) -> ServiceResult<()> {
        let mut state = self.state.lock().unwrap();
        if let Some(labels) = state.pm_labels.get_mut(&user_id) {
            if let Some(entry) = labels.get_mut(&label_id) {
                *entry = name.to_string();
            }
        }
        Ok(())
    }

    fn delete_pm_labels(&self, user_id: i64, labels: &[i64]) -> ServiceResult<()> {
        let mut state = self.state.lock().unwrap();
        if let Some(store) = state.pm_labels.get_mut(&user_id) {
            for label in labels {
                store.remove(label);
            }
        }
        for pm in state.personal_messages.values_mut() {
            for rec in &mut pm.recipients {
                if rec.member_id == user_id {
                    rec.labels.retain(|label| !labels.contains(label));
                }
            }
        }
        Ok(())
    }

    fn label_personal_messages(
        &self,
        user_id: i64,
        ids: &[i64],
        label_id: i64,
        add: bool,
    ) -> ServiceResult<()> {
        let mut state = self.state.lock().unwrap();
        for pm_id in ids {
            if let Some(pm) = state.personal_messages.get_mut(pm_id) {
                if let Some(rec) = pm
                    .recipients
                    .iter_mut()
                    .find(|rec| rec.member_id == user_id && !rec.deleted)
                {
                    if add {
                        if !rec.labels.contains(&label_id) {
                            rec.labels.push(label_id);
                        }
                    } else {
                        rec.labels.retain(|label| *label != label_id);
                    }
                }
            }
        }
        Ok(())
    }

    fn search_personal_messages(
        &self,
        user_id: i64,
        query: &PersonalMessageSearchQuery,
    ) -> ServiceResult<Vec<PersonalMessageSummary>> {
        let state = self.state.lock().unwrap();
        let q = query.text.to_lowercase();
        let mut results = Vec::new();
        for pm in state.personal_messages.values() {
            let mut allowed = pm.sender_id == user_id && !pm.sender_deleted;
            let mut recipient_view = None;
            for rec in &pm.recipients {
                if rec.member_id == user_id && !rec.deleted {
                    allowed = true;
                    recipient_view = Some(RecipientSnapshot {
                        labels: rec.labels.clone(),
                        is_read: rec.is_read,
                    });
                }
            }
            if !allowed {
                continue;
            }
            if let Some(filter) = query.member_filter {
                if pm.sender_id != filter
                    && !pm.recipients.iter().any(|rec| rec.member_id == filter)
                {
                    continue;
                }
            }
            if !pm.subject.to_lowercase().contains(&q) && !pm.body.to_lowercase().contains(&q) {
                continue;
            }
            results.push(self.pm_summary(pm, recipient_view));
        }
        results.sort_by_key(|summary| summary.sent_at);
        results.reverse();
        Ok(results)
    }

    fn clean_expired_bans(&self) -> ServiceResult<usize> {
        let mut state = self.state.lock().unwrap();
        let now = Utc::now();
        let mut removed = 0;
        state.ban_rules.retain(|_, rule| {
            rule.conditions.retain(|condition| {
                condition
                    .expires_at
                    .map(|expires| expires > now)
                    .unwrap_or(true)
            });
            if rule.conditions.is_empty() {
                removed += 1;
                false
            } else {
                true
            }
        });
        Ok(removed)
    }

    fn save_pm_draft(&self, mut draft: PmDraftRecord) -> ServiceResult<i64> {
        let mut state = self.state.lock().unwrap();
        if draft.id == 0 {
            draft.id = state.next_pm_draft_id;
            state.next_pm_draft_id += 1;
        }
        draft.saved_at = Utc::now();
        let owner_entry = state.pm_drafts.entry(draft.owner_id).or_default();
        owner_entry.insert(draft.id, draft.clone());
        Ok(draft.id)
    }

    fn delete_pm_draft(&self, owner_id: i64, draft_id: i64) -> ServiceResult<()> {
        let mut state = self.state.lock().unwrap();
        if let Some(entry) = state.pm_drafts.get_mut(&owner_id) {
            entry.remove(&draft_id);
        }
        Ok(())
    }

    fn list_pm_drafts(
        &self,
        owner_id: i64,
        start: usize,
        limit: usize,
    ) -> ServiceResult<Vec<PmDraftRecord>> {
        let state = self.state.lock().unwrap();
        if let Some(entry) = state.pm_drafts.get(&owner_id) {
            let mut drafts: Vec<PmDraftRecord> = entry.values().cloned().collect();
            drafts.sort_by_key(|draft| draft.saved_at);
            drafts.reverse();
            let slice = drafts.into_iter().skip(start).take(limit.max(1)).collect();
            Ok(slice)
        } else {
            Ok(Vec::new())
        }
    }

    fn read_pm_draft(&self, owner_id: i64, draft_id: i64) -> ServiceResult<Option<PmDraftRecord>> {
        let state = self.state.lock().unwrap();
        Ok(state
            .pm_drafts
            .get(&owner_id)
            .and_then(|entry| entry.get(&draft_id).cloned()))
    }
}
