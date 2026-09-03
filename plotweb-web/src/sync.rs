//! Client sync engine — sync engine slices 3 and 4 (`docs/sync-engine-design.md`).
//!
//! Three document shapes, one loop:
//!
//! - **Structure** (`user:` / `book:`) — plain [`automerge::AutoCommit`]s owned by
//!   [`crate::local_user`] / [`crate::local_book`]. Synced whenever they are open.
//! - **Bodies** (`chapter:` / `note:`) — the CRDT lives inside the editor's
//!   collaboration session, so the protocol is driven through the `EditorHandle` seam
//!   (rinch PR #182) and only while that body is open. Remote changes are integrated
//!   *through the attached session*, which rebuilds the model and re-projects the
//!   view; content is never loaded into the editor behind the session's back, which is
//!   what the chapter-crosstalk bug did.
//!
//! Bodies additionally need a **provenance handshake** before their first exchange:
//! a body seeded from REST shares no history with the server's canonical copy of the
//! same chapter, and Automerge merges disjoint histories by concatenation, so one side
//! must take the other wholesale first. See [`establish_body_provenance`] and §D8.
//!
//! # Shape: callbacks, not futures
//!
//! An exchange is several HTTP round trips. `rinch_http` is callback-based on both
//! targets, and the local-first [`spawn`](crate::local_store::spawn) is a *single-poll*
//! driver natively (it exists for storage futures that resolve immediately, and would
//! drop a future that actually pends). So the exchange is written as an explicit
//! callback chain: each reply schedules the next round. Nothing here needs a runtime,
//! and nothing crosses a thread.
//!
//! # One cycle = one fresh `SyncState`
//!
//! Automerge's protocol assumes a live connection where the peer pushes. Our server is
//! stateless per request and cannot push, so a `SyncState` kept across polls would
//! convince us we are still converged and we would stop asking — and silently miss
//! whatever another device wrote. Each cycle therefore starts a **fresh** state and
//! throws it away at the end. (The server-side rationale is in
//! `plotweb-server/src/sync.rs`; the failure mode is real and was caught by a test.)
//!
//! # Still additive
//!
//! Every REST write remains in place for a book that is not cut over; this only moves
//! CRDT bytes. Which books sync is [`enabled_for_book`]'s answer.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use automerge::sync::{Message as SyncMessage, State as SyncState, SyncDoc};

use crate::store::AppStore;

/// How long after a local change before pushing it (coalesces a burst of edits).
const NUDGE_DEBOUNCE_MS: u32 = 1_500;
/// Idle poll interval — how quickly another device's change shows up here.
const POLL_INTERVAL_MS: u32 = 20_000;
/// First backoff step after a server error; doubles up to [`MAX_BACKOFF_MS`].
const BASE_BACKOFF_MS: u32 = 5_000;
const MAX_BACKOFF_MS: u32 = 300_000;
/// Guard against a pathological exchange that never settles.
const MAX_ROUNDS: u32 = 12;

/// Which document a registration refers to, and therefore which local module owns the
/// CRDT and which URL syncs it.
#[derive(Clone, PartialEq, Eq)]
enum Doc {
    /// The signed-in account's index (`user:{id}`), synced at `/api/sync/user`.
    User(String),
    /// A book's structure (`book:{id}`).
    Book(String),
    /// A chapter or note **body** (`chapter:{id}` / `note:{id}`). Its CRDT lives
    /// inside the editor session rather than in a doc we hold directly, so it is
    /// driven through the `EditorHandle` seam and only while that body is open.
    Body { doc_id: String, book_id: String },
}

impl Doc {
    fn url(&self) -> String {
        match self {
            Doc::User(_) => "/api/sync/user".to_string(),
            Doc::Book(id) => format!("/api/books/{id}/sync/book:{id}"),
            Doc::Body { doc_id, book_id } => format!("/api/books/{book_id}/sync/{doc_id}"),
        }
    }

    fn label(&self) -> String {
        match self {
            Doc::User(id) => format!("user:{id}"),
            Doc::Book(id) => format!("book:{id}"),
            Doc::Body { doc_id, .. } => doc_id.clone(),
        }
    }

    /// The next protocol message to send, or `None` when we have nothing left (which
    /// is what ends an exchange) or the document is no longer open.
    fn generate(&self, state: &mut SyncState) -> Option<Vec<u8>> {
        match self {
            Doc::User(id) => crate::local_user::with_user_doc(id, |d| {
                d.sync().generate_sync_message(state).map(|m| m.encode())
            })
            .flatten(),
            Doc::Book(id) => crate::local_book::with_book_doc(id, |d| {
                d.sync().generate_sync_message(state).map(|m| m.encode())
            })
            .flatten(),
            // Bodies do not use this path at all — see `exchange_body`. Their CRDT is
            // yrs (rinch #190), which reconciles with a state vector and a diff rather
            // than a multi-round message protocol.
            Doc::Body { .. } => None,
        }
    }

    /// Merge a peer's message. `None` means the document went away mid-exchange;
    /// `Some(changed)` reports whether it actually moved.
    fn integrate(&self, state: &mut SyncState, message: SyncMessage) -> Option<bool> {
        match self {
            Doc::User(id) => crate::local_user::with_user_doc(id, |d| {
                let before = d.get_heads();
                d.sync().receive_sync_message(state, message).is_ok() && d.get_heads() != before
            }),
            Doc::Book(id) => crate::local_book::with_book_doc(id, |d| {
                let before = d.get_heads();
                d.sync().receive_sync_message(state, message).is_ok() && d.get_heads() != before
            }),
            // Integrating through the *attached session* is what keeps the editor and
            // the CRDT in step: it rebuilds the model from the converged document and
            // re-projects the view. Never load content into the editor behind the
            // session's back — that is the chapter-crosstalk failure mode.
            // Unreachable: `generate` never produces a message for a body, so no reply
            // is ever integrated through here.
            Doc::Body { .. } => Some(false),
        }
    }

