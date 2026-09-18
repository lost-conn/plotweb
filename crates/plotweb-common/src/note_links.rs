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
//! # What this extracts
//!
//! Sigil *text*, scanned out of the body's plain text, and — given a [`LinkIndex`] —
//! what each token points at. A `@` may name a **note or a chapter**, so the edge
//! carries a [`LinkTarget`] rather than leaving the kind to be worked out later by
//! whoever reads it: the kind is decided once, on write, beside the id.
//!
//! Resolution is by *title*, folded ([`fold_token`]) so that the token the editor's
//! autocomplete writes (`@Three-Doors`) and the title it came from (`Three Doors`)
//! compare equal. A token that matches nothing still yields an edge — the author may
//! be naming a note they have not written yet — with no id and the default kind.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Deserializer, Serialize};

/// What a sigil edge points at.
///
/// Stored **on the edge** rather than worked out when the edge is read: `@` reaches
/// notes and chapters alike, and a reader that had to guess would guess differently
/// once a title moved between the two. `Note` is the default so an unresolved token —
/// and every edge written before chapters were reachable — keeps its old meaning.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkTarget {
    #[default]
    Note,
    Chapter,
}

impl LinkTarget {
    fn is_note(&self) -> bool {
        matches!(self, LinkTarget::Note)
    }
}

/// One `@` or `$` edge: the token as the author wrote it, what kind of thing it names,
/// and — when it resolved — the id of that thing.
///
/// Serialises to a bare JSON string when it is an unresolved note edge, which is
/// exactly the shape edges had before chapters were reachable. That keeps a stored
/// index readable by both, and is why [`NoteLink`]'s deserialiser accepts a string.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NoteLink {
    /// The token as written in the body, sigil stripped (`Three-Doors`).
    pub text: String,
    /// What it names. See [`LinkTarget`].
    pub target: LinkTarget,
    /// The id it resolved to, or `None` for a token naming nothing that exists yet.
    pub id: Option<String>,
}

impl Serialize for NoteLink {
    /// A bare string for an unresolved note edge, an object otherwise.
    ///
    /// The string form is byte-identical to what this index held before chapters were
    /// reachable, which keeps an upgrade from rewriting the stored index of every note
    /// that never named anything — the structure document's `put_if_changed` would
    /// otherwise see a change on every note in every book at once.
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if self.target.is_note() && self.id.is_none() {
            return s.serialize_str(&self.text);
        }
        use serde::ser::SerializeStruct;
        let mut st = s.serialize_struct("NoteLink", 3)?;
        st.serialize_field("text", &self.text)?;
        st.serialize_field("target", &self.target)?;
        match &self.id {
            Some(id) => st.serialize_field("id", id)?,
            None => st.skip_field("id")?,
        }
        st.end()
    }
}

impl NoteLink {
    /// An unresolved edge — a token the author typed that matched nothing.
    pub fn unresolved(text: impl Into<String>) -> Self {
        NoteLink {
            text: text.into(),
            target: LinkTarget::Note,
            id: None,
        }
    }

    /// Whether this edge names `id` as a thing of `target`'s kind.
    pub fn points_at(&self, target: LinkTarget, id: &str) -> bool {
        self.target == target && self.id.as_deref() == Some(id)
    }
}

impl<'de> Deserialize<'de> for NoteLink {
    /// Accepts either the object form or a bare string.
    ///
    /// The string form is what an index written before this existed holds, and what
    /// [`Serialize`] still produces for an unresolved note edge. Refusing it would
    /// make every note saved under the earlier shape read as having no edges at all.
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Shape {
            Text(String),
            Full {
                text: String,
                #[serde(default)]
                target: LinkTarget,
                #[serde(default)]
                id: Option<String>,
            },
        }
        Ok(match Shape::deserialize(d)? {
            Shape::Text(text) => NoteLink::unresolved(text),
            Shape::Full { text, target, id } => NoteLink { text, target, id },
        })
    }
}

