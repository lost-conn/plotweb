//! Local-first **book structure** (Phase 2 · Slice 1 · deliverable 2).
//!
//! Where [`crate::local_store`] makes a chapter/note *body* (a `rinch-editor-collab`
//! CRDT) durable on the client, this module makes a book's **structure & metadata**
//! durable: chapter order + titles, the notes tree (order / nesting / collapse) +
//! note titles + colors, and book meta. Its persistence unit is a **hand-projected
//! Automerge document** `book:{book_id}` — a plain `automerge::AutoCommit` we build
//! and read directly (NOT the editor body CRDT), persisted through
//! [`crate::local_store::DocStore`] exactly like a chapter body.
//!
//! # Schema (locked v1, docs/offline-first-rinch-plan.md §2)
//!
//! ```text
//! ROOT
//!   meta:           Map { title, description, font_settings (JSON str), cover_ref?, created_at }
//!   chapters:       List<chapter_id>            // AUTHORITATIVE order
//!   chapter_titles: Map<chapter_id, String>
//!   notes:          Map {
//!     root_order: List<note_id>,
//!     children:   Map<note_id, List<note_id>>,  // parent → ordered child ids (only non-empty parents)
//!     collapsed:  Map<note_id, bool>,
//!     titles:     Map<note_id, String>,
//!     colors:     Map<note_id, String>,
//!   }
//! ```
//!
//! Order (a `List`) is deliberately decoupled from titles/colors/collapse (`Map`s)
//! so a later sync slice can merge a concurrent reorder and a rename cleanly. The
//! inline chapter/note **title field** is a plain text control bound to the title
//! `Map` — it is not part of any rich-text CRDT.
//!
//! # Read vs mutate (dual-write)
//!
//! - **Read:** [`enter`] seeds the doc from REST (first open) or loads the local doc
//!   (subsequent opens), then *projects* it back into the existing [`AppStore`]
//!   signals — so the sidebar chapter list, titles, and notes tree render from the
//!   local doc. On divergence, the **local doc wins** the projection.
//! - **Mutate:** every structural edit ([`sync_chapters`], [`sync_notes`],
//!   [`note_meta`]) applies to the local doc immediately and persists it, **beside**
//!   the untouched REST calls in `book.rs` (dual-write). The `AppStore` signals stay
//!   the render source; they are now fed from the doc (falling back to the REST seed).
//!
//! Everything here is `!Send` and stays on the main thread, scheduled through
//! [`crate::local_store::spawn`]; persistence is a full-snapshot re-publish per edit
//! (the doc is small), coalesced + serialized so rapid edits never race the
//! generation pointer.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use automerge::transaction::Transactable;
use automerge::{AutoCommit, ObjId, ObjType, ROOT, ReadDoc};

use plotweb_common::{Book, Chapter, Note, NoteTree};

use crate::local_store::{DocStore, spawn};
use crate::store::AppStore;

// ── Sequential, coalescing snapshot persister ────────────────────────────────

/// Persists the book doc's full snapshot through [`DocStore`], serialized so two
/// rapid edits can't race [`DocStore::publish_snapshot`]'s generation pointer.
///
/// Each edit stashes the *latest* bytes and, if no publish is in flight, kicks a
/// drain loop that publishes the newest pending snapshot until none remain. Newer
/// edits during a publish just replace the pending bytes (coalescing), so the last
/// write always reflects the final state — never an older snapshot landing last.
///
/// The publish itself is a real IndexedDB round trip (on web), so it does not land
/// in the same tick as the edit that queued it. A caller that flips something
/// observable — a DOM-visible `AppStore` signal — right after queuing a persist is
/// racing an immediate page reload: the reload's [`enter`] loads whatever
/// generation is durable *at that instant*, and because the local doc wins the
/// projection, a write that lost the race is not just delayed but silently
/// reverted (see [`on_settled`]). [`Persister::when_settled`] is how a caller
/// closes that window: it defers running something until the queue this edit just
/// joined has actually drained.
#[derive(Clone)]
struct Persister {
    store: DocStore,
    pending: Rc<RefCell<Option<Vec<u8>>>>,
    busy: Rc<Cell<bool>>,
    /// Callbacks waiting for the drain loop to empty `pending` and go idle.
    waiters: Rc<RefCell<Vec<Box<dyn FnOnce()>>>>,
}

impl Persister {
    fn new(store: DocStore) -> Self {
        Self {
            store,
            pending: Rc::new(RefCell::new(None)),
            busy: Rc::new(Cell::new(false)),
            waiters: Rc::new(RefCell::new(Vec::new())),
        }
    }

    fn persist(&self, bytes: Vec<u8>) {
        *self.pending.borrow_mut() = Some(bytes);
        self.drain();
    }

    /// Run `f` once every publish queued up to this call has been attempted (landed
    /// or given up after an error) and the drain loop has gone idle.
    fn when_settled(&self, f: impl FnOnce() + 'static) {
        self.waiters.borrow_mut().push(Box::new(f));
        self.drain();
    }

    /// Ensure a drain loop is running; a no-op if one already is (it will pick up
    /// whatever `persist`/`when_settled` just queued on its next iteration).
    fn drain(&self) {
        if self.busy.get() {
            return;
        }
        self.busy.set(true);
        let store = self.store.clone();
        let pending = self.pending.clone();
        let busy = self.busy.clone();
        let waiters = self.waiters.clone();
        spawn(async move {
            loop {
                let next = pending.borrow_mut().take();
                match next {
                    Some(b) => {
                        if let Err(e) = store.publish_snapshot(&b).await {
                            log::warn!("local-first book: publish failed: {e}");
                        }
                    }
                    None => break,
                }
            }
            busy.set(false);
            for f in waiters.borrow_mut().drain(..) {
                f();
            }
        });
    }
}

// ── Open book state (one at a time) ──────────────────────────────────────────

struct BookState {
    book_id: String,
    doc: AutoCommit,
    persister: Persister,
    /// The chapter list as **REST** last gave it — the seed on entry, plus the local
    /// creates, imports and deletions since.
    ///
    /// The projection reads this and never writes it. It used to read `store.chapters`,
    /// which is its own output: a chapter the document had dropped was re-added from the
    /// previous projection's list, so a deletion made on another device never landed
    /// (§D9 — a whole-state write whose source had moved on, here a signal rather than a
    /// CRDT).
    rest: Vec<Chapter>,
}

thread_local! {
    /// The currently-open book's structure doc. One book is open at a time (the
    /// book page is a single route); [`enter`] replaces it. A stale mutation for a
    /// different `book_id` is ignored, so a leaked timer from a previous page can't
    /// corrupt the newly-open book's doc.
    static BOOK: RefCell<Option<BookState>> = const { RefCell::new(None) };
}

// ── Sync seams ───────────────────────────────────────────────────────────────
// Mirror of `local_user`'s: generating a sync message must not persist; integrating a
// peer's message must. See [`crate::sync`].

