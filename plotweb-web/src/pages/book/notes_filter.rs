//! The notes filter — one state, every notes view.
//!
//! The filter is deliberately *not* owned by the tree. It sits on `BookState`, above the
//! Tree/Timeline switcher, so changing view is a re-render rather than a navigation and
//! the author never loses their narrowing by looking at the same notes a different way
//! (`design/04-notes-wireframes.html`, "Filtering, shared by every view"). Card 5's
//! timeline and card 8's graph read the same [`Filter`] through the same
//! [`Filter::matches`].
//!
//! Everything here is a pure function over the note list, so `cargo test` in
//! `plotweb-web/` exercises it off-wasm — the precedent is [`super::sigils`], and the
//! reason is the same: this is the part with rules in it.
//!
//! # The three states, and why "may" is not what they are called
//!
//! A chip cycles **off → must → any of → without → off**. `must` and `without` are
//! ordinary conjunctions — every `must` term has to hold, no `without` term may. The
//! third is the one the design flagged as badly named: the wireframe called it *may*,
//! but an unset chip already means "may", so a chip in that state would say nothing. It
//! is an **OR group**: once any chip is in it, a note has to satisfy *at least one* of
//! them. So it is labelled **"any of"** here, which stays true as a sentence when three
//! chips are in the group ("any of #siege, #act-two, entity") and is the only wording
//! tried that does not read as a synonym for off.

use std::collections::{HashMap, HashSet};

use plotweb_common::{fold_token, Note, NoteTree, TimeRelation, TICKS_PER_BASE_UNIT};

/// A facet a note can carry. Facets are **not exclusive** — a character with a lifespan
/// is both an entity and an event — which is precisely why the tree does not section by
/// them and why they appear here, as filter terms, instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Facet {
    /// Placed in time: a span, or a constraint against another note.
    Event,
    /// A person, place or thing that can appear in events.
    Entity,
    /// Neither — the state every note starts in.
    Lore,
}

impl Facet {
    /// The gutter glyph, shared with the note editor's facet strip and the sigil menu
    /// (`panes::note_editor`) so one vocabulary covers every surface.
    pub fn glyph(self) -> &'static str {
        match self {
            Facet::Event => "◇",
            Facet::Entity => "◆",
            Facet::Lore => "○",
        }
    }

    /// The word the chip and the row title use.
    pub fn label(self) -> &'static str {
        match self {
            Facet::Event => "event",
            Facet::Entity => "entity",
            Facet::Lore => "lore",
        }
    }

    /// The CSS modifier for the glyph, so a stylesheet can draw the three shapes.
    pub fn css(self) -> &'static str {
        match self {
            Facet::Event => "is-event",
            Facet::Entity => "is-entity",
            Facet::Lore => "is-lore",
        }
    }

    /// Whether `note` carries this facet. A note can answer `true` to both `Event` and
    /// `Entity`; `Lore` is the absence of both, so it never overlaps either.
    pub fn held_by(self, note: &Note) -> bool {
        match self {
            Facet::Event => note.is_event(),
            Facet::Entity => note.is_entity,
            Facet::Lore => !note.is_event() && !note.is_entity,
        }
    }
}

/// The facet a note is *drawn* as in the gutter, where only one glyph fits.
///
/// Entity wins over event because the entity is the durable thing — a character with a
/// lifespan is a character first — and lore is what is left. The filter never uses this:
/// it asks [`Facet::held_by`], which lets both answer true.
pub fn shown_facet(note: &Note) -> Facet {
    if note.is_entity {
        Facet::Entity
    } else if note.is_event() {
        Facet::Event
    } else {
        Facet::Lore
    }
}

/// One thing a chip can filter on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Term {
    Facet(Facet),
    /// A `#tag` from the link index. Compared folded, so `#Act-Two` and `#act-two` are
    /// one chip (`plotweb_common::fold_token` is the same rule the sigil menu resolves
    /// by, so the chip and the completion agree on what one tag is).
    Tag(String),
}

