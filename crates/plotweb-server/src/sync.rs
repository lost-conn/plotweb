//! Canonical Automerge sync — the server half of the sync engine
//! (Phase 2 · sync engine slice 1; see `docs/sync-engine-design.md`).
//!
//! The server owns **one canonical Automerge document per doc-id**, living in the
//! same blob store the phase-C migration backfill wrote (`PLOTWEB_CRDT_DIR`). A
//! client runs Automerge's sync protocol against it, one message per HTTP request.
//!
//! This is deliberately still **additive**: git remains authoritative and every REST
//! route is untouched. Sync only moves Automerge bytes between a client's local store
//! and this one. Deleting `PLOTWEB_CRDT_DIR` returns the system to git-only.
//!
//! # Statelessness — and the rule it forces on clients
//!
//! The server keeps **no per-peer `sync::State` between requests**; it builds a fresh
//! one each time. That is always *correct* (the protocol re-negotiates from the
//! `have`/`heads` the client's message carries) and costs at most an extra round trip.
//! Two consequences, both load-bearing:
//!
//! 1. With a fresh state `have_responded` is false, so the server's
//!    `generate_sync_message` **always** returns a message — the server can never
//!    signal "we're done". The **client** ends an exchange, when its own
//!    `generate_sync_message` returns `None`.
//! 2. **A client must start each poll with a fresh `SyncState` too.** Automerge's
//!    protocol assumes a live connection where the peer *pushes*; a client that keeps
//!    its state across polls believes it is still converged (its heads haven't moved
//!    and the last message said the server agreed) and generates nothing — so it would
//!    never learn about a change another device pushed in the meantime, and the two
//!    devices would silently stop converging. A `SyncState` is a per-exchange
//!    optimization here, not durable state; it is scoped to one poll cycle and thrown
//!    away. (This is what the paired `sync` engine on the client does, and what the
//!    tests below exercise.)
//!
//! # Durability
//!
//! A canonical doc is the one thing here a client may no longer hold, so it is never
//! written in place. Each save stages `{doc_id}/{generation}/snapshot` and then
//! publishes it with a single atomic `manifest` put (the same generation +
//! pointer-flip recipe [`crate::backfill`] and the client's `DocStore` use), sweeping
//! the previous generation only afterwards. A crash before the flip leaves the old
//! generation live; a crash after it leaves the new one complete.
//!
//! Docs written by the backfill have no generation — their snapshot is the flat
//! `{doc_id}/snapshot` key. Readers here fall back to that, so the first sync of a
//! migrated doc reads the backfilled blob and the first save moves it onto a
//! generation.

use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

use automerge::sync::{Message as SyncMessage, State as SyncState, SyncDoc};
use automerge::AutoCommit;
use rinch_storage::{FsStore, Store};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

/// Default location of the canonical Automerge blob store (mirrors the backfill's
/// `PLOTWEB_CRDT_DIR` default).
pub const DEFAULT_CRDT_DIR: &str = "data/crdt";

/// The `projection` marker the backfill stamps into a manifest.
const PROJECTION_V1: &str = "automerge-snapshot-v1";

/// Per-document serialization for the canonical store.
///
/// Automerge merges commute, but the *read-modify-write of a blob* does not: two
/// devices syncing one doc at once would otherwise race and one's changes would be
/// dropped on the floor. Keyed exactly like [`plotweb_git::BookStore`]'s per-book
/// locks — a std mutex guarding a map of async mutexes, so the map is never held
/// across an await.
#[derive(Clone, Default)]
pub struct DocLocks {
    locks: Arc<StdMutex<HashMap<String, Arc<Mutex<()>>>>>,
}

impl DocLocks {
    pub fn new() -> Self {
        Self::default()
    }

    /// The lock for one doc-id, creating it on first use.
    pub fn for_doc(&self, doc_id: &str) -> Arc<Mutex<()>> {
        self.locks
            .lock()
            .unwrap()
            .entry(doc_id.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }
}

/// What went wrong in a sync exchange.
#[derive(Debug)]
pub enum SyncError {
    /// The client's bytes were not a decodable Automerge sync message.
    BadMessage(String),
    /// The blob store could not be read or written.
    Store(String),
    /// The stored canonical document could not be loaded, or the protocol rejected
    /// the message.
    Automerge(String),
}

impl std::fmt::Display for SyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SyncError::BadMessage(m) => write!(f, "malformed sync message: {m}"),
            SyncError::Store(m) => write!(f, "crdt store: {m}"),
            SyncError::Automerge(m) => write!(f, "automerge: {m}"),
        }
    }
}