/// The edges one note's body carries. Order is first appearance in the body;
/// repeats are collapsed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteLinks {
    /// `#tag` — labels, for filtering. A tag names nothing, so it stays a plain string.
    #[serde(default)]
    pub tags: Vec<String>,
    /// `@mention` — soft links, to a note **or a chapter**. Quiet in the timeline;
    /// shown in the editor's context rail.
    #[serde(default)]
    pub mentions: Vec<NoteLink>,
    /// `$ref` — participation. These are what put an entity's lane in an event's tie.
    #[serde(default)]
    pub refs: Vec<NoteLink>,
}

impl NoteLinks {
    /// Whether this note points at nothing at all — the state every existing note
    /// migrates into, and the state a body with no sigils stays in.
    pub fn is_empty(&self) -> bool {
        self.tags.is_empty() && self.mentions.is_empty() && self.refs.is_empty()
    }
}

/// Titles a sigil token can resolve against, folded once so lookups are cheap.
///
/// Notes are added before chapters and win a title clash: in a note's own body the
/// nearer meaning of a bare name is the other note, and a chapter that happens to
/// share the title is still reachable by renaming either.
#[derive(Debug, Clone, Default)]
pub struct LinkIndex {
    by_title: HashMap<String, (LinkTarget, String)>,
}

impl LinkIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add every `(id, title)` as a note. First title wins, so a duplicate title
    /// resolves to the note that was listed first rather than flickering between them.
    pub fn with_notes<I, S, T>(mut self, notes: I) -> Self
    where
        I: IntoIterator<Item = (S, T)>,
        S: AsRef<str>,
        T: AsRef<str>,
    {
        for (id, title) in notes {
            self.insert(LinkTarget::Note, id.as_ref(), title.as_ref());
        }
        self
    }

    /// Add every `(id, title)` as a chapter. A title already claimed by a note stays
    /// with the note.
    pub fn with_chapters<I, S, T>(mut self, chapters: I) -> Self
    where
        I: IntoIterator<Item = (S, T)>,
        S: AsRef<str>,
        T: AsRef<str>,
    {
        for (id, title) in chapters {
            self.insert(LinkTarget::Chapter, id.as_ref(), title.as_ref());
        }
        self
    }

    fn insert(&mut self, target: LinkTarget, id: &str, title: &str) {
        let key = fold_token(title);
        if key.is_empty() {
            return;
        }
        self.by_title
            .entry(key)
            .or_insert_with(|| (target, id.to_string()));
    }

    /// What `token` names, if anything.
    pub fn resolve(&self, token: &str) -> Option<(LinkTarget, String)> {
        self.by_title.get(&fold_token(token)).cloned()
    }

    fn edge(&self, token: &str) -> NoteLink {
        match self.resolve(token) {
            Some((target, id)) => NoteLink {
                text: token.to_string(),
                target,
                id: Some(id),
            },
            None => NoteLink::unresolved(token),
        }
    }
}

/// The comparison form of a title or a sigil token: lowercase, with every run of
/// non-alphanumeric characters collapsed to a single space.
///
/// This is what lets `@Three-Doors` find "Three Doors", and `$Ka'ren` find "Ka'ren" —
/// a token cannot contain a space (it would swallow the rest of the sentence), so the
/// autocomplete writes separators where the spaces were and both sides fold back to
/// the same key.
pub fn fold_token(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut pending_gap = false;
    for ch in s.chars() {
        if ch.is_alphanumeric() {
            if pending_gap && !out.is_empty() {
                out.push(' ');
            }
            pending_gap = false;
            out.extend(ch.to_lowercase());
        } else {
            pending_gap = true;
        }
    }
    out
}

