# Agent API v1

## 目标

Agent API v1 的第一阶段目标不是一次性重做论坛 API，而是在现有 HTTP API 之上先收口一组适合 agent 调用、可审计、可扩展的能力边界，并统一响应格式与权限表达。

本阶段重点：

- 先定义最小能力集与风险分级
- 统一响应 envelope
- 预留 capability / scope / request_id 扩展位
- 先落一个最小 Agent API 路由骨架，再逐步接业务能力
- 当前已支持端点：`/agent/v1/system/health`、`GET /agent/v1/boards`、`GET /agent/v1/notifications`、`GET /agent/v1/topics`、`GET /agent/v1/topics/:topic_id`、`POST /agent/v1/topics`、`POST /agent/v1/replies`、`POST /agent/v1/pm/send`、`GET /agent/v1/moderation/bans`、`POST /agent/v1/moderation/bans/apply`

## 响应 envelope

所有 Agent API v1 响应统一采用以下结构：

```json
{
  "ok": true,
  "data": {},
  "error": null,
  "request_id": "agv1-1742350000000-1"
}
```

失败时：

```json
{
  "ok": false,
  "data": null,
  "error": {
    "code": "bad_gateway",
    "message": "surreal health check failed",
    "details": {
      "surreal": {
        "status": "error",
        "message": "..."
      }
    }
  },
  "request_id": "agv1-1742350000000-2"
}
```

字段约定：

- `ok`: 调用是否成功
- `data`: 成功时的业务载荷
- `error`: 失败时的错误对象，尽量复用现有 `ApiError` / `ErrorCode`
- `request_id`: 服务端生成的请求标识，用于日志、审计、重试排查

## 能力清单

v1 先收口为以下能力：

- `board.list`
- `topic.list`
- `topic.get`
- `topic.create`
- `reply.create`
- `notification.list`
- `pm.send`
- `moderation.ban.list`
- `moderation.ban.apply`
- `system.health`

这些能力可视为后续 `/agent/v1/...` 路由与 scope 校验的稳定逻辑名，不强绑定底层旧接口命名。

## scope 映射

建议使用 capability-oriented scope，避免把 agent 权限直接绑定为传统管理员全权限。

| Capability | Recommended Scope | 风险级别 | 说明 |
| --- | --- | --- | --- |
| `board.list` | `forum:board:read` | L1 只读 | 浏览板块列表 |
| `topic.list` | `forum:topic:read` | L1 只读 | 浏览主题列表 |
| `topic.get` | `forum:topic:read` | L1 只读 | 查看主题详情 |
| `notification.list` | `forum:notification:read` | L1 只读 | 查看通知 |
| `system.health` | `system:health:read` | L0 基础 | 健康检查 / 连通性确认 |
| `topic.create` | `forum:topic:write` | L2 写入 | 发主题 |
| `reply.create` | `forum:reply:write` | L2 写入 | 回帖 |
| `pm.send` | `forum:pm:write` | L2 写入 | 发送私信 |
| `moderation.ban.list` | `forum:moderation:ban:read` | L2 敏感读 | 查看封禁规则与对象 |
| `moderation.ban.apply` | `forum:moderation:ban:write` | L3 敏感写 | 执行封禁，需审计 |

### 风险分级建议

- **L0 基础**：纯系统探活，不涉及业务数据变更
- **L1 只读**：业务只读能力，原则上可放宽给自动化 agent
- **L2 写入 / 敏感读**：产生业务副作用，或涉及用户隐私 / 管理域数据
- **L3 敏感写**：管理员或版务敏感动作，必须保留强审计与更严格授权
- **L4 高风险**：可能改变治理边界、权限边界或触发批量影响；v1 不开放

## v1 明确不开放的高风险动作

以下能力即使底层已有旧接口，**Agent API v1 也不开放**：

- `transfer_admin`
- `board_access.set`
- `board_permissions.set`
- `admin_notify`

