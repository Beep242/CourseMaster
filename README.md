# CourseMaster

A local-first(-feeling) academic operating system: syllabus intelligence,
assignment tracking, and a spaced-scheduling engine, all in Rust, with AI
features powered by your own Claude Code CLI (no separate Anthropic API key).

Available as a Tauri desktop app and, once deployed, as a phone-reachable web
app at `coursemaster.iambeep.com` — both talk to the same hosted API and
database, gated by the same cross-app login as the rest of the Beep.dev suite
(StockMan, TruthSeeker, BPass).

## Architecture

```
crates/
  academic-core/     Models + SQLite persistence (sqlx), migrations
  scheduler/          Assignment prioritization — pure, independently tested
  document-engine/    Syllabus AI extraction + "Ask my syllabus"
  ai-engine/           AiProvider trait; ClaudeCliProvider shells out to `claude -p`
  api-server/          Axum HTTP API — the source of truth; verifies PortFolio
                        bridge JWTs, serves the built frontend as static files
desktop/src-tauri/     Thin Tauri shell — no local DB, no local AI; embeds the
                        same built frontend, which always calls the hosted API
ui/                    React + TS frontend (single build, used by both desktop
                        and the hosted web app)
```

Why one API instead of desktop being fully offline: multi-device access
(desktop + phone) was worth more than offline capability for this app — see
`DEPLOY.md` and the PortFolio bridge auth pattern in `ui/src/bridgeAuth.ts`.

## Local development

See `DEPLOY.md`'s "Local development" section.

## Testing

```bash
cargo test --workspace
```

Scheduler, syllabus-extraction parsing, and the Claude CLI's JSON-parsing
logic are all unit tested without needing a live `claude` process or database
(academic-core's repo tests use an in-memory SQLite DB instead).

## Status

Vertical slice complete: onboarding, courses, syllabus paste → AI extraction
→ human review → assignments, assignment tracking, and a basic "what should I
do right now" prioritized list. Flashcards, practice tests, the AI tutor,
analytics, and the rest of the full feature spec are not built yet — see the
original spec for the full feature list and phase plan.
