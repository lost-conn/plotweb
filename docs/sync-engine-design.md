# Sync engine — design (Phase 2, next-step ①)

**Status:** design, not built. Written to open handoff step ① (`docs/MIGRATION-HANDOFF.md`).
Companion docs: `docs/offline-first-rinch-plan.md` (architecture + locked v1 Automerge
schema), `docs/MIGRATION-HANDOFF.md` (migration state + runbook).

---

## 1. Goal & non-goals

**Goal:** one author, several devices (web, desktop, later Android), each fully offline-
capable, converging when online. The client is already local-first for all four v1 doc
types; the server already holds a canonical Automerge snapshot per doc (phase C backfill,
`PLOTWEB_CRDT_DIR`, 92 blobs). What is missing is the thing that moves changes between them.

**Non-goals (v1):**
- Live multi-author collaboration (two people in one chapter). The CRDT supports it; the
  product doesn't need it yet, and it changes the auth/presence surface materially.
- Retiring git or making Automerge authoritative — that is phases D/E/F, *after* this.
- Beta-reader traffic. Readers stay on the existing token REST endpoints.

**Sequencing claim:** the sync engine lands while **git is still authoritative**. Sync only
moves Automerge bytes between the client local store and the server CRDT store. Nothing in
this design writes git, changes REST behaviour, or removes a dual-write. Deleting
`PLOTWEB_CRDT_DIR` and reverting the client still leaves a working v16-equivalent app.

---

## 2. What already exists (the seams this builds on)

