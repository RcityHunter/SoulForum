# API ErrorCode 规范（btc-forum-rust）

## 格式
所有非 2xx 响应使用以下结构：

```json
{
  "code": "unauthorized",
  "message": "authorization required",
  "details": { }
}
```

`details` 目前未启用，保留扩展。

---

## ErrorCode 列表与语义

| code | 含义 | 常见场景 | HTTP 状态 |
|------|------|----------|----------|
| `unauthorized` | 未登录/缺少 token | 未带 Authorization | 401 |
| `forbidden` | 权限不足/被禁止 | 缺少权限、被封禁、CSRF 缺失 | 403 |
| `validation` | 参数不合法 | 缺字段、长度不对、格式错误 | 400 |
| `not_found` | 资源不存在 | 访问不存在 ID | 404 |
| `conflict` | 冲突 | 重复注册、状态冲突 | 409 |
| `rate_limited` | 触发限流 | 登录/注册频繁 | 429 |
| `bad_gateway` | 上游服务故障 | rainbow-auth 不可用 | 502 |
| `internal` | 内部错误 | 业务异常/DB异常 | 500 |

---

## 示例

### 1) 未登录
```
HTTP/1.1 401 Unauthorized
{"code":"unauthorized","message":"authorization required"}
```

### 2) 缺少 CSRF
```
HTTP/1.1 403 Forbidden
{"code":"forbidden","message":"missing csrf token"}
```

### 3) 参数错误
```
HTTP/1.1 400 Bad Request
{"code":"validation","message":"subject must be 1..200 chars"}
```

### 4) 后端异常
```
HTTP/1.1 500 Internal Server Error
{"code":"internal","message":"failed to load permissions"}
```

---

## 前端使用建议
- 根据 `code` 做 UI 分支：
  - `unauthorized` → 提示登录
  - `forbidden` → 显示权限不足/被封禁
  - `validation` → 显示具体 message
  - `bad_gateway` → 提示服务异常（auth 未启动）
  - `internal` → 提示系统异常