/// The token the autocomplete writes into a body for `title`, sigil excluded.
///
/// Every character a token may not hold becomes `-`, so the result folds back to the
/// title it came from. Leading and trailing separators are dropped so the token ends
/// on a word and the sentence's punctuation stays outside it.
pub fn token_for_title(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut pending_gap = false;
    for ch in title.chars() {
        if token_char(ch) && ch != '-' {
            if pending_gap && !out.is_empty() {
                out.push('-');
            }
            pending_gap = false;
            out.push(ch);
        } else {
            pending_gap = true;
        }
    }
    out
}

/// Pull the link index out of a stored note body.
///
/// `content` is the raw stored string, which is one of two things and must not be
/// assumed to be either: the editor's durable `DocNode` JSON, or — for a note last
/// saved before that shape existed — raw HTML. Both are reduced to plain text first
/// (see [`note_plain_text`]), so the scan never has to care.
pub fn extract_note_links(content: &str) -> NoteLinks {
    extract_note_links_in(content, &LinkIndex::default())
}

/// The same scan, resolving each `@` and `$` token against `index`.
///
/// This is the one parser: there is no second pass that reads the body again looking
/// for chapters. A caller that knows the book's titles passes them here and gets edges
/// that already say what they point at; a caller that does not gets the same edges
/// unresolved.
pub fn extract_note_links_in(content: &str, index: &LinkIndex) -> NoteLinks {
    scan_sigils(&note_plain_text(content), index)
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

fn scan_sigils(text: &str, index: &LinkIndex) -> NoteLinks {
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
                '@' => links.mentions.push(index.edge(&token)),
                _ => links.refs.push(index.edge(&token)),
            }
        }
        i = end;
    }
    links
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(edges: &[NoteLink]) -> Vec<String> {
        edges.iter().map(|e| e.text.clone()).collect()
    }

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
        assert_eq!(texts(&links.refs), vec!["Vess"]);
        assert_eq!(texts(&links.mentions), vec!["Karel"]);
        assert_eq!(links.tags, vec!["siege", "slow-burn"]);
    }

    #[test]
    fn repeats_collapse_and_order_is_first_appearance() {
        let links = extract_note_links(&doc("$Vess $Karel $Vess"));
        assert_eq!(texts(&links.refs), vec!["Vess", "Karel"]);
    }

    #[test]
    fn the_same_token_under_two_sigils_is_two_edges() {
        // `@Vess` is "this note talks about Vess"; `$Vess` is "Vess is in this".
        // Collapsing them would put a character in a scene they were only named in.
        let links = extract_note_links(&doc("@Vess and $Vess"));
        assert_eq!(texts(&links.mentions), vec!["Vess"]);
        assert_eq!(texts(&links.refs), vec!["Vess"]);
    }

    #[test]
    fn a_sigil_inside_a_word_is_not_an_edge() {
        let links = extract_note_links(&doc("write to karel@example.com about pay$day"));
        assert!(links.is_empty(), "got {links:?}");
    }

    #[test]
    fn trailing_punctuation_is_not_part_of_the_name() {
        let links = extract_note_links(&doc("Then $Vess, and #siege."));
        assert_eq!(texts(&links.refs), vec!["Vess"]);
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
        assert_eq!(texts(&extract_note_links(&body).refs), vec!["Vess"]);
    }

    #[test]
    fn a_legacy_html_body_is_scanned_too() {
        // Notes predating the DocNode save shape are stored as raw HTML, and they are
        // the ones that will carry the first hand-typed sigils.
        let links = extract_note_links("<p>The siege of <b>$Vess</b></p><p>#war</p>");
        assert_eq!(texts(&links.refs), vec!["Vess"]);
        assert_eq!(links.tags, vec!["war"]);
    }

    #[test]
    fn html_tags_do_not_glue_words_together() {
        let links = extract_note_links("<p>pay<b>$day</b></p>");
        assert_eq!(
            texts(&links.refs),
            vec!["day"],
            "a tag boundary is a word boundary, so the sigil does start a token here"
        );
    }

    // ── Target kinds ──────────────────────────────────────────────────────────

    fn book_index() -> LinkIndex {
        LinkIndex::new()
            .with_notes([("n-vess", "Vess"), ("n-siege", "The Siege of Ka'ren")])
            .with_chapters([("c-3", "Three Doors")])
    }

    #[test]
    fn a_mention_reaches_a_chapter_as_well_as_a_note() {
        let links = extract_note_links_in(&doc("@Vess walks through @Three-Doors"), &book_index());
        assert_eq!(
            links.mentions,
            vec![
                NoteLink {
                    text: "Vess".into(),
                    target: LinkTarget::Note,
                    id: Some("n-vess".into()),
                },
                NoteLink {
                    text: "Three-Doors".into(),
                    target: LinkTarget::Chapter,
                    id: Some("c-3".into()),
                },
            ],
            "the kind is decided on write, beside the id"
        );
    }

    #[test]
    fn a_separator_run_in_a_token_folds_back_to_the_title_it_came_from() {
        // `token_for_title` is what the editor's autocomplete inserts; a token cannot
        // hold a space, so the round trip has to survive the substitution.
        let token = token_for_title("The Siege of Ka'ren");
        assert_eq!(token, "The-Siege-of-Ka'ren", "only the spaces have to move");
        let links = extract_note_links_in(&doc(&format!("${token}")), &book_index());
        assert_eq!(
            links.refs.first().and_then(|e| e.id.clone()),
            Some("n-siege".into())
        );
    }

    #[test]
    fn a_token_naming_nothing_is_still_an_edge() {
        // The author may be naming a note they have not written yet — dropping the
        // edge would lose the intent, and the rail is where they notice it is missing.
        let links = extract_note_links_in(&doc("$Nobody"), &book_index());
        assert_eq!(links.refs, vec![NoteLink::unresolved("Nobody")]);
    }

    #[test]
    fn a_note_wins_a_title_a_chapter_also_claims() {
        let index = LinkIndex::new()
            .with_notes([("n-1", "The Parley")])
            .with_chapters([("c-1", "The Parley")]);
        let links = extract_note_links_in(&doc("@The-Parley"), &index);
        assert!(links.mentions[0].points_at(LinkTarget::Note, "n-1"));
    }

    #[test]
    fn an_edge_round_trips_through_json_in_both_shapes() {
        // The index is stored as JSON inside the `book:` structure document, so a
        // resolved edge has to survive a save/load — and an index written before
        // chapters were reachable holds bare strings, which must still read.
        let resolved = NoteLink {
            text: "Three-Doors".into(),
            target: LinkTarget::Chapter,
            id: Some("c-3".into()),
        };
        let json = serde_json::to_string(&resolved).unwrap();
        assert_eq!(
            serde_json::from_str::<NoteLink>(&json).unwrap(),
            resolved,
            "the target kind survives the round trip"
        );

        let legacy: NoteLinks = serde_json::from_str(r#"{"refs":["Vess"],"mentions":["Karel"]}"#)
            .expect("the pre-chapter shape still reads");
        assert_eq!(legacy.refs, vec![NoteLink::unresolved("Vess")]);
        assert_eq!(legacy.mentions, vec![NoteLink::unresolved("Karel")]);
    }

    #[test]
    fn an_unresolved_note_edge_serialises_as_a_bare_string() {
        // Keeps a stored index legible, and keeps it byte-identical to what the
        // pre-chapter shape wrote for the same body.
        let links = extract_note_links(&doc("$Vess"));
        assert_eq!(
            serde_json::to_value(&links.refs).unwrap(),
            serde_json::json!(["Vess"])
        );
    }

    #[test]
    fn folding_ignores_case_and_every_kind_of_separator() {
        assert_eq!(fold_token("Three Doors"), "three doors");
        assert_eq!(fold_token("Three-Doors"), "three doors");
        assert_eq!(fold_token("  three_doors! "), "three doors");
        assert_eq!(fold_token("Ka'ren"), "ka ren");
        assert_eq!(fold_token("---"), "");
    }
}
