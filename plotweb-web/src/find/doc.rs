//! Finding (and replacing) [`matcher`](super::matcher) hits inside a rinch
//! document.
//!
//! Two entry points, because a book has two kinds of chapter at any moment:
//!
//! - [`find_in_doc`] searches a live [`Node`] — the open chapter, read straight
//!   off `EditorHandle::doc()`, so what the panel counts is what the author is
//!   looking at.
//! - [`find_in_chapter_content`] searches a *stored* chapter body
//!   ([`plotweb_common::Chapter::content`], which every chapter in
//!   `AppStore::chapters` carries) by building the same document the editor
//!   would have built from it. That is what makes "whole book" possible without
//!   opening forty chapters, and it is legacy-tolerant for exactly the same
//!   reason `editor_utils::load_chapter_content` is.
//!
//! # Char offsets, not byte offsets
//!
//! rinch's [`Pos`] counts **chars**; the matcher works in **bytes** (it slices
//! the source). Every block is walked once with both cursors moving together
//! rather than re-counting per hit, the same trick `spell::plugin::scan` uses.
//!
//! # The flat text of a block
//!
//! A textblock's inline content is flattened into one string whose chars line up
//! one-for-one with the position space: a text node contributes its own text,
//! and any other inline leaf (an image) contributes exactly one char. That char
//! is U+FFFC OBJECT REPLACEMENT CHARACTER rather than a space, so a query
//! containing a space can never match *across* an image and a replace can never
//! swallow one.
//!
//! This is deliberately less fussy than the spellchecker's version of the same
//! walk, in two ways, and both differences are the point of the feature:
//!
//! - **Code blocks are searched.** A spellchecker underlining `fn` is noise; a
//!   find that silently skips a code block is a find that lies about the count.
//! - **Inline code is searched.** Same reason — the spellchecker masks it to
//!   avoid flagging identifiers, but an author renaming something wants the
//!   `code`-marked spelling of it found too.

use std::ops::Range;

use rinch_editor_core::model::Fragment;
use rinch_editor_core::serialize::DocNode;
use rinch_editor_core::{EditorState, Node, Pos, Schema, Transaction};

use super::matcher::{FindOptions, find_in_text};

/// One occurrence of the query in a document, located and quoted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    /// First position of the match.
    pub from: Pos,
    /// Position just past the match.
    pub to: Pos,
    /// The match with up to [`SNIPPET_CONTEXT`] chars of its block either side,
    /// ellipsised where it was cut.
    pub snippet: String,
    /// Where the match itself sits inside `snippet`, as a byte range — so the
    /// results list can embolden it without searching the snippet again (which
    /// would find the wrong occurrence when the context contains one too).
    pub snippet_match: Range<usize>,
}

impl Hit {
    /// The snippet split at the match: `(before, matched, after)`.
    pub fn snippet_parts(&self) -> (&str, &str, &str) {
        (
            &self.snippet[..self.snippet_match.start],
            &self.snippet[self.snippet_match.clone()],
            &self.snippet[self.snippet_match.end..],
        )
    }
}

/// How much of the surrounding sentence a result row quotes on each side.
pub const SNIPPET_CONTEXT: usize = 40;

/// U+FFFC, standing in for a non-text inline leaf so char offsets keep step with
/// the position space without inventing a character a query could match.
const OBJECT: char = '\u{FFFC}';

// ── Searching ────────────────────────────────────────────────────────────────

/// Every hit for `query` in `doc`, in document order.
pub fn find_in_doc(doc: &Node, query: &str, opts: FindOptions) -> Vec<Hit> {
    if query.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for_each_textblock(doc, &mut |block, block_pos| {
        let content_start = block_pos + 1;
        let text = block_flat_text(block);
        let ranges = find_in_text(&text, query, opts);
        if ranges.is_empty() {
            return;
        }
        // One pass over the block converting byte offsets to char offsets: the
        // ranges are sorted and disjoint, so a single moving cursor serves them
        // all (re-counting from the start per hit would be quadratic on a block
        // with many).
        let mut byte = 0usize;
        let mut ch = 0usize;
        for range in ranges {
            let Some(gap) = text.get(byte..range.start) else {
                break;
            };
            ch += gap.chars().count();
            let from = content_start + ch;
            let Some(matched) = text.get(range.clone()) else {
                break;
            };
            ch += matched.chars().count();
            byte = range.end;
            let to = content_start + ch;
            let (snippet, snippet_match) = snippet_for(&text, range);
            out.push(Hit {
                from: Pos(from),
                to: Pos(to),
                snippet,
                snippet_match,
            });
        }
    });
    out
}

