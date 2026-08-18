//! Reading a book's structure out of git, in the shape the CRDT holds it.
//!
//! Three callers need the same adaptation and must agree exactly: the shadow pass
//! (which compares git's structure to the canonical document), the write path (which
//! applies git's structure into the canonical document for a cut-over book), and the
//! mirror (which carries the canonical structure back). If two of them adapted `book.json`
//! slightly differently, the shadow pass would report a divergence the reconciler could
//! never resolve — so the adaptation lives here, once.

use plotweb_crdt::BookStructureInput;
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
        notes: notes
            .iter()
            .map(|n| (n.id.clone(), n.title.clone(), n.color.clone()))
            .collect(),
    }
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
