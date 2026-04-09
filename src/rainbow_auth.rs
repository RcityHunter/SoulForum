use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct RainbowAuthClient {
    base_url: String,
    http: Client,
}

impl RainbowAuthClient {
    pub fn new(base_url: String) -> Self {
        let http = Client::builder()
            .user_agent("btc-forum-rust/0.1")
            .build()
            .expect("reqwest client build failed");
        Self { base_url, http }
    }

    pub async fn login(
        &self,
        email: &str,
        password: &str,
    ) -> Result<RainbowLoginResponse, RainbowAuthError> {
        let url = format!("{}/api/auth/login", self.base_url.trim_end_matches('/'));
        let resp = self
            .http
            .post(url)
            .json(&RainbowLoginRequest { email, password })
            .send()
            .await
            .map_err(RainbowAuthError::from)?;
        parse_json_response(resp).await
    }

    pub async fn register(
        &self,
        email: &str,
        password: &str,
        username: &str,
    ) -> Result<String, RainbowAuthError> {
        let url = format!("{}/api/auth/register", self.base_url.trim_end_matches('/'));
        let resp = self
            .http
            .post(url)
            .json(&RainbowRegisterRequest {
                email,
                password,
                username,
            })
            .send()
            .await
            .map_err(RainbowAuthError::from)?;
        let status = resp.status();
        let body = resp.text().await.map_err(RainbowAuthError::from)?;
        if !status.is_success() {
            return Err(RainbowAuthError::Http {
                status,
                message: body,
            });
        }
        Ok(body)
    }

    pub async fn me(&self, token: &str) -> Result<RainbowUser, RainbowAuthError> {
        let url = format!("{}/api/auth/me", self.base_url.trim_end_matches('/'));
        let resp = self
            .http
            .get(url)
            .bearer_auth(token)
            .send()
            .await
            .map_err(RainbowAuthError::from)?;
        parse_json_response(resp).await
    }

    pub async fn get_user_permissions(
        &self,
        admin_token: &str,
        user_id: &str,
    ) -> Result<Vec<String>, RainbowAuthError> {
        let url = format!(
            "{}/api/rbac/users/{}/permissions",
            self.base_url.trim_end_matches('/'),
            user_id
        );
        let resp = self
            .http
            .get(url)
            .bearer_auth(admin_token)
            .send()
            .await
            .map_err(RainbowAuthError::from)?;
        let status = resp.status();
        let text = resp.text().await.map_err(RainbowAuthError::from)?;
        if !status.is_success() {
            return Err(RainbowAuthError::Http {
                status,
                message: text,
            });
        }
        let parsed: RainbowApiResponse<Vec<String>> = serde_json::from_str(&text)?;
        if parsed.success {
            Ok(parsed.data.unwrap_or_default())
        } else {
            Ok(Vec::new())
        }
    }

    pub async fn assign_role_to_user(
        &self,
        admin_token: &str,
        user_id: &str,
        role_name: &str,
    ) -> Result<(), RainbowAuthError> {
        let url = format!(
            "{}/api/rbac/users/{}/roles/assign",
            self.base_url.trim_end_matches('/'),
            user_id
        );
        let resp = self
            .http
            .post(url)
            .bearer_auth(admin_token)
            .json(&RainbowAssignRoleRequest { role_name })
            .send()
            .await
            .map_err(RainbowAuthError::from)?;
        let status = resp.status();
        let body = resp.text().await.map_err(RainbowAuthError::from)?;
        if !status.is_success() {
            return Err(RainbowAuthError::Http {
                status,
                message: body,
            });
        }
        Ok(())
    }

    pub async fn remove_role_from_user(
        &self,
        admin_token: &str,
        user_id: &str,
        role_name: &str,
    ) -> Result<(), RainbowAuthError> {
        let url = format!(
            "{}/api/rbac/users/{}/roles/remove",
            self.base_url.trim_end_matches('/'),
            user_id
        );
        let resp = self
            .http
            .post(url)
            .bearer_auth(admin_token)
            .json(&RainbowAssignRoleRequest { role_name })
            .send()
            .await
            .map_err(RainbowAuthError::from)?;
        let status = resp.status();
        let body = resp.text().await.map_err(RainbowAuthError::from)?;
        if !status.is_success() {
            return Err(RainbowAuthError::Http {
                status,
                message: body,
            });
        }
        Ok(())
    }
}

#[derive(Debug)]
pub enum RainbowAuthError {
    Http { status: StatusCode, message: String },
    Transport(String),
    Parse(String),
}

impl From<reqwest::Error> for RainbowAuthError {
    fn from(err: reqwest::Error) -> Self {
        Self::Transport(err.to_string())
    }
}

impl From<serde_json::Error> for RainbowAuthError {
    fn from(err: serde_json::Error) -> Self {
        Self::Parse(err.to_string())
    }
}

impl std::fmt::Display for RainbowAuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RainbowAuthError::Http { status, message } => {
                write!(f, "rainbow-auth http {}: {}", status.as_u16(), message)
            }
            RainbowAuthError::Transport(msg) => write!(f, "rainbow-auth transport: {}", msg),
            RainbowAuthError::Parse(msg) => write!(f, "rainbow-auth parse: {}", msg),
        }
    }
}

impl RainbowAuthError {
    pub fn is_retryable(&self) -> bool {
        match self {
            RainbowAuthError::Http { status, .. } => status.is_server_error(),
            RainbowAuthError::Transport(_) => true,
            RainbowAuthError::Parse(_) => false,
        }
    }
}

#[derive(Serialize)]
struct RainbowLoginRequest<'a> {
    email: &'a str,
    password: &'a str,
}

#[derive(Serialize)]
struct RainbowRegisterRequest<'a> {
    email: &'a str,
    password: &'a str,
    username: &'a str,
}

#[derive(Serialize)]
struct RainbowAssignRoleRequest<'a> {
    role_name: &'a str,
}

#[derive(Debug, Deserialize)]
struct RainbowApiResponse<T> {
    success: bool,
    #[allow(dead_code)]
    message: Option<String>,
    data: Option<T>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RainbowLoginResponse {
    pub token: String,
    pub user: RainbowUser,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RainbowUser {
    pub id: String,
    pub email: String,
    #[serde(rename = "verified")]
    pub is_email_verified: bool,
}

async fn parse_json_response<T: for<'de> Deserialize<'de>>(
    resp: reqwest::Response,
) -> Result<T, RainbowAuthError> {
    let status = resp.status();
    let text = resp.text().await.map_err(RainbowAuthError::from)?;
    if !status.is_success() {
        return Err(RainbowAuthError::Http {
            status,
            message: text,
        });
    }
    let parsed = serde_json::from_str(&text)?;
    Ok(parsed)
}