/// Every hit for `query` in a **stored** chapter body, or `None` when that body
/// could not be turned into a document at all.
///
/// `None` is the honest answer for content this build cannot read — it is
/// reported in the results as such rather than silently counting zero, because
/// "no matches" and "not searched" are different facts and only one of them
/// means replace-all is safe to trust.
pub fn find_in_chapter_content(content: &str, query: &str, opts: FindOptions) -> Option<Vec<Hit>> {
    Some(find_in_doc(&chapter_doc(content)?, query, opts))
}

/// Build the document the chapter editor would build from stored `content`.
///
/// Mirrors `editor_utils::load_chapter_content` step for step — DocNode JSON if
/// it parses, else the legacy Markdown → HTML path — and for the same reason it
/// has to: the hit a result row points at is found again, in the live editor,
/// once the chapter is actually opened, and the two searches only agree if they
/// read the same document.
pub fn chapter_doc(content: &str) -> Option<Node> {
    let schema = Schema::starter_kit();
    let trimmed = content.trim_start();
    if trimmed.starts_with('{')
        && let Ok(docnode) = serde_json::from_str::<DocNode>(trimmed)
        && let Ok(node) = schema.node_from_doc(&docnode)
    {
        return Some(node);
    }
    // Legacy: chapters were stored as Markdown. This is `EditorHandle::load_html`
    // spelled out — parse to a slice, wrap its content in a `doc` — so the
    // offline document matches the one a load would produce.
    let html = plotweb_common::markdown_to_html(content);
    let slice = rinch_editor_core::serialize::slice_from_html(&schema, &html).ok()?;
    schema.branch("doc", slice.content).ok()
}

/// The match with its surroundings, and where the match sits in the result.
fn snippet_for(text: &str, range: Range<usize>) -> (String, Range<usize>) {
    let before_all = &text[..range.start];
    let after_all = &text[range.end..];

    let before_chars = before_all.chars().count();
    let skip = before_chars.saturating_sub(SNIPPET_CONTEXT);
    let before: String = before_all.chars().skip(skip).collect();
    let after: String = after_all.chars().take(SNIPPET_CONTEXT).collect();

    let mut snippet = String::new();
    if skip > 0 {
        snippet.push('…');
    }
    snippet.push_str(&before);
    let start = snippet.len();
    snippet.push_str(&text[range]);
    let end = snippet.len();
    snippet.push_str(&after);
    if after.chars().count() < after_all.chars().count() {
        snippet.push('…');
    }
    // A block is one paragraph, but a hard break inside it would put a newline
    // in the middle of a one-line result row.
    let flattened: String = snippet
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect();
    debug_assert_eq!(flattened.len(), snippet.len(), "1:1 char substitution");
    (flattened, start..end)
}

/// Call `f(block, pos)` for every textblock in `doc`, where `pos` is the
/// position **before** the block (so its inline content starts at `pos + 1`).
fn for_each_textblock(doc: &Node, f: &mut impl FnMut(&Node, usize)) {
    doc.nodes_between(0, doc.content_size(), &mut |node, pos, _parent| {
        if !node.is_textblock() {
            // Not a textblock: descend, unless it is a leaf with nothing inside.
            return !node.is_leaf();
        }
        f(node, pos);
        // Inline content is read through `block_flat_text`, not walked here.
        false
    });
}

/// A textblock's inline content as one flat string whose chars line up with the
/// position space. See the module header for why the mask is U+FFFC.
fn block_flat_text(block: &Node) -> String {
    let mut out = String::new();
    for i in 0..block.child_count() {
        let child = block.child(i);
        match child.text() {
            Some(text) => out.push_str(text),
            None => out.push(OBJECT),
        }
    }
    out
}

// ── Replacing ────────────────────────────────────────────────────────────────

