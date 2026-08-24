# Single deployable image: the Rust API binary + the built frontend it
# serves as static files (same-origin, no CORS needed in production — see
# DEPLOY.md's Caddy block) + a Node.js runtime with the Claude Code CLI
# installed, since ai-engine's ClaudeCliProvider needs `claude` on PATH at
# container runtime, not just at build time.

FROM node:20-slim AS frontend-builder
WORKDIR /build/ui
COPY ui/package.json ui/package-lock.json* ./
RUN npm install
COPY ui/ ./
ARG VITE_PORTFOLIO_URL=https://iambeep.com
ARG VITE_API_BASE_URL=https://coursemaster.iambeep.com/api
ENV VITE_PORTFOLIO_URL=${VITE_PORTFOLIO_URL}
ENV VITE_API_BASE_URL=${VITE_API_BASE_URL}
RUN npm run build

FROM rust:1-slim-bookworm AS rust-builder
WORKDIR /build
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY desktop/src-tauri/Cargo.toml ./desktop/src-tauri/Cargo.toml
RUN mkdir -p desktop/src-tauri/src \
    && echo "fn main() {}" > desktop/src-tauri/src/main.rs \
    && echo "pub fn run() {}" > desktop/src-tauri/src/lib.rs
RUN cargo build --release -p api-server

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl gnupg \
    && mkdir -p /etc/apt/keyrings \
    && curl -fsSL https://deb.nodesource.com/setup_20.x | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && npm install -g @anthropic-ai/claude-code \
    && apt-get purge -y curl gnupg \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=rust-builder /build/target/release/coursemaster-api /app/coursemaster-api
COPY --from=frontend-builder /build/ui/dist /app/ui-dist

ENV STATIC_DIR=/app/ui-dist
ENV SQLITE_PATH=/app/data/coursemaster.db
ENV AI_SCRATCH_DIR=/app/data/ai-scratch
ENV PORT=8080
# `claude setup-token` writes here — mounted as a named volume in
# docker-compose.prod.yml so authentication survives image updates and
# container restarts; run it once per deploy via `docker compose exec`.
ENV HOME=/root

EXPOSE 8080
ENTRYPOINT ["/app/coursemaster-api"]
