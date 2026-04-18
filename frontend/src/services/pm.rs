use crate::api::client::ApiClient;
use btc_forum_shared::{
    PersonalMessageIdsPayload, PersonalMessageIdsResponse, PersonalMessageListResponse,
    PersonalMessageSendPayload, PersonalMessageSendResponse,
};

pub async fn load_pms(
    client: &ApiClient,
    folder: &str,
) -> Result<PersonalMessageListResponse, String> {
    let path = format!(
        "/surreal/personal_messages?folder={}",
        urlencoding::encode(folder)
    );
    client.get_json(&path).await
}

pub async fn mark_read(
    client: &ApiClient,
    payload: &PersonalMessageIdsPayload,
) -> Result<PersonalMessageIdsResponse, String> {
    client
        .post_json("/surreal/personal_messages/read", payload)
        .await
}

pub async fn delete(
    client: &ApiClient,
    payload: &PersonalMessageIdsPayload,
) -> Result<PersonalMessageIdsResponse, String> {
    client
        .post_json("/surreal/personal_messages/delete", payload)
        .await
}

pub async fn send(
    client: &ApiClient,
    payload: &PersonalMessageSendPayload,
) -> Result<PersonalMessageSendResponse, String> {
    client
        .post_json("/surreal/personal_messages/send", payload)
        .await
}