impl Term {
    /// How the chip reads.
    pub fn label(&self) -> String {
        match self {
            Term::Facet(f) => f.label().to_string(),
            Term::Tag(t) => format!("#{t}"),
        }
    }

    /// Stable identity, so a chip keeps its state when the tag list is rebuilt from a
    /// freshly loaded note (tags are discovered from bodies, not declared).
    pub fn key(&self) -> String {
        match self {
            Term::Facet(f) => format!("facet:{}", f.label()),
            Term::Tag(t) => format!("tag:{}", fold_token(t)),
        }
    }

    pub fn held_by(&self, note: &Note) -> bool {
        match self {
            Term::Facet(f) => f.held_by(note),
            Term::Tag(t) => {
                let want = fold_token(t);
                note.links.tags.iter().any(|tag| fold_token(tag) == want)
            }
        }
    }
}

/// What one chip is currently demanding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChipState {
    /// Says nothing.
    #[default]
    Off,
    /// Every note must hold this term.
    Must,
    /// Part of the OR group: a note must hold *at least one* of these. See the module
    /// header for why this is not called "may".
    AnyOf,
    /// No note holding this term is shown.
    Without,
}

impl ChipState {
    /// The cycle a click walks: off → must → any of → without → off.
    ///
    /// `must` comes first because it is what a chip is reached for: the common gesture
    /// is "show me the events", and that should be one click, not three.
    pub fn next(self) -> Self {
        match self {
            ChipState::Off => ChipState::Must,
            ChipState::Must => ChipState::AnyOf,
            ChipState::AnyOf => ChipState::Without,
            ChipState::Without => ChipState::Off,
        }
    }

    /// The operator drawn in the chip's left slot.
    pub fn op(self) -> &'static str {
        match self {
            ChipState::Off => "·",
            ChipState::Must => "+",
            ChipState::AnyOf => "|",
            ChipState::Without => "\u{2212}",
        }
    }

    /// The word for this state, in the UI's own language.
    pub fn label(self) -> &'static str {
        match self {
            ChipState::Off => "off",
            ChipState::Must => "must",
            ChipState::AnyOf => "any of",
            ChipState::Without => "without",
        }
    }

    /// The `data-state` value the stylesheet and the e2e specs key off.
    pub fn css(self) -> &'static str {
        match self {
            ChipState::Off => "off",
            ChipState::Must => "must",
            ChipState::AnyOf => "any",
            ChipState::Without => "without",
        }
    }
}

/// The whole filter: the chips that are saying something.
///
/// Only non-[`ChipState::Off`] terms are stored, keyed by [`Term::key`], so the chip row
/// can be rebuilt from whatever tags currently exist without disturbing the state of the
/// chips that survive — and a tag that vanishes from every body simply stops being
/// offered rather than silently narrowing the view forever.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Filter {
    set: Vec<(Term, ChipState)>,
}

impl Filter {
    pub fn state_of(&self, term: &Term) -> ChipState {
        let key = term.key();
        self.set
            .iter()
            .find(|(t, _)| t.key() == key)
            .map(|(_, s)| *s)
            .unwrap_or_default()
    }

    /// The filter that results from clicking `term`'s chip. Returns a new value rather
    /// than mutating, because the caller holds it in a `Signal` and a signal is only
    /// updated by being set.
    pub fn cycled(&self, term: &Term) -> Filter {
        let next = self.state_of(term).next();
        let key = term.key();
        let mut set: Vec<(Term, ChipState)> =
            self.set.iter().filter(|(t, _)| t.key() != key).cloned().collect();
        if next != ChipState::Off {
            set.push((term.clone(), next));
        }
        Filter { set }
    }

    /// Nothing is being asked, so every note passes and no row is dimmed.
    pub fn is_empty(&self) -> bool {
        self.set.is_empty()
    }

    pub fn cleared() -> Filter {
        Filter::default()
    }

