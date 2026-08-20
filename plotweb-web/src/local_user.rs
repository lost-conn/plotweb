//! Local-first **dashboard book list** (Phase 2 · Slice 1 · deliverable 3).
//!
//! Where [`crate::local_book`] makes one book's structure durable on the client,
//! this module makes the **account's book list** durable: the small per-book cache
//! the dashboard needs to render the shelf with no network — title, cover, and a
//! sortable `updated_at`. Its persistence unit is a **hand-projected Automerge
//! document** `user:{user_id}` — a plain `automerge::AutoCommit` we build and read
//! directly, persisted through [`crate::local_store::DocStore`] exactly like a
//! `book:` doc.
//!
//! # Schema (locked v1, docs/offline-first-rinch-plan.md §1)
//!
//! ```text
//! ROOT
//!   books: Map<book_id, {
//!     title:      String,   // cached for the dashboard (authoritative copy lives in book: doc)
//!     cover_ref:  String?,  // content-addressed cover ref (Option; may be absent)
//!     updated_at: String,   // "YYYY-MM-DD HH:MM:SS", lexicographically sortable
//!   }>
//! ```
//!
//! `books` is a `Map` (no user-defined order); the projection sorts by `updated_at`
//! descending (newest first), matching the dashboard's newest-first shelf.
//!
//! # Read vs mutate (dual-write)
//!
//! - **Read:** [`enter_user`] seeds the doc from the REST `/api/books` list (first
//!   open) or loads the local doc (subsequent opens), then *projects* it into the
//!   existing [`AppStore::books`] signal — so the shelf renders from the local doc.
//!   On divergence, the **local doc wins** the projection (it is the authoritative
//!   set of books; sync is a later slice).
//! - **Mutate:** book create ([`add_book`]), delete ([`remove_book`]), and
//!   rename / cover change ([`update_book`]) each apply to the local doc immediately
//!   and persist it, **beside** the untouched REST calls (dual-write).
//!
//! The cached entry is a strict subset of the full [`Book`] the dashboard card
//! renders (which also carries description / word-count / timestamps). The
//! projection overlays the cached fields onto the fuller REST `Book` list where a
//! book is present in both; a book present locally but missing from REST (offline)
//! renders from its cached fields alone.
//!
//! Everything here is `!Send`, stays on the main thread, scheduled through
//! [`crate::local_store::spawn`]; persistence is a full-snapshot re-publish per edit
//! (the doc is small), coalesced + serialized like [`crate::local_book`].

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use automerge::transaction::Transactable;
use automerge::{AutoCommit, ObjId, ObjType, ROOT, ReadDoc};

use plotweb_common::Book;

use crate::local_store::{DocStore, spawn};
use crate::store::AppStore;

// ── Sequential, coalescing snapshot persister ────────────────────────────────

/// Persists the user doc's full snapshot through [`DocStore`], serialized so two
/// rapid edits can't race [`DocStore::publish_snapshot`]'s generation pointer.
/// Mirror of [`crate::local_book`]'s persister (the doc is small; snapshot-per-edit).
#[derive(Clone)]
struct Persister {
    store: DocStore,
    pending: Rc<RefCell<Option<Vec<u8>>>>,
    busy: Rc<Cell<bool>>,
}

impl Persister {
    fn new(store: DocStore) -> Self {
        Self {
            store,
            pending: Rc::new(RefCell::new(None)),
            busy: Rc::new(Cell::new(false)),
        }
    }

    fn persist(&self, bytes: Vec<u8>) {
        *self.pending.borrow_mut() = Some(bytes);
        if self.busy.get() {
            return;
        }
        self.busy.set(true);
        let store = self.store.clone();
        let pending = self.pending.clone();
        let busy = self.busy.clone();
        spawn(async move {
            loop {
                let next = pending.borrow_mut().take();
                match next {
                    Some(b) => {
                        if let Err(e) = store.publish_snapshot(&b).await {
                            log::warn!("local-first user: publish failed: {e}");
                        }
                    }
                    None => break,
                }
            }
            busy.set(false);
        });
    }
}

// ── Open user state (one account at a time) ──────────────────────────────────

struct UserState {
    user_id: String,
    doc: AutoCommit,
    persister: Persister,
}

