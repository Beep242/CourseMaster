# Deploying CourseMaster to the VPS

CourseMaster ships as one Docker image (Rust API + built frontend as static
files, same-origin — no CORS in production) plus a Node.js runtime with the
Claude Code CLI installed inside the image for `ai-engine`'s subprocess calls.

None of this has been run against the live VPS yet — everything below is
prepared and ready, but every step here touches shared/production
infrastructure, so it's written as a checklist for you to run (or ask me to
run one step at a time) rather than something already done.

## 1. DNS

Point `coursemaster.iambeep.com` at the VPS (95.216.166.108), same as the
other `*.iambeep.com` subdomains.

## 2. PortFolio side (auth bridge)

Already done **locally** in this session (`C:\Users\Hendrix\Desktop\PortFolio\.env`
and `.env.example`): added `coursemaster` to `CROSS_APP_AUDIENCES` and
`https://coursemaster.iambeep.com` + `http://localhost:1430` to
`CROSS_APP_ALLOWED_ORIGINS`. This still needs to reach the **deployed**
PortFolio instance — commit + deploy PortFolio's `.env` change (or update the
VPS's live PortFolio `.env` directly) before CourseMaster's login will work
in production.

## 3. GitHub repo setup

- Push this repo to `https://github.com/Beep242/CourseMaster` (already the
  configured `origin`).
- Run `vps-auto-loader onboard --host 95.216.166.108 --type docker --remote-path /opt/coursemaster --compose-file docker-compose.prod.yml` from `E:\VPS-Auto-Loader` (or by hand, set the three secrets `VPS_HOST`, `VPS_USER`, `VPS_SSH_KEY` on the CourseMaster GitHub repo) — this writes `.github/workflows/deploy.yml`; the workflow already in this repo (`build-and-push.yml`) builds+pushes the image and then SSHes in to `docker compose pull && up -d`, so you may not need a second workflow file — check for overlap before adding one.

## 4. VPS-side app setup (one-time, `vps-auto-loader` doesn't do this part)

```bash
mkdir -p /opt/coursemaster
cd /opt/coursemaster
git clone https://github.com/Beep242/CourseMaster.git .
cp .env.example .env
# edit .env: CROSS_APP_JWT_SECRET (copy verbatim from PortFolio's live .env
# — NOT PortFolio's local dev .env, they differ), OWNER_EMAIL, GHCR_OWNER
docker compose -f docker-compose.prod.yml pull
docker compose -f docker-compose.prod.yml up -d
docker compose -f docker-compose.prod.yml exec api claude setup-token
# follow the printed URL, authorize with your Claude subscription, and watch
# the terminal through to "Long-lived authentication token created
# successfully!" — it prints the token directly rather than saving it
# anywhere on disk (confirmed: `claude auth status` inside the container
# still reports loggedIn: false after a completed run). Copy that token into
# .env as CLAUDE_CODE_OAUTH_TOKEN, then:
docker compose -f docker-compose.prod.yml up -d
# picks up the token as an env var — this is the one that actually needs to
# succeed; `claude auth status` in the container should now say loggedIn: true.
```

**Firewall/proxy gotcha already hit once during this deploy:** the api
container's port must NOT be bound loopback-only (`127.0.0.1:8080:8080`) —
Caddy reaches it via `host.docker.internal`, which arrives from the docker
bridge network, not `127.0.0.1`, so a loopback-only bind causes a silent 502
with no obvious cause. The compose file here binds openly and relies on a
`ufw` rule scoped to Caddy's bridge subnets instead (mirrors PortFolio's
existing port-4000 rule) — if you ever regenerate this file, keep that
pattern rather than reverting to loopback-only.

## 5. Caddy route

CourseMaster does **not** ship its own Caddy container — like StockMan and
BPass, it binds `127.0.0.1:8080` only and relies on whichever Caddy instance
already owns ports 80/443 on the VPS (the one TruthSeeker's compose stack
started). **Check that instance's actual current Caddyfile on the VPS before
editing** — the copy in `E:\TruthSeeker\Caddyfile` in this dev environment is
almost certainly stale (StockMan/BPass/CoLink's blocks were added directly on
the server, not reflected back into that local file). Add:

