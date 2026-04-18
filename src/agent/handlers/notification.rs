use axum::{
    extract::State,
    http::{Extensions, StatusCode},
    response::IntoResponse,
    Extension, Json,
};
use serde::Serialize;
use tracing::error;

use btc_forum_rust::{auth::AuthClaims, surreal::SurrealNotification};
use btc_forum_shared::{ApiError, ErrorCode, Notification};

use crate::agent::{
    auth::require_scope,
    request_id::RequestId,
    response::{err_response, ok_response},
};
use crate::api::{auth::ensure_user_ctx, state::AppState};

const NOTIFICATION_READ_SCOPE: &str = "forum:notification:read";
const NOTIFICATION_READ_LEGACY_PERMISSIONS: &[&str] =
    &["manage_boards", "post_new", "post_reply_any"];

#[derive(Debug, Serialize)]
pub struct NotificationListData {
    pub notifications: Vec<Notification>,
}

fn request_extensions(request_id: &RequestId) -> Extensions {
    let mut extensions = Extensions::new();
    extensions.insert(request_id.clone());
    extensions
}

fn to_notification(note: SurrealNotification) -> Notification {
    Notification {
        id: note.id.unwrap_or_default(),
        user: note.user,
        subject: note.subject,
        body: note.body,
        created_at: note.created_at,
        is_read: note.is_read,
    }
}

pub async fn list(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    Extension(request_id): Extension<RequestId>,
) -> impl IntoResponse {
    let request_extensions = request_extensions(&request_id);

    let claims = match require_scope(
        &claims,
        NOTIFICATION_READ_SCOPE,
        NOTIFICATION_READ_LEGACY_PERMISSIONS,
    ) {
        Ok(claims) => claims,
        Err((status, Json(error))) => {
            return err_response::<NotificationListData>(status, &request_extensions, error)
        }
    };

    let (user, _ctx) = match ensure_user_ctx(&state, claims).await {
        Ok(value) => value,
        Err((status, Json(error))) => {
            return err_response::<NotificationListData>(status, &request_extensions, error)
        }
    };

    match state.surreal.list_notifications(&user.name).await {
        Ok(items) => ok_response(
            StatusCode::OK,
            &request_extensions,
            NotificationListData {
                notifications: items.into_iter().map(to_notification).collect(),
            },
        ),
        Err(err) => {
            error!(
                error = %err,
                request_id = %request_id.0,
                user = %claims.sub,
                "agent v1 notification list failed"
            );
            err_response::<NotificationListData>(
                StatusCode::INTERNAL_SERVER_ERROR,
                &request_extensions,
                ApiError {
                    code: ErrorCode::Internal,
                    message: "failed to list notifications".to_string(),
                    details: Some(serde_json::json!({
                        "user": claims.sub,
                    })),
                },
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::to_notification;
    use btc_forum_rust::surreal::SurrealNotification;

    #[test]
    fn maps_surreal_notification_into_shared_notification() {
        let notification = to_notification(SurrealNotification {
            id: Some("notifications:1".into()),
            user: "alice@example.com".into(),
            subject: "Subject".into(),
            body: "Body".into(),
            is_read: Some(false),
            created_at: Some("2026-03-19T00:00:00Z".into()),
        });

        assert_eq!(notification.id, "notifications:1");
        assert_eq!(notification.user, "alice@example.com");
        assert_eq!(notification.subject, "Subject");
        assert_eq!(notification.body, "Body");
        assert_eq!(notification.is_read, Some(false));
    }
}
