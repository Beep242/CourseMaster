# FEATURES_PLAN.md — Gizmo parity for CourseMaster

Status: **plan only, awaiting approval. No feature code written.**

Target: match Gizmo (AI flashcard/quiz study app) *adapted to* CourseMaster's architecture — not bolted
onto it. 98 individual capabilities were mapped across Gizmo's 10 feature areas: **2 already present,
9 partial, 87 missing.**

---

## 1. Scope decision

This app is for **one student's personal learning**. Per that decision, the following are **out of
scope entirely** — not deferred, not phase 2:

- Leaderboards of every kind (friends, global, class) — Gizmo area 5
- All of Gizmo area 6's social layer: friends list, challenge a friend, live Kahoot-style
  multiplayer rooms joined by code

Consequently this plan contains **no** users table, **no** `user_id` migration, **no** replacement of
the `OWNER_EMAIL` gate, and **no** WebSocket/SSE layer. The single-owner architecture is treated as a
fixed constraint rather than a problem to solve. That removes 8 of the 98 capabilities outright and
downgrades another 4 ("multi-user foundation steps 1–4") to never.

**Kept** from area 5: XP, levels, daily streaks with freeze, hearts, achievements — all derived from
the student's own review history. **Reshaped** from area 6: "beat your own best on a deck" replaces
"challenge a friend"; a public read-only share-by-link survives as optional.

---

## 2. What CourseMaster already has

| Subsystem | Reality |
|---|---|
| `crates/academic-core` | sqlx/SQLite pool, models, one `repo/*` module per table, 4 append-only migrations. `user_profile` is `CHECK (id = 1)` — single row, single user. No `users` table, no `user_id` on any row. |
| `crates/scheduler` | Pure assignment prioritization scoring. No `academic-core` dependency, unit-tested in isolation. **This is the precedent to copy** for any new pure-logic crate. |
| `crates/document-engine` | Syllabus extraction, D2L ICS calendar import, study-guide and practice-test generation. Deps: `reqwest`, `ical`, `chrono`, `chrono-tz`. |
| `crates/ai-engine` | `AiProvider` trait + `ClaudeCliProvider`. The only AI seam. |
| `crates/api-server` | Axum 0.8, 39 handlers, also serves `ui/dist` as static files. |
| `desktop/src-tauri` | Thin shell, no custom commands, no local DB, no local AI. |
| Frontend | React 18 + Vite + TS. Runtime dependencies are **exactly `react` and `react-dom`**. No component library, no router (a `View` union + `setView`), no state manager, no icon package, no test runner, no eslint. |
| Study tools today | `study_guides` (one Markdown blob), `practice_tests` → `practice_questions` → `practice_attempts` (one `answers_json` blob per attempt). |

**What that means for Gizmo parity.** There are **no flashcards, no decks, no per-card state and no
spaced repetition of any kind**. `practice_questions` is not a usable card table: it is owned by one
`practice_tests` row via cascade so a card cannot be moved or shared, and it has no `created_at`,
`updated_at`, tags, media or provenance columns. `practice_attempts` stores one JSON blob with no
`question_id` column anywhere, so it cannot express per-card history. Decks and cards need new tables;
`practice_tests` stays the graded-mock-exam feature it already is.

**The AI seam is the binding constraint.** All AI runs by spawning `claude -p --tools ""` — so the
model cannot fetch a URL, read a file, or accept an image, and there is no vision path at all. Every
ingestion source must be extracted to text **in Rust** first, then handed to Claude. Two methods
exist: `complete()` → freeform text, `extract_structured()` → JSON against a caller-supplied schema.

---

## 3. Pre-existing defects that gate this work

Each of these was verified by reading the source in this repo, not assumed. All four get worse as
features pile on, so they are fixed first.

| # | Defect | Evidence |
|---|---|---|
| 1 | **UTF-8 panic kills the whole site.** `truncate` slices `&s[..max]` on a byte boundary. It is reached only on the malformed-JSON error path — exactly where non-ASCII math is most likely. `panic = "abort"` is set, `tower-http` is limited to `cors/fs/trace` so no `CatchPanicLayer` is possible, and the same binary serves the frontend. | `crates/ai-engine/src/claude_cli.rs:197` |
| 2 | **A large prompt can hang the server forever.** `stdin.write_all(...).await?` sits *outside* the `timeout(...)` that wraps `child.wait_with_output()`, and stdout is not drained while stdin is written. A prompt past the pipe buffer blocks with no timeout — directly on the path every document import takes. | `claude_cli.rs:160` vs `:163` |
| 3 | **A bridge token omitting `iss`/`aud` is accepted.** `auth.rs` calls `set_issuer`/`set_audience`, but `BridgeClaims` declares neither field, and jsonwebtoken 9.3.1 only enforces `required_spec_claims` (default: `exp` alone). The suite secret is shared with PortFolio, StockMan, TruthSeeker and BPass, so a token minted for another app that omits `aud` passes here. One-line fix. | `crates/api-server/src/auth.rs:17-23,57-61` |
| 4 | **Radio buttons are broken, on desktop too.** `input, textarea, select` sets `width:100%` plus padding and a border. There is an override for `input[type="checkbox"]` and **none** for `radio`, so the radios already shipped in the practice-test UI render as full-width padded boxes. Every multiple-choice and true/false mode would clone this. | `ui/src/index.css:268-298` vs `ui/src/pages/Study.tsx:264,278` |

Also verified: **no `DefaultBodyLimit` anywhere**, so axum's 2 MiB default already silently caps
*pasted* text; and production runs `journal_mode=delete` with a 5-connection pool, meaning a writer
blocks readers.

**And there is no backup of any kind in this repo and no working rollback.** Once a migration applies,
the older image boot-panics: sqlx's `validate_applied_migrations` returns `VersionMissing` for any
applied version the older binary lacks, `ignore_missing=false`, no override in `db.rs`. Since that
binary also serves the frontend, a bad migration takes the entire site down, and a push to `main` *is*
a production deploy with no health check. That is why increment 1 is a backup script, not a feature.

---

## 4. Gap analysis

`[status / effort / value]` — value judged for one student with 7 real courses.

### Area 1 — AI content import (20 capabilities)
```
[missing/M/critical] AI card generation from pasted notes — the core generator everything else feeds
[missing/L/critical] review / edit / delete generated cards before saving — the human-approval gate
[missing/M/critical] the review screen itself
[missing/M/critical] file upload transport — without it no PDF/PPTX/DOCX/image/apkg import can exist
[missing/M/critical] input size guard + chunking, and fixing the stdin deadlock (defect 2)
[missing/M/critical] generate cards from a PDF with a real text layer
[missing/M/high]     generate cards from DOCX
[missing/M/high]     generate cards from PPTX
[missing/S/high]     import a Quizlet deck by paste/export
[missing/S/high]     idempotent re-import (re-pasting a corrected set must not double the deck)
[missing/M/high]     move generation off the request thread
[missing/S/medium]   import an Anki CSV/TXT export
[missing/M/medium]   generate cards from a web page URL
[missing/S/medium]   record what each generation cost
[missing/L/low]      OCR from an image or screenshot
[missing/XL/low]     scanned / image-only PDF
[missing/L/low]      YouTube / video transcript
[missing/XL/low]     uploaded video or audio
[missing/L/low]      Anki .apkg archive
[missing/L/low]      Anki media images
```

