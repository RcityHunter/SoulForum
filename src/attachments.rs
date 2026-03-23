use crate::services::{
    AttachmentRecord, AttachmentUpload, ForumContext, ForumError, ForumService, ServiceResult,
};
use serde_json::json;

#[derive(Clone, Debug, Default)]
pub struct AttachmentLimits {
    pub per_file_bytes: Option<i64>,
    pub per_post_bytes: Option<i64>,
    pub per_post_files: Option<usize>,
    pub dir_size_bytes: Option<i64>,
    pub dir_file_count: Option<i64>,
    pub allowed_extensions: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct AttachmentProcessResult {
    pub stored: Vec<AttachmentRecord>,
    pub total_size: i64,
    pub total_files: usize,
}

pub fn process_attachments<S: ForumService>(
    ctx: &mut ForumContext,
    service: &S,
    uploads: Vec<AttachmentUpload>,
    limits: AttachmentLimits,
) -> ServiceResult<AttachmentProcessResult> {
    if uploads.is_empty() {
        return Ok(AttachmentProcessResult::default());
    }

    let dir_id = service.current_attachment_dir()?;
    let (mut dir_size, mut dir_files) = service.attachment_dir_usage(dir_id)?;
    let mut total_size = ctx.context.int("attachments_total_size").unwrap_or(0);
    let mut total_files = ctx.context.int("attachments_quantity").unwrap_or(0) as usize;

    let mut stored = Vec::new();

    for upload in uploads {
        enforce_extension(&upload, &limits)?;
        enforce_per_file_limit(&upload, &limits)?;
        enforce_dir_limit(upload.size, dir_size, dir_files, &limits, ctx, dir_id)?;
        enforce_post_limits(upload.size, total_size, total_files, &limits)?;

        let record = service.store_attachment(upload)?;
        service.update_attachment_dir_usage(dir_id, record.size, 1)?;
        dir_size += record.size;
        dir_files += 1;
        total_size += record.size;
        total_files += 1;
        stored.push(record);
    }

    ctx.context.set("attachments_total_size", total_size);
    ctx.context.set("attachments_quantity", total_files as i64);
    ctx.context.set(
        "current_attachments",
        stored
            .iter()
            .map(|att| {
                json!({
                    "id": att.id,
                    "name": att.name,
                    "size": att.size,
                })
            })
            .collect::<Vec<_>>(),
    );

    Ok(AttachmentProcessResult {
        stored,
        total_size,
        total_files,
    })
}

fn enforce_extension(upload: &AttachmentUpload, limits: &AttachmentLimits) -> ServiceResult<()> {
    if limits.allowed_extensions.is_empty() {
        return Ok(());
    }

    if let Some(ext) = upload.name.split('.').next_back() {
        if limits
            .allowed_extensions
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(ext))
        {
            return Ok(());
        }
    }

    Err(ForumError::Validation("cant_upload_type".into()))
}

fn enforce_per_file_limit(
    upload: &AttachmentUpload,
    limits: &AttachmentLimits,
) -> ServiceResult<()> {
    if let Some(limit) = limits.per_file_bytes {
        if upload.size > limit {
            return Err(ForumError::Validation("file_too_big".into()));
        }
    }
    Ok(())
}

fn enforce_dir_limit(
    file_size: i64,
    dir_size: i64,
    dir_files: i64,
    limits: &AttachmentLimits,
    ctx: &ForumContext,
    dir_id: i64,
) -> ServiceResult<()> {
    if let Some(limit) = limits.dir_size_bytes {
        if dir_size + file_size > limit {
            return Err(ForumError::Validation(format!(
                "dir_{dir_id}_size_exceeded"
            )));
        }
    }

    if let Some(limit) = limits.dir_file_count {
        if dir_files + 1 > limit {
            return Err(ForumError::Validation(format!(
                "dir_{dir_id}_filecount_exceeded"
            )));
        }
    }

    if ctx.context.string("dir_creation_error").is_some() {
        return Err(ForumError::Validation("dir_creation_error".into()));
    }

    Ok(())
}

fn enforce_post_limits(
    file_size: i64,
    current_total: i64,
    current_files: usize,
    limits: &AttachmentLimits,
) -> ServiceResult<()> {
    if let Some(limit) = limits.per_post_bytes {
        if current_total + file_size > limit {
            return Err(ForumError::Validation("attach_max_total_file_size".into()));
        }
    }

    if let Some(limit) = limits.per_post_files {
        if current_files + 1 > limit {
            return Err(ForumError::Validation("attachments_limit_per_post".into()));
        }
    }

    Ok(())
}

pub fn assign_attachments<S: ForumService>(
    service: &S,
    attachment_ids: &[i64],
    message_id: i64,
) -> ServiceResult<()> {
    if attachment_ids.is_empty() {
        return Ok(());
    }
    for att_id in attachment_ids {
        service.link_attachment_to_message(*att_id, message_id)?;
    }
    Ok(())
}

pub fn create_attachment<S: ForumService>(
    service: &S,
    upload: AttachmentUpload,
) -> ServiceResult<AttachmentRecord> {
    service.store_attachment(upload)
}

pub fn remove_attachments<S: ForumService>(
    service: &S,
    attachment_ids: &[i64],
) -> ServiceResult<()> {
    for att in attachment_ids {
        service.delete_attachment(*att)?;
    }
    Ok(())
}

pub fn list_message_attachments<S: ForumService>(
    service: &S,
    message_id: i64,
) -> ServiceResult<Vec<AttachmentRecord>> {
    service.list_message_attachments(message_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{ForumContext, InMemoryService};

    #[test]
    fn process_and_assign() {
        let service = InMemoryService::default();
        let mut ctx = ForumContext::default();
        let upload = AttachmentUpload {
            name: "file.txt".into(),
            tmp_path: "/tmp/file".into(),
            size: 64,
            mime_type: "text/plain".into(),
            width: None,
            height: None,
        };
        let result = process_attachments(
            &mut ctx,
            &service,
            vec![upload],
            AttachmentLimits {
                allowed_extensions: vec!["txt".into()],
                per_post_bytes: Some(256),
                dir_size_bytes: Some(1024),
                ..AttachmentLimits::default()
            },
        )
        .unwrap();
        assign_attachments(&service, &[result.stored[0].id], 99).unwrap();
        let linked = list_message_attachments(&service, 99).unwrap();
        assert_eq!(linked.len(), 1);
        remove_attachments(&service, &[result.stored[0].id]).unwrap();
        assert!(list_message_attachments(&service, 99).unwrap().is_empty());
    }
}
