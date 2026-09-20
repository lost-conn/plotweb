//! Splitting prose into the words a spellchecker should actually look at.
//!
//! The hard part of spellchecking manuscript text is not the dictionary — it is
//! deciding what *counts* as a word. A checker that underlines `https://…`, `NASA`,
//! `snake_case` and `2026` is one an author turns off within a minute, so the rules
//! here are deliberately conservative: when in doubt, don't check it.
//!
//! # What becomes a token
//!
//! A token is a maximal run of Unicode letters and digits, plus **internal**
//! apostrophes and hyphens, with surrounding quotes and punctuation trimmed off.
//! `don't`, `well-known` and `O'Brien` are each one token; `“Hello,”` yields
//! `Hello`.
//!
//! # What is skipped
//!
//! - anything containing a digit (`h3llo`, `2026`, `v2`)
//! - ALL-CAPS runs of two or more letters — acronyms (`NASA`, `US`, `POV`)
//! - anything in a whitespace-delimited chunk holding `@`, `/`, `\` or `_`, or an
//!   internal dot with word characters on both sides. That one rule covers email
//!   addresses, URLs, bare domains, file paths and identifiers in a single pass,
//!   which matters because those are exactly the strings whose *pieces* look like
//!   words (`example`, `com`, `snake`, `case`).
//!
//! # Apostrophes
//!
//! Typographic apostrophes (`’`) are normalised to `'` in [`Token::text`], because
//! that is the form the dictionary's own words use. [`Token::range`] always refers
//! to the **original** string, so a caller can underline exactly what the author
//! typed even though the text it checked differs byte-for-byte.

use std::ops::Range;

/// One checkable word: where it sits in the source, and the form to look up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    /// Byte range in the string passed to [`tokenize`] — suitable for slicing it
    /// or for positioning a squiggle.
    pub range: Range<usize>,
    /// The word to look up: the source slice with typographic apostrophes and
    /// hyphens folded to their ASCII forms.
    pub text: String,
}

/// Characters that may appear *inside* a word without ending it.
fn is_word_joiner(c: char) -> bool {
    matches!(c, '\'' | '\u{2019}' | '-' | '\u{2010}')
}

/// Characters a word run is built from.
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || is_word_joiner(c)
}

/// Fold typographic apostrophes/hyphens to ASCII. The bundled en_US affix file does
/// the same thing with its `ICONV ’ '` rule, but we cannot rely on every dictionary
/// carrying one, and the custom-word lists are compared here rather than in the
/// engine.
pub fn normalize(word: &str) -> String {
    word.chars()
        .map(|c| match c {
            '\u{2019}' => '\'',
            '\u{2010}' => '-',
            other => other,
        })
        .collect()
}

/// True if this whitespace-delimited chunk is an address, path or identifier rather
/// than prose — in which case none of its pieces are words.
fn is_unspellable_chunk(chunk: &str) -> bool {
    if chunk.contains(['@', '/', '\\', '_']) {
        return true;
    }
    // An internal dot flanked by word characters: `example.com`, `e.g`, `a.out`,
    // `self.field`. A sentence-ending dot has whitespace after it, so it is not
    // caught here.
    let bytes: Vec<char> = chunk.chars().collect();
    for (i, c) in bytes.iter().enumerate() {
        if *c == '.' && i > 0 && i + 1 < bytes.len() {
            let before = bytes[i - 1];
            let after = bytes[i + 1];
            if before.is_alphanumeric() && after.is_alphanumeric() {
                return true;
            }
        }
    }
    false
}

/// Every word-shaped run in `text`, **without** the "is this worth checking"
/// filtering [`tokenize`] applies.
///
/// Exposed because entity names from an author's notes want the raw runs: a
/// character called `ERIS` is a name, not an acronym, and dropping it would be the
/// opposite of helpful. Chunk-level skipping (URLs, paths) still applies.
pub fn word_runs(text: &str) -> Vec<Token> {
    let mut out = Vec::new();
    for (chunk_start, chunk) in whitespace_chunks(text) {
        if is_unspellable_chunk(chunk) {
            continue;
        }
        let mut run_start: Option<usize> = None;
        for (i, c) in chunk.char_indices() {
            if is_word_char(c) {
                run_start.get_or_insert(i);
            } else if let Some(s) = run_start.take() {
                push_run(&mut out, chunk, chunk_start, s..i);
            }
        }
        if let Some(s) = run_start {
            push_run(&mut out, chunk, chunk_start, s..chunk.len());
        }
    }
    out
}

/// Trim joiners off both ends of a run and record it if anything is left.
fn push_run(out: &mut Vec<Token>, chunk: &str, chunk_start: usize, run: Range<usize>) {
    let slice = &chunk[run.clone()];
    let trimmed = slice.trim_matches(is_word_joiner);
    if trimmed.is_empty() {
        return;
    }
    // Where the trimmed slice begins inside `slice` — byte offsets, so trimming a
    // multi-byte `’` still lands correctly.
    let lead = slice.len() - slice.trim_start_matches(is_word_joiner).len();
    let start = chunk_start + run.start + lead;
    out.push(Token {
        range: start..start + trimmed.len(),
        text: normalize(trimmed),
    });
}

/// `(byte offset, chunk)` for each whitespace-delimited chunk of `text`.
///
/// Hand-rolled rather than `split_whitespace` because the offsets are the point —
/// a token's range has to survive back into the author's original string.
fn whitespace_chunks(text: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    for (i, c) in text.char_indices() {
        if c.is_whitespace() {
            if let Some(s) = start.take() {
                out.push((s, &text[s..i]));
            }
        } else {
            start.get_or_insert(i);
        }
    }
    if let Some(s) = start {
        out.push((s, &text[s..]));
    }
    out
}

