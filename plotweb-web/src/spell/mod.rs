//! Spellchecking: the dictionary engine and the words that feed it.
//!
//! This module is the *foundation* only — it knows how to split prose into words,
//! how to decide whether a word is spelled correctly, and how to get a dictionary
//! onto the device. Nothing here draws anything; the editor plugin that underlines
//! and offers corrections is a separate layer built on [`Speller`].
//!
//! ```ignore
//! spell::speller(move |result| {
//!     let Ok(sp) = result else { return };
//!     sp.borrow_mut().set_entity_words(
//!         spell::entity_words_from_notes(&store.notes.get()).into_iter(),
//!     );
//!     for token in spell::tokenize(paragraph) {
//!         if !sp.borrow().check(&token.text) {
//!             // token.range is where to draw the squiggle
//!         }
//!     }
//! });
//! ```

pub mod engine;
pub mod loader;
pub mod plugin;
pub mod settings;
pub mod tokenize;
pub mod words;

pub use engine::Speller;
pub use loader::{load_speller, speller};
pub use plugin::{Misspelling, SpellcheckPlugin, SpellcheckShared, misspelling_at};
pub use tokenize::{Token, hyphen_parts, tokenize, word_runs};
pub use words::entity_words_from_notes;

/// The real bundled dictionary, against the real en_US files the server serves.
///
/// The unit tests in [`engine`] use a six-word fixture because they are about the
/// custom-word logic; these are about the thing authors actually type into.
#[cfg(test)]
mod real_dictionary_tests {
    use super::*;

    // The same bytes `/api/dictionaries/en_US.{aff,dic}` serves — embedded in the
    // TEST binary only, so neither the wasm bundle nor the desktop binary carries
    // them (they are fetched and cached at runtime; see `loader`).
    const AFF: &str = include_str!("../../../crates/plotweb-server/dictionaries/en_US.aff");
    const DIC: &str = include_str!("../../../crates/plotweb-server/dictionaries/en_US.dic");

    fn load() -> Speller {
        Speller::from_hunspell(AFF, DIC).expect("the bundled en_US dictionary loads")
    }

    #[test]
    fn loads_and_checks_ordinary_english() {
        let s = load();
        for w in [
            "the",
            "manuscript",
            "chapter",
            "rhythm",
            "separate",
            "receive",
        ] {
            assert!(s.check(w), "{w} should be a word");
        }
        for w in ["teh", "manuscrpt", "seperate", "recieve"] {
            assert!(!s.check(w), "{w} should be a misspelling");
        }
    }

    #[test]
    fn handles_contractions_and_compounds() {
        let s = load();
        assert!(s.check("don't"));
        assert!(s.check("don’t"), "typographic apostrophes are folded");
        assert!(s.check("well-known"));
        // Not in the dictionary as a compound, but both halves are.
        assert!(s.check("dragon-tamed"));
    }

    #[test]
    fn suggests_repairs_for_real_misspellings() {
        let s = load();
        let got = s.suggest("seperate", 5);
        assert!(
            got.iter().any(|g| g == "separate"),
            "expected `separate` among {got:?}"
        );
        assert!(got.len() <= 5);
    }

    #[test]
    fn an_authors_own_name_beats_the_dictionary() {
        let mut s = load();
        s.add_user_word("Elowen");
        assert!(s.check("Elowen"));
        assert!(s.check("elowen"));
        let got = s.suggest("Elowin", 5);
        assert_eq!(got.first().map(String::as_str), Some("Elowen"), "{got:?}");
    }

    #[test]
    fn a_paragraph_of_prose_flags_only_the_typos() {
        let mut s = load();
        s.set_entity_words(["Elowen".to_string(), "Corvane".to_string()].into_iter());
        let text = "Elowen rode for Corvane at dawn, past the NASA-era ruins at \
                    https://example.com/map, and recieved no answer.";
        let bad: Vec<String> = tokenize(text)
            .into_iter()
            .filter(|t| !s.check(&t.text))
            .map(|t| t.text)
            .collect();
        assert_eq!(bad, ["recieved"], "only the real typo should be flagged");
    }

    /// Not an assertion about speed — a printed measurement, so the cost of the
    /// lazy load is a number someone can see rather than a guess. Run with
    /// `--nocapture`.
    #[test]
    fn report_load_time() {
        let start = std::time::Instant::now();
        let s = load();
        let elapsed = start.elapsed();
        assert!(s.check("dictionary"));
        println!(
            "en_US load: {:?} ({} aff bytes, {} dic bytes)",
            elapsed,
            AFF.len(),
            DIC.len()
        );
    }
}
