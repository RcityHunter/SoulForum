use crate::api::client::ApiClient;
use btc_forum_shared::{
    AdminAccountsResponse, AdminTransferPayload, AdminTransferResponse, AdminUsersResponse,
    BanApplyResponse, BanListResponse, BanPayload, BanRevokeResponse, BoardAccessPayload,
    BoardAccessResponse, BoardPermissionPayload, BoardPermissionResponse,
    DocsPermissionGrantByRecordPayload, DocsPermissionGrantResponse,
    DocsPermissionRevokeByRecordPayload, DocsPermissionRevokeResponse,
    ModeratorUpdateByRecordPayload, ModeratorUpdateResponse, UpdateBoardAccessResponse,
    UpdateBoardPermissionResponse,
};

pub async fn load_admin_users(
    client: &ApiClient,
    q: Option<&str>,
) -> Result<AdminUsersResponse, String> {
    let mut path = "/admin/users?limit=200".to_string();
    if let Some(q) = q {
        let q = q.trim();
        if !q.is_empty() {
            path.push_str("&q=");
            path.push_str(&urlencoding::encode(q));
        }
    }
    client.get_json(&path).await
}

pub async fn load_admin_accounts(client: &ApiClient) -> Result<AdminAccountsResponse, String> {
    client.get_json("/admin/admins").await
}

pub async fn assign_moderator_by_record(
    client: &ApiClient,
    payload: &ModeratorUpdateByRecordPayload,
) -> Result<ModeratorUpdateResponse, String> {
    client
        .post_json("/admin/moderators/assign_by_record", payload)
        .await
}

pub async fn revoke_moderator_by_record(
    client: &ApiClient,
    payload: &ModeratorUpdateByRecordPayload,
) -> Result<ModeratorUpdateResponse, String> {
    client
        .post_json("/admin/moderators/revoke_by_record", payload)
        .await
}

pub async fn grant_docs_space_create(
    client: &ApiClient,
    payload: &DocsPermissionGrantByRecordPayload,
) -> Result<DocsPermissionGrantResponse, String> {
    client
        .post_json("/admin/docs/grant_space_create_by_record", payload)
        .await
}

pub async fn revoke_docs_space_create(
    client: &ApiClient,
    payload: &DocsPermissionRevokeByRecordPayload,
) -> Result<DocsPermissionRevokeResponse, String> {
    client
        .post_json("/admin/docs/revoke_space_create_by_record", payload)
        .await
}

pub async fn transfer_admin(
    client: &ApiClient,
    payload: &AdminTransferPayload,
) -> Result<AdminTransferResponse, String> {
    client.post_json("/admin/admins/transfer", payload).await
}

pub async fn load_board_access(client: &ApiClient) -> Result<BoardAccessResponse, String> {
    client.get_json("/admin/board_access").await
}

pub async fn update_board_access(
    client: &ApiClient,
    payload: &BoardAccessPayload,
) -> Result<UpdateBoardAccessResponse, String> {
    client.post_json("/admin/board_access", payload).await
}

pub async fn load_board_permissions(client: &ApiClient) -> Result<BoardPermissionResponse, String> {
    client.get_json("/admin/board_permissions").await
}

pub async fn update_board_permissions(
    client: &ApiClient,
    payload: &BoardPermissionPayload,
) -> Result<UpdateBoardPermissionResponse, String> {
    client.post_json("/admin/board_permissions", payload).await
}

pub async fn load_bans(client: &ApiClient) -> Result<BanListResponse, String> {
    client.get_json("/admin/bans").await
}

pub async fn apply_ban(
    client: &ApiClient,
    payload: &BanPayload,
) -> Result<BanApplyResponse, String> {
    client.post_json("/admin/bans/apply", payload).await
}

pub async fn revoke_ban(
    client: &ApiClient,
    payload: &BanPayload,
) -> Result<BanRevokeResponse, String> {
    client.post_json("/admin/bans/revoke", payload).await
}
