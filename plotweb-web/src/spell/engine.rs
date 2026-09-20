//! The speller: a Hunspell dictionary plus everything this author has told it.
//!
//! [`Speller`] wraps [`spellbook::Dictionary`] with three additional word sources,
//! all of which are accepted exactly like dictionary words:
//!
//! | source | lives for | set by |
//! |---|---|---|
//! | **user words** | the account, forever | the author choosing "Add to dictionary" |
//! | **entity words** | the open book | names pulled from entity notes ([`super::words`]) |
//! | **session ignores** | until reload | the author choosing "Ignore" |
//!
//! They are kept apart rather than merged into one set because they are *forgotten*
//! at different times: switching books must drop the entity words without touching
//! the author's own, and a session ignore must never be persisted.
//!
//! # Case matching
//!
//! A custom word matches its own exact spelling, and additionally any **regularly
//! cased** variant of it: all-lowercase, Capitalised, or ALL-CAPS. So adding
//! `Elowen` accepts `Elowen`, `elowen` and `ELOWEN` — the forms that legitimately
//! occur in prose (mid-sentence, sentence-initial, shouted) — while `ELowen` and
//! `eLoWeN` stay flagged, because those are typos rather than casing.
//!
//! This is deliberately looser than Hunspell's own rule (which would reject a
//! lowercase form of a capitalised entry). An author who adds a name to their
//! dictionary means "this string is a word", and being pedantic about which
//! capitalisation they meant produces exactly the kind of false positive that gets
//! a spellchecker switched off.

use std::collections::HashSet;

use super::tokenize::{hyphen_parts, normalize};

/// A loaded dictionary plus this author's own words.
pub struct Speller {
    dict: spellbook::Dictionary,
    /// Persisted, account-wide (see `crate::local_dictionary`).
    user: WordSet,
    /// Derived from the open book's entity notes; replaced wholesale on book change.
    entity: WordSet,
    /// "Ignore for now" — dropped when the app reloads.
    session: WordSet,
}

/// A set of accepted words, kept alongside a lowercase index so the case-variant
/// rule is a hash lookup rather than a scan.
#[derive(Default)]
struct WordSet {
    exact: HashSet<String>,
    lower: HashSet<String>,
}

impl WordSet {
    fn insert(&mut self, word: &str) -> bool {
        let w = normalize(word.trim());
        if w.is_empty() {
            return false;
        }
        self.lower.insert(w.to_lowercase());
        self.exact.insert(w)
    }

    fn clear(&mut self) {
        self.exact.clear();
        self.lower.clear();
    }

    fn accepts(&self, word: &str, lower: &str, regular_casing: bool) -> bool {
        self.exact.contains(word) || (regular_casing && self.lower.contains(lower))
    }

    fn iter(&self) -> impl Iterator<Item = &str> {
        self.exact.iter().map(String::as_str)
    }
}

/// All-lowercase, `Capitalised`, or `ALL-CAPS` — the capitalisations prose actually
/// uses. See the module docs.
fn is_regular_casing(word: &str) -> bool {
    let mut letters = word.chars().filter(|c| c.is_alphabetic());
    let Some(first) = letters.next() else {
        return true;
    };
    let rest_upper = letters.clone().all(char::is_uppercase);
    let rest_lower = letters.all(char::is_lowercase);
    (first.is_lowercase() && rest_lower) || (first.is_uppercase() && (rest_lower || rest_upper))
}

impl Speller {
    /// Build a speller from Hunspell `.aff` + `.dic` **text**.
    ///
    /// Both must already be UTF-8 `str`; the bundled en_US pair declares `SET UTF-8`
    /// and is stored that way (see `crates/plotweb-server/dictionaries/README.md`),
    /// so no transcoding happens on this path.
    pub fn from_hunspell(aff: &str, dic: &str) -> Result<Self, String> {
        let dict = spellbook::Dictionary::new(aff, dic)
            .map_err(|e| format!("could not parse the dictionary: {e}"))?;
        Ok(Self {
            dict,
            user: WordSet::default(),
            entity: WordSet::default(),
            session: WordSet::default(),
        })
    }

    /// Is `word` spelled correctly, as far as this author is concerned?
    ///
    /// A hyphenated compound the dictionary does not know is accepted when **every**
    /// piece is — `dragon-tamed` passes because `dragon` and `tamed` do.
    pub fn check(&self, word: &str) -> bool {
        let w = normalize(word.trim());
        if w.is_empty() {
            return true;
        }
        if self.check_atom(&w) {
            return true;
        }
        let parts = hyphen_parts(&w);
        parts.len() > 1 && parts.iter().all(|p| self.check_atom(p))
    }