    /// Terms in a given state, in the order they were switched on — used for the
    /// filter bar's plain-language summary.
    pub fn terms_in(&self, state: ChipState) -> Vec<&Term> {
        self.set
            .iter()
            .filter(|(_, s)| *s == state)
            .map(|(t, _)| t)
            .collect()
    }

    /// The rule, and the only place it lives.
    ///
    /// Every `must` holds, no `without` holds, and — if the OR group has any members at
    /// all — at least one of them holds. An empty group is not a demand: that is what
    /// keeps "off" and "any of" from meaning the same thing (see the module header).
    pub fn matches(&self, note: &Note) -> bool {
        let mut any_of_seen = false;
        let mut any_of_hit = false;
        for (term, state) in &self.set {
            match state {
                ChipState::Off => {}
                ChipState::Must => {
                    if !term.held_by(note) {
                        return false;
                    }
                }
                ChipState::Without => {
                    if term.held_by(note) {
                        return false;
                    }
                }
                ChipState::AnyOf => {
                    any_of_seen = true;
                    any_of_hit |= term.held_by(note);
                }
            }
        }
        !any_of_seen || any_of_hit
    }
}

/// What the filter does to the outline.
///
/// A tree cannot simply drop the rows that fail: a matching note filed three levels down
/// would vanish with its ancestors. So an ancestor of a match is **kept and dimmed**
/// (`muted`) — it is scaffolding, not a result — and a subtree with no match anywhere in
/// it is dropped entirely. With no filter set, everything is shown and nothing is muted.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TreeFilter {
    /// Note ids that render at all.
    pub shown: HashSet<String>,
    /// Of those, the ones shown only because a descendant matched.
    pub muted: HashSet<String>,
    /// How many notes match in their own right — the numerator of the header count.
    pub matched: usize,
}

impl TreeFilter {
    pub fn is_shown(&self, id: &str) -> bool {
        self.shown.contains(id)
    }
    pub fn is_muted(&self, id: &str) -> bool {
        self.muted.contains(id)
    }
}

/// Resolve `filter` against the outline.
///
/// Walks the tree rather than the note list because the answer is about ancestry. Notes
/// the tree does not mention are still counted as matches — they are real notes, and a
/// count that disagreed with the note list would be a bug report waiting to happen — but
/// they have no row to show.
pub fn apply_to_tree(notes: &[Note], tree: &NoteTree, filter: &Filter) -> TreeFilter {
    let by_id: HashMap<&str, &Note> = notes.iter().map(|n| (n.id.as_str(), n)).collect();
    let mut out = TreeFilter::default();
    if filter.is_empty() {
        out.shown = notes.iter().map(|n| n.id.clone()).collect();
        out.matched = notes.len();
        return out;
    }
    // A malformed tree (a cycle, a child listed twice) must not hang the render.
    let mut seen: HashSet<String> = HashSet::new();
    for root in &tree.root_order {
        walk(root, &by_id, tree, filter, &mut out, &mut seen);
    }
    // Notes with no row — orphans of a stale tree — still count as matches.
    out.matched += notes
        .iter()
        .filter(|n| !seen.contains(&n.id) && filter.matches(n))
        .count();
    out
}

/// Returns whether anything in this subtree (including `id`) matched.
fn walk(
    id: &str,
    by_id: &HashMap<&str, &Note>,
    tree: &NoteTree,
    filter: &Filter,
    out: &mut TreeFilter,
    seen: &mut HashSet<String>,
) -> bool {
    if !seen.insert(id.to_string()) {
        return false;
    }
    let mut subtree_hit = false;
    for child in tree.children.get(id).into_iter().flatten() {
        subtree_hit |= walk(child, by_id, tree, filter, out, seen);
    }
    let hit = by_id.get(id).is_some_and(|n| filter.matches(n));
    if hit {
        out.matched += 1;
    }
    if hit || subtree_hit {
        out.shown.insert(id.to_string());
        if !hit {
            out.muted.insert(id.to_string());
        }
    }
    hit || subtree_hit
}

