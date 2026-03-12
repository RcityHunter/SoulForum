use crate::api::client::ApiClient;
use btc_forum_shared::{
    CreateNotificationPayload, NotificationCreateResponse, NotificationListResponse,
};

pub async fn list_notifications(client: &ApiClient) -> Result<NotificationListResponse, String> {
    client.get_json("/surreal/notifications").await
}

pub async fn create_notification(
    client: &ApiClient,
    payload: &CreateNotificationPayload,
) -> Result<NotificationCreateResponse, String> {
    client.post_json("/surreal/notifications", payload).await
}
