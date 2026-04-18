# Frontend Refactor Progress

Last updated: 2026-03-07 15:32 (GMT+8)

## Overall
- Progress: **100%**

## Done (✅)
- [x] P0: Remove JWT-in-URL (`token=`) links for Blog/Docs (use `/blog/` and `/docs/`).
- [x] P0: Stop persisting JWT in `localStorage` (migrated to `sessionStorage`; best-effort cleanup of legacy keys).
- [x] P0: Polling loops improved: stop conditions + exponential backoff (max 60s) + jitter + visibility gating.
- [x] P1 (start): Extract CSS into `src/style.rs`.
- [x] P1 (start): Add `ApiClient` (`src/api/client.rs`) and route JSON requests through it.
- [x] P1: Extract pages: `LoginPage`, `RegisterPage`.
- [x] P1: Extract `TopNav` component (`src/components/nav.rs`).
- [x] P1: Extract Home sections (Hero + connection config) into `src/pages/home.rs`.
- [x] P1: Extract Topic detail view into `src/pages/topic_detail.rs`.
- [x] P1: Extract Admin page into `src/pages/admin.rs`.
- [x] Services: Introduce `src/services/forum.rs` and migrate boards/topics/posts + create_post + create_board/create_topic to it.
- [x] Services: Add `src/services/admin.rs` and migrate admin ops (users/admins/moderator/docs/transfer/board_access/permissions/bans) to it.
- [x] Services: Add `src/services/pm.rs` and migrate PM load/send/read/delete + polling refresh to it.
- [x] Services: Add `src/services/attachments.rs` and `src/services/notifications.rs` (API surface ready; UI integration pending full extraction).
- [x] UI: Add an in-page progress bar in the status bar.

## Doing (🛠)
- [ ] Follow-ups: reduce `src/app.rs` further (target <300 LOC) by extracting remaining forum list + PM/attachments/notifications UI blocks.
- [ ] Follow-ups: gate/remove placeholder/demo actions behind a feature flag (release-safe).

## Next (⏭)
1) Extract Home sections (`Hero` + connection config panel) into `src/pages/home.rs`.
2) Extract Topic detail view into `src/pages/topic_detail.rs`.
3) Split Admin area into `src/pages/admin/*`.
4) Move action logic into `src/services/*` so UI components are thinner.
5) Gate/remove placeholder/demo actions behind a feature flag (release-safe).

## Notes / Risks
- `sessionStorage` is a stop-gap only. Mid-term plan is cookie-based auth (HttpOnly) or one-time SSO codes.
- Polling is still polling; mid-term we should consider SSE/WebSocket for PM/notifications.
