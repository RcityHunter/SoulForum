use axum::{
    extract::State,
    http::{Extensions, StatusCode},
    response::IntoResponse,
    Extension, Json,
};
use serde::Serialize;
use tracing::error;

use btc_forum_rust::{auth::AuthClaims, services::BoardAccessEntry, surreal::SurrealBoard};
use btc_forum_shared::{ApiError, Board, ErrorCode};

use crate::agent::{
    auth::require_scope,
    request_id::RequestId,
    response::{err_response, ok_response},
};
use crate::api::{
    auth::{ensure_user_ctx, user_groups},
    guards::load_board_access,
    state::AppState,
};

const BOARD_READ_SCOPE: &str = "forum:board:read";
const BOARD_READ_LEGACY_PERMISSIONS: &[&str] = &["manage_boards", "post_new", "post_reply_any"];

#[derive(Debug, Serialize)]
pub struct BoardListData {
    pub boards: Vec<Board>,
}

fn request_extensions(request_id: &RequestId) -> Extensions {
    let mut extensions = Extensions::new();
    extensions.insert(request_id.clone());
    extensions
}

fn to_board(board: SurrealBoard) -> Board {
    Board {
        id: board.id,
        name: board.name,
        description: board.description,
        created_at: board.created_at,
        updated_at: None,
    }
}

fn board_visible(
    board: &SurrealBoard,
    access_rules: &[BoardAccessEntry],
    group_ids: &[i64],
    is_admin: bool,
) -> bool {
    if is_admin {
        return true;
    }

    let Some(board_id) = board.id.as_deref() else {
        return true;
    };

    let Some(rule) = access_rules.iter().find(|rule| rule.id == board_id) else {
        return true;
    };

    rule.allowed_groups.is_empty()
        || rule
            .allowed_groups
            .iter()
            .any(|gid| group_ids.iter().any(|group_id| group_id == gid))
}

pub async fn list(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    Extension(request_id): Extension<RequestId>,
) -> impl IntoResponse {
    let request_extensions = request_extensions(&request_id);

    let claims = match require_scope(&claims, BOARD_READ_SCOPE, BOARD_READ_LEGACY_PERMISSIONS) {
        Ok(claims) => claims,
        Err((status, Json(error))) => {
            return err_response::<BoardListData>(status, &request_extensions, error)
        }
    };

    let (_user, ctx) = match ensure_user_ctx(&state, claims).await {
        Ok(value) => value,
        Err((status, Json(error))) => {
            return err_response::<BoardListData>(status, &request_extensions, error)
        }
    };

    let access_rules = match load_board_access(&state).await {
        Ok(rules) => rules,
        Err(err) => {
            error!(
                error = %err,
                request_id = %request_id.0,
                "agent v1 board access load failed"
            );
            return err_response::<BoardListData>(
                StatusCode::INTERNAL_SERVER_ERROR,
                &request_extensions,
                ApiError {
                    code: ErrorCode::Internal,
                    message: "failed to load board access".to_string(),
                    details: None,
                },
            );
        }
    };

    let group_ids = user_groups(&ctx);
    let boards = match state.surreal.list_boards().await {
        Ok(boards) => boards,
        Err(err) => {
            error!(
                error = %err,
                request_id = %request_id.0,
                "agent v1 board list failed"
            );
            return err_response::<BoardListData>(
                StatusCode::INTERNAL_SERVER_ERROR,
                &request_extensions,
                ApiError {
                    code: ErrorCode::Internal,
                    message: "failed to list boards".to_string(),
                    details: None,
                },
            );
        }
    };

    let boards = boards
        .into_iter()
        .filter(|board| board_visible(board, &access_rules, &group_ids, ctx.user_info.is_admin))
        .map(to_board)
        .collect();

    ok_response(
        StatusCode::OK,
        &request_extensions,
        BoardListData { boards },
    )
}

#[cfg(test)]
mod tests {
    use super::board_visible;
    use btc_forum_rust::{services::BoardAccessEntry, surreal::SurrealBoard};

    fn board(id: Option<&str>) -> SurrealBoard {
        SurrealBoard {
            id: id.map(str::to_string),
            name: "General".into(),
            description: None,
            created_at: None,
        }
    }

    #[test]
    fn board_visible_allows_board_without_rule() {
        assert!(board_visible(
            &board(Some("boards:general")),
            &[],
            &[4],
            false
        ));
    }

    #[test]
    fn board_visible_respects_allowed_groups() {
        let rules = vec![BoardAccessEntry {
            id: "boards:general".into(),
            name: "General".into(),
            allowed_groups: vec![2],
        }];

        assert!(board_visible(
            &board(Some("boards:general")),
            &rules,
            &[2],
            false
        ));
        assert!(!board_visible(
            &board(Some("boards:general")),
            &rules,
            &[4],
            false
        ));
    }
}