/// True for a run of 2+ letters that are all uppercase — an acronym, not a word.
fn is_acronym(word: &str) -> bool {
    let letters = word.chars().filter(|c| c.is_alphabetic()).count();
    letters >= 2
        && word
            .chars()
            .filter(|c| c.is_alphabetic())
            .all(char::is_uppercase)
}

/// The checkable words in `text`, in order.
pub fn tokenize(text: &str) -> Vec<Token> {
    word_runs(text)
        .into_iter()
        .filter(|t| {
            !t.text.chars().any(|c| c.is_numeric())
                && t.text.chars().any(char::is_alphabetic)
                && !is_acronym(&t.text)
        })
        .collect()
}

/// The pieces of a hyphenated compound, for the "check the whole, then the parts"
/// fallback: `well-known` → `["well", "known"]`. A word with no hyphen yields one
/// piece (itself), so callers can treat the result uniformly.
pub fn hyphen_parts(word: &str) -> Vec<&str> {
    word.split(['-', '\u{2010}'])
        .filter(|p| !p.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(text: &str) -> Vec<String> {
        tokenize(text).into_iter().map(|t| t.text).collect()
    }

    #[test]
    fn plain_prose_splits_into_words() {
        assert_eq!(words("The cat sat down"), ["The", "cat", "sat", "down"]);
    }

    #[test]
    fn internal_apostrophes_and_hyphens_stay() {
        assert_eq!(
            words("don't touch that well-known O'Brien"),
            ["don't", "touch", "that", "well-known", "O'Brien"]
        );
    }

    #[test]
    fn curly_apostrophes_normalize_but_ranges_point_at_the_source() {
        let src = "don’t";
        let toks = tokenize(src);
        assert_eq!(toks.len(), 1);
        assert_eq!(toks[0].text, "don't");
        // The range covers the original bytes, curly apostrophe and all.
        assert_eq!(&src[toks[0].range.clone()], "don’t");
    }

    #[test]
    fn surrounding_quotes_and_punctuation_are_stripped() {
        let src = "“Hello,” said ‘Anna’ — (really).";
        assert_eq!(words(src), ["Hello", "said", "Anna", "really"]);
        let toks = tokenize(src);
        assert_eq!(&src[toks[0].range.clone()], "Hello");
        assert_eq!(&src[toks[2].range.clone()], "Anna");
    }

    #[test]
    fn leading_apostrophe_is_treated_as_a_quote() {
        // `'tis` is checked as `tis`; a leading straight quote is far more often a
        // quotation mark than part of the word.
        assert_eq!(words("'tis so'"), ["tis", "so"]);
    }

    #[test]
    fn digits_disqualify_a_token() {
        assert_eq!(words("h3llo v2 2026 chapter 12b"), ["chapter"]);
    }

    #[test]
    fn all_caps_acronyms_are_skipped_but_single_letters_are_not() {
        assert_eq!(words("NASA and the US POV of A"), ["and", "the", "of", "A"]);
    }

    #[test]
    fn urls_emails_paths_and_identifiers_are_skipped_whole() {
        assert_eq!(words("see https://example.com/a/b now"), ["see", "now"]);
        assert_eq!(words("mail anna@example.com today"), ["mail", "today"]);
        assert_eq!(words("open path/to/file please"), ["open", "please"]);
        assert_eq!(words("the snake_case name"), ["the", "name"]);
        assert_eq!(words("visit www.example.com soon"), ["visit", "soon"]);
        assert_eq!(words("a windows\\path here"), ["a", "here"]);
    }

    #[test]
    fn a_sentence_final_dot_does_not_disqualify_the_word() {
        assert_eq!(
            words("She left. He stayed."),
            ["She", "left", "He", "stayed"]
        );
    }

    #[test]
    fn ranges_are_correct_across_multibyte_text() {
        let src = "café naïve — résumé";
        let toks = tokenize(src);
        assert_eq!(toks.len(), 3);
        for t in &toks {
            assert_eq!(&src[t.range.clone()], t.text.as_str());
        }
        assert_eq!(toks[2].text, "résumé");
    }

    #[test]
    fn em_dashes_separate_words() {
        assert_eq!(words("wait—stop"), ["wait", "stop"]);
        assert_eq!(words("wait–stop"), ["wait", "stop"]);
    }

    #[test]
    fn hyphen_parts_splits_compounds() {
        assert_eq!(hyphen_parts("well-known"), ["well", "known"]);
        assert_eq!(hyphen_parts("mother-in-law"), ["mother", "in", "law"]);
        assert_eq!(hyphen_parts("plain"), ["plain"]);
        // A trailing hyphen leaves no empty piece behind.
        assert_eq!(hyphen_parts("half-"), ["half"]);
    }

    #[test]
    fn word_runs_keeps_what_tokenize_filters() {
        // Entity extraction wants ERIS; the prose checker does not.
        let raw: Vec<String> = word_runs("ERIS met R2")
            .into_iter()
            .map(|t| t.text)
            .collect();
        assert_eq!(raw, ["ERIS", "met", "R2"]);
        assert_eq!(words("ERIS met R2"), ["met"]);
    }

    #[test]
    fn empty_and_punctuation_only_input_yields_nothing() {
        assert!(tokenize("").is_empty());
        assert!(tokenize("  … — ,,, '' ").is_empty());
    }
}
