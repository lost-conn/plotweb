//! Local-first persistence for chapter bodies (Phase 2, Slice 1, deliverable 1).
//!
//! A chapter body is a `rinch-editor-collab` Automerge document. This module makes
//! that document **durable on the client**, independent of the server, via
//! [`rinch_storage`] (native filesystem / web IndexedDB). The editor edits the CRDT
//! directly (through the [`EditorHandle`] collaboration seam); every local edit's
//! change delta lands in local storage immediately, and a reopened chapter
//! reconstructs byte-identically from what was persisted.
//!
//! The existing REST autosave in `book.rs` is left fully intact — this layer is
//! **additive** (dual-write during the offline-first transition).
//!
//! # Persistence recipe: generation + manifest pointer-flip
//!
//! Per document (`chapter:{id}`), keyed under the `{doc_id}/` prefix:
//!
//! - `{doc_id}/manifest` — the id of the **live generation** (e.g. `"g3"`).
//! - `{doc_id}/{generation}/snapshot` — a full Automerge snapshot (the base for `generation`).
//! - `{doc_id}/{generation}/delta/{seq:010}` — the append-only log of incremental change
//!   deltas since that base, in order.
//!
//! This is the multi-key recipe [`rinch_storage`] documents (and pins in its
//! `manifest_pointer_flip_gives_multi_key_atomicity` test): stage the new blobs
//! under a **fresh** generation, then the single atomic `put` of `manifest`
//! publishes the whole set. A crash before that flip leaves the previous
//! generation intact; a crash after it leaves the new one complete. Opening a
//! chapter reconstructs the live generation (snapshot + replayed deltas) and then
//! **compacts** it into a brand-new generation (one fresh snapshot, empty log),
//! which both bounds log growth to a single editing session and sidesteps any
//! delta-sequence reuse (the new session's deltas live under a new prefix; the old
//! generation is swept only after the flip).
//!
//! # Async / main-thread model
//!
//! [`rinch_storage`] ops are `!Send`, `'static` futures. Everything here stays on
//! the main thread: [`EditorHandle`] and rinch `Signal`s are `!Send`, and the
//! collab session must be driven where it lives. We schedule with [`spawn`] —
//! `wasm_bindgen_futures::spawn_local` on web; a single-poll drive on native (the
//! native `FsStore` futures do their blocking work on first poll and resolve
//! immediately, per rinch-storage's native backend docs), so no runtime and
//! nothing `Send` is ever required.

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::rc::Rc;

use rinch_storage::{StorageResult, Store};

#[cfg(target_arch = "wasm32")]
use rinch_storage::IdbStore;
#[cfg(not(target_arch = "wasm32"))]
use rinch_storage::FsStore;

use crate::rinch_backend::EditorHandle;

/// IndexedDB database / object-store names for the web backend.
#[cfg(target_arch = "wasm32")]
const IDB_DB_NAME: &str = "plotweb";
#[cfg(target_arch = "wasm32")]
const IDB_STORE_NAME: &str = "docs";

// ── Backend singleton ────────────────────────────────────────────────────────

thread_local! {
    /// The process-wide storage backend, opened lazily once. `Rc<dyn Store>` so
    /// both platforms' concrete stores share one type behind the object-safe
    /// [`Store`] trait, and clones are cheap (the backend handle is itself an
    /// `Arc`/`Rc`).
    static BACKEND: RefCell<Option<Rc<dyn Store>>> = const { RefCell::new(None) };
}

/// The shared storage backend, opening (and caching) it on first use.
///
/// Single-threaded: two concurrent first-callers could both open, but opening is
/// idempotent (same directory / same IndexedDB database), so the cost is at worst
/// a redundant open, never divergent state.
pub async fn backend() -> StorageResult<Rc<dyn Store>> {
    if let Some(b) = BACKEND.with(|c| c.borrow().clone()) {
        return Ok(b);
    }
    let store = open_backend().await?;
    BACKEND.with(|c| *c.borrow_mut() = Some(store.clone()));
    Ok(store)
}

#[cfg(target_arch = "wasm32")]
async fn open_backend() -> StorageResult<Rc<dyn Store>> {
    let store = IdbStore::open(IDB_DB_NAME, IDB_STORE_NAME).await?;
    Ok(Rc::new(store))
}

#[cfg(not(target_arch = "wasm32"))]
async fn open_backend() -> StorageResult<Rc<dyn Store>> {
    let store = FsStore::open(native_data_dir())?;
    Ok(Rc::new(store))
}

/// The native per-user directory holding local documents.
///
/// Overridable by `PLOTWEB_LOCAL_DATA` (used by the durability repro). Otherwise
/// an OS-appropriate per-user data dir: `$XDG_DATA_HOME/plotweb/docs`, then
/// `$HOME/.local/share/plotweb/docs`, then a temp-dir fallback.
#[cfg(not(target_arch = "wasm32"))]
fn native_data_dir() -> std::path::PathBuf {
    use std::path::PathBuf;
    if let Ok(p) = std::env::var("PLOTWEB_LOCAL_DATA") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        if !xdg.is_empty() {
            return PathBuf::from(xdg).join("plotweb").join("docs");
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() {
            return PathBuf::from(home).join(".local/share/plotweb/docs");
        }
    }
    std::env::temp_dir().join("plotweb").join("docs")
}

// ── Cross-platform main-thread task scheduling ───────────────────────────────

/// Schedule a `!Send`, `'static` future on the main thread.
#[cfg(target_arch = "wasm32")]
pub fn spawn(fut: impl Future<Output = ()> + 'static) {
    wasm_bindgen_futures::spawn_local(fut);
}

/// Native: the `FsStore` futures do their (blocking) work on first poll and
/// resolve immediately, so a composed chain of them reaches `Ready` in one poll of
/// the outer future — no runtime needed, and nothing leaves the main thread. If a
/// future somehow pended (it won't for `FsStore`), the op is dropped rather than
/// spun on; that is non-fatal here because the REST autosave still persists.
#[cfg(not(target_arch = "wasm32"))]
pub fn spawn(fut: impl Future<Output = ()> + 'static) {
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
    fn noop(_: *const ()) {}
    fn clone_raw(_: *const ()) -> RawWaker {
        RawWaker::new(std::ptr::null(), &VTABLE)
    }
    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone_raw, noop, noop, noop);
    let waker = unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) };
    let mut cx = Context::from_waker(&waker);
    let mut fut = Box::pin(fut);
    if let Poll::Pending = fut.as_mut().poll(&mut cx) {
        // Unreachable for FsStore (single-poll completion); guarded so a stray
        // pend drops the op instead of hanging the UI thread.
    }
}

// ── Per-document store ───────────────────────────────────────────────────────

/// A base snapshot plus the ordered deltas recorded since it — everything needed
/// to reconstruct a document's live generation.
pub struct PersistedDoc {
    pub snapshot: Vec<u8>,
    pub deltas: Vec<Vec<u8>>,
}

/// Local-first persistence for one document id (`chapter:{id}`).
#[derive(Clone)]
pub struct DocStore {
    backend: Rc<dyn Store>,
    doc_id: String,
}

impl DocStore {
    /// The document this store is for (`chapter:…` / `note:…` / `book:…`).
    pub fn doc_id(&self) -> &str {
        &self.doc_id
    }

    /// Open a per-document store over the shared backend.
    pub async fn open(doc_id: &str) -> StorageResult<Self> {
        Ok(Self {
            backend: backend().await?,
            doc_id: doc_id.to_string(),
        })
    }

    /// Construct over an explicit backend — the injection seam the native
    /// durability test uses to point at a tempdir `FsStore`.
    pub fn with_backend(backend: Rc<dyn Store>, doc_id: impl Into<String>) -> Self {
        Self {
            backend,
            doc_id: doc_id.into(),
        }
    }

