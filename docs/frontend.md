# Dioxus 前端指引

该前端为 SurrealDB API 的交互面板，支持论坛与管理后台两种视图。

## 启动
```bash
cd frontend
cargo install dioxus-cli

dx serve --platform web --addr 127.0.0.1 --port 8080
```
访问 `http://127.0.0.1:8080`。

## 使用说明
- 页面顶部可填写 API 基址（默认 `http://127.0.0.1:3000`）。
- 登录/注册对接 Rainbow-Auth：使用邮箱+密码，注册通常需要邮箱验证，登录成功后会在本地存储 JWT，可直接粘贴或清空。
- “健康检查”按钮可快速确认后端与 SurrealDB 状态。
- 管理后台入口：`/admin`，需要管理员 JWT。

## 跨域与 CSRF
- 本地开发需确保 `CORS_ORIGIN=http://127.0.0.1:8080`。
- 如需关闭 CSRF 校验，设置 `ENFORCE_CSRF=0`。

## 备注
- 通知与附件当前偏向“占位交互”，用于验证后端流程。
- 上传文件会通过 `/uploads/*path` 提供下载访问。
