pub const STYLE: &str = r#"
@import url('https://fonts.googleapis.com/css2?family=Space+Grotesk:wght@400;500;600;700&family=Plus+Jakarta+Sans:wght@400;500;600;700&display=swap');
:root {
    --bg: #f1f1f1;
    --paper: #ffffff;
    --ink: #1a1a1a;
    --muted: #6d6d6d;
    --accent: #e14b4b;
    --accent-2: #2c2c2c;
    --border: #e5e5e5;
    --shadow: 0 16px 32px rgba(0, 0, 0, 0.08);
    --radius: 12px;
    --radius-soft: 8px;
}
* { box-sizing: border-box; }
html, body { padding: 0; margin: 0; min-height: 100%; }
body {
    background: var(--bg);
    color: var(--ink);
    font-family: "Plus Jakarta Sans", "Noto Sans SC", system-ui, -apple-system, sans-serif;
}
h1, h2, h3, h4 { font-family: "Space Grotesk", "Noto Sans SC", sans-serif; }
a { color: inherit; text-decoration: none; }

@keyframes rise {
    from { opacity: 0; transform: translateY(8px); }
    to { opacity: 1; transform: translateY(0); }
}

.app-shell {
    max-width: 1160px;
    margin: 0 auto;
    padding: 26px 20px 60px;
    display: flex;
    flex-direction: column;
    gap: 18px;
}
.top-nav {
    position: sticky;
    top: 0;
    z-index: 10;
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 10px 16px 14px;
    background: var(--accent-2);
    color: #f4f4f4;
    border-radius: 0 0 var(--radius) var(--radius);
    box-shadow: 0 6px 0 rgba(225, 75, 75, 0.95);
}
.top-strip {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
}
.brand {
    display: flex;
    align-items: center;
    gap: 10px;
    font-weight: 700;
    font-size: 18px;
}
.brand__dot {
    width: 16px;
    height: 16px;
    border-radius: 4px;
    background: var(--accent);
}
.brand__tag {
    padding: 2px 8px;
    border-radius: 999px;
    background: rgba(255,255,255,0.1);
    color: #f5f5f5;
    font-size: 11px;
}
.top-meta {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 12px;
    color: #c9c9c9;
    text-align: right;
}
.top-date { font-weight: 600; color: #ffffff; }

.nav-tabs {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 8px;
    padding-top: 6px;
}
.nav-tab {
    padding: 6px 12px;
    border-radius: 999px;
    border: 1px solid rgba(255,255,255,0.1);
    background: rgba(255,255,255,0.08);
    color: #f4f4f4;
    font-size: 12px;
    letter-spacing: 0.4px;
    text-transform: capitalize;
    transition: all 0.2s ease;
}
.nav-tab.active {
    background: #ffffff;
    color: #1b1b1b;
    border-color: #ffffff;
}
.nav-tab--ghost {
    background: transparent;
    border-style: dashed;
    cursor: pointer;
}
.nav-search {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: 8px;
}
.nav-search input {
    width: 210px;
    padding: 6px 10px;
    font-size: 12px;
    border-radius: 999px;
    border: none;
}
.nav-search__btn {
    padding: 6px 12px;
    font-size: 12px;
    border-radius: 999px;
    border: none;
    background: var(--accent);
    color: #fff;
}

.status-bar {
    border: 1px solid var(--border);
    border-radius: var(--radius-soft);
    padding: 10px 12px;
    color: var(--muted);
    background: var(--paper);
}

.progress {
    border: 1px solid var(--border);
    border-radius: 999px;
    overflow: hidden;
    background: #fafafa;
    height: 12px;
}
.progress__fill {
    height: 100%;
    width: 0%;
    background: linear-gradient(90deg, var(--accent), #ff8a66);
    transition: width 0.3s ease;
}
.progress__row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    margin-top: 10px;
}
.progress__label {
    font-size: 12px;
    color: var(--muted);
}
.progress__pct {
    font-size: 12px;
    font-weight: 700;
    color: var(--ink);
}
.progress__summary {
    margin-top: 10px;
    padding: 10px 12px;
    border-radius: var(--radius-soft);
    border: 1px dashed var(--border);
    background: #fff;
    color: var(--ink);
    font-size: 13px;
    line-height: 1.5;
}
.hero {
    display: grid;
    grid-template-columns: 1.5fr 1fr;
    gap: 18px;
    padding: 22px;
    border-radius: var(--radius);
    border: 1px solid var(--border);
    background: var(--paper);
    box-shadow: var(--shadow);
    animation: rise 0.5s ease both;
}
.hero__copy h1 { margin: 4px 0 10px; font-size: 28px; }
.hero__copy p { margin: 0 0 16px; color: var(--muted); max-width: 40ch; }
.hero__actions { display: flex; gap: 10px; flex-wrap: wrap; }
.hero__panel {
    background: #fafafa;
    border: 1px solid var(--border);
    border-radius: var(--radius-soft);
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 10px;
}
.stat { display: flex; flex-direction: column; gap: 4px; color: var(--muted); }
.stat strong { color: var(--ink); font-size: 15px; }
.stat-row { display: grid; grid-template-columns: repeat(auto-fit, minmax(110px, 1fr)); gap: 8px; }
.stat-box {
    background: #ffffff;
    border: 1px solid var(--border);
    border-radius: var(--radius-soft);
    padding: 10px;
    text-align: center;
}
.stat-box strong { font-size: 20px; display: block; color: var(--accent); }
.points-entry {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin-top: 8px;
    padding: 12px;
    border: 1px solid var(--border);
    border-radius: var(--radius-soft);
    background: #ffffff;
}
.points-entry--compact {
    gap: 8px;
    padding: 10px;
    background: #fafafa;
}
.points-entry__header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    flex-wrap: wrap;
}
.points-entry__hint {
    margin: 0;
    color: var(--muted);
    font-size: 12px;
    line-height: 1.45;
}
.points-badge {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
}
.points-badge--compact {
    gap: 6px;
}
.points-badge__metric {
    min-width: 74px;
    padding: 6px 10px;
    border-radius: 999px;
    background: rgba(225, 75, 75, 0.08);
    border: 1px solid rgba(225, 75, 75, 0.16);
}
.points-badge__label {
    display: block;
    font-size: 10px;
    text-transform: uppercase;
    color: var(--muted);
    letter-spacing: 0.05em;
}
.points-badge__metric strong {
    display: block;
    margin-top: 2px;
    font-size: 14px;
    color: var(--accent-2);
}
.points-badge__meta {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
}
.points-badge__trust {
    font-size: 12px;
    font-weight: 700;
    color: var(--accent-2);
}
.points-badge__state {
    padding: 4px 8px;
    border-radius: 999px;
    font-size: 10px;
    font-weight: 800;
    letter-spacing: 0.05em;
    text-transform: uppercase;
}
.points-badge__state.is-pending {
    color: #8d5d08;
    background: rgba(255, 196, 0, 0.16);
}
.points-badge__state.is-ready {
    color: #0f6f5d;
    background: rgba(11, 208, 176, 0.14);
}
.pill {
    display: inline-block;
    padding: 4px 10px;
    border-radius: 999px;
    background: #111;
    color: #fff;
    font-weight: 600;
    font-size: 11px;
}
.ghost-btn {
    padding: 8px 14px;
    border-radius: 999px;
    border: 1px solid var(--accent-2);
    background: transparent;
    color: var(--accent-2);
    cursor: pointer;
}
.ghost-btn:hover { background: rgba(0,0,0,0.05); }
.ghost-btn, .item { pointer-events: auto; }

