use crate::services::{ForumContext, ForumService, ServiceResult};
use serde_json::json;

pub struct BoardAccessController<S: ForumService> {
    service: S,
}

impl<S: ForumService> BoardAccessController<S> {
    pub fn new(service: S) -> Self {
        Self { service }
    }

    pub fn list_for_group(&self, ctx: &mut ForumContext, group_id: i64) -> ServiceResult<()> {
        let boards = self.service.list_board_access()?;
        let entries: Vec<_> = boards
            .into_iter()
            .map(|board| {
                let allowed = board.allowed_groups.iter().any(|gid| gid == &group_id);
                json!({
                    "id": board.id,
                    "name": board.name,
                    "allowed": allowed,
                })
            })
            .collect();
        ctx.context.set("board_access", entries);
        Ok(())
    }

    pub fn save_for_group(&self, group_id: i64, allowed_boards: &[String]) -> ServiceResult<()> {
        let all = self.service.list_board_access()?;
        for board in all {
            let mut groups = board.allowed_groups;
            if allowed_boards.contains(&board.id) {
                if !groups.contains(&group_id) {
                    groups.push(group_id);
                }
            } else {
                groups.retain(|gid| gid != &group_id);
            }
            self.service.set_board_access(&board.id, &groups)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{ForumContext, InMemoryService};

    #[test]
    fn board_access_roundtrip() {
        let service = InMemoryService::default();
        let controller = BoardAccessController::new(service.clone());
        let mut ctx = ForumContext::default();
        controller.list_for_group(&mut ctx, 1).unwrap();
        controller.save_for_group(1, &["1".into()]).unwrap();
        let boards = service.list_board_access().unwrap();
        assert!(boards
            .iter()
            .find(|board| board.id == "1")
            .unwrap()
            .allowed_groups
            .contains(&1));
    }
}