```
coursemaster.iambeep.com {
	reverse_proxy 127.0.0.1:8080
}
```

then reload/restart that Caddy container.

## 6. Verify

- `https://coursemaster.iambeep.com/` loads the app shell and shows "Sign in
  with Beep.dev".
- Signing in via PortFolio lands back in CourseMaster and completes
  onboarding.
- Settings page shows "Claude Code found" for the AI status chip. If a real
  AI action (e.g. submitting a syllabus) fails with "Not logged in", re-run
  step 4's `claude setup-token`.

## 7. Database snapshots, and the runbook for a schema change

**There is exactly one copy of this data.** It lives in the `coursemaster-data`
Docker volume on the VPS. There is no staging environment, and a push to `main`
*is* a production deploy.

**Rolling the image back does not roll a migration back.** `db::connect` runs
`sqlx::migrate!` on every boot with `ignore_missing = false`, so once migration
*N* has been applied, an older binary that does not contain *N* fails
`validate_applied_migrations` with `VersionMissing` and panics during startup.
That same binary serves `ui/dist`, so the entire site — not just the API — goes
down, and it will not come back by redeploying the previous tag. Restoring the
database is the only way out.

```bash
./scripts/backup-db.sh                  # snapshot, verify, pull a copy locally
./scripts/restore-db.sh <snapshot.db>   # destructive; re-verifies first
```

`backup-db.sh` uses `sqlite3 .backup` rather than copying the file, because a
plain copy races with in-flight writes and (once WAL is enabled) would miss the
`-wal` sidecar. It installs `sqlite3` into the container on demand — the runtime
image carries only Node and the Claude CLI, and an `apt-get install` there does
not survive a container recreate. Snapshots land in `/opt/coursemaster/backups`
on the VPS and `./backups` locally (both gitignored via `*.db`).

`restore-db.sh` verifies the snapshot *before* touching anything, snapshots the
database it is about to replace, then stops the container and swaps the file on
the host — not via `docker exec`, because a stopped container cannot be exec'd
and the stale `-wal`/`-shm` sidecars must be deleted while nothing is running.
It resolves the volume's host path from `docker inspect`, so it works for both a
named volume and a bind mount, and it aborts if the container does not stay up.

### Before any deploy that adds a migration

1. `./scripts/backup-db.sh` — confirm it prints `integrity_check: ok` and a row
   summary that looks like your actual data.
2. Confirm the local copy exists in `./backups`. A snapshot that only exists on
   the same host you are about to change is not a backup.
3. `cargo test --workspace` locally. **CI never runs it** —
   `build-and-push.yml` runs `tsc -b` and nothing else, so an untested push is
   genuinely untested.
4. Push, then watch the Action through to the SSH deploy step.
5. Hit `https://coursemaster.iambeep.com/` and sign in. A migration failure
   shows up as the whole site being down, not as an API error.
6. If it is down: `docker logs --tail 50 coursemaster-api-1`. A
   `VersionMissing` or migration error means restore —
   `./scripts/restore-db.sh <the snapshot from step 1>` — and redeploy the
   previous image only *after* the database is back.

Both scripts honour `CM_SSH_TARGET`, `CM_CONTAINER`, `CM_DB_PATH` and
`CM_BACKUP_DIR`, which is also how the restore path gets exercised against a
throwaway container instead of production.

## Local development against a local server

```bash
# terminal 1 — API
cd crates/api-server
CROSS_APP_JWT_SECRET=<same value as your local PortFolio .env> \
OWNER_EMAIL=<your PortFolio account email> \
PORT=8080 \
STATIC_DIR=../../ui/dist \
WEB_ALLOWED_ORIGINS=http://localhost:1430 \
cargo run

# terminal 2 — frontend with hot reload
cd ui
npm run dev   # uses .env.development (localhost:8080 API, localhost:3000 PortFolio)
```

The Tauri desktop shell (`cargo tauri dev` / `cargo tauri build` from
`desktop/src-tauri`) embeds whatever's currently built in `ui/dist` — rebuild
the frontend (`npm run build` in `ui/`) before a desktop build to pick up
frontend changes, since the desktop shell doesn't have its own dev-server
wiring to the API the way the browser build does.
