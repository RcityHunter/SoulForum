use btc_forum_shared::{ApiError, ErrorCode};

pub fn format_api_error(status: u16, err: ApiError) -> String {
    let mut base = match err.code {
        ErrorCode::Unauthorized => {
            if err.message.trim().is_empty() || err.message.contains("authorization") {
                "未登录或登录失效，请先登录".to_string()
            } else {
                err.message.clone()
            }
        }
        ErrorCode::Forbidden => {
            if err.message.contains("csrf") {
                "CSRF 校验失败，请点击“同步 CSRF”后重试".to_string()
            } else if err.message.contains("banned") {
                "账号已被封禁，无法执行此操作".to_string()
            } else {
                "权限不足，无法执行此操作".to_string()
            }
        }
        ErrorCode::Validation => {
            if err.message.trim().is_empty() {
                "参数错误，请检查输入".to_string()
            } else {
                format!("参数错误：{}", err.message)
            }
        }
        ErrorCode::NotFound => "资源不存在".to_string(),
        ErrorCode::Conflict => {
            if err.message.trim().is_empty() {
                "操作冲突，请刷新后重试".to_string()
            } else {
                format!("操作冲突：{}", err.message)
            }
        }
        ErrorCode::RateLimited => "操作过于频繁，请稍后再试".to_string(),
        ErrorCode::BadGateway => {
            if err.message.trim().is_empty() {
                "上游服务不可用，请检查 Rainbow-Auth/SurrealDB".to_string()
            } else {
                format!("上游服务错误：{}", err.message)
            }
        }
        ErrorCode::Internal => {
            if err.message.trim().is_empty() {
                "服务内部错误，请稍后再试".to_string()
            } else {
                format!("服务内部错误：{}", err.message)
            }
        }
    };
    if let Some(details) = err.details {
        let details_str = details.to_string();
        if !details_str.trim().is_empty() && details_str != "null" {
            base = format!("{}（详情：{}）", base, details_str);
        }
    }
    format!("{base}（HTTP {status}）")
}