/// The book whose `book:` doc is open, if any.
pub(crate) fn open_book_id() -> Option<String> {
    BOOK.with(|b| b.borrow().as_ref().map(|s| s.book_id.clone()))
}

/// Run `f` against the open book's CRDT **without** persisting.
pub(crate) fn with_book_doc<R>(book_id: &str, f: impl FnOnce(&mut AutoCommit) -> R) -> Option<R> {
    BOOK.with(|b| {
        let mut slot = b.borrow_mut();
        let state = slot.as_mut()?;
        (state.book_id == book_id).then(|| f(&mut state.doc))
    })
}

/// Persist the open book's doc as it now stands (after a sync merge).
pub(crate) fn persist_book(book_id: &str) {
    BOOK.with(|b| {
        let mut slot = b.borrow_mut();
        let Some(state) = slot.as_mut() else { return };
        if state.book_id != book_id {
            return;
        }
        let bytes = state.doc.save();
        state.persister.persist(bytes);
    });
}

/// Replace the open book's document with the server's canonical copy.
///
/// The way out of §D8 for a structure document: ours was seeded independently (built
/// from REST data rather than pulled), so it descends from nothing the server holds and
/// the two would merge by concatenation. The server sees that from the documents
/// themselves and refuses; this is what we do about it.
///
/// "What we hold is a second copy of the same book, not unsynced work" was true while
/// every structural change also dual-wrote to git. Under cutover it is not: a chapter
/// created or reordered on this device may exist nowhere else. So this device's copy
/// is kept before the replacement ([`crate::local_store::preserve_local_bytes`]).
///
/// `false` if a different book (or none) is open, in which case the caller's cycle is
/// moot rather than failed.
pub(crate) fn install_server_book(book_id: &str, bytes: &[u8]) -> bool {
    let Ok(doc) = AutoCommit::load(bytes) else {
        log::warn!("sync book:{book_id}: canonical copy did not load");
        return false;
    };
    BOOK.with(|b| {
        let mut slot = b.borrow_mut();
        let Some(state) = slot.as_mut() else {
            return false;
        };
        if state.book_id != book_id {
            return false;
        }
        // Captured before the replacement, so the write that follows cannot race it.
        let ours = state.doc.save();
        let doc_id = format!("book:{book_id}");
        crate::local_store::spawn(async move {
            if let Err(e) = crate::local_store::preserve_local_bytes(&doc_id, &ours).await {
                log::warn!("local-first: {doc_id}: could not keep this device's copy: {e}");
            }
        });
        state.doc = doc;
        state.persister.persist(state.doc.save());
        true
    })
}

/// Record a typography change in the book document.
///
/// Font settings used to reach only REST: the `book:` document took them once, when it
/// was seeded, and never again. So the two copies parted company the moment an author
/// touched the typography panel — which is exactly what the phase-D shadow pass
/// reported on two production books, and would have ridden into cutover as the
/// structure document became authoritative.
pub fn set_font_settings(book_id: &str, font_settings: &plotweb_common::FontSettings) {
    let json = serde_json::to_string(font_settings).unwrap_or_else(|_| "{}".to_string());
    with_book(book_id, |doc| {
        let Some(meta) = get_obj(doc, &ROOT, "meta") else {
            return;
        };
        let _ = doc.put(&meta, "font_settings", json);
    });
}

/// Edit the REST chapter snapshot the projection reads.
///
/// Every local change to *which chapters exist* — create, import, delete — belongs here
/// beside its REST call, for the same reason the server's `apply_book_structure` takes a
/// `removable` list: absence is not evidence of deletion, so the caller that knows names
/// what it did. Renames and reorders don't: title and order come from the document.
///
/// No-op unless that book is open.
pub fn rest_chapters(book_id: &str, f: impl FnOnce(&mut Vec<Chapter>)) {
    BOOK.with(|b| {
        let mut slot = b.borrow_mut();
        let Some(state) = slot.as_mut() else { return };
        if state.book_id != book_id {
            return;
        }
        f(&mut state.rest);
    });
}

/// Run `f` against the open book's doc iff it matches `book_id`, then persist the
/// resulting full snapshot. No-op if no matching book is open (REST still persists).
fn with_book(book_id: &str, f: impl FnOnce(&mut AutoCommit)) {
    BOOK.with(|b| {
        let mut slot = b.borrow_mut();
        let Some(state) = slot.as_mut() else { return };
        if state.book_id != book_id {
            return;
        }
        f(&mut state.doc);
        let bytes = state.doc.save();
        state.persister.persist(bytes);
    });
    // Local change → push it soon (debounced; no-op unless sync is enabled).
    crate::sync::nudge(book_id, true);
}

/// Run `f` once the open book's most recently queued local-doc write has settled,
/// or immediately if no matching book is open to wait on.
///
/// The reload race this closes: `sync_chapters` / `sync_notes` / `note_meta` queue
/// their publish and return before it lands (it's a real IndexedDB round trip on
/// web), so a caller that flips a DOM-visible `AppStore` signal right after them —
/// the optimistic swap — is racing an immediate page reload. Lose that race and the
/// reload's [`enter`] loads the *previous* generation; because the local doc wins
/// the projection (by design, so a genuine offline edit survives a stale REST
/// fetch), the edit is not merely delayed but silently reverted, even though the
/// REST call for the same edit reliably lands first. Routing the visible flip
/// through here instead of firing it inline means nothing observable happens until
/// the edit is durable on this device, so nothing can trigger a reload inside the
/// window where it could still be lost.
pub fn on_settled(book_id: &str, f: impl FnOnce() + 'static) {
    let persister = BOOK.with(|b| {
        b.borrow()
            .as_ref()
            .filter(|s| s.book_id == book_id)
            .map(|s| s.persister.clone())
    });
    match persister {
        Some(p) => p.when_settled(f),
        None => f(),
    }
}

// ── Public entry point: seed-from-REST-or-load-local, then project ───────────

/// Enter `book_id`: back its structure with a local `book:` doc and project that
/// doc into the [`AppStore`] signals. Seeds from the REST-fetched `book` / `chapters`
/// / `notes` / `tree` when no local doc exists; otherwise loads the local doc (which
/// then wins the projection). Schedules its async work and returns immediately.
pub fn enter(
    book_id: String,
    book: Book,
    chapters: Vec<Chapter>,
    notes: Vec<Note>,
    tree: NoteTree,
    store: AppStore,
) {
    let doc_id = format!("book:{book_id}");
    // Before anything is registered: whether this book is cut over is what decides
    // whether sync carries it, and this REST payload is the server's answer.
    crate::sync::note_cutover(&book_id, book.cutover, store.clone());
    spawn(async move {
        let ds = match DocStore::open(&doc_id).await {
            Ok(ds) => ds,
            Err(e) => {
                log::warn!("local-first book: open {doc_id}: {e}");
                return;
            }
        };

        let doc = match ds.load().await {
            Ok(Some(persisted)) => {
                // Existing local doc: adopt it (snapshot + any folded deltas). The
                // local structure now wins the projection below.
                match AutoCommit::load(&persisted.snapshot) {
                    Ok(mut doc) => {
                        for delta in &persisted.deltas {
                            let _ = doc.load_incremental(delta);
                        }
                        doc
                    }
                    Err(e) => {
                        log::warn!("local-first book: corrupt snapshot {doc_id}: {e}; reseeding");
                        seed_doc(&ds, &book, &chapters, &notes, &tree)
                    }
                }
            }
            Ok(None) => seed_doc(&ds, &book, &chapters, &notes, &tree),
            Err(e) => {
                log::warn!("local-first book: load {doc_id}: {e}");
                return;
            }
        };

        let persister = Persister::new(ds);
        BOOK.with(|b| {
            *b.borrow_mut() = Some(BookState {
                book_id: book_id.clone(),
                doc,
                persister,
                rest: chapters,
            });
        });

        // Project the (now-authoritative) local doc into the render signals.
        project(store.clone());

        // The doc exists now, so it can be synced. No-op unless this book syncs here.
        crate::sync::register_book(&book_id, store);
    });
}

