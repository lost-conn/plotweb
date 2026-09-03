#!/usr/bin/env bash
# Launch the native desktop app against the live server.
#
# The binary itself defaults to `http://127.0.0.1:3000` (a dev server); this points it at
# production. It no longer sets PLOTWEB_SYNC: the app switches sync on by itself for a
# cut-over book, where sync is the only path an edit takes to the server. Forcing it on
# would also switch it on for books that are *not* cut over, and there two writers are
# live at once — a whole-content PUT into git and CRDT ops into the canonical copy, with
# nothing mirroring between them. `PLOTWEB_SYNC=0` remains the way to switch it off.
#
# Local documents live in the OS data dir ($XDG_DATA_HOME/plotweb/docs, else
# ~/.local/share/plotweb/docs) and persist across launches — that is the offline copy,
# and it is what keeps the app usable with no network. Override with PLOTWEB_LOCAL_DATA
# to run against a scratch store instead.
#
# Build it with:  cd plotweb-web && cargo build --release
# NEVER with `--features debug-mcp` — that opens an unauthenticated control port.
set -euo pipefail

SERVER="${PLOTWEB_SERVER:-https://pw.lostconnection.dev}"
BIN="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/plotweb-web/target/release/plotweb-web"

if [ ! -x "$BIN" ]; then
  echo "No release binary at $BIN" >&2
  echo "Build it first:  cd plotweb-web && cargo build --release" >&2
  exit 1
fi

# A release build must not carry the debug control port. `debug-mcp` pulls in rinch's
# debug server, which registers a listener under ~/.rinch/debug — cheap to check, and
# the failure it prevents is an open port on a machine you use for writing.
if strings "$BIN" 2>/dev/null | grep -q 'rinch/debug'; then
  echo "Refusing to run: $BIN looks like it was built with --features debug-mcp," >&2
  echo "which opens an unauthenticated control port. Rebuild without that feature." >&2
  exit 1
fi

echo "PlotWeb → $SERVER   (cut-over books sync, documents in ${PLOTWEB_LOCAL_DATA:-the OS data dir})"
exec env PLOTWEB_SERVER="$SERVER" "$BIN" "$@"