    fn manifest_key(&self) -> String {
        format!("{}/manifest", self.doc_id)
    }
    fn doc_prefix(&self) -> String {
        format!("{}/", self.doc_id)
    }
    fn gen_prefix(&self, generation: &str) -> String {
        format!("{}/{}/", self.doc_id, generation)
    }
    fn snap_key(&self, generation: &str) -> String {
        format!("{}/{}/snapshot", self.doc_id, generation)
    }
    fn delta_prefix(&self, generation: &str) -> String {
        format!("{}/{}/delta/", self.doc_id, generation)
    }
    fn delta_key(&self, generation: &str, seq: u64) -> String {
        format!("{}/{}/delta/{:010}", self.doc_id, generation, seq)
    }

    async fn read_manifest(&self) -> StorageResult<Option<String>> {
        match self.backend.get(&self.manifest_key()).await? {
            Some(bytes) => Ok(String::from_utf8(bytes).ok()),
            None => Ok(None),
        }
    }

    /// Load the live generation's base snapshot + ordered delta log.
    ///
    /// `Ok(None)` means nothing durable exists yet (fresh chapter, or a
    /// manifest that points at a snapshot no longer present — treated as absent so
    /// the caller re-seeds and republishes cleanly).
    pub async fn load(&self) -> StorageResult<Option<PersistedDoc>> {
        let Some(generation) = self.read_manifest().await? else {
            return Ok(None);
        };
        let Some(snapshot) = self.backend.get(&self.snap_key(&generation)).await? else {
            return Ok(None);
        };
        let mut keys = self.backend.list(&self.delta_prefix(&generation)).await?;
        keys.sort(); // zero-padded seq → lexicographic order == append order
        let mut deltas = Vec::with_capacity(keys.len());
        for key in keys {
            if let Some(bytes) = self.backend.get(&key).await? {
                deltas.push(bytes);
            }
        }
        Ok(Some(PersistedDoc { snapshot, deltas }))
    }

    /// Publish `snapshot` as a brand-new generation and make it live with the single
    /// atomic `manifest` flip, then sweep every key that isn't part of it. Returns
    /// the new generation id — the caller keys subsequent [`append_delta`] calls to
    /// it.
    ///
    /// [`append_delta`]: DocStore::append_delta
    pub async fn publish_snapshot(&self, snapshot: &[u8]) -> StorageResult<String> {
        let prev = self.read_manifest().await?;
        let generation = next_gen(prev.as_deref());
        // Stage the snapshot under the fresh generation (nothing references it yet).
        self.backend.put(&self.snap_key(&generation), snapshot).await?;
        // Atomic commit point: this single put publishes the whole generation.
        self.backend
            .put(&self.manifest_key(), generation.as_bytes())
            .await?;
        // Sweep the now-unreferenced predecessor (and any crash-left garbage).
        self.sweep_except(&generation).await?;
        Ok(generation)
    }

    /// Append one incremental change `delta` to `generation`'s log at `seq`.
    pub async fn append_delta(&self, generation: &str, seq: u64, delta: &[u8]) -> StorageResult<()> {
        self.backend.put(&self.delta_key(generation, seq), delta).await
    }

    /// Delete the generations this document no longer needs.
    ///
    /// Only **generation** keys (`{doc_id}/g7/…`). The sweep used to take everything
    /// under the doc prefix that was not the manifest or the live generation, which
    /// included the metadata stored beside the document: `origin` — §D8's provenance
    /// flag — and `server-fingerprint`. Since opening a chapter compacts it, and
    /// compaction publishes a generation, both were destroyed every time an author
    /// opened a chapter: the client forgot it shared history with the server, and the
    /// headless sweep forgot what it had already seen. The state-vector check on the
    /// server is what kept that from corrupting anything.
    async fn sweep_except(&self, keep_gen: &str) -> StorageResult<()> {
        let keep = self.gen_prefix(keep_gen);
        let doc_prefix = self.doc_prefix();
        for key in self.backend.list(&doc_prefix).await? {
            if key.starts_with(&keep) {
                continue;
            }
            let Some(rest) = key.strip_prefix(&doc_prefix) else {
                continue;
            };
            // `g<digits>/…` is a generation; anything else is metadata that belongs to
            // the document rather than to one of its generations.
            let is_generation = rest
                .split_once('/')
                .and_then(|(seg, _)| seg.strip_prefix('g'))
                .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
            if is_generation {
                self.backend.delete(&key).await?;
            }
        }
        Ok(())
    }
}

/// Next generation id after `prev` (`"g3"` → `"g4"`), or `"g0"` when there is none.
/// Monotonic across opens, so a delta key is never reused within a doc's lifetime.
fn next_gen(prev: Option<&str>) -> String {
    let n = prev
        .and_then(|g| g.strip_prefix('g'))
        .and_then(|s| s.parse::<u64>().ok())
        .map(|n| n + 1)
        .unwrap_or(0);
    format!("g{n}")
}

// ── Outbound (per-edit persistence) sink ─────────────────────────────────────

/// Shared state behind the collab `outbound` closure: it buffers deltas produced
/// before the generation is published, then appends every delta (buffered and
/// live) under the live generation in order.
///
/// The buffering matters because the collab session is attached (so local edits
/// can fire `outbound`) *before* [`DocStore::publish_snapshot`] has assigned the
/// generation. Both the published base snapshot and every delta are taken from the
/// same session, so buffered deltas are genuinely incremental-after-base and flush
/// without duplication.
struct OutboundSink {
    store: DocStore,
    generation: RefCell<Option<String>>,
    seq: Cell<u64>,
    pending: RefCell<Vec<Vec<u8>>>,
}

impl OutboundSink {
    fn new(store: DocStore) -> Rc<Self> {
        Rc::new(Self {
            store,
            generation: RefCell::new(None),
            seq: Cell::new(0),
            pending: RefCell::new(Vec::new()),
        })
    }

    /// Record one local-edit delta: persist it under the live generation, or buffer
    /// it until the generation is known.
    fn record(&self, delta: Vec<u8>) {
        match self.generation.borrow().clone() {
            Some(generation) => self.persist(generation, delta),
            None => self.pending.borrow_mut().push(delta),
        }
        // Tell the sync engine there is something to send. Structure and the user index
        // have always done this; bodies did not, so a chapter edit waited out the poll.
        crate::sync::nudge_body(&self.store.doc_id());
    }

    /// Publish the generation and flush any buffered deltas to it in order.
    fn publish(&self, generation: String) {
        *self.generation.borrow_mut() = Some(generation.clone());
        let pending = std::mem::take(&mut *self.pending.borrow_mut());
        for delta in pending {
            self.persist(generation.clone(), delta);
        }
    }

    fn persist(&self, generation: String, delta: Vec<u8>) {
        let seq = self.seq.get();
        self.seq.set(seq + 1);
        let store = self.store.clone();
        spawn(async move {
            if let Err(e) = store.append_delta(&generation, seq, &delta).await {
                log::warn!("local-first: append delta failed: {e}");
            }
        });
    }
}

// ── Which document each editor surface is bound to ───────────────────────────
//
// Attaching is asynchronous — opening the backend, reading the manifest, listing
// deltas — and on web those awaits are *real* (IndexedDB), unlike native where the
// `FsStore` futures resolve on first poll. An author who switches chapters during
// that window would otherwise have the earlier chapter's continuation resume against
// the editor now showing the *later* chapter, and every path here writes to the
// editor: the adopt path calls `start_collaboration_guest`, which replaces the
// document, and the seed path loads `seed_content` into it. The editor would then
// display chapter A while the page believes it holds chapter B — and the REST
// autosave, which only checks that the page's own load finished, would persist A's
// text over chapter B. (The stale `OutboundSink` would likewise record B's keystrokes
// into A's local doc.)
//
// So each surface records the doc-id it is currently bound to, set **synchronously**
// when the attach is requested. Every continuation re-checks it before touching the
// editor and abandons itself if it has been superseded. Chapter and note editors are
// separate surfaces with separate handles, so they track separately — opening a note
// must not abandon an in-flight chapter attach.