    /// One piece, with no hyphen fallback — the recursion base of [`check`](Self::check).
    fn check_atom(&self, word: &str) -> bool {
        if self.accepts_custom(word) {
            return true;
        }
        self.dict.check(word)
    }

    /// The custom-word half of the lookup, with the case rule from the module docs.
    fn accepts_custom(&self, word: &str) -> bool {
        let lower = word.to_lowercase();
        let regular = is_regular_casing(word);
        self.user.accepts(word, &lower, regular)
            || self.entity.accepts(word, &lower, regular)
            || self.session.accepts(word, &lower, regular)
    }

    /// Up to `max` corrections for a misspelling, best first.
    ///
    /// Returns nothing for a word [`check`](Self::check) already accepts — there is
    /// no correction to offer for something that is not wrong.
    ///
    /// The author's own words come first: a mistyped character name is the single
    /// likeliest mistake in a manuscript, and the bundled dictionary can never
    /// suggest a name it does not contain. The rest come from Hunspell's own
    /// suggester.
    ///
    /// Suggestions are de-duplicated **case-insensitively**, so a list never offers
    /// `Elowen` and `elowen` as if they were different repairs.
    pub fn suggest(&self, word: &str, max: usize) -> Vec<String> {
        if max == 0 {
            return Vec::new();
        }
        let w = normalize(word.trim());
        if w.is_empty() || self.check(&w) {
            return Vec::new();
        }

        let mut out: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();

        let mut near = self.near_custom_words(&w);
        near.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(b.1)));
        for (_, candidate) in near {
            if seen.insert(candidate.to_lowercase()) {
                out.push(candidate.to_string());
            }
            if out.len() >= max {
                return out;
            }
        }

        let mut raw = Vec::new();
        self.dict.suggest(&w, &mut raw);
        for s in raw {
            if seen.insert(s.to_lowercase()) {
                out.push(s);
            }
            if out.len() >= max {
                break;
            }
        }
        out
    }

    /// Custom words within a small edit distance of `word`, as `(distance, word)`.
    fn near_custom_words(&self, word: &str) -> Vec<(usize, &str)> {
        let lower = word.to_lowercase();
        // One edit for short words, two for longer ones: two edits on a four-letter
        // word matches almost anything.
        let budget = if lower.chars().count() <= 4 { 1 } else { 2 };
        self.user
            .iter()
            .chain(self.entity.iter())
            .chain(self.session.iter())
            .filter_map(|candidate| {
                let d = edit_distance(&lower, &candidate.to_lowercase(), budget)?;
                (d > 0).then_some((d, candidate))
            })
            .collect()
    }

    /// Add a word to the account's permanent dictionary.
    pub fn add_user_word(&mut self, word: &str) {
        self.user.insert(word);
    }

    /// Replace the account's permanent dictionary — what a pull from the server (or
    /// the local mirror) installs.
    pub fn set_user_words(&mut self, words: impl IntoIterator<Item = String>) {
        self.user.clear();
        for w in words {
            self.user.insert(&w);
        }
    }

    /// Replace the entity words for the book currently open.
    pub fn set_entity_words(&mut self, words: impl Iterator<Item = String>) {
        self.entity.clear();
        for w in words {
            self.entity.insert(&w);
        }
    }

    /// Accept `word` until the app reloads, without persisting anything.
    pub fn ignore_session(&mut self, word: &str) {
        self.session.insert(word);
    }

    /// The account's permanent words, unordered — for pushing the list somewhere.
    pub fn user_words(&self) -> impl Iterator<Item = &str> {
        self.user.iter()
    }
}