thread_local! {
    /// The signed-in account's book-list doc. One account is open at a time;
    /// [`enter_user`] replaces it. A stale mutation for a different `user_id` is
    /// ignored, so a leaked timer from a previous session can't corrupt it.
    static USER: RefCell<Option<UserState>> = const { RefCell::new(None) };
}

// ── Sync seams ───────────────────────────────────────────────────────────────
// The sync engine ([`crate::sync`]) needs the live CRDT, but must control *when* a
// snapshot is persisted: generating a sync message mutates the doc's internal state
// without changing content (nothing to persist), whereas integrating a peer's message
// does change content (persist + re-project). So the seams are split.

/// The signed-in account whose `user:` doc is open, if any.
pub(crate) fn open_user_id() -> Option<String> {
    USER.with(|u| u.borrow().as_ref().map(|s| s.user_id.clone()))
}

/// Run `f` against the open user's CRDT **without** persisting. `None` if a different
/// account (or none) is open — the caller's work is then moot, not an error.
pub(crate) fn with_user_doc<R>(user_id: &str, f: impl FnOnce(&mut AutoCommit) -> R) -> Option<R> {
    USER.with(|u| {
        let mut slot = u.borrow_mut();
        let state = slot.as_mut()?;
        (state.user_id == user_id).then(|| f(&mut state.doc))
    })
}

/// Persist the open user's doc as it now stands — after the sync engine has merged a
/// peer's changes into it.
pub(crate) fn persist_user(user_id: &str) {
    USER.with(|u| {
        let mut slot = u.borrow_mut();
        let Some(state) = slot.as_mut() else { return };
        if state.user_id != user_id {
            return;
        }
        let bytes = state.doc.save();
        state.persister.persist(bytes);
    });
}

/// Replace the open account's document with the server's canonical copy — the `user:`
/// counterpart of [`crate::local_book::install_server_book`], for the same §D8 reason.
pub(crate) fn install_server_user(user_id: &str, bytes: &[u8]) -> bool {
    let Ok(doc) = AutoCommit::load(bytes) else {
        log::warn!("sync user:{user_id}: canonical copy did not load");
        return false;
    };
    USER.with(|u| {
        let mut slot = u.borrow_mut();
        let Some(state) = slot.as_mut() else {
            return false;
        };
        if state.user_id != user_id {
            return false;
        }
        state.doc = doc;
        state.persister.persist(state.doc.save());
        true
    })
}

/// Run `f` against the open user's doc iff it matches `user_id`, then persist the
/// resulting full snapshot. No-op if no matching account is open (REST still persists).
fn with_user(user_id: &str, f: impl FnOnce(&mut AutoCommit)) {
    USER.with(|u| {
        let mut slot = u.borrow_mut();
        let Some(state) = slot.as_mut() else { return };
        if state.user_id != user_id {
            return;
        }
        f(&mut state.doc);
        let bytes = state.doc.save();
        state.persister.persist(bytes);
    });
    // Local change → push it soon (debounced; no-op unless sync is enabled).
    crate::sync::nudge(user_id, false);
}

// ── Public entry point: seed-from-REST-or-load-local, then project ───────────

/// Enter the dashboard for `user_id`: back the book list with a local `user:` doc
/// and project that doc into [`AppStore::books`]. Seeds from the REST-fetched
/// `books` when no local doc exists; otherwise loads the local doc (which then wins
/// the projection). Schedules its async work and returns immediately.
pub fn enter_user(user_id: String, books: Vec<Book>, store: AppStore) {
    let doc_id = format!("user:{user_id}");
    spawn(async move {
        let ds = match DocStore::open(&doc_id).await {
            Ok(ds) => ds,
            Err(e) => {
                log::warn!("local-first user: open {doc_id}: {e}");
                return;
            }
        };

        let doc = match ds.load().await {
            Ok(Some(persisted)) => match AutoCommit::load(&persisted.snapshot) {
                Ok(mut doc) => {
                    for delta in &persisted.deltas {
                        let _ = doc.load_incremental(delta);
                    }
                    doc
                }
                Err(e) => {
                    log::warn!("local-first user: corrupt snapshot {doc_id}: {e}; reseeding");
                    seed_doc(&ds, &books)
                }
            },
            Ok(None) => seed_doc(&ds, &books),
            Err(e) => {
                log::warn!("local-first user: load {doc_id}: {e}");
                return;
            }
        };

        let persister = Persister::new(ds);
        USER.with(|u| {
            *u.borrow_mut() = Some(UserState {
                user_id: user_id.clone(),
                doc,
                persister,
            });
        });

        // Fold in anything the server knows and this document does not — a book made
        // on another device, or before this device had a document at all. Without it
        // the local doc is authoritative about a world it can only ever have learned
        // about from itself: a second device stays frozen at whatever it last did,
        // and a book created elsewhere never appears on it again.
        //
        // Additive only. A book the doc has and the server does not is *kept*, because
        // from here the two cases are indistinguishable — deleted elsewhere, or created
        // here while offline and not yet pushed — and only one of them is safe to guess
        // wrong. A stale entry is a dead card; a dropped one is a lost book.
        fold_in_server_books(&user_id, &books);

        // Project the (now-authoritative) local doc into the render signal.
        project_books(store.clone());

        // The doc exists now, so it can be synced. No-op unless sync is enabled.
        crate::sync::register_user(&user_id, store);
    });
}

