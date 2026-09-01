//! Mirroring sync-originated writes into git (migration phase E).
//!
//! Under cutover the canonical document is the source of truth and git is the mirror.
//! A REST write keeps both in step by itself — it goes to git and is applied to the
//! canonical copy in the same request. A **sync** write does not: a syncing client
//! pushes an update straight into the canonical document and git never hears about it.
//!
//! Left alone that would quietly undo the reason for keeping git at all. Version
//! history, export and beta-reader views read git, so they would freeze at the moment
//! of cutover; and the flag would stop being a rollback, because flipping it back would
//! return the book to whatever git last saw rather than to current content. It would
//! also make the shadow pass permanently red for exactly the documents being used most.
//!
//! # Why it is debounced rather than immediate
//!
//! A client syncing while its author types pushes an update every second or two.
//! Committing each one would bury a book's history in noise — worse than today, where a
//! commit tracks a save. So a changed document is *marked*, and a background pass
//! writes it to git once it has been quiet for [`IDLE`], or in any case within
//! [`MAX_WAIT`] of the first change, so a long writing session still checkpoints.

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

use plotweb_common::{UpdateBookRequest, UpdateChapterRequest};
use plotweb_git::error::GitStoreError;
use plotweb_git::BookStore;
use plotweb_crdt::{BodyKind, BookStructure};

use crate::AppState;

/// How long a document must be quiet before it is written to git.
pub const IDLE: Duration = Duration::from_secs(30);
/// How long a continuously-edited document may go unwritten before it is checkpointed
/// anyway. Without this a long session never commits, which is the failure mode that
/// makes "git is the mirror" a promise rather than a fact.
pub const MAX_WAIT: Duration = Duration::from_secs(300);
/// How often the background pass looks for due documents.
const TICK: Duration = Duration::from_secs(10);

/// What a marked document owes git.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    /// A `chapter:` / `note:` body — one file, written through the ordinary update call.
    Body(BodyKind),
    /// The `book:` document — chapter order and titles, the notes tree, book metadata.
    Structure,
}

#[derive(Clone, Copy)]
struct Pending {
    target: Target,
    first_marked: Instant,
    last_marked: Instant,
}

/// Documents whose canonical copy has moved and whose git mirror has not.
#[derive(Clone, Default)]
pub struct MirrorQueue {
    pending: Arc<StdMutex<HashMap<(String, String), Pending>>>,
}

/// What a document id owes git, or `None` for one git has no representation of
/// (`user:`, which caches dashboard fields rather than authored content).
pub fn target_of_doc(doc_id: &str) -> Option<Target> {
    if doc_id.starts_with("chapter:") {
        Some(Target::Body(BodyKind::Chapter))
    } else if doc_id.starts_with("note:") {
        Some(Target::Body(BodyKind::Note))
    } else if doc_id.starts_with("book:") {
        Some(Target::Structure)
    } else {
        None
    }
}

impl MirrorQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Note that `doc_id` (in `book_id`) has changed and owes git a write.
    pub fn mark(&self, book_id: &str, doc_id: &str, target: Target) {
        let now = Instant::now();
        let mut pending = self.pending.lock().unwrap();
        pending
            .entry((book_id.to_string(), doc_id.to_string()))
            .and_modify(|p| p.last_marked = now)
            .or_insert(Pending {
                target,
                first_marked: now,
                last_marked: now,
            });
    }

    pub fn len(&self) -> usize {
        self.pending.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Take everything due, leaving the rest. `idle`/`max_wait` are parameters so a
    /// test can demand everything immediately.
    fn take_due(&self, idle: Duration, max_wait: Duration) -> Vec<(String, String, Target)> {
        let now = Instant::now();
        let mut pending = self.pending.lock().unwrap();
        let due: Vec<(String, String)> = pending
            .iter()
            .filter(|(_, p)| {
                now.duration_since(p.last_marked) >= idle
                    || now.duration_since(p.first_marked) >= max_wait
            })
            .map(|(key, _)| key.clone())
            .collect();
        due.into_iter()
            .filter_map(|key| {
                pending
                    .remove(&key)
                    .map(|p| (key.0.clone(), key.1.clone(), p.target))
            })
            .collect()
    }
}

