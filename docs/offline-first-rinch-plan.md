# PlotWeb → Offline-First, Pure-Rinch, Cross-Platform: Plan

_A comprehensive plan to rebuild PlotWeb's frontend entirely in rinch (compilable
to web / desktop / android), make it work 100% offline, and sync when online._

Status: planning. Date: 2026-07-14. Supersedes the "true-native" path in
[`native-builds-assessment.md`](./native-builds-assessment.md).

## Locked decisions

1. **Collaboration:** single-user, multi-device sync (offline-first personal
   sync). Automerge foundation stays collab-ready, but no live multi-author /
   presence / cursors now.
2. **First native target:** desktop (macOS/Win/Linux), alongside the web PWA.
   Android deferred (its editor-input glue is a separate later contribution).
3. **Auth:** token-based, cached locally (open + edit fully offline; re-auth on
   sync). Beta-reader links stay token-scoped as today.
4. **Storage:** **drop server-side git.** Automerge CRDT is the single source of
   truth; the features git provided become CRDT-native (see §6).

## Does a PWA work 100% offline? Yes.

Two independent pieces, and they're the *same* pieces native needs:

- **App shell offline** — the service worker (`plotweb-web/sw.js`) precaches the
  wasm bundle + assets. Already mostly there (it does app-shell + hashed assets;
  today it deliberately never caches `/api/*`). Once data is local, that's enough
  to *load* offline.
- **Data offline** — the real work: data must live **on the client** (OPFS/
  IndexedDB in the browser, filesystem on native), with a sync engine that
  reconciles with the server when online. This is identical to native offline-
  first, which is exactly why unifying them in rinch is the right call. The
  web/native difference collapses to *which local-storage backend* the sync
  engine writes to.

## What rinch already gives us (and what it doesn't)

**Already first-party — the two hardest pieces:**

- **Rich-text editor** — `rinch-editor-core` (ProseMirror-style model: blocks +
  inline text + marks; invertible steps; history) + `rinch-editor-view`
  (`EditorHandle`/`Editor`, **model-first, no contenteditable**). Runs on **web
  and desktop** from one codebase (`examples/editor-web`). Adopting it deletes
  PlotWeb's worst web-coupled code: `execCommand`, the hand-rolled
  markdown↔HTML converters, `sanitize_html`, `set_inner_html`, and the
  `SWITCH_GEN`/`editor_loaded`/rAF-readiness save machinery.
- **Offline-first CRDT** — `rinch-editor-collab`: an **Automerge** layer with a
  **transport-agnostic byte seam** (`start_collaboration_*`, `collab_receive`,
  `save_incremental`/`snapshot`, full sync protocol), wasm-compatible, demoed
  in-browser (`examples/collab-editor-web`). This is the sync engine. Automerge
  auto-merges concurrent prose edits — **we never show an author a merge
  conflict.**
- **Cross-platform WebSocket** — `rinch-signaling` (`WebSocketSignaling`, native
  tungstenite / wasm `web_sys`), for optional live sync.
- **Cross-platform clipboard** — `rinch-clipboard`.

