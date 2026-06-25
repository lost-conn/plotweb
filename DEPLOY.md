# Deploying PlotWeb to jkbase (buildpack flow)

PlotWeb deploys to [jkbase](https://github.com/joeleaver/jkbase) using the
**buildpack** flow, not the Dockerfile. The Rust buildpack builds the backend on
the platform; the WASM frontend is built locally and shipped as static files.

The committed `Dockerfile` is kept as a fallback only — `jkbase.toml` selects the
buildpack (`builder` is left unset → `auto`).

## How the pieces fit

- **Backend** — `[servers.api]` uses the Rust buildpack. The platform build VM
  runs `cargo fetch` (network up, through an egress proxy that allows github.com
  and crates.io) then `cargo build --release --offline`. The single binary is
  shipped to `/app/plotweb-server`.
- **Frontend** — jkbase **cannot** build WASM (it never runs `trunk`).
  `[hosting]` only *copies* a pre-built `plotweb-web/dist/`. So you must run
  `trunk build --release` locally before each deploy.
- **rhypedb** — pulled as a **git dep** (pinned `rev`) in the root `Cargo.toml`,
  because the build VM can't see sibling path deps but the fetch phase can reach
  github. (rinch stays a path dep — it's frontend-only and resolves locally where
  the frontend is built.)
- **Routing** — `[routes."/api/*"]` sends `/api/*` (including the
  `/api/.../feedback/ws` WebSocket upgrades) to the `api` server; everything else
  falls through to the static SPA, with SPA fallback to `index.html`.

## One-time setup

1. Authenticate: `jkbase login`
2. Set the secrets / env (see below).
3. Attach the domain (if not already): `jkbase domain add pw.lostconnection.dev`
   then add the DNS record and `jkbase domain verify pw.lostconnection.dev`.

## Deploy workflow

```bash
# 1. Build the WASM frontend locally — MUST happen before deploy.
cd plotweb-web && trunk build --release && cd ..

# 2. Deploy from the repo root (where jkbase.toml lives).
jkbase deploy
```

> **`dist/` must be present on disk at deploy time.** It is `.gitignore`d, but
> the jkbase CLI's source tarball excludes only `node_modules/`, `.git/`, and
> `target/` — it does **not** honour `.gitignore`. So `plotweb-web/dist/` is
> uploaded as long as the `trunk build` output exists. If you `git clean -dfx` or
> deploy from a fresh checkout, re-run `trunk build --release` first or the static
> site will be empty.

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

# --- email (Resend) ---
jkbase secret set RESEND_API_KEY=<resend-api-key>
jkbase secret set RESEND_FROM='PlotWeb <noreply@pw.lostconnection.dev>'
jkbase secret set APP_URL=https://pw.lostconnection.dev
```

Notes:

- **`DATA_DIR` / `RHYPEDB_DATA_DIR` / `DATABASE_URL`** are not secret, but jkbase
  only delivers env via the secret mechanism, so they go through `secret set` too.
  All three point under `/data`, which is the mounted volume — so git book repos,
  the rhypedb metadata store, and the SQLite db all persist across redeploys.
- **`DIST_DIR`** is intentionally NOT set. In the buildpack flow the static SPA is
  served by jkbase's own static server (`[hosting]`), not by the backend binary,
  so the server's built-in `ServeDir` fallback is never hit by routed traffic.
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

## Co-developing rhypedb locally (UNCOMMITTED patch)

Because rhypedb is now a git dep, a plain `cargo build` fetches it from github
instead of your local checkout. To iterate on rhypedb and PlotWeb together, add a
`[patch]` to the **root `Cargo.toml`** pointing back at the sibling checkout:

```toml
# DO NOT COMMIT — local rhypedb co-dev only. The jkbase build VM cannot see
# sibling path deps, so committing this would break the buildpack deploy.
[patch."https://github.com/joeleaver/rhypedb"]
rhypedb-engine = { path = "../../personal/rhypedb/crates/rhypedb-engine" }
rhypedb-query  = { path = "../../personal/rhypedb/crates/rhypedb-query" }
rhypedb-schema = { path = "../../personal/rhypedb/crates/rhypedb-schema" }
```

Keep this out of commits (e.g. `git update-index --skip-worktree Cargo.toml`
while iterating, or just remember to drop it before committing). The `default-
features` flags on the original git deps still apply; the patch only swaps the
source.

## Follow-up: repin the rhypedb git dep

The git dep is pinned to `rev = 680de58…`, the tip of rhypedb's
`feat/optional-fastembed` branch. That branch makes the fastembed/ONNX stack an
opt-in feature, which our `default-features = false` build requires (rhypedb
`master` does not have it yet). **Once that work merges to rhypedb `master`,
repin all three `rhypedb-*` deps in the root `Cargo.toml` to the squash-merge
commit on `master`** (and update the same note in `Cargo.toml` / `Dockerfile`).

## What this repo could NOT verify

A live `jkbase deploy` was **not** run — there is no jkbase host / KVM available
in this environment. Verified locally instead:

- `cargo build -p plotweb-server` — builds, fetching the rhypedb git dep from
  github; the dependency tree is ONNX/fastembed-free.
- `trunk build --release` in `plotweb-web/` — produces `dist/` (incl.
  `index.html` + the wasm bundle).
- `jkbase.toml` parses and resolves correctly against
  `jkbase-common::config::ProjectConfig` (all fields, routes, volume, health
  check, and domains verified).