/// Write every due document's canonical content into git.
///
/// Returns how many documents were mirrored. Failures are logged and the document is
/// dropped from the queue rather than retried forever: the shadow pass reports the
/// resulting difference, which is a better signal than a queue silently growing.
pub async fn flush(state: &AppState, idle: Duration, max_wait: Duration) -> usize {
    let due = state.mirror.take_due(idle, max_wait);
    let mut written = 0;

    for (book_id, doc_id, target) in due {
        // The canonical copy is read under the same per-document lock a sync write
        // takes, so a half-applied update is never what gets mirrored.
        let bytes = {
            let lock = state.doc_locks.for_doc(&doc_id);
            let _guard = lock.lock().await;
            match crate::sync::canonical_snapshot(&state.crdt_dir, &doc_id) {
                Ok(Some(bytes)) => bytes,
                Ok(None) => continue,
                Err(e) => {
                    eprintln!("[mirror] {doc_id}: store read failed: {e}");
                    continue;
                }
            }
        };

        let mirrored = match target {
            Target::Body(kind) => mirror_body(state, &book_id, &doc_id, &bytes, kind).await,
            // `false`: the mirror never empties a manuscript on its own. Reconcile is
            // where that decision is made deliberately.
            Target::Structure => mirror_structure(&state.books, &book_id, &bytes, false).await,
        };
        if mirrored {
            written += 1;
            println!("[mirror] {doc_id}: written to git");
        }
    }
    written
}

/// Write one body document's canonical content into its git file.
async fn mirror_body(
    state: &AppState,
    book_id: &str,
    doc_id: &str,
    bytes: &[u8],
    kind: BodyKind,
) -> bool {
    let content = match plotweb_crdt::materialize_body(bytes) {
        Ok(content) => content,
        Err(e) => {
            eprintln!("[mirror] {doc_id}: could not materialize: {e}");
            return false;
        }
    };

    // Skip a document whose git copy already says this. A sync round can move the
    // canonical document without changing what it materializes to (a formatting no-op,
    // or an update the server already had), and committing that would put a commit in
    // the author's history that represents no change to their book.
    let current = match kind {
        BodyKind::Chapter => {
            let id = doc_id.trim_start_matches("chapter:");
            state.books.get_chapter(book_id, id).await.map(|c| c.content)
        }
        BodyKind::Note => {
            let id = doc_id.trim_start_matches("note:");
            state.books.get_note(book_id, id).await.map(|n| n.content)
        }
    };
    if current.as_deref().ok() == Some(content.as_str()) {
        return false;
    }

    let result = match kind {
        BodyKind::Chapter => {
            let id = doc_id.trim_start_matches("chapter:").to_string();
            state
                .books
                .update_chapter(
                    book_id,
                    &id,
                    &UpdateChapterRequest {
                        title: None,
                        content: Some(content),
                    },
                )
                .await
        }
        BodyKind::Note => {
            let id = doc_id.trim_start_matches("note:").to_string();
            state
                .books
                .update_note(book_id, &id, None, Some(&content), None)
                .await
        }
    };

    match result {
        Ok(()) => true,
        // The document was deleted while an update for it was still in flight — a
        // device pushing a body it had not yet learned was gone. Expected under §D7,
        // where removal from the parent index is the deletion and the orphaned body
        // outlives it server-side. Reporting it as a failure would train whoever reads
        // these logs to ignore them.
        Err(GitStoreError::ChapterNotFound(_)) | Err(GitStoreError::NoteNotFound(_)) => {
            println!("[mirror] {doc_id}: deleted before this update landed; nothing to mirror");
            false
        }
        Err(e) => {
            eprintln!("[mirror] {doc_id}: git write failed: {e}");
            false
        }
    }
}

