# ErrorCode Reference

This project returns a unified error payload:

```
{
  "code": "<snake_case_error_code>",
  "message": "<human_readable_message>",
  "details": { ... } // optional
}
```

The `code` values are defined in `crates/shared/src/error.rs`.

## Codes

### `unauthorized`
- Meaning: Missing or invalid authentication (no token, expired token, invalid token).
- Typical HTTP status: `401 Unauthorized`

### `forbidden`
- Meaning: Authenticated but not allowed (insufficient permissions, banned user, access rule denies).
- Typical HTTP status: `403 Forbidden`

### `validation`
- Meaning: Request validation failure (missing fields, invalid values, bad input).
- Typical HTTP status: `400 Bad Request`

### `not_found`
- Meaning: Resource does not exist.
- Typical HTTP status: `404 Not Found`

### `conflict`
- Meaning: Resource already exists or request conflicts with current state.
- Typical HTTP status: `409 Conflict`

### `rate_limited`
- Meaning: Too many requests for a given rate limit window.
- Typical HTTP status: `429 Too Many Requests`

### `bad_gateway`
- Meaning: Upstream or dependency failure (e.g., rainbow-auth or database connectivity issues).
- Typical HTTP status: `502 Bad Gateway` / `503 Service Unavailable` / `504 Gateway Timeout`

### `internal`
- Meaning: Unexpected server error.
- Typical HTTP status: `500 Internal Server Error`

## Mapping rules (backend)
- See `src/api/error.rs` for the authoritative mapping:
  - `401` → `unauthorized`
  - `403` → `forbidden`
  - `404` → `not_found`
  - `409` → `conflict`
  - `429` → `rate_limited`
  - `502/503/504` → `bad_gateway`
  - other `5xx` → `internal`
  - other `4xx` → `validation`
