# Surreal 模型约定（初版）

当前 API/CLI 默认使用 SurrealDB，采用以下 collection 字段约定（迁移脚本位于 `migrations/surreal/0001_init.surql`，包含核心表及基础索引，可通过 `surreal sql --conn $SURREAL_ENDPOINT --user $SURREAL_USER --pass $SURREAL_PASS --ns ${SURREAL_NAMESPACE:-auth} --db ${SURREAL_DATABASE:-main} -f migrations/surreal/0001_init.surql` 执行）。

## boards
- `id`: Surreal record id（字符串）
- `name`: 标题
- `description`: 描述（可空）
- `created_at`: `time::now()`

索引：按 `created_at` 倒序列出。

## topics
- `id`: Surreal record id
- `board_id`: `boards` 记录 id（字符串）
- `subject`: 标题
- `author`: 作者名称（来自上下文/claims）
- `created_at` / `updated_at`: `time::now()`

索引：按 `board_id` 过滤，`created_at` 倒序。

## posts
- `id`: Surreal record id
- `topic_id`: 主题 id
- `board_id`: 版块 id
- `subject`: 标题（回复默认 `Re: topic`）
- `body`: 正文
- `author`: 作者名称
- `created_at`: `time::now()`

索引：按 `topic_id` 升序列出；也可按 `created_at` 全局倒序查看。

## users（规划）
- `id`: Surreal record id
- `name`: 显示名
- `password_hash`: Argon2id 哈希（登录用），可空（外部 OAuth 创建时为空）
- `role`: 角色（如 `admin`/`mod`/`member`/`guest`）
- `permissions`: 数组，按需存储字符串权限（如 `post_new`/`post_reply_any` 等）
- 其他：`created_at`/`updated_at` 等

说明：代码会在需要时按 `name=sub` 查询/创建用户，填充 `ForumContext.user_info` 的 `role`/`permissions`，`admin`/`mod` 角色会赋予 `is_admin`/`is_mod`。

## personal_messages（基础版）
- `pm_id`: 数字型业务 id（毫秒时间戳）
- `owner_id`: 收件人/发件人所属用户 id
- `sender_id`/`sender_name`: 发送者信息
- `subject` / `body`
- `is_read`: 是否已读
- `folder`: `"Inbox"` 或 `"Sent"`
- `recipients`: 收件人 id 列表
- `created_at_ms`: 发送时间（毫秒）

## attachments（基础版）
- `id`: 数字型 id（毫秒时间戳）
- `name` / `tmp_path` / `size` / `mime_type` / `width` / `height`
- `message_id`: 关联帖子 id，可空
- `approved`: 是否通过
- `created_at_ms`: 创建时间

## notifications（占位）
- `id`: 字符串 id（Surreal 记录）
- `user`: 接收方用户名
- `subject` / `body`
- `is_read`: 是否已读
- `created_at_ms`: 创建时间

## agent_verification_challenges / agent_verification_failures
- `agent_verification_challenges`: 以 `verification_code` 作为记录键，字段包括 `challenge_id`、`verification_code`、`agent_subject`、`action_kind`、`payload_json`、`challenge_text`、`expected_answer`、`generator_version`、`generator_seed`、`status`、`attempt_count`、`max_attempts`、`expires_at`、`verified_at`、`created_at`
- `agent_verification_failures`: 以 `agent_subject` 作为记录键，字段包括 `agent_subject`、`consecutive_failures`、`last_failure_at`、`last_success_at`
- 索引：`agent_verification_challenges.verification_code` 唯一；`agent_verification_challenges(agent_subject, created_at)`、`agent_verification_challenges(status, expires_at)`；`agent_verification_failures.agent_subject` 唯一

## drafts / pm_drafts / pm_labels / pm_preferences
- `drafts`: `id`、`board_id`、`topic_id`、`subject`、`body`、`icon`、`smileys_enabled`、`locked`、`sticky`、`poster_time_ms`
- `pm_drafts`: `id`、`owner_id`、`subject`、`body`、`to_members`、`bcc_members`、`saved_at_ms`
- `pm_labels`: `label_id`、`owner_id`、`name`、`created_at_ms`
- `pm_preferences`: `owner_id`、`receive_from`、`notify_level`
- `pm_ignore_lists`: `owner_id`、`ignored_id`
- `buddy_lists`: `owner_id`、`buddy_id`

## polls / poll_options（基础版）
- `polls`: `id`、`topic_id`、`question`、`max_votes`、`change_vote`、`guest_vote`、`created_at_ms`
- `poll_options`: `poll_id`、`option_id`、`label`、`votes`

## 权限与版块访问
- `membergroups`: `id`、`name`、`description`、`type`、`min_posts`、`color`、`hidden`、`permissions`（数组，存储字符串权限）
- `board_access`: `board_id`、`allowed_groups`（数组，表示可见分组）
- `board_permissions`: `board_id`、`group_id`、`allow`（数组）、`deny`（数组）
- `permission_groups` / `permission_profiles`: `id`、`name`、`color` 等

## ban_rules / ban_logs / action_logs
- `ban_rules`: `id`、`reason`、`expires_at_ms`
- `ban_logs`: `id`、`ban_id`、`email`、`hit_at_ms`
- `action_logs`: `id`、`action`、`member_id`、`details`、`created_at_ms`

说明：
- SurrealService 中的发帖/回帖路径使用上述字段；其余 ForumService 方法目前返回空/占位，后续按需要实现。
- 作者/身份默认取 JWT `sub`，匿名为 `guest`。默认会在缺版块时创建 `General` 版块。
- 权限要求：创建版块需 `manage_boards` 或管理员角色；创建主题需 `post_new`；回帖需 `post_reply_any`。JWT 中未携带权限时，会为普通用户填充 `post_new`/`post_reply_any` 便于调试。
- 未实现的版主管理/成员管理等接口仍会显式返回 “not supported in SurrealService”。私信、附件、投票、封禁/审计日志已有基础能力。