/// Bring git's structure into line with the canonical `book:` document.
///
/// Unlike a body — one file, one write — a structure is spread across `book.json`, a
/// file per chapter and the notes repo, so this walks the difference and issues the same
/// calls the REST routes do. Deletions are carried through as well as additions: a git
/// delete removes the file but keeps every past version of it, so mirroring one is no
/// more destructive than the author deleting the chapter in the browser, which does
/// exactly this.
///
/// `allow_emptying` is what separates the background pass from a decision. The pass
/// passes `false` and refuses a canonical structure that has lost every chapter or note
/// while git still has them — likelier a half-written document than an emptied book.
/// [`crate::reconcile`] passes `true`, because there a human named the direction.
pub(crate) async fn mirror_structure(
    books: &BookStore,
    book_id: &str,
    bytes: &[u8],
    allow_emptying: bool,
) -> bool {
    let want = match plotweb_crdt::materialize_book_structure(bytes) {
        Ok(structure) => structure,
        Err(e) => {
            eprintln!("[mirror] book:{book_id}: could not materialize: {e}");
            return false;
        }
    };
    let Some(input) = crate::structure::read_structure_input(books, book_id).await else {
        eprintln!("[mirror] book:{book_id}: no readable structure in git, nothing written");
        return false;
    };
    let have = input.structure();
    if have == want {
        return false;
    }

    // A canonical structure that has lost *everything* is far more likely to be a
    // half-written document than an author who deleted every chapter in their book —
    // and they can do that through the UI, which needs no help from this pass. Refusing
    // costs a stale mirror and a line in the log; not refusing empties a manuscript.
    if !allow_emptying
        && (want.chapters.is_empty() && !have.chapters.is_empty()
            || want.note_titles.is_empty() && !have.note_titles.is_empty())
    {
        eprintln!(
            "[mirror] book:{book_id}: canonical structure is empty but git is not — \
             refusing to mirror; reconcile this deliberately"
        );
        return false;
    }

    let mut wrote = false;

    // Metadata. Sent as one update with only the differing fields, so an unchanged
    // description does not turn into a commit.
    let font_settings = serde_json::from_str(&want.font_settings_json).ok();
    let meta_changed = have.title != want.title
        || have.description != want.description
        || have.font_settings_json != want.font_settings_json
        || have.cover_ref != want.cover_ref;
    if meta_changed {
        let update = UpdateBookRequest {
            title: (have.title != want.title).then(|| want.title.clone()),
            description: (have.description != want.description).then(|| want.description.clone()),
            font_settings: (have.font_settings_json != want.font_settings_json)
                .then_some(font_settings)
                .flatten(),
            cover_image: (have.cover_ref != want.cover_ref).then(|| want.cover_ref.clone()),
        };
        match books.update_book(book_id, &update).await {
            Ok(()) => wrote = true,
            Err(e) => eprintln!("[mirror] book:{book_id}: metadata write failed: {e}"),
        }
    }

    wrote |= mirror_chapters(books, book_id, &have, &want).await;
    wrote |= mirror_notes(books, book_id, &have, &want).await;
    wrote
}

