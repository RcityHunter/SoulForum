use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct LoginRequest {
    #[serde(alias = "username")]
    pub email: String,
    pub password: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct RegisterRequest {
    pub email: String,
    pub password: String,
    pub username: Option<String>,
    pub role: Option<String>,
    pub permissions: Option<Vec<String>>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct AuthUser {
    pub name: String,
    pub role: Option<String>,
    pub permissions: Option<Vec<String>>,
    pub member_id: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct AuthResponse {
    pub status: String,
    pub token: String,
    pub user: AuthUser,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct RegisterResponse {
    pub status: String,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct AuthMeResponse {
    pub status: String,
    pub user: AuthUser,
}
