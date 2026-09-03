# One writer, and document lineage (Phase 2 course correction)

**Status:** decided 2026-08-30; slices 1–5 implemented 2026-08-30/09-01 (see §6 for what
each one landed as). Supersedes the two-writer model that
phase E shipped. Written after a production data loss; read §1 before the design.

This note answers two questions the migration left open, and it narrows the system rather
than extending it. Nothing here adds a store, a protocol, or a flag — it deletes the
negotiation between two writers and replaces a destructive conflict rule with a
non-destructive one.

---

## 1. What went wrong, exactly

On 2026-08-29 a writing session (~1.4k characters, chapter `671d77fc`) was lost. It
survived only in a hand-taken snapshot of Chrome's IndexedDB write-ahead log; the
canonical document had been overwritten, git had never received the text, and the local
delta log compacted on the next page load. The chain:

1. A boot-time `reconcile --prefer git` pass rebuilt several canonical documents from git
   and cleared their ownership, exactly as `reconcile.rs` documents.
2. The rebuilt document shares no history with the copy the client holds, so the next
   exchange is answered `409` — §D8 detection working correctly.
3. The client resolves the `409` by **installing the canonical copy over its own**
   (`take_server_body` / `take_server_structure`). Local content the server had never seen
   was discarded, silently.
4. Because the client kept resetting rather than pushing, the mirror materialized
   unchanged content: ~15 `[mirror] … written to git` lines across the session, and **zero
   commits**.

§D8 already named the assumption this depended on:

> Adopting the server doc discards local Automerge *history* (not content) for
> migration-era docs. Pre-cutover that history is redundant — git holds the real history.
> **This rule must be revisited before any doc is created Automerge-first post-cutover.**

Pre-cutover, "adopt the server's copy" was safe *because every local edit had also been
dual-written to git*, so the server's git-derived document already contained it. Cutover
removed that guarantee — under cutover, sync is the path by which an edit reaches the
server at all. The rule was not revisited. "Discards history, not content" became
"discards content", and nothing in the gate list could see it: the shadow report compares
*server-side* copies to each other, and this failure happened before the server ever saw
the text.

**The lesson worth keeping:** every gate we built measures agreement between copies the
server holds. None of them ask whether a device holding unsent work survives contact with
the server.

---

## 2. Decision: one writer

For a cut-over book, **sync is the only path by which client edits reach the server.** The
REST body write stops carrying content for those books.