/// Every tag any note carries, folded to one chip per tag and ordered alphabetically.
///
/// The first spelling seen wins the label, so a tag written `#Siege` once and `#siege`
/// twice still reads the way it was first written rather than in a normalised form the
/// author never typed.
pub fn tags_in(notes: &[Note]) -> Vec<String> {
    let mut seen: HashMap<String, String> = HashMap::new();
    for note in notes {
        for tag in &note.links.tags {
            seen.entry(fold_token(tag)).or_insert_with(|| tag.clone());
        }
    }
    let mut out: Vec<String> = seen.into_values().collect();
    out.sort_by_key(|t| fold_token(t));
    out
}

/// The chips the bar offers: the three facets, then whatever tags exist.
///
/// Facets are always present — they are the tree's own vocabulary, and an empty book
/// should still show what the chips do — while tags only appear once something is
/// tagged, so the bar does not open on a row of controls that filter nothing.
pub fn chips_for(notes: &[Note]) -> Vec<Term> {
    let mut terms = vec![
        Term::Facet(Facet::Event),
        Term::Facet(Facet::Entity),
        Term::Facet(Facet::Lore),
    ];
    terms.extend(tags_in(notes).into_iter().map(Term::Tag));
    terms
}

/// What the right-hand gutter says about when a note happens.
///
/// Card 4 brings the per-book calendar that turns a tick into "3 Frostmonth"; until then
/// the base unit is the only thing that can be said without inventing units the book has
/// not defined, so that is what is said — `1206`, `1181 – 1211`, `1198 –`, `~1207` —
/// matching the wireframe's gutter. `None` means the note is undated and the gutter stays
/// empty rather than carrying a word like "undated" on every row of a mostly-lore tree.
pub fn span_label(note: &Note, notes: &[Note]) -> Option<String> {
    if let Some(span) = &note.span {
        let start = base_unit(span.start.tick);
        let mut out = String::new();
        if span.approximate {
            out.push('~');
        }
        out.push_str(&start.to_string());
        match (&span.end, span.open_ended) {
            (Some(end), _) => {
                out.push_str(" \u{2013} ");
                out.push_str(&base_unit(end.tick).to_string());
            }
            (None, true) => out.push_str(" \u{2013}"),
            (None, false) => {}
        }
        return Some(out);
    }
    let rel = note.relative.as_ref()?;
    let word = match rel.relation {
        TimeRelation::After => "after",
        TimeRelation::Before => "before",
        TimeRelation::During => "during",
    };
    let target = notes
        .iter()
        .find(|n| n.id == rel.note_id)
        .map(|n| n.title.clone())
        .unwrap_or_else(|| "another note".to_string());
    Some(format!("{word} {target}"))
}

/// Ticks to whole base units, rounding towards negative infinity so a point before the
/// epoch reads as the unit it falls in rather than the one after it.
fn base_unit(tick: i64) -> i64 {
    tick.div_euclid(TICKS_PER_BASE_UNIT)
}

/// The filter is the part of this card with rules in it, and the rules are exactly the
/// place a "may" that meant nothing would have hidden. These pin the three states,
/// their interaction, and the tree's ancestor-keeping.
#[cfg(test)]
mod tests {
    use super::*;
    use plotweb_common::{NoteLinks, TimePoint, TimeSpan};

    fn note(id: &str, title: &str) -> Note {
        Note {
            id: id.to_string(),
            book_id: "b".to_string(),
            title: title.to_string(),
            content: String::new(),
            color: None,
            created_at: String::new(),
            updated_at: String::new(),
            span: None,
            relative: None,
            is_entity: false,
            event_parent: None,
            links: NoteLinks::default(),
        }
    }

    fn entity(id: &str) -> Note {
        Note {
            is_entity: true,
            ..note(id, id)
        }
    }

    fn event(id: &str, year: i64) -> Note {
        Note {
            span: Some(TimeSpan::at(TimePoint::base_unit(year))),
            ..note(id, id)
        }
    }