/// The manifest stored beside each canonical doc.
///
/// Shape-compatible with what [`crate::backfill`] writes (`doc_id` / `type` /
/// `src_sha` / `projection`); sync adds `generation`, `heads` and `synced_at`. The
/// `synced_at` field is also the backfill's **stop sign**: a doc that has been synced
/// must never be re-projected from git, or the server would grow a second,
/// history-disjoint copy of the same content (see `docs/sync-engine-design.md` §D8).
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct DocManifest {
    #[serde(default)]
    pub doc_id: String,
    #[serde(rename = "type", default)]
    pub doc_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub src_sha: Option<String>,
    #[serde(default)]
    pub projection: String,
    /// Live generation directory (`"g1"`), absent for a backfill-written flat blob.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<String>,
    /// Hex change hashes of the canonical doc as of the last save.
    #[serde(default)]
    pub heads: Vec<String>,
    /// Set the first time a client syncs this doc. Presence means "client-owned now".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub synced_at: Option<String>,
}

/// Read a doc's manifest, if it has one.
///
/// Also the backfill's guard: `Some(m)` with `m.synced_at.is_some()` means hands off.
pub fn read_manifest(store: &FsStore, doc_id: &str) -> Result<Option<DocManifest>, SyncError> {
    let key = format!("{doc_id}/manifest");
    let bytes = block_on(store.get(&key)).map_err(|e| SyncError::Store(e.to_string()))?;
    Ok(bytes.and_then(|b| serde_json::from_slice::<DocManifest>(&b).ok()))
}

/// Load a doc's canonical snapshot bytes: the live generation's, or the flat blob a
/// backfill wrote, or `None` when the server has never held this doc.
fn load_snapshot(
    store: &FsStore,
    doc_id: &str,
    manifest: Option<&DocManifest>,
) -> Result<Option<Vec<u8>>, SyncError> {
    let key = match manifest.and_then(|m| m.generation.as_deref()) {
        Some(generation) => format!("{doc_id}/{generation}/snapshot"),
        None => format!("{doc_id}/snapshot"),
    };
    block_on(store.get(&key)).map_err(|e| SyncError::Store(e.to_string()))
}

/// Publish `doc`'s snapshot as a fresh generation and make it live with one atomic
/// manifest put, then sweep everything the new generation replaced.
fn save_snapshot(
    store: &FsStore,
    doc_id: &str,
    doc_type: &str,
    prev: Option<&DocManifest>,
    snapshot: &[u8],
    heads: Vec<String>,
) -> Result<(), SyncError> {
    let generation = next_gen(prev.and_then(|m| m.generation.as_deref()));
    let snap_key = format!("{doc_id}/{generation}/snapshot");
    block_on(store.put(&snap_key, snapshot)).map_err(|e| SyncError::Store(e.to_string()))?;

    let manifest = DocManifest {
        doc_id: doc_id.to_string(),
        doc_type: prev
            .map(|m| m.doc_type.clone())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| doc_type.to_string()),
        src_sha: prev.and_then(|m| m.src_sha.clone()),
        projection: PROJECTION_V1.to_string(),
        generation: Some(generation.clone()),
        heads,
        synced_at: Some(now_stamp()),
    };
    let encoded =
        serde_json::to_vec(&manifest).map_err(|e| SyncError::Store(format!("encode: {e}")))?;

    // Atomic commit point: this single put publishes the new generation.
    block_on(store.put(&format!("{doc_id}/manifest"), &encoded))
        .map_err(|e| SyncError::Store(e.to_string()))?;

    sweep_except(store, doc_id, &generation)?;
    Ok(())
}

