use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Board {
    pub id: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct BoardsResponse {
    pub status: String,
    pub boards: Vec<Board>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct CreateBoardPayload {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct CreateBoardResponse {
    pub status: String,
    pub board: Board,
}