    fn tagged(id: &str, tags: &[&str]) -> Note {
        Note {
            links: NoteLinks {
                tags: tags.iter().map(|t| t.to_string()).collect(),
                ..Default::default()
            },
            ..note(id, id)
        }
    }

    fn filter(terms: &[(Term, ChipState)]) -> Filter {
        let mut f = Filter::default();
        for (term, want) in terms {
            // Cycle to the wanted state the way a user would, which also exercises the
            // cycle order rather than trusting a constructor.
            while f.state_of(term) != *want {
                f = f.cycled(term);
            }
        }
        f
    }

    #[test]
    fn a_chip_cycles_off_must_any_without_and_back() {
        let term = Term::Facet(Facet::Event);
        let f = Filter::default();
        assert_eq!(f.state_of(&term), ChipState::Off);
        let f = f.cycled(&term);
        assert_eq!(f.state_of(&term), ChipState::Must);
        let f = f.cycled(&term);
        assert_eq!(f.state_of(&term), ChipState::AnyOf);
        let f = f.cycled(&term);
        assert_eq!(f.state_of(&term), ChipState::Without);
        let f = f.cycled(&term);
        assert_eq!(f.state_of(&term), ChipState::Off);
        assert!(f.is_empty(), "a chip back at off leaves nothing behind");
    }

    #[test]
    fn an_empty_filter_matches_everything() {
        let f = Filter::default();
        assert!(f.matches(&note("a", "A")));
        assert!(f.matches(&entity("b")));
        assert!(f.matches(&event("c", 1206)));
    }

    #[test]
    fn must_terms_all_have_to_hold() {
        // A character with a lifespan is the case the whole facet design exists for:
        // it is both an entity and an event, so both `must` chips pass on it.
        let mut character = entity("vess");
        character.span = Some(TimeSpan::at(TimePoint::base_unit(1181)));
        let f = filter(&[
            (Term::Facet(Facet::Entity), ChipState::Must),
            (Term::Facet(Facet::Event), ChipState::Must),
        ]);
        assert!(f.matches(&character));
        assert!(!f.matches(&entity("order")), "an undated entity is not an event");
        assert!(!f.matches(&event("siege", 1206)), "an event is not an entity");
    }

    #[test]
    fn without_terms_exclude_even_when_a_must_passes() {
        let mut character = entity("vess");
        character.span = Some(TimeSpan::at(TimePoint::base_unit(1181)));
        let f = filter(&[
            (Term::Facet(Facet::Entity), ChipState::Must),
            (Term::Facet(Facet::Event), ChipState::Without),
        ]);
        assert!(!f.matches(&character), "dated, so excluded");
        assert!(f.matches(&entity("order")), "an undated entity survives");
    }

    /// The semantics the rename is about. One chip in the group is indistinguishable
    /// from `must`; *two* is where it stops being a synonym for anything else.
    #[test]
    fn any_of_means_at_least_one_of_the_group() {
        let siege = tagged("siege", &["siege", "act-two"]);
        let ride = tagged("ride", &["act-two"]);
        let amber = tagged("amber", &["lore"]);

        let one = filter(&[(Term::Tag("siege".into()), ChipState::AnyOf)]);
        assert!(one.matches(&siege));
        assert!(!one.matches(&ride), "a single-member group still excludes");

        let two = filter(&[
            (Term::Tag("siege".into()), ChipState::AnyOf),
            (Term::Tag("act-two".into()), ChipState::AnyOf),
        ]);
        assert!(two.matches(&siege));
        assert!(two.matches(&ride), "holding either member is enough");
        assert!(!two.matches(&amber), "holding neither is not");
    }

    /// The bug the wireframe's open question named: if an empty group demanded
    /// anything, every chip would have to be in it for the filter to show a single note.
    #[test]
    fn an_empty_any_of_group_demands_nothing() {
        let f = filter(&[(Term::Facet(Facet::Entity), ChipState::Must)]);
        assert!(f.terms_in(ChipState::AnyOf).is_empty());
        assert!(f.matches(&entity("vess")));
    }