### Area 2 — Spaced repetition (6)
```
[missing/S/critical] SM-2 itself: pure fn review(state, rating, today) -> state, no DB/HTTP/AI
[missing/M/critical] per-card schedule state + a normalized per-review log, written atomically
[missing/S/critical] submit one review, returning the new schedule
[missing/M/critical] "Due today" queue spanning ALL courses and decks, plus a top-level Review page
[missing/S/high]     queue shaping for 7 courses: new/day cap, total/day cap, course interleaving
[missing/S/low]      suspend/bury, and leech flagging after N lapses
```

### Area 3 — Study modes (11)
```
[partial/S/high]     fix the broken radio/toggle styling every MC and T/F mode would inherit (defect 4)
[missing/M/critical] classic flip session: one card, flip to reveal, grade buttons, keyboard + swipe
[missing/M/critical] deterministic fuzzy matcher: Correct/Incorrect/Undecided with NO AI call
[partial/S/high]     refactor the existing practice-test grader onto that crate — one implementation
[missing/M/high]     typed-answer mode, with self-grade fallback for Undecided
[partial/M/high]     multiple-choice mode, distractors PRE-GENERATED at save time, never live
[partial/S/medium]   optional AI adjudication for Undecided only, with per-session/per-day caps
[missing/S/medium]   true/false mode
[missing/S/medium]   mixed mode interleaving per card based on what each card supports
[missing/M/medium]   LaTeX math rendering
[missing/L/low]      images on cards
```

### Area 4 — AI tutor (5)
```
[missing/S/critical] card provenance: verbatim source excerpt + explanation + accepted answers
[missing/M/critical] AI seam guardrails: per-call timeout/budget, max_input_chars, persisted cost
[partial/M/high]     "Explain" on a wrong answer, grounded in that card's own excerpt, cached
[missing/M/high]     deterministic grounding selection (no vector store, no embedding model)
[partial/L/medium]   multi-turn chat with a deck/course
```

### Area 5 — Gamification (15, of which 6 are out of scope)
```
[missing/S/critical] day-boundary contract: client-supplied local_date + a timezone on user_profile
[missing/S/critical] daily streak, derived from distinct local dates in the review log
[missing/S/high]     XP per review, derived from the log — never stored as a counter
[missing/M/high]     streak freeze: offered on read, spent on confirm, no cron
[missing/S/high]     personal history — the single-user replacement for a leaderboard
[missing/M/high]     gamification chips on the existing Dashboard
[missing/S/medium]   levels with progress to next
[missing/M/medium]   achievements, evaluated from the log on read
[missing/S/low]      hearts per session (opt-in, non-blocking)
OUT OF SCOPE: friends leaderboard, global/class leaderboard, multi-user foundation steps 1-4
```

### Area 6 — Social (8, of which 5 are out of scope)
```
[have/S/low]         PortFolio already issues bridge tokens for non-owner accounts
[missing/S/medium]   beat your own best on a deck — the single-user "challenge a friend"
[missing/M/medium]   share a deck by link: public, read-only, no accounts
[missing/M/low]      guest scores on a shared deck via typed nicknames
OUT OF SCOPE: friends list, challenge a friend, realtime transport, live quiz rooms
```

### Area 7 — Organization (7)
```
[missing/M/critical] decks: named, mutable, course-scoped container (course_id nullable)
[missing/M/critical] cards: front/back, distractors, explanation, full CRUD so generated cards are editable
[missing/M/critical] deck + card browsing UI
[missing/S/high]     tags on cards, and filtering by tag
[missing/S/high]     search across all cards in all decks and courses
[partial/S/medium]   move a deck between courses, rename/recolour, edit the parent course
[missing/L/low]      nested folders
```

### Area 8 — Progress & analytics (10)
```
[missing/M/critical] per-review event log — one row per graded answer, not one blob per attempt
[missing/S/critical] atomic review write so schedule and log can never disagree
[missing/S/critical] weak-card list, ranked, per course or across all
[missing/S/high]     accuracy history — per-day correct/total time series
[missing/M/high]     mastery % per deck, defined forward-looking rather than as lifetime accuracy
[missing/L/high]     "exam ready" score: explicit weighted formula + human-readable reason string
[missing/M/high]     analytics page composing all of the above
[missing/S/medium]   study time, client-reported and server-clamped
[partial/S/medium]   one-time backfill of practice_attempts into the review log
[missing/S/low]      per-topic mastery as distinct from per-deck
```

### Area 9 — Reminders (6)
```
[partial/S/high]     exam-date countdowns, computed on read — no scheduler, no state
[missing/M/high]     exam cram bias: approaching exam surfaces that course sooner, without mutating due dates
[missing/S/high]     daily study reminder, in-app, computed on open
[missing/M/low]      in-process tokio interval task
[missing/L/low]      email reminders
[missing/XL/low]     web push
```

### Area 10 — Cross-device (10)
```
[have/S/high]        progress already syncs — the hosted API is the single source of truth
[missing/M/critical] phone layout: bottom tab bar below 560px (today the 76px sidebar rail is permanent)
[missing/S/critical] fix .tabs overflow — the main nav of the most-used screen, no overflow-x, no wrap
[missing/S/high]     fix radio controls (defect 4)
[missing/S/high]     44px minimum touch targets
[missing/S/medium]   100vh -> 100dvh so mobile browser chrome stops clipping
[missing/S/medium]   fix table overflow on the Study screen
[missing/S/medium]   installable to the home screen (manifest + icons)
[missing/S/low]      .field-grid row gap — today `gap: 0 1rem`, so stacked fields touch on a phone
[missing/S/low]      prefers-reduced-motion guard
```

Verified: the only layout media queries are `860px` (sidebar → 76px rail) and `560px` (topbar stacks
only). There is no phone layout.

---

## 5. Implementation order

24 increments. Every one leaves the app building, tested and deployable — a push to `main` is a
production deploy, so a half-finished increment on `main` is a broken production site.

**Sequencing principle:** pure logic crates come *before* the migrations that store their output, so
column shapes are transcribed from working code rather than guessed. SQLite cannot retype a column,
and the only workaround is the table-rebuild pattern, which under `foreign_keys(true)` performs an
implicit DELETE that fires child cascades.