/// Build a fresh `book:` doc from REST data and publish its first snapshot.
fn seed_doc(
    ds: &DocStore,
    book: &Book,
    chapters: &[Chapter],
    notes: &[Note],
    tree: &NoteTree,
) -> AutoCommit {
    let mut doc = AutoCommit::new();
    build_doc(&mut doc, book, chapters, notes, tree);
    let persister = Persister::new(ds.clone());
    persister.persist(doc.save());
    doc
}

// ── Doc construction ─────────────────────────────────────────────────────────

fn build_doc(doc: &mut AutoCommit, book: &Book, chapters: &[Chapter], notes: &[Note], tree: &NoteTree) {
    // meta
    let meta = doc.put_object(ROOT, "meta", ObjType::Map).unwrap();
    let _ = doc.put(&meta, "title", book.title.as_str());
    let _ = doc.put(&meta, "description", book.description.as_str());
    let fs_json = serde_json::to_string(&book.font_settings.clone().unwrap_or_default())
        .unwrap_or_else(|_| "{}".to_string());
    let _ = doc.put(&meta, "font_settings", fs_json);
    if let Some(cover) = &book.cover_image {
        let _ = doc.put(&meta, "cover_ref", cover.as_str());
    }
    let _ = doc.put(&meta, "created_at", book.created_at.as_str());

    // chapters (order List + titles Map)
    let chs = doc.put_object(ROOT, "chapters", ObjType::List).unwrap();
    let ctitles = doc.put_object(ROOT, "chapter_titles", ObjType::Map).unwrap();
    for (i, c) in chapters.iter().enumerate() {
        let _ = doc.insert(&chs, i, c.id.as_str());
        let _ = doc.put(&ctitles, c.id.as_str(), c.title.as_str());
    }

    // notes
    let notes_obj = doc.put_object(ROOT, "notes", ObjType::Map).unwrap();
    write_notes(doc, &notes_obj, notes, tree);
}

/// Full (lossless) replace of the `notes` sub-object's structure + meta from a
/// `NoteTree` (order/nesting/collapse) and the note list (titles/colors).
fn write_notes(doc: &mut AutoCommit, notes_obj: &ObjId, notes: &[Note], tree: &NoteTree) {
    // root_order List
    let root = ensure_obj(doc, notes_obj, "root_order", ObjType::List);
    set_list(doc, &root, &tree.root_order);

    // children Map<note_id, List> — only parents that actually have children
    let children = ensure_obj(doc, notes_obj, "children", ObjType::Map);
    let parents: Vec<String> = tree
        .children
        .iter()
        .filter(|(_, kids)| !kids.is_empty())
        .map(|(p, _)| p.clone())
        .collect();
    retain_keys(doc, &children, &parents);
    for (parent, kids) in &tree.children {
        if kids.is_empty() {
            continue;
        }
        let list = ensure_obj(doc, &children, parent.as_str(), ObjType::List);
        set_list(doc, &list, kids);
    }

    // collapsed Map<note_id, bool>
    let collapsed = ensure_obj(doc, notes_obj, "collapsed", ObjType::Map);
    retain_keys(doc, &collapsed, &tree.collapsed);
    for id in &tree.collapsed {
        if !doc
            .get(&collapsed, id.as_str())
            .ok()
            .flatten()
            .and_then(|(v, _)| v.to_bool())
            .unwrap_or(false)
        {
            let _ = doc.put(&collapsed, id.as_str(), true);
        }
    }

    // titles + colors Maps
    let ids: Vec<String> = notes.iter().map(|n| n.id.clone()).collect();
    let coloured: Vec<String> = notes
        .iter()
        .filter(|n| n.color.is_some())
        .map(|n| n.id.clone())
        .collect();
    let titles = ensure_obj(doc, notes_obj, "titles", ObjType::Map);
    let colors = ensure_obj(doc, notes_obj, "colors", ObjType::Map);
    retain_keys(doc, &titles, &ids);
    retain_keys(doc, &colors, &coloured);
    for n in notes {
        put_if_changed(doc, &titles, n.id.as_str(), n.title.as_str());
        if let Some(c) = &n.color {
            put_if_changed(doc, &colors, n.id.as_str(), c.as_str());
        }
    }

    write_note_facets(doc, notes_obj, notes);
}

/// The notes revamp's facet maps: span, relative constraint, entity mark, containment
/// in time, and the link index derived from each body.
///
/// They live beside `titles` / `colors` in the `book:` document rather than in each
/// `note:{id}` body document, because the timeline is drawn from them — and opening
/// every note body to draw one screen is the thing this arrangement exists to avoid.
///
/// Spans, constraints and link indices are stored as whole-value JSON, the way
/// `font_settings` is: half a span is not a span, and a per-field merge of two retyped
/// dates would produce a third date neither author typed.
fn write_note_facets(doc: &mut AutoCommit, notes_obj: &ObjId, notes: &[Note]) {
    let spans = ensure_obj(doc, notes_obj, "spans", ObjType::Map);
    let relatives = ensure_obj(doc, notes_obj, "relatives", ObjType::Map);
    let entities = ensure_obj(doc, notes_obj, "entities", ObjType::Map);
    let event_parents = ensure_obj(doc, notes_obj, "event_parents", ObjType::Map);
    let links = ensure_obj(doc, notes_obj, "links", ObjType::Map);

    let with = |f: &dyn Fn(&Note) -> bool| -> Vec<String> {
        notes.iter().filter(|n| f(n)).map(|n| n.id.clone()).collect()
    };
    retain_keys(doc, &spans, &with(&|n| n.span.is_some()));
    retain_keys(doc, &relatives, &with(&|n| n.relative.is_some()));
    retain_keys(doc, &entities, &with(&|n| n.is_entity));
    retain_keys(doc, &event_parents, &with(&|n| n.event_parent.is_some()));
    // The link index now rides on the note itself (`Note::links`, derived server-side
    // from the body the server holds), so a full note list can seed it — without which
    // a device that has only ever fetched would project an empty index over a perfectly
    // good one and show a rail with nothing in it.
    //
    // Only the keys of notes that no longer exist are dropped. An entry is **not**
    // removed just because the incoming note carries no edges: on a cut-over book the
    // server derives its copy from git, which lags the canonical body by the mirror's
    // debounce, so an empty answer there is as likely to mean "not seen yet" as "no
    // edges". Emptying is the job of `note_links`, which runs beside the save that
    // emptied it, holding the body that did.
    retain_keys(doc, &links, &notes.iter().map(|n| n.id.clone()).collect::<Vec<_>>());

    for n in notes {
        let id = n.id.as_str();
        if let Some(json) = n.span.as_ref().and_then(|s| serde_json::to_string(s).ok()) {
            put_if_changed(doc, &spans, id, &json);
        }
        if let Some(json) = n
            .relative
            .as_ref()
            .and_then(|r| serde_json::to_string(r).ok())
        {
            put_if_changed(doc, &relatives, id, &json);
        }
        if n.is_entity
            && !doc
                .get(&entities, id)
                .ok()
                .flatten()
                .and_then(|(v, _)| v.to_bool())
                .unwrap_or(false)
        {
            let _ = doc.put(&entities, id, true);
        }
        if let Some(parent) = &n.event_parent {
            put_if_changed(doc, &event_parents, id, parent.as_str());
        }
        if !n.links.is_empty()
            && let Ok(json) = serde_json::to_string(&n.links)
        {
            put_if_changed(doc, &links, id, &json);
        }
    }
}

