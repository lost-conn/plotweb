# Deploying PlotWeb to jkbase (buildpack flow)

PlotWeb deploys to [jkbase](https://github.com/joeleaver/jkbase) using the
**buildpack** flow, not the Dockerfile. **Both** the Rust backend and the WASM
frontend are built **server-side** on the platform — the Rust buildpack for the
backend, the trunk buildpack for the frontend. Nothing is pre-built locally and
no `dist/` is committed.

The committed `Dockerfile` is kept as a fallback only — `jkbase.toml` selects the
buildpack (`builder` is left unset → `auto`).

> **Platform requirement:** server-side frontend builds need jkbase.app to be
> running a jkbase build that includes the trunk buildpack
> (joeleaver/jkbase **#49**, **#50**) and the monorepo build-`context` feature
> (**#52**). All are merged upstream; the platform must have rolled them. If it
> hasn't yet, fall back to the pre-built `[hosting]` form (git history of this
> file) until it does.

## How the pieces fit

- **Backend** — `[servers.api]` uses the Rust buildpack. The platform build VM
  runs `cargo fetch` (network up, through an egress proxy that allows github.com
  and crates.io) then `cargo build --release --offline`. The single binary is
  shipped to `/app/plotweb-server`.
- **Frontend** — `[sites.app] build = "trunk"` runs the trunk buildpack
  server-side (`trunk build --release` in `plotweb-web/`) and serves the produced
  `dist/` as a static SPA. `context = "."` mounts the whole repo into the build VM
  so the frontend crate's in-repo sibling dep (`plotweb-common`) resolves.
- **External deps as git deps** — both **rhypedb** (root `Cargo.toml`) and
  **rinch** (`plotweb-web/Cargo.toml`) are pinned **git deps**, because the sealed
  build VM can't see sibling path deps but the fetch phase can reach github.
  (`plotweb-common` stays a path dep — it's in-repo and resolves via `context`.)
- **Routing** — `[routes."/api/*"]` sends `/api/*` (including the
  `/api/.../feedback/ws` WebSocket upgrades) to the `api` server; everything else
  falls through to the static SPA, with SPA fallback to `index.html`.

## One-time setup

1. Authenticate: `jkbase login`
2. Set the secrets / env (see below).
3. Attach the domain (if not already): `jkbase domain add pw.lostconnection.dev`
   then add the DNS record and `jkbase domain verify pw.lostconnection.dev`.

## Deploy workflow

Both builds happen server-side, so there's **no local pre-build**. Two options:

```bash
# A. Tarball upload, from the repo root (where jkbase.toml lives).
jkbase deploy

# B. Push-to-deploy: one-time connect (mints a token + adds a `jkbase` remote),
#    then every push to the trigger branch deploys.
jkbase repo connect
git push jkbase main
```

Push-to-deploy works precisely *because* nothing is pre-built: `git push` ships
only committed content, and there is no committed `dist/` to be omitted — jkbase
builds the frontend from source. (`jkbase repo github` scaffolds a GitHub Actions
workflow for the same thing.)

## Environment variables and secrets

jkbase has **no env field in `jkbase.toml`** — every runtime env var (both
non-secret config and real secrets) is set with `jkbase secret set`. The runtime
clears the environment and injects these.

Set these once per project (re-run to change a value):

```bash
# --- storage layout: both stores live under the single mounted /data volume ---
jkbase secret set DATA_DIR=/data/books
jkbase secret set RHYPEDB_DATA_DIR=/data/rhypedb
jkbase secret set DATABASE_URL=sqlite:/data/plotweb.db

# --- email (Resend) — optional; email features are disabled if RESEND_API_KEY is unset ---
jkbase secret set RESEND_API_KEY=<resend-api-key>
jkbase secret set RESEND_FROM='PlotWeb <noreply@pw.lostconnection.dev>'
jkbase secret set APP_URL=https://pw.lostconnection.dev
```

Notes:

- **`DATA_DIR` / `RHYPEDB_DATA_DIR` / `DATABASE_URL`** are not secret, but jkbase
  only delivers env via the secret mechanism, so they go through `secret set` too.
  All three point under `/data`, the mounted volume — so git book repos, the
  rhypedb metadata store, and the SQLite db all persist across redeploys.
- **`DIST_DIR`** is intentionally NOT set. The static SPA is served by jkbase's
  static server (the built `[sites.app]`), not by the backend binary, so the
  server's built-in `ServeDir` fallback is never hit by routed traffic.
- **Rotating `RESEND_API_KEY`**: create a new key in the Resend dashboard, then
  `jkbase secret set RESEND_API_KEY=<new-key>` and redeploy (or restart) so the
  new value is injected. Revoke the old key afterwards. `jkbase secret list`
  shows keys (names only); `jkbase secret rm <KEY>` removes one.

## Data volume layout

A single persistent volume named `data` is mounted at `/data`:

```
/data
├── books/        # DATA_DIR — one git repo per book (manuscript + notes)
├── rhypedb/      # RHYPEDB_DATA_DIR — embedded rhypedb metadata store
└── plotweb.db    # DATABASE_URL — legacy SQLite (still opened + migrated at boot)
```

## Health check

`jkbase.toml` declares `[servers.api.health_check] path = "/health"`. The backend
serves `GET /health` → `200 OK` (no auth, no session). interval `10s`, timeout
`5s`.

## Co-developing rhypedb / rinch locally (UNCOMMITTED patch)

Because rhypedb and rinch are now git deps, a plain `cargo build` / `trunk build`
fetches them from github instead of your local checkouts. To iterate on them and
PlotWeb together, add `[patch]` sections pointing back at the sibling checkouts —
rhypedb in the **root `Cargo.toml`**, rinch in **`plotweb-web/Cargo.toml`**:

```toml
# DO NOT COMMIT — local co-dev only. The jkbase build VM cannot see sibling path
# deps, so committing these would break the buildpack deploy.

# root Cargo.toml:
[patch."https://github.com/joeleaver/rhypedb"]
rhypedb-engine = { path = "../../personal/rhypedb/crates/rhypedb-engine" }
rhypedb-query  = { path = "../../personal/rhypedb/crates/rhypedb-query" }
rhypedb-schema = { path = "../../personal/rhypedb/crates/rhypedb-schema" }

# plotweb-web/Cargo.toml:
[patch."https://github.com/joeleaver/rinch"]
rinch              = { path = "../../rinch/crates/rinch" }
rinch-core         = { path = "../../rinch/crates/rinch-core" }
rinch-tabler-icons = { path = "../../rinch/crates/rinch-tabler-icons" }
```

Keep these out of commits (e.g. `git update-index --skip-worktree <file>` while
iterating, or just remember to drop them before committing). The original git
deps' `default-features`/`features` flags still apply; the patch only swaps the
source.

## Follow-up: repin the git deps

- **rhypedb** is pinned to `rev = 680de58…`, the tip of rhypedb's
  `feat/optional-fastembed` branch (it makes the fastembed/ONNX stack opt-in,
  which our `default-features = false` build requires; rhypedb `master` may not
  have it yet). Once that merges to `master`, repin all three `rhypedb-*` deps in
  the root `Cargo.toml` (and the matching note in `Cargo.toml` / `Dockerfile`).
- **rinch** is pinned to `rev = 1f16bed…` in `plotweb-web/Cargo.toml`, kept in
  sync with the `Dockerfile`'s `RINCH_COMMIT`. Repin both together as rinch's
  `main` advances.

## Verification status

Verified locally:

- `cargo build -p plotweb-server` — builds, fetching the rhypedb git dep from
  github; the dependency tree is ONNX/fastembed-free.
- `trunk build --release` in `plotweb-web/` — builds against **rinch as a git
  dep** and produces `dist/` (index.html + the wasm bundle).
- `jkbase.toml` parses against `jkbase-common::config::ProjectConfig`.

Not yet verified end-to-end: a live deploy exercising the **server-side trunk
build + monorepo `context`** on jkbase.app — that depends on the platform running
the trunk/context features (see the platform-requirement note up top).
