# Future Incompatibilities

## sqlx-postgres v0.7.4 (Rust 2024 / never type fallback)

Running:

```
cargo report future-incompatibilities
```

produces warnings for `sqlx-postgres v0.7.4` related to **never type fallback**.

Key points:
- These warnings will become hard errors in Rust 2024 and later.
- They impact `sqlx-postgres` internals (e.g. `recv_expect` in executor/copy).
- Newer sqlx versions are available (0.8.x+).

Recommended upgrade window:
1) Plan a dependency upgrade from `sqlx-postgres 0.7.4` → `0.8.x`
2) Run `cargo report future-incompatibilities --id 1 --package sqlx-postgres@0.7.4`
3) Validate with `cargo check` + integration tests

Reference output (trimmed):
- `sqlx-postgres` depends on never type fallback being `()`
- See: <https://doc.rust-lang.org/edition-guide/rust-2024/never-type-fallback.html>
