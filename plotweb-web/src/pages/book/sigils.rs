//! Sigil autocomplete and the context rail — the parts that are just data.
//!
//! Everything here is a pure function over the note list, so it is tested off-wasm by
//! `cargo test` in `plotweb-web/`. The rendering half lives in
//! [`super::panes::note_editor`]; the parsing half — what a *saved* body means — lives
//! in `plotweb_common::note_links` and is the single source of truth. This module never
//! parses a body: it reads the index that parser produced, and it decides what to offer
//! while the author is still typing a token that is not yet part of any body.

use plotweb_common::{fold_token, token_for_title, Chapter, LinkTarget, Note, NoteLink};

/// The three sigils, as the editor sees them mid-word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sigil {
    /// `#` — a label.
    Tag,
    /// `@` — a soft link, to a note **or a chapter**.
    Mention,
    /// `$` — participation. Offers entities only; this is the edge the timeline draws
    /// a thread along, and offering lore here would put a place in a scene as a person.
    Reference,
}

impl Sigil {
    pub fn char(self) -> char {
        match self {
            Sigil::Tag => '#',
            Sigil::Mention => '@',
            Sigil::Reference => '$',
        }
    }

    fn from_char(c: char) -> Option<Self> {
        match c {
            '#' => Some(Sigil::Tag),
            '@' => Some(Sigil::Mention),
            '$' => Some(Sigil::Reference),
            _ => None,
        }
    }
}

/// A sigil token the caret is currently sitting at the end of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveSigil {
    pub sigil: Sigil,
    /// What has been typed after the sigil, which may be empty — typing the bare sigil
    /// opens the list on everything.
    ///
    /// Its length is also what the insertion replaces: the token is a contiguous run of
    /// text ending at the caret, so `query.chars().count() + 1` characters back from the
    /// caret is exactly the sigil.
    pub query: String,
}

/// The token the caret is at the end of, if it is at the end of one.
///
/// `before` is the block's text up to the caret. The rules match the body scanner's
/// (`plotweb_common::note_links`) so the menu cannot offer a completion for something
/// that would not have become an edge: the sigil must start a token, and the token may
/// hold only the characters an edge's token may hold.
///
/// Returns `None` once the caret leaves the token — including when a space is typed,
/// which is how the author dismisses the menu by carrying on writing.
pub fn active_sigil(before: &str) -> Option<ActiveSigil> {
    let chars: Vec<(usize, char)> = before.char_indices().collect();
    // Walk back over the token body to the sigil.
    let mut i = chars.len();
    while i > 0 && is_token_char(chars[i - 1].1) {
        i -= 1;
    }
    let (sigil_at, sigil_ch) = *chars.get(i.checked_sub(1)?)?;
    let sigil = Sigil::from_char(sigil_ch)?;
    // Same word-boundary rule the scanner uses: `a@b` is an email, not a mention.
    if let Some(prev) = i.checked_sub(2).map(|p| chars[p].1)
        && prev.is_alphanumeric()
    {
        return None;
    }
    Some(ActiveSigil {
        sigil,
        query: before[sigil_at + sigil_ch.len_utf8()..].to_string(),
    })
}

/// Mirrors `plotweb_common::note_links::token_char`, which is private to that module by
/// design — this is the only other place that needs the rule, and duplicating three
/// characters is cheaper than widening that module's surface.
fn is_token_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '-' || c == '\''
}

/// What one row of the menu offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Offer {
    /// An existing note. `is_entity` drives the gutter glyph.
    Note { id: String, is_entity: bool },
    /// An existing chapter — reachable from `@` only.
    Chapter { id: String },
    /// A tag already used somewhere in this book.
    Tag,
    /// The last entry: make the thing the author is describing.
    Create,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    /// What the row reads as.
    pub label: String,
    /// What gets written into the body, sigil excluded.
    pub token: String,
    pub offer: Offer,
}

