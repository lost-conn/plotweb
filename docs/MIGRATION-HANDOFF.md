# PlotWeb Offline-First Migration — Handoff

**Purpose:** hand this work to a future session. Read this + `docs/offline-first-rinch-plan.md`
(the locked architecture + v1 Automerge schema) and you can pick up the next steps.

---

## TL;DR

PlotWeb is migrating from **git-backed storage** to **local-first Automerge CRDTs**
(offline-first, multi-device). The client local-first layer is built for all four
doc types; the git→Automerge migration has been **audited clean and backfilled on
production**, with git untouched throughout. **Nothing Automerge is live yet** — the
canonical store is staged but unread. The remaining work is the *sync engine* and the
*cutover* (make Automerge authoritative), each reversible.

Production is on **v16**, healthy. Real data: 4 books, **92/92 docs audit-clean**,
**92 Automerge blobs backfilled**.

---

## What's done (this arc), with commits

All on `origin/main`. Deploy = push to `origin` → GitHub Action → jkbase build (see Runbook).

**Native port + upstream rinch** (earlier in the arc):
- Pure-rinch frontend runs on web *and* native desktop. Fixed the native crash classes
  (`Closure::wrap` panics off-wasm even behind a `platform::` guard — use the `web_only!`
  macro), `set_inner_html` via `NodeHandle`, reactive inputs, `key:` on loops, Typography
  panel rewrite. Commits `7e109fa`,`c15fc78`,`d9cac44`,`0d5762e`,`04d8877`,`fdd17be`.