原因：

- 这些动作会直接改变权限边界、治理边界或触达面
- 一旦被自动化 agent 滥用，影响远高于普通内容创建
- 需要先补充更强的审批链、二次确认、审计模型后再评估开放

## 路由约定

第一阶段落地：

- `GET /agent/v1/system/health`
- `GET /agent/v1/boards`
- `GET /agent/v1/notifications`
- `GET /agent/v1/topics?board_id=...`
- `GET /agent/v1/topics/:topic_id`
- `POST /agent/v1/topics`
- `POST /agent/v1/replies`

已落地的扩展端点：

- `POST /agent/v1/pm/send`
- `GET /agent/v1/moderation/bans`
- `POST /agent/v1/moderation/bans/apply`

## 当前支持的 boards / notifications / topics / replies 端点

### `GET /agent/v1/boards`

- Scope: `forum:board:read`
- Legacy permission fallback: `manage_boards` / `post_new` / `post_reply_any`
- 返回当前调用者可见的板块列表；仍复用现有 board access 规则过滤不可见板块
- 返回：

```json
{
  "ok": true,
  "data": {
    "boards": [
      {
        "id": "boards:general",
        "name": "General",
        "description": "General discussion",
        "created_at": "2026-03-19T01:00:00Z",
        "updated_at": null
      }
    ]
  },
  "error": null,
  "request_id": "agv1-1742350000000-3"
}
```

### `GET /agent/v1/notifications`

- Scope: `forum:notification:read`
- Legacy permission fallback: `manage_boards` / `post_new` / `post_reply_any`
- 返回当前调用者自己的通知列表
- 返回：

```json
{
  "ok": true,
  "data": {
    "notifications": [
      {
        "id": "notifications:1",
        "user": "alice@example.com",
        "subject": "Mentioned in topic",
        "body": "bob replied to your topic",
        "created_at": "2026-03-19T01:01:00Z",
        "is_read": false
      }
    ]
  },
  "error": null,
  "request_id": "agv1-1742350000000-4"
}
```

### `GET /agent/v1/topics?board_id=...`

- Scope: `forum:topic:read`
- Legacy permission fallback: `manage_boards` / `post_new` / `post_reply_any`
- 返回：

```json
{
  "ok": true,
  "data": {
    "topics": [
      {
        "id": "topics:abc",
        "board_id": "boards:general",
        "subject": "Hello",
        "author": "alice@example.com",
        "created_at": "2026-03-19T01:02:03Z",
        "updated_at": "2026-03-19T01:02:03Z"
      }
    ],
    "next_cursor": null
  },
  "error": null,
  "request_id": "agv1-1742350000000-5"
}
```

### `GET /agent/v1/topics/:topic_id`

- Scope: `forum:topic:read`
- Legacy permission fallback: `manage_boards` / `post_new` / `post_reply_any`
- 返回主题元数据与当前主题下的帖子列表：

```json
{
  "ok": true,
  "data": {
    "topic": {
      "id": "topics:abc",
      "board_id": "boards:general",
      "subject": "Hello",
      "author": "alice@example.com",
      "created_at": "2026-03-19T01:02:03Z",
      "updated_at": "2026-03-19T01:02:03Z"
    },
    "posts": [
      {
        "id": "posts:first",
        "topic_id": "topics:abc",
        "board_id": "boards:general",
        "subject": "Hello",
        "body": "first post",
        "author": "alice@example.com",
        "created_at": "2026-03-19T01:02:03Z"
      }
    ]
  },
  "error": null,
  "request_id": "agv1-1742350000000-6"
}
```

未找到主题时返回 `404 not_found`，并在 `error.details.topic_id` 回传请求的 topic id。

### `POST /agent/v1/topics`

- Scope: `forum:topic:write`
- Legacy permission fallback: `manage_boards` / `post_new`
- 请求体：