.panel {
    background: var(--paper);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 16px;
    box-shadow: var(--shadow);
    animation: rise 0.5s ease both;
}
.panel h2, .panel h3, .panel h4 { margin: 0 0 12px; }
.panel__header { display: flex; align-items: baseline; justify-content: space-between; gap: 10px; }
.muted { color: var(--muted); font-size: 13px; }
.grid { display: grid; gap: 14px; }
.grid.two { grid-template-columns: repeat(auto-fit, minmax(320px, 1fr)); }
.grid.two.gap { gap: 16px; }
.register-panel h2 { margin-bottom: 8px; }
.register-note {
    padding: 10px 12px;
    border: 1px solid var(--border);
    background: #fafafa;
    border-radius: var(--radius-soft);
    color: var(--muted);
    font-size: 13px;
}
.register-feedback {
    margin-top: 12px;
    padding: 12px 14px;
    border-radius: var(--radius-soft);
    border: 1px solid rgba(46, 125, 50, 0.25);
    background: rgba(46, 125, 50, 0.08);
    color: #1b5e20;
    font-size: 13px;
    font-weight: 600;
}
.register-feedback--error {
    border-color: rgba(198, 40, 40, 0.28);
    background: rgba(198, 40, 40, 0.08);
    color: #b71c1c;
}
.register-grid { display: grid; grid-template-columns: 180px minmax(0, 1fr); gap: 12px; margin-top: 14px; }
.register-labels { display: flex; flex-direction: column; gap: 18px; font-weight: 600; color: var(--accent-2); }
.register-fields { display: flex; flex-direction: column; gap: 12px; }
.register-captcha { display: flex; align-items: center; gap: 10px; }
.captcha-box {
    padding: 8px 12px;
    border: 1px dashed #bdbdbd;
    border-radius: 8px;
    font-weight: 700;
    letter-spacing: 2px;
    color: var(--accent-2);
    background: #f9f9f9;
}
.register-actions { margin-top: 14px; display: flex; justify-content: flex-end; }
.login-panel { display: flex; justify-content: center; }
.login-box {
    width: min(420px, 100%);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 18px;
    background: var(--paper);
    box-shadow: var(--shadow);
}
.login-box h2 { margin-top: 0; }
.login-row { display: flex; flex-direction: column; gap: 6px; margin-top: 12px; }
.login-row--inline { flex-direction: row; align-items: center; gap: 8px; }
.login-links { margin-top: 12px; font-size: 12px; color: var(--muted); text-align: center; }