| Piece | Where | Note |
|---|---|---|
| Client per-doc local store | `plotweb-web/src/local_store.rs` (`DocStore`) | generation + manifest pointer-flip; snapshot + delta log |
| Client body docs (`chapter:`/`note:`) | `local_store.rs` via `EditorHandle` collab seam | CRDT owned by `rinch-editor-collab`; app sees opaque bytes |
| Client structure docs (`book:`/`user:`) | `local_book.rs`, `local_user.rs` | plain `automerge::AutoCommit` — full protocol access already |
| Server canonical blobs | `crates/plotweb-server/src/backfill.rs` → `FsStore` | `{doc_id}/{snapshot,manifest,src-sha}` |
| Canonical projection | `crates/plotweb-crdt` | shared validate/emit; `automerge = "0.5"` |
| Auth | `auth.rs::AuthSession` (session cookie) | works on web (`SameOrigin` credentials) *and* native (ureq's process-wide cookie jar) |
| Spike relay | `crates/plotweb-server/src/routes/sync.rs` | **dead code that is live** — see §8 slice 0 |

---

## 3. The decisions (forks the handoff left open)

### D1 — Transport: periodic HTTP, book-scoped, binary body

```
POST /api/books/{book_id}/sync/{doc_id}     Content-Type: application/octet-stream
  body:  one Automerge sync message (raw bytes, not hex)
  200:   one Automerge sync message (possibly empty → "nothing more to send")
```

One message per request; the client loops until both sides go quiet (Automerge's protocol
is multi-round in the general case). Raw bytes, not the spike's hex-in-JSON — halves the
payload and `rinch-http` already carries binary both ways.

**Book-scoped path** rather than the plan's flat `/api/sync/{doc_id}`: it makes
authorization a lookup we already have (§D4) instead of a new global doc→owner index.
The `user:` doc, which has no book, gets `POST /api/sync/user` (implicitly the session's
own user — the id is never taken from the path).

WebSocket upgrade (`rinch-ws`, PR #114, present at the pinned rev) is deliberately **not**
in v1. HTTP polling while a doc is open is enough for single-author multi-device, and the
message shape is identical, so the WS slice later is a transport swap, not a redesign.

### D2 — Protocol: the real Automerge sync protocol (heads-based), not a change-log cursor

The considered alternative was a dumb append-only change log per doc with a client-held
cursor (works today with zero upstream changes, because the client already produces
`save_incremental` deltas and can apply them with `collab_receive`). Rejected as the
primary design: a cursor is only meaningful relative to one server log — it silently
misses changes after a store rebuild, a restored client backup, or any divergence, and it
forces the server to keep every change forever. The heads-based protocol is self-healing:
both sides converge from whatever they actually have.

**Cost of that choice:** `EditorHandle` (rinch-editor-view) exposes
`start_collaboration_host/guest`, `collab_receive`, `collab_snapshot`, `stop_collaboration`
— but **not** the sync protocol, though the underlying `CollabSession` already has
`generate_sync_message` / `integrate_sync_message` / `heads`. So body docs need a small
upstream rinch PR adding the pass-through (§8 slice 2). `book:`/`user:` docs need nothing —
they are plain `AutoCommit` in our own code.

### D3 — Auth: session cookie in v1, token slice later

`AuthSession` already works on both targets. v1 uses it unchanged.

Sessions are **SQLite-backed** (`session_layer` → `tower_sessions_sqlx_store::SqliteStore`
over the shared pool), so they survive a restart/deploy — the `CLAUDE.md` line calling the
session store in-memory is stale. Slice 1's `a_synced_doc_survives_a_server_restart` test
covers exactly this path (same cookie, rebuilt app, sync continues).

What remains true: a cookie still expires (30 days inactivity), and a browser can drop it.
So the sync loop treats 401 as a first-class state (§6) — visible, no retry-storm, resumes
after the next login — rather than an error. The cached-token slice from the plan's "Auth"
bullet is what enables sync outside a foreground session; it stays deferred.

### D4 — Authorization: book ownership + doc membership

1. `AuthSession` gives `user_id`.
2. `book_id` → owner check (the existing rhypedb `Book.user_id` lookup every book route
   already does).
3. `doc_id` must **belong to that book**: `book:{book_id}` itself, or a `chapter:`/`note:`
   id listed in that book's git storage (pre-cutover) — checked against the same
   `list_chapters`/`list_notes` the backfill walks. An unknown doc id is `404`, never an
   implicit create. This closes the spike relay's "any string is a doc" hole.
4. `POST /api/sync/user` syncs `user:{session_user_id}` only.

### D5 — Server storage: extend the phase-C blob store, add a per-doc lock

Canonical doc per id stays exactly where the backfill put it (`PLOTWEB_CRDT_DIR`,
`{doc_id}/snapshot`). The sync handler:

```
load  {doc_id}/snapshot            → automerge::AutoCommit::load
receive_sync_message(state, msg)   → mutates the canonical doc
generate_sync_message(state)       → reply bytes
save (compacted)                   → {doc_id}/snapshot   (write-new + pointer flip, §D5.1)
```

- **Per-doc mutex**, keyed like `plotweb-git`'s per-book locks (`HashMap<String,
  Arc<Mutex<()>>>` in `AppState`). Two devices syncing one doc must serialize; Automerge
  merge is commutative but the *read-modify-write of the blob* is not.
- **Server-side `SyncState` is per-request, not persisted.** It is a bandwidth
  optimization; a fresh state each request costs an extra round trip and is always correct.
  Persisting it per (doc, device) is a later optimization, and needs a device id we don't
  have yet.
- **Two consequences of that statelessness, both verified against automerge 0.5 and both
  load-bearing for the client** (slice 1 pinned them in tests):
  1. A fresh `State` has `have_responded == false`, so the server's
     `generate_sync_message` **always** returns a message. The server can never say
     "done" — the *client* ends an exchange, when its own `generate_sync_message`
     returns `None`.
  2. **The client must also start each poll with a fresh `SyncState`.** Automerge's
     protocol assumes a live connection in which the peer *pushes*; a client that keeps
     its state across polls sees unchanged local heads plus a remembered "we agreed"
     and generates nothing — so it never learns about a change another device pushed,
     and the two devices silently stop converging. (This was caught by the
     two-devices-converge test, which failed exactly this way.) A `SyncState` here is a
     per-exchange optimization, scoped to one poll cycle and discarded — so there is no
     sync state to persist on either side.
- **`{doc_id}/manifest`** gains `heads` (hex change hashes) + `updated_at`, so a listing
  endpoint can answer "what changed" without loading every doc.
- **`src-sha` stays untouched** by sync. It is the *git-source* fingerprint owned by the
  backfill. Once a doc has been synced, the backfill must not clobber it — see §7.

**D5.1 Durability.** Same recipe as the client: write the new snapshot under a fresh key,
then flip a pointer, then sweep. A crash mid-write must never leave a truncated canonical
doc; a truncated canonical doc is the one failure in this design that loses data the client
may no longer hold.

### D6 — Discovery: sync the index docs first, then walk them

A new device (or a device that missed a chapter created elsewhere) learns what exists by
syncing **downward**:

```
user:{me}          → the set of book_ids
  book:{book_id}   → chapters[], notes.* (ids, order, titles)
    chapter:{id}, note:{id}   → bodies (lazily: on open, plus a background sweep)
```

No separate "list docs" endpoint is required for correctness. One is still worth adding for
efficiency —

```
GET /api/books/{book_id}/sync/heads → { doc_id: heads[] }
```

— so the client can skip docs whose server heads it already has instead of paying a
round trip per doc. Not required for v1 correctness; ship if the poll cost bites.

### D7 — Deletion

Automerge has no doc delete. v1 follows the locked schema: removal from the parent index
(`book:` doc's `chapters` list / notes tree) **is** the deletion; the orphaned body doc is
dropped from the local store and left server-side. Server-side GC of unreferenced blobs is
deferred to phase F, where git retirement forces the question anyway.

### D8 — Provenance: the duplicate-content trap (**the highest-risk part of this design**)

Today the client seeds a local doc from REST content, and the server's canonical doc was
seeded independently by the backfill from the *same* git content. Two Automerge docs built
independently share **no history**. Merging them does not deduplicate — it concatenates.
The first naive sync of a migration-era doc would show the author their chapter **twice**.

Rule, per doc, at first sync:

| Local origin | Server has canonical? | Action |
|---|---|---|
| `seeded-local` (built from REST/git content) | yes | **Adopt the server doc wholesale** — discard the local doc, publish the server snapshot as a new generation, then sync normally forever after. Safe pre-cutover: every local edit also dual-wrote to git, so the server's git-derived doc already contains it. |
| `seeded-local` | no (flagged doc, or created offline) | **Push local as canonical** — server adopts the client's doc as the initial canonical. |
| `synced` (already shares history) | yes | Normal sync protocol. |

Implementation: `DocStore`'s manifest gains `origin: "seeded-local" | "synced"`, set to
`synced` the moment a doc first exchanges messages with the server. Adoption is a local
pointer-flip (existing `publish_snapshot`), so it is atomic and crash-safe.

**Server mirror of the same rule (built, and sharper than first written):** the canonical
manifest's `synced_at` is stamped on **any** successful exchange — including a pure *pull*
that writes no snapshot. First draft stamped it only when the client moved the doc, which
is wrong: once a device holds the canonical history, a git re-projection forks a disjoint
second history, and the device's next push merges the two by concatenation. Holding a copy
is what makes re-projection unsafe, not writing. A slice-1 test pins this
(`pulling_a_backfilled_doc_marks_it_client_owned`).

**Consequence to accept knowingly:** adopting the server doc discards local Automerge
*history* (not content) for migration-era docs. Pre-cutover that history is redundant — git
holds the real history. This rule must be revisited before any doc is created
Automerge-first post-cutover; then `seeded-local` no longer occurs.

---

## 4. What the client does

Per doc, on open and then on a timer while online — one **poll cycle**:

1. Ensure local doc exists (existing seed/adopt path) and apply §D8 provenance.
2. `let mut sync_state = SyncState::new()` — **fresh for this cycle** (§D5); never reused
   across cycles, never persisted.
3. `msg = generate_sync_message(&mut sync_state)`; if `None`, the cycle is over.
4. `reply = POST …/sync/{doc_id}` with `msg`.
5. `integrate_sync_message(&mut sync_state, reply)`; changes land in the live CRDT — for a
   body doc that means the open editor updates, exactly like `collab_receive` today.
6. Persist: the sync-integrated state goes through the same `DocStore` generation flip.
7. Loop to 3. Termination is ours alone: the server always replies.

Cadence: on doc open, on a ~5s debounce after a local edit settles, on a slow background
tick (~60s) for docs not open, and on regaining connectivity. Deliberately unaggressive —
single-author sync has no latency requirement; a WS upgrade covers it if that changes.

---

## 5. Files this touches

- **new** `crates/plotweb-server/src/sync.rs` — canonical store + protocol (the handler is
  thin; this holds load/merge/save + the per-doc lock).
- **rewrite** `crates/plotweb-server/src/routes/sync.rs` — the spike relay becomes the real
  authorized endpoint.
- `crates/plotweb-server/src/lib.rs` — routes + per-doc lock map in `AppState`.
- **new** `plotweb-web/src/sync.rs` — the client loop + per-doc `SyncState` + backoff.
- `plotweb-web/src/local_store.rs` — manifest `origin`, adopt-server-snapshot path, a
  sync-integration entry point next to `collab_receive`.
- `local_book.rs` / `local_user.rs` — expose their `AutoCommit` to the loop.
- **upstream rinch** `rinch-editor-view/src/handle.rs` — the `EditorHandle` sync
  pass-through (slice 2), then a repin here.

---

## 6. Client sync loop as a state machine (SMDP)

States: `Idle` · `Syncing{doc, round}` · `Backoff{until, attempt}` · `Unauthed` · `Offline`.

| From | Event | To | Notes |
|---|---|---|---|
| Idle | tick / local edit settled / doc opened | Syncing | one doc at a time; queue the rest |
| Syncing | 200 + reply integrated, we still have something to send | Syncing (round+1) | cap rounds (e.g. 8) → Backoff, log |
| Syncing | our `generate_sync_message` returns `None` | Idle | converged for this doc — the only real end condition (§D5) |
| Syncing | network error | Offline | no error toast; this is normal |
| Syncing | 401 | Unauthed | stop the loop, surface it, resume on login |
| Syncing | 403/404 | Idle (doc quarantined) | never retry-storm a doc that isn't ours/doesn't exist |
| Syncing | 5xx / malformed reply | Backoff | exponential, jittered, capped ~5 min |
| Offline | connectivity regained / next tick | Syncing | |
| any | app closed mid-round | — | **no state to lose**: local writes are already durable; the cycle's `SyncState` is disposable by construction |

Disruptions handled explicitly: refresh/quit mid-sync (nothing to recover), server restart
(new server `SyncState`, extra round trip, converges), duplicate delivery (Automerge dedups
by change hash), a local edit landing *during* a sync round (next round carries it — never
block the editor), two devices syncing the same doc at once (server per-doc mutex
serializes; both converge).

---

## 7. Interaction with the migration tooling

- The **backfill** (`PLOTWEB_BACKFILL_ON_BOOT`) re-projects from git whenever `src-sha`
  changes. Once a doc has been synced, re-projecting from git would create a *second*
  independent history and re-introduce the §D8 trap on the server side. **Slice 1 must make
  the backfill skip any doc whose manifest records a sync** (`synced_at` present), and say
  so in the summary output. Until that guard exists, do not run backfill-on-boot together
  with a live sync endpoint.
- The **audit** stays valid and read-only; it validates git→Automerge projection, which is
  unrelated to sync traffic.
- **Phase D (shadow read)** gets easier after this, not harder: the canonical doc is now
  also being updated by clients, so divergence logging compares git against a doc that
  actually moves.

---

## 8. Slices (each shippable, verifiable, reversible)

Status: **slices 0–5 are done**, as is handoff step ② (`user:` backfill). Slice 2 merged
upstream as rinch PR #182 and was then superseded by #190's move to yrs; bodies now use
that engine's seam instead (§8b). Slice 6 (WebSocket transport) remains optional.

**Slice 0 — remove the spike relay.** ✅ `routes/sync.rs`'s three routes are live in
production, unauthenticated, and back an unbounded process-global `HashMap` any anonymous
caller can grow. Delete the routes (the spike proved its point in Phase 0 and is recorded
in the plan doc). Standalone, no dependencies, ship first.

**Slice 1 — server canonical sync endpoint.** ✅ `crates/plotweb-server/src/sync.rs` + the
authorized route + per-doc lock + manifest `heads`/`synced_at` + the backfill guard (§7).
Verified by `crates/plotweb-server/tests/sync.rs` (8 HTTP-level tests driving the real
protocol as a client) plus 6 unit tests in `sync.rs`: two devices converge, a synced doc
survives a restart, the `user:` doc syncs, a backfilled doc reaches a fresh device, the
backfill skips synced docs, another user's book / unknown doc-ids / anonymous callers are
rejected, a malformed message is a 400 that creates nothing, an up-to-date client causes no
canonical rewrite, and stale generations are swept.

Two notes for whoever builds the client: authorization returns **404, not 403**, for a book
the caller doesn't own (matching every other book route — existence isn't leaked); and the
poll-cycle `SyncState` rule in §D5 is not optional, it is the difference between converging
and silently drifting.

**Slice 2 — upstream rinch `EditorHandle` sync seam.** `collab_generate_sync_message` /
`collab_receive_sync_message` / `collab_heads` pass-throughs onto the existing
`CollabSession` methods, with a loopback test. Then repin PlotWeb. Same shape as PRs
#113–#118.

**Slice 3 — client loop for `book:` + `user:` docs.** ✅ `plotweb-web/src/sync.rs`. No
upstream dependency (plain `AutoCommit`), and it delivers the visible win first: create a
book on one device, see it on another.

Shape notes worth knowing before touching it:
- **Callbacks, not futures.** `rinch_http` is callback-based on both targets, and the
  local-first `spawn` is a *single-poll* driver natively (built for storage futures that
  resolve immediately — it would drop a future that actually pends). So an exchange is an
  explicit callback chain, each reply scheduling the next round.
- **Timers** are `rinch_core::set_timeout` (cross-platform: `window.setTimeout` on web, the
  shared timer thread natively). Poll every 20s, debounce local changes 1.5s, exponential
  backoff 5s→5min on failure.
- **Seams**: `local_user`/`local_book` expose `with_*_doc` (no persist — generating a
  message mutates protocol state, not content) and `persist_*` (after a merge), plus
  `open_*_id` so a cycle for a book the user navigated away from is abandoned. After a
  merge the engine re-projects into the render signals, so a remote change appears without
  a reload.
- **401 is a state, not an error**: the engine parks in `Unauthed` until the next sign-in
  re-registers it. 403/404 unregisters the doc. Status 0 (offline) backs off.
- **Off by default** — `PLOTWEB_SYNC=1` natively, `localStorage["plotweb_sync"] == "1"` on
  web. Every entry point is a no-op when off.
- **The projections had to change too** (found by the native drive, not by a test): both
  `project_chapters` and `project_notes` used the CRDT only to *refine* REST-fetched
  records — a chapter or note whose id existed solely in the synced `book:` doc was
  silently dropped from the sidebar. They now **materialize** a record from what the doc
  knows (id · title · order · colour) when REST has none. The `book:` doc is the authority
  on which chapters and notes exist; bodies arrive separately. Without this, structure sync
  is invisible, which is the whole point of the slice.

Proven natively (rinch MCP, two app instances with separate local stores against one
server): book created on A → appears on B; book created on B → appears on A; and, the
decisive one, with **both devices sitting on the same open book**, a chapter added on A
appeared in B's sidebar and chapter list **in place** — no navigation, no REST refetch.

**Slice 4 — client loop for `chapter:`/`note:` bodies.** Needs slice 2. Includes §D8
provenance adoption. Proven natively: edit on device A offline, reconnect, device B shows it.

**Read the chapter-crosstalk fix before starting slice 4** (`editor_utils::detach_before_load`
+ `local_store`'s surface binding, shipped as the v17 hotfix). Bodies live behind an editor
whose attached session records *any* document change into that document's CRDT — a load
included. Two invariants that fix established, which body sync must keep:

- **Never load into an editor that still holds another document's session.** Detach first.
  Applying a remote body change is the sync engine's own version of this hazard: integrate
  through the *attached* session (`collab_receive_sync_message`), never by loading content
  into the editor behind the session's back.
- **Re-check the binding after every await.** Anything asynchronous that ends by touching
  the editor must confirm the surface still holds the document it started with, or abandon
  itself. A sync round trip is exactly this shape.

Note also that both defects were invisible natively (the storage futures resolve on first
poll, so those windows never open) — so slice 4 needs browser verification, not only the
usual native MCP drive.

**Slice 5 — `heads` listing + background sweep.** ✅ `GET /api/books/{id}/sync/heads`
returns one map of doc-id → heads, so a sweep costs a single request for a quiet book
instead of one per chapter. Every 60s the client reconciles the open book's bodies that
no editor holds, driving them "headless" (the stored document loaded as a plain
`AutoCommit`, no editor involved) and fetching outright any it has never stored.

Two rules keep it safe:

- **The listing reports only client-owned documents.** A pristine backfill blob is
  frozen at backfill time while git kept moving, so a device that cached one would show
  backfill-era text — and, since a local body doc wins over the REST copy on open, would
  then autosave that stale text over current content. Such a document becomes shareable
  only once a device claims it via `adopt`, which republishes it from git-current
  content.
- **The sweep never runs the §D8 handshake.** Settling provenance can replace editor
  content, which is not something a background pass should do; a locally-seeded document
  waits until its author opens it.

**Slice 6 — optional WS live transport** (`rinch-ws`). Transport swap only.

Reversibility: slices 1–2 are additive (a new endpoint, an upstream method). Slices 3–4 sit
behind a client flag (`PLOTWEB_SYNC=1` natively / a build flag on web) until proven; off =
today's behaviour exactly.

---

## 8b. Two CRDTs, and why

rinch #190 replaced Automerge with **yrs** inside `rinch-editor-collab`, so the engine
behind a chapter or note body changed under us. PlotWeb now runs both, split by what
owns the document:

| Documents | CRDT | Owned by | Reconciles via |
|---|---|---|---|
| `user:` / `book:` (structure) | Automerge | PlotWeb (`local_user` / `local_book`, `plotweb-crdt`) | Automerge sync protocol |
| `chapter:` / `note:` (bodies) | yrs | the editor (`rinch-editor-collab`) | state vector + diff |

The server routes on the doc-id prefix; nothing else needs to know. Porting the
structure docs to yrs as well would buy consistency and little else — they are ours,
they work, and their protocol is already proven.

**The body exchange is two fixed steps**, not Automerge's loop: `POST .../sync/{doc}`
carries the client's state vector and returns `[u32 LE length][diff][server state
vector]`; the client applies the diff and posts what the server lacks to
`POST .../sync/{doc}/update`. This suits a stateless server *better* than Automerge
did — there is no per-peer state to fake, so §D5's "fresh `SyncState` per poll" rule
simply does not arise for bodies.

**A yrs state vector is not a fingerprint.** It counts insertions, so a delete-only
change (or a mark removal, which the engine implements by deleting format markers)
leaves it untouched. Where Automerge would compare heads — the sweep's "did this
move?" check — bodies compare a hash of the encoded document instead
(`sync::body_fingerprint`).

**The sweep is pull-only for bodies**, and can be: editing a body requires its editor,
so a body no editor holds cannot be ahead of the server, only behind. That removes any
need for a CRDT library on the client at all — every byte crossing the `EditorHandle`
seam is opaque.

**Migration fallout, deliberate:** body blobs written by the phase-C backfill under
Automerge are undecodable as yrs. `sync::load_body_doc` treats such a blob as absent,
so the first client to sync simply claims the document (§D8) and overwrites it, and a
re-run of the backfill emits yrs. Local body docs from the Automerge era are discarded
by the `BODY_STORE_VERSION` bump. Nothing is lost: git remains authoritative and every
body re-seeds from it.

## 9. Open questions

1. **Session-store durability** — do we persist sessions (or ship the token slice) before
   sync goes on by default? Background sync against an in-memory session store is a slow
   leak of "why isn't it syncing".
2. ~~**`user:` backfill ordering**~~ — **resolved: ② is done** (`backfill::run_user_backfill`,
   ownership-aware, run from the boot hook with the server's own rhypedb handle so it needs
   no downtime). Every migration-era doc is now server-seeded, so §D8 has exactly one shape.
3. **Device identity** — not needed for v1, but a stable device id unlocks persisted
   per-device `SyncState` (fewer round trips) and better diagnostics. Cheap to add now
   (random uuid in local storage), awkward to retrofit into stored state later.
