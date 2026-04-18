use axum::{extract::State, http::StatusCode, response::IntoResponse, Extension};
use tracing::error;

use btc_forum_shared::{ApiError, ErrorCode, HealthResponse, HealthSurrealStatus};

use crate::agent::request_id::RequestId;
use crate::agent::response::{err_response, ok_response};
use crate::api::state::AppState;

pub async fn health(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
) -> impl IntoResponse {
    let surreal_status = match state.surreal.health().await {
        Ok(_) => HealthSurrealStatus {
            status: "ok".to_string(),
            message: None,
        },
        Err(err) => {
            error!(error = %err, request_id = %request_id.0, "agent v1 surreal connectivity check failed");
            return err_response::<HealthResponse>(
                StatusCode::BAD_GATEWAY,
                &{
                    let mut extensions = axum::http::Extensions::new();
                    extensions.insert(request_id);
                    extensions
                },
                ApiError {
                    code: ErrorCode::BadGateway,
                    message: "surreal health check failed".to_string(),
                    details: Some(serde_json::json!({
                        "surreal": {
                            "status": "error",
                            "message": err.to_string(),
                        }
                    })),
                },
            );
        }
    };

    ok_response(
        StatusCode::OK,
        &{
            let mut extensions = axum::http::Extensions::new();
            extensions.insert(request_id);
            extensions
        },
        HealthResponse {
            service: "ok (surreal-only)".to_string(),
            surreal: surreal_status,
            timestamp: chrono::Utc::now().to_rfc3339(),
        },
    )
}
