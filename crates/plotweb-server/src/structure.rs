//! Reading a book's structure out of git, in the shape the CRDT holds it.
//!
//! Three callers need the same adaptation and must agree exactly: the shadow pass
//! (which compares git's structure to the canonical document), the write path (which
//! applies git's structure into the canonical document for a cut-over book), and the
//! mirror (which carries the canonical structure back). If two of them adapted `book.json`
//! slightly differently, the shadow pass would report a divergence the reconciler could
//! never resolve — so the adaptation lives here, once.

use plotweb_common::{extract_note_links_in, LinkIndex};
use plotweb_crdt::{BookStructureInput, NoteEntry};
use plotweb_git::note::{NoteData, NotesTreeJson};
use plotweb_git::{BookData, BookStore, ChapterData};

/// Adapt what the git store returns into the CRDT's structure input.
///
/// `chapters` must already be in authoritative order (`book.json`'s `chapter_order`),
/// which is the order `list_chapters` returns them in.
pub fn structure_input(
    book: &BookData,
    chapters: &[ChapterData],
    notes: &[NoteData],
    tree: &NotesTreeJson,
) -> BookStructureInput {
    let index = link_index(chapters, notes);
    BookStructureInput {
        title: book.title.clone(),
        description: book.description.clone(),
        font_settings: book.font_settings.clone(),
        cover_ref: book.cover_image.clone(),
        created_at: book.created_at.clone(),
        chapters: chapters
            .iter()
            .map(|c| (c.id.clone(), c.title.clone()))
            .collect(),
        root_order: tree.root_order.clone(),
        children: tree.children.clone(),
        collapsed: tree.collapsed.clone(),
        notes: notes.iter().map(|n| note_entry(n, &index)).collect(),
    }
}

/// The titles a `@` or `$` in any of this book's notes can name.
///
/// Chapters are in here because `@` reaches them: a note can point at the chapter that
/// dramatises it, and the edge records which of the two it found. Notes are added first
/// so they win a shared title (see `LinkIndex`).
fn link_index(chapters: &[ChapterData], notes: &[NoteData]) -> LinkIndex {
    LinkIndex::new()
        .with_notes(notes.iter().map(|n| (&n.id, &n.title)))
        .with_chapters(chapters.iter().map(|c| (&c.id, &c.title)))
}

/// One note, with its link index derived from its body.
///
/// The index is **derived here rather than stored**, so it cannot drift from the body:
/// there is no second source to keep in step, and a body edited by any path produces a
/// fresh index the next time the structure is written. The cost is that a cut-over
/// book's index tracks git's copy of the body, which lags the canonical copy by the
/// mirror's debounce — the client writes its own index into its local document beside
/// the REST save (`local_book::note_meta`), which is what closes that gap for the device
/// doing the typing.
fn note_entry(n: &NoteData, index: &LinkIndex) -> NoteEntry {
    NoteEntry {
        id: n.id.clone(),
        title: n.title.clone(),
        color: n.color.clone(),
        span: n.span.clone(),
        relative: n.relative.clone(),
        is_entity: n.is_entity,
        event_parent: n.event_parent.clone(),
        links: extract_note_links_in(&n.content, index),
    }
}

/// The titles a sigil in this book's notes can name, fetched.
///
/// Only the **non**-cut-over read paths need this: a cut-over book's edges are already
/// resolved in the canonical structure document, and re-deriving them here would read
/// git's lagging copy of the body. Returns an empty index when the book is unreadable,
/// which leaves every token unresolved rather than mis-resolved.
pub async fn read_link_index(books: &BookStore, book_id: &str) -> LinkIndex {
    let chapters = books.list_chapters(book_id).await.unwrap_or_default();
    let notes = books.list_notes(book_id).await.unwrap_or_default().0;
    link_index(&chapters, &notes)
}

/// The same thing, fetched. `None` when the book has no readable `book.json` — there is
/// no structure to record, and inventing an empty one would look like a book that had
/// been emptied.
pub async fn read_structure_input(books: &BookStore, book_id: &str) -> Option<BookStructureInput> {
    let book = books.get_book(book_id).await.ok()?;
    let chapters = books.list_chapters(book_id).await.unwrap_or_default();
    let (notes, tree) = books.list_notes(book_id).await.unwrap_or_default();
    Some(structure_input(&book, &chapters, &notes, &tree))
}