/// A transaction replacing `from..to` with `replacement`, keeping the marks the
/// matched text was wearing.
///
/// The marks are read off a position strictly *inside* the match, so a boundary
/// between an unmarked and a marked run is never read from the wrong side — the
/// same rule (and the same reason) as `spell::plugin::replace_word`: replacing
/// a word inside an italic line with plain text would leave the repair upright.
///
/// An empty `replacement` is a deletion, which is a perfectly ordinary thing to
/// ask a find-and-replace for and which `text_with_marks` refuses (an empty text
/// node is not a node), so it takes the `delete` path instead.
fn push_replace(
    tr: &mut Transaction,
    doc: &Node,
    schema: &Schema,
    from: Pos,
    to: Pos,
    replacement: &str,
) -> Option<()> {
    if replacement.is_empty() {
        tr.delete(from.0, to.0).ok()?;
        return Some(());
    }
    let marks = doc
        .resolve(Pos(from.0 + 1))
        .ok()
        .map(|r| r.marks())
        .unwrap_or_default();
    let node = schema.text_with_marks(replacement, marks).ok()?;
    tr.replace_with(from.0, to.0, Fragment::from_node(node))
        .ok()?;
    Some(())
}

/// One transaction replacing **every** hit for `query` in `state`'s document,
/// or `None` when there is nothing to replace.
///
/// One transaction, not one per hit, because the history plugin records one undo
/// event per transaction (a multi-step transaction inverts as a unit): a
/// replace-all the author regrets is a single Ctrl+Z, not one per occurrence.
///
/// The hits are applied **back to front** so that each step's positions are
/// still the ones the search reported — an earlier replacement of a different
/// length shifts everything after it, and going backwards means there is nothing
/// after it left to shift.
pub fn replace_all_transaction(
    state: &EditorState,
    query: &str,
    opts: FindOptions,
    replacement: &str,
) -> Option<Transaction> {
    let hits = find_in_doc(&state.doc, query, opts);
    if hits.is_empty() {
        return None;
    }
    let schema = state.schema();
    let mut tr = state.tr();
    for hit in hits.iter().rev() {
        push_replace(&mut tr, &state.doc, schema, hit.from, hit.to, replacement)?;
    }
    Some(tr)
}

/// A transaction replacing the single range `from..to`, leaving the caret just
/// past the replacement — where retyping the word by hand would have left it.
pub fn replace_one_transaction(
    state: &EditorState,
    from: Pos,
    to: Pos,
    replacement: &str,
) -> Option<Transaction> {
    if from >= to {
        return None;
    }
    let schema = state.schema();
    let mut tr = state.tr();
    push_replace(&mut tr, &state.doc, schema, from, to, replacement)?;
    tr.set_selection(rinch_editor_core::Selection::cursor(Pos(
        from.0 + replacement.chars().count()
    )));
    Some(tr)
}

/// Replace every hit in `handle`'s document in one undo step. Returns how many
/// were replaced.
pub fn replace_all(
    handle: &crate::rinch_backend::EditorHandle,
    query: &str,
    opts: FindOptions,
    replacement: &str,
) -> usize {
    let count = find_in_doc(&handle.doc(), query, opts).len();
    if count == 0 {
        return 0;
    }
    let replacement = replacement.to_string();
    let applied = handle.update(|state| replace_all_transaction(state, query, opts, &replacement));
    if applied { count } else { 0 }
}

/// Replace one located hit in `handle`'s document. Returns whether it applied.
pub fn replace_one(
    handle: &crate::rinch_backend::EditorHandle,
    from: Pos,
    to: Pos,
    replacement: &str,
) -> bool {
    let replacement = replacement.to_string();
    handle.update(|state| replace_one_transaction(state, from, to, &replacement))
}