/// Add every server book the local document has never heard of.
///
/// Idempotent: a book already in the document is left exactly as it is, so this cannot
/// overwrite a title changed here but not yet pushed.
fn fold_in_server_books(user_id: &str, books: &[Book]) {
    let known: std::collections::HashSet<String> = with_user_doc(user_id, |doc| {
        get_obj(doc, &ROOT, "books")
            .map(|o| doc.keys(&o).collect())
            .unwrap_or_default()
    })
    .unwrap_or_default();

    for book in books.iter().filter(|b| !known.contains(&b.id)) {
        add_book(user_id, book);
    }
}

/// Build a fresh `user:` doc from the REST book list and publish its first snapshot.
fn seed_doc(ds: &DocStore, books: &[Book]) -> AutoCommit {
    let mut doc = AutoCommit::new();
    build_doc(&mut doc, books);
    let persister = Persister::new(ds.clone());
    persister.persist(doc.save());
    doc
}

fn build_doc(doc: &mut AutoCommit, books: &[Book]) {
    let books_obj = doc.put_object(ROOT, "books", ObjType::Map).unwrap();
    for b in books {
        write_entry(doc, &books_obj, b);
    }
}

/// Upsert one book's cached entry (title / cover_ref / updated_at).
fn write_entry(doc: &mut AutoCommit, books_obj: &ObjId, book: &Book) {
    let entry = ensure_obj(doc, books_obj, book.id.as_str(), ObjType::Map);
    let _ = doc.put(&entry, "title", book.title.as_str());
    match &book.cover_image {
        Some(c) => {
            let _ = doc.put(&entry, "cover_ref", c.as_str());
        }
        None => {
            let _ = doc.delete(&entry, "cover_ref");
        }
    }
    let _ = doc.put(&entry, "updated_at", book.updated_at.as_str());
}

// ── Mutations (dual-write; called beside the existing REST calls) ────────────

/// Book **create**: add the new book's cached entry (from the REST-returned `Book`,
/// whose `updated_at` is the server's). Called beside the dashboard's create POST.
pub fn add_book(user_id: &str, book: &Book) {
    with_user(user_id, |doc| {
        let books_obj = ensure_obj(doc, &ROOT, "books", ObjType::Map);
        write_entry(doc, &books_obj, book);
    });
}

/// Book **delete**: remove the cached entry. Called beside the dashboard's delete.
pub fn remove_book(user_id: &str, book_id: &str) {
    with_user(user_id, |doc| {
        let books_obj = ensure_obj(doc, &ROOT, "books", ObjType::Map);
        let _ = doc.delete(&books_obj, book_id);
    });
}

/// Book **rename / cover change**: update the cached `title` and `cover_ref` of an
/// existing entry, preserving its `updated_at`. Called beside the book-settings PUT.
///
/// `updated_at` is intentionally left untouched: the rename PUT returns no body, so
/// the client has no fresh server timestamp and does not fabricate one (no
/// `Date::now()` — it panics off-wasm). The dashboard cards key on title/cover; the
/// stable `updated_at` only affects shelf ordering, which a later seed re-syncs.
pub fn update_book(user_id: &str, book_id: &str, title: &str, cover_ref: Option<&str>) {
    with_user(user_id, |doc| {
        let books_obj = ensure_obj(doc, &ROOT, "books", ObjType::Map);
        let entry = ensure_obj(doc, &books_obj, book_id, ObjType::Map);
        let _ = doc.put(&entry, "title", title);
        match cover_ref {
            Some(c) => {
                let _ = doc.put(&entry, "cover_ref", c);
            }
            None => {
                let _ = doc.delete(&entry, "cover_ref");
            }
        }
    });
}