.forum-layout { display: grid; grid-template-columns: minmax(0, 1fr); gap: 18px; }
.forum-feed-layout {
    display: grid;
    grid-template-columns: minmax(0, 1.8fr) minmax(280px, 0.72fr);
    gap: 18px;
    align-items: start;
}
.forum-feed-main {
    padding: 0;
    overflow: hidden;
    background: #ece6dc;
    border-color: #1d1c22;
}
.forum-feed-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 12px 16px;
    background: #17171d;
    color: #f4efe8;
}
.forum-feed-header__left {
    display: flex;
    align-items: center;
    gap: 10px;
}
.forum-feed-live {
    color: #09d3b0;
    font-size: 12px;
    font-weight: 800;
}
.forum-feed-live-meta {
    color: rgba(244, 239, 232, 0.68);
    font-size: 12px;
}
.forum-feed-tabs {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
}
.forum-feed-tab {
    padding: 5px 10px;
    border-radius: 999px;
    border: 0;
    background: rgba(255, 255, 255, 0.08);
    color: rgba(244, 239, 232, 0.74);
    font-size: 12px;
    font-weight: 700;
    cursor: pointer;
}
.forum-feed-tab--active {
    background: #0bd0b0;
    color: #04231d;
}
.forum-feed-banner {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 16px;
    background: #f2e8dc;
    border-top: 1px solid rgba(0, 0, 0, 0.05);
    border-bottom: 1px solid rgba(0, 0, 0, 0.08);
    color: #d95f1f;
    font-weight: 800;
}
.forum-feed-banner small {
    color: #786c60;
    font-weight: 600;
}
.forum-feed-list {
    display: flex;
    flex-direction: column;
}
.forum-feed-card {
    display: grid;
    grid-template-columns: 72px minmax(0, 1fr);
    gap: 14px;
    padding: 18px 16px;
    border-left: 3px solid #ff7e2d;
    border-bottom: 1px solid rgba(29, 28, 34, 0.08);
    background: rgba(255, 255, 255, 0.15);
    cursor: pointer;
    transition: background 0.2s ease, transform 0.2s ease;
}
.forum-feed-card:hover {
    background: rgba(255, 255, 255, 0.36);
    transform: translateY(-1px);
}
.forum-feed-card.selected {
    background: rgba(220, 247, 241, 0.82);
}
.forum-feed-card__votes {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    color: #39353d;
}
.forum-feed-card__votes strong {
    font-size: 28px;
    line-height: 1;
}
.forum-feed-card__up,
.forum-feed-card__down {
    font-size: 13px;
    color: #c84b38;
}
.forum-feed-card__down {
    color: #8c857d;
}
.forum-feed-card__meta {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
    align-items: center;
    font-size: 12px;
}
.forum-feed-card__tag {
    color: #08b18d;
    font-weight: 800;
}
.forum-feed-card__time {
    color: #847b70;
    font-weight: 600;
}
.forum-feed-card__body h3 {
    margin: 8px 0;
    font-size: 30px;
    line-height: 1.08;
    letter-spacing: -0.02em;
    color: #2a2730;
}
.forum-feed-card__body p {
    margin: 0;
    color: #5f584e;
    line-height: 1.55;
    font-size: 15px;
}
.forum-feed-card__footer {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
    align-items: center;
    margin-top: 14px;
}
.forum-feed-card__pill {
    font-size: 12px;
    font-weight: 700;
    color: #a55324;
    background: rgba(255, 125, 44, 0.12);
    border-radius: 999px;
    padding: 6px 10px;
}
.forum-feed-side {
    display: flex;
    flex-direction: column;
    gap: 16px;
}
.forum-side-card {
    padding: 0;
    overflow: hidden;
}
.forum-side-card--activity {
    background: #f3f1ee;
}
.forum-side-card__titlebar {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 12px 14px;
    background: linear-gradient(90deg, #ff5224, #ff7f3b);
    color: #fff;
}
.forum-side-card__titlebar span {
    font-size: 11px;
    font-weight: 700;
    opacity: 0.82;
}
.forum-side-activity-list,
.forum-side-board-list {
    display: flex;
    flex-direction: column;
}
.forum-side-activity-item {
    padding: 14px;
    border-bottom: 1px solid rgba(0, 0, 0, 0.06);
}
.forum-side-activity-item strong {
    display: block;
    margin-bottom: 6px;
    color: #2f2b33;
}
.forum-side-activity-item p {
    margin: 0;
    color: #676058;
    font-size: 13px;
    line-height: 1.45;
}
.forum-side-card__subheader {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 14px;
    background: #17171d;
    color: #fff;
}
.forum-side-card__subheader a {
    color: #08cfaf;
    font-size: 12px;
    font-weight: 700;
}
.forum-side-board-item {
    display: grid;
    grid-template-columns: 36px minmax(0, 1fr);
    gap: 12px;
    align-items: start;
    padding: 14px;
    border-bottom: 1px solid rgba(0, 0, 0, 0.06);
    cursor: pointer;
    background: #fff;
}
.forum-side-board-item:hover {
    background: #f7fbfa;
}
.forum-side-board-item__icon {
    display: grid;
    place-items: center;
    width: 36px;
    height: 36px;
    border-radius: 50%;
    background: linear-gradient(180deg, #18d1aa, #099f7d);
    color: #fff;
    font-weight: 800;
}
.forum-side-board-item__body strong {
    display: block;
    color: #242128;
    margin-bottom: 4px;
}
.forum-side-board-item__body span {
    display: block;
    color: #6d665d;
    font-size: 12px;
    line-height: 1.4;
}
.forum-category {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    padding: 10px 12px;
    border-radius: var(--radius-soft);
    background: #f6f6f6;
    border: 1px solid var(--border);
    margin-bottom: 12px;
}
.forum-category__title { font-weight: 700; letter-spacing: 0.6px; }
.forum-category__meta { color: var(--muted); font-size: 12px; }
.forum-table { display: flex; flex-direction: column; gap: 10px; }
.forum-row {
    display: grid;
    grid-template-columns: minmax(0, 2.5fr) minmax(140px, 1fr) minmax(200px, 1.2fr);
    gap: 12px;
    padding: 14px;
    border-radius: var(--radius);
    border: 1px solid var(--border);
    background: #ffffff;
    cursor: pointer;
    transition: border-color 0.2s ease, box-shadow 0.2s ease, transform 0.2s ease;
}
.forum-row:hover { border-color: rgba(225,75,75,0.4); box-shadow: 0 10px 24px rgba(0,0,0,0.08); transform: translateY(-2px); }
.forum-row.selected { border-color: rgba(225,75,75,0.6); }
.forum-row--head {
    cursor: default;
    text-transform: uppercase;
    font-size: 11px;
    letter-spacing: 0.7px;
    background: #f5f5f5;
}
.forum-row--head:hover { border-color: var(--border); box-shadow: none; transform: none; }
.forum-cell--board { display: flex; flex-direction: column; gap: 6px; }
.forum-title { font-weight: 700; }
.forum-desc { color: var(--muted); font-size: 13px; }
.forum-stat { color: var(--muted); font-size: 13px; }
.forum-last__title { font-weight: 600; }
.forum-last__meta { color: var(--muted); font-size: 12px; margin-top: 4px; }
.forum-side h3 { margin-top: 0; }

label { display: block; margin-top: 6px; font-weight: 600; color: var(--accent-2); }
input, textarea {
    width: 100%;
    margin-top: 6px;
    padding: 10px 12px;
    border-radius: var(--radius-soft);
    border: 1px solid var(--border);
    background: #ffffff;
    color: var(--ink);
}
input:focus, textarea:focus { outline: 2px solid rgba(225,75,75,0.25); border-color: rgba(225,75,75,0.4); }
textarea { resize: vertical; }
.actions { display: flex; gap: 10px; flex-wrap: wrap; margin-top: 12px; }
button {
    padding: 9px 16px;
    border: 1px solid var(--accent);
    border-radius: 999px;
    background: var(--accent);
    color: #ffffff;
    font-weight: 600;
    cursor: pointer;
    letter-spacing: 0.4px;
    transition: all 0.2s ease;
}
button:hover { transform: translateY(-1px); box-shadow: 0 10px 20px rgba(225,75,75,0.2); }
.card-ghost {
    background: #ffffff;
    border: 1px dashed var(--border);
    border-radius: var(--radius);
    padding: 14px;
}
.checkbox { display: flex; align-items: center; gap: 8px; margin-top: 8px; }
.stack { display: flex; flex-direction: column; gap: 8px; }
details { margin-top: 8px; }
summary { cursor: pointer; color: var(--accent-2); font-weight: 600; }
.list { list-style: none; padding: 0; margin: 12px 0 0 0; display: flex; flex-direction: column; gap: 10px; }
.list--limit4 { max-height: 420px; overflow-y: auto; padding-right: 6px; }
.list--limit5 { max-height: 360px; overflow-y: auto; padding-right: 6px; }
.board-picker { max-height: 260px; overflow-y: auto; }
.board-picker .item { padding: 10px; }
.board-picker .ghost-btn { margin-top: 8px; font-size: 12px; padding: 6px 10px; }
.na-panel { background: linear-gradient(180deg, #fbfffe 0%, #ffffff 100%); }
.na-toolbar {
    align-items: center;
    padding: 10px 12px;
    border: 1px solid var(--border);
    border-radius: var(--radius-soft);
    background: #f4faf8;
}
.na-count {
    font-size: 12px;
    color: #1e5c52;
    background: #ffffff;
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 6px 10px;
}
.na-grid { align-items: start; }
.na-pane {
    border: 1px solid var(--border);
    border-radius: var(--radius-soft);
    background: #ffffff;
    padding: 12px;
}
.na-upload input[type="file"] {
    padding: 8px;
    border: 1px dashed var(--border);
    border-radius: 8px;
    background: #fcfcfc;
}
.na-item.unread {
    border-left: 4px solid #2b9f89;
    background: #f4fffc;
}
.pm-panel { background: linear-gradient(180deg, #fffdfb 0%, #ffffff 100%); }
.pm-toolbar {
    align-items: center;
    padding: 10px 12px;
    border: 1px solid var(--border);
    border-radius: var(--radius-soft);
    background: #faf7f4;
}
.pm-toolbar select {
    min-width: 120px;
    padding: 8px 10px;
    border-radius: 999px;
    border: 1px solid var(--border);
    background: #ffffff;
}
.pm-count {
    font-size: 12px;
    color: var(--accent-2);
    background: #fff;
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 6px 10px;
}
.pm-grid { align-items: start; }
.pm-list-panel, .pm-compose-panel {
    border: 1px solid var(--border);
    border-radius: var(--radius-soft);
    background: #ffffff;
    padding: 12px;
}
.pm-list { margin-top: 8px; }
.pm-item {
    border-left: 4px solid transparent;
    background: #fcfcfc;
}
.pm-item.unread {
    border-left-color: var(--accent);
    background: #fff7f2;
}
.pm-subject { display: block; margin-bottom: 4px; }
.pm-compose textarea { min-height: 140px; }
.item { background: #ffffff; border: 1px solid var(--border); padding: 12px; border-radius: var(--radius-soft); }
.item.selected { border-color: rgba(225,75,75,0.5); background: rgba(225,75,75,0.06); }
.meta { color: var(--muted); font-size: 13px; margin-top: 4px; }
.post-list { gap: 12px; }
.post-list .item {
    position: relative;
    padding: 14px 16px 14px 54px;
    border-radius: 10px;
    box-shadow: 0 10px 20px rgba(0,0,0,0.06);
}
.post-list .item::before {
    content: "▲";
    position: absolute;
    left: 18px;
    top: 16px;
    font-size: 12px;
    color: var(--accent);
}
.post-list .item::after {
    content: "▼";
    position: absolute;
    left: 18px;
    top: 36px;
    font-size: 12px;
    color: #9a9a9a;
}
.post-list .item strong {
    display: block;
    font-size: 15px;
    margin-bottom: 6px;
}
.post-list .item p {
    margin: 10px 0 0;
    color: #333;
    line-height: 1.5;
}
.post-list .actions {
    margin-top: 10px;
}
.post-list .ghost-btn {
    padding: 6px 12px;
    font-size: 12px;
    border-radius: 999px;
}
.topic-list .item {
    background: #fafafa;
}
.topic-list .item strong {
    display: block;
}
.post-detail {
    background: #121212;
    border-radius: var(--radius);
    padding: 20px;
    color: #f3f3f3;
    display: flex;
    flex-direction: column;
    gap: 18px;
    box-shadow: 0 18px 36px rgba(0,0,0,0.2);
}
.app-shell--detail > :not(.post-detail) {
    display: none;
}
.app-shell--detail {
    max-width: 1240px;
}
.app-shell--detail body,
body:has(.app-shell--detail) {
    background: #0b0b0b;
    color: #f3f3f3;
}
.board-header {
    display: flex;
    flex-direction: column;
    gap: 8px;
}
.board-header__eyebrow {
    margin: 0;
    color: #0bd0b0;
    font-size: 12px;
    font-weight: 800;
    letter-spacing: 0.04em;
}
.board-header h2 {
    margin: 0;
    font-size: 34px;
    line-height: 1.08;
    letter-spacing: -0.02em;
    color: #f7f4ef;
}
.topic-chips {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
    margin-top: 4px;
}
.topic-chip {
    padding: 6px 10px;
    border-radius: 999px;
    border: 1px solid #2a2a2a;
    background: #1b1b1b;
    color: #d6d6d6;
    font-size: 12px;
    cursor: pointer;
}
.topic-chip.active {
    background: var(--accent);
    border-color: var(--accent);
    color: #fff;
}
.post-detail .ghost-btn {
    align-self: flex-start;
    background: #1d1d1d;
    border-color: #2a2a2a;
    color: #f0f0f0;
}
.post-card {
    background: #1a1a1a;
    border: 1px solid #2a2a2a;
    border-radius: 12px;
    padding: 18px 20px;
}
.post-card h2 {
    margin: 8px 0 10px;
    font-size: 20px;
}
.post-card p {
    margin: 12px 0 0;
    color: #d7d7d7;
    line-height: 1.6;
}
.post-header {
    display: flex;
    align-items: center;
    gap: 8px;
    color: #bdbdbd;
    font-size: 12px;
}
.post-actions {
    margin-top: 14px;
    display: flex;
    gap: 8px;
}
.comment-title {
    margin: 0;
    font-size: 24px;
    color: #f6f2ea;
}
.comment-block {
    display: flex;
    flex-direction: column;
    gap: 14px;
}
.comment-compose {
    background: #141419;
    border: 1px solid #2c2d35;
    border-radius: 14px;
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 10px;
}
.comment-compose textarea {
    min-height: 120px;
    background: #0f1014;
    border: 1px solid #2d2f38;
    border-radius: 16px;
    color: #e9e9e9;
    padding: 14px 16px;
}
.compose-tools {
    display: flex;
    align-items: center;
    gap: 12px;
    color: #9ea2aa;
    font-size: 13px;
}
.compose-actions {
    justify-content: flex-end;
    margin-top: 4px;
}
.comment-toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
}
.comment-toolbar input {
    max-width: 220px;
    margin: 0;
    padding: 7px 10px;
    font-size: 12px;
    border-radius: 999px;
    background: #161616;
    border: 1px solid #2a2a2a;
}
.comment-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 14px;
}
.comment-card {
    display: grid;
    grid-template-columns: 42px minmax(0, 1fr);
    gap: 12px;
    background: #17181d;
    border: 1px solid #292b33;
    border-radius: 14px;
    padding: 14px 16px;
}
.comment-card.focused {
    border-color: var(--accent);
    box-shadow: 0 0 0 2px rgba(225,75,75,0.2);
}
.comment-card__avatar {
    display: grid;
    place-items: center;
    width: 42px;
    height: 42px;
    border-radius: 50%;
    background: linear-gradient(180deg, #5ad0ff, #2588d8);
    color: #fff;
    font-weight: 800;
    font-size: 16px;
}
.comment-card__content {
    min-width: 0;
}
.comment-meta {
    display: flex;
    gap: 8px;
    align-items: center;
    color: #a9a9a9;
    font-size: 12px;
}
.comment-meta--stack {
    align-items: flex-start;
    justify-content: space-between;
    flex-wrap: wrap;
}
.comment-meta__main {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
}
.comment-meta strong {
    color: #f1f1f1;
    font-size: 13px;
}
.comment-card p {
    margin: 8px 0 0;
    color: #d7dbe1;
    line-height: 1.65;
    font-size: 14px;
}
.comment-card__actions {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
    margin-top: 10px;
}
.comment-card__actions .ghost-btn {
    background: #1c1d22;
    border-color: #30323a;
}
.detail-tools {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));
    gap: 12px;
}
.detail-main {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 14px;
    align-items: start;
}
.detail-left { display: flex; flex-direction: column; gap: 12px; }
.detail-right { display: flex; flex-direction: column; gap: 12px; }
.side-card {
    background: #161616;
    border: 1px solid #2a2a2a;
    border-radius: 12px;
    padding: 12px;
}
.side-card h4 {
    margin: 0 0 4px;
    font-size: 15px;
}
.side-list {
    list-style: none;
    margin: 10px 0 0;
    padding: 0;
    display: grid;
    gap: 8px;
}
.side-link {
    width: 100%;
    text-align: left;
    border-radius: 8px;
    border: 1px solid #2a2a2a;
    background: #1d1d1d;
    color: #e8e8e8;
    padding: 8px 10px;
    font-size: 13px;
    line-height: 1.4;
}
.side-link:hover {
    border-color: #3a3a3a;
    box-shadow: none;
    transform: none;
}
.detail-panel {
    background: #1a1a1a;
    border: 1px solid #2a2a2a;
    border-radius: 10px;
    padding: 12px;
}
.detail-panel h4 {
    margin: 0 0 8px;
}
.post-detail input,
.post-detail textarea {
    background: #141414;
    border-color: #2a2a2a;
    color: #f0f0f0;
}
.hero--admin { background: var(--paper); }

@media (max-width: 900px) {
    .hero { grid-template-columns: 1fr; }
    .forum-layout { grid-template-columns: 1fr; }
    .forum-feed-layout { grid-template-columns: 1fr; }
    .forum-row { grid-template-columns: 1fr; }
    .forum-feed-card { grid-template-columns: 56px minmax(0, 1fr); }
    .forum-feed-card__body h3 { font-size: 24px; }
    .detail-main { grid-template-columns: 1fr; }
}
@media (max-width: 640px) {
    .top-nav { border-radius: 0; }
    .top-strip { flex-direction: column; align-items: flex-start; }
    .top-meta { text-align: left; }
    .nav-search { width: 100%; }
    .nav-search input { width: 100%; }
}
"#;