thread_local! {
    /// Doc-id the chapter editor surface is bound to.
    static ACTIVE_CHAPTER_BODY: RefCell<Option<String>> = const { RefCell::new(None) };
    /// Doc-id the note editor surface is bound to.
    static ACTIVE_NOTE_BODY: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Bind `kind`'s surface to `doc_id`, superseding whatever it held. Call
/// **synchronously** with the request, before any await.
fn claim_surface(kind: BodyKind, doc_id: &str) {
    let slot = match kind {
        BodyKind::Chapter => &ACTIVE_CHAPTER_BODY,
        BodyKind::Note => &ACTIVE_NOTE_BODY,
    };
    slot.with(|a| *a.borrow_mut() = Some(doc_id.to_string()));
}

/// Whether `doc_id` is still what `kind`'s surface is bound to.
fn surface_holds(kind: BodyKind, doc_id: &str) -> bool {
    let slot = match kind {
        BodyKind::Chapter => &ACTIVE_CHAPTER_BODY,
        BodyKind::Note => &ACTIVE_NOTE_BODY,
    };
    slot.with(|a| a.borrow().as_deref() == Some(doc_id))
}

// ── One-time recovery sweep ──────────────────────────────────────────────────

/// Bumped when locally-stored body documents must be discarded and re-seeded.
///
/// `2` discards every body doc written before the chapter-crosstalk fixes. Until
/// those landed, switching chapters recorded the *next* chapter's content into the
/// *previous* chapter's document (a load is a document change, and the previous
/// chapter's session was still attached — see `editor_utils::detach_before_load`),
/// and a stale attach could do the same asynchronously. Devices that hit either one
/// are carrying documents holding the wrong chapter's text, and because a local body
/// doc is adopted in preference to the server copy on reopen, they would keep showing
/// it even once the causes were fixed.
///
/// Dropping the body docs once is safe and is the recovery path: they are a
/// dual-write cache, git remains authoritative, and the next open re-seeds each body
/// from the server.
/// `3` additionally discards every body doc written while the editor's CRDT was
/// Automerge: rinch #190 moved it to yrs, so those bytes are no longer loadable as a
/// collaboration session at all. Re-seeding from the server is the whole recovery.
const BODY_STORE_VERSION: &[u8] = b"3";
const BODY_STORE_VERSION_KEY: &str = "local-store/body-version";

/// Progress of the one-time sweep, so a second attach can't adopt a document the
/// sweep is about to delete.
#[derive(Clone, Copy, PartialEq)]
enum Recovery {
    NotStarted,
    Running,
    Done,
}

thread_local! {
    static BODY_RECOVERY: Cell<Recovery> = const { Cell::new(Recovery::NotStarted) };
}

/// Ensure the one-time body-doc sweep has happened, returning whether it is now safe
/// to **adopt** a stored body document.
///
/// The first caller performs the sweep (the state flips synchronously, before any
/// await, so a second caller can't start a second one). A caller arriving while the
/// sweep is in flight gets `false` and seeds from REST instead of adopting — adopting
/// there would race the deletion and could resurrect exactly what is being discarded.
///
/// Structure docs (`book:` / `user:`) are untouched: they were never written by the
/// affected path, since their mutations are already guarded by book/user id.
async fn body_docs_adoptable(backend: &Rc<dyn Store>) -> StorageResult<bool> {
    match BODY_RECOVERY.with(|r| r.get()) {
        Recovery::Done => return Ok(true),
        Recovery::Running => return Ok(false),
        Recovery::NotStarted => BODY_RECOVERY.with(|r| r.set(Recovery::Running)),
    }

    if backend.get(BODY_STORE_VERSION_KEY).await?.as_deref() != Some(BODY_STORE_VERSION) {
        for prefix in ["chapter:", "note:"] {
            for key in backend.list(prefix).await? {
                backend.delete(&key).await?;
            }
        }
        // Written last: an interrupted sweep simply runs again next time.
        backend
            .put(BODY_STORE_VERSION_KEY, BODY_STORE_VERSION)
            .await?;
        log::info!("local-first: discarded local body docs once (recovery sweep)");
    }

    BODY_RECOVERY.with(|r| r.set(Recovery::Done));
    Ok(true)
}

// ── Public entry point wired from book.rs ────────────────────────────────────

/// Attach local-first persistence to `handle` for chapter `chapter_id`, seeding
/// from `seed_content` (the REST-fetched chapter body) when no local document
/// exists yet. Schedules its async work on the main thread and returns immediately.
///
/// Idempotent per open: any collaboration session from a previously-open chapter is
/// detached first so seeding / guest-load can never record onto the wrong doc's
/// CRDT or fire the wrong `outbound`. Superseded by the next call for this surface —
/// see the surface-binding note above.
pub fn attach_chapter(
    handle: EditorHandle,
    book_id: String,
    chapter_id: String,
    seed_content: String,
) {
    let doc_id = format!("chapter:{chapter_id}");
    claim_surface(BodyKind::Chapter, &doc_id);
    spawn(async move {
        match attach_body_inner(&handle, &doc_id, &seed_content, BodyKind::Chapter).await {
            Ok(Some(sink)) => register_body_session(BodySession {
                doc_id,
                book_id,
                kind: BodyKind::Chapter,
                handle,
                sink,
            }),
            Ok(None) => {}
            Err(e) => log::warn!("local-first: {doc_id}: {e}"),
        }
    });
}

// ── The open body document, for the sync engine ──────────────────────────────

/// Everything sync needs for the body document an editor currently holds: the live
/// editor (which owns the CRDT), and the sink whose generation must be re-pointed
/// after a remote change is merged in.
pub(crate) struct BodySession {
    pub doc_id: String,
    /// Body sync endpoints are book-scoped, so the attach carries the book through.
    pub book_id: String,
    kind: BodyKind,
    pub handle: EditorHandle,
    sink: Rc<OutboundSink>,
}

thread_local! {
    static CHAPTER_SESSION: RefCell<Option<BodySession>> = const { RefCell::new(None) };
    static NOTE_SESSION: RefCell<Option<BodySession>> = const { RefCell::new(None) };
}

fn session_slot(kind: BodyKind) -> &'static std::thread::LocalKey<RefCell<Option<BodySession>>> {
    match kind {
        BodyKind::Chapter => &CHAPTER_SESSION,
        BodyKind::Note => &NOTE_SESSION,
    }
}

/// Publish a freshly-attached body session, and offer it to the sync engine.
fn register_body_session(session: BodySession) {
    // Superseded between the attach finishing and this call: drop it rather than
    // overwrite the surface's current session.
    if !surface_holds(session.kind, &session.doc_id) {
        return;
    }
    let (kind, doc_id, book_id) = (session.kind, session.doc_id.clone(), session.book_id.clone());
    session_slot(kind).with(|s| *s.borrow_mut() = Some(session));
    crate::sync::register_body(&doc_id, &book_id);
}

/// Run `f` against the open body session for `doc_id`, or `None` if that document is
/// no longer the one its surface holds (the author moved on).
pub(crate) fn with_body_session<R>(doc_id: &str, f: impl FnOnce(&BodySession) -> R) -> Option<R> {
    // Which surface (if either) currently holds this document — resolved before `f`
    // is consumed, since it can only be called once.
    let kind = [BodyKind::Chapter, BodyKind::Note].into_iter().find(|&kind| {
        session_slot(kind).with(|s| {
            s.borrow()
                .as_ref()
                .is_some_and(|session| session.doc_id == doc_id)
        }) && surface_holds(kind, doc_id)
    })?;
    session_slot(kind).with(|s| s.borrow().as_ref().map(f))
}

// ── Provenance (design §D8) ──────────────────────────────────────────────────
//
// A body doc seeded locally from REST and the server's canonical copy of the same
// chapter share **no history** — both were built independently from the same git
// content. Automerge merges them by concatenation, not deduplication, so the author
// would see their chapter twice. A doc is therefore marked once it provably shares
// history with the server's (we adopted theirs, or they adopted ours), and only then
// may the sync protocol run against it.

