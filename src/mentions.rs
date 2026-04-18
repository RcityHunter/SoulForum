use crate::services::{ForumContext, ForumService, MemberRecord, MentionRecord, ServiceResult};
use lazy_static::lazy_static;
use regex::Regex;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug)]
pub struct MentionedMember {
    pub record: MemberRecord,
    pub mentioned_by: Option<MemberRecord>,
}

pub fn get_mentions_by_content<S: ForumService>(
    service: &S,
    content_type: &str,
    content_id: i64,
    restrict_to: Option<&[i64]>,
) -> ServiceResult<HashMap<i64, MentionedMember>> {
    let mut results = HashMap::new();
    let mentions = service.list_mentions(content_type, content_id)?;
    for entry in mentions {
        if let Some(filter) = restrict_to {
            if !filter.contains(&entry.mentioned_id) {
                continue;
            }
        }
        if let Some(member) = service.get_member_record(entry.mentioned_id)? {
            let mentioned_by = service.get_member_record(entry.author_id)?;
            results.insert(
                member.id,
                MentionedMember {
                    record: member,
                    mentioned_by,
                },
            );
        }
    }
    Ok(results)
}

pub fn insert_mentions<S: ForumService>(
    service: &S,
    content_type: &str,
    content_id: i64,
    members: &[MemberRecord],
    author_id: i64,
) -> ServiceResult<()> {
    let now = chrono::Utc::now();
    let records: Vec<MentionRecord> = members
        .iter()
        .map(|member| MentionRecord {
            id: 0,
            content_type: content_type.to_string(),
            content_id,
            author_id,
            mentioned_id: member.id,
            time: now,
        })
        .collect();
    service.insert_mentions(&records)
}

pub fn modify_mentions<S: ForumService>(
    service: &S,
    content_type: &str,
    content_id: i64,
    members: HashMap<i64, MemberRecord>,
    author_id: i64,
) -> ServiceResult<MentionDelta> {
    let existing = get_mentions_by_content(service, content_type, content_id, None)?;
    let existing_keys: HashSet<i64> = existing.keys().copied().collect();
    let new_keys: HashSet<i64> = members.keys().copied().collect();

    let removed: Vec<_> = existing_keys.difference(&new_keys).copied().collect();
    if !removed.is_empty() {
        service.delete_mentions(content_type, content_id, &removed)?;
    }

    let added: Vec<_> = new_keys.difference(&existing_keys).copied().collect();
    if !added.is_empty() {
        let targets: Vec<MemberRecord> = added
            .iter()
            .filter_map(|id| members.get(id).cloned())
            .collect();
        insert_mentions(service, content_type, content_id, &targets, author_id)?;
    }

    let unchanged: Vec<_> = existing_keys.intersection(&new_keys).copied().collect();

    Ok(MentionDelta {
        unchanged,
        removed,
        added,
    })
}

#[derive(Clone, Debug, Default)]
pub struct MentionDelta {
    pub unchanged: Vec<i64>,
    pub removed: Vec<i64>,
    pub added: Vec<i64>,
}

pub fn get_body(body: &str, members: &[MemberRecord]) -> String {
    let mut result = body.to_string();
    for member in members {
        let token = format!("@{}", member.name);
        let replacement = format!("[member={}]{}[/member]", member.id, member.name);
        result = result.replace(&token, &replacement);
    }
    result
}

pub fn get_mentioned_members<S: ForumService>(
    service: &S,
    ctx: &ForumContext,
    body: &str,
) -> ServiceResult<HashMap<i64, MemberRecord>> {
    if body.is_empty() || !ctx.user_info.permissions.contains("mention") {
        return Ok(HashMap::new());
    }
    let possible_names = get_possible_mentions(body);
    let existing_mentions = get_existing_mentions(body);
    if possible_names.is_empty() && existing_mentions.is_empty() {
        return Ok(HashMap::new());
    }
    let mut lookups = possible_names.clone();
    lookups.extend(existing_mentions.values().cloned());
    let members = service.find_members_by_name(&lookups)?;
    let mut matches = HashMap::new();
    for member in members {
        if existing_mentions.contains_key(&member.id)
            || body
                .to_lowercase()
                .contains(&format!("@{}", member.name.to_lowercase()))
        {
            matches.insert(member.id, member);
        }
    }
    Ok(matches)
}

pub fn get_existing_mentions(body: &str) -> HashMap<i64, String> {
    let mut existing = HashMap::new();
    if body.is_empty() {
        return existing;
    }
    lazy_static! {
        static ref MEMBER_TAG: Regex = Regex::new(r"\[member=(\d+)\]([^\[]+)\[/member\]").unwrap();
    }
    for caps in MEMBER_TAG.captures_iter(body) {
        if let Ok(id) = caps[1].parse::<i64>() {
            existing.insert(id, caps[2].to_string());
        }
    }
    existing
}

fn get_possible_mentions(body: &str) -> Vec<String> {
    if body.is_empty() {
        return Vec::new();
    }
    let cleaned = body.replace("&nbsp;", " ").replace("<br>", "\n");
    let mut names = Vec::new();
    for token in cleaned.split_whitespace() {
        if token.starts_with('@') {
            let trimmed = token
                .trim_matches(|c: char| !(c.is_alphanumeric() || c == '_' || c == '.' || c == '-'));
            if trimmed.len() > 1 {
                names.push(trimmed[1..].to_string());
            }
        }
    }
    names
}

pub fn verify_mentioned_members(
    body: &str,
    members: &HashMap<i64, MemberRecord>,
) -> HashMap<i64, MemberRecord> {
    let mut valid = HashMap::new();
    for (id, member) in members {
        let pattern = format!("[member={}]{}[/member]", id, member.name);
        if body.contains(&pattern) {
            valid.insert(*id, member.clone());
        }
    }
    valid
}

pub fn get_quoted_members<S: ForumService>(
    service: &S,
    body: &str,
    poster_id: i64,
) -> ServiceResult<Vec<MemberRecord>> {
    if body.is_empty() {
        return Ok(Vec::new());
    }
    lazy_static! {
        static ref QUOTE_AUTHOR: Regex =
            Regex::new(r"\[quote[^\]]*author=([^\]\n]+)[^\]]*\]").unwrap();
    }
    let mut names = Vec::new();
    for caps in QUOTE_AUTHOR.captures_iter(body) {
        names.push(caps[1].to_string());
    }
    if names.is_empty() {
        return Ok(Vec::new());
    }
    let members = service.find_members_by_name(&names)?;
    Ok(members
        .into_iter()
        .filter(|member| member.id != poster_id)
        .collect())
}