/// Rows to offer for `sigil` given what has been typed.
///
/// `self_id` is the note being edited, excluded throughout — a note that references
/// itself draws a tie from a lane to its own event, which is noise on the timeline and
/// nonsense in the rail.
///
/// "Create new" is always last and always present, so the list never becomes a dead end
/// for a name that does not exist yet. It is omitted only for an empty query, where
/// there is no name to create.
pub fn completions(
    sigil: Sigil,
    query: &str,
    self_id: &str,
    notes: &[Note],
    chapters: &[Chapter],
) -> Vec<Completion> {
    let folded = fold_token(query);
    let mut rows: Vec<(u8, String, Completion)> = Vec::new();

    match sigil {
        Sigil::Tag => {
            let mut seen: Vec<String> = Vec::new();
            for tag in notes.iter().flat_map(|n| n.links.tags.iter()) {
                if seen.iter().any(|t| fold_token(t) == fold_token(tag)) {
                    continue;
                }
                seen.push(tag.clone());
                if let Some(rank) = rank(&fold_token(tag), &folded) {
                    rows.push((
                        rank,
                        fold_token(tag),
                        Completion {
                            label: tag.clone(),
                            token: tag.clone(),
                            offer: Offer::Tag,
                        },
                    ));
                }
            }
        }
        Sigil::Mention | Sigil::Reference => {
            for note in notes {
                if note.id == self_id {
                    continue;
                }
                // `$` is participation, so only a note carrying the entity facet can
                // take one. `@` is a soft link and takes anything.
                if sigil == Sigil::Reference && !note.is_entity {
                    continue;
                }
                if let Some(rank) = rank(&fold_token(&note.title), &folded) {
                    rows.push((
                        rank,
                        fold_token(&note.title),
                        Completion {
                            label: note.title.clone(),
                            token: token_for_title(&note.title),
                            offer: Offer::Note {
                                id: note.id.clone(),
                                is_entity: note.is_entity,
                            },
                        },
                    ));
                }
            }
            if sigil == Sigil::Mention {
                for chapter in chapters {
                    if let Some(rank) = rank(&fold_token(&chapter.title), &folded) {
                        rows.push((
                            rank,
                            fold_token(&chapter.title),
                            Completion {
                                label: chapter.title.clone(),
                                token: token_for_title(&chapter.title),
                                offer: Offer::Chapter {
                                    id: chapter.id.clone(),
                                },
                            },
                        ));
                    }
                }
            }
        }
    }

    rows.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    let mut out: Vec<Completion> = rows.into_iter().map(|(_, _, c)| c).take(8).collect();

    // An exact match is what the author already has; offering to create it again would
    // be a second note with the same title, which resolution then has to choose between.
    let exact = out.iter().any(|c| fold_token(&c.label) == folded);
    if !folded.is_empty() && !exact {
        out.push(Completion {
            label: query.to_string(),
            token: match sigil {
                Sigil::Tag => query.to_string(),
                _ => token_for_title(query),
            },
            offer: Offer::Create,
        });
    }
    out
}

/// How well a candidate matches: 0 = starts with it, 1 = contains it, `None` = no.
/// An empty query matches everything at the same rank, so the list falls back to
/// alphabetical order rather than an arbitrary one.
fn rank(candidate: &str, query: &str) -> Option<u8> {
    if query.is_empty() {
        Some(0)
    } else if candidate.starts_with(query) {
        Some(0)
    } else if candidate.contains(query) {
        Some(1)
    } else {
        None
    }
}

// ── The context rail ─────────────────────────────────────────────────────────

/// Which sigil an edge came from — the rail groups by this, since `@` and `$` mean
/// different things and only `$` will draw a thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeKind {
    Mention,
    Reference,
}

/// One line in the rail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RailLink {
    /// The id to open, or `None` for a token naming something that does not exist.
    pub id: Option<String>,
    pub label: String,
    pub target: LinkTarget,
    pub edge: EdgeKind,
}

