#![allow(dead_code)]

pub const BOARD_LIST: &str = "board.list";
pub const TOPIC_LIST: &str = "topic.list";
pub const TOPIC_GET: &str = "topic.get";
pub const TOPIC_CREATE: &str = "topic.create";
pub const REPLY_CREATE: &str = "reply.create";
pub const NOTIFICATION_LIST: &str = "notification.list";
pub const PM_SEND: &str = "pm.send";
pub const MODERATION_BAN_LIST: &str = "moderation.ban.list";
pub const MODERATION_BAN_APPLY: &str = "moderation.ban.apply";
pub const SYSTEM_HEALTH: &str = "system.health";

pub const ALL: &[&str] = &[
    BOARD_LIST,
    TOPIC_LIST,
    TOPIC_GET,
    TOPIC_CREATE,
    REPLY_CREATE,
    NOTIFICATION_LIST,
    PM_SEND,
    MODERATION_BAN_LIST,
    MODERATION_BAN_APPLY,
    SYSTEM_HEALTH,
];