- Six upstream rinch PRs merged (#113 text_align, #114 rinch-ws, #115 rinch-storage,
  #116 render_surface, #117 editor-collab lists, #118 rinch-dom viewport relayout).
  PlotWeb repinned to rinch `f7e1c37` (`85f3218`).

**Phase 2 — client local-first store** (all 4 v1 doc types; each proven natively via the
rinch MCP — local edit beats a divergent server after restart):
- `72f8467` chapter bodies · `150979f` note bodies + book structure · `11f07d4` user-index
  (dashboard). Files: `plotweb-web/src/local_store.rs`, `local_book.rs`, `local_user.rs`.
- **Dual-write**: every existing REST save stays; the local Automerge write is *additive*.
  Local docs seed from the REST API when absent. Nothing lost; web behavior unchanged.

**Migration (git → Automerge), reversible/git-as-fallback:**
- `fb3ffd0` the plan (in `docs/offline-first-rinch-plan.md`, §"Migration off git").
- `da3fcac` **A+B**: `crates/plotweb-crdt` = canonical projection + `roundtrip_*` validators;
  `plotweb-server audit-migration` read-only dry-run.
- `6b67d31` **boot-time lock-free audit** (env `PLOTWEB_AUDIT_ON_BOOT`) — runs the content
  audit alongside the live server, logs to `jkbase logs`.
- `9039640` **legacy-tolerant**: converts pre-DocNode content (Markdown chapters / HTML
  notes) the same way the editor does. `plotweb_common::markdown_to_html` (line-based, no
  paragraph collapse) + `slice_from_html` mirror of `EditorHandle::load_html`.
- `d3fa62b` **Option 3**: split `hard_break`→paragraphs so legacy `<br>` notes migrate.
- `e220df7` **Phase C backfill**: `plotweb-server backfill-migration` (+ env
  `PLOTWEB_BACKFILL_ON_BOOT`) writes one Automerge snapshot per clean doc into
  `PLOTWEB_CRDT_DIR` (default `data/crdt`). Additive, reversible, idempotent, lock-free.

**Prod audit progression** (each an app restart with the audit flag on): 50 flagged →
5 → **0**. Then backfill: **92 blobs, 0 flagged**. Git byte-identical throughout.

---

## Codebase map

- **`crates/plotweb-crdt/`** — the ONE canonical git-DocNode→Automerge projection, shared
  by validate (audit) and emit (backfill). `body.rs` (`roundtrip_body`/`project_body` +
  `BodyKind` + `prepare_body_node` + `split_hard_breaks_doc`), `book.rs`
  (`roundtrip_book_structure`/`project_book_structure`), `user.rs` (user index). **Equality
  is semantic**: `coalesce` merges adjacent same-mark text runs before comparing (so inline
  segmentation isn't a false diff). Change the projection ONLY here so client + server + the
  audit's "clean" verdict never diverge.
- **`crates/plotweb-server/src/audit.rs`** — `run` (the `audit-migration` subcommand, rhypedb-
  enumerated, incl. `user:`), `run_content_audit`/`run_boot_audit` (lock-free, DATA_DIR-
  enumerated, content-only), `audit_one_book` (shared per-book walk).
- **`crates/plotweb-server/src/backfill.rs`** — `run` (subcommand) / `run_boot_backfill` /
  `run_content_backfill`. FsStore blob store, keyed `{doc_id}/{snapshot,manifest,src-sha}`.
  Idempotent via `src-sha` = sha256 of the **source** content (Automerge `save()` bytes have
  a random actor id, so blob-hashing wouldn't be stable).
- **`crates/plotweb-server/src/main.rs`** — subcommand dispatch (`audit-migration`,
  `backfill-migration`) + the two boot hooks (env-gated).
- **`plotweb-web/src/local_store.rs`** — client `DocStore` (generation + manifest pointer-
  flip recipe), the cross-platform `spawn` for `!Send` storage futures, `attach_chapter`/
  `attach_note`. `local_book.rs`/`local_user.rs` — hand-projected `book:`/`user:` docs.
- **`docs/offline-first-rinch-plan.md`** — architecture + **v1 Automerge schema** (doc types
  `user:` / `book:` / `chapter:` / `note:`) + the migration phase list.

---

## Operational runbook (READ THIS — non-obvious)

**Deploy.** `git push origin main` → GitHub Action `.github/workflows/jkbase-deploy.yml`
does `git push jkbase main` → jkbase builds + deploys. **`deploy.sh` is LEGACY, unused.**
The jkbase build is a **Rust/Trunk buildpack** (`~/projects/personal/jkbase`
`crates/jkbuild/src/buildpacks/{rust,trunk}.rs`), NOT the Dockerfile (also legacy). The
buildpack caches `CARGO_HOME` but **NOT `target/`** → every deploy is a full recompile;
**build-minutes are metered** and the monthly quota is the real constraint (we hit it once;
it was raised). A cheap win is carded: point `CARGO_TARGET_DIR` at the jkbase `/cache` drive
in `rust.rs`. Watch a deploy with `jkbase project info plotweb` (Version bumps vN→vN+1) +
`https://pw.lostconnection.dev/health`. jkbase health-checks before cutover — a broken build
keeps the old version (fail-safe). Builds take several minutes.

**jkbase CLI** (`~/.cargo/bin/jkbase`, authed). Read-only: `deployments`, `logs
--project plotweb -n N`, `project info plotweb`, `usage`, `quota`. **The installed CLI is
older than `~/projects/personal/jkbase`** (it lacks `restart`, `db`, `backup`). Setting a
secret does NOT auto-apply — it needs a **restart** to re-inject env. The CLI can't
`restart`; the user restarts from the jkbase console (or `jkbase deploy` = full rebuild).

**`jkbase restart` exists now** (`jkbase restart --project plotweb --force`): it re-injects
secrets/env **without a rebuild**, which is the cheap way to apply a `jkbase secret set`.
The note below about the CLI lacking it is out of date — a full `deploy` for a flag change
burns metered build minutes for nothing.

**The migration flags** (env, via `jkbase secret set NAME=1` + restart; unset with `=0`):
- `PLOTWEB_AUDIT_ON_BOOT` — runs the read-only content audit on boot, logs `[boot-audit]`.
- `PLOTWEB_BACKFILL_ON_BOOT` — runs the backfill on boot, logs `[backfill]`. Writes only
  `PLOTWEB_CRDT_DIR`; git+rhypedb read-only. Idempotent (re-runs skip unchanged).
- `PLOTWEB_SHADOW_ON_BOOT` — phase D: compares every canonical document against git and
  logs `[boot-shadow]`. Read-only. With the backfill flag also set, the two run in one
  task, backfill first, so a single boot does "refresh, then measure".
- All three are lock-free (no rhypedb lock) so they run alongside the live server. Turn
  them off when not in use (they re-run every restart otherwise).

**Reconciling on the deployment** (`PLOTWEB_RECONCILE_ON_BOOT=dry-run|git|crdt` + restart):
the subcommand cannot be run on jkbase — the platform gives logs, secrets, restart and
deploy, no shell — so the boot hook is the only way to resolve a divergence where the
divergences actually are. Runs between the backfill and the shadow pass. Anything
unrecognised is treated as a dry run; a typo must not rewrite prose. Turn it off after.

**Cutting a book over** (phase E, `PLOTWEB_CUTOVER_BOOKS=<book-id>[,<book-id>]` + restart):
that book's chapter/note bodies are read from the canonical store and REST writes land in
both it and git. A body whose two copies **disagree** is refused with `409` rather than
served from either side (there is no safe base to author from); an **absent** canonical
copy falls back to git. Reversible by unsetting and restarting; git has been mirroring, so
a rollback returns to current content. Cut over a book of ours first, never a user's.

**Sync writes mirror into git automatically, for cut-over books only.** A sync push moves
the canonical copy without touching git, so a background pass (`mirror.rs`, always on, no
flag) materializes and writes it back — once the document has been quiet for 30s, or
within 5 minutes of the first change if someone keeps typing. No commit when git already
matches. Structure as well as bodies: a chapter added, renamed, reordered or deleted on a
device is carried across the same way. It refuses one thing — a canonical structure that
has lost *every* chapter or note while git still has them, which is likelier a
half-written document than an emptied book. Non-cut-over books are left alone on purpose: git is still authoritative there,
so a difference should surface in the shadow report and be reconciled with a stated
direction, not be silently overwritten by the CRDT. Consequence worth knowing: for a
cut-over book, commits now appear a little after the edit rather than on save.

**Resolving a divergence** (`plotweb-server reconcile --prefer git|crdt [--dry-run]`):
only touches documents the shadow pass reports as **diverged** — client-owned *and*
disagreeing with git. Staleness is not its job; a backfill run fixes that. `--prefer git`
re-projects git and clears ownership (so the backfill maintains that document again);
`--prefer crdt` materializes the stored document into git through the ordinary write path.
Always dry-run first: it rewrites someone's prose.

**Run the migration tooling locally** (subcommands; server can be stopped or not — lock-free):
```
DATABASE_URL=sqlite:$S/plotweb.db DATA_DIR=$S/books RHYPEDB_DATA_DIR=$S/rhypedb \
  ./target/release/plotweb-server audit-migration [--json report.json]
... PLOTWEB_CRDT_DIR=$C ./target/release/plotweb-server backfill-migration
```

**`plotweb-git` reads COMMITTED content, not the working tree.** To seed legacy/flagged test
content, set it via the REST API (`PUT /api/books/{b}/chapters/{c}` with a `content` string) —
editing the on-disk JSON file WITHOUT committing is NOT seen (cost real debugging time).
Book layout: `DATA_DIR/<book_id>/manuscript/{book.json,chapters/<id>.json}` +
`notes/`. Chapter/note content is the `content` field (DocNode JSON for new, raw
Markdown/HTML for legacy). `POST /api/books` needs a `description` field.

**Native desktop + rinch MCP** (drive the real GUI) — see the `plotweb-native-desktop-build`
memory. Build `cd plotweb-web && cargo build --features debug-mcp`, run with
`PLOTWEB_LOCAL_DATA=<dir>` (client local store) + `PLOTWEB_SERVER=http://127.0.0.1:3000`,
then rinch MCP `list_apps`→`connect <PID>`→`click`/`type_text`/`screenshot`. Window size
varies per launch — `query_selector` for real coordinates, don't hardcode. For rinch co-dev,
`plotweb-web/Cargo.toml` takes an **uncommitted** `[patch."…/rinch"]` → `../../rinch` (see
DEPLOY.md; committing it breaks the jkbase build).

**Verification bar used throughout:** builds at **16 frontend warnings** (baseline), full
`cargo test`, `e2e` (26 specs — see below), and for anything user-facing a **native MCP
drive** proving local beats a divergent server.

---

## Next steps (the work to pick up)

The canonical Automerge store is **staged but unread**. Everything below is where Automerge
*goes live* — weightier than the additive work so far; open each with a written design.

1. **Sync engine (the big one).** → **designed in `docs/sync-engine-design.md`; slices 0–1
   are built.** The forks are settled there (book-scoped periodic HTTP, real heads-based
   protocol, session-cookie auth, canonical store = the backfilled `PLOTWEB_CRDT_DIR`).
   Landed: the Phase-0 spike relay is gone (it was live, unauthenticated, and unbounded),
   and `POST /api/books/{book_id}/sync/{doc_id}` + `POST /api/sync/user` now run the real
   protocol against the canonical store, authorized and per-doc locked.
   Slice 3 also landed: `plotweb-web/src/sync.rs` syncs the `user:` and `book:` docs
   (callback-driven, `rinch_core::set_timeout` poll, off unless `PLOTWEB_SYNC=1` / web
   `localStorage["plotweb_sync"]="1"`). Proven natively with two app instances: a chapter
   added on one device appears in the other's open book **in place**.
   Next: slice 2 (upstream rinch `EditorHandle` sync pass-through — the CRDT has the
   methods, the handle doesn't expose them), then slice 4 (chapter/note bodies, which
   needs slice 2). **Read §D8 before writing that client code**: client and server seeded
   their docs independently, so a naive first merge duplicates content.
2. ~~**`user:` index backfill.**~~ **DONE** — `backfill::run_user_backfill` enumerates
   `Book` rows from rhypedb, groups by owner (same git fallbacks as `books::list`), and
   emits one `user:{id}` blob each. **No downtime needed**: the boot hook
   (`PLOTWEB_BACKFILL_ON_BOOT`) now also runs this pass using the *server's own* rhypedb
   handle, so the single-writer lock is never contended. The `backfill-migration`
   subcommand runs it too, but only with the server stopped (it says so if the lock is
   held). Idempotent via `src-sha`, and it skips client-synced docs like the content pass.
3. ~~**Phase D — shadow read.**~~ **BUILT** — `plotweb-server shadow-report`, or env
   `PLOTWEB_SHADOW_ON_BOOT` to soak beside live traffic (read-only, lock-free, like the
   audit). It compares the **stored** canonical document against git, which is the
   question the audit never asked: the audit projects fresh and proves the projection is
   lossless, while this proves that what clients have actually written still agrees with
   git. Reports match / diverged / no-canonical-copy / unreadable, and only a clean run
   should let phase E proceed.
4. ~~**Phase E — cutover.**~~ **DONE.** Per-book `PLOTWEB_CUTOVER_BOOKS` flag; reads come
   from the canonical store, writes reach both, git stays a **live mirror** (REST writes
   directly, sync writes via the debounced `mirror` pass) so the flag really flips back.
   Bodies and structure both: the chapter list, notes tree and book metadata read from
   the `book:` document, every structure-changing route records into it, and structure
   changes made on a device are mirrored into git. Deletion follows §D7 in both
   directions. The `book:`/`user:` disjoint-history check now exists too
   (`sync::histories_are_disjoint`), so §D8 is enforced for both CRDTs.
5. **Phase F — retire git.** Last, manual, with a backup/tag. The one hard-to-reverse step.

**Also worth landing** (would remove Option-3's compromise): extend `rinch-editor-collab`'s
projection to **inline atoms** (`hard_break`/`image`/`horizontal_rule`) — upstream rinch PR,
like #117 did for lists. Then legacy `<br>` notes keep their line breaks instead of splitting.

---

## Known issues / open cards (Overboard project `cmr2954t9007fk4f3d3fbzagp`, lane TODO)

- **rinch-dom native layout** — inline-block `%`-width collapses; native `<select>` renders
  options inline (needs a real combobox widget). Framework gaps; the Typography panel is
  functional but visually cramped natively.
- **Import paragraph-collapse** — `plotweb-import` (external `.md` import) collapses
  single-newline-separated paragraphs into one block (CommonMark soft-break). Does NOT affect
  the migration (legacy *stored* content uses the line-based `markdown_to_html`, which is
  fine), but affects importing manuscripts.
- **`chapter-reorder.spec.ts` e2e** — regressed mid-session; fails on the deployed baseline
  too (not migration-related). Likely environmental (`:3000` squatter breaking Playwright
  cookie auth) or a flake. The reorder feature itself works (verified natively).
- **Pre-existing, non-fatal** (seen in `jkbase logs`, not from this work): legacy SQLite
  migration warning `no such column: description` on every boot (content lives in
  git/rhypedb, not that column); Resend email fails (`pw.lostconnection.dev` unverified on
  Resend + a malformed `from`) — password-reset email won't send until fixed.

---

## How to verify (quick reference)

- **Builds:** `cargo build` (workspace) + `cargo build --release -p plotweb-server` (deploy
  mirror) + `cd plotweb-web && cargo build --target wasm32-unknown-unknown` (16 warnings).
- **Tests:** `cargo test` (workspace); `cargo test -p plotweb-crdt` (20 — the projection +
  audit fixtures); `cargo test -p plotweb-server` (integration + `tests/backfill.rs`).
- **e2e:** `pkill -x plotweb-server; cd e2e && npx playwright test` (26 specs; `chapter-reorder`
  currently fails pre-existing).
- **Migration on real data:** flip `PLOTWEB_AUDIT_ON_BOOT`/`PLOTWEB_BACKFILL_ON_BOOT`, read
  `jkbase logs`. Locally: the subcommands above against a seeded tempdir (seed content via the
  REST API, not file edits).