/// What this note points at — `@` then `$`, in the order the body names them.
///
/// The target kind comes off the stored edge, not from looking the id up in both
/// lists: the edge was resolved when the body was written and is the record of what
/// the author picked.
pub fn outbound(note: &Note, notes: &[Note], chapters: &[Chapter]) -> Vec<RailLink> {
    let mut out = Vec::new();
    for (edge, kind) in note
        .links
        .mentions
        .iter()
        .map(|e| (e, EdgeKind::Mention))
        .chain(note.links.refs.iter().map(|e| (e, EdgeKind::Reference)))
    {
        out.push(RailLink {
            id: edge.id.clone(),
            label: label_for(edge, notes, chapters),
            target: edge.target,
            edge: kind,
        });
    }
    out
}

/// A resolved edge reads as the current title of what it points at, so a rename shows
/// up in the rail immediately rather than at the next save of the pointing note. An
/// unresolved one reads as what was typed, with the separators put back.
fn label_for(edge: &NoteLink, notes: &[Note], chapters: &[Chapter]) -> String {
    match (&edge.id, edge.target) {
        (Some(id), LinkTarget::Note) => notes
            .iter()
            .find(|n| &n.id == id)
            .map(|n| n.title.clone())
            .unwrap_or_else(|| edge.text.replace('-', " ")),
        (Some(id), LinkTarget::Chapter) => chapters
            .iter()
            .find(|c| &c.id == id)
            .map(|c| c.title.clone())
            .unwrap_or_else(|| edge.text.replace('-', " ")),
        (None, _) => edge.text.replace('-', " "),
    }
}

/// What points at this note — the other direction of the same edges.
///
/// Matching is by id, which is what makes a rename safe. An edge that never resolved
/// is matched by its folded text as a fallback, so a note written before its target
/// existed still shows up once the target is created, without waiting for the pointing
/// note to be saved again.
pub fn inbound(note: &Note, notes: &[Note]) -> Vec<RailLink> {
    let title = fold_token(&note.title);
    let mut out = Vec::new();
    for other in notes {
        if other.id == note.id {
            continue;
        }
        let hits = |edges: &[NoteLink]| {
            edges.iter().any(|e| {
                e.points_at(LinkTarget::Note, &note.id)
                    || (e.id.is_none()
                        && !title.is_empty()
                        && e.target == LinkTarget::Note
                        && fold_token(&e.text) == title)
            })
        };
        for (edges, kind) in [
            (&other.links.mentions, EdgeKind::Mention),
            (&other.links.refs, EdgeKind::Reference),
        ] {
            if hits(edges) {
                out.push(RailLink {
                    id: Some(other.id.clone()),
                    label: other.title.clone(),
                    target: LinkTarget::Note,
                    edge: kind,
                });
            }
        }
    }
    out
}