/// The plain text of a whole document, blocks joined by newlines — what the
/// tests read a result back as, and nothing else.
#[cfg(test)]
pub(crate) fn doc_text(doc: &Node) -> String {
    let mut blocks = Vec::new();
    for_each_textblock(doc, &mut |block, _| blocks.push(block_flat_text(block)));
    blocks.join("\n")
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use super::*;
    use rinch_editor_core::{Mark, Selection};

    fn plain() -> FindOptions {
        FindOptions::default()
    }

    /// A doc of paragraphs, each a single unmarked text node — the same fixture
    /// shape the spellcheck plugin's tests use.
    fn doc_of(schema: &Schema, paragraphs: &[&str]) -> Node {
        let blocks: Vec<Node> = paragraphs
            .iter()
            .map(|p| {
                let content = if p.is_empty() {
                    Fragment::empty()
                } else {
                    Fragment::from_node(schema.text(p).unwrap())
                };
                schema.branch("paragraph", content).unwrap()
            })
            .collect();
        schema
            .branch("doc", Fragment::from_children(blocks))
            .unwrap()
    }

    fn state_of(doc: Node) -> EditorState {
        EditorState::create(Rc::new(Schema::starter_kit()), doc, Vec::new())
    }

    fn ranges(hits: &[Hit]) -> Vec<(usize, usize)> {
        hits.iter().map(|h| (h.from.0, h.to.0)).collect()
    }

    #[test]
    fn a_hit_lands_on_exactly_the_matched_text() {
        let schema = Schema::starter_kit();
        // The paragraph opens at 0, so its text starts at 1: "She rode" puts
        // `rode` at chars 4..8 of the block → positions 5..9.
        let doc = doc_of(&schema, &["She rode out"]);
        let hits = find_in_doc(&doc, "rode", plain());
        assert_eq!(ranges(&hits), vec![(5, 9)]);
        let slice = doc.slice(5, 9).unwrap();
        assert_eq!(slice.content.child(0).text(), Some("rode"));
    }

    #[test]
    fn a_later_block_is_offset_by_the_earlier_blocks_sizes() {
        let schema = Schema::starter_kit();
        // "hello" is 5 chars, so its paragraph spans 0..7 and the second
        // paragraph's text starts at 8.
        let doc = doc_of(&schema, &["hello", "rode"]);
        assert_eq!(ranges(&find_in_doc(&doc, "rode", plain())), vec![(8, 12)]);
    }

    #[test]
    fn positions_are_char_offsets_not_byte_offsets() {
        let schema = Schema::starter_kit();
        // Every char before the match is two bytes wide; a byte-offset answer
        // would be twice as far along.
        let doc = doc_of(&schema, &["мама мыла раму"]);
        let hits = find_in_doc(&doc, "мыла", plain());
        assert_eq!(ranges(&hits), vec![(6, 10)]);
        assert_eq!(
            doc.slice(6, 10).unwrap().content.child(0).text(),
            Some("мыла")
        );
    }

    #[test]
    fn marks_splitting_a_word_still_yield_one_hit_at_the_right_place() {
        // `rode` written as three text nodes — `ro` + bold `d` + `e` — which is
        // what bolding a letter mid-word leaves behind. The flat text is what
        // gets searched, so it is still one hit.
        let schema = Schema::starter_kit();
        let bold = Mark::simple(schema.mark_type("bold").unwrap().clone());
        let para = schema
            .branch(
                "paragraph",
                Fragment::from_children(vec![
                    schema.text("She ro").unwrap(),
                    schema.text_with_marks("d", vec![bold]).unwrap(),
                    schema.text("e out").unwrap(),
                ]),
            )
            .unwrap();
        let doc = schema.branch("doc", Fragment::from_node(para)).unwrap();
        assert_eq!(ranges(&find_in_doc(&doc, "rode", plain())), vec![(5, 9)]);
    }

    #[test]
    fn an_image_masks_one_char_and_does_not_shift_what_follows() {
        let schema = Schema::starter_kit();
        let image = schema
            .create_node(
                "image",
                rinch_editor_core::Attrs::new().with("src", "x.png"),
                Fragment::empty(),
            )
            .unwrap();
        let para = schema
            .branch(
                "paragraph",
                Fragment::from_children(vec![
                    schema.text("a ").unwrap(),
                    image,
                    schema.text(" rode").unwrap(),
                ]),
            )
            .unwrap();
        let doc = schema.branch("doc", Fragment::from_node(para)).unwrap();
        // "a " (2) + image (1) + " " (1) = 4 chars, +1 for the block open.
        assert_eq!(ranges(&find_in_doc(&doc, "rode", plain())), vec![(5, 9)]);
        // And a query spanning the image finds nothing, because the mask is not
        // a character anybody types.
        assert!(find_in_doc(&doc, "a  rode", plain()).is_empty());
    }

    #[test]
    fn a_code_block_is_searched_unlike_the_spellcheckers_walk() {
        let schema = Schema::starter_kit();
        let code = schema
            .branch(
                "code_block",
                Fragment::from_node(schema.text("let rode = 1;").unwrap()),
            )
            .unwrap();
        let doc = schema.branch("doc", Fragment::from_node(code)).unwrap();
        assert_eq!(ranges(&find_in_doc(&doc, "rode", plain())), vec![(5, 9)]);
    }

    #[test]
    fn an_empty_query_finds_nothing() {
        let schema = Schema::starter_kit();
        let doc = doc_of(&schema, &["She rode out"]);
        assert!(find_in_doc(&doc, "", plain()).is_empty());
    }

    #[test]
    fn a_snippet_quotes_the_context_and_says_where_the_match_is() {
        let schema = Schema::starter_kit();
        let doc = doc_of(&schema, &["She rode out at dawn"]);
        let hits = find_in_doc(&doc, "rode", plain());
        let (before, matched, after) = hits[0].snippet_parts();
        assert_eq!(before, "She ");
        assert_eq!(matched, "rode");
        assert_eq!(after, " out at dawn");
        // Short enough that nothing was cut, so no ellipses.
        assert_eq!(hits[0].snippet, "She rode out at dawn");
    }

    #[test]
    fn a_long_block_is_ellipsised_on_both_sides() {
        let schema = Schema::starter_kit();
        let filler = "x".repeat(200);
        let doc = doc_of(&schema, &[&format!("{filler} rode {filler}")]);
        let hits = find_in_doc(&doc, "rode", plain());
        assert_eq!(hits.len(), 1);
        let snippet = &hits[0].snippet;
        assert!(snippet.starts_with('…'), "{snippet}");
        assert!(snippet.ends_with('…'), "{snippet}");
        let (before, matched, after) = hits[0].snippet_parts();
        assert_eq!(matched, "rode");
        // `…` + 40 chars either side.
        assert_eq!(before.chars().count(), SNIPPET_CONTEXT + 1);
        assert_eq!(after.chars().count(), SNIPPET_CONTEXT + 1);
    }

    #[test]
    fn the_snippet_match_is_the_hit_not_another_occurrence_in_the_context() {
        let schema = Schema::starter_kit();
        let doc = doc_of(&schema, &["rode and rode again"]);
        let hits = find_in_doc(&doc, "rode", plain());
        assert_eq!(hits.len(), 2);
        // Both snippets quote the whole (short) block, so a naive "find the
        // query in the snippet" would embolden the first one twice.
        assert_eq!(hits[0].snippet_match, 0..4);
        assert_eq!(hits[1].snippet_match, 9..13);
    }

    // ── Stored content ───────────────────────────────────────────────

    #[test]
    fn stored_docnode_json_is_searched_as_the_editor_would_load_it() {
        let schema = Schema::starter_kit();
        let doc = doc_of(&schema, &["She rode out", "and rode back"]);
        let json = serde_json::to_string(&doc.to_doc().unwrap()).unwrap();
        let hits = find_in_chapter_content(&json, "rode", plain()).expect("readable");
        // Identical to searching the live document — which is the property the
        // click-through to an unopened chapter depends on.
        assert_eq!(ranges(&hits), ranges(&find_in_doc(&doc, "rode", plain())));
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn legacy_markdown_content_is_searched_through_the_same_path_a_load_takes() {
        let content = "She rode out.\n\nAnd rode back.";
        let hits = find_in_chapter_content(content, "rode", plain()).expect("readable");
        assert_eq!(hits.len(), 2);
        // Two paragraphs: the first opens at 0, so `rode` sits at 5..9; the
        // first paragraph is 13 chars of text + 2 → the second starts at 15+1.
        let doc = chapter_doc(content).unwrap();
        assert_eq!(ranges(&hits), ranges(&find_in_doc(&doc, "rode", plain())));
        assert_eq!(hits[0].from, Pos(5));
    }

    #[test]
    fn an_empty_chapter_is_readable_and_simply_has_no_hits() {
        let hits = find_in_chapter_content("", "rode", plain()).expect("readable");
        assert!(hits.is_empty());
    }

    // ── Replacing ────────────────────────────────────────────────────

    #[test]
    fn replace_all_rewrites_every_hit_in_one_transaction() {
        let schema = Schema::starter_kit();
        let state = state_of(doc_of(&schema, &["She rode out", "and rode back"]));
        let tr = replace_all_transaction(&state, "rode", plain(), "walked").expect("hits");
        // One transaction, one step per hit — which is what makes it one undo.
        assert_eq!(tr.steps().len(), 2);
        let next = state.apply(tr);
        assert_eq!(doc_text(&next.doc), "She walked out\nand walked back");
        assert!(find_in_doc(&next.doc, "rode", plain()).is_empty());
    }

    #[test]
    fn replacing_with_a_longer_string_does_not_derail_the_later_hits() {
        // The back-to-front rule. Three hits in one block, each replacement
        // longer than what it replaces: applied front-to-back with the search's
        // own positions, the second and third would land off by 2 and 4.
        let schema = Schema::starter_kit();
        let state = state_of(doc_of(&schema, &["a cat, a cat, a cat"]));
        let tr = replace_all_transaction(&state, "cat", plain(), "kitten").expect("hits");
        let next = state.apply(tr);
        assert_eq!(doc_text(&next.doc), "a kitten, a kitten, a kitten");
    }

    #[test]
    fn replacing_with_a_shorter_string_works_the_same_way() {
        let schema = Schema::starter_kit();
        let state = state_of(doc_of(&schema, &["kitten kitten kitten"]));
        let tr = replace_all_transaction(&state, "kitten", plain(), "cat").expect("hits");
        let next = state.apply(tr);
        assert_eq!(doc_text(&next.doc), "cat cat cat");
    }

    #[test]
    fn an_empty_replacement_deletes_the_hits() {
        let schema = Schema::starter_kit();
        let state = state_of(doc_of(&schema, &["she very much rode"]));
        let tr = replace_all_transaction(&state, "very much ", plain(), "").expect("hits");
        let next = state.apply(tr);
        assert_eq!(doc_text(&next.doc), "she rode");
    }

    #[test]
    fn a_replacement_keeps_the_marks_the_match_was_wearing() {
        let schema = Schema::starter_kit();
        let italic = Mark::simple(schema.mark_type("italic").unwrap().clone());
        let para = schema
            .branch(
                "paragraph",
                Fragment::from_children(vec![
                    schema.text("She ").unwrap(),
                    schema
                        .text_with_marks("rode", vec![italic.clone()])
                        .unwrap(),
                    schema.text(" out").unwrap(),
                ]),
            )
            .unwrap();
        let doc = schema.branch("doc", Fragment::from_node(para)).unwrap();
        let state = state_of(doc);
        let tr = replace_all_transaction(&state, "rode", plain(), "walked").expect("hits");
        let next = state.apply(tr);
        assert_eq!(doc_text(&next.doc), "She walked out");
        // The repair is still italic — a plain-text replacement would have left
        // it upright in the middle of an italic phrase.
        let resolved = next.doc.resolve(Pos(6)).unwrap();
        assert!(
            resolved.marks().iter().any(|m| m.type_name() == "italic"),
            "the replacement kept the italic run's marks"
        );
    }

    #[test]
    fn replace_all_declines_when_there_is_nothing_to_replace() {
        let schema = Schema::starter_kit();
        let state = state_of(doc_of(&schema, &["She rode out"]));
        assert!(replace_all_transaction(&state, "walked", plain(), "x").is_none());
        assert!(replace_all_transaction(&state, "", plain(), "x").is_none());
    }

    #[test]
    fn replace_all_honours_match_case_and_whole_word() {
        let schema = Schema::starter_kit();
        let state = state_of(doc_of(&schema, &["Cat cat catalogue"]));

        let cased = FindOptions {
            match_case: true,
            whole_word: false,
        };
        let next = state.apply(replace_all_transaction(&state, "cat", cased, "dog").unwrap());
        assert_eq!(doc_text(&next.doc), "Cat dog dogalogue");

        let word = FindOptions {
            match_case: false,
            whole_word: true,
        };
        let next = state.apply(replace_all_transaction(&state, "cat", word, "dog").unwrap());
        assert_eq!(doc_text(&next.doc), "dog dog catalogue");
    }

    #[test]
    fn replace_one_rewrites_a_single_hit_and_leaves_the_caret_past_it() {
        let schema = Schema::starter_kit();
        let state = state_of(doc_of(&schema, &["rode and rode"]));
        let hits = find_in_doc(&state.doc, "rode", plain());
        let second = &hits[1];
        let tr = replace_one_transaction(&state, second.from, second.to, "walked").unwrap();
        let next = state.apply(tr);
        assert_eq!(doc_text(&next.doc), "rode and walked");
        assert_eq!(
            next.selection,
            Selection::cursor(Pos(second.from.0 + "walked".chars().count()))
        );
    }
}