async fn mirror_chapters(
    books: &BookStore,
    book_id: &str,
    have: &BookStructure,
    want: &BookStructure,
) -> bool {
    let mut wrote = false;
    let now = crate::sync::now_stamp();
    let have_ids: Vec<&String> = have.chapters.iter().map(|(id, _)| id).collect();

    for (id, title) in &want.chapters {
        match have.chapters.iter().find(|(hid, _)| hid == id) {
            None => {
                // A chapter created on a device. Its body arrives as its own document,
                // and is mirrored by its own pass; this only has to make the file exist.
                match books.create_chapter(book_id, id, title, &now).await {
                    Ok(_) => wrote = true,
                    Err(e) => eprintln!("[mirror] book:{book_id}: create {id} failed: {e}"),
                }
            }
            Some((_, have_title)) if have_title != title => {
                let update = UpdateChapterRequest {
                    title: Some(title.clone()),
                    content: None,
                };
                match books.update_chapter(book_id, id, &update).await {
                    Ok(()) => wrote = true,
                    Err(e) => eprintln!("[mirror] book:{book_id}: rename {id} failed: {e}"),
                }
            }
            Some(_) => {}
        }
    }

    let wanted: Vec<&String> = want.chapters.iter().map(|(id, _)| id).collect();
    for id in have_ids.iter().filter(|id| !wanted.contains(id)) {
        match books.delete_chapter(book_id, id).await {
            Ok(()) => wrote = true,
            Err(e) => eprintln!("[mirror] book:{book_id}: delete {id} failed: {e}"),
        }
    }

    // Order last: creates append, so the list is only right once everything exists.
    let want_order: Vec<String> = want.chapters.iter().map(|(id, _)| id.clone()).collect();
    let have_order: Vec<String> = have.chapters.iter().map(|(id, _)| id.clone()).collect();
    if want_order != have_order {
        match books.reorder_chapters(book_id, &want_order).await {
            Ok(()) => wrote = true,
            Err(e) => eprintln!("[mirror] book:{book_id}: reorder failed: {e}"),
        }
    }
    wrote
}

async fn mirror_notes(
    books: &BookStore,
    book_id: &str,
    have: &BookStructure,
    want: &BookStructure,
) -> bool {
    let mut wrote = false;
    let now = crate::sync::now_stamp();

    for (id, title) in &want.note_titles {
        let color = want.note_colors.get(id).map(String::as_str);
        match have.note_titles.get(id) {
            None => {
                // Parentage comes from the tree write below, so every note is created at
                // the root and moved into place in one go rather than note by note.
                match books
                    .create_note(book_id, id, title, None, color, &now)
                    .await
                {
                    Ok(_) => wrote = true,
                    Err(e) => eprintln!("[mirror] book:{book_id}: create note {id} failed: {e}"),
                }
            }
            Some(have_title) => {
                let retitle = have_title != title;
                let recolor = have.note_colors.get(id) != want.note_colors.get(id);
                if retitle || recolor {
                    let result = books
                        .update_note(
                            book_id,
                            id,
                            retitle.then_some(title.as_str()),
                            None,
                            recolor.then_some(color),
                        )
                        .await;
                    match result {
                        Ok(()) => wrote = true,
                        Err(e) => {
                            eprintln!("[mirror] book:{book_id}: update note {id} failed: {e}")
                        }
                    }
                }
            }
        }
    }

    for id in have.note_titles.keys() {
        if !want.note_titles.contains_key(id) {
            match books.delete_note(book_id, id).await {
                Ok(()) => wrote = true,
                Err(e) => eprintln!("[mirror] book:{book_id}: delete note {id} failed: {e}"),
            }
        }
    }

    if have.root_order != want.root_order
        || have.children != want.children
        || have.collapsed != want.collapsed
    {
        let tree = plotweb_git::note::NotesTreeJson {
            root_order: want.root_order.clone(),
            children: want.children.clone().into_iter().collect(),
            collapsed: want.collapsed.iter().cloned().collect(),
        };
        match books.update_note_tree(book_id, &tree).await {
            Ok(()) => wrote = true,
            Err(e) => eprintln!("[mirror] book:{book_id}: tree write failed: {e}"),
        }
    }
    wrote
}

/// The background pass. Cheap when idle: it holds no locks and touches nothing unless a
/// sync write has marked a document.
pub async fn run(state: AppState) {
    loop {
        tokio::time::sleep(TICK).await;
        flush(&state, IDLE, MAX_WAIT).await;
    }
}
