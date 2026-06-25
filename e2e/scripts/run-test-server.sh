#!/usr/bin/env bash
# Launch the PlotWeb server for e2e tests against a throwaway data dir.
#
# - Builds the server (cached/fast) and, if needed, the frontend dist.
# - Points DATABASE_URL / DATA_DIR / RHYPEDB_DATA_DIR at a fresh temp dir so each
#   run starts clean and never touches the developer's real data.
# - Serves the prebuilt SPA from plotweb-web/dist on :3000.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_ROOT"

# Build the frontend if the dist is missing (otherwise reuse it — it's slow).
if [[ ! -f plotweb-web/dist/index.html ]]; then
  echo "[e2e] building frontend (trunk build)…"
  (cd plotweb-web && trunk build)
fi

echo "[e2e] building server…"
cargo build -p plotweb-server

# Fresh, isolated state for this run.
E2E_STATE="$(mktemp -d "${TMPDIR:-/tmp}/plotweb-e2e.XXXXXX")"
trap 'rm -rf "$E2E_STATE"' EXIT

export DATABASE_URL="sqlite:${E2E_STATE}/plotweb.db"
export DATA_DIR="${E2E_STATE}/books"
export RHYPEDB_DATA_DIR="${E2E_STATE}/rhypedb"
export DIST_DIR="${REPO_ROOT}/plotweb-web/dist"

echo "[e2e] starting server on :3000 (state: ${E2E_STATE})"
exec cargo run -q -p plotweb-server
