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

    /// Delete every key for this document except the manifest and the keep-generation.
    async fn sweep_except(&self, keep_gen: &str) -> StorageResult<()> {
        let keep = self.gen_prefix(keep_gen);
        let manifest = self.manifest_key();
        for key in self.backend.list(&self.doc_prefix()).await? {
            if key == manifest || key.starts_with(&keep) {
                continue;
            }
            self.backend.delete(&key).await?;
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

// ── Public entry point wired from book.rs ────────────────────────────────────

/// Attach local-first persistence to `handle` for chapter `chapter_id`, seeding
/// from `seed_content` (the REST-fetched chapter body) when no local document
/// exists yet. Schedules its async work on the main thread and returns immediately.
///
/// Idempotent per open: any collaboration session from a previously-open chapter is
/// detached first so seeding / guest-load can never record onto the wrong doc's
/// CRDT or fire the wrong `outbound`.
pub fn attach_chapter(handle: EditorHandle, chapter_id: String, seed_content: String) {
    let doc_id = format!("chapter:{chapter_id}");
    spawn(async move {
        if let Err(e) = attach_body_inner(&handle, &doc_id, &seed_content, BodyKind::Chapter).await {
            log::warn!("local-first: {doc_id}: {e}");
        }
    });
}

/// Shared body-doc attach for a chapter or a note (they differ only in `kind`'s
/// legacy-content seed loader). Adopts a local doc if one exists (guest + delta
/// replay + compaction), else seeds a fresh host doc from `seed_content`.
async fn attach_body_inner(
    handle: &EditorHandle,
    doc_id: &str,
    seed_content: &str,
    kind: BodyKind,
) -> StorageResult<()> {
    // Detach any prior session before touching the editor model.
    handle.stop_collaboration();

    let store = DocStore::open(doc_id).await?;

    match store.load().await? {
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
            Ok(())
        }
        None => seed_and_host_kind(handle, &store, seed_content, kind).await,
    }
}

/// Attach local-first persistence to `handle` for note `note_id` — the note-body
/// mirror of [`attach_chapter`] (doc-id `note:{note_id}`, same collab byte seam,
/// same dual-write). Seeds from `seed_content` (the REST-fetched note body) when no
/// local document exists yet. Note *structure* (title/color/tree position) is not a
/// body concern — it lives in the `book:` doc (see [`crate::local_book`]).
pub fn attach_note(handle: EditorHandle, note_id: String, seed_content: String) {
    let doc_id = format!("note:{note_id}");
    spawn(async move {
        if let Err(e) = attach_body_inner(&handle, &doc_id, &seed_content, BodyKind::Note).await {
            log::warn!("local-first: {doc_id}: {e}");
        }
    });
}

/// Which editor loader seeds a fresh body doc (chapter vs note). The two differ only
/// in the legacy-content fallback (`load_chapter_content` / `load_note_content`); the
/// CRDT collab seam and dual-write are identical.
#[derive(Clone, Copy)]
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
) -> StorageResult<()> {
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
            return Ok(());
        }
    };
    let generation = store.publish_snapshot(&snapshot).await?;
    sink.publish(generation);
    Ok(())
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
            let delta = session.save_incremental();
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
