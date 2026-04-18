use crate::api::client::ApiClient;
use btc_forum_shared::{
    AttachmentCreateResponse, AttachmentDeletePayload, AttachmentDeleteResponse,
    AttachmentListResponse, CreateAttachmentPayload,
};

pub async fn list_attachments(client: &ApiClient) -> Result<AttachmentListResponse, String> {
    client.get_json("/surreal/attachments").await
}

pub async fn create_attachment_meta(
    client: &ApiClient,
    payload: &CreateAttachmentPayload,
) -> Result<AttachmentCreateResponse, String> {
    client.post_json("/surreal/attachments", payload).await
}

pub async fn delete_attachment(
    client: &ApiClient,
    payload: &AttachmentDeletePayload,
) -> Result<AttachmentDeleteResponse, String> {
    client
        .post_json("/surreal/attachments/delete", payload)
        .await
}