// ── Mutations (dual-write; called beside the existing REST PUTs) ─────────────

/// Sync chapter **order** (the `chapters` List) and **titles** (the `chapter_titles`
/// Map) from the current chapter list. Covers create / delete / reorder / rename.
pub fn sync_chapters(book_id: &str, chapters: &[Chapter]) {
    let ids: Vec<String> = chapters.iter().map(|c| c.id.clone()).collect();
    with_book(book_id, |doc| {
        let chs = ensure_obj(doc, &ROOT, "chapters", ObjType::List);
        set_list(doc, &chs, &ids);
        let titles = ensure_obj(doc, &ROOT, "chapter_titles", ObjType::Map);
        retain_keys(doc, &titles, &ids);
        for c in chapters {
            put_if_changed(doc, &titles, c.id.as_str(), c.title.as_str());
        }
    });
}

/// Full lossless sync of the notes sub-object (structure + titles + colors +
/// collapse). Covers create / delete / move / nest / reorder / collapse whenever a
/// full note list + tree is on hand.
pub fn sync_notes(book_id: &str, notes: &[Note], tree: &NoteTree) {
    with_book(book_id, |doc| {
        let notes_obj = ensure_obj(doc, &ROOT, "notes", ObjType::Map);
        write_notes(doc, &notes_obj, notes, tree);
    });
}

/// Targeted rename / recolor: put into the note `titles` / `colors` Maps without
/// touching structure. Used by the note-editor save path (which has no fresh full
/// note list).
pub fn note_meta(book_id: &str, note_id: &str, title: Option<&str>, color: Option<&str>) {
    with_book(book_id, |doc| {
        let notes_obj = ensure_obj(doc, &ROOT, "notes", ObjType::Map);
        if let Some(t) = title {
            let titles = ensure_obj(doc, &notes_obj, "titles", ObjType::Map);
            let _ = doc.put(&titles, note_id, t);
        }
        if let Some(c) = color {
            let colors = ensure_obj(doc, &notes_obj, "colors", ObjType::Map);
            let _ = doc.put(&colors, note_id, c);
        }
    });
}

/// Refresh one note's link index from the body just saved.
///
/// Called beside the REST save rather than derived on read, and derived from the body
/// in hand rather than from the note list, because this device is the only one that has
/// the new text yet: on a cut-over book the server's copy arrives through sync, and
/// until it does a server-side derivation would rebuild the index from the previous
/// draft.
///
/// An empty index is written as a **removal** — a note whose last `$ref` was deleted has
/// no edges, and leaving the old entry would keep a character in a scene they were
/// written out of.
pub fn note_links(book_id: &str, note_id: &str, content: &str) {
    let links = plotweb_common::extract_note_links_in(content, &link_index(book_id));
    with_book(book_id, |doc| {
        let notes_obj = ensure_obj(doc, &ROOT, "notes", ObjType::Map);
        let links_obj = ensure_obj(doc, &notes_obj, "links", ObjType::Map);
        match serde_json::to_string(&links) {
            Ok(json) if !links.is_empty() => put_if_changed(doc, &links_obj, note_id, &json),
            _ => {
                if doc.get(&links_obj, note_id).ok().flatten().is_some() {
                    let _ = doc.delete(&links_obj, note_id);
                }
            }
        }
    });
}

/// The titles a `@` or `$` in this book can name, read out of the open document.
///
/// Both note titles and chapter titles, because `@` reaches a chapter as well as a note
/// and the edge records which it found. The document is the right source rather than
/// `AppStore`: it is what this device and its peers have actually done, and a rename
/// that has not round-tripped through REST yet is still in here.
pub fn link_index(book_id: &str) -> plotweb_common::LinkIndex {
    let mut index = plotweb_common::LinkIndex::new();
    BOOK.with(|b| {
        let slot = b.borrow();
        let Some(state) = slot.as_ref().filter(|s| s.book_id == book_id) else {
            return;
        };
        let doc = &state.doc;
        let note_titles = get_obj(doc, &ROOT, "notes")
            .and_then(|notes| get_obj(doc, &notes, "titles"))
            .map(|o| read_map_strings(doc, &o))
            .unwrap_or_default();
        let chapter_titles = get_obj(doc, &ROOT, "chapter_titles")
            .map(|o| read_map_strings(doc, &o))
            .unwrap_or_default();
        index = plotweb_common::LinkIndex::new()
            .with_notes(note_titles.iter())
            .with_chapters(chapter_titles.iter());
    });
    index
}

/// Targeted facet write for one note: span, relative constraint, entity mark and
/// containment in time, as a patch. `None` leaves the stored value alone; `Some(None)`
/// clears it.
///
/// `event_parent` is written exactly as given and is never derived from spans
/// overlapping — see `plotweb_common::Note::event_parent`. Setting it does not touch
/// the note's place in `root_order` / `children`, and moving it in the tree does not
/// touch this.
pub fn note_facets(
    book_id: &str,
    note_id: &str,
    span: Option<Option<plotweb_common::TimeSpan>>,
    relative: Option<Option<plotweb_common::RelativeTime>>,
    is_entity: Option<bool>,
    event_parent: Option<Option<String>>,
) {
    with_book(book_id, |doc| {
        let notes_obj = ensure_obj(doc, &ROOT, "notes", ObjType::Map);

        let mut put_json = |prop: &str, json: Option<String>| {
            let obj = ensure_obj(doc, &notes_obj, prop, ObjType::Map);
            match json {
                Some(json) => put_if_changed(doc, &obj, note_id, &json),
                None => {
                    if doc.get(&obj, note_id).ok().flatten().is_some() {
                        let _ = doc.delete(&obj, note_id);
                    }
                }
            }
        };
        if let Some(span) = span {
            put_json("spans", span.and_then(|s| serde_json::to_string(&s).ok()));
        }
        if let Some(relative) = relative {
            put_json(
                "relatives",
                relative.and_then(|r| serde_json::to_string(&r).ok()),
            );
        }
        if let Some(parent) = event_parent {
            put_json("event_parents", parent);
        }
        if let Some(is_entity) = is_entity {
            let obj = ensure_obj(doc, &notes_obj, "entities", ObjType::Map);
            if is_entity {
                let _ = doc.put(&obj, note_id, true);
            } else if doc.get(&obj, note_id).ok().flatten().is_some() {
                let _ = doc.delete(&obj, note_id);
            }
        }
    });
}