| # | Increment | Effort | Deps |
|---|---|---|---|
| 1 | Production safety net: DB snapshot/restore scripts, deploy runbook, JWT claim hardening (defect 3) | S | — |
| 2 | AI seam hardening: stdin deadlock (defect 2), UTF-8 panic (defect 1), size/timeout/budget knobs, cost plumbing | M | — |
| 3 | `crates/srs` — SM-2 as a pure, exactly-asserted function | M | — |
| 4 | `crates/grading` — deterministic answer matching; refactor the existing grader onto it | M | — |
| 5 | Mobile layout + broken form controls (defect 4) | S | — |
| 6 | Migration 0005: decks + cards + deck_sources, WAL, repo layer, HTTP API | M | 1,3,4 |
| 7 | Deck/card UI: browse, create, edit, search | M | 5,6 |
| **8** | **MILESTONE — flip-card study session.** You can actually study. | M | 7 |
| 9 | Migration 0006: import staging (`card_imports` + `card_candidates`) | S | 1,6 |
| **10** | **MILESTONE — AI card generation from pasted notes + review/approve screen.** | L | 2,8,9 |
| 11 | Migration 0007: `card_schedule` + `card_reviews`, atomic review write | M | 1,3,6 |
| 12 | Review API: submit a review, due-today queue across all 7 courses, session caps | M | 11 |
| 13 | Grade buttons in the session, suspend + leech flagging | M | 8,12 |
| 14 | Quizlet paste and Anki CSV import, idempotent re-import | S | 9,10 |
| 15 | **Ingestion spike:** PDF/DOCX/PPTX text extraction, wired to nothing | L | — |
| 16 | File upload transport; wire the document extractors | M | 2,10,15 |
| 17 | Typed-answer study mode | S | 4,13 |
| 18 | Multiple-choice, true/false and mixed modes | M | 10,17 |
| 19 | "Explain" on a wrong answer, cached per normalised mistake | M | 1,2,4,13 |
| 20 | Gamification: XP, levels, streaks with freeze, achievements, opt-in hearts | L | 1,11 |
| 21 | Analytics endpoints: weak cards, accuracy history, time on cards | M | 11 |
| 22 | `crates/examready`: mastery + explainable exam-ready score, Progress page | M | 11,21 |
| 23 | Exam countdowns, cram bias, in-app daily nudge | S | 12,21 |
| 24 | Installable web app (PWA manifest + icons) | S | 5 |

**Why 8 lands before spaced repetition:** "paste my notes and study tonight" needs cards and a
flip-through, not scheduling. Putting the flip session behind SRS, five study modes and every document
importer would mean ~19 increments of unvalidated work before you ever touch a card.

**Why 15 is wired to nothing:** it writes the PDF/DOCX/PPTX extractors as pure functions with fixtures
and a dump test over your own lecture handouts, with no endpoint, no table and no UI. If `pdf-extract`
interleaves a two-column handout into text that would yield plausible-looking *wrong* flashcards, that
costs one module to find out — instead of being discovered after upload transport, staging tables and a
review screen were all built assuming it works.

Tests land in the same increment as the code they test: SM-2 in 3, the grading ladder in 4, the import
parsers in 14/15. All of them live in `serde`/`chrono`-only crates, because `build-and-push.yml` never
runs `cargo test` — `tsc -b` is the only automated gate in the pipeline, so **run `cargo test
--workspace` by hand before every push**, and logic written in TypeScript is logic this project cannot
test at all.

---

## 6. Decisions taken as defaults

Taken rather than asked, to avoid padding the question list. Each is reversible cheaply except where noted.

- **SM-2, not FSRS.** FSRS's advantage comes from optimising 17–21 weights against your own review
  history; on day one there is none, so it would run on defaults where it is not measurably better for
  one user — and the optimiser means pulling an ML stack into a workspace that recompiles under
  `lto=true, codegen-units=1` on every commit. SM-2 is a pure total function whose transitions are
  exact-equality assertions, matching `crates/scheduler`'s existing test style.
- **Courses → decks → cards**, `decks.course_id` nullable (so a flat course→cards model is the
  degenerate case). Nested folders deferred.
- **`cards.source_excerpt` nullable** — a hand-written card has no source, and SQLite cannot relax a
  `NOT NULL` later.
- **Upload is base64-in-JSON at a 25 MiB `DefaultBodyLimit`.** Multipart needs an axum feature plus
  `multer` *and* a second fetch path, because `ui/src/api.ts` hardcodes `application/json` +
  `JSON.stringify`. The body-limit layer is mandatory either way.
- **Five per-feature migrations (0005–0009), not one combined file** — each additive-only and
  independently verifiable, matching the one-feature-per-commit requirement.
- **Write-per-answer with `journal_mode=WAL`** (production is currently rollback-journal, where a
  writer blocks readers). Batching at session end loses everything on a tab close.
- **LaTeX stage 1 only:** store verbatim, prompt the generator to prefer Unicode math. Images
  descoped with a nullable `image_data_uri` column reserved, so enabling either later needs no
  migration against `cards`.
- **Hearts opt-in and non-blocking.** Being locked out of your own cards the night before an exam
  makes the app worse at its only job.
- **A configurable daily AI spend ceiling, warn-and-continue.**

---

## 7. Deferred, with reasons

**Needs a native system library or model weights** — each would mean editing the Docker runtime stage:
- *OCR of images/screenshots* — tesseract (~+100 MB on an image the VPS re-pulls every deploy), or
  `ocrs`+rten (pure Rust but ~15 MB of weights and worse on dense text), or tesseract.js (~2 MB wasm,
  breaking the two-npm-dependency invariant). macOS and Windows both do system-wide live text
  recognition, so "select the text and paste it" is nearly free.
- *Scanned/image-only PDFs* — needs pdfium or mupdf **plus** the OCR path. Two native deps stacked.
- *Uploaded video/audio transcription* — ffmpeg plus either whisper weights on a shared VPS or a paid
  STT API, which contradicts the no-API-key premise the whole AI seam is built on.
- *YouTube transcripts* — no keyless official captions API; the watch-page scrape breaks whenever
  YouTube changes shape, and YouTube bot-checks datacenter IPs, which is exactly what this VPS is. It
  would work on your laptop and fail from the server.

**Deferred for a guard, not for crates:**
- *Web-page import by URL* — `reqwest` is already a dependency and already fetches a user-supplied URL.
  But this container can reach `host.docker.internal:8080` and the shared Caddy bridge network fronting
  PortFolio and TruthSeeker, so it is SSRF against your own services. It needs a scheme-and-resolved-IP
  guard, which should also be retrofitted onto the currently-unguarded ICS fetch.

**Deferred on cost/benefit:**
- *Anki `.apkg`* — the highest-effort parser against the lowest certainty of need: zip + zstd, plus a
  new read-only non-migrating SQLite connection path (today's `db.rs` unconditionally runs
  `sqlx::migrate!`, which would try to migrate your Anki file), plus notetype schema variants and
  zip-slip guards. Increment 14's CSV path covers the common case.
