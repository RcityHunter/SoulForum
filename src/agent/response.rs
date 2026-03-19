use axum::{
    http::{Extensions, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::Serialize;

use btc_forum_shared::ApiError;

use super::request_id::{generate_request_id, RequestId};

#[derive(Debug, Serialize)]
pub struct AgentEnvelope<T>
where
    T: Serialize,
{
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ApiError>,
    pub request_id: String,
}

impl<T> AgentEnvelope<T>
where
    T: Serialize,
{
    pub fn ok(data: T, request_id: String) -> Self {
        Self {
            ok: true,
            data: Some(data),
            error: None,
            request_id,
        }
    }

    pub fn err(error: ApiError, request_id: String) -> Self {
        Self {
            ok: false,
            data: None,
            error: Some(error),
            request_id,
        }
    }
}

pub fn request_id_from_extensions(extensions: &Extensions) -> String {
    extensions
        .get::<RequestId>()
        .map(|request_id| request_id.0.clone())
        .unwrap_or_else(generate_request_id)
}

pub fn ok_response<T>(status: StatusCode, extensions: &Extensions, data: T) -> impl IntoResponse
where
    T: Serialize,
{
    let request_id = request_id_from_extensions(extensions);
    (status, Json(AgentEnvelope::ok(data, request_id)))
}

pub fn err_response<T>(
    status: StatusCode,
    extensions: &Extensions,
    error: ApiError,
) -> impl IntoResponse
where
    T: Serialize,
{
    let request_id = request_id_from_extensions(extensions);
    (status, Json(AgentEnvelope::<T>::err(error, request_id)))
}
