//! The link index — what a note body points at.
//!
//! Three sigils, kept deliberately distinct (see `design/04-notes-wireframes.html`):
//!
//! | sigil | edge | what it means |
//! |---|---|---|
//! | `#` | tag | a label, for filtering |
//! | `@` | mention | a soft link — "this note talks about that one" |
//! | `$` | reference | **participation** — the only edge that draws a thread through an event |
//!
//! # Derived, never authored
//!
//! A [`NoteLinks`] is a *cache of what the body says*, recomputed from the body on
//! every write. Nothing may edit it independently: if the index and the body ever
//! disagree, the body is right. That is why it is safe to mirror into the `book:`
//! structure document as a whole value — it can always be rebuilt.
//!
//! Mirroring it is the load-bearing reason this module exists. The timeline needs to
//! know who appears in which event to draw a single tie, and the alternative to holding
//! that in the structure document is opening every `note:{id}` Automerge document on
//! every render.
//!
//! # What this card extracts
//!
//! Sigil *text*, scanned out of the body's plain text. The editor-side autocomplete that
//! writes a real link is card 2 of the notes revamp, so today a body with no sigils
//! simply yields no edges, and a body someone typed `$Vess` into yields the token
//! `Vess`. Resolving tokens to note ids arrives with the thing that writes them.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

/// The edges one note's body carries. Order is first appearance in the body;
/// repeats are collapsed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteLinks {
    /// `#tag` — labels, for filtering.
    #[serde(default)]
    pub tags: Vec<String>,
    /// `@mention` — soft links. Quiet in the timeline; shown in the editor's context
    /// rail.
    #[serde(default)]
    pub mentions: Vec<String>,
    /// `$ref` — participation. These are what put an entity's lane in an event's tie.
    #[serde(default)]
    pub refs: Vec<String>,
}

impl NoteLinks {
    /// Whether this note points at nothing at all — the state every existing note
    /// migrates into, and the state a body with no sigils stays in.
    pub fn is_empty(&self) -> bool {
        self.tags.is_empty() && self.mentions.is_empty() && self.refs.is_empty()
    }
}

/// Pull the link index out of a stored note body.
///
/// `content` is the raw stored string, which is one of two things and must not be
/// assumed to be either: the editor's durable `DocNode` JSON, or — for a note last
/// saved before that shape existed — raw HTML. Both are reduced to plain text first
/// (see [`note_plain_text`]), so the scan never has to care.
pub fn extract_note_links(content: &str) -> NoteLinks {
    scan_sigils(&note_plain_text(content))
}

/// The plain text of a stored note body, for either storage shape.
///
/// Blocks are joined with newlines so a sigil at the end of one paragraph cannot run
/// into the word starting the next.
pub fn note_plain_text(content: &str) -> String {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.starts_with('{') {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
            let mut out = String::new();
            collect_doc_text(&value, &mut out);
            return out;
        }
    }
    strip_html(trimmed)
}

/// Walk a `DocNode` tree collecting `text` leaves. Deliberately structural rather than
/// typed: this runs on the client too, and pulling the editor's schema crate into the
/// shared types crate to read one field would be a heavy dependency for no more
/// accuracy.
fn collect_doc_text(value: &serde_json::Value, out: &mut String) {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(serde_json::Value::String(text)) = map.get("text") {
                out.push_str(text);
            }
            if let Some(content) = map.get("content") {
                collect_doc_text(content, out);
            }
            // A block ends here; the newline keeps adjacent blocks' words apart.
            if map.get("content").is_some() {
                out.push('\n');
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_doc_text(item, out);
            }
        }
        _ => {}
    }
}

/// Drop tags from legacy raw-HTML bodies, turning each one into a break so words either
/// side of it stay separate. Entities are left as written — a sigil scan does not care,
/// and decoding them here would be a second, divergent copy of the editor's load.
fn strip_html(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => {
                in_tag = true;
                out.push('\n');
            }
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}

/// Whether a sigil at this position starts a token, as opposed to sitting inside a
/// word. `a@b` is an email, not a mention; `#ff0000` in prose is not a tag someone
/// meant, but `color:#ff0000` is not a word boundary either way — the rule is simply
/// that the character before must not be alphanumeric.
fn starts_a_token(previous: Option<char>) -> bool {
    !matches!(previous, Some(c) if c.is_alphanumeric())
}

/// Characters a sigil token may contain. Letters, digits, `_`, `-` and `'` — enough for
/// `#slow-burn`, `@Vess`, `$Ka'ren`, and nothing that would swallow the punctuation
/// ending the sentence.
fn token_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '-' || c == '\''
}

