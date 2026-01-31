use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Topic {
    pub id: Option<String>,
    pub board_id: Option<String>,
    pub subject: String,
    pub author: String,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Post {
    pub id: Option<String>,
    pub topic_id: Option<String>,
    pub board_id: Option<String>,
    pub subject: String,
    pub body: String,
    pub author: String,
    pub created_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct TopicsResponse {
    pub status: String,
    pub topics: Vec<Topic>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct PostsResponse {
    pub status: String,
    pub posts: Vec<Post>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct TopicCreateResponse {
    pub status: String,
    pub topic: Topic,
    pub first_post: Post,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct PostResponse {
    pub status: String,
    pub post: Post,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct CreateTopicPayload {
    pub board_id: String,
    pub subject: String,
    pub body: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct CreatePostPayload {
    pub topic_id: String,
    pub board_id: String,
    pub subject: Option<String>,
    pub body: String,
}