- *KaTeX* — would be the **first** addition to `ui/package.json`'s deliberate two-dependency list.
  Against the current measured baseline — `vite build` reports 57.98 kB gzip JS and 4.29 kB gzip CSS —
  it is roughly +90 KB gzip JS, +23 KB CSS and the app's first web fonts, i.e. **larger than the entire
  existing application**. (The KaTeX figures are from knowledge, not measured here.) Server-side MathML is not the
  cheaper escape: it needs `dangerouslySetInnerHTML` on model output, trading bytes for an XSS surface
  in an app that currently has none.
- *Card images / Anki media* — no BLOB or file-path column anywhere, and `/app/data` is a volume but is
  not served. Would also be a new content-sniffing surface.
- *Nested folders* — the schema's first self-referential FK, a cycle guard on every move (an infinite
  subtree loop under `panic="abort"` kills the container), and a tree UI for which `App.css` has zero
  precedent. Courses → decks plus tags already does the thing folders cannot: group across courses.
- *Deck/course chat* — needs prompt **replay** every turn, because the provider passes
  `--no-session-persistence` and the request type has no messages array. Cost is O(turns²) and a
  10-turn conversation is 10 sequential process spawns at up to 180s each, with no streaming.
- *FTS5 search* — verified **available** (the sqlx `sqlite` feature resolves to bundled SQLite built
  with FTS5), so not a dead end. But it needs either the schema's first triggers or three extra writes
  per card mutation to beat a `LIKE` scan that is imperceptible over low-thousands of rows.
- *Email reminders* — Hetzner blocks outbound port 25, so this needs an SMTP relay account you must
  create, plus `lettre`, plus four env vars that require a manual SSH session (the deploy job only runs
  `docker compose pull && up -d`). A daily email about an app you open daily is thin return.
- *Web push* — **permanently unavailable on one of your two clients**: there is no service-worker push
  in WRY's WebView2 or WebKitGTK, so the Tauri desktop app can never receive one. Increment 24's
  installable manifest is the honest substitute.
- *A background cron/interval task* — nothing in scope needs one: countdowns and the nudge are computed
  on read. It would also need a persisted `last_run_at`, since the container is recreated on every deploy.
- *Backfilling `practice_attempts` into the review log* — the data genuinely exists, and it belongs in an
  idempotent admin **endpoint**, never a migration. Deferred because it produces `card_id`-NULL rows
  every analytics query must then handle deliberately, for a one-week head start on non-empty charts.

---

## 8. Open questions — both need your answer before increment 6

**Q1. Have you run the snapshot script against the production volume, and is WAL OK?**
This is the only hard gate on increment 6 and it is genuinely irreversible. Once migration 0005
applies you cannot roll the image back — the older binary boot-panics and takes the frontend with it.
Increment 1 gives you the script and runbook; I need confirmation you actually ran it before 0005
ships. Separately, 0005 enables WAL, which changes the on-disk representation of your only copy of the
data (`-wal`/`-shm` files appear beside `coursemaster.db`).
**Recommendation: yes to both — run the backup, enable WAL in the same commit as 0005.**