fn origin_key(doc_id: &str) -> String {
    format!("{doc_id}/origin")
}

/// Whether this body doc shares history with the server's canonical copy.
pub(crate) async fn body_shares_server_history(doc_id: &str) -> StorageResult<bool> {
    let backend = backend().await?;
    Ok(backend.get(&origin_key(doc_id)).await?.as_deref() == Some(b"synced"))
}

/// Record that this body doc now shares history with the server's.
pub(crate) async fn mark_body_shares_server_history(doc_id: &str) -> StorageResult<()> {
    let backend = backend().await?;
    backend.put(&origin_key(doc_id), b"synced").await
}

// ── Document identity (lineage + epoch) ──────────────────────────────────────
//
// What the server reports beside a canonical document. Stored beside ours so the next
// conflict can be classified rather than guessed at: same lineage means the same
// chapter rebuilt (reconcilable), a different lineage means a different document
// (not reconcilable by anything but a person). Safe to keep here now that the
// generation sweep leaves a document's metadata alone.

fn lineage_key(doc_id: &str) -> String {
    format!("{doc_id}/lineage")
}
fn epoch_key(doc_id: &str) -> String {
    format!("{doc_id}/epoch")
}

/// The identity this device last learned for `doc_id`.
pub(crate) async fn local_identity(doc_id: &str) -> StorageResult<(Option<String>, u64)> {
    let backend = backend().await?;
    let lineage = backend
        .get(&lineage_key(doc_id))
        .await?
        .and_then(|b| String::from_utf8(b).ok());
    let epoch = backend
        .get(&epoch_key(doc_id))
        .await?
        .and_then(|b| String::from_utf8(b).ok())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    Ok((lineage, epoch))
}

/// Record the identity of the canonical document this device has just adopted.
pub(crate) async fn record_identity(
    doc_id: &str,
    lineage: Option<&str>,
    epoch: u64,
) -> StorageResult<()> {
    let backend = backend().await?;
    if let Some(lineage) = lineage {
        backend.put(&lineage_key(doc_id), lineage.as_bytes()).await?;
    }
    backend
        .put(&epoch_key(doc_id), epoch.to_string().as_bytes())
        .await
}

// ── Rescue copies ────────────────────────────────────────────────────────────
//
// Installing the server's document over ours is the §D8 resolution, and it was
// justified by an assumption that stopped holding at cutover: that everything we
// hold also reached git, so replacing our copy costs history and not content. Under
// cutover, sync is the *only* path an edit takes to the server, so a device that has
// been writing holds text no other copy has — and on 2026-08-29 that text was
// discarded to resolve a lineage conflict, silently.
//
// Until lineage reconciliation lands (`docs/one-writer-and-lineage.md` §4), the rule
// here is simply: never overwrite a local document without keeping what it held.
// Rescue keys live *outside* the doc prefix, so `DocStore::sweep_except` cannot
// collect them, and they are numbered rather than timestamped because there is no
// cross-platform clock on this side (`Date::now()` panics off-wasm).

const RESCUE_PREFIX: &str = "_rescue/";

fn rescue_prefix(doc_id: &str) -> String {
    format!("{RESCUE_PREFIX}{doc_id}/")
}

/// The next unused `rNNNN` slot for this doc, so repeated rescues accumulate rather
/// than overwrite each other.
async fn next_rescue_slot(backend: &Rc<dyn Store>, doc_id: &str) -> StorageResult<String> {
    let prefix = rescue_prefix(doc_id);
    let mut highest = 0u32;
    for key in backend.list(&prefix).await? {
        let Some(rest) = key.strip_prefix(&prefix) else {
            continue;
        };
        let slot = rest.split('/').next().unwrap_or_default();
        if let Some(n) = slot.strip_prefix('r').and_then(|n| n.parse::<u32>().ok()) {
            highest = highest.max(n);
        }
    }
    Ok(format!("r{:04}", highest + 1))
}

/// Copy the stored document aside before something replaces it.
///
/// Preserves the live generation exactly as persisted — base snapshot plus every
/// delta in order — rather than a materialization, so nothing depends on the text
/// projection being correct at rescue time. `Ok(false)` means there was nothing
/// durable to keep.
pub(crate) async fn preserve_local_copy(doc_id: &str) -> StorageResult<bool> {
    let backend = backend().await?;
    preserve_local_copy_in(&backend, doc_id).await
}

/// [`preserve_local_copy`] against an explicit backend.
pub(crate) async fn preserve_local_copy_in(
    backend: &Rc<dyn Store>,
    doc_id: &str,
) -> StorageResult<bool> {
    let store = DocStore::with_backend(backend.clone(), doc_id);
    let Some(doc) = store.load().await? else {
        return Ok(false);
    };
    let slot = next_rescue_slot(backend, doc_id).await?;
    let base = format!("{}{slot}", rescue_prefix(doc_id));
    for (seq, delta) in doc.deltas.iter().enumerate() {
        backend
            .put(&format!("{base}/delta/{seq:010}"), delta)
            .await?;
    }
    // Written last: the snapshot's presence is what marks the rescue complete.
    backend.put(&format!("{base}/snapshot"), &doc.snapshot).await?;
    log::warn!(
        "local-first: {doc_id}: kept a copy of this device's document as {slot} \
         ({} delta(s)) before replacing it with the server's",
        doc.deltas.len()
    );
    Ok(true)
}

/// Keep `bytes` — an already-materialized document a caller holds in memory — aside
/// under the same scheme. Used by the structure documents, which replace an in-memory
/// `AutoCommit` rather than a stored generation.
pub(crate) async fn preserve_local_bytes(doc_id: &str, bytes: &[u8]) -> StorageResult<bool> {
    let backend = backend().await?;
    preserve_local_bytes_in(&backend, doc_id, bytes).await
}

/// [`preserve_local_bytes`] against an explicit backend.
pub(crate) async fn preserve_local_bytes_in(
    backend: &Rc<dyn Store>,
    doc_id: &str,
    bytes: &[u8],
) -> StorageResult<bool> {
    if bytes.is_empty() {
        return Ok(false);
    }
    let slot = next_rescue_slot(backend, doc_id).await?;
    backend
        .put(&format!("{}{slot}/snapshot", rescue_prefix(doc_id)), bytes)
        .await?;
    log::warn!("local-first: {doc_id}: kept a copy of this device's document as {slot}");
    Ok(true)
}

/// Every rescued copy on this device, as `(doc_id, slot)` pairs.
///
/// The surface a "this device held work the server never saw" indicator reads; also
/// the thing that makes a rescue recoverable rather than merely stored.
pub async fn rescued_copies() -> StorageResult<Vec<(String, String)>> {
    let backend = backend().await?;
    rescued_copies_in(&backend).await
}

/// [`rescued_copies`] against an explicit backend.
pub(crate) async fn rescued_copies_in(
    backend: &Rc<dyn Store>,
) -> StorageResult<Vec<(String, String)>> {
    let mut found = Vec::new();
    for key in backend.list(RESCUE_PREFIX).await? {
        let Some(rest) = key.strip_prefix(RESCUE_PREFIX) else {
            continue;
        };
        // `{doc_id}/{slot}/snapshot` — only a complete rescue carries a snapshot.
        let Some(base) = rest.strip_suffix("/snapshot") else {
            continue;
        };
        if let Some((doc_id, slot)) = base.rsplit_once('/') {
            found.push((doc_id.to_string(), slot.to_string()));
        }
    }
    found.sort();
    Ok(found)
}

/// Materialize a rescued **body** copy into its `DocNode` JSON — the same shape a
/// chapter's content has everywhere else, so the surface that shows it can reuse the
/// ordinary rendering path.
///
/// A rescue that cannot be projected returns `Ok(None)` rather than an error: the bytes
/// are still on disk either way, and a viewer that cannot render one should say so
/// rather than lose it.
pub async fn materialize_rescued_copy(doc_id: &str, slot: &str) -> StorageResult<Option<String>> {
    let backend = backend().await?;
    let Some(doc) = read_rescued_copy_in(&backend, doc_id, slot).await? else {
        return Ok(None);
    };
    Ok(project_rescue(&doc))
}