// ── Projection (doc → AppStore signals; local doc wins) ──────────────────────

/// Project the open book's doc into the chapter + note signals.
pub fn project(store: AppStore) {
    project_chapters(store);
    project_notes(store);
}

/// Reorder + retitle `store.chapters` from the doc's `chapters` List + titles Map,
/// keeping every full [`Chapter`] object (body/word-count/timestamps) from the REST
/// seed. Chapters present on the server but not yet in the doc are appended in their
/// existing order; doc ids with no REST chapter are skipped.
pub fn project_chapters(store: AppStore) {
    BOOK.with(|b| {
        let slot = b.borrow();
        let Some(state) = slot.as_ref() else { return };
        let doc = &state.doc;

        let Some(chs) = get_obj(doc, &ROOT, "chapters") else { return };
        let order = read_list_strings(doc, &chs);
        let titles = get_obj(doc, &ROOT, "chapter_titles")
            .map(|o| read_map_strings(doc, &o))
            .unwrap_or_default();

        let rest = &state.rest;
        let mut by_id: HashMap<String, Chapter> =
            rest.iter().map(|c| (c.id.clone(), c.clone())).collect();

        let mut out = Vec::with_capacity(rest.len());
        for (i, id) in order.iter().enumerate() {
            match by_id.remove(id) {
                Some(mut c) => {
                    if let Some(t) = titles.get(id) {
                        c.title = t.clone();
                    }
                    out.push(c);
                }
                // In the doc but with no REST record: a chapter created on ANOTHER
                // device and learned about through sync. The `book:` doc is the
                // authority on which chapters exist, so materialize it from what the
                // doc knows (id · title · order). Its body arrives separately — from
                // REST when this chapter is opened, or via body sync (slice 4).
                // Without this the sidebar would silently omit it.
                None => out.push(Chapter {
                    id: id.clone(),
                    book_id: state.book_id.clone(),
                    title: titles.get(id).cloned().unwrap_or_default(),
                    content: String::new(),
                    sort_order: i as i64,
                    word_count: 0,
                    created_at: String::new(),
                    updated_at: String::new(),
                }),
            }
        }
        // Server chapters not (yet) in the doc order: keep them, in REST order. A
        // chapter the document *dropped* is not among them — `rest` is what the server
        // last said exists, not what this projection last painted.
        for c in rest {
            if by_id.contains_key(&c.id) {
                out.push(c.clone());
            }
        }
        store.chapters.set(out);
    });
}

/// Project the notes tree (order/nesting/collapse) and note titles/colors from the
/// doc into `store.note_tree` / `store.notes`. Note **bodies** stay whatever the REST
/// seed put in `store.notes`; only structure + titles + colors come from the doc.
pub fn project_notes(store: AppStore) {
    BOOK.with(|b| {
        let slot = b.borrow();
        let Some(state) = slot.as_ref() else { return };
        let doc = &state.doc;

        let Some(notes_obj) = get_obj(doc, &ROOT, "notes") else { return };

        // Tree
        let root_order = get_obj(doc, &notes_obj, "root_order")
            .map(|o| read_list_strings(doc, &o))
            .unwrap_or_default();
        let mut children: HashMap<String, Vec<String>> = HashMap::new();
        if let Some(children_obj) = get_obj(doc, &notes_obj, "children") {
            for key in doc.keys(&children_obj) {
                if let Some(list) = get_obj(doc, &children_obj, key.as_str()) {
                    let kids = read_list_strings(doc, &list);
                    if !kids.is_empty() {
                        children.insert(key, kids);
                    }
                }
            }
        }
        let mut collapsed: Vec<String> = Vec::new();
        if let Some(collapsed_obj) = get_obj(doc, &notes_obj, "collapsed") {
            for key in doc.keys(&collapsed_obj) {
                if doc
                    .get(&collapsed_obj, key.as_str())
                    .ok()
                    .flatten()
                    .and_then(|(v, _)| v.to_bool())
                    .unwrap_or(false)
                {
                    collapsed.push(key);
                }
            }
        }
        store.note_tree.set(Some(NoteTree {
            root_order,
            children,
            collapsed,
        }));

        // Titles / colors overrides onto the existing note list
        let titles = get_obj(doc, &notes_obj, "titles")
            .map(|o| read_map_strings(doc, &o))
            .unwrap_or_default();
        let colors = get_obj(doc, &notes_obj, "colors")
            .map(|o| read_map_strings(doc, &o))
            .unwrap_or_default();
        // Facets, from the same document. They are structure, not body: the note list's
        // REST copy is whatever the last fetch said, and the document is what this
        // device and its peers have actually done since.
        let spans: HashMap<String, plotweb_common::TimeSpan> =
            read_json_map(doc, &notes_obj, "spans");
        let relatives: HashMap<String, plotweb_common::RelativeTime> =
            read_json_map(doc, &notes_obj, "relatives");
        let event_parents = get_obj(doc, &notes_obj, "event_parents")
            .map(|o| read_map_strings(doc, &o))
            .unwrap_or_default();
        // The link index, from the same document and for the same reason: the context
        // rail draws the graph from the note list alone, and re-deriving it here would
        // mean holding every note's body in memory.
        let links: HashMap<String, plotweb_common::NoteLinks> =
            read_json_map(doc, &notes_obj, "links");
        let mut entities: std::collections::HashSet<String> = std::collections::HashSet::new();
        if let Some(entities_obj) = get_obj(doc, &notes_obj, "entities") {
            for key in doc.keys(&entities_obj) {
                if doc
                    .get(&entities_obj, key.as_str())
                    .ok()
                    .flatten()
                    .and_then(|(v, _)| v.to_bool())
                    .unwrap_or(false)
                {
                    entities.insert(key);
                }
            }
        }

        let mut notes = store.notes.get();
        for n in notes.iter_mut() {
            if let Some(t) = titles.get(&n.id) {
                n.title = t.clone();
            }
            n.color = colors.get(&n.id).cloned();
            n.span = spans.get(&n.id).cloned();
            n.relative = relatives.get(&n.id).cloned();
            n.is_entity = entities.contains(&n.id);
            n.event_parent = event_parents.get(&n.id).cloned();
            // Unlike the facets above, an absent entry does **not** clear: the index
            // is derived, and a device that has not written one yet should show the
            // server's rather than nothing at all.
            if let Some(l) = links.get(&n.id) {
                n.links = l.clone();
            }
        }
        // Notes that exist in the doc's tree but have no REST record came from another
        // device via sync — materialize them, same reasoning as chapters above.
        let known: std::collections::HashSet<String> =
            notes.iter().map(|n| n.id.clone()).collect();
        let mut in_tree: Vec<String> = store
            .note_tree
            .get()
            .map(|t| {
                let mut ids = t.root_order.clone();
                ids.extend(t.children.values().flatten().cloned());
                ids
            })
            .unwrap_or_default();
        in_tree.retain(|id| !known.contains(id));
        in_tree.dedup();
        for id in in_tree {
            notes.push(Note {
                id: id.clone(),
                book_id: state.book_id.clone(),
                title: titles.get(&id).cloned().unwrap_or_default(),
                content: String::new(),
                color: colors.get(&id).cloned(),
                created_at: String::new(),
                updated_at: String::new(),
                span: spans.get(&id).cloned(),
                relative: relatives.get(&id).cloned(),
                is_entity: entities.contains(&id),
                event_parent: event_parents.get(&id).cloned(),
                links: links.get(&id).cloned().unwrap_or_default(),
            });
        }
        store.notes.set(notes);
    });
}