**Missing — we build these (upstream, since they're broadly useful):**

- **Persistence** — no OPFS/IndexedDB/filesystem abstraction anywhere in rinch.
- **HTTP client** — no cross-platform fetch/reqwest wrapper (only desktop image
  loading via `ureq`).
- **CRDT projection breadth** — `rinch-editor-collab` covers flat text-blocks +
  marks today and **fails loud** on lists/nested blocks; prose needs lists.
- **Router** — none (apps roll their own signal + match).

**Ruled out:** using **rhypedb as the client store**. Its storage engine is
hardwired to `mmap` + `std::fs` + background threads with no VFS seam, so a
`wasm32` build is itself a large refactor. rhypedb stays **server-side** for
auth/ownership/beta metadata.

## Target architecture

```
ONE rinch codebase → web (PWA) / desktop / (android later)
┌──────────────────────────────────────────────────────────────┐
│ UI      rsx! + components + rinch-editor-view (rich text)      │
│ State   rinch Signals (AppStore)                               │
│ Content Automerge CRDT docs — per chapter / note / book-index  │
│         editor edits the CRDT directly; offline edits queue    │
│ Local   rinch-storage: OPFS (web) / filesystem (native) ◄─ NEW │
│ Sync    Automerge sync protocol over HTTP (periodic) ◄──── NEW │
│         transport: rinch-http (fetch/reqwest) ◄────────── NEW  │
│         optional live WS via rinch-signaling (already x-plat)  │
└───────────────────────────────┬──────────────────────────────┘
              Automerge sync messages (opaque bytes) + auth token
┌───────────────────────────────┴──────────────────────────────┐
│ Server (Axum)                                                  │
│  Sync service: canonical Automerge doc per document ◄──── NEW  │
│  Doc blob store: per-doc Automerge bytes on the filesystem     │
│  Auth / ownership / beta metadata: rhypedb (unchanged)         │
│  Export / history / diff / beta-snapshot: CRDT-native (§6)     │
│  Beta-reader surface: online (materialized snapshot + WS)      │
└──────────────────────────────────────────────────────────────┘
```

## Data model

Everything the client owns is an **Automerge document**; the sync unit is the
document. Granularity chosen to match how the editor loads (one chapter at a
time) and to keep sync deltas small:

- **Book-index doc** (one per book): `title`, `description`, `font_settings`,
  `cover_image` ref, **`chapter_order: List<chapter_id>`**, chapter titles,
  **notes tree** (`root_order`, `children`, `collapsed`) + note titles.
- **Chapter doc** (one per chapter): the rich-text body as a `rinch-editor-core`
  model projected into Automerge (blocks of inline text + marks). This is what
  `rinch-editor-view` edits directly.
- **Note doc** (one per note): title + rich-text body (same projection).
- **User-index doc** (one per account): the list of books the user owns (so the
  dashboard works offline). Syncs like any other doc.
- **Images**: content-addressed blobs (hash → bytes) in the local blob store;
  referenced by hash from chapter/note docs; synced as opaque blobs (not CRDT).

Content format: the durable shape is `rinch-editor-core`'s `DocNode` JSON (blocks
+ marks), **not markdown**. Markdown/HTML/DOCX become **export/import
transforms** only (the editor core already serializes to markdown/HTML), so the
lossy hand-rolled converters and the `<u>/<mark>/<a>`-preservation hacks go away.

## Sync design

- **Client:** on sign-in + connectivity, for each local doc run Automerge's sync
  protocol (`generate_sync_message` ↔ `integrate_sync_message`) with the server,
  keeping per-doc `SyncState`. Edits are **local-first**: apply to the local
  Automerge doc immediately (instant, offline), persist the incremental change to
  `rinch-storage`, and let the sync loop ship changes opportunistically.
- **Transport:** **periodic HTTP sync** — `POST /api/sync/{doc_id}` exchanging
  opaque Automerge sync-message bytes (the collab byte seam over `rinch-http`).
  Optional upgrade to a live WebSocket (`rinch-signaling`) while an editor is
  open, for near-real-time multi-device.
- **Server:** stores the **canonical Automerge doc per document** (bytes on the
  filesystem blob store, keyed by doc uuid; rhypedb holds the index: which docs
  belong to which book/user + current heads). Runs the sync protocol; is a
  relay + durable store, not a CRUD API.
- **Conflict handling:** none needed at the app layer — Automerge converges.
  Single-user-multi-device means conflicts are rare (same author, two devices)
  and always auto-merged.
- **Auth:** a locally-cached token (issued at online login/registration). The app
  opens and edits offline with the cached identity; the sync loop attaches the
  token and re-auths when online. Registration/first login require connectivity.

## Feature migration (how each current thing maps)

| Today | After |
|---|---|
| `contenteditable` + `execCommand` + markdown↔HTML + `set_inner_html` | `rinch-editor-view` editing an Automerge-backed model; `DocNode` JSON is the format |
| `api.rs` fetch (REST CRUD per edit) | local Automerge writes + background `rinch-http` sync of opaque bytes |
| `ws.rs` feedback WebSocket | `rinch-signaling` WS (beta feedback stays; optional live doc sync) |
| `router.rs` History/PopState | app-level signal + `match` router (or a small `rinch-router` contribution) |
| `fonts.rs` Google Fonts load | bundled fonts (native has no browser font loader) |
| **Server git** (manuscript/notes repos) | **dropped**; Automerge is the store |
| Git history / `list_commits` / restore | Automerge change history: browse by change metadata (time/actor); "restore" = fork from past `heads` |
| Git diff endpoint | materialize doc at two `heads` → text diff |
| Export (`plotweb-export`) | materialize `DocNode` → markdown/DOCX/… (source is the CRDT, not git) |
| Beta "frozen at commit" (`pinned_commit`) | pin the Automerge **version (heads hash)** on the `BetaLink`; server materializes the doc at those heads |
| rhypedb metadata (users/books/beta) | **unchanged**, server-side |
| SQLite sessions | replaced by token auth for the app (beta links stay token-scoped) |
| Reader CSS-column pagination | keep on web; native paged reading needs a rinch layout-measurement pass (Phase 3) |
| Images: multipart upload + `<img>` insertHTML | content-addressed blob in local store; synced as a blob; referenced by hash |
| No client persistence (in-memory Signals) | `rinch-storage` (OPFS/fs) is the durable local copy; Signals become a view over it |