// ── Projection (doc → AppStore::books; local doc wins) ───────────────────────

/// Project the open user's doc into [`AppStore::books`]: order the doc's cached
/// entries by `updated_at` descending (newest first, tie-break by id for a stable
/// order) and, for each, overlay the cached title / cover onto the matching full
/// [`Book`] from the REST seed (`store.books`). A book present in the doc but not in
/// the REST list (offline / server divergence) is rendered from its cached fields
/// alone. The doc is the authoritative set — REST-only books are not appended, so
/// the local list wins on divergence.
pub fn project_books(store: AppStore) {
    USER.with(|u| {
        let slot = u.borrow();
        let Some(state) = slot.as_ref() else { return };
        let doc = &state.doc;

        let Some(books_obj) = get_obj(doc, &ROOT, "books") else { return };
        let mut entries = read_entries(doc, &books_obj);
        // Newest-first; id tie-break keeps equal timestamps deterministic.
        entries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(a.id.cmp(&b.id)));

        let rest = store.books.get();
        let mut by_id: HashMap<String, Book> =
            rest.iter().map(|b| (b.id.clone(), b.clone())).collect();

        let mut out = Vec::with_capacity(entries.len());
        for e in entries {
            if let Some(mut b) = by_id.remove(&e.id) {
                b.title = e.title;
                b.cover_image = e.cover_ref;
                b.updated_at = e.updated_at;
                out.push(b);
            } else {
                // Cached-only (offline): render from the subset the doc holds.
                out.push(Book {
                    id: e.id,
                    title: e.title,
                    description: String::new(),
                    created_at: e.updated_at.clone(),
                    updated_at: e.updated_at,
                    chapter_count: None,
                    word_count: None,
                    font_settings: None,
                    cover_image: e.cover_ref,
                });
            }
        }
        // Server books the document has not caught up with. `project_chapters` has
        // always done this for chapters; the omission here is why a book created on
        // another device never appeared on one that had loaded before.
        for book in by_id.into_values() {
            out.push(book);
        }
        out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(a.id.cmp(&b.id)));
        store.books.set(out);
    });
}

/// A cached book entry read out of the doc.
struct Entry {
    id: String,
    title: String,
    cover_ref: Option<String>,
    updated_at: String,
}

fn read_entries(doc: &AutoCommit, books_obj: &ObjId) -> Vec<Entry> {
    let mut out = Vec::new();
    for id in doc.keys(books_obj) {
        let Some(entry) = get_obj(doc, books_obj, id.as_str()) else { continue };
        out.push(Entry {
            title: get_str(doc, &entry, "title").unwrap_or_default(),
            cover_ref: get_str(doc, &entry, "cover_ref"),
            updated_at: get_str(doc, &entry, "updated_at").unwrap_or_default(),
            id,
        });
    }
    out
}

// ── Automerge helpers (mirror of local_book's) ───────────────────────────────

/// Resolve the object id at `parent[prop]` if it is an object.
fn get_obj(doc: &AutoCommit, parent: &ObjId, prop: &str) -> Option<ObjId> {
    match doc.get(parent, prop) {
        Ok(Some((v, id))) if v.is_object() => Some(id),
        _ => None,
    }
}

/// Object id at `parent[prop]`, creating it as `ty` if absent.
fn ensure_obj(doc: &mut AutoCommit, parent: &ObjId, prop: &str, ty: ObjType) -> ObjId {
    if let Some(id) = get_obj(doc, parent, prop) {
        return id;
    }
    doc.put_object(parent, prop, ty).unwrap()
}

/// Read a scalar string value at `parent[prop]`.
fn get_str(doc: &AutoCommit, parent: &ObjId, prop: &str) -> Option<String> {
    doc.get(parent, prop)
        .ok()
        .flatten()
        .and_then(|(v, _)| v.to_str().map(|s| s.to_string()))
}

