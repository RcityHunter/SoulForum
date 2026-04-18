use btc_forum_shared::PointsBalanceResponse;

use crate::api::client::ApiClient;

pub async fn load_my_points(client: &ApiClient) -> Result<PointsBalanceResponse, String> {
    client.get_json("/surreal/points/me").await
}