    #[test]
    fn must_and_any_of_and_without_compose() {
        // "must be an entity, tagged either #siege or #act-two, and not lore."
        let mut vess = entity("vess");
        vess.links.tags = vec!["act-two".into()];
        let mut order = entity("order");
        order.links.tags = vec!["lore".into()];
        let siege = tagged("siege", &["siege"]);

        let f = filter(&[
            (Term::Facet(Facet::Entity), ChipState::Must),
            (Term::Tag("siege".into()), ChipState::AnyOf),
            (Term::Tag("act-two".into()), ChipState::AnyOf),
            (Term::Tag("lore".into()), ChipState::Without),
        ]);
        assert!(f.matches(&vess));
        assert!(!f.matches(&order), "excluded by #lore even though it is an entity");
        assert!(!f.matches(&siege), "not an entity");
    }

    #[test]
    fn tags_compare_folded_so_one_tag_is_one_chip() {
        let n = tagged("a", &["Act-Two"]);
        let f = filter(&[(Term::Tag("act two".into()), ChipState::Must)]);
        assert!(f.matches(&n), "the chip and the body name the same tag");
        assert_eq!(
            Term::Tag("Act-Two".into()).key(),
            Term::Tag("act two".into()).key(),
            "so they are also one chip, not two"
        );
    }

    #[test]
    fn lore_is_the_absence_of_both_other_facets() {
        let f = filter(&[(Term::Facet(Facet::Lore), ChipState::Must)]);
        assert!(f.matches(&note("a", "Amberwork")));
        assert!(!f.matches(&entity("vess")));
        assert!(!f.matches(&event("siege", 1206)));
    }

    #[test]
    fn a_note_placed_only_against_another_note_is_still_an_event() {
        let mut n = note("a", "The parley");
        n.relative = Some(plotweb_common::RelativeTime {
            relation: TimeRelation::After,
            note_id: "siege".into(),
        });
        let f = filter(&[(Term::Facet(Facet::Event), ChipState::Must)]);
        assert!(f.matches(&n));
    }

    // ── The tree ─────────────────────────────────────────────────────────────

    fn tree(roots: &[&str], children: &[(&str, &[&str])]) -> NoteTree {
        NoteTree {
            root_order: roots.iter().map(|s| s.to_string()).collect(),
            children: children
                .iter()
                .map(|(p, cs)| {
                    (
                        p.to_string(),
                        cs.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
                    )
                })
                .collect(),
            collapsed: Vec::new(),
        }
    }

    #[test]
    fn no_filter_shows_the_whole_tree_undimmed() {
        let notes = vec![note("a", "A"), note("b", "B")];
        let t = tree(&["a"], &[("a", &["b"])]);
        let out = apply_to_tree(&notes, &t, &Filter::default());
        assert!(out.is_shown("a") && out.is_shown("b"));
        assert!(out.muted.is_empty());
        assert_eq!(out.matched, 2);
    }

    /// The reason a tree filter cannot just drop the misses: House Vaun is lore, and
    /// filtering to entities must not take Vess Calloran off the screen with it.
    #[test]
    fn an_ancestor_of_a_match_is_kept_but_dimmed() {
        let notes = vec![note("house", "House Vaun"), entity("vess")];
        let t = tree(&["house"], &[("house", &["vess"])]);
        let f = filter(&[(Term::Facet(Facet::Entity), ChipState::Must)]);
        let out = apply_to_tree(&notes, &t, &f);
        assert!(out.is_shown("vess") && !out.is_muted("vess"));
        assert!(out.is_shown("house"), "the path to a match is kept");
        assert!(out.is_muted("house"), "but it is scaffolding, not a result");
        assert_eq!(out.matched, 1, "and it is not counted as a hit");
    }