## Upstream rinch contributions (desktop-first scope)

1. **`rinch-storage`** — cross-platform persistence trait + backends: OPFS (web,
   sync file handles in a worker — ideal) / IndexedDB fallback; filesystem
   (native). Highest-value gap.
2. **`rinch-http`** — cross-platform HTTP client: `web_sys::fetch` (wasm) /
   `reqwest` or `ureq` (native), behind one API.
3. **`rinch-editor-collab` projection extension** — cover lists (and any nested
   structure PlotWeb uses) so prose docs don't hit `CollabError::Unsupported`.
4. **`rinch-router`** (optional, small) — signal-backed routing usable on both
   backends.
5. **(Deferred)** Android editor input glue; native reader pagination primitive.

## Phased roadmap

**Phase 0 — Spikes (de-risk the unknowns).**
- Replace the chapter editor with `rinch-editor-view` behind a flag in the web
  build; validate formatting/undo/images parity.
- Round-trip an Automerge doc client↔toy-server over HTTP (`rinch-http` seam).
- Persist + reload an Automerge doc via OPFS in the wasm build (`rinch-storage`
  spike).
- Lock the doc schema (per-chapter + per-book-index + user-index) and the
  `DocNode` mapping.

**Phase 1 — Pure-rinch frontend, still online.**
- Adopt `rinch-editor-view` app-wide; content becomes `DocNode`; retire the
  markdown↔HTML/execCommand/`set_inner_html` machinery. Server temporarily
  stores `DocNode` JSON (git can be materialized during transition or dropped
  now per the decision).
- Replace `api.rs`/`ws.rs`/`router.rs`/`fonts.rs` with cross-platform seams
  (`rinch-http`, `rinch-signaling`, app router, bundled fonts).
- **Milestone:** PlotWeb compiles + runs as a **native desktop app** against the
  hosted server (online). Proves the cross-platform frontend end-to-end.

**Phase 2 — Local-first + sync.**
- Build `rinch-storage`; make the client read/write local Automerge docs;
  editor edits the CRDT directly; Signals become a view over local state.
- Build the sync engine + server sync endpoints; server stores canonical
  Automerge blobs + rhypedb index; token auth.
- Service worker: precache the full app shell (data is already local).
- **Milestone:** **100% offline** on web (PWA) and desktop; syncs when online.

**Phase 3 — Native packaging & polish.**
- Desktop installers (`.dmg`/`.msi`/AppImage); native reader pagination; fonts +
  images on native; beta-reader surface reconciliation; export/import over the
  CRDT; then Android (editor glue) if desired.

## Risks & open questions

- **CRDT projection breadth** — lists/nested blocks need the projection extended
  before Phase 1 can fully adopt the editor for real prose. Size this early
  (spike). If it's large, a stopgap is flat-prose-only with lists as a fast-
  follow.
- **Automerge doc size / performance** for long books (per-chapter granularity
  mitigates; measure).
- **Dropping git loses free readable history/diff/export** — all now ride on
  Automerge primitives; confirm `heads`-based history + materialized diff/export
  meet expectations (they should, but it's net-new code vs. free-from-git today).
- **OPFS constraints** — sync access handles require a Worker context; plan the
  wasm threading/worker setup early.
- **Beta readers** stay online and server-materialized — make sure "share a
  frozen snapshot" maps cleanly onto pinned Automerge heads.
- **Migration** of existing books (git markdown → Automerge `DocNode`) — a
  one-time server-side import (parse markdown → editor model → Automerge).

## The web_sys elimination target

The frontend's ~163 `web_sys` sites resolve as: the **69 DOM/`set_inner_html`
sites collapse into `rinch-editor-view`** (the editor/reader own their rendering);
**fetch/WS/routing/fonts** move behind cross-platform seams; **image FormData/
object-URLs** become blob-store ops. What remains web-only (and acceptable) is a
thin platform layer, not app logic.