/// Replay a rescued generation — base snapshot, then each delta in order — and
/// serialize the result. Pure, so the tests drive it directly.
pub(crate) fn project_rescue(doc: &PersistedDoc) -> Option<String> {
    use rinch_editor_collab::CollabSession;
    use rinch_editor_core::serialize::DocNode;
    use rinch_editor_core::{EditorState, Schema};

    let schema = Rc::new(Schema::starter_kit());
    let mut session = CollabSession::from_bytes(&doc.snapshot).ok()?;
    for delta in &doc.deltas {
        let projected = session.projected_doc(&schema).ok()?;
        let base = EditorState::create(
            schema.clone(),
            projected,
            rinch_editor_core::default_plugins(),
        );
        // A delta that will not integrate stops the replay rather than failing it: the
        // text up to that point is still worth handing back.
        if session.integrate_incremental(&base, delta).is_err() {
            break;
        }
    }
    let node: DocNode = session.projected_doc(&schema).ok()?.to_doc().ok()?;
    serde_json::to_string(&node).ok()
}

/// Forget a rescued copy, once its author has taken what they need from it.
pub async fn discard_rescued_copy(doc_id: &str, slot: &str) -> StorageResult<()> {
    let backend = backend().await?;
    let base = format!("{}{slot}/", rescue_prefix(doc_id));
    for key in backend.list(&base).await? {
        backend.delete(&key).await?;
    }
    Ok(())
}

/// Read one rescued copy back: its base snapshot and the deltas that followed.
pub async fn read_rescued_copy(doc_id: &str, slot: &str) -> StorageResult<Option<PersistedDoc>> {
    let backend = backend().await?;
    read_rescued_copy_in(&backend, doc_id, slot).await
}

/// [`read_rescued_copy`] against an explicit backend.
pub(crate) async fn read_rescued_copy_in(
    backend: &Rc<dyn Store>,
    doc_id: &str,
    slot: &str,
) -> StorageResult<Option<PersistedDoc>> {
    let base = format!("{}{slot}", rescue_prefix(doc_id));
    let Some(snapshot) = backend.get(&format!("{base}/snapshot")).await? else {
        return Ok(None);
    };
    let mut keys = backend.list(&format!("{base}/delta/")).await?;
    keys.sort();
    let mut deltas = Vec::with_capacity(keys.len());
    for key in keys {
        if let Some(bytes) = backend.get(&key).await? {
            deltas.push(bytes);
        }
    }
    Ok(Some(PersistedDoc { snapshot, deltas }))
}

/// Replace the open body document with the server's canonical one.
///
/// The §D8 resolution when the server's copy is owned by another device: our
/// independently-seeded doc can never be merged into it, so we take theirs wholesale.
///
/// **This device's copy is kept first** ([`preserve_local_copy`]). The original
/// justification — the peer's doc is git-current and our content reached git the same
/// way — was true only while every edit dual-wrote to git, which cutover ended. A
/// failure to preserve aborts the install: losing the ability to sync is recoverable,
/// losing the author's text is not.
///
/// Installing goes through `start_collaboration_guest`, which loads the document and
/// *then* attaches, so the load is never recorded into the session we are replacing.
pub(crate) async fn install_server_body(doc_id: &str, bytes: &[u8]) -> StorageResult<bool> {
    preserve_local_copy(doc_id).await?;

    let Some((handle, book_id, kind, store)) = with_body_session(doc_id, |s| {
        (
            s.handle.clone(),
            s.book_id.clone(),
            s.kind,
            s.sink.store.clone(),
        )
    }) else {
        return Ok(false);
    };

    let sink = OutboundSink::new(store.clone());
    let out = sink.clone();
    if handle
        .start_collaboration_guest(bytes, move |delta| out.record(delta))
        .is_err()
    {
        log::warn!("local-first: {doc_id}: server document is outside the collab scope");
        return Ok(false);
    }

    let generation = store.publish_snapshot(bytes).await?;
    sink.publish(generation);
    mark_body_shares_server_history(doc_id).await?;

    register_body_session(BodySession {
        doc_id: doc_id.to_string(),
        book_id,
        kind,
        handle,
        sink,
    });
    Ok(true)
}

// ── Headless bodies (sync engine slice 5) ────────────────────────────────────
//
// A body only has an editor session while it is open, but the *other* chapters of an
// open book still need to converge — otherwise a device only ever learns about the
// one chapter its author happens to be looking at. Those are synced "headless": the
// stored document is loaded as a plain `AutoCommit`, the protocol runs against it,
// and the result is published back. No editor is involved, so nothing can be
// projected into the wrong surface.

/// Store the server's copy of a body no editor holds, with the fingerprint it came
/// with so the next sweep can tell whether anything moved.
///
/// Safe as a plain overwrite: editing a body requires its editor, so a body without
/// one cannot hold changes the server lacks.
pub(crate) async fn install_headless_body(
    doc_id: &str,
    bytes: &[u8],
    fingerprint: &str,
) -> StorageResult<()> {
    // Refuse to overwrite a document an editor is driving.
    if body_is_open(doc_id) {
        return Ok(());
    }
    let store = DocStore::open(doc_id).await?;
    store.publish_snapshot(bytes).await?;
    let backend = backend().await?;
    backend
        .put(&fingerprint_key(doc_id), fingerprint.as_bytes())
        .await?;
    mark_body_shares_server_history(doc_id).await
}

fn fingerprint_key(doc_id: &str) -> String {
    format!("{doc_id}/server-fingerprint")
}

/// The server fingerprint this device last stored for `doc_id`, if any.
pub(crate) async fn body_fingerprint(doc_id: &str) -> StorageResult<Option<String>> {
    let backend = backend().await?;
    Ok(backend
        .get(&fingerprint_key(doc_id))
        .await?
        .and_then(|b| String::from_utf8(b).ok()))
}

// ── Which books are cut over (device-local cache) ────────────────────────────
//
// Cutover is the server's fact, and it arrives with a book's REST payload. But it is
// also the fact that decides *whether writing on this device can reach the server at
// all*: under cutover, sync is the only path a body edit takes. A device that starts
// offline therefore cannot afford to wait for a fetch to learn it — with no answer it
// would treat the book as git-backed, take the REST path, and report a plain "Saved"
// for text that is going nowhere.
//
// So each answer is cached here as it arrives, and read back at startup. Keys live
// outside any doc prefix (like the rescues above), so `DocStore::sweep_except` cannot
// collect them when a chapter is compacted.

const CUTOVER_PREFIX: &str = "_cutover/";

fn cutover_key(book_id: &str) -> String {
    format!("{CUTOVER_PREFIX}{book_id}")
}

/// Remember what the server just said about `book_id`, so the next cold start knows
/// before it can ask.
pub async fn remember_cutover(book_id: &str, cut_over: bool) -> StorageResult<()> {
    let backend = backend().await?;
    remember_cutover_in(&backend, book_id, cut_over).await
}

/// [`remember_cutover`] against an explicit backend.
pub(crate) async fn remember_cutover_in(
    backend: &Rc<dyn Store>,
    book_id: &str,
    cut_over: bool,
) -> StorageResult<()> {
    let key = cutover_key(book_id);
    if cut_over {
        backend.put(&key, b"1").await
    } else {
        // A book can be moved back (the flag is reversible to current content), and a
        // stale "1" would keep this device sending body edits through sync only.
        backend.delete(&key).await
    }
}

/// Every book this device has been told is cut over.
pub async fn cut_over_books() -> StorageResult<Vec<String>> {
    let backend = backend().await?;
    cut_over_books_in(&backend).await
}

/// [`cut_over_books`] against an explicit backend.
pub(crate) async fn cut_over_books_in(backend: &Rc<dyn Store>) -> StorageResult<Vec<String>> {
    let mut found = Vec::new();
    for key in backend.list(CUTOVER_PREFIX).await? {
        if let Some(book_id) = key.strip_prefix(CUTOVER_PREFIX) {
            found.push(book_id.to_string());
        }
    }
    found.sort();
    Ok(found)
}

