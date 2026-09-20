//! Plain-text search — the part that knows nothing about documents.
//!
//! One function, [`find_in_text`], and the two switches an author gets: Match
//! case and Whole word. No regex, deliberately — a find that can be typed
//! wrongly into a syntax error is not a find an author reaches for mid-sentence,
//! and "replace all in book" backed by a regex is a much sharper tool than the
//! one this feature is.
//!
//! # Why this is not `str::find` plus `to_lowercase`
//!
//! The obvious case-insensitive spelling — lowercase the haystack, lowercase the
//! needle, `str::find`, reuse the byte offsets — is wrong, and wrong in a way
//! that corrupts documents rather than merely missing matches. Unicode
//! lowercasing is **not length-preserving**: `İ` (U+0130) lowercases to two
//! chars (`i` + U+0307), `İ` is two bytes and its lowercase is three. Offsets
//! taken in the folded string therefore do not index the original, and a replace
//! driven by them would cut a document at a byte that is not even a char
//! boundary.
//!
//! So the comparison runs over **char iterators**: each haystack char is folded
//! as it is reached and its folded chars are matched against the pre-folded
//! needle, while the byte cursor stays in the original string. A match ends only
//! on a haystack char boundary — a char whose folding would satisfy only *part*
//! of the remaining needle is not a match, because there is no position in the
//! original text that corresponds to the split.
//!
//! # Matches do not overlap
//!
//! Scanning resumes at the end of each match, so `aa` in `aaaa` is two matches
//! and `aba` in `ababa` is one. That is the rule replace needs (overlapping
//! matches have no consistent replacement), and it is what every editor's find
//! does.

use std::ops::Range;

/// The two switches on the find panel. `Default` is the plain search: any case,
/// anywhere in a word.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FindOptions {
    /// `a` does not match `A`.
    pub match_case: bool,
    /// The match must be bounded by a non-word character (or the end of the
    /// text) on both sides. "Word" here is `char::is_alphanumeric` plus `_` —
    /// Unicode-aware, so `café` is one word and `naïve` is not two.
    pub whole_word: bool,
}

/// Every non-overlapping occurrence of `query` in `text`, as **byte** ranges
/// into `text`, in order.
///
/// An empty `query` matches nothing: the alternative (a zero-width match at
/// every position) is a result list as long as the book and a replace-all that
/// rewrites every character, which is never what clearing the field meant.
pub fn find_in_text(text: &str, query: &str, opts: FindOptions) -> Vec<Range<usize>> {
    let needle = fold_str(query, opts.match_case);
    if needle.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut cursor = 0usize;
    loop {
        match match_at(text, cursor, &needle, opts) {
            Some(end) => {
                out.push(cursor..end);
                // Non-overlapping: resume past the match. `end > cursor` always,
                // since a non-empty needle consumed at least one haystack char.
                cursor = end;
            }
            None => {
                // No match here (or one the whole-word rule refused): try the
                // next char. Stepping by one *char* keeps the cursor on a
                // boundary, which `match_at` relies on.
                let Some(ch) = text[cursor..].chars().next() else {
                    break;
                };
                cursor += ch.len_utf8();
            }
        }
        if cursor >= text.len() {
            break;
        }
    }
    out
}

/// The needle, folded once: the query's chars, lowercased unless the author
/// asked for a case-sensitive search.
fn fold_str(query: &str, match_case: bool) -> Vec<char> {
    if match_case {
        query.chars().collect()
    } else {
        query.chars().flat_map(char::to_lowercase).collect()
    }
}