/// Read the current book meta out of the doc (used by tests / future consumers).
#[cfg(test)]
fn read_meta(doc: &AutoCommit) -> Option<(String, String, plotweb_common::FontSettings)> {
    let meta = get_obj(doc, &ROOT, "meta")?;
    let title = doc
        .get(&meta, "title")
        .ok()
        .flatten()
        .and_then(|(v, _)| v.to_str().map(|s| s.to_string()))
        .unwrap_or_default();
    let description = doc
        .get(&meta, "description")
        .ok()
        .flatten()
        .and_then(|(v, _)| v.to_str().map(|s| s.to_string()))
        .unwrap_or_default();
    let fs = doc
        .get(&meta, "font_settings")
        .ok()
        .flatten()
        .and_then(|(v, _)| v.to_str().map(|s| s.to_string()))
        .and_then(|s| serde_json::from_str::<plotweb_common::FontSettings>(&s).ok())
        .unwrap_or_default();
    Some((title, description, fs))
}

// ── Automerge helpers ────────────────────────────────────────────────────────

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

/// Every id in a `List`, with repeats collapsed — see [`dedupe`]. Every projection and
/// every comparison goes through this, so a duplicated entry can never reach the UI.
fn read_list_strings(doc: &AutoCommit, obj: &ObjId) -> Vec<String> {
    dedupe(read_list_strings_raw(doc, obj))
}

fn read_list_strings_raw(doc: &AutoCommit, obj: &ObjId) -> Vec<String> {
    let len = doc.length(obj);
    let mut out = Vec::with_capacity(len);
    for i in 0..len {
        if let Ok(Some((v, _))) = doc.get(obj, i) {
            if let Some(s) = v.to_str() {
                out.push(s.to_string());
            }
        }
    }
    out
}

fn read_map_strings(doc: &AutoCommit, obj: &ObjId) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for key in doc.keys(obj) {
        if let Ok(Some((v, _))) = doc.get(obj, key.as_str()) {
            if let Some(s) = v.to_str() {
                out.insert(key, s.to_string());
            }
        }
    }
    out
}

/// Read a whole-value JSON map (a note's span, relative constraint, link index) back
/// into typed values. A value this build cannot parse is **dropped**, not defaulted: an
/// unreadable span must not become a note dated to the epoch.
fn read_json_map<T: serde::de::DeserializeOwned>(
    doc: &AutoCommit,
    parent: &ObjId,
    prop: &str,
) -> HashMap<String, T> {
    let Some(obj) = get_obj(doc, parent, prop) else {
        return HashMap::new();
    };
    read_map_strings(doc, &obj)
        .into_iter()
        .filter_map(|(id, json)| serde_json::from_str(&json).ok().map(|v| (id, v)))
        .collect()
}

/// Overwrite a `List` with `items` (clear all, then insert in order).
/// Bring a `List` to `items` with as few edits as possible.
///
/// It used to empty the list and re-insert, which is a change touching every element.
/// Two devices doing that concurrently merge into a list holding *both* copies — every
/// chapter twice — and even alone it makes an unrelated rename conflict with a reorder
/// arriving by sync. Removing what left and moving only what moved keeps the change as
/// small as the edit that caused it. Mirror of `plotweb_crdt::book::reconcile_list`.
fn set_list(doc: &mut AutoCommit, obj: &ObjId, items: &[String]) {
    // `items` is deduped by the caller's data model; `have` may not be, if a merge
    // already put a duplicate here. Walking the target below removes the extra copy,
    // so writing the list is also what repairs it.
    let mut have = read_list(doc, obj);
    for i in (0..have.len()).rev() {
        if !items.contains(&have[i]) {
            let _ = doc.delete(obj, i);
            have.remove(i);
        }
    }
    for (i, id) in items.iter().enumerate() {
        if have.get(i) == Some(id) {
            continue;
        }
        if let Some(pos) = have.iter().position(|h| h == id) {
            let _ = doc.delete(obj, pos);
            have.remove(pos);
        }
        let _ = doc.insert(obj, i, id.as_str());
        have.insert(i, id.clone());
    }
    // Anything past the target is a leftover duplicate — the loop above positions each
    // wanted id once and stops. Without this the repeat simply stays, and writing the
    // list would never repair a document a merge had already doubled up.
    while have.len() > items.len() {
        let _ = doc.delete(obj, have.len() - 1);
        have.pop();
    }
}

/// Drop repeated ids, keeping the first occurrence.
///
/// A chapter cannot be in two places at once, so a repeat in an order list carries no
/// meaning — but Automerge will happily hold one. Two writers inserting the same id is
/// enough: the browser's dual-write into this document and the server's apply of the
/// same REST change into the canonical one are concurrent insertions of equal values,
/// and a merge keeps both. Collapsing on read *and* on write means neither a stale
/// document nor a future second writer can show an author their chapter twice.
fn dedupe(ids: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    ids.into_iter().filter(|id| seen.insert(id.clone())).collect()
}

/// Raw contents, duplicates included — [`set_list`] needs to see a duplicate in order
/// to delete it.
fn read_list(doc: &AutoCommit, obj: &ObjId) -> Vec<String> {
    let len = doc.length(obj);
    let mut out = Vec::with_capacity(len);
    for i in 0..len {
        if let Ok(Some((v, _))) = doc.get(obj, i)
            && let Some(str_val) = v.to_str()
        {
            out.push(str_val.to_string());
        }
    }
    out
}

/// Delete the keys of a `Map` that `keep` does not mention.
///
/// The counterpart of the above, and it takes `keep` for the same reason: clearing the
/// map and rewriting it re-put every key, so a title arriving by sync was overwritten
/// by whatever this device last read over REST.
fn retain_keys(doc: &mut AutoCommit, obj: &ObjId, keep: &[String]) {
    let keys: Vec<String> = doc.keys(obj).collect();
    for k in keys {
        if !keep.contains(&k) {
            let _ = doc.delete(obj, k.as_str());
        }
    }
}

