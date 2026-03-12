use axum::{http::StatusCode, Json};
use btc_forum_shared::{ApiError, ErrorCode};

use btc_forum_rust::rainbow_auth::RainbowAuthError;

pub(crate) fn api_error(
    status: StatusCode,
    code: ErrorCode,
    message: impl Into<String>,
) -> (StatusCode, Json<ApiError>) {
    (
        status,
        Json(ApiError {
            code,
            message: message.into(),
            details: None,
        }),
    )
}

pub(crate) fn api_error_from_status(
    status: StatusCode,
    message: impl Into<String>,
) -> (StatusCode, Json<ApiError>) {
    let code = match status {
        StatusCode::UNAUTHORIZED => ErrorCode::Unauthorized,
        StatusCode::FORBIDDEN => ErrorCode::Forbidden,
        StatusCode::NOT_FOUND => ErrorCode::NotFound,
        StatusCode::CONFLICT => ErrorCode::Conflict,
        StatusCode::TOO_MANY_REQUESTS => ErrorCode::RateLimited,
        StatusCode::BAD_GATEWAY | StatusCode::GATEWAY_TIMEOUT | StatusCode::SERVICE_UNAVAILABLE => {
            ErrorCode::BadGateway
        }
        s if s.is_server_error() => ErrorCode::Internal,
        _ => ErrorCode::Validation,
    };
    api_error(status, code, message)
}

pub(crate) fn rainbow_auth_error_response(err: RainbowAuthError) -> (StatusCode, Json<ApiError>) {
    match err {
        RainbowAuthError::Http { status, message } => {
            // Upstream auth service 5xx should be surfaced as gateway failure
            // instead of pretending our API itself failed internally.
            let response_status = if status.is_server_error() {
                StatusCode::BAD_GATEWAY
            } else {
                status
            };
            let code = match status {
                StatusCode::UNAUTHORIZED => ErrorCode::Unauthorized,
                StatusCode::FORBIDDEN => ErrorCode::Forbidden,
                StatusCode::NOT_FOUND => ErrorCode::NotFound,
                StatusCode::CONFLICT => ErrorCode::Conflict,
                StatusCode::BAD_REQUEST => ErrorCode::Validation,
                _ if status.is_server_error() => ErrorCode::BadGateway,
                _ => ErrorCode::Validation,
            };
            api_error(response_status, code, message)
        }
        RainbowAuthError::Transport(message) | RainbowAuthError::Parse(message) => {
            api_error(StatusCode::BAD_GATEWAY, ErrorCode::BadGateway, message)
        }
    }
}