/// The body documents the two editor surfaces currently hold.
///
/// For the case where a book turns out to be cut over *after* its chapter was already
/// attached: the body's registration is decided at attach time, so learning late has to
/// go back for what is open rather than wait for the next chapter to be opened.
pub(crate) fn open_body_ids() -> Vec<String> {
    [&ACTIVE_CHAPTER_BODY, &ACTIVE_NOTE_BODY]
        .into_iter()
        .filter_map(|slot| slot.with(|a| a.borrow().clone()))
        .filter(|doc_id| body_is_open(doc_id))
        .collect()
}

/// Whether either editor surface currently holds `doc_id`.
pub(crate) fn body_is_open(doc_id: &str) -> bool {
    with_body_session(doc_id, |_| ()).is_some()
}

/// Persist the body document as it now stands and re-point its delta log.
///
/// Called after the sync engine merges a peer's changes into the live session. The
/// merged state must become the new base: leaving it unpublished would keep the
/// stored snapshot (plus a delta log that never saw those changes) behind the CRDT,
/// so a reopen would silently lose the merged content. Publishing a fresh generation
/// and re-pointing the sink keeps subsequent local deltas anchored to it.
pub(crate) async fn republish_body(doc_id: &str) -> StorageResult<()> {
    let Some((store, sink, snapshot)) = with_body_session(doc_id, |session| {
        (
            session.sink.store.clone(),
            session.sink.clone(),
            session.handle.collab_snapshot(),
        )
    }) else {
        return Ok(());
    };
    let Some(snapshot) = snapshot else {
        return Ok(());
    };
    let generation = store.publish_snapshot(&snapshot).await?;
    sink.publish(generation);
    Ok(())
}

/// Shared body-doc attach for a chapter or a note (they differ only in `kind`'s
/// legacy-content seed loader). Adopts a local doc if one exists (guest + delta
/// replay + compaction), else seeds a fresh host doc from `seed_content`.
/// Returns the sink for the attached session, or `None` when no session was attached
/// (superseded mid-attach, or content outside the staged collab scope).
async fn attach_body_inner(
    handle: &EditorHandle,
    doc_id: &str,
    seed_content: &str,
    kind: BodyKind,
) -> StorageResult<Option<Rc<OutboundSink>>> {
    // Detach any prior session before touching the editor model.
    handle.stop_collaboration();

    let store = DocStore::open(doc_id).await?;

    // Superseded while the backend was opening: the surface has moved to another
    // document, so everything below would write this one's content into it.
    if !surface_holds(kind, doc_id) {
        return Ok(None);
    }

    // One-time recovery for documents written before the surface-binding fix; also
    // decides whether adopting a stored body doc is safe right now.
    let adoptable = body_docs_adoptable(&store.backend).await?;

    let loaded = if adoptable { store.load().await? } else { None };

    // Same check after the (IndexedDB) reads — this is the wide window in practice.
    if !surface_holds(kind, doc_id) {
        return Ok(None);
    }

    match loaded {
        Some(persisted) => {
            let sink = OutboundSink::new(store.clone());
            let out = sink.clone();
            if handle
                .start_collaboration_guest(&persisted.snapshot, move |delta| out.record(delta))
                .is_err()
            {
                // Base snapshot unreadable / out of the staged collab scope — the
                // editor is still untouched, so re-seed from REST and host afresh.
                return seed_and_host_kind(handle, &store, seed_content, kind).await;
            }
            // Replay the persisted delta log to reach the last-saved content.
            // `collab_receive` integrates without re-broadcasting, so `sink` stays
            // untouched here.
            for delta in &persisted.deltas {
                handle.collab_receive(delta);
            }
            // Compact the replayed session into a fresh generation snapshot: bounds
            // the log and gives this session's future deltas a clean base + prefix.
            let snapshot = handle
                .collab_snapshot()
                .unwrap_or_else(|| persisted.snapshot.clone());
            let generation = store.publish_snapshot(&snapshot).await?;
            sink.publish(generation);
            Ok(Some(sink))
        }
        None => seed_and_host_kind(handle, &store, seed_content, kind).await,
    }
}

/// Attach local-first persistence to `handle` for note `note_id` — the note-body
/// mirror of [`attach_chapter`] (doc-id `note:{note_id}`, same collab byte seam,
/// same dual-write). Seeds from `seed_content` (the REST-fetched note body) when no
/// local document exists yet. Note *structure* (title/color/tree position) is not a
/// body concern — it lives in the `book:` doc (see [`crate::local_book`]).
pub fn attach_note(
    handle: EditorHandle,
    book_id: String,
    note_id: String,
    seed_content: String,
) {
    let doc_id = format!("note:{note_id}");
    claim_surface(BodyKind::Note, &doc_id);
    spawn(async move {
        match attach_body_inner(&handle, &doc_id, &seed_content, BodyKind::Note).await {
            Ok(Some(sink)) => register_body_session(BodySession {
                doc_id,
                book_id,
                kind: BodyKind::Note,
                handle,
                sink,
            }),
            Ok(None) => {}
            Err(e) => log::warn!("local-first: {doc_id}: {e}"),
        }
    });
}

/// Which editor loader seeds a fresh body doc (chapter vs note). The two differ only
/// in the legacy-content fallback (`load_chapter_content` / `load_note_content`); the
/// CRDT collab seam and dual-write are identical.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BodyKind {
    Chapter,
    Note,
}

impl BodyKind {
    fn seed_editor(self, handle: &EditorHandle, seed_content: &str) {
        match self {
            BodyKind::Chapter => {
                crate::pages::editor_utils::load_chapter_content(handle, seed_content)
            }
            BodyKind::Note => crate::pages::editor_utils::load_note_content(handle, seed_content),
        }
    }
}

/// Fresh-doc path: seed the editor from the REST content, project it onto a new
/// CRDT as host, and publish the initial snapshot.
async fn seed_and_host_kind(
    handle: &EditorHandle,
    store: &DocStore,
    seed_content: &str,
    kind: BodyKind,
) -> StorageResult<Option<Rc<OutboundSink>>> {
    kind.seed_editor(handle, seed_content);

    let sink = OutboundSink::new(store.clone());
    let out = sink.clone();
    let snapshot = match handle.start_collaboration_host(move |delta| out.record(delta)) {
        Ok(snapshot) => snapshot,
        Err(_) => {
            // Content outside the staged flat-text collab scope (e.g. lists/tables,
            // a follow-up deliverable). Leave the editor in normal REST-only mode —
            // no local doc this deliverable, no regression to editing/autosave.
            handle.stop_collaboration();
            return Ok(None);
        }
    };
    let generation = store.publish_snapshot(&snapshot).await?;
    sink.publish(generation);
    Ok(Some(sink))
}

// ── Surface-binding tests ────────────────────────────────────────────────────
//
// The race these guard against lives on web (`spawn_local` + IndexedDB); natively the
// storage futures resolve on first poll, so an attach runs start-to-finish
// synchronously and the window never opens. These test the decision function itself —
// the thing every continuation consults — rather than trying to stage a race that
// cannot occur on this target.

#[cfg(test)]
mod surface_tests {
    use super::*;

    #[test]
    fn the_latest_attach_owns_the_surface() {
        claim_surface(BodyKind::Chapter, "chapter:a");
        assert!(surface_holds(BodyKind::Chapter, "chapter:a"));

        // The author clicks another chapter while chapter:a is still attaching.
        claim_surface(BodyKind::Chapter, "chapter:b");
        assert!(
            !surface_holds(BodyKind::Chapter, "chapter:a"),
            "chapter:a's continuation must abandon itself — resuming would load its \
             content into the editor now showing chapter:b, and the autosave would \
             then persist it over chapter:b"
        );
        assert!(surface_holds(BodyKind::Chapter, "chapter:b"));
    }