/// Put `value` only if it differs from what is stored — so an unchanged title is not a
/// change at all, and cannot lose a race with one arriving by sync.
fn put_if_changed(doc: &mut AutoCommit, obj: &ObjId, key: &str, value: &str) {
    let current = doc
        .get(obj, key)
        .ok()
        .flatten()
        .and_then(|(v, _)| v.to_str().map(|s| s.to_string()));
    if current.as_deref() != Some(value) {
        let _ = doc.put(obj, key, value);
    }
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

    fn chapter(id: &str, title: &str, order: i64) -> Chapter {
        Chapter {
            id: id.into(),
            book_id: "b".into(),
            title: title.into(),
            content: String::new(),
            sort_order: order,
            word_count: 0,
            created_at: "2026-01-01 00:00:00".into(),
            updated_at: "2026-01-01 00:00:00".into(),
        }
    }

    /// A plain lore note: no span, no entity mark, no event parent — what every note
    /// in every existing book is.
    fn note(id: &str, title: &str, color: Option<&str>) -> Note {
        Note {
            id: id.into(),
            book_id: "b".into(),
            title: title.into(),
            content: String::new(),
            color: color.map(|c| c.to_string()),
            created_at: "2026-01-01 00:00:00".into(),
            updated_at: "2026-01-01 00:00:00".into(),
            span: None,
            relative: None,
            is_entity: false,
            event_parent: None,
            links: plotweb_common::NoteLinks::default(),
        }
    }

    fn sample_book() -> Book {
        Book {
            id: "b".into(),
            title: "The Book".into(),
            description: "desc".into(),
            created_at: "2026-01-01 00:00:00".into(),
            updated_at: "2026-01-01 00:00:00".into(),
            chapter_count: Some(2),
            word_count: Some(0),
            font_settings: None,
            cover_image: None,
            cutover: false,
        }
    }

    /// Read chapter order + titles straight out of a doc (mirrors project_chapters
    /// without needing an AppStore).
    fn read_chapters(doc: &AutoCommit) -> Vec<(String, String)> {
        let chs = get_obj(doc, &ROOT, "chapters").unwrap();
        let titles = get_obj(doc, &ROOT, "chapter_titles")
            .map(|o| read_map_strings(doc, &o))
            .unwrap_or_default();
        read_list_strings(doc, &chs)
            .into_iter()
            .map(|id| {
                let t = titles.get(&id).cloned().unwrap_or_default();
                (id, t)
            })
            .collect()
    }

    /// Reconstruct the notes tree + titles/colors from a doc (mirrors project_notes).
    fn read_notes(doc: &AutoCommit) -> (NoteTree, HashMap<String, String>, HashMap<String, String>) {
        let notes_obj = get_obj(doc, &ROOT, "notes").unwrap();
        let root_order = get_obj(doc, &notes_obj, "root_order")
            .map(|o| read_list_strings(doc, &o))
            .unwrap_or_default();
        let mut children = HashMap::new();
        if let Some(children_obj) = get_obj(doc, &notes_obj, "children") {
            for key in doc.keys(&children_obj) {
                if let Some(list) = get_obj(doc, &children_obj, key.as_str()) {
                    let kids = read_list_strings(doc, &list);
                    if !kids.is_empty() {
                        children.insert(key, kids);
                    }
                }
            }
        }
        let mut collapsed = Vec::new();
        if let Some(collapsed_obj) = get_obj(doc, &notes_obj, "collapsed") {
            for key in doc.keys(&collapsed_obj) {
                if doc
                    .get(&collapsed_obj, key.as_str())
                    .ok()
                    .flatten()
                    .and_then(|(v, _)| v.to_bool())
                    .unwrap_or(false)
                {
                    collapsed.push(key);
                }
            }
        }
        let titles = get_obj(doc, &notes_obj, "titles")
            .map(|o| read_map_strings(doc, &o))
            .unwrap_or_default();
        let colors = get_obj(doc, &notes_obj, "colors")
            .map(|o| read_map_strings(doc, &o))
            .unwrap_or_default();
        (
            NoteTree {
                root_order,
                children,
                collapsed,
            },
            titles,
            colors,
        )
    }

    /// The acceptance proof: build a `book:` doc (2 chapters in order + a nested
    /// notes tree), mutate it (reorder a chapter, rename a note), persist through
    /// [`DocStore`] onto an `FsStore`, drop everything, reopen from a *fresh* store
    /// over the same dir, and assert the projected structure equals the mutated
    /// original — losslessly, nesting and all.
    #[test]
    fn book_doc_survives_persist_drop_reload() {
        // n1 has child n2; n3 is a second root. n1 is collapsed.
        let notes = vec![
            note("n1", "Characters", Some("teal")),
            note("n2", "Alice", Some("red")),
            note("n3", "Places", None),
        ];
        let tree = NoteTree {
            root_order: vec!["n1".into(), "n3".into()],
            children: HashMap::from([("n1".to_string(), vec!["n2".to_string()])]),
            collapsed: vec!["n1".into()],
        };
        let chapters = vec![chapter("c1", "Opening", 0), chapter("c2", "The Storm", 1)];
        let book = sample_book();

        let dir = tempfile::tempdir().expect("tempdir");

        // ── Session 1: seed, mutate, persist ──
        {
            let store: Rc<dyn Store> = Rc::new(FsStore::open(dir.path()).unwrap());
            let ds = DocStore::with_backend(store, "book:proof");

            let mut doc = AutoCommit::new();
            build_doc(&mut doc, &book, &chapters, &notes, &tree);

            // Reorder chapters: c2 before c1.
            let reordered = vec![chapters[1].clone(), chapters[0].clone()];
            let chs = ensure_obj(&mut doc, &ROOT, "chapters", ObjType::List);
            set_list(
                &mut doc,
                &chs,
                &reordered.iter().map(|c| c.id.clone()).collect::<Vec<_>>(),
            );

            // Rename note n2 (targeted Map put, structure untouched).
            let notes_obj = ensure_obj(&mut doc, &ROOT, "notes", ObjType::Map);
            let titles = ensure_obj(&mut doc, &notes_obj, "titles", ObjType::Map);
            doc.put(&titles, "n2", "Alice Liddell").unwrap();

            block_on(ds.publish_snapshot(&doc.save())).unwrap();
        } // drop store + doc entirely — simulate app exit

        // ── Session 2: fresh store over the same dir, reconstruct, compare ──
        let reopened: Rc<dyn Store> = Rc::new(FsStore::open(dir.path()).unwrap());
        let ds = DocStore::with_backend(reopened, "book:proof");
        let persisted = block_on(ds.load()).unwrap().expect("a persisted book doc");
        let mut doc = AutoCommit::load(&persisted.snapshot).unwrap();
        for d in &persisted.deltas {
            doc.load_incremental(d).unwrap();
        }

        // Chapter order + titles survived, with the reorder applied.
        assert_eq!(
            read_chapters(&doc),
            vec![
                ("c2".to_string(), "The Storm".to_string()),
                ("c1".to_string(), "Opening".to_string()),
            ],
            "chapter order + titles must round-trip with the reorder applied"
        );

        // Notes tree nesting + collapse survived; the rename applied; colors kept.
        let (rt_tree, rt_titles, rt_colors) = read_notes(&doc);
        assert_eq!(rt_tree, tree, "notes tree (order/nesting/collapse) must round-trip");
        assert_eq!(
            rt_titles.get("n2").map(String::as_str),
            Some("Alice Liddell"),
            "the note rename must survive the reload"
        );
        assert_eq!(rt_titles.get("n1").map(String::as_str), Some("Characters"));
        assert_eq!(rt_colors.get("n1").map(String::as_str), Some("teal"));
        assert_eq!(rt_colors.get("n2").map(String::as_str), Some("red"));
        assert_eq!(rt_colors.get("n3"), None, "a colorless note stays absent");

        // Meta round-tripped.
        let (title, desc, _fs) = read_meta(&doc).expect("meta");
        assert_eq!(title, "The Book");
        assert_eq!(desc, "desc");
    }

    /// The facets a note can carry, read back out of a document the way `project_notes`
    /// does. Everything the timeline needs has to survive a reload from the `book:`
    /// document alone — if it did not, drawing a timeline would mean opening every
    /// `note:{id}` body document.
    fn read_note_facets(
        doc: &AutoCommit,
    ) -> (
        HashMap<String, plotweb_common::TimeSpan>,
        HashMap<String, plotweb_common::RelativeTime>,
        Vec<String>,
        HashMap<String, String>,
        HashMap<String, plotweb_common::NoteLinks>,
    ) {
        let notes_obj = get_obj(doc, &ROOT, "notes").unwrap();
        let mut entities: Vec<String> = get_obj(doc, &notes_obj, "entities")
            .map(|o| doc.keys(&o).collect())
            .unwrap_or_default();
        entities.sort();
        (
            read_json_map(doc, &notes_obj, "spans"),
            read_json_map(doc, &notes_obj, "relatives"),
            entities,
            get_obj(doc, &notes_obj, "event_parents")
                .map(|o| read_map_strings(doc, &o))
                .unwrap_or_default(),
            read_json_map(doc, &notes_obj, "links"),
        )
    }

    #[test]
    fn note_facets_and_links_survive_a_reload_and_stay_off_the_tree() {
        use plotweb_common::{RelativeTime, TimePoint, TimeRelation, TimeSpan};

        // n2 is filed under n1 in the tree, and contained in time by n3. The two say
        // different things and must not be confused for one another.
        let span = TimeSpan {
            start: TimePoint::base_unit(1204),
            end: Some(TimePoint::base_unit(1261)),
            approximate: true,
            open_ended: false,
        };
        let mut n2 = note("n2", "Vess", Some("red"));
        n2.span = Some(span.clone());
        n2.is_entity = true;
        n2.event_parent = Some("n3".into());
        let mut n3 = note("n3", "The siege", None);
        n3.relative = Some(RelativeTime {
            relation: TimeRelation::After,
            note_id: "n2".into(),
        });
        let notes = vec![note("n1", "Characters", Some("teal")), n2, n3];
        let tree = NoteTree {
            root_order: vec!["n1".into(), "n3".into()],
            children: HashMap::from([("n1".to_string(), vec!["n2".to_string()])]),
            collapsed: vec![],
        };

        let dir = tempfile::tempdir().expect("tempdir");
        {
            let store: Rc<dyn Store> = Rc::new(FsStore::open(dir.path()).unwrap());
            let ds = DocStore::with_backend(store, "book:facets");
            let mut doc = AutoCommit::new();
            build_doc(&mut doc, &sample_book(), &[], &notes, &tree);

            // The link index is written per note from the body just saved, the way
            // `note_links` does it beside a save.
            let notes_obj = ensure_obj(&mut doc, &ROOT, "notes", ObjType::Map);
            let links_obj = ensure_obj(&mut doc, &notes_obj, "links", ObjType::Map);
            let links = plotweb_common::extract_note_links(
                r#"{"type":"doc","content":[{"type":"paragraph","content":[
                    {"type":"text","text":"$Vess held the wall. #siege @Karel"}]}]}"#,
            );
            doc.put(&links_obj, "n3", serde_json::to_string(&links).unwrap())
                .unwrap();

            block_on(ds.publish_snapshot(&doc.save())).unwrap();
        }

        let reopened: Rc<dyn Store> = Rc::new(FsStore::open(dir.path()).unwrap());
        let ds = DocStore::with_backend(reopened, "book:facets");
        let persisted = block_on(ds.load()).unwrap().expect("a persisted book doc");
        let doc = AutoCommit::load(&persisted.snapshot).unwrap();

        let (spans, relatives, entities, event_parents, links) = read_note_facets(&doc);
        assert_eq!(spans.get("n2"), Some(&span));
        assert_eq!(entities, vec!["n2".to_string()]);
        assert_eq!(
            relatives.get("n3").map(|r| r.relation),
            Some(TimeRelation::After)
        );
        assert_eq!(
            links
                .get("n3")
                .map(|l| l.refs.iter().map(|e| e.text.clone()).collect::<Vec<_>>()),
            Some(vec!["Vess".to_string()])
        );
        assert_eq!(links.get("n3").map(|l| l.tags.clone()), Some(vec!["siege".to_string()]));

        // The two hierarchies, side by side and disagreeing on purpose.
        let (rt_tree, _, _) = read_notes(&doc);
        assert_eq!(
            rt_tree.children.get("n1"),
            Some(&vec!["n2".to_string()]),
            "n2 is still filed under n1"
        );
        assert_eq!(
            event_parents.get("n2").map(String::as_str),
            Some("n3"),
            "and still contained in time by n3 — neither placement implies the other"
        );
        assert!(
            !rt_tree.root_order.contains(&"n2".to_string())
                && rt_tree.children.get("n3").is_none(),
            "setting an event parent must not have moved n2 under n3 in the tree"
        );
    }

    #[test]
    fn a_note_without_facets_writes_nothing_into_the_facet_maps() {
        // What every existing note must look like: indistinguishable from before the
        // revamp, with no entry anywhere new.
        let notes = vec![note("n1", "Characters", Some("teal"))];
        let tree = NoteTree {
            root_order: vec!["n1".into()],
            children: HashMap::new(),
            collapsed: vec![],
        };
        let mut doc = AutoCommit::new();
        build_doc(&mut doc, &sample_book(), &[], &notes, &tree);

        let (spans, relatives, entities, event_parents, links) = read_note_facets(&doc);
        assert!(spans.is_empty());
        assert!(relatives.is_empty());
        assert!(entities.is_empty());
        assert!(event_parents.is_empty());
        assert!(links.is_empty());
    }
}
