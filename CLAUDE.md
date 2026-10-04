# CourseMaster

Single-user academic operating system: courses, AI syllabus extraction with human review, D2L (Brightspace) `.ics`
calendar import, assignment prioritization, AI study guides/practice tests. Rust workspace (Axum + SQLite) + React/TS
frontend, shipped as one Docker image at `coursemaster.iambeep.com` and as a thin Tauri v2 desktop shell. All AI runs
through the user's own Claude Code CLI (`claude -p`) — no Anthropic API key anywhere.

## Layout

- `crates/academic-core` — models, sqlx/SQLite pool, `repo/*` (one module per table), `migrations/`
- `crates/scheduler` — pure prioritization scoring; deliberately no `academic-core` dependency
- `crates/document-engine` — `syllabus_extraction`, `calendar_feed` (D2L ICS), `study_guide`, `practice_test`
- `crates/ai-engine` — `AiProvider` trait + `ClaudeCliProvider` (subprocess); the only AI seam
- `crates/api-server` — Axum API (bin `coursemaster-api`), source of truth, also serves `ui/dist`
- `ui/` — React 18 + Vite + TS; one build used by both the web app and the desktop shell
- `desktop/src-tauri` — thin Tauri shell, no local DB, no local AI, no custom commands

## Commands

```bash
cargo test --workspace                 # README; 32 unit tests, no live claude/DB needed
cargo build --release -p api-server    # what the Dockerfile builds
cd ui && npm run dev                   # vite, port 1430 (strictPort), uses ui/.env.development
cd ui && npm run build                 # tsc -b && vite build -> ui/dist
cargo tauri dev | cargo tauri build    # from desktop/src-tauri; runs the ui npm script itself
```

Local API (DEPLOY.md "Local development"), from `crates/api-server`: `CROSS_APP_JWT_SECRET=... OWNER_EMAIL=...
PORT=8080 STATIC_DIR=../../ui/dist WEB_ALLOWED_ORIGINS=http://localhost:1430 cargo run`. No linter or formatter
is configured (no rustfmt.toml, clippy.toml, or eslint config).

## Architecture notes

- The hosted API is the source of truth: browser and desktop load the *same* bundle and both call it over HTTPS
  — offline desktop was traded away for multi-device access (README).
- `ui/src/api.ts` is a compatibility shim: page components still call Tauri-style `invoke("command_name", {...})`,
  and `resolveRequest`'s switch maps each name to an HTTP method + path. **Adding an endpoint means editing two
  places**: the router in `crates/api-server/src/main.rs` and that switch.
- **No auth middleware.** The router carries no auth layer — each of the 39 handlers takes the `AuthUser`
  extractor (`crates/api-server/src/auth.rs`) as an argument, so a new handler that omits it is public.
  `AuthUser` verifies the HS256 bridge JWT, then 403s any email != `OWNER_EMAIL`.
- Auth: no passwords here. `ui/src/bridgeAuth.ts` POSTs PortFolio's `/api/auth/token?app=coursemaster` (**not**
  `/auth/token` — Auth.js claims that prefix) to trade a session cookie for that JWT; a 401 retries once.
- `ClaudeCliProvider` spawns `claude -p --output-format json` from `AI_SCRATCH_DIR` with `--tools ""`,
  `--strict-mcp-config`, `--setting-sources ""`, a 180s timeout and a $0.50 budget cap, so no stray CLAUDE.md can
  influence it. System prompt and JSON schema go into **stdin, not argv** (cmd.exe re-quoting mangles schema JSON
  on Windows); it also retries `claude.cmd`/`claude.exe` (CreateProcessW does no PATHEXT search).
- Syllabus paste and D2L sync both land in `syllabus_extractions` as **pending**, and
  `repo::syllabus::approve_extraction` is the only code that inserts an `assignments` row — it refuses a
  non-pending extraction or one with no resolvable course. Nothing becomes an assignment without approval.
- D2L import: course identity is the `ou=` org-unit id scraped from the ICS `DESCRIPTION` link, never the UID
  prefix (constant per subscription) or title text; stored on `courses.external_org_unit_id`. Sync tags items
  confidence 0.98 (org-unit) / 0.9 (name guess) / 0.6 (no course), and linking a course afterwards backfills
  that org unit's pending extractions.