/// Try to match `needle` in `text` starting at byte offset `start` (which must
/// be a char boundary). Returns the **byte** offset just past the match.
fn match_at(text: &str, start: usize, needle: &[char], opts: FindOptions) -> Option<usize> {
    let mut consumed = 0usize;
    let mut end = start;
    let mut complete = false;

    for (offset, ch) in text[start..].char_indices() {
        // Fold this haystack char and spend its output against the needle. A
        // char whose folding runs past the end of the needle, or disagrees
        // partway, ends the attempt: the match would have to stop *inside* one
        // source char, and no byte offset describes that.
        let mut spent_cleanly = true;
        if opts.match_case {
            spent_cleanly = needle.get(consumed) == Some(&ch);
            if spent_cleanly {
                consumed += 1;
            }
        } else {
            for folded in ch.to_lowercase() {
                if needle.get(consumed) == Some(&folded) {
                    consumed += 1;
                } else {
                    spent_cleanly = false;
                    break;
                }
            }
        }
        if !spent_cleanly {
            return None;
        }
        end = start + offset + ch.len_utf8();
        if consumed == needle.len() {
            complete = true;
            break;
        }
    }

    if !complete {
        return None;
    }
    if opts.whole_word && !bounded_by_non_word(text, start, end) {
        return None;
    }
    Some(end)
}

/// Whether `text[start..end]` has a non-word char (or nothing at all) on both
/// sides — the whole-word rule.
fn bounded_by_non_word(text: &str, start: usize, end: usize) -> bool {
    let before = text[..start].chars().next_back();
    let after = text[end..].chars().next();
    !before.is_some_and(is_word_char) && !after.is_some_and(is_word_char)
}

