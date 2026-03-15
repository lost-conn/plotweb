# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What is PlotWeb

PlotWeb is a fiction writing web application with a Rust backend (Axum) and a Rust/WASM frontend using a custom UI framework called **rinch** (local dependency at `../../rinch/`, not part of this repo).

## Architecture

**Workspace layout** — The root `Cargo.toml` is a workspace containing `crates/*` but **excluding** `plotweb-web` (it has its own `Cargo.toml` and build toolchain).

- `crates/plotweb-common` — Shared types (User, Book, Chapter, API request/response structs) used by both server and web client.
- `crates/plotweb-server` — Axum REST API server. SQLite via sqlx, session auth via tower-sessions (in-memory store). Runs on port 3000. Serves the built frontend as a static SPA fallback from `plotweb-web/dist/`.
- `crates/plotweb-git` — Git-backed storage engine for book/chapter content. Per-book locking via `HashMap<String, Arc<Mutex<()>>>`. All git/disk operations wrapped in `tokio::task::spawn_blocking`.
- `crates/plotweb-import` — Document import supporting Markdown and DOCX. Auto-detects chapter boundaries.
- `plotweb-web` — WASM frontend built with **Trunk**. Uses the rinch UI framework (signals, `rsx!` macro, components). Proxies `/api/` to `localhost:3000` in dev via Trunk config.

**Storage** — After migration 003, chapters live only in git repositories (one repo per book under `DATA_DIR`). SQLite tracks ownership (user→book mapping) but not content. Chapters are stored as JSON files with a `book.json` containing chapter order.

**Database** — SQLite (`plotweb.db`), WAL mode, foreign keys enabled at connection time. Migrations applied at startup from `migrations/*.sql` via `include_str!` in `crates/plotweb-server/src/db.rs`. Migrations are run manually in order (not using sqlx migrate). Five migrations: initial schema → font_settings → git migration → beta readers → pinned commits.

**Frontend state** — Single `AppStore` struct with `Signal` fields (rinch reactive primitives). Client-side routing via `Route` enum in `store.rs` — no URL-based router, routes are set by mutating `store.current_route`.

**Auth** — Session-based (cookie), in-memory store (sessions lost on restart). Argon2 password hashing. The `/api/auth/me` endpoint is called on app start to check if a session exists.

**Real-time** — WebSocket endpoints for live feedback updates between authors and beta readers, managed by `FeedbackBroadcaster`.

## API Routes

All under `/api/`:

- **Auth**: `/auth/register`, `/auth/login`, `/auth/logout`, `/auth/me`
- **Books**: `/books` (list/create), `/books/{id}` (get/update/delete)
- **Chapters**: `/books/{book_id}/chapters` (CRUD + reorder)
- **Import**: `/books/{book_id}/import/preview`, `/books/{book_id}/import/confirm`
- **Fonts**: `/fonts` (list Google Fonts, cached)
- **Beta Links** (auth'd): `/books/{book_id}/beta-links` (CRUD)
- **Author Feedback** (auth'd): `/books/{book_id}/feedback` (list/resolve/delete/reply)
- **Public Beta** (token-based, no auth): `/beta/{token}`, `/beta/{token}/chapters/{id}`, `/beta/{token}/feedback`
- **WebSockets**: `/books/{book_id}/feedback/ws`, `/beta/{token}/feedback/ws`

## Build & Run Commands

```bash
# Backend (from repo root)
cargo build                    # build server
cargo run                      # run server on :3000

# Frontend (from plotweb-web/)
trunk serve                    # dev server on :8080 with proxy to :3000
trunk build                    # production build to plotweb-web/dist/

# Both must be running for local dev:
#   Terminal 1: cargo run          (API server, port 3000)
#   Terminal 2: cd plotweb-web && trunk serve  (frontend, port 8080)

# Tests
cargo test                     # all workspace tests
cargo test -p plotweb-import   # import crate tests only (markdown chapter detection)
```

## Environment Variables

| Variable | Default | Purpose |
|----------|---------|---------|
| `DATABASE_URL` | `sqlite:plotweb.db` | SQLite database path |
| `DATA_DIR` | `data/books` | Root directory for git-backed book repositories |
| `DIST_DIR` | `../plotweb-web/dist` | Path to built frontend dist/ folder |

## Key Conventions

- Rust edition 2024 (workspace-level).
- IDs are UUID v4 strings.
- The frontend uses `wasm-bindgen` + `web-sys` directly for DOM and fetch — no `reqwest` on the client side. API helpers are in `plotweb-web/src/api.rs`.
- Font settings are stored as JSON text in the `font_settings` column of `books`.
- Deployment via Docker (single image, port 7919) with a cron-based `deploy.sh` script. The Dockerfile clones rinch at a pinned commit during build.