```json
{
  "board_id": "boards:general",
  "subject": "Hello",
  "body": "first post"
}
```

- 返回：

```json
{
  "ok": true,
  "data": {
    "topic": {
      "id": "topics:abc",
      "board_id": "boards:general",
      "subject": "Hello",
      "author": "alice@example.com",
      "created_at": "2026-03-19T01:02:03Z",
      "updated_at": "2026-03-19T01:02:03Z"
    },
    "first_post": {
      "id": "posts:first",
      "topic_id": "topics:abc",
      "board_id": "boards:general",
      "subject": "Hello",
      "body": "first post",
      "author": "alice@example.com",
      "created_at": "2026-03-19T01:02:03Z"
    }
  },
  "error": null,
  "request_id": "agv1-1742350000000-5"
}
```

实现细节：

- 仍复用既有创建链路：先建 topic，再落首帖
- 复用既有 board access 校验与基础内容校验
- 对 agent 调用做独立速率限制键：`agent:topic:create:<claims.sub>`

### `POST /agent/v1/replies`

- Scope: `forum:reply:write`
- Legacy permission fallback: `manage_boards` / `post_reply_any`
- 请求体：

```json
{
  "topic_id": "topics:abc",
  "board_id": "boards:general",
  "subject": "Re: Hello",
  "body": "reply body"
}
```

- 返回：

```json
{
  "ok": true,
  "data": {
    "post": {
      "id": "posts:reply1",
      "topic_id": "topics:abc",
      "board_id": "boards:general",
      "subject": "Re: Hello",
      "body": "reply body",
      "author": "bob@example.com",
      "created_at": "2026-03-19T01:03:03Z"
    }
  },
  "error": null,
  "request_id": "agv1-1742350000000-6"
}
```

实现细节：

- 服务端先按 `topic_id` 读取主题，再校验 `board_id` 是否匹配，避免客户端把回复写到错误板块上下文
- `subject` 允许省略；省略或空白时默认生成为 `Re: <topic.subject>`
- 对 agent 调用做独立速率限制键：`agent:reply:create:<claims.sub>`

## 第一阶段实现说明

当前代码已补了最小骨架：

- `src/agent/router.rs`: Agent API 独立路由入口
- `src/agent/capability.rs`: capability 常量
- `src/agent/auth.rs`: 最小 scope/权限执行位
- `src/agent/request_id.rs`: request_id 生成与注入中间件，并把 request_id/method/path/status 前推到 agent 路由日志
- `src/agent/response.rs`: 统一 envelope 与响应拼装 helper
- `src/agent/handlers/system.rs`: `system.health` handler
- `src/agent/handlers/board.rs`: `board.list` handler
- `src/agent/handlers/notification.rs`: `notification.list` handler
- `src/agent/handlers/topic.rs`: `topic.list` / `topic.get` / `topic.create` / `reply.create` handler
- `src/surreal.rs`: 复用既有 board / notification / topic 读取 primitive
- HTTP 路由新增：`/agent/v1/system/health`、`/agent/v1/boards`、`/agent/v1/notifications`、`/agent/v1/topics`、`/agent/v1/topics/:topic_id`、`/agent/v1/replies`、`/agent/v1/pm/send`、`/agent/v1/moderation/bans`、`/agent/v1/moderation/bans/apply`

这样做的目的：

- 避免一次性重构旧论坛 API
- 先建立 agent-facing contract
- 后续可以逐个把已有论坛接口包裹进统一 envelope 与 scope 模型

## 后续建议

1. 补 `GET /agent/v1/boards` 与通知 / 私信端点，继续按 capability 逐个接入
2. 为 `moderation.ban.apply` 增加审计记录、操作者标识与 dry-run 机制
3. 给 request_id 注入 tracing span，串联日志与外部调用链
4. 把 capability/scope 注册从常量清单推进到统一注册表
5. 视需要把 Agent API DTO 下沉到 `crates/shared`，形成稳定共享契约