    #[test]
    fn a_subtree_with_no_match_anywhere_is_dropped() {
        let notes = vec![note("lore", "Amberwork"), note("child", "Seasons")];
        let t = tree(&["lore"], &[("lore", &["child"])]);
        let f = filter(&[(Term::Facet(Facet::Entity), ChipState::Must)]);
        let out = apply_to_tree(&notes, &t, &f);
        assert!(out.shown.is_empty());
        assert_eq!(out.matched, 0);
    }

    #[test]
    fn a_cyclic_tree_terminates() {
        // Not reachable through the UI, but a stale tree from another device is a plain
        // JSON document and this walk must not hang on one.
        let notes = vec![entity("a"), note("b", "B")];
        let t = tree(&["a"], &[("a", &["b"]), ("b", &["a"])]);
        let f = filter(&[(Term::Facet(Facet::Entity), ChipState::Must)]);
        let out = apply_to_tree(&notes, &t, &f);
        assert!(out.is_shown("a"));
        assert_eq!(out.matched, 1);
    }

    #[test]
    fn a_note_the_tree_never_mentions_still_counts() {
        let notes = vec![entity("orphan")];
        let t = tree(&[], &[]);
        let f = filter(&[(Term::Facet(Facet::Entity), ChipState::Must)]);
        let out = apply_to_tree(&notes, &t, &f);
        assert_eq!(out.matched, 1, "it is a real note, whatever the tree says");
        assert!(!out.is_shown("orphan"), "but it has no row");
    }

    // ── Chips and gutter ─────────────────────────────────────────────────────

    #[test]
    fn chips_are_the_three_facets_plus_whatever_is_tagged() {
        let notes = vec![tagged("a", &["Siege"]), tagged("b", &["act-two", "siege"])];
        let chips = chips_for(&notes);
        assert_eq!(
            chips.iter().map(|t| t.label()).collect::<Vec<_>>(),
            vec!["event", "entity", "lore", "#act-two", "#Siege"],
            "facets first, then tags alphabetically, in their first-written spelling"
        );
    }

    #[test]
    fn the_gutter_reads_the_span_in_base_units() {
        let mut n = event("siege", 1206);
        assert_eq!(span_label(&n, &[]).as_deref(), Some("1206"));

        n.span = Some(TimeSpan {
            start: TimePoint::base_unit(1181),
            end: Some(TimePoint::base_unit(1211)),
            approximate: false,
            open_ended: false,
        });
        assert_eq!(span_label(&n, &[]).as_deref(), Some("1181 \u{2013} 1211"));

        n.span = Some(TimeSpan {
            start: TimePoint::base_unit(1198),
            end: None,
            approximate: false,
            open_ended: true,
        });
        assert_eq!(span_label(&n, &[]).as_deref(), Some("1198 \u{2013}"));

        n.span = Some(TimeSpan {
            approximate: true,
            ..TimeSpan::at(TimePoint::base_unit(1207))
        });
        assert_eq!(span_label(&n, &[]).as_deref(), Some("~1207"));
    }

    #[test]
    fn an_undated_note_has_an_empty_gutter_and_a_relative_one_names_its_anchor() {
        let plain = note("a", "Amberwork");
        assert_eq!(span_label(&plain, &[]), None);

        let mut rel = note("b", "The parley");
        rel.relative = Some(plotweb_common::RelativeTime {
            relation: TimeRelation::After,
            note_id: "siege".into(),
        });
        let siege = note("siege", "The Siege of Vaun");
        assert_eq!(
            span_label(&rel, &[siege]).as_deref(),
            Some("after The Siege of Vaun")
        );
    }

    #[test]
    fn the_gutter_glyph_shows_one_facet_but_the_filter_sees_both() {
        let mut character = entity("vess");
        character.span = Some(TimeSpan::at(TimePoint::base_unit(1181)));
        assert_eq!(shown_facet(&character), Facet::Entity);
        assert!(Facet::Event.held_by(&character), "and it is still an event");
    }
}
