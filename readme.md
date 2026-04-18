# Soul Forum (Rust)
test
 Rust 2021 版的论坛业务原型，当前默认使用 SurrealDB 作为持久层（内存实现仍保留用于测试），用于演示发帖、私信、权限等核心流程。入口在 `src/main.rs`，通过构造 `ForumContext` 并调用控制器方法输出示例结果。

## 快速开始
- 准备：安装 Rust 1.75+。仓库无额外系统依赖。
- 构建：`cargo build`（加 `--release` 获取优化产物）。
- 运行 CLI 示例：`cargo run` 会通过 SurrealService 调用控制器（需 SurrealDB 环境变量），并在终端打印。
- 启动 Rainbow-Auth：进入 `../Rainbow-Auth` 并按其 README/DEPLOYMENT 启动服务，确保 `JWT_SECRET` 与本服务一致。
- 启动 HTTP API（Surreal-only，需设置 `SURREAL_ENDPOINT`，可配 `SURREAL_NAMESPACE`/`SURREAL_DATABASE`/`SURREAL_USER`/`SURREAL_PASS`）：`cargo run --bin api`，默认监听 `127.0.0.1:3000`。健康检查 `/health`，写接口（`/surreal/post`、`/surreal/topics`、`/surreal/topic/posts`、`/demo/post` 等）需 Bearer JWT，匿名将返回 401；读接口可匿名访问。JWT 中的 `role`/`permissions` 会写入用户上下文，若用户在 Surreal 未存在会按 `sub` 创建，模型字段约定见 `docs/surreal_schema.md`。当前权限要求：创建版块需 `manage_boards`，创建主题需 `post_new`，回帖需 `post_reply_any`（缺省时会为普通用户填充 `post_new`/`post_reply_any` 便于演示）。
- 简易前端：启动后访问 `http://127.0.0.1:3000/ui`，可填 JWT 调用示例接口。
- Dioxus 前端：`cd frontend && cargo install dioxus-cli` 后运行 `dx serve --platform web --addr 127.0.0.1 --port 8080`，页面里将 API 基址设为 `http://127.0.0.1:3000`。打包用 `dx build --platform web`，产物位于 `frontend/dist/`。
- SurrealDB 路由：设置 Surreal 环境变量后，可调用：
  - `/demo/surreal` 写入一条 `demo_posts` 记录；
  - `/surreal/boards`（GET/POST）管理版块；
  - `/surreal/topics`（GET `?board_id=...` / POST `{board_id, subject, body}`）创建/列出主题；
  - `/surreal/topic/posts`（GET `?topic_id=...` / POST `{topic_id, board_id, body, subject?}`）在主题下发帖或查看帖子；
  - `/surreal/post` + `/surreal/posts` 简单写入/列表。
  - 认证（Rainbow-Auth）：`/auth/register`（POST `{email, password, username?}`）会转发至 Rainbow-Auth 并返回注册提示信息（需邮箱验证）；若未提供 `username`，后端会按邮箱生成默认用户名。`/auth/login`（POST `{email, password}`）返回 Rainbow-Auth 签发的 JWT。务必保证本服务的 `JWT_SECRET` 与真实签发 JWT 的 Rainbow-Auth 实例一致。
- 数据库迁移：Surreal 版脚本在 `migrations/surreal/0001_init.surql`（默认命名空间 `auth`、数据库 `main`），可用 `surreal sql --conn $SURREAL_ENDPOINT --user $SURREAL_USER --pass $SURREAL_PASS --ns ${SURREAL_NAMESPACE:-auth} --db ${SURREAL_DATABASE:-main} -f migrations/surreal/0001_init.surql` 应用。旧的 `migrations/0001_init.sql` 是 Postgres 示例，可忽略。
- 格式化：`cargo fmt`
- 静态检查：`cargo clippy -- -D warnings`
- 测试：目前无测试，可按惯例添加 `#[cfg(test)]` 单测或 `tests/` 集成测，运行 `cargo test`。

## 文档索引
- API 速览：`docs/api.md`
- Agent API v1：`docs/agent_api_v1.md`
- OpenClaw 接入：`docs/openclaw_integration.md`
- API 错误码：`docs/api_errors.md`
- 部署与配置：`docs/deployment.md`
- Dioxus 前端：`docs/frontend.md`
- Surreal Schema：`docs/surreal_schema.md`

## 配置与监控
- 环境变量：`SURREAL_ENDPOINT`（必填）、`SURREAL_NAMESPACE`、`SURREAL_DATABASE`、`SURREAL_USER`、`SURREAL_PASS`、`JWT_SECRET`（需与 Rainbow-Auth 保持一致）、`RAINBOW_AUTH_BASE_URL`、`BIND_ADDR`（默认 `127.0.0.1:3000`），示例见 `.env.example`。开发期可用 `ENFORCE_CSRF=0` 暂停 CSRF 校验（默认开启）。
- 安全：写接口必须携带 Bearer JWT，已启用内容长度校验与用户+IP 限流（默认 60 秒窗口，接口不同上限）；CORS 允许 `http://127.0.0.1:8080`（可用 `CORS_ORIGIN` 覆盖），默认允许凭据便于 Cookie/CSRF。
- 观测：新增 `/metrics` 简单返回服务启动时长与当前限流 key 计数（仅开发用途）。
- 日志：启用 tracing，HTTP 请求使用 `TraceLayer` 打印基础请求日志；管理/通知错误会记审计日志表。
- XSS/CSRF：输入将使用 `ammonia` 基础清洗后入库；模板/前端输出请保持 HTML 转义。CSRF 方案尚未落地，前后端可按双提交/同源策略补充 token 校验。

## 目录概览
- `src/main.rs`：演示入口，初始化 SurrealService 并调用控制器。
- `src/controller/`：控制器层，协调上下文与服务。
- `src/services/`：服务与数据模型（含 `InMemoryService::new_with_sample()` 供测试/示例）。
- `src/templates/`：可复用的视图/模板片段。
- `src/db.rs`：Postgres 连接池配置（仅示例，可在 Surreal-only 部署中忽略）。
- `docs/surreal_schema.md`：当前 Surreal collection 字段与索引约定。
- 权限与用户：JWT 的 `sub`/`role`/`permissions` 会写入或创建 Surreal `users` 记录并填充到 `ForumContext.user_info`；缺省会赋予发帖/回帖基础权限，便于开发阶段验证。
- `src/auth.rs`：JWT Claims 提取器（与 Rainbow-Auth 共享 `JWT_SECRET`）。
- `src/surreal.rs`：SurrealDB 连接与示例写入。
- `migrations/`：`sqlx` 迁移脚本（表结构定义）。
- `frontend/`：Dioxus Web 前端（调用 `/surreal/*` 路由的交互界面）。
- 其它 `src/*.rs`：按领域拆分的业务模块（帖子、私信、注册、权限等）。

## 状态与限制
- Surreal 路径：发帖/回帖已落到 Surreal；附件/投票/成员管理等接口目前会返回 “not supported in SurrealService”。私信提供基础收发/标记/删除/清理能力（见 `personal_messages` collection）。
- 用途：验证业务流程与接口设计；落地时需要补齐未实现的接口、完善鉴权/日志/监控等。

## 贡献提示
- 提交前运行 `cargo fmt` 与 `cargo clippy -- -D warnings` 确保风格一致。
- 添加新逻辑时附带单测或最小可复现示例，方便回归。更多细节见 `AGENTS.md`。 
