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

## Spike ① results (editor swap) — 2026-07-14

Validated at `/editor-spike` (flag-gated dev route, commit `6dadc2a`):

- **Zero new dependencies** — `rinch-web` already links `rinch-editor-view`/
  `-core` and re-exports `Editor`/`EditorHandle`/`create_editor`. Compiled
  clean in PlotWeb's Trunk build on the first try.
- **Content path works** — a PlotWeb chapter (markdown → `markdown_to_html` →
  the editor's `load_html`) rendered with full fidelity: bold/italic/`code`
  marks, H1/H2, bullet + ordered lists, blockquote. No contenteditable, no
  `set_inner_html`.
- **Edit loop works** — caret placement + toolbar `handle.command(...)` →
  model transform → re-render, verified in-browser (H1→H2 demotion).
- **Parity is high.** Covered: bold/italic/underline/strike/code/**highlight**/
  sub/sup marks; paragraph, H1–H6, code block, blockquote; **bullet/ordered/
  task lists**; HR, tables, undo/redo — a superset of PlotWeb's current toolbar.
- **Gaps for Phase 1:** (a) **text-alignment** has no editor command (schema
  gap — PlotWeb's `{align:…}`); needs a paragraph-align attr + command upstream.
  (b) **Links/images** are supported as a mark/node but via arg-based handle
  calls, not bare command strings — wiring, not missing capability.
- **Lists clarification:** lists edit fine *in the editor*; the flat-text limit
  is only in `rinch-editor-collab`'s Automerge projection, so it's a **Phase 2
  (sync)** concern, not a blocker for adopting the editor in Phase 1.

## Spike ② results (Automerge persistence) — 2026-07-14

Validated at `/opfs-spike` (commit `d2f8a82`):

- **Automerge runs in the PlotWeb wasm build.** `automerge v0.5.12` compiled to
  `wasm32-unknown-unknown` cleanly via rinch-web's `collaboration` feature; the
  only extra plumbing is `uuid`'s `js` feature (actor-id randomness). No
  getrandom config needed.
- **The persistence round-trip works.** Editor doc →
  `EditorHandle::start_collaboration_host` → a **720-byte Automerge snapshot** →
  persisted → **page reload** → `start_collaboration_guest` → editor doc, with
  content + marks intact. Verified in-browser ("Saved 720 bytes" → reload →
  "Restored 720 bytes").
- **OPFS via web-sys is blocked — actionable finding for `rinch-storage`.** OPFS
  write handles (`FileSystemFileHandle`/`FileSystemWritableFileStream`) are
  behind `web_sys_unstable_apis`, and enabling that cfg globally **fails to
  compile rinch**: `rinch/src/render_surface.rs:1312` passes `f64` to
  `put_image_data`, whose *unstable* web-sys signature wants `i32`. So the spike
  uses **localStorage** as the sink (identical Automerge/persist/reload path).
  `rinch-storage` must either (a) land the one-line rinch fix so the unstable
  cfg compiles, or (b) bind OPFS through manual `wasm-bindgen`/`js-sys` (avoiding
  the global cfg). Prefer (a) — it's trivial and OPFS is the right backend.
- **Build-weight note:** the `collaboration` feature adds Automerge to the prod
  bundle. Gate both Phase-0 spike routes behind a cargo feature (or remove them)
  once Phase 1/2 subsumes them.

## Spike ③ results (Automerge sync over HTTP) — 2026-07-14

Validated at `/sync-spike` (commit `3eae484`):

- **The sync transport works.** Editor A projected its doc onto a CRDT
  (`start_collaboration_host`) and pushed a **506-byte Automerge snapshot** to a
  dumb relay server over `fetch`; "Pull into B" fetched it and reconstructed A's
  document in editor B (`start_collaboration_guest`). Verified in-browser: "B
  converged to A." Confirms the C→S→C round-trip carrying opaque CRDT bytes both
  directions.
- **The server can be a dumb relay.** It stores a snapshot + append-only delta
  log per doc id and never runs Automerge — the clients merge. The real server
  will *persist* canonical Automerge (blob store), but the transport shape (post
  bytes / fetch bytes) is exactly this. Good news for `rinch-http`: the existing
  `fetch`-based JSON transport already carried the (hex-encoded) CRDT bytes.
- **Delta relay is wired** through the same proven POST path (`outbound` →
  `POST /api/sync/{id}/delta`); live-edit deltas travel identically to the
  snapshot. (Driving the model-first editor's keyboard input via the test
  harness proved finicky, so the in-browser demo exercised the snapshot path;
  the delta path shares the same transport.)

**Phase-0 verdict: all three spikes green.** The editor, offline persistence,
and HTTP sync are each validated end-to-end. The upstream gaps are now precisely
scoped: `rinch-storage` (with the OPFS/`web_sys_unstable_apis` rinch fix),
`rinch-http`, and the `rinch-editor-collab` lists projection. Next: **lock the
Automerge doc schema** (the last Phase-0 card), then Phase 1.

## Locked Automerge schema (v1)

Finalized after the Phase-0 spikes. This supersedes the sketch in "Data model"
above with concrete shapes. **Sync unit = one Automerge document**; each has a
stable **doc-id** `"{type}:{uuid}"` (uuids stay v4, as today — they're already
the keys in URLs, blob paths, and rhypedb). Four doc types:

### 1. `user:{user_id}` — the account index (offline dashboard)
```
{
  books: Map<book_id, {
    title:      String,     // cached for the dashboard (authoritative in book-index)
    cover_ref:  String?,    // content-addressed image hash
    updated_at: String,     // "YYYY-MM-DD HH:MM:SS", lexicographically sortable
  }>
}
```
One per account. Lets the dashboard list the user's books with no network.

### 2. `book:{book_id}` — book structure & metadata
```
{
  meta: {
    title:         String,
    description:   String,
    font_settings: String,   // JSON blob (v1: whole-value; → Map for per-field merge later)
    cover_ref:     String?,  // content-addressed image hash
    created_at:    String,
  },
  chapters:        List<chapter_id>,          // AUTHORITATIVE order
  chapter_titles:  Map<chapter_id, String>,   // title per chapter
  notes: {
    root_order:  List<note_id>,
    children:    Map<note_id, List<note_id>>, // parent → ordered child ids
    collapsed:   Map<note_id, bool>,
    titles:      Map<note_id, String>,
    colors:      Map<note_id, String>,
  }
}
```
**Titles/colors/order live here, not in the body docs.** Rationale: the
sidebar, reorder, and notes-tree must render and mutate from *one* doc without
loading every body; and keeping order (a `List<id>`) decoupled from titles (a
`Map`) lets a concurrent reorder and a title edit merge cleanly. The editor's
**inline title field** is a plain text control bound to
`chapter_titles[chapter_id]` — it is *not* part of the rich-text CRDT.

### 3. `chapter:{chapter_id}` — the chapter body
A **`rinch-editor-collab` CRDT** (blocks list · per-block Automerge `Text` ·
marks). We do **not** hand-roll this projection — the editor owns it (the same
one Spike ② snapshotted / Spike ③ synced). Holds prose only. Inline images are
`image` nodes referencing a content-addressed **hash** (not bytes). Requires the
`rinch-editor-collab` **lists** extension before real prose adopts it.

### 4. `note:{note_id}` — the note body
Same body-CRDT as a chapter. Its title/color/tree-position live in the parent
`book:` doc (structure), so the notes pane renders without loading note bodies.

### Images — content-addressed blobs (not CRDTs)
An image is `sha256(bytes)` → bytes, stored in the local blob store and synced
to the server blob store as an **opaque, immutable, deduplicated** blob (upload-
by-hash). Referenced by hash from `meta.cover_ref` and from `image` nodes inside
body docs. Never embedded in an Automerge doc.

### Lifecycle / deletion
Automerge has no "delete doc". Deleting a chapter = remove it from the `book:`
doc's `chapters` list + `chapter_titles`, and mark its `chapter:` doc for
removal (dropped locally + a tombstone told to the server, which GCs the blob).
Notes delete recursively over `children`. A doc referenced by no index is
collectable.

### Server side (unchanged split, new content form)
- **Canonical Automerge bytes** per doc-id → filesystem/rhypedb blob store.
- **rhypedb** keeps the index (which docs belong to which book/user), ownership,
  auth, and beta metadata — as today.
- **Beta "frozen snapshot"** = pin the Automerge **heads** of the `book:` doc +
  its `chapter:` docs on the `BetaLink`; the server materializes at those heads.
- **Export / beta read / history** materialize a body doc → `DocNode` →
  markdown/HTML/DOCX (history = diff two `heads`).

### v1 simplifications (revisit later)
- `font_settings` as a JSON string (whole-value LWW) rather than a merged `Map`.
- `user:` `books` as a `Map` (no user-defined dashboard order) — sort by
  `updated_at`.
- Task-list / table blocks exist in the editor but ride on the
  `rinch-editor-collab` lists/nested extension landing first.

## The web_sys elimination target

The frontend's ~163 `web_sys` sites resolve as: the **69 DOM/`set_inner_html`
sites collapse into `rinch-editor-view`** (the editor/reader own their rendering);
**fetch/WS/routing/fonts** move behind cross-platform seams; **image FormData/
object-URLs** become blob-store ops. What remains web-only (and acceptable) is a
thin platform layer, not app logic.