/// What counts as being "inside a word" for the whole-word switch.
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain() -> FindOptions {
        FindOptions::default()
    }

    fn cased() -> FindOptions {
        FindOptions {
            match_case: true,
            whole_word: false,
        }
    }

    fn word() -> FindOptions {
        FindOptions {
            match_case: false,
            whole_word: true,
        }
    }

    /// The ranges, as the substrings they name — an assertion that reads as the
    /// text rather than as arithmetic.
    fn found<'a>(text: &'a str, query: &str, opts: FindOptions) -> Vec<&'a str> {
        find_in_text(text, query, opts)
            .into_iter()
            .map(|r| &text[r])
            .collect()
    }

    #[test]
    fn an_empty_query_matches_nothing() {
        assert!(find_in_text("anything at all", "", plain()).is_empty());
        assert!(find_in_text("", "", plain()).is_empty());
        assert!(find_in_text("", "x", plain()).is_empty());
    }

    #[test]
    fn a_plain_search_is_case_insensitive_and_finds_every_occurrence() {
        let text = "Elowen rode. ELOWEN waited. elowen slept.";
        assert_eq!(
            found(text, "elowen", plain()),
            ["Elowen", "ELOWEN", "elowen"]
        );
        assert_eq!(
            find_in_text(text, "elowen", plain()),
            vec![0..6, 13..19, 28..34]
        );
    }

    #[test]
    fn match_case_narrows_to_the_exact_spelling() {
        let text = "Elowen rode. ELOWEN waited. elowen slept.";
        assert_eq!(found(text, "Elowen", cased()), ["Elowen"]);
        assert_eq!(found(text, "elowen", cased()), ["elowen"]);
        assert!(find_in_text(text, "ELOWEn", cased()).is_empty());
    }

    #[test]
    fn whole_word_refuses_a_match_inside_a_longer_word() {
        let text = "the cat sat on the cathedral scatter mat";
        assert_eq!(found(text, "cat", plain()), ["cat", "cat", "cat"]);
        // Only the standalone one survives the boundary check.
        assert_eq!(found(text, "cat", word()), ["cat"]);
        assert_eq!(find_in_text(text, "cat", word()), vec![4..7]);
    }

    #[test]
    fn whole_word_counts_punctuation_and_the_ends_of_the_text_as_boundaries() {
        assert_eq!(found("cat", "cat", word()), ["cat"]);
        assert_eq!(found("(cat)", "cat", word()), ["cat"]);
        assert_eq!(found("cat, cat.", "cat", word()), ["cat", "cat"]);
        // An underscore is part of the word, so this is not a whole-word match.
        assert!(find_in_text("cat_nap", "cat", word()).is_empty());
        assert!(find_in_text("nap_cat", "cat", word()).is_empty());
        // Digits too: `cat5` is one token.
        assert!(find_in_text("cat5", "cat", word()).is_empty());
    }

    #[test]
    fn whole_word_is_unicode_aware() {
        // `é` is a word char, so `caf` is not a whole word inside `café`.
        assert!(find_in_text("café", "caf", word()).is_empty());
        assert_eq!(found("le café noir", "café", word()), ["café"]);
    }

    #[test]
    fn matches_do_not_overlap() {
        // `aa` in `aaaa`: two matches, not three.
        assert_eq!(find_in_text("aaaa", "aa", plain()), vec![0..2, 2..4]);
        // `aaa` in `aaaaa`: one match, and the tail is too short for a second.
        assert_eq!(find_in_text("aaaaa", "aaa", plain()), vec![0..3]);
        // The classic overlapping case: `aba` in `ababa` is one match here.
        assert_eq!(find_in_text("ababa", "aba", plain()), vec![0..3]);
    }

    #[test]
    fn byte_ranges_are_right_when_the_text_is_not_ascii() {
        // Every char before the match is multi-byte, so a char-offset answer
        // would be wrong here and an ascii-only one would be off by six.
        let text = "мама мыла раму";
        let hits = find_in_text(text, "мыла", plain());
        assert_eq!(hits.len(), 1);
        assert_eq!(&text[hits[0].clone()], "мыла");
        assert_eq!(hits[0], 9..17);
    }

    #[test]
    fn case_folding_does_not_shift_the_offsets_it_reports() {
        // `İ` (U+0130, two bytes) lowercases to *two* chars and three bytes.
        // Lowercasing the haystack and reusing the offsets would put the end of
        // this match one byte past where it belongs — inside `s`, here, and
        // inside a multi-byte char in the next test.
        let text = "the İstanbul road";
        let hits = find_in_text(text, "i\u{307}stanbul", plain());
        assert_eq!(hits.len(), 1, "the folded needle matches the folded source");
        assert_eq!(&text[hits[0].clone()], "İstanbul");
    }

    #[test]
    fn a_needle_that_would_end_inside_one_source_char_is_not_a_match() {
        // `İ` folds to `i` + U+0307. A needle of just `i` would be satisfied by
        // the first half of that folding — but there is no byte offset in the
        // source between the two, so this must not match.
        assert!(find_in_text("İ", "i", plain()).is_empty());
        // The same char with its full folding does match.
        assert_eq!(find_in_text("İ", "i\u{307}", plain()), vec![0..2]);
        // And an ordinary `i` elsewhere is still found — at byte 4, because the
        // `İ` ahead of it is two bytes wide.
        assert_eq!(find_in_text("İn it", "i", plain()), vec![4..5]);
    }

    #[test]
    fn a_case_sensitive_search_compares_chars_without_folding() {
        // No folding at all, so the multi-char lowercase never comes up.
        assert_eq!(find_in_text("İstanbul", "İ", cased()), vec![0..2]);
        assert!(find_in_text("İstanbul", "i", cased()).is_empty());
    }

    #[test]
    fn a_query_longer_than_the_text_matches_nothing() {
        assert!(find_in_text("ab", "abcdef", plain()).is_empty());
    }

    #[test]
    fn a_multi_word_query_with_spaces_is_matched_literally() {
        let text = "she rode out at dawn, and rode out again";
        assert_eq!(found(text, "rode out", plain()), ["rode out", "rode out"]);
        assert_eq!(found(text, "rode  out", plain()), Vec::<&str>::new());
    }

    #[test]
    fn whole_word_applies_to_both_ends_of_a_multi_word_query() {
        assert_eq!(found("rode out", "rode out", word()), ["rode out"]);
        assert!(find_in_text("strode outward", "rode out", word()).is_empty());
    }
}