Today there are two delivery mechanisms for the same text — a whole-state `PUT` and a
stream of CRDT ops — and the server must referee which one owns each body. That referee is
`sync_owned` + `canonical_is_authoritative` + `disown_canonical` + the override path, and
its failure modes have been, in order: both writers standing down (silently dropped writes,
two days to diagnose), a stale whole-state write resurrecting deleted text (§D9, "the shape
five production bugs shared"), and now a client deleting its own unsent work to resolve a
lineage conflict.

None of those live in git, and none live in the CRDT. They all live in the negotiation.

### What this deletes

| Removed | Why it existed |
|---|---|
| `UpdateChapterRequest::sync_owned` (and the note equivalent) | tell the server which writer owns this body |
| `canonical_is_authoritative` / `overrode_claim` / `disown_canonical` | distrust the client's half of that claim |
| §D9's stale-whole-state guard for bodies | a second writer could always be stale |
| `take_server_body` as a *replacement* | resolve a lineage conflict destructively |

Structure documents (`book:`) keep a REST path — chapter creation, reorder, and deletion
are structural operations issued by the UI, not editor ops — but they get the same lineage
rules as bodies (§4).

### What it costs

A client with sync disabled cannot deliver body edits for a cut-over book. Offline editing
is unaffected — every edit already lands in the local `DocStore` first, synchronously, and
survives a restart; a syncing client queues ops offline and pushes them on reconnect, which
is strictly better than today's "overwrite with whole state on next autosave".

What is genuinely lost is **the sync toggle as a safety switch**, which is how we protected
the surviving copy of the lost chapter on 2026-08-30. Replacement, and it must ship with
this change: a *pause* that stops outbound pushes while continuing to record locally,
surfaces "this device has N unsent changes", and can export them. A switch that silently
strands writes is what we are removing; a switch that visibly holds them is what authors
actually need.

---

## 3. Git stays, and stops being a writer's destination

Git is not the source of these bugs, and on 2026-08-29 it was the only reason 5,507
characters survived. The CRDT lost the text twice — canonical overwritten, local deltas
compacted — because a CRDT keeps history only until compaction, while git keeps it until
the repo is deleted.

Under one writer, git becomes a **pure projection**:

- The mirror writes every canonical change to git, **unconditionally** — not "when sync
  owns it". Git is therefore always current.
- Nothing reads git for truth in a cut-over book; it backs history, diff, restore, export,
  beta pinning and images, which is most of what it is for.
- No client ever writes it.

This also repairs history under cutover: the History view can keep reading git, because
git is guaranteed current rather than guaranteed stale.

**Do not replace git with a hand-written version store.** That means rebuilding immutable
snapshots, diff, restore and retention, and still needing somewhere durable to put them.
`plotweb-git` is a working implementation of exactly that, and it is not where the bugs are.

---

## 4. Versioning, under one writer: lineage, not concurrency

With a single writer, per-write optimistic concurrency (an ETag on the body `PUT`) buys
nothing — ops merge by construction, and the sync protocol already negotiates by heads and
state vectors. The failure that remains is the one that caused the loss: **two documents
with unrelated lineages**, which no amount of op-merging can reconcile because there is no
shared ancestor.

So versioning attaches to the document, not to the write.

### 4.1 Lineage id

Minted when a document is created; **carried through any rebuild**. A reconcile that
re-projects a chapter from git produces a new *history* but keeps the same lineage id. That
distinguishes the two questions the server currently cannot tell apart:

- "This is a different document." → refuse, as now.
- "This is the same chapter, rebuilt." → reconcile by **text**, not by lineage.

### 4.2 Epoch

Increments on every server-side rebuild. A client arriving at an older epoch is not stale
and is not wrong; it is *forked*. It must:

1. materialize its own text,
2. fetch the canonical text,
3. three-way merge against the last common checkpoint (§4.3) where one exists, otherwise
4. keep both — canonical becomes the document, and the local text is preserved as a
   recovered copy the author can see and merge.

The one outcome that is never acceptable is today's: discard local and continue silently.

### 4.3 Checkpoints

A named, immutable materialization of a document at a point in time, stored in git (which
is what git is). Two jobs: a merge base for §4.2, and the user-facing version history. A
checkpoint is written by the mirror on the same debounce it already uses, so this is
mostly a naming and retention decision rather than new machinery.

---

## 5. `reconcile` under the new model

`reconcile --prefer git` stays, but stops stranding clients:

- It preserves the pre-rebuild canonical document (it is already reading it) rather than
  sweeping it, so a forked client's history can be re-derived server-side.
- It keeps the lineage id and bumps the epoch, so clients reconcile by text (§4.2) instead
  of resetting.
- It stops running unattended from a boot hook. `PLOTWEB_RECONCILE_ON_BOOT` mutates
  documents while clients are connected; the `*_ON_BOOT` ergonomics were built for
  read-only audits and reconcile is not read-only.

---

## 6. Slices

Each is shippable and reversible on its own.

1. **Stop the bleeding.** ✅ Local content is preserved before a canonical copy is
   installed (`preserve_local_copy`), and surfaced — a banner and a viewer, with an
   explicit per-copy discard.
2. **`GET /api/sync/user`.** ✅ The user-index reset had no GET to fetch, so it 405'd
   forever. Added, with a test that *every* `Doc::url()` answers GET.
3. **Mirror / history.** ✅ Not what this slice predicted. History was never invisible
   under cutover — the routes read git and the mirror keeps it current. The real defect
   was `repo::commit_paths` amending without a time bound, so a session on one chapter
   left a single commit whose content was overwritten on every save. Now bounded to a
   five-minute window matching `mirror::MAX_WAIT`.
4. **Lineage id + epoch.** ✅ In the manifest (atomic with the generation flip), reported
   as `x-plotweb-lineage` / `x-plotweb-epoch`, carried across rebuilds, and a rebuild
   quarantines what it replaces. The client classifies a `409` by identity: same lineage
   is a fork to reconcile, a different lineage is refused.
5. **One writer.** ✅ Split in two, because doing it in one step reintroduced the failure
   class from the other side:
   - **5a** — the book reports `cutover`; a device with sync off sees a banner and
     "Saved on this device" instead of "Saved". The state was already lossy-in-waiting;
     this made it visible before it became lossy.
   - **5b** — `content` dropped from body REST writes for a cut-over book, and
     `sync_owned` deleted from the wire. The server decides from state it can verify
     (`cut_over && canonical_is_authoritative`) rather than trusting a client flag, and
     still takes the write when the canonical copy cannot carry it.
6. **Checkpoints / recovery surfaces.** ✅ Landed as *making the difference visible*
   rather than as an automatic merge — see the note below. Quarantined copies are
   reachable (`plotweb-server quarantine list|show`), `reconcile` no longer runs from a
   boot hook, and the rescue viewer marks the paragraphs a rescued copy holds that the
   chapter does not. The history half was covered by the bounded amend window in slice 3.

### Why slice 6 is not an automatic three-way merge

§4.2 above says a forked client should "three-way merge against the last common
checkpoint". Building that literally would mean an algorithm deciding, unattended, how to
combine two versions of someone's prose — and a bad merge produces text that reads as
though the author wrote it, which is the one failure mode nobody catches by looking.

Every loss in this arc came from the system resolving an ambiguity on its own. So the
merge base earns its keep by making the difference **visible and reviewable**: the viewer
marks what the rescued copy has and the chapter lacks, and the author moves it. That is
the same conclusion §2 reached about conflicts generally — *ask, or preserve; never
resolve silently* — applied to merging rather than to discarding.

An automatic merge stays possible later, on top of this, if the reviewable version turns
out to be tedious in practice. It should not come first.

**Widening the cutover is blocked until at least 1, 3 and 5 have landed with regression
tests.** The gate list in the widen card measures server-side agreement only, and must gain
a gate of the form: *a device holding unsent work, meeting a rebuilt canonical document,
still has its work afterwards.*

---

## 7. Open questions

- **Structure documents under one writer.** Chapter create/reorder/delete stay REST. Do
  they need the §4.2 merge, or is last-writer-wins on an ordered list acceptable given the
  reconcile work already landed (#33)?
- **Retention.** How many checkpoints, and does the CRDT delta log get pruned on the same
  schedule? The compaction that erased the local deltas on 2026-08-30 was correct behaviour
  that we happened to need.
- ~~**Native app.** `PLOTWEB_SYNC=1` is opt-in there too; one writer makes it mandatory for
  cut-over books, so the native build must default it on before its books are cut over.~~
  **Settled**: the gate is per book on every client — a cut-over book syncs whether or not
  anything asked, and `PLOTWEB_SYNC` / `plotweb_sync` only overrides that (`"1"` on
  everywhere, `"0"` off everywhere). Nothing has to remember to switch a device on.