// ── Durability + projection proof (native) ───────────────────────────────────

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::local_store::DocStore;
    use rinch_storage::{FsStore, Store};
    use std::future::Future;
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

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

    fn book(id: &str, title: &str, updated_at: &str, cover: Option<&str>) -> Book {
        Book {
            id: id.into(),
            title: title.into(),
            description: "desc".into(),
            created_at: "2026-01-01 00:00:00".into(),
            updated_at: updated_at.into(),
            chapter_count: Some(1),
            word_count: Some(0),
            font_settings: None,
            cover_image: cover.map(|c| c.to_string()),
        }
    }

    /// Read the doc's book entries back as a sorted `(id, title, cover, updated_at)`
    /// list — mirrors `project_books`' ordering without an AppStore.
    fn read_sorted(doc: &AutoCommit) -> Vec<(String, String, Option<String>, String)> {
        let books_obj = get_obj(doc, &ROOT, "books").unwrap();
        let mut entries = read_entries(doc, &books_obj);
        entries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(a.id.cmp(&b.id)));
        entries
            .into_iter()
            .map(|e| (e.id, e.title, e.cover_ref, e.updated_at))
            .collect()
    }

    /// The acceptance proof: build a `user:` doc (two books), mutate it (add a third
    /// book, rename the first), persist through [`DocStore`] onto an `FsStore`, drop
    /// everything, reopen from a *fresh* store over the same dir, and assert the
    /// projected book list (titles + `updated_at` order) equals the mutated original.
    #[test]
    fn user_doc_survives_persist_drop_reload() {
        let books = vec![
            book("b1", "Moon Over Water", "2026-02-01 10:00:00", Some("cover-a")),
            book("b2", "The Long Road", "2026-03-15 09:00:00", None),
        ];

        let dir = tempfile::tempdir().expect("tempdir");

        // ── Session 1: seed, mutate, persist ──
        {
            let store: Rc<dyn Store> = Rc::new(FsStore::open(dir.path()).unwrap());
            let ds = DocStore::with_backend(store, "user:proof");

            let mut doc = AutoCommit::new();
            build_doc(&mut doc, &books);

            // Add a third book (later updated_at → should sort to the top).
            let books_obj = ensure_obj(&mut doc, &ROOT, "books", ObjType::Map);
            write_entry(
                &mut doc,
                &books_obj,
                &book("b3", "Winter Harbour", "2026-05-20 12:00:00", None),
            );

            // Rename b1 (title Map put; updated_at preserved).
            let entry = ensure_obj(&mut doc, &books_obj, "b1", ObjType::Map);
            doc.put(&entry, "title", "Moon Over Deep Water").unwrap();

            block_on(ds.publish_snapshot(&doc.save())).unwrap();
        } // drop store + doc entirely — simulate app exit

        // ── Session 2: fresh store over the same dir, reconstruct, compare ──
        let reopened: Rc<dyn Store> = Rc::new(FsStore::open(dir.path()).unwrap());
        let ds = DocStore::with_backend(reopened, "user:proof");
        let persisted = block_on(ds.load()).unwrap().expect("a persisted user doc");
        let mut doc = AutoCommit::load(&persisted.snapshot).unwrap();
        for d in &persisted.deltas {
            doc.load_incremental(d).unwrap();
        }

        // Newest-first order (b3, b2, b1) with the rename applied and covers kept.
        assert_eq!(
            read_sorted(&doc),
            vec![
                ("b3".to_string(), "Winter Harbour".to_string(), None, "2026-05-20 12:00:00".to_string()),
                ("b2".to_string(), "The Long Road".to_string(), None, "2026-03-15 09:00:00".to_string()),
                (
                    "b1".to_string(),
                    "Moon Over Deep Water".to_string(),
                    Some("cover-a".to_string()),
                    "2026-02-01 10:00:00".to_string(),
                ),
            ],
            "book list must round-trip newest-first with the add + rename applied"
        );

        // A delete removes the entry cleanly.
        let books_obj = get_obj(&doc, &ROOT, "books").unwrap();
        doc.delete(&books_obj, "b2").unwrap();
        let ids: Vec<String> = read_sorted(&doc).into_iter().map(|(id, ..)| id).collect();
        assert_eq!(ids, vec!["b3".to_string(), "b1".to_string()], "delete removes the entry");
    }
}
