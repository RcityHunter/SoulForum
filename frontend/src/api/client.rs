use crate::api::errors::format_api_error;
use btc_forum_shared::ApiError;
use reqwasm::http::{Request, RequestCredentials};
use serde::de::DeserializeOwned;
use serde::Serialize;
use web_sys::wasm_bindgen::JsCast;
use web_sys::HtmlDocument;

#[derive(Clone, Debug, Default)]
pub struct ApiClient {
    pub base_url: String,
    pub token: Option<String>,
    /// Optional CSRF value to send via `X-CSRF-TOKEN`.
    /// If `None`, we will try to read from cookie `XSRF-TOKEN`.
    pub csrf: Option<String>,
    /// When set, uses `credentials: include`.
    pub include_credentials: bool,
}

impl ApiClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            token: None,
            csrf: None,
            include_credentials: true,
        }
    }

    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        let token = token.into();
        self.token = if token.trim().is_empty() {
            None
        } else {
            Some(token)
        };
        self
    }

    pub fn with_optional_token(mut self, token: Option<String>) -> Self {
        self.token = token.and_then(|t| if t.trim().is_empty() { None } else { Some(t) });
        self
    }

    pub fn with_csrf(mut self, csrf: impl Into<String>) -> Self {
        let csrf = csrf.into();
        self.csrf = if csrf.trim().is_empty() {
            None
        } else {
            Some(csrf)
        };
        self
    }

    pub fn with_optional_csrf(mut self, csrf: Option<String>) -> Self {
        self.csrf = csrf.and_then(|c| if c.trim().is_empty() { None } else { Some(c) });
        self
    }

    pub fn include_credentials(mut self, include: bool) -> Self {
        self.include_credentials = include;
        self
    }

    pub fn resolve_url(&self, path: &str) -> String {
        format!(
            "{}/{}",
            self.base_url.trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }

    pub fn read_csrf_cookie() -> Option<String> {
        let win = web_sys::window()?;
        let doc = win.document()?;
        let html: HtmlDocument = doc.dyn_into().ok()?;
        let cookie = html.cookie().ok()?;
        cookie
            .split(';')
            .find_map(|part| part.trim().strip_prefix("XSRF-TOKEN="))
            .map(|v| v.to_string())
    }

    fn csrf_value(&self) -> Option<String> {
        if let Some(csrf) = self.csrf.clone() {
            return Some(csrf);
        }
        Self::read_csrf_cookie()
    }

    fn apply_common_headers(&self, mut req: Request) -> Request {
        if let Some(token) = self.token.as_ref() {
            req = req.header("Authorization", &format!("Bearer {token}"));
        }
        if let Some(csrf) = self.csrf_value() {
            req = req.header("X-CSRF-TOKEN", &csrf);
        }
        if self.include_credentials {
            req = req.credentials(RequestCredentials::Include);
        }
        req
    }

    pub async fn get_json<T: DeserializeOwned>(&self, path: &str) -> Result<T, String> {
        let url = self.resolve_url(path);
        let req = Request::get(&url);
        let req = self.apply_common_headers(req);
        let resp = req.send().await.map_err(|e| format!("网络错误: {e}"))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| format!("读取响应失败: {e}"))?;
        if !resp.ok() {
            if let Ok(err) = serde_json::from_str::<ApiError>(&text) {
                return Err(format_api_error(status, err));
            }
            return Err(format!("HTTP {status}: {text}"));
        }
        serde_json::from_str(&text).map_err(|e| format!("解析失败: {e}，原始响应: {text}"))
    }

    pub async fn post_json<T: DeserializeOwned, B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, String> {
        let url = self.resolve_url(path);
        let req = Request::post(&url)
            .header("Content-Type", "application/json")
            .body(serde_json::to_string(body).map_err(|e| format!("序列化请求失败: {e}"))?);
        let req = self.apply_common_headers(req);

        let resp = req.send().await.map_err(|e| format!("网络错误: {e}"))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| format!("读取响应失败: {e}"))?;
        if !resp.ok() {
            if let Ok(err) = serde_json::from_str::<ApiError>(&text) {
                return Err(format_api_error(status, err));
            }
            return Err(format!("HTTP {status}: {text}"));
        }
        serde_json::from_str(&text).map_err(|e| format!("解析失败: {e}，原始响应: {text}"))
    }
}