/// Notes whose body `@`s this chapter — the manuscript side of the same edges.
///
/// The chapter has no link index of its own: a chapter body is prose, not a note, and
/// is never scanned for sigils. This is the notes' index read backwards, which is why
/// it costs one pass over a list the store already holds.
pub fn notes_mentioning_chapter(chapter_id: &str, notes: &[Note]) -> Vec<RailLink> {
    notes
        .iter()
        .filter(|n| {
            n.links
                .mentions
                .iter()
                .any(|e| e.points_at(LinkTarget::Chapter, chapter_id))
        })
        .map(|n| RailLink {
            id: Some(n.id.clone()),
            label: n.title.clone(),
            target: LinkTarget::Note,
            edge: EdgeKind::Mention,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use plotweb_common::NoteLinks;

    fn note(id: &str, title: &str) -> Note {
        Note {
            id: id.into(),
            book_id: "b".into(),
            title: title.into(),
            content: String::new(),
            color: None,
            created_at: String::new(),
            updated_at: String::new(),
            span: None,
            relative: None,
            is_entity: false,
            event_parent: None,
            pinned: false,
            links: NoteLinks::default(),
        }
    }

    fn entity(id: &str, title: &str) -> Note {
        let mut n = note(id, title);
        n.is_entity = true;
        n
    }

    fn chapter(id: &str, title: &str) -> Chapter {
        Chapter {
            id: id.into(),
            book_id: "b".into(),
            title: title.into(),
            content: String::new(),
            sort_order: 0,
            word_count: 0,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    fn labels(rows: &[Completion]) -> Vec<String> {
        rows.iter().map(|c| c.label.clone()).collect()
    }

    // ── Detecting the token under the caret ──────────────────────────────────

    #[test]
    fn the_caret_at_the_end_of_a_token_finds_it() {
        let a = active_sigil("The siege of $Ve").expect("a token");
        assert_eq!(a.sigil, Sigil::Reference);
        assert_eq!(a.query, "Ve");
    }

    #[test]
    fn a_bare_sigil_opens_the_list_on_everything() {
        let a = active_sigil("and then @").expect("a token");
        assert_eq!(a.query, "");
    }

    #[test]
    fn a_space_after_the_token_closes_the_menu() {
        assert_eq!(active_sigil("$Vess "), None);
        assert_eq!(active_sigil("no sigil here"), None);
        assert_eq!(active_sigil(""), None);
    }

    #[test]
    fn a_sigil_inside_a_word_is_not_a_token() {
        // Same rule the body scanner applies, so the menu cannot offer a completion
        // for something that would never have become an edge.
        assert_eq!(active_sigil("karel@exa"), None);
    }

    // ── What each sigil offers ───────────────────────────────────────────────

    #[test]
    fn reference_offers_entities_only() {
        let notes = vec![
            entity("n1", "Vess"),
            note("n2", "Veil politics"),
            entity("n3", "Karel"),
        ];
        let chapters = vec![chapter("c1", "Vespers")];
        let rows = completions(Sigil::Reference, "Ve", "self", &notes, &chapters);
        assert_eq!(
            labels(&rows),
            vec!["Vess", "Ve"],
            "lore and chapters are not participants; the trailing row is create-new"
        );
        assert_eq!(rows.last().map(|c| c.offer.clone()), Some(Offer::Create));
    }

    #[test]
    fn mention_offers_chapters_as_well_as_notes() {
        let notes = vec![note("n1", "The Three Kings")];
        let chapters = vec![chapter("c1", "Three Doors")];
        let rows = completions(Sigil::Mention, "Three", "self", &notes, &chapters);
        assert_eq!(labels(&rows), vec!["Three Doors", "The Three Kings", "Three"]);
        assert_eq!(
            rows[0].offer,
            Offer::Chapter { id: "c1".into() },
            "a prefix match outranks a substring one"
        );
        assert_eq!(
            rows[0].token, "Three-Doors",
            "the inserted token folds back to the title"
        );
    }

    #[test]
    fn tag_offers_tags_already_used_in_this_book() {
        let mut a = note("n1", "A");
        a.links.tags = vec!["siege".into(), "slow-burn".into()];
        let mut b = note("n2", "B");
        b.links.tags = vec!["siege".into(), "sea".into()];
        let rows = completions(Sigil::Tag, "s", "self", &[a, b], &[]);
        assert_eq!(
            labels(&rows),
            vec!["sea", "siege", "slow-burn", "s"],
            "repeats collapse across notes"
        );
    }

    #[test]
    fn the_note_being_edited_is_never_offered() {
        let notes = vec![entity("self", "Vess"), entity("n2", "Vessel")];
        let rows = completions(Sigil::Reference, "Ves", "self", &notes, &[]);
        assert_eq!(labels(&rows), vec!["Vessel", "Ves"]);
    }

    #[test]
    fn an_exact_match_does_not_also_offer_to_create_itself() {
        let notes = vec![entity("n1", "Vess")];
        let rows = completions(Sigil::Reference, "Vess", "self", &notes, &[]);
        assert_eq!(labels(&rows), vec!["Vess"]);
        assert_eq!(rows[0].offer, Offer::Note { id: "n1".into(), is_entity: true });
    }

    #[test]
    fn an_empty_query_offers_everything_and_nothing_to_create() {
        let notes = vec![entity("n1", "Vess"), entity("n2", "Karel")];
        let rows = completions(Sigil::Reference, "", "self", &notes, &[]);
        assert_eq!(labels(&rows), vec!["Karel", "Vess"]);
    }

    // ── The rail ─────────────────────────────────────────────────────────────

    fn pointing_at(id: &str, title: &str, target: LinkTarget, at: &str, text: &str) -> Note {
        let mut n = note(id, title);
        let edge = NoteLink {
            text: text.into(),
            target,
            id: Some(at.into()),
        };
        match target {
            LinkTarget::Chapter => n.links.mentions.push(edge),
            LinkTarget::Note => n.links.refs.push(edge),
        }
        n
    }

    #[test]
    fn backlinks_resolve_in_both_directions() {
        // The siege references Vess and mentions the chapter that dramatises it.
        let vess = entity("n-vess", "Vess");
        let mut siege = pointing_at("n-siege", "The siege", LinkTarget::Note, "n-vess", "Vess");
        siege.links.mentions.push(NoteLink {
            text: "Three-Doors".into(),
            target: LinkTarget::Chapter,
            id: Some("c1".into()),
        });
        let notes = vec![vess.clone(), siege.clone()];
        let chapters = vec![chapter("c1", "Three Doors")];

        // Outbound, from the siege.
        let out = outbound(&siege, &notes, &chapters);
        assert_eq!(
            out.iter()
                .map(|l| (l.label.as_str(), l.target, l.edge))
                .collect::<Vec<_>>(),
            vec![
                ("Three Doors", LinkTarget::Chapter, EdgeKind::Mention),
                ("Vess", LinkTarget::Note, EdgeKind::Reference),
            ]
        );
        // Inbound, from Vess — the same edge read the other way.
        let back = inbound(&vess, &notes);
        assert_eq!(
            back.iter()
                .map(|l| (l.id.clone(), l.label.as_str(), l.edge))
                .collect::<Vec<_>>(),
            vec![(
                Some("n-siege".to_string()),
                "The siege",
                EdgeKind::Reference
            )]
        );

        // And nothing points at the siege.
        assert!(inbound(&siege, &notes).is_empty());

        // The chapter side of the same edge, which is how an author sees from the
        // manuscript which notes claim a chapter dramatises them.
        assert_eq!(
            notes_mentioning_chapter("c1", &notes)
                .into_iter()
                .map(|l| l.label)
                .collect::<Vec<_>>(),
            vec!["The siege".to_string()]
        );
        assert!(notes_mentioning_chapter("c2", &notes).is_empty());
    }

    #[test]
    fn a_rename_shows_through_a_resolved_edge_without_resaving_the_body() {
        let mut vess = entity("n-vess", "Vess");
        let siege = pointing_at("n-siege", "The siege", LinkTarget::Note, "n-vess", "Vess");
        vess.title = "Vessaine".into();
        let notes = vec![vess, siege.clone()];
        assert_eq!(outbound(&siege, &notes, &[])[0].label, "Vessaine");
    }

    #[test]
    fn an_edge_that_never_resolved_still_backlinks_once_its_target_exists() {
        // The siege named Vess before Vess was a note, so its edge carries no id. The
        // rail should still show it rather than wait for the siege to be saved again.
        let vess = entity("n-vess", "Vess");
        let mut siege = note("n-siege", "The siege");
        siege.links.refs.push(NoteLink::unresolved("Vess"));
        let notes = vec![vess.clone(), siege];
        assert_eq!(inbound(&vess, &notes).len(), 1);
    }

    #[test]
    fn an_unresolved_edge_reads_as_what_was_typed() {
        let mut n = note("n1", "A");
        n.links.mentions.push(NoteLink::unresolved("Three-Doors"));
        assert_eq!(outbound(&n, &[], &[])[0].label, "Three Doors");
        assert_eq!(outbound(&n, &[], &[])[0].id, None);
    }
}
