//! Which books read from the canonical CRDT (migration phase E).
//!
//! Cutover **inverts the direction rather than switching git off**: for a book that has
//! been cut over, reads come from the canonical document and writes go to both — the
//! CRDT as the source of truth, git as a live mirror. That keeps version history,
//! export and beta-reader views working (all of which read git), keeps rollback real,
//! and lets the shadow pass carry on proving the two agree.
//!
//! # Per book, and reversible in one restart
//!
//! The set lives in `PLOTWEB_CUTOVER_BOOKS` — a comma-separated list of book ids —
//! rather than a column, deliberately. A schema migration would make the first cutover
//! harder to undo than to do, and the whole promise of phase E is that the flag flips
//! back. Unsetting the variable and restarting (`jkbase restart`, no rebuild) returns a
//! book to git-authoritative in about thirty seconds, and because git has been mirroring
//! throughout, it returns to *current* content rather than to whatever it held at the
//! moment of cutover.
//!
//! A per-book field is the right home once more than a handful of books are cut over;
//! it is not the right home for the first one.

use std::collections::HashSet;

/// The books reading from the canonical store, parsed once at startup.
#[derive(Clone, Debug, Default)]
pub struct Cutover {
    books: HashSet<String>,
}

impl Cutover {
    /// Read `PLOTWEB_CUTOVER_BOOKS`. Absent or empty means nothing is cut over, which
    /// is the state every deployment starts in.
    pub fn from_env() -> Self {
        Self::parse(&std::env::var("PLOTWEB_CUTOVER_BOOKS").unwrap_or_default())
    }

    pub fn parse(list: &str) -> Self {
        Cutover {
            books: list
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect(),
        }
    }

    /// Whether this book reads from the canonical document.
    pub fn is_cut_over(&self, book_id: &str) -> bool {
        self.books.contains(book_id)
    }

    pub fn is_empty(&self) -> bool {
        self.books.is_empty()
    }

    pub fn book_ids(&self) -> impl Iterator<Item = &String> {
        self.books.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_absent_or_empty_list_cuts_nothing_over() {
        assert!(Cutover::parse("").is_empty());
        assert!(!Cutover::parse("").is_cut_over("any-book"));
        assert!(!Cutover::parse("  ,  ").is_cut_over("any-book"));
    }

    #[test]
    fn the_list_is_exact_and_tolerates_spacing() {
        let c = Cutover::parse(" book-a , book-b ");
        assert!(c.is_cut_over("book-a"));
        assert!(c.is_cut_over("book-b"));
        assert!(
            !c.is_cut_over("book-"),
            "matching must be exact — a prefix is a different book"
        );
    }
}
