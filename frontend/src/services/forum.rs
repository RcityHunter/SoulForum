use crate::api::client::ApiClient;
use btc_forum_shared::{
    BoardsResponse, CreateBoardPayload, CreateBoardResponse, CreatePostPayload, CreateTopicPayload,
    PostResponse, PostsResponse, TopicCreateResponse, TopicsResponse,
};

pub async fn load_boards(client: &ApiClient) -> Result<BoardsResponse, String> {
    client.get_json("/surreal/boards").await
}

pub async fn load_topics(client: &ApiClient, board_id: &str) -> Result<TopicsResponse, String> {
    let path = format!("/surreal/topics?board_id={}", urlencoding::encode(board_id));
    client.get_json(&path).await
}

pub async fn load_posts(client: &ApiClient, topic_id: &str) -> Result<PostsResponse, String> {
    let path = format!(
        "/surreal/topic/posts?topic_id={}",
        urlencoding::encode(topic_id)
    );
    client.get_json(&path).await
}

pub async fn create_post(
    client: &ApiClient,
    payload: &CreatePostPayload,
) -> Result<PostResponse, String> {
    client.post_json("/surreal/topic/posts", payload).await
}

pub async fn create_board(
    client: &ApiClient,
    payload: &CreateBoardPayload,
) -> Result<CreateBoardResponse, String> {
    client.post_json("/surreal/boards", payload).await
}

pub async fn create_topic(
    client: &ApiClient,
    payload: &CreateTopicPayload,
) -> Result<TopicCreateResponse, String> {
    client.post_json("/surreal/topics", payload).await
}
