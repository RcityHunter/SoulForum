use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;

use btc_forum_rust::{
    auth::AuthClaims,
    points,
    surreal::{connect_from_env, reauth_from_env},
};
use btc_forum_shared::{
    CreatePointsEventPayload, ErrorCode, PointsBalanceResponse, PointsEventCreateResponse,
    PointsLeaderboardResponse,
};

use super::{
    auth::{ensure_user_ctx, require_auth},
    error::api_error,
    state::AppState,
};

fn is_auth_error(message: &str) -> bool {
    message.contains("401") || message.contains("Unauthorized") || message.contains("InvalidAuth")
}

#[derive(Debug, Deserialize)]
pub(crate) struct LeaderboardParams {
    pub(crate) metric: Option<String>,
    pub(crate) limit: Option<usize>,
}

pub(crate) async fn my_points(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c,
        Err(resp) => return resp.into_response(),
    };
    let (_user, ctx) = match ensure_user_ctx(&state, claims).await {
        Ok(value) => value,
        Err(resp) => return resp.into_response(),
    };
    let member_id = ctx.user_info.id;
    match load_balance_with_events(&state, member_id).await {
        Ok((balance, recent_events)) => (
            StatusCode::OK,
            Json(PointsBalanceResponse {
                status: "ok".into(),
                balance,
                recent_events,
            }),
        )
            .into_response(),
        Err(err) => {
            api_error(StatusCode::INTERNAL_SERVER_ERROR, ErrorCode::Internal, err).into_response()
        }
    }
}

pub(crate) async fn user_points(
    State(state): State<AppState>,
    Path(member_id): Path<i64>,
) -> impl IntoResponse {
    match load_balance_with_events(&state, member_id).await {
        Ok((balance, recent_events)) => (
            StatusCode::OK,
            Json(PointsBalanceResponse {
                status: "ok".into(),
                balance,
                recent_events,
            }),
        )
            .into_response(),
        Err(err) => {
            api_error(StatusCode::INTERNAL_SERVER_ERROR, ErrorCode::Internal, err).into_response()
        }
    }
}

pub(crate) async fn points_leaderboard(
    State(state): State<AppState>,
    Query(params): Query<LeaderboardParams>,
) -> impl IntoResponse {
    let metric = points::parse_metric(params.metric.as_deref());
    let limit = params.limit.unwrap_or(20).clamp(1, 100);
    let client = state.surreal.client();
    match points::leaderboard(&client, metric.clone(), limit).await {
        Ok(leaderboard) => (
            StatusCode::OK,
            Json(PointsLeaderboardResponse {
                status: "ok".into(),
                metric,
                leaderboard,
            }),
        )
            .into_response(),
        Err(err) => {
            let message = err.to_string();
            if is_auth_error(&message) {
                let _ = reauth_from_env(&client).await;
                match points::leaderboard(&client, metric.clone(), limit).await {
                    Ok(leaderboard) => (
                        StatusCode::OK,
                        Json(PointsLeaderboardResponse {
                            status: "ok".into(),
                            metric,
                            leaderboard,
                        }),
                    )
                        .into_response(),
                    Err(retry_err) if is_auth_error(&retry_err.to_string()) => {
                        match connect_from_env().await {
                            Ok(fresh) => match points::leaderboard(&fresh, metric.clone(), limit).await {
                                Ok(leaderboard) => (
                                    StatusCode::OK,
                                    Json(PointsLeaderboardResponse {
                                        status: "ok".into(),
                                        metric,
                                        leaderboard,
                                    }),
                                )
                                    .into_response(),
                                Err(fresh_err) => api_error(
                                    StatusCode::INTERNAL_SERVER_ERROR,
                                    ErrorCode::Internal,
                                    fresh_err.to_string(),
                                )
                                .into_response(),
                            },
                            Err(connect_err) => api_error(
                                StatusCode::INTERNAL_SERVER_ERROR,
                                ErrorCode::Internal,
                                connect_err.to_string(),
                            )
                            .into_response(),
                        }
                    }
                    Err(retry_err) => api_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        ErrorCode::Internal,
                        retry_err.to_string(),
                    )
                    .into_response(),
                }
            } else {
                api_error(StatusCode::INTERNAL_SERVER_ERROR, ErrorCode::Internal, message)
                    .into_response()
            }
        }
    }
}