fn scan_sigils(text: &str) -> NoteLinks {
    let mut links = NoteLinks::default();
    let mut seen: HashSet<(char, String)> = HashSet::new();

    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let sigil = chars[i];
        if !matches!(sigil, '#' | '@' | '$') {
            i += 1;
            continue;
        }
        if !starts_a_token(i.checked_sub(1).map(|p| chars[p])) {
            i += 1;
            continue;
        }
        let start = i + 1;
        let mut end = start;
        while end < chars.len() && token_char(chars[end]) {
            end += 1;
        }
        if end == start {
            i += 1;
            continue;
        }
        // A trailing `-` or `'` is the sentence's punctuation, not part of the name.
        let mut token: String = chars[start..end].iter().collect();
        while token.ends_with('-') || token.ends_with('\'') {
            token.pop();
        }
        if !token.is_empty() && seen.insert((sigil, token.clone())) {
            match sigil {
                '#' => links.tags.push(token),
                '@' => links.mentions.push(token),
                _ => links.refs.push(token),
            }
        }
        i = end;
    }
    links
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(text: &str) -> String {
        serde_json::json!({
            "type": "doc",
            "content": [{
                "type": "paragraph",
                "content": [{ "type": "text", "text": text }]
            }]
        })
        .to_string()
    }

    #[test]
    fn a_body_with_no_sigils_yields_no_edges() {
        let links = extract_note_links(&doc("The siege lasted three weeks."));
        assert!(links.is_empty());
    }

    #[test]
    fn an_empty_body_yields_no_edges() {
        assert!(extract_note_links("").is_empty());
        assert!(extract_note_links("   ").is_empty());
    }

    #[test]
    fn each_sigil_lands_in_its_own_bucket() {
        let links = extract_note_links(&doc("$Vess met @Karel at the gate. #siege #slow-burn"));
        assert_eq!(links.refs, vec!["Vess"]);
        assert_eq!(links.mentions, vec!["Karel"]);
        assert_eq!(links.tags, vec!["siege", "slow-burn"]);
    }

    #[test]
    fn repeats_collapse_and_order_is_first_appearance() {
        let links = extract_note_links(&doc("$Vess $Karel $Vess"));
        assert_eq!(links.refs, vec!["Vess", "Karel"]);
    }

    #[test]
    fn the_same_token_under_two_sigils_is_two_edges() {
        // `@Vess` is "this note talks about Vess"; `$Vess` is "Vess is in this".
        // Collapsing them would put a character in a scene they were only named in.
        let links = extract_note_links(&doc("@Vess and $Vess"));
        assert_eq!(links.mentions, vec!["Vess"]);
        assert_eq!(links.refs, vec!["Vess"]);
    }

    #[test]
    fn a_sigil_inside_a_word_is_not_an_edge() {
        let links = extract_note_links(&doc("write to karel@example.com about pay$day"));
        assert!(links.is_empty(), "got {links:?}");
    }

    #[test]
    fn trailing_punctuation_is_not_part_of_the_name() {
        let links = extract_note_links(&doc("Then $Vess, and #siege."));
        assert_eq!(links.refs, vec!["Vess"]);
        assert_eq!(links.tags, vec!["siege"]);
    }

    #[test]
    fn blocks_do_not_run_into_each_other() {
        let body = serde_json::json!({
            "type": "doc",
            "content": [
                { "type": "paragraph", "content": [{ "type": "text", "text": "the parley" }] },
                { "type": "paragraph", "content": [{ "type": "text", "text": "$Vess" }] }
            ]
        })
        .to_string();
        assert_eq!(extract_note_links(&body).refs, vec!["Vess"]);
    }

    #[test]
    fn a_legacy_html_body_is_scanned_too() {
        // Notes predating the DocNode save shape are stored as raw HTML, and they are
        // the ones that will carry the first hand-typed sigils.
        let links = extract_note_links("<p>The siege of <b>$Vess</b></p><p>#war</p>");
        assert_eq!(links.refs, vec!["Vess"]);
        assert_eq!(links.tags, vec!["war"]);
    }

    #[test]
    fn html_tags_do_not_glue_words_together() {
        let links = extract_note_links("<p>pay<b>$day</b></p>");
        assert_eq!(
            links.refs,
            vec!["day"],
            "a tag boundary is a word boundary, so the sigil does start a token here"
        );
    }
}
