# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What is PlotWeb

PlotWeb is a fiction writing web application with a Rust backend (Axum) and a Rust/WASM frontend using a custom UI framework called **rinch** (local dependency at `../../rinch/`, not part of this repo).

## Architecture

**Workspace layout** — The root `Cargo.toml` is a workspace containing `crates/*` but **excluding** `plotweb-web` (it has its own `Cargo.toml` and build toolchain).

- `crates/plotweb-common` — Shared types (User, Book, Chapter, API request/response structs) used by both server and web client.
- `crates/plotweb-server` — Axum REST API server. SQLite via sqlx, session auth via tower-sessions (in-memory store). Runs on port 3000. Serves the built frontend as a static SPA fallback from `plotweb-web/dist/`.
- `crates/plotweb-git` — Git-backed storage engine for book/chapter content and notes. Per-book locking via `HashMap<String, Arc<Mutex<()>>>`. All git/disk operations wrapped in `tokio::task::spawn_blocking`.
- `crates/plotweb-import` — Document import supporting Markdown and DOCX. Auto-detects chapter boundaries.
- `plotweb-web` — WASM frontend built with **Trunk**. Uses the rinch UI framework (signals, `rsx!` macro, components). Proxies `/api/` to `localhost:3000` in dev via Trunk config.

**Storage** — After migration 003, chapters live only in git repositories (one repo per book under `DATA_DIR`). SQLite tracks ownership (user→book mapping) but not content. Chapters are stored as JSON files with a `book.json` containing chapter order. Notes are also stored in git with a hierarchical tree structure (`notes.json` for tree, individual note JSON files).

**Database** — SQLite (`plotweb.db`), WAL mode, foreign keys enabled at connection time. Migrations applied at startup from `migrations/*.sql` via `include_str!` in `crates/plotweb-server/src/db.rs`. Migrations are run manually in order (not using sqlx migrate). Five migrations: initial schema → font_settings → git migration → beta readers → pinned commits.

**Frontend state** — Single `AppStore` struct with `Signal` fields (rinch reactive primitives). Client-side routing via `Route` enum in `store.rs` — no URL-based router, routes are set by mutating `store.current_route`.

**Auth** — Session-based (cookie), in-memory store (sessions lost on restart). Argon2 password hashing. The `/api/auth/me` endpoint is called on app start to check if a session exists.

**Real-time** — WebSocket endpoints for live feedback updates between authors and beta readers, managed by `FeedbackBroadcaster`.

## API Routes

All under `/api/`:

- **Auth**: `/auth/register`, `/auth/login`, `/auth/logout`, `/auth/me`
- **Books**: `/books` (list/create), `/books/{id}` (get/update/delete)
- **Chapters**: `/books/{book_id}/chapters` (CRUD + reorder)
- **Notes**: `/books/{book_id}/notes` (CRUD + `/move` + `/tree`)
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
cargo test                     # all workspace tests (incl. server HTTP integration tests)
cargo test -p plotweb-import   # import crate tests only (markdown chapter detection)
cargo test -p plotweb-server   # server integration tests (tests/*.rs drive the real Axum app)

# Faster local links (optional, Linux): route host-target links through mold.
# Writes .cargo/config.toml, which is GITIGNORED — see "Linking" below.
./scripts/setup-mold.sh        # enable;  --disable to remove

# End-to-end (Playwright, browser): builds the SPA + server over a temp data dir
cd e2e && npm install && npx playwright install chromium-headless-shell
cd e2e && npx playwright test

# Native desktop build (same frontend, rinch's winit/wgpu shell instead of the DOM)
cd plotweb-web && cargo build            # host target; `--target wasm32-...` is web
cd plotweb-web && ./target/debug/plotweb-web
```

The frontend targets **web and desktop from one codebase** — see
`plotweb-web/src/rinch_backend.rs` (rinch-web on wasm / rinch natively) and
`src/platform.rs` (browser-only APIs, `None` on native — `web_sys::window()`
*panics* off-wasm rather than returning `None`, so never call it directly in
shared page code). Linux desktop builds need the GTK dev stack
(`libgtk-3-dev libfontconfig-dev`); the native app reaches the server via
`PLOTWEB_SERVER` (default `http://127.0.0.1:3000`).

## Linking (mold)

Host Linux builds can link through **mold** instead of GNU ld — worth it here
because the debug binaries are ~320 MB each and `cargo test` links ~20 of them.
Run `./scripts/setup-mold.sh` once per clone (needs `mold` and `clang` on PATH).

The generated `.cargo/config.toml` is **gitignored on purpose**. jkbase builds
this repo in a sealed, offline buildpack VM that we cannot install packages
into, so a committed config demanding mold would fail the deploy at link time
with no way to fix it from here. mold stays a per-developer opt-in.

Only `x86_64-unknown-linux-gnu` is configured; `wasm32` links with rust-lld and
is deliberately untouched. `./scripts/setup-mold.sh --disable` reverts it, and
the script refuses to overwrite a `.cargo/config.toml` it did not write.

## Driving the native app (rinch MCP)

The desktop window can't be driven by Playwright. Build it with the opt-in
`debug-mcp` feature and rinch's MCP server takes over that role — screenshots,
DOM queries, computed styles, click/type/key, caret positions:

```bash
cd plotweb-web && cargo build --features debug-mcp   # NEVER for release: opens a control port
./target/debug/plotweb-web                            # registers in ~/.rinch/debug/{pid}.json
```

`.mcp.json` registers `rinch-mcp-server` from the sibling rinch checkout
(`../rinch`, the same layout DEPLOY.md's co-dev `[patch]` assumes), so build it
once with `cargo build -p rinch-mcp-server` in that repo. Then `list_apps` →
`connect` (or `launch`) drives the running window.

The server is a **lib + bin**: routing/state live in `crates/plotweb-server/src/lib.rs`
(`api_router`, `build_state`, `test_router`); `main.rs` is a thin wrapper that adds
env-based state and static-file serving. Integration tests in
`crates/plotweb-server/tests/` build the app in-process via `tower::oneshot` over
tempdir SQLite/rhypedb/git stores (see `tests/common/mod.rs`). E2E lives in `e2e/`
(see `e2e/README.md`); the `RHYPEDB_DATA_DIR` env var (default `data/rhypedb`)
points the metadata store at a directory.

## Environment Variables

| Variable | Default | Purpose |
|----------|---------|---------|
| `DATABASE_URL` | `sqlite:plotweb.db` | SQLite database path |
| `DATA_DIR` | `data/books` | Root directory for git-backed book repositories |
| `RHYPEDB_DATA_DIR` | `data/rhypedb` | Embedded rhypedb metadata store directory |
| `DIST_DIR` | `../plotweb-web/dist` | Path to built frontend dist/ folder |

## Key Conventions

- Rust edition 2024 (workspace-level).
- IDs are UUID v4 strings.
- The frontend uses `wasm-bindgen` + `web-sys` directly for DOM and fetch — no `reqwest` on the client side. API helpers are in `plotweb-web/src/api.rs`.
- Font settings are stored as JSON text in the `font_settings` column of `books`.
- Deployment via Docker (single image, port 7919) with a cron-based `deploy.sh` script. The Dockerfile clones rinch at a pinned commit during build.
