# 部署与配置指引

## 依赖
- Rust 1.75+（本仓库使用 2024 edition）。
- SurrealDB 2.x（HTTP 模式）。

## 本地启动
1) 启动 Rainbow-Auth（确保 `JWT_SECRET` 与本服务一致）。
2) 准备环境变量（见 `.env.example`）。
3) 启动 API：
```bash
cargo run --bin api
```
默认监听 `127.0.0.1:3000`。

## 环境变量要点
- Surreal 连接：`SURREAL_ENDPOINT`、`SURREAL_NAMESPACE`、`SURREAL_DATABASE`、`SURREAL_USER`、`SURREAL_PASS`
- 监听地址：`BIND_ADDR`（默认 `127.0.0.1:3000`）
- JWT：`JWT_SECRET`（需与 Rainbow-Auth 一致；或 `JWT_PUBLIC_KEY_PEM` 用于公钥校验）
- Rainbow-Auth：`RAINBOW_AUTH_BASE_URL`（默认 `http://127.0.0.1:8080`）
- CORS：`CORS_ORIGIN`（前端开发常用 `http://127.0.0.1:8080`）
- CSRF：`ENFORCE_CSRF=0` 可在开发期关闭
- 上传：`UPLOAD_DIR`、`UPLOAD_BASE_URL`、`MAX_UPLOAD_MB`、`ALLOWED_MIME`

## SurrealDB 初始化
默认命名空间 `auth`、数据库 `main`：
```bash
surreal sql --conn $SURREAL_ENDPOINT \
  --user $SURREAL_USER --pass $SURREAL_PASS \
  --ns ${SURREAL_NAMESPACE:-auth} --db ${SURREAL_DATABASE:-main} \
  -f migrations/surreal/0001_init.surql
```

## 生产建议
- 使用稳定的 `JWT_SECRET` 或公钥校验（`JWT_PUBLIC_KEY_PEM`）。
- 为 `UPLOAD_DIR` 配置持久化存储与备份策略。
- 配置反向代理（Nginx/Caddy）处理 TLS 与静态上传路径。
