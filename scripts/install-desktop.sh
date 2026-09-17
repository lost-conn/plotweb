#!/usr/bin/env bash
# Install the native desktop app into this user's XDG directories, so PlotWeb shows
# up in the application launcher with a real icon and can be run without a terminal.
#
# Nothing is copied to a system location and nothing needs root: the launcher entry
# goes in ~/.local/share/applications, icons in the user's hicolor theme.
#
# The entry's Exec points at `scripts/plotweb-desktop.sh` **in this checkout**, so
# rebuilding (`cd plotweb-web && cargo build --release`) updates the installed app
# with no reinstall. Moving or deleting the checkout breaks the launcher — rerun this
# script from the new location to repoint it.
#
#   ./scripts/install-desktop.sh              # build release, then install
#   ./scripts/install-desktop.sh --no-build   # install what is already built
#   ./scripts/install-desktop.sh --uninstall  # remove everything this installed
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DATA_HOME="${XDG_DATA_HOME:-$HOME/.local/share}"

# The window's Wayland app_id / X11 WM_CLASS — must match `APP_ID` in
# plotweb-web/src/main.rs.
APP_ID="dev.lostconnection.plotweb"

# The launcher entry is deliberately NOT named after $APP_ID. rinch writes its own
# `$XDG_DATA_HOME/applications/$APP_ID.desktop` stub carrying `NoDisplay=true` on
# every single launch, and it only declines to do so when a *system* entry of that
# name exists — a user-level entry is simply overwritten. An entry named after the
# app id would therefore disappear from the launcher the first time the app ran.
# Using a different basename sidesteps that entirely; `StartupWMClass` below is what
# ties the running window back to this entry for the icon and taskbar grouping.
ENTRY="plotweb"

DESKTOP_FILE="$DATA_HOME/applications/$ENTRY.desktop"
ASSET_DIR="$DATA_HOME/plotweb/assets"
BIN="$REPO/plotweb-web/target/release/plotweb-web"
LAUNCHER="$REPO/scripts/plotweb-desktop.sh"

# ── uninstall ────────────────────────────────────────────────────────────────
if [ "${1:-}" = "--uninstall" ]; then
  rm -fv "$DESKTOP_FILE"
  for px in 128 192 512; do
    rm -fv "$DATA_HOME/icons/hicolor/${px}x${px}/apps/$ENTRY.png"
  done
  rm -rfv "$ASSET_DIR"
  # rinch's own per-launch stub + icon; they come back on the next run.
  rm -fv "$DATA_HOME/applications/$APP_ID.desktop" "$DATA_HOME/rinch/icons/$APP_ID.png"
  command -v update-desktop-database >/dev/null && update-desktop-database "$DATA_HOME/applications" || true
  echo "Uninstalled. Local documents under $DATA_HOME/plotweb/docs were left alone."
  exit 0
fi

# ── build ────────────────────────────────────────────────────────────────────
if [ "${1:-}" != "--no-build" ]; then
  echo "Building release binary..."
  ( cd "$REPO/plotweb-web" && cargo build --release )
fi

if [ ! -x "$BIN" ]; then
  echo "No release binary at $BIN" >&2
  echo "Build it first:  cd plotweb-web && cargo build --release" >&2
  exit 1
fi

# ── icons ────────────────────────────────────────────────────────────────────
# Into the user's hicolor theme, where the `Icon=plotweb` name below resolves from.
# Sized directories rather than a single absolute path so the compositor can pick
# the resolution it wants (taskbar, alt-tab, and window decoration differ).
install -Dm644 "$REPO/plotweb-web/icons/icon-512.png" "$DATA_HOME/icons/hicolor/512x512/apps/$ENTRY.png"
install -Dm644 "$REPO/plotweb-web/icons/icon-192.png" "$DATA_HOME/icons/hicolor/192x192/apps/$ENTRY.png"
install -Dm644 "$REPO/plotweb-web/favicon.png"        "$DATA_HOME/icons/hicolor/128x128/apps/$ENTRY.png"

# ── in-app assets ────────────────────────────────────────────────────────────
# `platform::asset_src` looks here first. Installing the copy means the app does not
# read images out of the checkout, and the logo still renders with no network.
install -Dm644 "$REPO/plotweb-web/assets/logo.png" "$ASSET_DIR/logo.png"
install -Dm644 "$REPO/plotweb-web/assets/icon.png" "$ASSET_DIR/icon.png"

# ── launcher entry ───────────────────────────────────────────────────────────
mkdir -p "$DATA_HOME/applications"
cat > "$DESKTOP_FILE" <<EOF
[Desktop Entry]
Type=Application
Name=PlotWeb
GenericName=Fiction Writing
Comment=Write and organise fiction, offline-first
Exec=$LAUNCHER
Icon=$ENTRY
Terminal=false
Categories=Office;WordProcessor;
Keywords=writing;fiction;novel;manuscript;
StartupNotify=true
# Binds the running window (Wayland app_id / X11 WM_CLASS) to this entry, so the
# taskbar shows this icon and groups the window under this launcher.
StartupWMClass=$APP_ID
EOF
chmod 644 "$DESKTOP_FILE"

command -v update-desktop-database >/dev/null && update-desktop-database "$DATA_HOME/applications" || true
command -v gtk-update-icon-cache  >/dev/null && gtk-update-icon-cache -f -t "$DATA_HOME/icons/hicolor" 2>/dev/null || true

echo
echo "Installed:"
echo "  launcher   $DESKTOP_FILE"
echo "  icons      $DATA_HOME/icons/hicolor/*/apps/$ENTRY.png"
echo "  assets     $ASSET_DIR"
echo "  runs       $LAUNCHER  ->  $BIN"
echo
echo "Search your launcher for \"PlotWeb\". Server: ${PLOTWEB_SERVER:-https://pw.lostconnection.dev}"
echo "Logs: ${XDG_STATE_HOME:-$HOME/.local/state}/plotweb/plotweb.log"