/// Levenshtein distance between two strings, giving up (returning `None`) once it
/// is certain to exceed `budget`.
///
/// Bounded on purpose: this runs over every custom word for every misspelling, and
/// the answer past the budget is never used.
fn edit_distance(a: &str, b: &str, budget: usize) -> Option<usize> {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.len().abs_diff(b.len()) > budget {
        return None;
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr = vec![0usize; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        curr[0] = i + 1;
        let mut row_min = curr[0];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            curr[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(curr[j] + 1);
            row_min = row_min.min(curr[j + 1]);
        }
        if row_min > budget {
            return None;
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    let d = prev[b.len()];
    (d <= budget).then_some(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny hand-written dictionary, for the behaviour that does not need 79,000
    /// words. The real en_US pair is exercised in `super::real_dictionary_tests`.
    fn tiny() -> Speller {
        let aff = "SET UTF-8\n";
        let dic = "6\ncat\ndog\ndragon\ntamed\nknown\nwell\n";
        Speller::from_hunspell(aff, dic).expect("tiny dictionary loads")
    }

    #[test]
    fn checks_against_the_dictionary() {
        let s = tiny();
        assert!(s.check("cat"));
        assert!(!s.check("ct"));
    }

    #[test]
    fn empty_input_is_never_a_mistake() {
        let s = tiny();
        assert!(s.check(""));
        assert!(s.check("   "));
    }

    #[test]
    fn hyphenated_compounds_fall_back_to_their_parts() {
        let s = tiny();
        assert!(!s.check("dragontamed"));
        assert!(s.check("dragon-tamed"), "both halves are known words");
        assert!(!s.check("dragon-tamd"), "one bad half fails the whole");
    }

    #[test]
    fn user_words_are_accepted_with_regular_case_variants() {
        let mut s = tiny();
        s.add_user_word("Elowen");
        assert!(s.check("Elowen"));
        assert!(s.check("elowen"), "sentence-initial vs mid-sentence");
        assert!(s.check("ELOWEN"), "shouted");
        assert!(
            !s.check("ELowen"),
            "irregular casing is a typo, not a variant"
        );
        assert!(!s.check("Elowyn"));
    }

    #[test]
    fn a_lowercase_user_word_still_accepts_its_capitalised_form() {
        let mut s = tiny();
        s.add_user_word("skyfarer");
        assert!(s.check("Skyfarer"));
        assert!(s.check("skyfarer"));
    }

    #[test]
    fn entity_and_session_words_are_accepted_and_kept_separate() {
        let mut s = tiny();
        s.set_entity_words(["Corvane".to_string()].into_iter());
        s.ignore_session("brrrap");
        assert!(s.check("Corvane"));
        assert!(s.check("brrrap"));

        // Changing books replaces entity words without touching the rest.
        s.set_entity_words(std::iter::empty());
        assert!(!s.check("Corvane"));
        assert!(s.check("brrrap"));
    }

    #[test]
    fn set_user_words_replaces_the_whole_list() {
        let mut s = tiny();
        s.add_user_word("Elowen");
        s.set_user_words(vec!["Corvane".to_string()]);
        assert!(!s.check("Elowen"));
        assert!(s.check("Corvane"));
    }

    #[test]
    fn user_words_round_trip_out_again() {
        let mut s = tiny();
        s.add_user_word("Elowen");
        s.add_user_word("  Corvane  ");
        s.add_user_word("");
        let mut got: Vec<&str> = s.user_words().collect();
        got.sort_unstable();
        assert_eq!(got, ["Corvane", "Elowen"]);
    }

    #[test]
    fn a_curly_apostrophe_matches_a_straight_one() {
        let mut s = tiny();
        s.add_user_word("Ka'ren");
        assert!(s.check("Ka’ren"));
    }

    #[test]
    fn a_correct_word_gets_no_suggestions() {
        let mut s = tiny();
        s.add_user_word("Elowen");
        assert!(s.suggest("cat", 5).is_empty());
        assert!(s.suggest("elowen", 5).is_empty());
    }

    #[test]
    fn suggestions_offer_the_authors_own_names_first() {
        let mut s = tiny();
        s.add_user_word("Elowen");
        let got = s.suggest("Elowyn", 5);
        assert_eq!(got.first().map(String::as_str), Some("Elowen"), "{got:?}");
    }

    #[test]
    fn suggestions_are_capped_and_case_deduplicated() {
        let mut s = tiny();
        s.add_user_word("Elowen");
        s.ignore_session("elowen");
        let got = s.suggest("Elowyn", 5);
        assert_eq!(got.len(), 1, "one repair, not two casings of it: {got:?}");
        assert!(s.suggest("Elowyn", 0).is_empty());
    }

    #[test]
    fn edit_distance_respects_its_budget() {
        assert_eq!(edit_distance("cat", "cat", 2), Some(0));
        assert_eq!(edit_distance("cat", "cot", 2), Some(1));
        assert_eq!(edit_distance("cat", "dog", 2), None);
        assert_eq!(edit_distance("cat", "catastrophe", 2), None);
    }

    #[test]
    fn regular_casing_recognises_the_three_prose_forms() {
        assert!(is_regular_casing("elowen"));
        assert!(is_regular_casing("Elowen"));
        assert!(is_regular_casing("ELOWEN"));
        assert!(!is_regular_casing("ELowen"));
        assert!(!is_regular_casing("eLoWeN"));
    }
}
