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
/// themselves and refuses; this is what we do about it. Replacing rather than merging
/// is the whole point — anything we hold that the canonical copy does not is lost, but
/// what we hold is a second copy of the same book, not unsynced work.
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
            });
        });

        // Project the (now-authoritative) local doc into the render signals.
        project(store.clone());

        // The doc exists now, so it can be synced. No-op unless sync is enabled.
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

        let rest = store.chapters.get();
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
        // Server chapters not (yet) in the doc order: keep them, in REST order.
        for c in &rest {
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
        let mut notes = store.notes.get();
        for n in notes.iter_mut() {
            if let Some(t) = titles.get(&n.id) {
                n.title = t.clone();
            }
            n.color = colors.get(&n.id).cloned();
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

    fn note(id: &str, title: &str, color: Option<&str>) -> Note {
        Note {
            id: id.into(),
            book_id: "b".into(),
            title: title.into(),
            content: String::new(),
            color: color.map(|c| c.to_string()),
            created_at: "2026-01-01 00:00:00".into(),
            updated_at: "2026-01-01 00:00:00".into(),
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
}
