pub mod dto;
pub mod error;

pub use dto::admin::{
    ActionLogEntry, ActionLogsResponse, AdminAccount, AdminAccountsResponse, AdminGroup,
    AdminGroupsResponse, AdminNotifyPayload, AdminNotifyResponse, AdminUser, AdminUsersResponse,
    BanApplyResponse, BanListResponse, BanMemberView, BanPayload, BanRevokeResponse, BanRuleView,
    BoardAccessEntry, BoardAccessPayload, BoardAccessResponse, BoardPermissionEntry,
    BoardPermissionPayload, BoardPermissionResponse, UpdateBoardAccessResponse,
    UpdateBoardPermissionResponse,
};
pub use dto::attachment::{
    AttachmentCreateResponse, AttachmentDeletePayload, AttachmentDeleteResponse,
    AttachmentListResponse, AttachmentMeta, AttachmentUploadResponse, CreateAttachmentPayload,
};
pub use dto::auth::{
    AuthMeResponse, AuthResponse, AuthUser, LoginRequest, RegisterRequest, RegisterResponse,
};
pub use dto::board::{Board, BoardsResponse, CreateBoardPayload, CreateBoardResponse};
pub use dto::demo::{
    CreateSurrealPostPayload, DemoPostResponse, DemoSurrealResponse, HealthResponse,
    HealthSurrealStatus, MetricsResponse,
};
pub use dto::forum::{
    CreatePostPayload, CreateTopicPayload, Post, PostResponse, PostsResponse, Topic,
    TopicCreateResponse, TopicsResponse,
};
pub use dto::notification::{
    CreateNotificationPayload, MarkNotificationPayload, MarkNotificationResponse, Notification,
    NotificationCreateResponse, NotificationListResponse,
};
pub use dto::personal_message::{
    PersonalMessage, PersonalMessageIdsPayload, PersonalMessageIdsResponse,
    PersonalMessageListResponse, PersonalMessagePeer, PersonalMessageSendPayload,
    PersonalMessageSendResponse,
};
pub use error::{ApiError, ErrorCode};