    #[test]
    fn chapter_and_note_surfaces_do_not_supersede_each_other() {
        claim_surface(BodyKind::Chapter, "chapter:a");
        claim_surface(BodyKind::Note, "note:n");
        assert!(
            surface_holds(BodyKind::Chapter, "chapter:a"),
            "they are separate editors with separate handles: opening a note must not \
             abandon an in-flight chapter attach"
        );
        assert!(surface_holds(BodyKind::Note, "note:n"));
    }

    #[test]
    fn an_unclaimed_surface_holds_nothing() {
        // A doc-id that was never claimed is never treated as current.
        assert!(!surface_holds(BodyKind::Note, "note:never-claimed"));
    }
}

// ── Durability proof (native): seed → persist → drop → reload → identical ─────

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use std::future::Future;
    use std::rc::Rc;
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

    use rinch_editor_collab::CollabSession;
    use rinch_editor_core::serialize::DocNode;
    use rinch_editor_core::{EditorState, Schema};

    /// Single-poll block-on: the `FsStore` futures resolve on first poll (see
    /// rinch-storage native docs), so one poll drives a composed chain to `Ready`.
    fn block_on<F: Future>(fut: F) -> F::Output {
        fn noop(_: *const ()) {}
        fn clone_raw(_: *const ()) -> RawWaker {
            RawWaker::new(std::ptr::null(), &VTABLE)
        }
        static VTABLE: RawWakerVTable = RawWakerVTable::new(clone_raw, noop, noop, noop);
        let waker = unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) };
        let mut cx = Context::from_waker(&waker);
        let mut fut = Box::pin(fut);
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(v) => v,
            Poll::Pending => panic!("fs-backed storage future unexpectedly pended"),
        }
    }

    fn node_from_json(schema: &Schema, json: &str) -> rinch_editor_core::Node {
        let doc: DocNode = serde_json::from_str(json).expect("valid DocNode json");
        schema.node_from_doc(&doc).expect("node from doc")
    }

    /// The gate the 2026-08-29 loss earned: a device holding work the server has never
    /// seen still has it after the §D8 resolution replaces its document.
    ///
    /// The rescue keeps the live generation as persisted — base snapshot plus every
    /// delta — because the deltas are where unsent keystrokes live; a snapshot-only
    /// rescue would have preserved a document that stops exactly where the lost
    /// chapter did.
    #[test]
    fn a_replaced_document_leaves_this_devices_copy_behind() {
        let dir = tempfile::tempdir().expect("tempdir");
        let backend: Rc<dyn Store> = Rc::new(FsStore::open(dir.path()).unwrap());

        let store = DocStore::with_backend(backend.clone(), "chapter:lost");
        let generation = block_on(store.publish_snapshot(b"base snapshot")).unwrap();
        block_on(store.append_delta(&generation, 1, b"unsent keystrokes")).unwrap();
        block_on(store.append_delta(&generation, 2, b"more unsent text")).unwrap();

        assert!(block_on(preserve_local_copy_in(&backend, "chapter:lost")).unwrap());

        let kept = block_on(read_rescued_copy_in(&backend, "chapter:lost", "r0001"))
            .unwrap()
            .expect("the rescue is readable");
        assert_eq!(kept.snapshot, b"base snapshot");
        assert_eq!(
            kept.deltas,
            vec![b"unsent keystrokes".to_vec(), b"more unsent text".to_vec()],
            "the deltas are the unsent work; a snapshot alone is not a rescue"
        );

        assert_eq!(
            block_on(rescued_copies_in(&backend)).unwrap(),
            vec![("chapter:lost".to_string(), "r0001".to_string())]
        );
    }

    /// A rescue is only useful if the author can read it back, so the stored
    /// generation must project to the same text the editor would have shown — deltas
    /// replayed, not just the base snapshot.
    #[test]
    fn a_rescued_copy_projects_back_to_its_text() {
        let schema = Rc::new(Schema::starter_kit());
        let seed = node_from_json(
            &schema,
            r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"the sentence that reached the server"}]}]}"#,
        );
        let edited = node_from_json(
            &schema,
            r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"the sentence that reached the server, and the one that did not"}]}]}"#,
        );

        let state = EditorState::create(
            schema.clone(),
            seed.clone(),
            rinch_editor_core::default_plugins(),
        );
        let mut session = CollabSession::new(&state).expect("project seed");
        let snapshot = session.snapshot();
        session.record_local(&seed, &edited).expect("project edit");
        let delta = session.save_incremental().expect("encode the edit");

        let rescued = PersistedDoc {
            snapshot,
            deltas: vec![delta],
        };
        let json = project_rescue(&rescued).expect("a rescue projects back");
        assert!(
            json.contains("and the one that did not"),
            "the unsent edit must survive the round trip: {json}"
        );
    }

    /// A device must be able to answer "is this book cut over" before it can ask the
    /// server.
    ///
    /// Under cutover, body edits reach the server through sync and nothing else. A
    /// device that starts offline and has no cached answer treats the book as
    /// git-backed, takes the REST path, and reports an ordinary "Saved" for text that
    /// is going nowhere — so the answer has to survive a restart, and a chapter's
    /// compaction, which is what took the metadata beside a document before.
    #[test]
    fn the_cutover_answer_survives_a_restart_and_a_compaction() {
        let dir = tempfile::tempdir().expect("tempdir");
        let backend: Rc<dyn Store> = Rc::new(FsStore::open(dir.path()).unwrap());
        block_on(remember_cutover_in(&backend, "book-a", true)).unwrap();
        block_on(remember_cutover_in(&backend, "book-b", false)).unwrap();

        // Opening a chapter in that book: reconstruct, then compact.
        let store = DocStore::with_backend(backend.clone(), "chapter:probe");
        block_on(store.publish_snapshot(b"one")).unwrap();
        block_on(store.publish_snapshot(b"two")).unwrap();

        // A fresh backend over the same directory is what a restart looks like.
        let reopened: Rc<dyn Store> = Rc::new(FsStore::open(dir.path()).unwrap());
        assert_eq!(
            block_on(cut_over_books_in(&reopened)).unwrap(),
            vec!["book-a".to_string()],
            "the cut-over book must still be known, and a book that never was must not              be invented"
        );

        // Reversible: the flag can be taken off a book, and this device must stop
        // treating sync as the only way its writing leaves.
        block_on(remember_cutover_in(&reopened, "book-a", false)).unwrap();
        assert!(
            block_on(cut_over_books_in(&reopened)).unwrap().is_empty(),
            "moving a book back must clear the cached answer"
        );
    }

    /// Compaction must not take the metadata stored beside a document with it.
    ///
    /// Opening a chapter compacts it, and the sweep used to delete everything under the
    /// doc prefix that was not the live generation — including `origin` (§D8's
    /// provenance flag) and `server-fingerprint`. So a client forgot it shared history
    /// with the server every time its author opened a chapter.
    #[test]
    fn compaction_keeps_the_metadata_beside_a_document() {
        let dir = tempfile::tempdir().expect("tempdir");
        let backend: Rc<dyn Store> = Rc::new(FsStore::open(dir.path()).unwrap());
        let store = DocStore::with_backend(backend.clone(), "chapter:probe");
        block_on(store.publish_snapshot(b"one")).unwrap();
        block_on(backend.put("chapter:probe/origin", b"synced")).unwrap();
        block_on(backend.put("chapter:probe/server-fingerprint", b"abc")).unwrap();
        // What opening the chapter again does (reconstruct, then compact).
        block_on(store.publish_snapshot(b"two")).unwrap();
        let origin = block_on(backend.get("chapter:probe/origin")).unwrap();
        let fp = block_on(backend.get("chapter:probe/server-fingerprint")).unwrap();
        assert!(
            origin.is_some(),
            "the provenance flag must survive compaction, or §D8's first gate is \
             always wrong after a chapter is opened"
        );
        assert!(fp.is_some(), "so must the headless sweep's bookkeeping");

        // Old generations still go.
        assert!(
            block_on(backend.get("chapter:probe/g0/snapshot")).unwrap().is_none(),
            "the superseded generation is still swept"
        );
    }

    /// Publishing a new generation sweeps the old one — the rescue must live outside
    /// the doc prefix or the replacement it exists to survive would collect it.
    #[test]
    fn a_rescue_survives_the_replacement_that_follows_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let backend: Rc<dyn Store> = Rc::new(FsStore::open(dir.path()).unwrap());

        let store = DocStore::with_backend(backend.clone(), "chapter:lost");
        let generation = block_on(store.publish_snapshot(b"ours")).unwrap();
        block_on(store.append_delta(&generation, 1, b"unsent")).unwrap();
        block_on(preserve_local_copy_in(&backend, "chapter:lost")).unwrap();

        // What `install_server_body` does next.
        block_on(store.publish_snapshot(b"the server's copy")).unwrap();

        let kept = block_on(read_rescued_copy_in(&backend, "chapter:lost", "r0001"))
            .unwrap()
            .expect("the rescue outlives the sweep");
        assert_eq!(kept.snapshot, b"ours");
        assert_eq!(kept.deltas, vec![b"unsent".to_vec()]);
    }

    /// Two conflicts in a row keep two copies; the second must not overwrite the first.
    #[test]
    fn repeated_rescues_accumulate() {
        let dir = tempfile::tempdir().expect("tempdir");
        let backend: Rc<dyn Store> = Rc::new(FsStore::open(dir.path()).unwrap());

        block_on(preserve_local_bytes_in(&backend, "book:b", b"first")).unwrap();
        block_on(preserve_local_bytes_in(&backend, "book:b", b"second")).unwrap();

        let copies = block_on(rescued_copies_in(&backend)).unwrap();
        assert_eq!(copies.len(), 2, "each conflict keeps its own copy");
        assert_eq!(
            block_on(read_rescued_copy_in(&backend, "book:b", "r0001"))
                .unwrap()
                .unwrap()
                .snapshot,
            b"first"
        );
        assert_eq!(
            block_on(read_rescued_copy_in(&backend, "book:b", "r0002"))
                .unwrap()
                .unwrap()
                .snapshot,
            b"second"
        );
    }

    /// Nothing stored yet is not a rescue — a fresh document has nothing to keep.
    #[test]
    fn nothing_durable_means_nothing_to_preserve() {
        let dir = tempfile::tempdir().expect("tempdir");
        let backend: Rc<dyn Store> = Rc::new(FsStore::open(dir.path()).unwrap());

        assert!(!block_on(preserve_local_copy_in(&backend, "chapter:fresh")).unwrap());
        assert!(block_on(rescued_copies_in(&backend)).unwrap().is_empty());
    }

    /// The one-time recovery sweep: body docs written before the surface-binding fix
    /// are discarded (they may hold another chapter's text, and a stored body doc is
    /// adopted in preference to the REST copy), structure docs are kept, and a second
    /// run is a no-op.
    #[test]
    fn recovery_discards_body_docs_once_and_keeps_structure_docs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let backend: Rc<dyn Store> = Rc::new(FsStore::open(dir.path()).unwrap());

        block_on(backend.put("chapter:one/manifest", b"g0")).unwrap();
        block_on(backend.put("chapter:one/g0/snapshot", b"stale bytes")).unwrap();
        block_on(backend.put("note:n/manifest", b"g0")).unwrap();
        block_on(backend.put("book:b/manifest", b"g0")).unwrap();
        block_on(backend.put("user:u/manifest", b"g0")).unwrap();

        assert!(block_on(body_docs_adoptable(&backend)).unwrap());

        assert!(
            block_on(backend.list("chapter:")).unwrap().is_empty(),
            "chapter bodies are discarded"
        );
        assert!(
            block_on(backend.list("note:")).unwrap().is_empty(),
            "note bodies are discarded"
        );
        assert_eq!(
            block_on(backend.list("book:")).unwrap().len(),
            1,
            "book structure docs are untouched — they were never written by the race"
        );
        assert_eq!(block_on(backend.list("user:")).unwrap().len(), 1);

        // A doc written after the sweep survives a second call (no repeat wipe).
        block_on(backend.put("chapter:two/manifest", b"g0")).unwrap();
        assert!(block_on(body_docs_adoptable(&backend)).unwrap());
        assert_eq!(
            block_on(backend.list("chapter:")).unwrap().len(),
            1,
            "the sweep runs once, not on every attach"
        );
    }

    /// The acceptance test: a chapter Automerge doc, edited, persisted through
    /// [`DocStore`] (snapshot + delta log) onto an `FsStore`, dropped, then
    /// reconstructed from a *fresh* store over the same directory — and the
    /// reconstructed editor content must equal the original, byte-for-byte in the
    /// durable `DocNode` shape.
    #[test]
    fn chapter_doc_survives_persist_drop_reload() {
        let schema = Rc::new(Schema::starter_kit());

        // Seed content (what start_collaboration_host would snapshot).
        let seed_json = r#"{"type":"doc","content":[
            {"type":"paragraph","content":[{"type":"text","text":"The lantern guttered against the fog."}]}
        ]}"#;
        // Edited content (what the author types afterwards — the delta).
        let edited_json = r#"{"type":"doc","content":[
            {"type":"paragraph","content":[{"type":"text","text":"The lantern guttered against the fog while the harbour bell counted the hours."}]},
            {"type":"paragraph","content":[
                {"type":"text","text":"A second line, "},
                {"type":"text","text":"bold","marks":[{"type":"bold"}]},
                {"type":"text","text":" for good measure."}
            ]}
        ]}"#;

        let seed_node = node_from_json(&schema, seed_json);
        let edited_node = node_from_json(&schema, edited_json);
        let expected: DocNode = edited_node.to_doc().unwrap();

        let dir = tempfile::tempdir().expect("tempdir");

        // ── Author session: host from seed, edit, persist snapshot + delta ──
        {
            let store: Rc<dyn Store> = Rc::new(FsStore::open(dir.path()).unwrap());
            let ds = DocStore::with_backend(store, "chapter:proof");

            let state = EditorState::create(
                schema.clone(),
                seed_node.clone(),
                rinch_editor_core::default_plugins(),
            );
            let mut session = CollabSession::new(&state).expect("project seed");
            let snapshot = session.snapshot();
            let generation = block_on(ds.publish_snapshot(&snapshot)).unwrap();

            // Project the edit and capture its incremental delta.
            session
                .record_local(&seed_node, &edited_node)
                .expect("project edit");
            // Fallible since rinch #190 (yrs encodes the delta rather than handing
            // back a buffer it always has).
            let delta = session.save_incremental().expect("encode the edit's delta");
            assert!(!delta.is_empty(), "an edit must produce a non-empty delta");
            block_on(ds.append_delta(&generation, 0, &delta)).unwrap();
        } // drop store + DocStore entirely — simulate app exit

        // ── Reopen: fresh FsStore over the same dir, reconstruct, compare ──
        let reopened: Rc<dyn Store> = Rc::new(FsStore::open(dir.path()).unwrap());
        let ds = DocStore::with_backend(reopened, "chapter:proof");
        let persisted = block_on(ds.load()).unwrap().expect("a persisted doc");
        assert_eq!(persisted.deltas.len(), 1, "one delta was appended");

        // Reconstruct exactly as attach_chapter_inner's guest path does: adopt the
        // base snapshot, then replay the delta log.
        let mut guest = CollabSession::from_bytes(&persisted.snapshot).expect("load snapshot");
        let base_state = EditorState::create(
            schema.clone(),
            guest.projected_doc(&schema).unwrap(),
            rinch_editor_core::default_plugins(),
        );
        guest
            .integrate_incremental(&base_state, &persisted.deltas[0])
            .expect("replay delta");

        let reconstructed: DocNode = guest.projected_doc(&schema).unwrap().to_doc().unwrap();
        assert_eq!(
            reconstructed, expected,
            "reconstructed chapter content must equal the edited original"
        );
    }
}
