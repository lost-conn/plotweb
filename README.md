# PlotWeb

A fiction writing app for people who think Google Docs is too cheerful and Scrivener is too sane.

PlotWeb is a web-based writing environment built entirely in Rust — yes, both sides — with a contenteditable editor, per-book typography settings, and git-backed storage so your prose has better version control than most startups' production code.

## What it does

**Write things.** PlotWeb gives you a distraction-free editor with the formatting tools you'd expect (bold, italic, headings, blockquotes, lists) and a warm dark theme that won't burn your retinas at 2 AM when inspiration strikes. Chapters auto-save, so you can close the tab in a panic when someone walks by and your terrible first draft will still be there when you come back.

**Organize things.** Books live on a shelf. Chapters live in books. You can drag them around, reorder them, delete the ones that weren't working (we won't judge). The collapsible sidebar keeps your chapter list accessible without eating your writing space.

**Make it pretty.** Every book gets its own typography settings — heading fonts, body fonts, blockquote fonts, paragraph spacing, indentation. The whole Google Fonts catalog is at your fingertips, searchable and live-previewed. Want to write your fantasy epic in Macondo Swash Caps with 24px paragraph spacing? We support your creative vision.

## Architecture

The kind of stack that makes people say "wait, really?"

- **Backend:** Rust, Axum, SQLite. Sessions in memory, passwords in Argon2. Serves the frontend as a static SPA because we believe in vertical integration.
- **Frontend:** Rust compiled to WebAssembly via [Trunk](https://trunkrs.dev/). Uses [rinch](https://github.com/phaestos/rinch), a custom reactive UI framework with fine-grained signals and an `rsx!` macro. No JavaScript was harmed in the making of this application.
- **Storage:** Every book is a git repository. Chapters are JSON files. Your writing has commits, diffs, and a full history — even if you'll never look at it. SQLite just tracks who owns what.
- **Deployment:** Single Docker image, one `docker compose up` away from running on port 7919. A cron-friendly deploy script watches for remote changes and rebuilds automatically.

## Features

- **Rich text editor** with toolbar (bold, italic, underline, strikethrough, code, headings, blockquotes, lists, indent/outdent, undo/redo) and Ctrl+S saving for the anxious among us
- **Auto-save** with debouncing, plus a status indicator so you know where you stand
- **Per-book typography** — six font slots (h1, h2, h3+, body, blockquote, code), paragraph spacing, body indent, heading indent, all with live preview
- **Google Fonts integration** — searchable picker with 1,500+ fonts, cached and lazy-loaded
- **Dark and light modes** with a warm color palette that doesn't look like a hospital
- **Mobile responsive** — collapsible sidebar, hamburger menu, touch-friendly
- **Session auth** with "remember me" (stay logged in for 30 days, or don't — your call)
- **Enter to submit** on every form, because clicking buttons is for 2008
- **Git-versioned content** — every save is a commit, every book is a repo

## Running locally

You'll need two terminals and a willingness to compile Rust twice (once for your CPU, once for a CPU that doesn't exist):

```bash
# Terminal 1 — API server on :3000
cargo run

# Terminal 2 — Frontend dev server on :8080 (proxies /api/ to :3000)
cd plotweb-web && trunk serve
```

Or, if you prefer your software containerized:

```bash
docker compose up --build
# Now running on http://localhost:7919
```

## Project layout

```
crates/
  plotweb-common/    Shared types — the diplomatic middle ground
  plotweb-server/    Axum API, SQLite, session auth, font proxy
  plotweb-git/       Git-backed book/chapter storage engine
plotweb-web/         WASM frontend (separate build toolchain)
migrations/          SQLite schema (001: initial, 002: fonts, 003: git migration)
deploy.sh            Auto-deploy script for the cron-inclined
```

## Status

PlotWeb is a personal project in active development. It has users (at least one) and features (more than expected). The session store is in-memory, so server restarts log everyone out — consider it a feature that encourages frequent saving.