    /// Persist after a merge, then re-project into the render signals so a change
    /// that arrived from another device appears without a reload.
    fn persist_and_project(&self, store: &AppStore) {
        match self {
            Doc::User(id) => {
                crate::local_user::persist_user(id);
                crate::local_user::project_books(store.clone());
            }
            Doc::Book(id) => {
                crate::local_book::persist_book(id);
                crate::local_book::project(store.clone());
            }
            // A body needs no projection — the session updated the editor itself — but
            // the merged state must become the stored base, or a reopen would replay a
            // delta log that never saw these changes.
            Doc::Body { doc_id, .. } => {
                // A swept body is published when its exchange ends (`finish_headless`),
                // not per round — there is no editor to keep in step meanwhile.
                if !crate::local_store::body_is_open(doc_id) {
                    return;
                }
                let doc_id = doc_id.clone();
                crate::local_store::spawn(async move {
                    if let Err(e) = crate::local_store::republish_body(&doc_id).await {
                        log::warn!("sync {doc_id}: republish failed: {e}");
                    }
                });
            }
        }
    }

    /// Whether this doc is still the one open in its module. A cycle for a book the
    /// user has navigated away from is abandoned rather than resurrected.
    fn still_open(&self) -> bool {
        match self {
            Doc::User(id) => crate::local_user::open_user_id().as_deref() == Some(id),
            Doc::Book(id) => crate::local_book::open_book_id().as_deref() == Some(id),
            Doc::Body { doc_id, .. } => crate::local_store::body_is_open(doc_id),
        }
    }
}

/// Per-document engine state. `Unauthed` is terminal until the next
/// [`register_user`] (i.e. the next sign-in) — a background loop must never
/// retry-storm a 401.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Idle,
    Running,
    Unauthed,
}

struct Entry {
    doc: Doc,
    phase: Phase,
    /// Consecutive failures, for the backoff curve.
    failures: u32,
    /// A cycle was requested while one was running; run once more when it ends.
    again: bool,
    /// A poll/backoff timer is already armed (so we don't stack timers).
    armed: bool,
    /// The delay that armed timer was set for, and its handle — so a request for a
    /// *sooner* wake-up can replace it rather than being dropped. Without this a nudge
    /// is silently discarded whenever a poll is already pending, which is always: every
    /// cycle ends by arming the next poll.
    armed_delay_ms: u32,
    timer: Option<rinch_core::TimeoutHandle>,
}

thread_local! {
    /// Registered documents, keyed by label. Small and short-lived: one `user:` entry
    /// plus the open book.
    static ENGINE: RefCell<HashMap<String, Entry>> = RefCell::new(HashMap::new());
    /// The app store, for re-projecting merged changes. Set on first registration.
    static STORE: RefCell<Option<AppStore>> = const { RefCell::new(None) };
}

// ── Whether sync runs, and for which documents ───────────────────────────────
//
// Slice 3 shipped this as one global opt-in flag, off by default, because sync was
// additive: every write still went to git, so a device with sync off lost nothing.
// One writer (#48/#49) ended that. For a **cut-over** book the client sends no body
// content over REST and the server drops any that arrives, so sync is the only path
// an edit has — and a device with sync off is writing to itself.
//
// So the gate is no longer global: **cutover implies sync**, per book. Pre-cutover
// books are unchanged (sync stays off unless asked for), which matters because with
// sync on and cutover off both writers are live at once — a whole-content PUT into
// git and yrs ops into the canonical copy, with nothing mirroring between them.
//
// The flag survives as an override in both directions: `"1"` forces sync on for every
// book (how the e2e suite and the native app ask for it), `"0"` is the kill switch for
// a device where sync itself is the problem.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Gate {
    /// `plotweb_sync=1` — sync every open document, cut over or not.
    On,
    /// `plotweb_sync=0` — sync nothing. The escape hatch.
    Off,
    /// Unset: sync a book iff it is cut over.
    ByBook,
}