/// Delete every key for this doc except the manifest, the `src-sha` (the backfill's
/// idempotency record) and the live generation. Best-effort: a failed sweep leaves
/// garbage, never an unreadable doc, so it is not fatal.
fn sweep_except(store: &FsStore, doc_id: &str, keep_gen: &str) -> Result<(), SyncError> {
    let keep = format!("{doc_id}/{keep_gen}/");
    let manifest = format!("{doc_id}/manifest");
    let src_sha = format!("{doc_id}/src-sha");
    let keys = block_on(store.list(&format!("{doc_id}/")))
        .map_err(|e| SyncError::Store(e.to_string()))?;
    for key in keys {
        if key == manifest || key == src_sha || key.starts_with(&keep) {
            continue;
        }
        let _ = block_on(store.delete(&key));
    }
    Ok(())
}

/// `"g3"` → `"g4"`; no previous generation → `"g0"`.
fn next_gen(prev: Option<&str>) -> String {
    let n = prev
        .and_then(|g| g.strip_prefix('g'))
        .and_then(|s| s.parse::<u64>().ok())
        .map(|n| n + 1)
        .unwrap_or(0);
    format!("g{n}")
}

fn now_stamp() -> String {
    chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Run one round of the sync protocol for `doc_id` against the canonical store.
///
/// Blocking (the `FsStore` futures resolve on first poll and the Automerge work is
/// CPU-bound), so callers run it on a blocking thread. Returns the reply message's
/// bytes — empty only if the protocol genuinely had nothing to say.
///
/// A doc the server has never seen is created from the client's message: that is the
/// "client pushes local as canonical" case (a doc created offline, or one the
/// migration flagged and left git-only). Route-level authorization decides *whether*
/// this doc-id is allowed to exist at all — this function never sees an unvetted id.
/// The heads (change frontier) the server holds for each of `doc_ids` that it has a
/// canonical copy of. Ids with no canonical document are simply absent.
///
/// Lets a client skip documents it is already level with instead of paying a round
/// trip per document per poll — the difference between a constant trickle and a burst
/// of requests for a book with many chapters. Read from the manifests; no document is
/// loaded.
pub fn canonical_heads(
    crdt_dir: &Path,
    doc_ids: &[String],
) -> Result<HashMap<String, Vec<String>>, SyncError> {
    let store = FsStore::open(PathBuf::from(crdt_dir))
        .map_err(|e| SyncError::Store(format!("open {}: {e}", crdt_dir.display())))?;
    let mut out = HashMap::new();
    for doc_id in doc_ids {
        if let Some(manifest) = read_manifest(&store, doc_id)? {
            // Only **client-owned** documents are listed. A pristine backfill blob is
            // frozen at backfill time while git kept moving, so a client that cached
            // it would show backfill-era text — and, because a local body doc wins
            // over the REST copy when a chapter is opened, would then autosave that
            // stale text over the current content. Such a document becomes safe to
            // share only once a device claims it (`adopt`), which republishes it from
            // git-current content.
            if manifest.synced_at.is_none() {
                continue;
            }
            out.insert(doc_id.clone(), manifest.heads);
        }
    }
    Ok(out)
}

/// The canonical document's full snapshot bytes, or `None` when the server has never
/// held this document. Read-only.
pub fn canonical_snapshot(crdt_dir: &Path, doc_id: &str) -> Result<Option<Vec<u8>>, SyncError> {
    let store = FsStore::open(PathBuf::from(crdt_dir))
        .map_err(|e| SyncError::Store(format!("open {}: {e}", crdt_dir.display())))?;
    let manifest = read_manifest(&store, doc_id)?;
    load_snapshot(&store, doc_id, manifest.as_ref())
}

/// Outcome of a client's bid to take ownership of a document (see [`adopt_doc`]).
#[derive(Debug, PartialEq, Eq)]
pub enum Adoption {
    /// The canonical doc was replaced by the client's; it now owns this document.
    Adopted,
    /// A client already owns this document — use the sync protocol, not adoption.
    AlreadyOwned,
}

/// Let a client replace a **pristine** canonical document with its own.
///
/// Needed only for the migration era, and only for bodies. The phase-C backfill
/// projected each git document into an Automerge doc, but git keeps moving: every
/// REST save since is in git and *not* in that blob, so the canonical copy of a body
/// is frozen at backfill time. Neither of the obvious moves is safe:
///
/// - **Merging** the client's doc with the backfilled one concatenates rather than
///   deduplicates — they share no history (`docs/sync-engine-design.md` §D8), so the
///   author would see their chapter twice.
/// - **Adopting the server's** could hand back backfill-era text, older than git, and
///   the client's autosave would then write that over the current content.
///
/// So the backfill blob is treated as **provisional**: the first client to sync a
/// document replaces it with its own git-current doc and takes ownership. Afterwards
/// (`synced_at` present) this returns [`Adoption::AlreadyOwned`] and callers must use
/// the sync protocol — adoption would discard a peer's changes.
pub fn adopt_doc(
    crdt_dir: &Path,
    doc_id: &str,
    doc_type: &str,
    full_doc: &[u8],
) -> Result<Adoption, SyncError> {
    let store = FsStore::open(PathBuf::from(crdt_dir))
        .map_err(|e| SyncError::Store(format!("open {}: {e}", crdt_dir.display())))?;

    let manifest = read_manifest(&store, doc_id)?;
    if manifest.as_ref().is_some_and(|m| m.synced_at.is_some()) {
        return Ok(Adoption::AlreadyOwned);
    }

    // Validate before writing: a doc we can't load is not a doc we should canonicalize.
    let mut doc = AutoCommit::load(full_doc)
        .map_err(|e| SyncError::BadMessage(format!("not a loadable Automerge document: {e}")))?;
    let heads = doc.get_heads().iter().map(|h| h.to_string()).collect();

    save_snapshot(&store, doc_id, doc_type, manifest.as_ref(), full_doc, heads)?;
    Ok(Adoption::Adopted)
}

pub fn sync_round(
    crdt_dir: &Path,
    doc_id: &str,
    doc_type: &str,
    incoming: &[u8],
) -> Result<Vec<u8>, SyncError> {
    let store = FsStore::open(PathBuf::from(crdt_dir))
        .map_err(|e| SyncError::Store(format!("open {}: {e}", crdt_dir.display())))?;

    let manifest = read_manifest(&store, doc_id)?;
    let snapshot = load_snapshot(&store, doc_id, manifest.as_ref())?;
    let mut doc = match snapshot {
        Some(bytes) => AutoCommit::load(&bytes)
            .map_err(|e| SyncError::Automerge(format!("load canonical {doc_id}: {e}")))?,
        None => AutoCommit::new(),
    };

    let before = doc.get_heads();

    let message =
        SyncMessage::decode(incoming).map_err(|e| SyncError::BadMessage(e.to_string()))?;
    let mut state = SyncState::new();
    doc.sync()
        .receive_sync_message(&mut state, message)
        .map_err(|e| SyncError::Automerge(e.to_string()))?;

    let reply = doc
        .sync()
        .generate_sync_message(&mut state)
        .map(SyncMessage::encode)
        .unwrap_or_default();

    if doc.get_heads() != before {
        // The client moved the canonical doc: republish it.
        let heads = doc.get_heads().iter().map(|h| h.to_string()).collect();
        let snapshot = doc.save();
        save_snapshot(
            &store,
            doc_id,
            doc_type,
            manifest.as_ref(),
            &snapshot,
            heads,
        )?;
    } else if let Some(m) = manifest.as_ref().filter(|m| m.synced_at.is_none()) {
        // A pure *pull* — no snapshot rewrite, but the doc still becomes client-owned,
        // and that has to be recorded. A client now holds this document's history, so
        // re-projecting it from git would fork a second, disjoint history that merges
        // by concatenation (§D8). Stamping `synced_at` is what stops the backfill.
        let stamped = DocManifest {
            synced_at: Some(now_stamp()),
            ..m.clone()
        };
        let encoded =
            serde_json::to_vec(&stamped).map_err(|e| SyncError::Store(format!("encode: {e}")))?;
        block_on(store.put(&format!("{doc_id}/manifest"), &encoded))
            .map_err(|e| SyncError::Store(e.to_string()))?;
    }

    Ok(reply)
}

/// Single-poll drive of a `!Send` [`FsStore`] future — the native backend does its
/// blocking work on first poll and resolves immediately (same pattern as
/// [`crate::backfill`] and the client's `local_store`). `Pending` is unreachable here.
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

#[cfg(test)]
mod tests {
    use super::*;
    use automerge::transaction::Transactable;
    use automerge::{ReadDoc, ROOT};

    /// One poll cycle, exactly as the real client runs it: a **fresh** `SyncState`,
    /// then round-trip until *our* `generate_sync_message` returns `None`.
    fn converge(dir: &Path, doc_id: &str, doc: &mut AutoCommit) -> usize {
        let state = &mut SyncState::new();
        let mut rounds = 0;
        // The generated message is taken in its own statement: `doc.sync()` borrows
        // the doc mutably, and we need that borrow released before integrating.
        loop {
            let outgoing = doc.sync().generate_sync_message(state);
            let Some(msg) = outgoing else { break };
            let reply = sync_round(dir, doc_id, "chapter", &msg.encode()).expect("sync round");
            rounds += 1;
            assert!(rounds < 20, "sync did not converge");
            if reply.is_empty() {
                break;
            }
            let reply = SyncMessage::decode(&reply).expect("decode reply");
            doc.sync()
                .receive_sync_message(state, reply)
                .expect("integrate reply");
        }
        rounds
    }

    #[test]
    fn a_new_doc_pushed_by_a_client_becomes_canonical_and_reaches_a_second_client() {
        let dir = tempfile::tempdir().unwrap();

        let mut a = AutoCommit::new();
        a.put(ROOT, "title", "The Lantern").unwrap();
        converge(dir.path(), "chapter:x", &mut a);

        // A second, empty client pulls it down.
        let mut b = AutoCommit::new();
        converge(dir.path(), "chapter:x", &mut b);

        assert_eq!(
            b.get(ROOT, "title").unwrap().unwrap().0.to_str(),
            Some("The Lantern"),
            "the second client must see the first client's document"
        );
    }

    #[test]
    fn two_clients_editing_the_same_doc_converge_through_the_server() {
        let dir = tempfile::tempdir().unwrap();

        let mut a = AutoCommit::new();
        a.put(ROOT, "a", "from A").unwrap();
        converge(dir.path(), "chapter:y", &mut a);

        let mut b = AutoCommit::new();
        converge(dir.path(), "chapter:y", &mut b);
        b.put(ROOT, "b", "from B").unwrap();
        converge(dir.path(), "chapter:y", &mut b);

        // A pulls B's edit back down.
        converge(dir.path(), "chapter:y", &mut a);

        for doc in [&a, &b] {
            assert_eq!(doc.get(ROOT, "a").unwrap().unwrap().0.to_str(), Some("from A"));
            assert_eq!(doc.get(ROOT, "b").unwrap().unwrap().0.to_str(), Some("from B"));
        }
    }

    #[test]
    fn a_converged_client_syncing_again_writes_nothing_new() {
        let dir = tempfile::tempdir().unwrap();

        let mut a = AutoCommit::new();
        a.put(ROOT, "k", "v").unwrap();
        converge(dir.path(), "chapter:z", &mut a);

        let store = FsStore::open(dir.path().to_path_buf()).unwrap();
        let after_first = read_manifest(&store, "chapter:z").unwrap().unwrap();

        // A fresh peer that already holds the same doc: it must not move the canonical
        // generation (a pure catch-up is a read).
        let mut b = AutoCommit::load(&a.save()).unwrap();
        converge(dir.path(), "chapter:z", &mut b);

        let after_second = read_manifest(&store, "chapter:z").unwrap().unwrap();
        assert_eq!(
            after_first.generation, after_second.generation,
            "an up-to-date client must not cause a canonical rewrite"
        );
    }

    #[test]
    fn the_canonical_doc_survives_a_generation_flip_and_sweep() {
        let dir = tempfile::tempdir().unwrap();
        let mut a = AutoCommit::new();

        for i in 0..3 {
            a.put(ROOT, format!("k{i}"), i).unwrap();
            converge(dir.path(), "chapter:g", &mut a);
        }

        let store = FsStore::open(dir.path().to_path_buf()).unwrap();
        let manifest = read_manifest(&store, "chapter:g").unwrap().unwrap();
        assert!(manifest.synced_at.is_some(), "sync stamps synced_at");
        assert_eq!(manifest.heads.len(), 1);

        // Exactly one generation survives (plus the manifest).
        let keys = block_on(store.list("chapter:g/")).unwrap();
        let live = format!("chapter:g/{}/", manifest.generation.as_deref().unwrap());
        assert!(
            keys.iter()
                .all(|k| k.ends_with("/manifest") || k.starts_with(&live)),
            "stale generations must be swept: {keys:?}"
        );

        // And it still loads.
        let bytes = load_snapshot(&store, "chapter:g", Some(&manifest))
            .unwrap()
            .unwrap();
        let reloaded = AutoCommit::load(&bytes).unwrap();
        assert_eq!(reloaded.get(ROOT, "k2").unwrap().unwrap().0.to_i64(), Some(2));
    }

    #[test]
    fn pulling_a_backfilled_doc_marks_it_client_owned() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsStore::open(dir.path().to_path_buf()).unwrap();

        // A backfilled doc, never yet synced.
        let mut canonical = AutoCommit::new();
        canonical.put(ROOT, "from", "git").unwrap();
        block_on(store.put("chapter:p/snapshot", &canonical.save())).unwrap();
        block_on(store.put(
            "chapter:p/manifest",
            br#"{"doc_id":"chapter:p","type":"chapter","src_sha":"abc","projection":"automerge-snapshot-v1"}"#,
        ))
        .unwrap();

        // A device only *reads* it — no local edits at all.
        let mut client = AutoCommit::new();
        converge(dir.path(), "chapter:p", &mut client);

        let manifest = read_manifest(&store, "chapter:p").unwrap().unwrap();
        assert!(
            manifest.synced_at.is_some(),
            "a pull hands this doc's history to a client, so the backfill must be locked \
             out from re-projecting it even though nothing was written"
        );
        assert!(
            manifest.generation.is_none(),
            "a pure pull must not rewrite the snapshot"
        );
        assert_eq!(manifest.src_sha.as_deref(), Some("abc"));
    }

    #[test]
    fn a_backfilled_flat_snapshot_is_adopted_onto_a_generation_on_first_sync() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsStore::open(dir.path().to_path_buf()).unwrap();

        // Simulate exactly what the phase-C backfill leaves behind: a flat snapshot,
        // a manifest with no generation, and a src-sha.
        let mut canonical = AutoCommit::new();
        canonical.put(ROOT, "from", "git").unwrap();
        block_on(store.put("chapter:m/snapshot", &canonical.save())).unwrap();
        block_on(store.put(
            "chapter:m/manifest",
            br#"{"doc_id":"chapter:m","type":"chapter","src_sha":"abc","projection":"automerge-snapshot-v1"}"#,
        ))
        .unwrap();
        block_on(store.put("chapter:m/src-sha", b"abc")).unwrap();

        // A fresh client syncs: it must receive the backfilled content.
        let mut client = AutoCommit::new();
        converge(dir.path(), "chapter:m", &mut client);
        assert_eq!(
            client.get(ROOT, "from").unwrap().unwrap().0.to_str(),
            Some("git"),
            "the client must receive the migrated content"
        );

        // Then it edits, which moves the canonical doc onto a generation.
        client.put(ROOT, "edited", true).unwrap();
        converge(dir.path(), "chapter:m", &mut client);

        let manifest = read_manifest(&store, "chapter:m").unwrap().unwrap();
        assert!(manifest.generation.is_some(), "first save assigns a generation");
        assert_eq!(
            manifest.src_sha.as_deref(),
            Some("abc"),
            "the backfill's source fingerprint is preserved"
        );
        assert!(manifest.synced_at.is_some());
        assert!(
            block_on(store.get("chapter:m/src-sha")).unwrap().is_some(),
            "the backfill's src-sha key must survive the sweep"
        );
    }

    #[test]
    fn a_malformed_message_is_rejected_without_touching_the_store() {
        let dir = tempfile::tempdir().unwrap();
        let err = sync_round(dir.path(), "chapter:bad", "chapter", b"not a sync message");
        assert!(matches!(err, Err(SyncError::BadMessage(_))), "{err:?}");

        let store = FsStore::open(dir.path().to_path_buf()).unwrap();
        assert!(
            block_on(store.list("chapter:bad/")).unwrap().is_empty(),
            "a rejected message must not create a canonical doc"
        );
    }
}
