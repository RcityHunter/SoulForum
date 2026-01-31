use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct AttachmentMeta {
    pub id: Option<String>,
    pub filename: String,
    pub size_bytes: i64,
    pub mime_type: Option<String>,
    pub created_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct CreateAttachmentPayload {
    pub filename: String,
    pub size_bytes: i64,
    pub mime_type: Option<String>,
    pub board_id: Option<String>,
    pub topic_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct AttachmentListResponse {
    pub status: String,
    pub attachments: Vec<AttachmentMeta>,
    pub base_url: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct AttachmentCreateResponse {
    pub status: String,
    pub attachment: AttachmentMeta,
    pub base_url: Option<String>,
    pub url: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct AttachmentUploadResponse {
    pub status: String,
    pub attachment: AttachmentMeta,
    pub base_url: Option<String>,
    pub url: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct AttachmentDeletePayload {
    pub id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct AttachmentDeleteResponse {
    pub status: String,
    pub id: String,
}