- D2L emits one VEVENT per *state change*, suffixing SUMMARY: `Quiz 1 - Available` (opens),
  `Quiz 1 - Availability Ends` (closes), `Quiz 1 - Due`. `calendar_feed::collapse_to_deadlines` drops
  `Available` (an opening, not a deadline) and undated events, then keeps one event per
  `canonical_key(org_unit, suffix-stripped title)`, preferring `Due` > `Availability Ends` > unsuffixed and
  the earlier deadline on a tie. A live 107-event SRU feed collapses to 86 deadlines across 7 courses.
  Stored titles keep no suffix; the raw SUMMARY is preserved in `source_excerpt` for provenance.
- Feed dedup is therefore **two keys**, not one: exact `external_uid` (the same event seen again) *and*
  `canonical_key` (a sibling state-change event of an item already imported — each sibling has its own UID,
  so UID alone re-imports them forever). `repo::syllabus::known_feed_items` returns title + org unit so the
  key is derived identically for stored rows, which is what keeps pre-collapse rows (titles still ending
  ` - Due`) recognised.
- Every D2L timestamp is UTC (`...Z`), so the wall clock must be converted before the date is taken —
  `20260828T035900Z` is 11:59 PM on Aug **27** in Eastern time, and reading it raw filed late-evening work a
  day late. `configured_timezone()` reads `LOCAL_TIMEZONE` (default `America/New_York`); a value without a
  trailing `Z` is a floating local wall clock and is taken as written.
- `ical` 0.11 unfolds continuation lines (space *and* tab) but does **no** RFC 5545 text unescaping, so
  SUMMARY/DESCRIPTION/LOCATION go through `unescape_ics_text` — otherwise `General Chem I (02\, 934)` becomes
  a course name containing literal backslashes.
- Study tools: guides use `AiProvider::complete` (Markdown); practice tests use `extract_structured` + a JSON schema, and an attempt is one `answers_json` blob with short answers model-graded.

## Conventions & gotchas

- Migrations are append-only numbered files in `crates/academic-core/migrations`, embedded by `sqlx::migrate!` at
  **compile time** and run by `db::connect` — a new file needs a rebuild, not just a restart; never edit an applied one.
- `ui/dist` is gitignored but is what both the Docker image and Tauri (`frontendDist: ../../ui/dist`) serve;
  `cargo tauri dev|build` invoke the ui npm script themselves via `beforeDevCommand`/`beforeBuildCommand`.
- The Dockerfile builds `-p api-server` and stubs `desktop/src-tauri`'s sources; keep server logic out of the desktop
  crate. `VITE_PORTFOLIO_URL` / `VITE_API_BASE_URL` are baked in at build time (Dockerfile build-args) and must be
  absolute — the same bundle runs inside Tauri, which has no origin to be relative to.
- Required env vars (names only, values live in an uncommitted `.env`): `CROSS_APP_JWT_SECRET` (must byte-match
  PortFolio's), `OWNER_EMAIL`, `CLAUDE_CODE_OAUTH_TOKEN` in prod. Optional: `CROSS_APP_JWT_ISSUER`, `CROSS_APP_JWT_AUDIENCE`,
  `WEB_ALLOWED_ORIGINS` (an entry that fails to parse is dropped silently), `CLAUDE_MODEL`, `PORT`, `SQLITE_PATH`, `AI_SCRATCH_DIR`, `STATIC_DIR`,
  `LOCAL_TIMEZONE` (IANA name used to turn a feed's UTC deadlines into local dates; default `America/New_York`,
  and an unparseable value silently falls back to that rather than failing a sync).
- Do not rebind the prod container to `127.0.0.1:8080` — Caddy reaches it via `host.docker.internal` from the
  bridge network; a loopback-only bind causes a silent 502.
- Pushing to `main` triggers `build-and-push.yml` (GHCR push + SSH restart on the VPS): a push to main is a
  production deploy. `data/coursemaster.db` is local dev only (`*.db` is gitignored).
