#!/usr/bin/env bash
# deploy.sh — Auto-deploy PlotWeb when the watched branch has new commits.
#
# Usage:
#   ./deploy.sh [--force] [--branch <name>]
#
# Designed to be called from a cron job, e.g.:
#   */5 * * * * /path/to/plotweb/deploy.sh --branch main >> /var/log/plotweb-deploy.log 2>&1

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BRANCH="main"
FORCE=false

while [[ $# -gt 0 ]]; do
    case "$1" in
        --force)  FORCE=true; shift ;;
        --branch) BRANCH="$2"; shift 2 ;;
        *)        echo "Unknown option: $1" >&2; exit 1 ;;
    esac
done

log() { echo "[$(date '+%Y-%m-%d %H:%M:%S')] $*"; }

cd "$SCRIPT_DIR"

# Fetch latest remote state
git fetch origin "$BRANCH" --quiet

LOCAL_SHA="$(git rev-parse "origin/$BRANCH" 2>/dev/null || echo "none")"
REMOTE_SHA="$(git ls-remote origin "refs/heads/$BRANCH" | awk '{print $1}')"

if [[ -z "$REMOTE_SHA" ]]; then
    log "ERROR: branch '$BRANCH' not found on remote"
    exit 1
fi

if [[ "$FORCE" == false && "$LOCAL_SHA" == "$REMOTE_SHA" ]]; then
    log "No changes on $BRANCH ($LOCAL_SHA). Skipping."
    exit 0
fi

log "Deploying $BRANCH: $LOCAL_SHA -> $REMOTE_SHA"

# Update the local branch to match remote
git checkout "$BRANCH" --quiet 2>/dev/null || git checkout -b "$BRANCH" "origin/$BRANCH" --quiet
git reset --hard "origin/$BRANCH" --quiet

# Build and restart
log "Building containers..."
docker compose build --no-cache

log "Restarting..."
docker compose up -d

log "Deploy complete. Running on port 7919."
