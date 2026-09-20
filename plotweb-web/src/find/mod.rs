//! Book-wide find and replace.
//!
//! Three layers, the same shape as [`crate::spell`]:
//!
//! - [`matcher`] — what a match *is*: plain text, Match case, Whole word, no
//!   regex. Knows nothing about documents.
//! - [`doc`] — where the matches are in a rinch document, and the transactions
//!   that replace them. Searches the open chapter's live document and a stored
//!   chapter body through the same code path, which is what lets a result row
//!   for an unopened chapter be clicked and land on the right word.
//! - [`plugin`] — the decorations that paint the hits into the open chapter.
//!
//! The panel that drives all three is `pages::book::panes::find`.
//!
//! # Why every replacement is an editor transaction
//!
//! Every book is cut over (see [`crate::sync`]): the CRDT sync engine is the
//! only writer of chapter bodies, and it runs only through a live
//! `EditorHandle`. A replace-all that wrote chapter bodies over REST, or built a
//! document headlessly and PUT it, would be a second writer on a body sync
//! already owns — which is how deleted text comes back. So "replace all in
//! book" opens each chapter through the ordinary chapter-switch path, waits for
//! its body to be attached, and runs the replacement as a transaction on the
//! mounted editor. It is slower than a batch write, and it is the only spelling
//! that is actually a save.

pub mod doc;
pub mod matcher;
pub mod plugin;

pub use doc::{Hit, find_in_chapter_content, find_in_doc, replace_all, replace_one};
pub use matcher::{FindOptions, find_in_text};
pub use plugin::{SearchHighlightPlugin, SearchShared};