fn gate_of(flag: Option<&str>) -> Gate {
    match flag {
        Some("1") => Gate::On,
        Some("0") => Gate::Off,
        _ => Gate::ByBook,
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn gate() -> Gate {
    gate_of(std::env::var("PLOTWEB_SYNC").ok().as_deref())
}

#[cfg(target_arch = "wasm32")]
fn gate() -> Gate {
    let flag = crate::platform::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .and_then(|s| s.get_item("plotweb_sync").ok().flatten());
    gate_of(flag.as_deref())
}

thread_local! {
    /// Books known to be cut over. Server-derived (it arrives with the book's REST
    /// payload) and cached in local storage, so a cold start knows before it can ask
    /// — see [`crate::local_store::cut_over_books`].
    static CUTOVER: RefCell<std::collections::HashSet<String>> =
        RefCell::new(std::collections::HashSet::new());
}

/// The decision itself, without asking where the book or the flag came from.
fn allows(gate: Gate, cut_over: bool) -> bool {
    match gate {
        Gate::On => true,
        Gate::Off => false,
        Gate::ByBook => cut_over,
    }
}

/// Whether sync carries this book's documents on this device.
pub fn enabled_for_book(book_id: &str) -> bool {
    allows(gate(), book_is_cut_over(book_id))
}

/// Whether sync carries the account's `user:` index.
///
/// It belongs to no book, so it rides along as soon as *any* book on this device is
/// cut over: the index is how a book created on another device appears here at all,
/// and a cut-over book whose card never arrives cannot be opened to be synced.
pub fn enabled_for_user() -> bool {
    allows(gate(), CUTOVER.with(|c| !c.borrow().is_empty()))
}

/// Whether `book_id` is cut over as far as this device knows.
pub fn book_is_cut_over(book_id: &str) -> bool {
    CUTOVER.with(|c| c.borrow().contains(book_id))
}

/// Record what the server said about a book, and act on it: learning that a book is
/// cut over is what switches sync on for it, so anything already open must be
/// registered now rather than at the next navigation.
pub fn note_cutover(book_id: &str, cut_over: bool, store: AppStore) {
    let changed = CUTOVER.with(|c| {
        let mut set = c.borrow_mut();
        if cut_over {
            set.insert(book_id.to_string())
        } else {
            set.remove(book_id)
        }
    });
    if !changed {
        return;
    }
    let owned = book_id.to_string();
    crate::local_store::spawn(async move {
        if let Err(e) = crate::local_store::remember_cutover(&owned, cut_over).await {
            log::warn!("sync: could not cache the cutover flag for {owned}: {e}");
        }
    });
    if cut_over && crate::local_book::open_book_id().as_deref() == Some(book_id) {
        register_book(book_id, store);
        // And whatever is already attached: a body registers at attach time, so one
        // opened before the answer arrived (a cold offline start goes straight to the
        // chapter it was left on) would otherwise wait for the next chapter to be
        // opened before any of it left the device.
        for doc_id in crate::local_store::open_body_ids() {
            register_body(&doc_id, book_id);
        }
    }
}

/// Load the cached cutover answers this device kept from previous sessions.
///
/// Additive: an answer already learned this session (a fetch that beat the read) wins,
/// since it is the newer one.
pub fn hydrate_cutover(store: AppStore) {
    crate::local_store::spawn(async move {
        match crate::local_store::cut_over_books().await {
            Ok(books) => {
                for book_id in books {
                    // Through `note_cutover`, so a book already open when the answer
                    // lands starts syncing instead of waiting to be reopened.
                    note_cutover(&book_id, true, store.clone());
                }
            }
            Err(e) => log::warn!("sync: could not read the cached cutover flags: {e}"),
        }
    });
}

/// Register the signed-in account's `user:` doc and sync it now.
///
/// Also clears any previous `Unauthed` phase: a fresh sign-in is exactly the event
/// that makes syncing worth attempting again.
pub fn register_user(user_id: &str, store: AppStore) {
    register(Doc::User(user_id.to_string()), store);
}

/// Register the open book's `book:` doc and sync it now.
///
/// Also registers the account's `user:` index. Under [`Gate::ByBook`] the index's own
/// gate only opens once a cut-over book is known, and at sign-in none was — so without
/// this the book list would stay unsynced until the next launch.
pub fn register_book(book_id: &str, store: AppStore) {
    if !enabled_for_book(book_id) {
        return;
    }
    register(Doc::Book(book_id.to_string()), store.clone());
    arm_sweep(book_id);
    if let Some(user_id) = crate::local_user::open_user_id() {
        let label = format!("user:{user_id}");
        if !ENGINE.with(|e| e.borrow().contains_key(&label)) {
            register_user(&user_id, store);
        }
    }
}

/// Register the body document an editor just attached (`chapter:` / `note:`), and
/// sync it now.
///
/// Called by `local_store` once a session is attached, because only then does the
/// CRDT exist to sync. A body syncs while it is open; the previous body's
/// registration is dropped, since a cycle against a closed editor has nothing to
/// drive. (Background sync of unopened bodies is slice 5.)
pub fn register_body(doc_id: &str, book_id: &str) {
    if !enabled_for_book(book_id) {
        return;
    }
    let store = STORE.with(|s| s.borrow().clone());
    let Some(store) = store else {
        // No dashboard/book has registered yet, so we have no AppStore to project
        // with. Bodies need no projection, but the engine keeps one store handle;
        // registering later (on the next open) is harmless.
        return;
    };
    // Only one body syncs at a time per surface; drop any previous body entry so a
    // closed editor's document isn't polled forever.
    ENGINE.with(|e| {
        e.borrow_mut()
            .retain(|_, entry| !matches!(entry.doc, Doc::Body { .. }) || entry.doc.still_open())
    });
    register(
        Doc::Body {
            doc_id: doc_id.to_string(),
            book_id: book_id.to_string(),
        },
        store,
    );
}

/// Whether this body document is actively syncing, and so must **not** also be saved
/// over REST.
///
/// The co-existence rule from `docs/sync-engine-design.md`, finally enforced: a client
/// must never be both writers of the same document. Sync carries incremental edits; a
/// REST save carries a whole snapshot of the editor model taken at some earlier moment.
/// Under cutover the server applies that snapshot *into* the canonical document as a
/// diff — so if sync has already carried a deletion across, the snapshot's diff says
/// "put it back", and the text reappears under the author's cursor mid-sentence.
///
/// Dropping the REST write drops a duplicate, not a save: the edit is already in the
/// local store (which is durable) and already on its way up as a change. A document
/// parked for sign-in is *not* syncing — REST is the only writer then, and must stay on.
pub fn body_is_syncing(doc_id: &str) -> bool {
    // No separate gate: a body is in the engine only if its book's gate let it in.
    ENGINE.with(|e| {
        e.borrow()
            .get(doc_id)
            .is_some_and(|entry| !matches!(entry.phase, Phase::Unauthed))
    })
}

fn register(doc: Doc, store: AppStore) {
    let allowed = match &doc {
        Doc::User(_) => enabled_for_user(),
        Doc::Book(book_id) => enabled_for_book(book_id),
        Doc::Body { book_id, .. } => enabled_for_book(book_id),
    };
    if !allowed {
        return;
    }
    STORE.with(|s| *s.borrow_mut() = Some(store));
    let label = doc.label();
    ENGINE.with(|e| {
        e.borrow_mut().insert(
            label.clone(),
            Entry {
                doc,
                phase: Phase::Idle,
                failures: 0,
                again: false,
                armed: false,
                armed_delay_ms: 0,
                timer: None,
            },
        );
    });
    begin(&label);
}

/// A local change landed — push it soon (debounced, so a burst of edits is one push).
/// A body's local edit landed — push it soon, rather than waiting for the poll.
///
/// Without this a chapter edit sat in the browser for up to [`POLL_INTERVAL_MS`] (20s)
/// before the server heard about it, while the editor said "Saved" the whole time. That
/// is true — it was saved locally — but under cutover the canonical copy is the only
/// one anybody else reads, so for twenty seconds the text existed on exactly one
/// device. It also made every race around switching chapters twenty seconds wide
/// instead of one and a half.
pub fn nudge_body(doc_id: &str) {
    // No gate of its own: `arm_timer` no-ops for a label the engine does not hold, and
    // registration is what the gate decides.
    arm_timer(doc_id, NUDGE_DEBOUNCE_MS);
}

pub fn nudge(label_owner: &str, is_book: bool) {
    let label = if is_book {
        format!("book:{label_owner}")
    } else {
        format!("user:{label_owner}")
    };
    arm_timer(&label, NUDGE_DEBOUNCE_MS);
}

// ── Bodies: the yrs exchange (slices 4 and 5) ────────────────────────────────
//
// Since rinch #190 a body's CRDT is yrs, which reconciles in two fixed steps rather
// than Automerge's multi-round loop: send a state vector, receive the update you lack
// plus the peer's state vector, then send back what the peer lacks. Both directions
// are opaque bytes through the `EditorHandle` seam, so this file needs no CRDT
// dependency of its own.

/// How often a book's unopened bodies are swept. Deliberately slower than the
/// per-document poll: it is a catch-up pass, not a latency path.
const SWEEP_INTERVAL_MS: u32 = 60_000;

thread_local! {
    /// Books with a sweep timer already armed, so re-registering a book doesn't stack
    /// sweeps on top of each other.
    static SWEEPING: RefCell<std::collections::HashSet<String>> =
        RefCell::new(std::collections::HashSet::new());
}

/// Run one exchange for the open body `doc_id`.
///
/// Ends by republishing the local snapshot: the merged state must become the stored
/// base, or a reopen would replay a delta log that never saw the peer's changes.
fn exchange_body(label: String, doc: Doc) {
    let Doc::Body { doc_id, .. } = &doc else {
        return;
    };
    let doc_id = doc_id.clone();

    let Some(Some(state_vector)) =
        crate::local_store::with_body_session(&doc_id, |s| s.handle.collab_state_vector())
    else {
        finish(&label, true);
        return;
    };

    let update_url = format!("{}/update", doc.url());
    let exchange_url = doc.url();
    crate::api::post_bytes(&exchange_url, state_vector, move |result| match result {
        Ok(framed) => {
            let Some((diff, server_state_vector)) = unframe(&framed) else {
                log::warn!("sync {label}: malformed exchange reply");
                finish(&label, false);
                return;
            };

            // Apply what we lack, through the attached session so the editor and the
            // CRDT stay in step.
            if !diff.is_empty() {
                let applied = crate::local_store::with_body_session(&doc_id, |s| {
                    s.handle.collab_receive(&diff)
                });
                if applied == Some(true) {
                    let doc_id = doc_id.clone();
                    crate::local_store::spawn(async move {
                        if let Err(e) = crate::local_store::republish_body(&doc_id).await {
                            log::warn!("sync {doc_id}: republish failed: {e}");
                        }
                    });
                }
            }

            // Send back what the server lacks. `collab_sync_diff` is a pure read: it
            // broadcasts nothing and leaves the local delta stream untouched.
            let ours = crate::local_store::with_body_session(&doc_id, |s| {
                s.handle.collab_sync_diff(&server_state_vector)
            });
            match ours {
                Some(Some(update)) if !update.is_empty() => {
                    crate::api::post_bytes(&update_url, update, move |result| match result {
                        Ok(_) => finish(&label, true),
                        Err(e) if e.status == 401 => park_unauthed(&label),
                        Err(e) if e.status == 403 || e.status == 404 => {
                            unregister(&label, e.status)
                        }
                        Err(_) => finish(&label, false),
                    });
                }
                // Nothing to send, or the body closed under us.
                _ => finish(&label, true),
            }
        }
        // The server can see, from the documents themselves, that ours descends from
        // nothing it holds — merging would concatenate rather than converge. Replace
        // ours with the canonical copy. This is authoritative where the local
        // provenance flag is only a memory: after a reconcile resolves a document in
        // git's favour, our flag still says "synced" and would be wrong.
        Err(e) if e.status == 409 => {
            log::info!("sync {label}: unrelated to the canonical document; replacing ours");
            take_server_body(label, doc, doc_id);
        }
        Err(e) if e.status == 401 => park_unauthed(&label),
        Err(e) if e.status == 403 || e.status == 404 => unregister(&label, e.status),
        Err(_) => finish(&label, false),
    });
}

/// Split the exchange reply: `[u32 LE length][diff][server state vector]`.
fn unframe(framed: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    if framed.len() < 4 {
        return None;
    }
    let len = u32::from_le_bytes(framed[0..4].try_into().ok()?) as usize;
    let rest = framed.get(4..)?;
    let diff = rest.get(..len)?.to_vec();
    let state_vector = rest.get(len..)?.to_vec();
    Some((diff, state_vector))
}

/// Arm the recurring sweep for `book_id` (idempotent).
fn arm_sweep(book_id: &str) {
    let fresh = SWEEPING.with(|s| s.borrow_mut().insert(book_id.to_string()));
    if !fresh {
        return;
    }
    schedule_sweep(book_id.to_string());
}

fn schedule_sweep(book_id: String) {
    // `unowned` for the same reason the debounced saves are: a callback parked during
    // a render dies with that scope (rinch #141), and the sync loop must outlive any
    // component that happened to be rendering when a nudge armed it.
    rinch_core::reactive::unowned(|| rinch_core::set_timeout(SWEEP_INTERVAL_MS, move || {
        // Stop sweeping a book the author has left; the next open re-arms it.
        if crate::local_book::open_book_id().as_deref() != Some(book_id.as_str()) {
            SWEEPING.with(|s| {
                s.borrow_mut().remove(&book_id);
            });
            return;
        }
        sweep_book(book_id.clone());
        schedule_sweep(book_id);
    }));
}

/// Bring the open book's *unopened* bodies up to date.
///
/// **Pull-only, by construction.** Editing a body requires its editor, so a body no
/// editor holds cannot be ahead of the server — only behind. That turns the sweep into
/// a fetch rather than a merge: no protocol, no CRDT library on this side, and no way
/// for a background pass to disturb what an editor is showing.
fn sweep_book(book_id: String) {
    let url = format!("/api/books/{book_id}/sync/heads");
    crate::api::get::<HashMap<String, Vec<String>>>(&url, move |result| {
        let Ok(server) = result else { return };
        for (doc_id, fingerprint) in server {
            if !doc_id.starts_with("chapter:") && !doc_id.starts_with("note:") {
                continue;
            }
            // The open body runs its own exchange.
            if crate::local_store::body_is_open(&doc_id) {
                continue;
            }
            let Some(fingerprint) = fingerprint.first().cloned() else {
                continue;
            };
            pull_body(doc_id, book_id.clone(), fingerprint);
        }
    });
}

/// Fetch a body whose server fingerprint differs from the one we stored.
fn pull_body(doc_id: String, book_id: String, fingerprint: String) {
    crate::local_store::spawn(async move {
        match crate::local_store::body_fingerprint(&doc_id).await {
            Ok(Some(known)) if known == fingerprint => return,
            Ok(_) => {}
            Err(e) => {
                log::warn!("sync sweep {doc_id}: {e}");
                return;
            }
        }
        let url = format!("/api/books/{book_id}/sync/{doc_id}");
        crate::api::get_bytes(&url, move |result| {
            let Ok(Some(bytes)) = result else { return };
            crate::local_store::spawn(async move {
                if let Err(e) =
                    crate::local_store::install_headless_body(&doc_id, &bytes, &fingerprint).await
                {
                    log::warn!("sync sweep {doc_id}: install failed: {e}");
                }
            });
        });
    });
}

// ── Provenance handshake for bodies (design §D8) ─────────────────────────────

/// Establish shared history with the server before a body's first exchange, then
/// start syncing it.
///
/// A body doc seeded from REST shares no history with the server's canonical copy of
/// the same chapter, so merging the two would concatenate rather than deduplicate.
/// Exactly one of two things must happen first:
///
/// - **We claim it.** The canonical copy is still the migration backfill's — frozen at
///   backfill time while git moved on — so it is provisional and we replace it with
///   our git-current document.
/// - **We take theirs.** Another device already owns it; our copy is the disjoint one,
///   so we install the server's wholesale.
///
/// Only after that does the protocol run. Structure docs skip all of this: they are
/// created by the client and the server has no independently-built copy.
fn establish_body_provenance(label: String, doc: Doc) {
    let Doc::Body { doc_id, .. } = &doc else {
        start_cycle(&label);
        return;
    };
    let doc_id = doc_id.clone();

    let known = crate::local_store::with_body_session(&doc_id, |s| s.handle.collab_snapshot());
    let Some(Some(snapshot)) = known else {
        // The body closed before we got to it.
        return;
    };

    let claim_url = format!("{}/adopt", doc.url());
    crate::api::post_bytes(&claim_url, snapshot, move |result| match result {
        // The reply says whether ours was adopted; both answers lead to the same
        // action, so it is not read. See below.
        Ok(_) => {
            // Two outcomes, one action. Adopted means ours *is* the canonical copy and
            // the histories match by construction. Refused means a client already owns
            // it — and the adopt endpoint's own answer to that is "use the sync protocol
            // instead", because an exchange merges: anything typed here since the last
            // push survives it.
            //
            // Refusal used to replace our copy with the server's, which threw those
            // edits away. Every attach re-ran provenance, so switching chapters and
            // coming back before a push had landed silently lost whatever had been
            // typed — reported as text vanishing on switching, "even after it says
            // saved". Replacing is only right when the two share no history at all, and
            // that is not something to guess: the server detects it from the documents
            // themselves and answers 409, handled in `exchange_body`.
            let (label, doc_id) = (label.clone(), doc_id.clone());
            crate::local_store::spawn(async move {
                if let Err(e) = crate::local_store::mark_body_shares_server_history(&doc_id).await {
                    log::warn!("sync {doc_id}: {e}");
                }
                start_cycle(&label);
            });
        }
        Err(e) if e.status == 401 => park_unauthed(&label),
        Err(e) if e.status == 403 || e.status == 404 => unregister(&label, e.status),
        Err(_) => finish(&label, false),
    });
}

/// Install the server's canonical body document over ours, then start syncing — or
/// refuse, when it is not our document at all.
///
/// Two very different situations answer `409`, and until the server carried a document
/// identity they were indistinguishable:
///
/// - **Forked.** Same lineage — the same chapter, rebuilt from git by a reconcile, so
///   our history was abandoned deliberately. Taking the canonical copy is right, and
///   what we hold is kept first (`preserve_local_copy`) so nothing we wrote is lost.
/// - **Unrelated.** A different lineage — not this chapter's document at all. Replacing
///   ours would destroy one document to install another, so we keep ours and stop. A
///   person has to look.
fn take_server_body(label: String, doc: Doc, doc_id: String) {
    crate::api::get_canonical(&doc.url(), move |result| match result {
        Ok(canonical) if canonical.bytes.is_none() => {
            // No canonical copy at all: nothing to reconcile, our document stands and
            // the first exchange will establish it server-side.
            let (label, doc_id) = (label.clone(), doc_id.clone());
            crate::local_store::spawn(async move {
                if let Err(e) = crate::local_store::mark_body_shares_server_history(&doc_id).await {
                    log::warn!("sync {doc_id}: {e}");
                }
                start_cycle(&label);
            });
        }
        Ok(canonical) => {
            let (label, doc_id) = (label.clone(), doc_id.clone());
            crate::local_store::spawn(async move {
                let bytes = canonical.bytes.unwrap_or_default();
                let ours = match crate::local_store::local_identity(&doc_id).await {
                    Ok((lineage, _)) => lineage,
                    Err(e) => {
                        log::warn!("sync {doc_id}: {e}");
                        None
                    }
                };
                if !identities_agree(ours.as_deref(), canonical.lineage.as_deref()) {
                    log::warn!(
                        "sync {doc_id}: the canonical copy is a different document (ours \
                         {ours:?}, theirs {:?}) — keeping ours, and stopping",
                        canonical.lineage
                    );
                    unregister(&label, 409);
                    return;
                }
                match crate::local_store::install_server_body(&doc_id, &bytes).await {
                    Ok(true) => {
                        if let Err(e) = crate::local_store::record_identity(
                            &doc_id,
                            canonical.lineage.as_deref(),
                            canonical.epoch,
                        )
                        .await
                        {
                            log::warn!("sync {doc_id}: {e}");
                        }
                        start_cycle(&label);
                    }
                    // The body closed, or the server's document is outside the collab
                    // scope. Leave it unsynced rather than risk a duplicate merge.
                    Ok(false) => {}
                    Err(e) => log::warn!("sync {doc_id}: install failed: {e}"),
                }
            });
        }
        Err(e) if e.status == 401 => park_unauthed(&label),
        Err(e) if e.status == 403 || e.status == 404 => unregister(&label, e.status),
        Err(_) => finish(&label, false),
    });
}

/// Whether a canonical document is the same document as ours.
///
/// Unknown on either side is **not** a disagreement: a document from before identities
/// existed, or one this device has never adopted, has nothing to contradict. Only two
/// known-and-different lineages mean "not the same document".
pub(crate) fn identities_agree(ours: Option<&str>, theirs: Option<&str>) -> bool {
    match (ours, theirs) {
        (Some(a), Some(b)) => a == b,
        _ => true,
    }
}

/// Park a document as signed-out: stop, quietly, until the next sign-in.
fn park_unauthed(label: &str) {
    log::info!("sync {label}: not signed in; pausing sync");
    ENGINE.with(|eng| {
        if let Some(entry) = eng.borrow_mut().get_mut(label) {
            entry.phase = Phase::Unauthed;
            entry.again = false;
        }
    });
}

/// Drop a document that isn't ours or no longer exists — retrying cannot help.
fn unregister(label: &str, status: u16) {
    log::warn!("sync {label}: rejected ({status}); unregistering");
    ENGINE.with(|eng| {
        eng.borrow_mut().remove(label);
    });
}

// ── The cycle ────────────────────────────────────────────────────────────────

/// Begin an exchange for `label`, unless one is already running (in which case it is
/// flagged to run again afterwards) or we are signed out.
fn start_cycle(label: &str) {
    let ready = ENGINE.with(|e| {
        let mut map = e.borrow_mut();
        let Some(entry) = map.get_mut(label) else {
            return false;
        };
        match entry.phase {
            Phase::Unauthed => false,
            Phase::Running => {
                entry.again = true;
                false
            }
            Phase::Idle => {
                entry.phase = Phase::Running;
                true
            }
        }
    });
    if ready {
        match doc_of(label) {
            Some(doc @ Doc::Body { .. }) => exchange_body(label.to_string(), doc),
            _ => round(label.to_string(), Rc::new(RefCell::new(SyncState::new())), 0),
        }
    }
}

/// Entry point for a freshly-registered document: a body must settle provenance
/// before its first exchange (§D8); everything else can start syncing at once.
fn begin(label: &str) {
    match doc_of(label) {
        Some(doc @ Doc::Body { .. }) => {
            let doc_id = doc.label();
            let label = label.to_string();
            crate::local_store::spawn(async move {
                match crate::local_store::body_shares_server_history(&doc_id).await {
                    Ok(true) => start_cycle(&label),
                    Ok(false) => establish_body_provenance(label, doc),
                    Err(e) => log::warn!("sync {doc_id}: provenance read failed: {e}"),
                }
            });
        }
        Some(_) => start_cycle(label),
        None => {}
    }
}

/// One request/response of an exchange, recursing until we have nothing left to send.
fn round(label: String, state: Rc<RefCell<SyncState>>, n: u32) {
    let Some(doc) = doc_of(&label) else { return };
    if !doc.still_open() {
        finish(&label, true);
        return;
    }
    if n >= MAX_ROUNDS {
        log::warn!("sync {label}: exchange did not settle in {MAX_ROUNDS} rounds");
        finish(&label, false);
        return;
    }

    // Termination is ours: the server is stateless and always replies.
    let Some(msg) = doc.generate(&mut state.borrow_mut()) else {
        finish(&label, true);
        return;
    };

    let url = doc.url();
    crate::api::post_bytes(&url, msg, move |result| match result {
        Ok(reply) => {
            if reply.is_empty() {
                finish(&label, true);
                return;
            }
            let Some(doc) = doc_of(&label) else { return };
            match SyncMessage::decode(&reply) {
                Ok(message) => {
                    // Scoped so the borrow ends before `state` moves into the next
                    // round.
                    let outcome = doc.integrate(&mut state.borrow_mut(), message);
                    match outcome {
                        // Changed: persist (and re-project, for the structure docs).
                        Some(true) => {
                            STORE.with(|s| {
                                if let Some(store) = s.borrow().as_ref() {
                                    doc.persist_and_project(store);
                                }
                            });
                            round(label, state, n + 1);
                        }
                        // Accepted but nothing moved — keep the exchange going; our
                        // own `generate` returning `None` is what ends it.
                        Some(false) => round(label, state, n + 1),
                        // The document closed under us.
                        None => finish(&label, false),
                    }
                }
                Err(e) => {
                    log::warn!("sync {label}: undecodable reply: {e}");
                    finish(&label, false);
                }
            }
        }
        // The server can see, from the documents themselves, that ours descends from
        // nothing it holds. Merging would concatenate the two — the book coming back
        // with every chapter twice (§D8). Replace ours with the canonical copy; the
        // same answer the body path gives, for the same reason.
        Err(e) if e.status == 409 => {
            log::info!("sync {label}: unrelated to the canonical document; replacing ours");
            take_server_structure(label);
        }
        // Signed out: stop, quietly, until the next sign-in re-registers us.
        Err(e) if e.status == 401 => park_unauthed(&label),
        // 403/404 — not ours, or gone. Retrying can't help; drop the registration.
        Err(e) if e.status == 403 || e.status == 404 => unregister(&label, e.status),
        // Offline (status 0) or a server error: back off and try later.
        Err(e) => {
            log::debug!("sync {label}: {} {}", e.status, e.message);
            finish(&label, false);
        }
    });
}

/// Install the server's canonical structure document over ours, then sync again.
fn take_server_structure(label: String) {
    let Some(doc) = doc_of(&label) else { return };
    let doc_id = doc.label();
    crate::api::get_canonical(&doc.url(), move |result| match result {
        // No canonical copy at all: nothing to reconcile, ours stands.
        Ok(canonical) if canonical.bytes.is_none() => finish(&label, true),
        Ok(canonical) => {
            // Same rule as bodies: a different lineage is a different document, and
            // installing it would destroy ours to make room for something that was
            // never this book's structure.
            let (label, doc, doc_id) = (label.clone(), doc.clone(), doc_id.clone());
            crate::local_store::spawn(async move {
                let ours = match crate::local_store::local_identity(&doc_id).await {
                    Ok((lineage, _)) => lineage,
                    Err(e) => {
                        log::warn!("sync {doc_id}: {e}");
                        None
                    }
                };
                if !identities_agree(ours.as_deref(), canonical.lineage.as_deref()) {
                    log::warn!(
                        "sync {doc_id}: the canonical copy is a different document (ours \
                         {ours:?}, theirs {:?}) — keeping ours, and stopping",
                        canonical.lineage
                    );
                    unregister(&label, 409);
                    return;
                }

                let bytes = canonical.bytes.unwrap_or_default();
                let installed = match &doc {
                    Doc::Book(id) => crate::local_book::install_server_book(id, &bytes),
                    Doc::User(id) => crate::local_user::install_server_user(id, &bytes),
                    // Bodies take `take_server_body` instead; they never reach this path.
                    Doc::Body { .. } => false,
                };
                if installed {
                    if let Err(e) = crate::local_store::record_identity(
                        &doc_id,
                        canonical.lineage.as_deref(),
                        canonical.epoch,
                    )
                    .await
                    {
                        log::warn!("sync {doc_id}: {e}");
                    }
                    STORE.with(|s| {
                        if let Some(store) = s.borrow().as_ref() {
                            doc.persist_and_project(store);
                        }
                    });
                    start_cycle(&label);
                } else {
                    // A different book is open now, or the copy did not load. Leave it
                    // rather than risk merging two unrelated documents.
                    finish(&label, false);
                }
            });
        }
        Err(e) if e.status == 401 => park_unauthed(&label),
        Err(e) if e.status == 403 || e.status == 404 => unregister(&label, e.status),
        Err(_) => finish(&label, false),
    });
}

/// End an exchange: reset or advance the backoff, then arm the next attempt.
fn finish(label: &str, success: bool) {
    let next = ENGINE.with(|e| {
        let mut map = e.borrow_mut();
        let Some(entry) = map.get_mut(label) else {
            return None;
        };
        entry.phase = Phase::Idle;
        if success {
            entry.failures = 0;
        } else {
            entry.failures = entry.failures.saturating_add(1);
        }
        let run_again = std::mem::take(&mut entry.again);
        Some(if run_again {
            0
        } else if success {
            POLL_INTERVAL_MS
        } else {
            backoff(entry.failures)
        })
    });
    match next {
        Some(0) => start_cycle(label),
        Some(delay) => arm_timer(label, delay),
        None => {}
    }
}

/// Exponential backoff with a ceiling: 5s, 10s, 20s … capped at 5 min.
fn backoff(failures: u32) -> u32 {
    BASE_BACKOFF_MS
        .saturating_mul(1u32 << failures.min(6))
        .min(MAX_BACKOFF_MS)
}

/// Arm a one-shot timer to start a cycle, unless one is already pending. Uses
/// rinch's cross-platform timer (`window.setTimeout` on web, the shared timer thread
/// natively), so this works in both shells.
/// Arm a wake-up for `label` in `delay_ms`, unless one is already coming sooner.
///
/// A sooner request *replaces* a later one. That is the whole point of a nudge: every
/// cycle ends by arming the next poll, so without replacement an entry is permanently
/// armed for 20s and a 1.5s nudge is dropped every time — which is exactly what
/// happened to bodies, leaving a chapter edit on one device for up to twenty seconds
/// while the editor said "Saved".
///
/// Comparing the requested delays rather than the remaining time is deliberately
/// approximate: a pending poll might have less than 1.5s left, and this would still
/// replace it. Erring toward waking sooner costs one extra exchange; erring the other
/// way is what the bug was.
fn arm_timer(label: &str, delay_ms: u32) {
    let stale = ENGINE.with(|e| {
        let mut map = e.borrow_mut();
        let Some(entry) = map.get_mut(label) else {
            return None;
        };
        if entry.armed && delay_ms >= entry.armed_delay_ms {
            return None;
        }
        let previous = entry.timer.take();
        entry.armed = true;
        entry.armed_delay_ms = delay_ms;
        Some(previous)
    });
    let Some(previous) = stale else {
        return;
    };
    if let Some(handle) = previous {
        rinch_core::clear_timeout(handle);
    }

    let owned = label.to_string();
    let handle = rinch_core::reactive::unowned(|| {
        rinch_core::set_timeout(delay_ms, move || {
            ENGINE.with(|e| {
                if let Some(entry) = e.borrow_mut().get_mut(&owned) {
                    entry.armed = false;
                    entry.timer = None;
                }
            });
            start_cycle(&owned);
        })
    });
    ENGINE.with(|e| {
        if let Some(entry) = e.borrow_mut().get_mut(label) {
            entry.timer = Some(handle);
        }
    });
}

fn doc_of(label: &str) -> Option<Doc> {
    ENGINE.with(|e| e.borrow().get(label).map(|entry| entry.doc.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_then_caps() {
        assert_eq!(backoff(0), BASE_BACKOFF_MS);
        assert_eq!(backoff(1), BASE_BACKOFF_MS * 2);
        assert_eq!(backoff(2), BASE_BACKOFF_MS * 4);
        assert_eq!(backoff(20), MAX_BACKOFF_MS, "backoff is capped");
    }

    #[test]
    fn urls_are_book_scoped() {
        assert_eq!(Doc::User("u1".into()).url(), "/api/sync/user");
        assert_eq!(
            Doc::Book("b1".into()).url(),
            "/api/books/b1/sync/book:b1",
            "the book doc syncs on its own book's route"
        );
    }

    /// The distinction slice 4 exists to make. Two known-and-different lineages mean
    /// "not the same document" — install nothing. Anything else (either side unknown,
    /// a pre-identity document, a device that has never adopted one) has nothing to
    /// contradict, so the ordinary path runs.
    #[test]
    fn only_two_known_and_different_lineages_disagree() {
        assert!(
            identities_agree(Some("L"), Some("L")),
            "the same chapter, rebuilt: reconcilable"
        );
        assert!(
            !identities_agree(Some("L"), Some("M")),
            "a different document must never be installed over ours"
        );
        assert!(
            identities_agree(None, Some("M")),
            "a device that has never adopted an identity has nothing to contradict"
        );
        assert!(
            identities_agree(Some("L"), None),
            "a canonical copy from before identities existed is not a conflict"
        );
        assert!(identities_agree(None, None));
    }
}

#[cfg(test)]
mod seam_canary {
    /// The `rinch` pin must carry the collaboration reconciliation seam on
    /// `EditorHandle`. Body sync is built on these three, and they are what rinch #190
    /// left in place of #182's Automerge trio, so a pin that slips to either side of
    /// that change fails here — loudly, at compile time — rather than at runtime.
    #[test]
    fn the_editor_handle_exposes_the_reconciliation_seam() {
        use crate::rinch_backend::EditorHandle;
        let _state_vector: fn(&EditorHandle) -> Option<Vec<u8>> = EditorHandle::collab_state_vector;
        let _diff: fn(&EditorHandle, &[u8]) -> Option<Vec<u8>> = EditorHandle::collab_sync_diff;
        let _receive: fn(&EditorHandle, &[u8]) -> bool = EditorHandle::collab_receive;
    }
}

#[cfg(test)]
mod gate_tests {
    use super::*;

    #[test]
    fn the_flag_is_an_override_in_both_directions() {
        assert_eq!(gate_of(Some("1")), Gate::On, "the e2e suite and the native app ask with 1");
        assert_eq!(gate_of(Some("0")), Gate::Off, "0 is the kill switch");
        assert_eq!(gate_of(None), Gate::ByBook, "unset is the ordinary case");
        assert_eq!(gate_of(Some("yes")), Gate::ByBook, "anything else is not an override");
    }

    /// The rule this replaced a global opt-in with: a cut-over book has no path to the
    /// server except sync, so sync is not optional for it. A book that is *not* cut
    /// over stays off — with sync on and cutover off there are two live writers of the
    /// same body (a whole-content PUT into git, yrs ops into the canonical copy) and
    /// nothing mirroring between them.
    #[test]
    fn cutover_implies_sync_and_nothing_else_does() {
        assert!(allows(Gate::ByBook, true), "a cut-over book must sync unasked");
        assert!(!allows(Gate::ByBook, false), "a git-backed book must not");
        assert!(allows(Gate::On, false), "the override reaches books that are not cut over");
        assert!(
            !allows(Gate::Off, true),
            "the kill switch must beat cutover, or a device where sync is the problem \
             has nowhere to stand"
        );
    }
}
