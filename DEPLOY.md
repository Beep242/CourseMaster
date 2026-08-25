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
