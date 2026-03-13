# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What is PlotWeb

PlotWeb is a fiction writing web application with a Rust backend (Axum) and a Rust/WASM frontend using a custom UI framework called **rinch** (local dependency at `../../rinch/`).

## Architecture

**Workspace layout** — The root `Cargo.toml` is a workspace containing `crates/*` but **excluding** `plotweb-web` (it has its own `Cargo.toml` and build toolchain).

- `crates/plotweb-common` — Shared types (User, Book, Chapter, API request/response structs) used by both server and web client.
- `crates/plotweb-server` — Axum REST API server. SQLite via sqlx, session auth via tower-sessions (in-memory store). Runs on port 3000. Serves the built frontend as a static SPA fallback from `plotweb-web/dist/`.
- `plotweb-web` — WASM frontend built with **Trunk**. Uses the rinch UI framework (signals, `rsx!` macro, components). Proxies `/api/` to `localhost:3000` in dev via Trunk config.

**Database** — SQLite (`plotweb.db`), WAL mode, migrations applied at startup from `migrations/*.sql` via `include_str!` in `crates/plotweb-server/src/db.rs`. Migrations are run manually in order (not using sqlx migrate).

**Frontend state** — Single `AppStore` struct with `Signal` fields (rinch reactive primitives). Client-side routing via `Route` enum in `store.rs` — no URL-based router, routes are set by mutating `store.current_route`.

**Auth** — Session-based (cookie). Argon2 password hashing. The `/api/auth/me` endpoint is called on app start to check if a session exists.

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
```

## Key Conventions

- Rust edition 2024 (workspace-level).
- IDs are UUID v4 strings.
- The frontend uses `wasm-bindgen` + `web-sys` directly for DOM and fetch — no `reqwest` on the client side. API helpers are in `plotweb-web/src/api.rs`.
- Font settings are stored as JSON text in the `font_settings` column of `books`.
