//! Turning an author's notes into words the spellchecker should not flag.
//!
//! A manuscript's proper nouns already exist in the book: they are the entity notes
//! — the people, places and things the author wrote down. Feeding those names to the
//! speller means a character called Elowen stops being underlined from the moment
//! she has a note, without the author adding her to a dictionary by hand.
//!
//! Only the **title** is used. A note's body is prose like any other and should be
//! spellchecked, not trusted.

use std::collections::HashSet;

use plotweb_common::Note;

use super::tokenize::word_runs;

/// Every word in the titles of `notes` that carry the entity facet, de-duplicated
/// and in first-seen order.
///
/// Uses the raw word runs rather than [`super::tokenize::tokenize`]: a name like
/// `ERIS` is exactly the sort of thing the prose tokenizer throws away as an
/// acronym, and here it is the point. Runs shorter than two letters, and any run
/// carrying a digit, are dropped — a one-letter title gives the checker nothing, and
/// `Unit-7` is not a word.
pub fn entity_words_from_notes(notes: &[Note]) -> Vec<String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for note in notes.iter().filter(|n| n.is_entity) {
        for token in word_runs(&note.title) {
            if token.text.chars().any(|c| c.is_numeric()) {
                continue;
            }
            if token.text.chars().filter(|c| c.is_alphabetic()).count() < 2 {
                continue;
            }
            if seen.insert(token.text.clone()) {
                out.push(token.text);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(title: &str, is_entity: bool) -> Note {
        Note {
            id: "n".into(),
            book_id: "b".into(),
            title: title.into(),
            content: String::new(),
            color: None,
            created_at: String::new(),
            updated_at: String::new(),
            span: None,
            relative: None,
            is_entity,
            event_parent: None,
            pinned: false,
            links: Default::default(),
        }
    }

    #[test]
    fn only_entity_notes_contribute() {
        let notes = vec![
            note("Elowen Vash", true),
            note("The siege of Corvane", false),
        ];
        assert_eq!(entity_words_from_notes(&notes), ["Elowen", "Vash"]);
    }

    #[test]
    fn multiword_titles_split_and_dedupe_in_order() {
        let notes = vec![
            note("Elowen Vash", true),
            note("Vash Keep", true),
            note("Elowen", true),
        ];
        assert_eq!(entity_words_from_notes(&notes), ["Elowen", "Vash", "Keep"]);
    }

    #[test]
    fn all_caps_names_survive_but_short_and_numeric_runs_do_not() {
        let notes = vec![note("ERIS, Unit-7 and a K drone", true)];
        // `Unit`/`7` are dropped together (the run carries a digit), `a`/`K` are
        // single letters, `and` and `drone` are ordinary words that do no harm.
        assert_eq!(entity_words_from_notes(&notes), ["ERIS", "and", "drone"]);
    }

    #[test]
    fn hyphenated_and_apostrophed_names_stay_whole() {
        let notes = vec![note("Ka’ren Half-Moon", true)];
        assert_eq!(entity_words_from_notes(&notes), ["Ka'ren", "Half-Moon"]);
    }

    #[test]
    fn no_entity_notes_gives_nothing() {
        assert!(entity_words_from_notes(&[note("anything", false)]).is_empty());
        assert!(entity_words_from_notes(&[]).is_empty());
    }
}