**Q2. Keep bundling `ui/dist` in the Tauri shell, or point the window at the hosted origin?**
`tauri.conf.json` sets `frontendDist: "../../ui/dist"` with **no** remote `url`, so the installed
desktop app serves a frozen snapshot of the frontend while the API moves on. Since adding an endpoint
is a three-place edit (router, `api.ts`'s `resolveRequest`, `types.ts`), the desktop copy silently
misses the last two and throws `Unknown command` at runtime — and nothing catches it, because CI has no
`cargo tauri build`. By increment 20 an installed desktop app would be badly broken.
- **(a)** Add a remote `url` pointing at `https://coursemaster.iambeep.com` — one line, eliminates the
  whole skew class, and makes the "same bundle over HTTPS" architecture note actually true. Costs: the
  app needs network to open (already effectively true — no local DB, no local AI), and it moves from
  `csp: null` to whatever CSP the shared Caddy sends. Needs a local `cargo tauri build` to validate.
- **(b)** Keep bundling, and treat the desktop shell as requiring a manual rebuild-and-reinstall after
  every endpoint change.

**Recommendation: (a), as a standalone commit alongside increment 5, before the endpoint count climbs.**

---

## 9. Progress log

### ✅ Increment 1 — Production safety net + JWT hardening

- `scripts/backup-db.sh` — `sqlite3 .backup` (not a file copy, which races with
  in-flight writes and would miss the `-wal` sidecar), verifies `integrity_check`,
  prints a row summary, pulls a copy to `./backups`. Installs `sqlite3` into the
  container on demand, since the runtime image has none and an install there does
  not survive a recreate.
- `scripts/restore-db.sh` — verifies the snapshot *before* touching anything,
  snapshots the database it is about to replace, then stops the container and
  swaps the file **on the host** at the path resolved from `docker inspect`.
- `DEPLOY.md` §7 — snapshot docs plus a pre-migration runbook, including the fact
  that an image rollback cannot undo a migration (`VersionMissing` boot-panic
  takes the frontend down with the API).
- `auth.rs` — `iss`/`aud` added to `BridgeClaims` as required `String`s and
  `set_required_spec_claims(["exp","iss","aud"])`. Verified against PortFolio's
  `crossAppToken.js` that `aud` is minted as a single string, so a sequence type
  would have 401'd every real token. **11 new tests.**

**Both scripts were exercised end to end, not just written.** The restore ran
against a throwaway container seeded with wrong data and stale sidecars
(`courses=0` → `courses=5`), which caught two real bugs in my own first draft:
the `-wal`/`-shm` cleanup used `docker exec` on a *stopped* container so it
silently never ran, and snapshot verification was skipped entirely because the VPS
host has no `sqlite3`. Production was never stopped (`Up 4 weeks` throughout).

A test also caught that `Validation` carries a **60s default `leeway`**, so a
token a few seconds past `exp` is still valid. That is correct (it absorbs clock
skew against PortFolio, whose bridge TTL is 900s) and is now pinned by a test.

**This answers Q1's first half:** a verified snapshot exists at
`/opt/coursemaster/backups/coursemaster-20261004-022139.db` (and locally),
`courses=5 assignments=1 extractions=1 feeds=1 schema_version=4`. The WAL half of
Q1 is still open, and is only needed at increment 6.

### ✅ Increment 2 — AI seam hardening

- **Deadlock fixed.** The subprocess call now writes stdin, drains stdout *and*
  stderr, and waits for exit concurrently inside one `timeout`, and kills the
  child if it elapses. Added `stdin.shutdown()` — closing the pipe is what
  signals end-of-prompt. Also returns `AiError::Io` rather than swallowing a
  write failure.
- **Panic fixed.** `truncate` steps back to a char boundary. Pinned by a test
  that sweeps 60 byte-limits over a string of maths symbols, emoji and accents
  and asserts it never panics and always returns valid UTF-8.
- **Knobs:** `with_timeout_secs`, `with_max_budget_usd`, `with_max_input_chars`
  on the provider; `AI_TIMEOUT_SECS`, `AI_MAX_BUDGET_USD`, `AI_MAX_INPUT_CHARS`
  wired in `main.rs` via a new `env_parsed` that warns and falls back rather
  than refusing to boot. Nonsense values (0, negative) fall back to defaults.
- **Size guard:** `AI_MAX_INPUT_CHARS` (default 240k ≈ 60k tokens) is checked on
  the *composed* prompt — system prompt and schema included — **before** the
  spawn, so an oversized input fails in microseconds with an actionable message
  instead of after 180s.
- **Cost plumbing:** `extract_structured` now returns
  `ExtractionResponse { value, total_cost_usd }`. Updated the 3 call sites.
- **Also:** an explicit `DefaultBodyLimit` (`MAX_REQUEST_BYTES`, default 8 MiB)
  on the API routes. Not strictly the AI seam, but axum's invisible 2 MiB
  default was the *first* thing truncating a large paste, so it is the same
  user-visible failure.

**66 tests pass** (was 48 before this session). Two test expectations of mine
were wrong and the code was right: `Ω` is 2 bytes not 3, and jsonwebtoken's
`leeway` is 60s.

### ✅ Increment 3 — `crates/srs` (SM-2)

New crate with `serde` + `chrono` only, mirroring `crates/scheduler`'s
precedent: no `academic-core`, no DB, no HTTP, no AI. Written before the
migration that will store it, so `card_schedule`'s columns get transcribed from
`CardState` rather than guessed.

`CardState { repetitions, interval_days, ease_factor, due_date, lapses }`,
`Rating { Again, Hard, Good, Easy }`, `review()`, `project_interval_days()`,
`is_due()`, `is_new()`, `is_leech()`. Every tunable is a named public constant.

Two documented departures from the 1987 paper:
- **Ease deltas** are the gentler per-rating values rather than the paper's
  q-derived ones. The paper's −0.80 for a lapse collapses a card from 2.5 to the
  1.3 floor in two slips, and a card pinned at the floor stops being
  distinguishable from one never seen.
- **`Hard` gets its own 1.2× multiplier.** In the paper Hard and Good both
  schedule at `interval × ease`, so pressing Hard shows the card again on
  exactly the same day as Good — not what the button means. A test asserts
  `hard < good` directly.

**16 tests, all exact-equality.** Two are adversarial sweeps: one asserts
`project_interval_days` (what the grade buttons will show) never disagrees with
what `review` applies, across 600 state/rating combinations — a separate preview
implementation is exactly the kind of thing that drifts unnoticed. The other
sweeps `u32::MAX`, 0, negative, `f64::MAX`, `NaN` and both infinities and asserts
nothing panics, no interval is 0, and no reviewed card is still due today.

**That second sweep found a real bug in this crate:** `f64::clamp` passes a NaN
*value* straight through (it only panics on NaN *bounds*), so a corrupt
`ease_factor` would have been written back and poisoned every later review of
that card — and since `f64::NAN as u32` saturates to 0, the interval would have
been clamped up from zero and the card silently rescheduled as brand new.
`clamp_ease` now resets a non-finite ease to the default.

### ✅ Increment 4 — `crates/grading`, and the existing grader refactored onto it

New pure crate (`serde` only). `Verdict { Correct, Incorrect, Undecided }` plus
`normalize`, `levenshtein`, `typo_budget`, `parse_numeric`, `grade_short_answer`,
`grade_choice`, `grade_true_false`, `parse_boolean`.

Grading is a ladder, cheapest rung first: normalise and compare → compare as
numbers with a unit check → allow a typo budget scaled to answer length → only
then `Undecided`.

`Undecided` is deliberately **not** a synonym for "probably wrong". A one-word
mismatch is `Incorrect`: paying a model to confirm that "meiosis" is not
"mitosis" spends money to reach the obvious. `Undecided` is for real ambiguity —
multi-word free text, or a right number with a missing or mismatched unit.

Resolved for free: case and whitespace, edge and intra-word punctuation, curly
quotes and en/em dashes (what Word and PDFs substitute silently), thousands
separators (`1,000` = `1000`), `5` = `5.0` = `1e3`, `0.5` = `1/2`, `5Ω` = `5 Ω`,
and typos scaled to length. A wrong *number* is never rescued by the typo
budget, so `24` against `42` stays wrong despite being 2 edits on a 2-char
answer.

**Refactored `practice_test.rs::grade_attempt`.** It previously sent *every*
short answer to `claude -p` — including answers identical to the model answer —
at real cost and up to 180s each. Now MC uses `grade_choice`, TF uses
`grade_true_false` (which also accepts `T`/`yes`/`1`), and short answers only
reach the model on `Undecided`. MC/TF also got stricter: the old
`eq_ignore_ascii_case` missed whitespace, punctuation and non-ASCII case.

**27 tests**, including an adversarial pass over nulls, emoji, 5000-char
strings, `1/0`, `1e999999`, bare `-`/`.`/`+`, `NaN` and combining marks,
asserting nothing panics. Two of my own assertions were wrong and the code's
behaviour was the question — fixing them properly meant deciding that
apostrophes and commas are *intra-word* (removed, so `it's` = `its`) while `/`
survives next to any alphanumeric so `m/s2` stays one token.

**109 tests pass workspace-wide.**

### ✅ Increment 5 — Phone layout and broken form controls

Before this, the app's only adaptation was an 860px breakpoint collapsing the
sidebar to a 76px icon rail. On a 375px phone that rail permanently ate a fifth
of the screen and left four unlabelled icons.

- **Bottom tab bar below 560px.** The sidebar becomes a fixed bottom bar —
  where a thumb reaches, and where the labels fit again (`.side-item span` is
  un-hidden). `env(safe-area-inset-bottom)` clears the iPhone home bar, and
  `.app-main` gets matching bottom padding so the last card is reachable.
- **`.tabs` overflow.** It was `display:flex` with no `overflow-x` and no wrap,
  so the primary navigation of the most-used screen ran off the edge with no
  way to reach the tabs past it. Now scrolls horizontally inside its own box.
- **Radio controls.** `input,textarea,select` sets `width:100%` + padding +
  border; there was a `checkbox` override and **none for radio**, so every radio
  rendered as a full-width padded box — broken on desktop too, and the MC and
  true/false study modes would have inherited it.
- **`.field-grid` row gap** was `0 1rem`, so stacked fields touched once the
  grid wrapped to one column.
- `100vh` → `100dvh` (vh is the *largest* viewport on iOS Safari, so the layout
  clipped behind the URL bar), 44px touch targets under `(pointer: coarse)`,
  a `.table-scroll` wrapper on the two real tables in `Study.tsx`, and a
  `prefers-reduced-motion` guard.

**Verified in a real browser at 375×812 and 1280×800**, against the actual
stylesheets, since the signed-in shell needs a PortFolio session: no horizontal
page scroll at either width, the bar pinned at `bottom === innerHeight`, tabs
scrolling (`scrollWidth 409 > clientWidth 343`) while staying inside the
viewport, zero controls under 44px, and the desktop sidebar unchanged at 232px
with its brand, labels and AI chip.

**That caught a bug I had just introduced:** the base `.sidebar` sets `top: 0`
for its sticky desktop behaviour, and on a *fixed* element `top: 0` together
with `bottom: 0` stretches it to the full viewport height — `height: auto`
cannot override that. The bar covered the entire screen. Fixed by giving the top
offset back explicitly. It also showed that a `border-radius: 50%` I had written
for the radio was a no-op (Chrome ignores author border-radius on a
default-appearance radio), so that was removed rather than left looking
load-bearing.

### ✅ Q1 and Q2 — answered by taking the recommended defaults

You said carry on without answering, so I took both recommendations and am
recording them as decisions rather than leaving them hanging.

**Q2 (Tauri), done first** — `frontendDist` is now the URL
`https://coursemaster.iambeep.com` instead of a bundled `ui/dist`. Done *before*
increment 6 precisely because that increment adds endpoints, which is what the
skew feeds on. Validated with `cargo tauri info` and `cargo build -p
course-master`. The validation earned its keep: my first attempt documented the
choice in a `_comment` key, and the config is schema-validated and rejects
unknown top-level properties, so it failed to parse. Rationale moved to
`CLAUDE.md`.

**Q1 (WAL)** — enabled in the same commit as 0005, as recommended.

### ✅ Increment 6 — Migration 0005: decks + cards, WAL, repo layer, HTTP API

- **Migration 0005**: `decks` (nullable `course_id`) + `cards` (`deck_id NOT
  NULL`, cascade). `cards.source_excerpt` nullable, `image_data_uri` reserved.
  Additive only — no table rebuild, so no cascade risk.
- **WAL + 5s `busy_timeout`** in `db::connect`. `connect_in_memory` deliberately
  skips it (meaningless for `:memory:`).
- **`models::Patch<T>`** = `Option<Option<T>>` + a `deserialize_patch` helper.
  This is a deliberate divergence from the house convention, where a patch field
  of `None` means "keep" and there is therefore no way to set a nullable column
  back to NULL. A student editing a generated card must be able to *delete* a
  wrong explanation.
- **`repo/decks.rs` + `repo/cards.rs`**, including `create_many` in one
  transaction (a half-saved set of approved cards is worse than none) and
  `search` as a `LIKE` scan with `%`/`_` escaped.
- **10 new endpoints**, all gated by `AuthUser`.
- **`api.ts` + `types.ts`** — the other two places an endpoint has to be added.

**Deviation from the plan:** I left `deck_sources` out. The plan put it here,
but nothing writes it until increment 10, and increment 9 already creates the
import staging tables where a deck's source material naturally belongs. An
unused table in production is schema debt.

**Verified by booting the server against a throwaway database**, not just by
unit tests: migration 0005 applied, the router constructed without a route
conflict, WAL confirmed by `-wal`/`-shm` appearing on disk, and then the full
API exercised with a minted token — 401 without one, name trimmed, `card_count`
going 0→2, search carrying deck and course name, 404 for a missing deck, 404 for
a card added to a missing deck, 400 for a blank front, cascade on deck delete,
and the `Patch` semantics proven over real JSON (`"explanation":"x"` sets,
`"explanation":null` clears, `front` untouched by both).

Also checked that **every one of the 49 handlers takes `AuthUser`** — the router
has no auth layer, so a handler that omits it is silently public.

**130 rust tests pass** (24 in academic-core, up from 5); tsc clean.

### ✅ Increment 7 — Deck and card UI

A new top-level **Decks** nav item, plus two pages:

- **`Decks.tsx`** — deck grid with card counts and course (or "unfiled"), create
  form with course + colour, delete with a confirm naming the card count, and
  **cross-library search** showing each hit's deck and course.
- **`DeckDetail.tsx`** — card list, add card, inline edit (front, back,
  explanation, tags as a comma string), delete, and a course picker that files
  or unfiles the deck.
- Three icons added (`IconCards`, `IconSearch`, `IconPencil`) in the existing
  hand-rolled style; still zero npm dependencies.

Blanking the explanation field sends an explicit `null`, which is precisely the
`Patch` case increment 6 built — the UI can *remove* a bad generated
explanation, not merely overwrite it.

**Driven in a real browser against a local API**, not just typechecked. Since
the signed-in shell needs a PortFolio session, I built the UI pointed at a local
throwaway server, minted a test token against a test secret, and injected it —
`ensureBridgeSession` reads a cached session from localStorage, so no real
credential was involved. Then clicked through the actual flow: created a deck
(filed under a seeded course), opened it, added a card, edited it to add an
explanation and tags (parsed from `"acids, definitions"` into two badges),
re-edited to blank the explanation — confirmed `explanation: null` with tags and
front untouched — and searched, getting "1 match" with the deck and course badge.
Also confirmed the nav active state: on the Decks page, `Decks` is the only
`.side-item.active`.

Afterwards the server was stopped, the test database deleted, and `ui/dist`
rebuilt so it no longer points at `localhost:18080`.

**130 rust tests pass; tsc clean.** No Rust changed in this increment — it is UI
against the API increment 6 already proved.

### ✅ Increment 8 — MILESTONE: flip-card study session

**You can now actually study.** `StudySession.tsx`: one card at a time, a 3D
flip to reveal the answer, and a read-through of the deck.

- Click or **Space** flips; **← →** move; **Esc** exits; swipe works on a phone
  (48px threshold, below which a swipe is indistinguishable from a tap that
  moved — stealing those would make the card unflippable on touch).
- Progress bar, `n / total`, and a "seen" count that takes the high-water mark
  so going back and forth doesn't inflate it.
- Moving to a new card resets the flip, so you never land on an answer.
- At the end, Next becomes **Shuffle and restart** (Fisher-Yates on a copy).
- The explanation shows on the answer face when a card has one.
- Entry point is a **Study (n)** button on the deck page, disabled at zero cards.

No scheduling in it, deliberately — "study this deck tonight" needs cards and a
way to move through them, which is exactly why this lands before spaced
repetition rather than behind it. Increments 12–13 add grading on top of this
screen.

The flip is a transform on an inner wrapper with `backface-visibility: hidden`
on both faces; the `prefers-reduced-motion` guard from increment 5 collapses the
duration, so with motion reduced the flip still *works*, it just happens
instantly.

**Verified by driving it in a browser** against a local throwaway API seeded
with a 3-card deck: Space set `flipped` and the transform was caught
mid-rotation; → advanced to card 2 with the flip reset and "2 seen"; at 3/3 the
progress bar read 100% and the button had become "Shuffle and restart"; going
back twice returned to 1/3 with "seen" still 3 and Previous disabled; Esc
returned to the deck page.

**130 rust tests pass; tsc clean.** No Rust changed.

### ✅ Increment 9 — Migration 0006: import staging

`card_imports` + `card_candidates`, shaped like `syllabus_extractions`: the
established review-before-commit pattern. Candidates land as `pending`, and
`approve_candidate` is the **only** path from a generated card to a `cards` row,
exactly as `approve_extraction` is the only path from an extraction to an
assignment.

The import row is written *before* the AI call, so a generation that times out
leaves a `failed` row carrying the reason rather than vanishing — and parsed
candidates are durable immediately, so closing the tab mid-review does not throw
away work that cost money. `source_text` is kept whole (not just per-card
excerpts) so a later "explain this" can be grounded in the student's own notes.

`resulting_card_id` is `ON DELETE SET NULL`, not CASCADE: deleting a card
approved earlier must not erase the record that it was reviewed.

**13 repo tests**, covering double-approve, approve-after-reject, an edit that
blanks a card, `approve_all` skipping rejected ones and being a no-op on a second
run, cost not being wiped by a later status change, and the SET NULL behaviour.

### ✅ Increment 10 — MILESTONE: generate cards from pasted notes, with review

`document-engine/src/card_generation.rs` + a **Generate from notes** tab on the
deck page.

Paste notes → Claude writes cards from *that material only* → every candidate is
shown with the verbatim passage it came from → Add, Edit, or Discard, or Approve
all. Nothing becomes a card without that step.

- The schema **requires** `source_excerpt` on every card, so the review screen
  can always show provenance rather than asking the student to trust a claim.
- Parsing is defensive like `practice_test::parse_questions`: one malformed card
  is dropped, the other eleven survive.
- Count clamped to 40; cost recorded per import and shown in the UI.
- Distractors are *not* generated here — they are generated when a card is first
  studied as multiple choice (increment 18), so a plain flip-through never pays
  for them.

**Found and fixed a pre-existing bug:** `ApiError::from(DocumentError)` mapped
*every* document error to 502, so "you sent no material" was reported as
`502 Bad Gateway` — the server blaming itself for the caller's input. Now mapped
per variant, delegating to the `CoreError` mapping so validation stays 400 and a
missing row stays 404. This also affected the existing calendar and syllabus
endpoints. Verified: empty material 400, missing deck 404, AI unavailable 502,
missing candidate 404.

Also verified locally that a failed generation records `status: failed` with the
reason on the import row and leaks **no** cards into the deck. Real generation
needs the live `claude` CLI, so it is verified against production after deploy.

**150 rust tests pass; tsc clean.**

### ✅ Increments 11–13 — Spaced repetition, on screen

**11. Migration 0007** — `card_schedule` (one row per card, created on first
review, not for every generated card) + `card_reviews` (one row per graded
answer, normalized rather than a blob, because every later metric is a query
over it).

`record_review` does the read, the SM-2 computation and both writes **in one
transaction**. A crash between them would leave a card whose history says it was
answered and whose schedule says it was not, and every analytic built on the log
would be wrong for that card forever. That is also why `academic-core` now
depends on `srs`: computing in a caller would mean read → compute outside the
transaction → write back, the exact interleaving this avoids.

`local_date` comes from the **client**. The server runs in UTC, so an 11pm
review in Eastern time is already tomorrow to it, and "due today" is a local-day
concept.

**12. Review API** — `GET /review/queue` spanning every deck and course, plus
submit, suspend, schedule and history. Caps are per course and separate for new
vs due: falling behind on reviews while still being shown new material is how a
backlog becomes unrecoverable. The queue interleaves courses via
`ROW_NUMBER() OVER (PARTITION BY course)` so a session covers the seven courses
you actually have instead of forty cards of chemistry.

**13. Review page** — a top-level **Review** nav item. Space reveals, **1–4**
grade, each button labelled with what it would actually do. Those projections
are computed **server-side** from the same `srs` code a review applies — a
second implementation in TypeScript would be free to drift and nothing would
notice. A card you keep failing offers to suspend itself, with the honest reason
(the card is doing too much, not that you need to see it more often).

**17 new tests** (167 total), including that the second review's `interval_before`
equals the first's `interval_after` — only true if both writes really shared a
transaction — plus suspended cards never appearing, suspension preserving a
card's schedule, a review not silently un-suspending, the new-card cap bounding
a 50-card deck, per-course interleaving, and a corrupt `due_date` not breaking
the queue.

**Verified in a browser against a local API**, two courses seeded: the queue
showed 6 due across both, Space revealed the answer, `3` graded it, the card left
the queue (6 → 5), the flip reset, and the next card came from the *other*
course — interleaving visible in the real UI. Then matured a card to three
reviews and confirmed the projections differentiate exactly as designed:
**Again 1 · Hard 18 · Good 38 · Easy 52** on a 15-day card — 15×1.2 for Hard and
15×2.65×1.3 for Easy, the two documented deviations from the 1987 paper working
end to end. History recorded `0→1 → 1→6 → 6→15`.

Note: on a brand-new card all four buttons read "1 day". That is correct SM-2 —
the first successful interval is fixed regardless of grade — not a bug.

### ✅ Increment 14 — Quizlet / Anki paste import

`document-engine/src/deck_import.rs` plus a paste box on the deck's **Generate
from notes** tab. Entirely deterministic — no AI call, no cost, no latency. A
deck you already wrote does not need a model to read it, and routing it through
the review queue would be friction for content you authored yourself, so these
become cards directly with duplicates reported rather than created.

- **Separator auto-detected** (tab / comma / semicolon / `" - "`) by whichever
  yields the most two-sided lines, tried tab-first and dash-last because a dash
  appears inside ordinary prose far more often than a tab does.
- **Quote-aware splitting**, so `"Boyle's law, simplified"` is not torn in half
  by its own comma; only the *first* separator splits a line.
- **HTML stripped**, which Anki exports carry by default, plus the common
  entities.
- **Idempotent**: a pair whose front already exists is skipped, matched with
  `grading::normalize` — the same folding the answer grader uses — so "Mole" and
  "mole." are recognised as the same card. A paste containing the same term
  twice does not create it twice either.
- Lines with no separator are skipped and counted, not fatal: an export often
  starts with a header, and losing the other 200 cards over it would be absurd.
- A **Preview** button parses and reports without writing.

**21 tests.** One caught a real bug: dropping every tag outright fused the words
either side of a `<br>` — "and&lt;br&gt;a break" became "anda break". Boundary
tags now become a space while inline tags still vanish, so `un<b>frie</b>ndly`
stays "unfriendly".

Verified end to end: auto-detected tab, skipped the header, stripped the HTML,
imported 3 — and **re-importing the same text created 0 and reported 3
duplicates**, with the deck still holding exactly 3 cards.

### ✅ Increment 17 — Typed-answer study mode

A **Flip / Type the answer** switch on the Review page. Typing is graded by
`crates/grading` through a new `POST /cards/{id}/check` — **no AI call**, so it
is instant and free. Measured at **~2.4 ms** per answer.

Correct and Incorrect resolve immediately and pre-select a grade (outlined);
`Undecided` shows both answers side by side and deliberately suggests nothing,
because guessing on the student's behalf is the exact thing to avoid. The
keyboard handler stands down while the input has focus, or Space would make the
field unusable.

Verified live: `mitochondria` ✓, `Mitochondria.` ✓ (case and punctuation),
`mitochondira` ✓ (typo budget), `nucleus` ✗, `the mitochondria organelle` →
undecided. And in the browser: typed the typo, the card revealed, and **Good**
came back outlined.

**Fixed a real false negative found by this testing:** `6.022 x 10^23` was
graded **incorrect** against `6.022e23`. Students write powers of ten longhand
constantly, and this is a chemistry student's app. `parse_numeric` now reads a
trailing `× 10^n`. The subtlety is that the multiplication sign has to be
*optional*: `normalize` turns `×` and `*` into spaces (they are not
alphanumeric), so `6.022 × 10^23` arrives as `6.022 10 23` — requiring the sign
would have missed the very form most likely to be typed. A real unit is still
safe: `5 x 10 apples` is not an exponent.

### ✅ Increment 18 — Multiple choice and mixed modes

Four modes on the Review page: **Flip · Type · Multiple choice · Mixed**.

Distractors are generated **once per deck, ahead of time**, by a *Prepare
options* button — never during a review. A review that waited on `claude -p`
would cost money per card and stall for seconds mid-session; options live in
`cards.options_json` so answering is a pure comparison. One AI call covers 25
cards rather than one call per card, since per-call overhead dominates at this
size.

The generator drops any "wrong" answer that is actually the correct one
(compared through `grading::normalize`, so "Mitochondria." does not slip past
"mitochondria") — two right options would make the question unanswerable — and
drops duplicates. A card it can produce nothing usable for stays a plain card
rather than becoming a broken question. Cards already having options are left
alone, so re-running is cheap.

**Mixed** asks each card the way it best supports: multiple choice when it has
options, typed otherwise. Choice mode falls back to typing rather than refusing
to show a card with no options yet.

Options are **shuffled for display**, seeded off the card id so the order is
stable while the card is on screen — storage puts the correct answer first, so
showing stored order would give it away.

**7 new tests.** Verified in the browser: options rendered shuffled (the stored-
first correct answer appeared last), picking a wrong one showed "Not quite",
revealed the card, and outlined **Again**.

### ✅ Increment 19 — "Explain why", cached per mistake

An **Explain why** button appears in the review session after a wrong answer.
The explanation is grounded in the card's own `source_excerpt` — the passage the
card was generated from — not in the model's general knowledge. That is what
increment 10 requires a source excerpt on every card *for*: an explanation that
quietly contradicts your lecture notes is worse than none, because you are
examined on the notes. There is no vector store and none is needed; the relevant
passage was captured when the card was made, so "retrieval" is a column read.

**Migration 0008** caches explanations keyed on the card plus the *normalised*
mistake, using the same `grading::normalize` that decided the answer was wrong.
So "Nucleus!" and "nucleus" are one misunderstanding, explained and paid for
once. Not answering at all is cached separately and asks a different question —
"why is your answer wrong" reads badly when there was no answer.

Writes use `ON CONFLICT DO UPDATE` so two requests racing on the same mistake
both succeed instead of one failing.

### ✅ Increments 21–22 — Analytics and an explainable exam-ready score

A **Progress** nav item, backed by one request rather than four round trips from
a phone.

**21 — analytics** (`repo/analytics.rs`): weak cards, per-day accuracy, study
totals, per-deck mastery. All queries over the normalized review log from
increment 11 — none of them expressible against a JSON blob. Grouped on
`local_date`, so a late-night session counts toward the day you experienced.

Weak cards rank by **lapses then accuracy**, not accuracy alone: a card failed 4
times in 20 is a worse problem than one failed once in one, and raw accuracy
would put the second on top. Mastery is counted over a deck's *cards*, not over
what has been reviewed — a 200-card deck with 3 reviewed reads 1.5%, not 100%,
because coverage is what predicts an exam.

**22 — `crates/examready`**: a fourth pure crate. Coverage 0.45 / retention 0.35
/ accuracy 0.20, every weight a named constant, returning the **reason**
alongside the score and naming the component actually holding it back —
*"held back by coverage: 80 of 100 cards never seen. Exam in 5 days."* A score
nobody can interrogate is a vibe with a percent sign.

Two deliberate behaviours: accuracy is **damped below 10 reviews**, so one lucky
answer is not 100% and one slip is not 0%; and an imminent exam can only
*temper* a thin score, never raise one — there is no time left to fix coverage
by Thursday.

The sparkline is inline SVG, so the frontend still has exactly two npm
dependencies. Bar height is accuracy, bar opacity is volume, so a 100% day built
on two cards doesn't look like one built on forty.

**17 new tests.** The adversarial sweep over `i64::MIN`/`i64::MAX` caught a real
**subtraction overflow** building the reason string — now saturating.

### ✅ Increment 23 — Exam countdowns, cram bias, daily nudge

A `GET /today` endpoint backing the Dashboard: what's due, whether you've
studied today, exams and quizzes ahead with days remaining, and one honest
sentence. **No scheduler, no background job, no stored counters** — all computed
on read, because the container is recreated on every deploy and anything
depending on a timer would silently reset. The nudge returns `None` when there
is genuinely nothing to say, rather than manufacturing a notification.

**Cram bias** brings a course with an exam inside 14 days to the front of the
review queue. Deliberately a *reordering*, not a rescheduling: nothing touches
`card_schedule`. Pulling SRS due dates forward to cram would corrupt the spacing
permanently and you'd still be paying for it in November; showing chemistry
first costs nothing and undoes itself once the exam passes. `sort_by_key` is
stable, so courses with no imminent exam keep their interleaved order.

Only exams and quizzes count — an essay deadline is not something flashcards
prepare you for.

### ✅ Increment 24 — Installable to the home screen

`manifest.webmanifest`, theme colour, iOS meta tags and real icons.

The icons are generated by a **hand-rolled PNG encoder** rather than an image
dependency — palette colour type, one byte per pixel. The first attempt used
RGBA with uncompressed deflate blocks and produced a **1 MB** 512×512 icon;
switching to a palette brought 192×192 to 36 KB. An SVG covers larger sizes, and
a 180×180 PNG is there because iOS ignores the manifest's icons for the home
screen.

Still exactly two npm runtime dependencies.

### Remaining

Status line per increment as each lands, plus anything deferred. Next:
9 (import staging tables) and **10 — paste notes, generate cards, review them**.

**Prerequisite, already done (not part of the 24):** the three D2L calendar-crawler defects — UTC read
as a local date, one item becoming three via D2L's `Available`/`Availability Ends`/`Due` state events,
and missing RFC 5545 text unescaping — are fixed and verified against the live feed (107 raw events →
86 canonical deadlines across 7 courses), and the polluted production rows have been cleaned up from a
verified backup. That work is **uncommitted** at the time of writing; increment 2 touches
`document-engine` call sites and increment 15 touches its `Cargo.toml`, so **land the calendar work
first.**