pub(crate) async fn create_points_event_api(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    Json(payload): Json<CreatePointsEventPayload>,
) -> impl IntoResponse {
    let claims = match require_auth(&claims) {
        Ok(c) => c,
        Err(resp) => return resp.into_response(),
    };
    let (_user, ctx) = match ensure_user_ctx(&state, claims).await {
        Ok(value) => value,
        Err(resp) => return resp.into_response(),
    };
    if !(ctx.user_info.is_admin || ctx.user_info.permissions.contains("manage_boards")) {
        return api_error(
            StatusCode::FORBIDDEN,
            ErrorCode::Forbidden,
            "points event write requires admin-level permission",
        )
        .into_response();
    }
    let client = state.surreal.client();
    match points::create_points_event(&client, payload.clone()).await {
        Ok((event, balance)) => (
            StatusCode::CREATED,
            Json(PointsEventCreateResponse {
                status: "ok".into(),
                event,
                balance,
            }),
        )
            .into_response(),
        Err(err) if is_auth_error(&err.to_string()) => {
            let _ = reauth_from_env(&client).await;
            match points::create_points_event(&client, payload.clone()).await {
                Ok((event, balance)) => (
                    StatusCode::CREATED,
                    Json(PointsEventCreateResponse {
                        status: "ok".into(),
                        event,
                        balance,
                    }),
                )
                    .into_response(),
                Err(btc_forum_rust::services::ForumError::Validation(message)) => {
                    api_error(StatusCode::BAD_REQUEST, ErrorCode::Validation, message)
                        .into_response()
                }
                Err(err) if is_auth_error(&err.to_string()) => match connect_from_env().await {
                    Ok(fresh) => match points::create_points_event(&fresh, payload).await {
                        Ok((event, balance)) => (
                            StatusCode::CREATED,
                            Json(PointsEventCreateResponse {
                                status: "ok".into(),
                                event,
                                balance,
                            }),
                        )
                            .into_response(),
                        Err(btc_forum_rust::services::ForumError::Validation(message)) => {
                            api_error(StatusCode::BAD_REQUEST, ErrorCode::Validation, message)
                                .into_response()
                        }
                        Err(fresh_err) => api_error(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            ErrorCode::Internal,
                            fresh_err.to_string(),
                        )
                        .into_response(),
                    },
                    Err(connect_err) => api_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        ErrorCode::Internal,
                        connect_err.to_string(),
                    )
                    .into_response(),
                },
                Err(err) => api_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    ErrorCode::Internal,
                    err.to_string(),
                )
                .into_response(),
            }
        }
        Err(btc_forum_rust::services::ForumError::Validation(message)) => {
            api_error(StatusCode::BAD_REQUEST, ErrorCode::Validation, message).into_response()
        }
        Err(err) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            ErrorCode::Internal,
            err.to_string(),
        )
        .into_response(),
    }
}

async fn load_balance_with_events(
    state: &AppState,
    member_id: i64,
) -> Result<
    (
        btc_forum_shared::PointsBalance,
        Vec<btc_forum_shared::PointsEvent>,
    ),
    String,
> {
    let client = state.surreal.client();
    let balance = match points::get_points_balance(&client, member_id).await {
        Ok(balance) => balance,
        Err(err) => {
            let message = err.to_string();
            if is_auth_error(&message) {
                let _ = reauth_from_env(&client).await;
                match points::get_points_balance(&client, member_id).await {
                    Ok(balance) => balance,
                    Err(retry_err) if is_auth_error(&retry_err.to_string()) => {
                        let fresh = connect_from_env().await.map_err(|e| e.to_string())?;
                        points::get_points_balance(&fresh, member_id)
                            .await
                            .map_err(|e| e.to_string())?
                    }
                    Err(retry_err) => return Err(retry_err.to_string()),
                }
            } else {
                return Err(message);
            }
        }
    };
    let recent_events = match points::list_recent_points_events(&client, member_id, 20).await {
        Ok(events) => events,
        Err(err) => {
            let message = err.to_string();
            if is_auth_error(&message) {
                let _ = reauth_from_env(&client).await;
                match points::list_recent_points_events(&client, member_id, 20).await {
                    Ok(events) => events,
                    Err(retry_err) if is_auth_error(&retry_err.to_string()) => {
                        let fresh = connect_from_env().await.map_err(|e| e.to_string())?;
                        points::list_recent_points_events(&fresh, member_id, 20)
                            .await
                            .map_err(|e| e.to_string())?
                    }
                    Err(retry_err) => return Err(retry_err.to_string()),
                }
            } else {
                return Err(message);
            }
        }
    };
    Ok((balance, recent_events))
}
