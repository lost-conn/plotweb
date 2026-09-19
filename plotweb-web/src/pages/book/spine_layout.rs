//! The phone timeline — a vertical spine (notes card 7).
//!
//! `design/04-notes-wireframes.html`, "Timeline, take B — vertical spine": time runs
//! top to bottom, events are cards on a spine, nesting is indentation, simultaneity is
//! an explicit paired row rather than a geometric accident. "Note what disappears:
//! there is no lane-packing problem, no crossing minimisation, and no undated rail —
//! an undated note just sits inline where its relative constraint puts it."
//!
//! This is **a new drawing of existing data, not new logic**
//! (`design/05-notes-build-plan.md`, card 7): nesting comes from
//! [`super::ribbon`] via [`super::timeline_layout::ribbon_of`] and
//! [`super::timeline_layout::shown_events`] — the same span rule, the same cycle
//! breaking, the same "ancestor of a match is drawn muted" filter rule the ribbon
//! uses — and participation comes from [`super::timeline_layout::participants`].
//! Nothing here re-derives any of that; it only decides an order (top to bottom
//! instead of left to right), groups overlapping siblings into a "same time" row, and
//! places relative/undated notes that the ribbon has nowhere to draw.
//!
//! Pure functions over the note list, host-tested like [`super::notes_filter`],
//! [`super::ribbon`] and [`super::timeline_layout`]. `panes::timeline_spine` turns a
//! [`Spine`] into HTML and does nothing else.
//!
//! # Ordering
//!
//! Each level (the roots, and each event's children) is sorted by its drawn start
//! tick, then end, then title, then id — the same tie-break the ribbon's bars use for
//! left-to-right order, read top to bottom instead.
//!
//! # Simultaneity
//!
//! Siblings — never events at different depths, which are already excluded from
//! "simultaneous" by nesting instead — whose drawn extents overlap are merged into one
//! [`Row::Simultaneous`], transitively: if A overlaps B and B overlaps C, all three
//! share one row, even if A and C do not themselves overlap. That is a deliberate
//! simplification over pairwise-only grouping — three events sharing a moment is one
//! "at the same time" block, not two.
//!
//! # Badges instead of markers
//!
//! The ribbon marks a clamped child with a zigzag and an escaping one with a red dot;
//! the spine has no drawing to hang a marker on, so [`Badge`] is text next to the date
//! instead: "cut at parent" for [`ribbon::Drawn::clipped_start`]/`clipped_end`
//! (only possible under [`SpanRule::Clamp`]), "outside parent" for
//! `escapes_start`/`escapes_end` (Free, or a pinned parent under Fit). The two never
//! both apply to one event, so one `Option<Badge>` is enough.
//!
//! # Inline placement and "not yet dated"
//!
//! A note with no span is either anchored to another note (`relative`) or it isn't.
//! Anchored: `before` goes immediately above the target's own row (or its shared row,
//! if the target is part of a simultaneous group); `after`/`during` go immediately
//! after the target's *whole subtree* — its own row plus everything nested under it —
//! so it reads as the target's neighbour, not as one of its children. A relative note
//! whose target is not itself drawn on the spine (undated, filtered out, or lore with
//! no span and no event descendant) has nothing to anchor to and falls into the same
//! bucket as a plain `$ref`-only note: [`Spine::not_yet_dated`]. Plain lore — no span,
//! no relation, no `$ref` — is excluded entirely, exactly as the desktop holding rail
//! excludes it (`timeline_layout`'s `held`), so a lore-heavy book does not bury the
//! group.

use std::collections::{BTreeMap, HashMap, HashSet};

use plotweb_common::{fold_token, Calendar, Note, SpanRule, TimePoint, TimeRelation};

use super::notes_filter::Filter;
use super::timeline_layout::{self as tl, participants};

/// The span rule's effect on how an event is drawn, shown as text where the ribbon
/// would draw a marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Badge {
    /// Clamp: cut off at the parent's edge ([`ribbon::Drawn::clipped_start`] or
    /// `clipped_end`).
    Clamped,
    /// Free, or a pinned parent under Fit: runs outside the event it is in
    /// (`escapes_start`/`escapes_end`).
    Escapes,
}

impl Badge {
    pub fn label(self) -> &'static str {
        match self {
            Badge::Clamped => "cut at parent",
            Badge::Escapes => "outside parent",
        }
    }
}

/// One event's card on the spine, or one relative note placed inline beside another
/// event's card.
#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    pub id: String,
    pub title: String,
    /// The date, through the book's calendar — the note's own typed span if it has
    /// one, else the fitted extent (a date-less parent has nothing else to show), or
    /// the relation as typed ("after The Siege of Vaun") for an inline relative note.
    pub when: String,
    pub who: Vec<String>,
    pub who_titles: Vec<String>,
    /// How many `event_parent` levels deep — indentation.
    pub depth: usize,
    pub fuzzy: bool,
    pub open: bool,
    /// Drawn only because it contains a match, the same rule the tree and the ribbon
    /// use for the path to a result.
    pub muted: bool,
    pub badge: Option<Badge>,
    /// The selected note is this one, or takes part in it.
    pub on: bool,
}

/// Two or more sibling events whose spans overlap without one containing the other —
/// drawn as an explicit group rather than left to the reader to notice.
#[derive(Debug, Clone, PartialEq)]
pub enum Row {
    Card(Card),
    Simultaneous { depth: usize, items: Vec<Card> },
}

/// A note with nothing to place it: no span, no relative constraint that resolves to a
/// drawn event. Named for what the strip calls this state (`time_entry::TimeState`).
#[derive(Debug, Clone, PartialEq)]
pub struct Loose {
    pub id: String,
    pub title: String,
    pub who: Vec<String>,
    pub who_titles: Vec<String>,
    pub on: bool,
}

pub struct SpineInput<'a> {
    pub notes: &'a [Note],
    pub filter: &'a Filter,
    pub calendar: &'a Calendar,
    pub selected: Option<&'a str>,
    pub rule: SpanRule,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Spine {
    pub rows: Vec<Row>,
    pub not_yet_dated: Vec<Loose>,
    /// Nothing is drawn on the spine at all — a placeholder hint, the phone reading of
    /// `timeline_layout::Layout::undated`.
    pub undated: bool,
}

/// A date-less event's extent, formatted at the calendar's base unit — the coarsest
/// reading, which is all a fitted-not-typed extent deserves.
fn format_extent(calendar: &Calendar, start: i64, end: Option<i64>) -> String {
    let s = calendar.format_point(&TimePoint { tick: start, precision: 0 });
    match end {
        Some(e) => format!("{} \u{2013} {}", s, calendar.format_point(&TimePoint { tick: e, precision: 0 })),
        None => format!("{} \u{2013}", s),
    }
}

/// Cluster consecutive (already start-sorted) siblings into overlap groups, transitively:
/// each item joins the current cluster when its start is before the cluster's running
/// max end. A `Drawn::end` of `None` (open-ended) is treated as later than everything.
fn cluster_overlaps<'a>(ids: &[&'a str], rib: &tl::Ribbon) -> Vec<Vec<&'a str>> {
    let mut out: Vec<Vec<&str>> = Vec::new();
    let mut cur: Vec<&str> = Vec::new();
    let mut cur_end = i64::MIN;
    for &id in ids {
        let d = &rib.drawn[id];
        let end = d.end.unwrap_or(i64::MAX);
        if !cur.is_empty() && d.start >= cur_end {
            out.push(std::mem::take(&mut cur));
        }
        cur.push(id);
        cur_end = cur_end.max(end);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn row_depth(rows: &[Row], idx: usize) -> usize {
    match &rows[idx] {
        Row::Card(c) => c.depth,
        Row::Simultaneous { depth, .. } => *depth,
    }
}

struct Ctx<'a> {
    rib: &'a tl::Ribbon,
    by_id: &'a HashMap<&'a str, &'a Note>,
    children: &'a BTreeMap<&'a str, Vec<&'a str>>,
    entities: &'a HashSet<&'a str>,
    shown: &'a BTreeMap<String, bool>,
    rule: SpanRule,
    calendar: &'a Calendar,
    notes: &'a [Note],
    selected: Option<&'a str>,
}

impl<'a> Ctx<'a> {
    fn lit(&self, id: &str, who: &[String]) -> bool {
        self.selected.is_some_and(|s| s == id || who.iter().any(|w| w == s))
    }

    fn who_titles(&self, who: &[String]) -> Vec<String> {
        who.iter()
            .map(|w| self.by_id.get(w.as_str()).map(|n| n.title.clone()).unwrap_or_default())
            .collect()
    }

    fn card(&self, id: &str, depth: usize) -> Card {
        let n = self.by_id[id];
        let d = &self.rib.drawn[id];
        let who = participants(n, self.entities);
        let who_titles = self.who_titles(&who);
        let badge = match self.rule {
            SpanRule::Clamp if d.clipped_start || d.clipped_end => Some(Badge::Clamped),
            _ if d.escapes_start || d.escapes_end => Some(Badge::Escapes),
            _ => None,
        };
        let when = match &n.span {
            Some(span) => super::time_entry::format_entry(Some(span), None, self.calendar, self.notes),
            None => format_extent(self.calendar, d.start, d.end),
        };
        Card {
            id: id.to_string(),
            title: n.title.clone(),
            when,
            fuzzy: n.span.as_ref().is_some_and(|s| s.approximate),
            open: d.end.is_none(),
            muted: self.shown[id],
            badge,
            on: self.lit(id, &who),
            who_titles,
            who,
            depth,
        }
    }

    /// An inline relative note, anchored beside `target`'s row at `depth` — the
    /// target's own depth, so it reads as a neighbour rather than a child.
    fn relative_card(&self, n: &Note, depth: usize) -> Card {
        let who = participants(n, self.entities);
        let who_titles = self.who_titles(&who);
        let when = super::time_entry::format_entry(None, n.relative.as_ref(), self.calendar, self.notes);
        Card {
            id: n.id.clone(),
            title: n.title.clone(),
            when,
            fuzzy: false,
            open: false,
            muted: false,
            badge: None,
            on: self.lit(&n.id, &who),
            who_titles,
            who,
            depth,
        }
    }

    /// Emit one sibling level, clustering overlaps and recursing into each item's
    /// children right after its own row — depth-first, so a nested subtree always
    /// reads immediately under its parent. `span_of` records, for every id, the row
    /// index range `[start, end)` its own row and everything nested under it occupy —
    /// what an inline relative note is anchored against.
    fn emit(
        &self,
        ids: &[&'a str],
        depth: usize,
        rows: &mut Vec<Row>,
        span_of: &mut HashMap<String, (usize, usize)>,
    ) {
        for cluster in cluster_overlaps(ids, self.rib) {
            let row_idx = rows.len();
            if let [only] = cluster[..] {
                rows.push(Row::Card(self.card(only, depth)));
            } else {
                let items = cluster.iter().map(|id| self.card(id, depth)).collect();
                rows.push(Row::Simultaneous { depth, items });
            }
            for &id in &cluster {
                span_of.insert(id.to_string(), (row_idx, row_idx + 1));
            }
            for &id in &cluster {
                if let Some(kids) = self.children.get(id) {
                    self.emit(kids, depth + 1, rows, span_of);
                }
                span_of.get_mut(id).unwrap().1 = rows.len();
            }
        }
    }
}

pub fn layout(input: &SpineInput) -> Spine {
    let SpineInput { notes, filter, calendar, selected, rule } = *input;
    let entities: HashSet<&str> = notes.iter().filter(|n| n.is_entity).map(|n| n.id.as_str()).collect();
    let by_id: HashMap<&str, &Note> = notes.iter().map(|n| (n.id.as_str(), n)).collect();

    let rib = tl::ribbon_of(notes, calendar, rule);
    let shown = tl::shown_events(notes, filter, &rib);

    let mut children: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut roots: Vec<&str> = Vec::new();
    for id in shown.keys() {
        match rib.parent.get(id) {
            Some(p) if shown.contains_key(p.as_str()) => {
                children.entry(p.as_str()).or_default().push(id.as_str())
            }
            _ => roots.push(id.as_str()),
        }
    }
    let sort_key = |id: &str| -> (i64, i64, String, String) {
        let d = &rib.drawn[id];
        (d.start, d.end.unwrap_or(i64::MAX), fold_token(&by_id[id].title), id.to_string())
    };
    roots.sort_by(|a, b| sort_key(a).cmp(&sort_key(b)));
    for kids in children.values_mut() {
        kids.sort_by(|a, b| sort_key(a).cmp(&sort_key(b)));
    }

    let ctx = Ctx {
        rib: &rib,
        by_id: &by_id,
        children: &children,
        entities: &entities,
        shown: &shown,
        rule,
        calendar,
        notes,
        selected,
    };
    let mut rows: Vec<Row> = Vec::new();
    let mut span_of: HashMap<String, (usize, usize)> = HashMap::new();
    ctx.emit(&roots, 0, &mut rows, &mut span_of);
    let drawn_count = rows.len();

    // Notes with no span: anchored against a drawn event (`relative`, resolved), or
    // not. Plain lore — neither a relation nor a `$ref` — is excluded exactly as the
    // desktop holding rail excludes it.
    let mut relative_by_target: HashMap<String, Vec<&Note>> = HashMap::new();
    let mut not_yet_dated: Vec<&Note> = Vec::new();
    for n in notes
        .iter()
        .filter(|n| n.span.is_none() && !n.is_entity && filter.matches(n) && !rib.drawn.contains_key(&n.id))
    {
        match &n.relative {
            Some(rel) if span_of.contains_key(&rel.note_id) => {
                relative_by_target.entry(rel.note_id.clone()).or_default().push(n);
            }
            Some(_) => not_yet_dated.push(n),
            None if !participants(n, &entities).is_empty() => not_yet_dated.push(n),
            None => {}
        }
    }

    let mut before_at: HashMap<usize, Vec<Card>> = HashMap::new();
    let mut after_at: HashMap<usize, Vec<Card>> = HashMap::new();
    for (target, group) in relative_by_target {
        let (start, end) = span_of[&target];
        let depth = row_depth(&rows, start);
        let mut befores: Vec<&Note> = Vec::new();
        let mut afters: Vec<&Note> = Vec::new();
        for n in group {
            match n.relative.as_ref().unwrap().relation {
                TimeRelation::Before => befores.push(n),
                TimeRelation::After | TimeRelation::During => afters.push(n),
            }
        }
        let by_title = |a: &&Note, b: &&Note| fold_token(&a.title).cmp(&fold_token(&b.title)).then(a.id.cmp(&b.id));
        befores.sort_by(by_title);
        afters.sort_by(by_title);
        before_at.entry(start).or_default().extend(befores.iter().map(|n| ctx.relative_card(n, depth)));
        after_at.entry(end).or_default().extend(afters.iter().map(|n| ctx.relative_card(n, depth)));
    }

    let mut final_rows: Vec<Row> = Vec::with_capacity(rows.len());
    let mut rows_iter = rows.into_iter();
    for i in 0..=drawn_count {
        if let Some(v) = after_at.remove(&i) {
            final_rows.extend(v.into_iter().map(Row::Card));
        }
        if let Some(v) = before_at.remove(&i) {
            final_rows.extend(v.into_iter().map(Row::Card));
        }
        if i < drawn_count {
            final_rows.push(rows_iter.next().expect("drawn_count is rows' original length"));
        }
    }

    let mut not_yet_dated_cards: Vec<Loose> = not_yet_dated
        .into_iter()
        .map(|n| {
            let who = participants(n, &entities);
            let who_titles = ctx.who_titles(&who);
            Loose {
                id: n.id.clone(),
                title: n.title.clone(),
                on: ctx.lit(&n.id, &who),
                who_titles,
                who,
            }
        })
        .collect();
    not_yet_dated_cards.sort_by(|a, b| fold_token(&a.title).cmp(&fold_token(&b.title)).then(a.id.cmp(&b.id)));

    Spine {
        undated: final_rows.is_empty(),
        rows: final_rows,
        not_yet_dated: not_yet_dated_cards,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::notes_filter::Term;
    use plotweb_common::{LinkTarget, NoteLink, RelativeTime, TimeSpan};

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
            links: Default::default(),
        }
    }

    fn at(y: i64) -> TimeSpan {
        TimeSpan::at(TimePoint::base_unit(y))
    }

    fn span(a: i64, b: i64) -> TimeSpan {
        TimeSpan { start: TimePoint::base_unit(a), end: Some(TimePoint::base_unit(b)), approximate: false, open_ended: false }
    }

    fn dated(id: &str, title: &str, s: TimeSpan) -> Note {
        Note { span: Some(s), ..note(id, title) }
    }

    fn entity(id: &str, title: &str) -> Note {
        Note { is_entity: true, ..note(id, title) }
    }

    fn refs(note: &mut Note, ids: &[&str]) {
        note.links.refs = ids
            .iter()
            .map(|id| NoteLink { text: id.to_string(), target: LinkTarget::Note, id: Some(id.to_string()) })
            .collect();
    }

    fn cal() -> Calendar {
        Calendar::default()
    }

    fn input(notes: &[Note]) -> Spine {
        layout(&SpineInput {
            notes,
            filter: &Filter::default(),
            calendar: &cal(),
            selected: None,
            rule: SpanRule::Fit,
        })
    }

    fn card_ids(spine: &Spine) -> Vec<(String, usize)> {
        spine
            .rows
            .iter()
            .flat_map(|r| match r {
                Row::Card(c) => vec![(c.id.clone(), c.depth)],
                Row::Simultaneous { depth, items } => items.iter().map(|c| (c.id.clone(), *depth)).collect(),
            })
            .collect()
    }

    #[test]
    fn events_are_ordered_top_to_bottom_by_start() {
        let notes = vec![
            dated("b", "Winter", at(1208)),
            dated("a", "Harrowgate", at(1204)),
        ];
        let spine = input(&notes);
        assert_eq!(card_ids(&spine), vec![("a".into(), 0), ("b".into(), 0)]);
    }

    #[test]
    fn nesting_is_indentation_and_follows_event_parent() {
        let mut siege = dated("siege", "Siege", span(1206, 1207));
        let mut breach = dated("breach", "Breach", at(1206));
        breach.event_parent = Some("siege".into());
        let spine = input(&[siege.clone(), breach.clone()]);
        assert_eq!(card_ids(&spine), vec![("siege".into(), 0), ("breach".into(), 1)]);

        // A cycle is broken exactly as the ribbon breaks it: each member draws as a
        // root rather than hanging the layout (and, freed of their nesting, the siege
        // and the breach now overlap in time, so they land in one simultaneous row —
        // an emergent consequence of breaking the cycle, not a special case here).
        siege.event_parent = Some("breach".into());
        breach.event_parent = Some("siege".into());
        let spine = input(&[siege, breach]);
        let depths: Vec<usize> = card_ids(&spine).into_iter().map(|(_, d)| d).collect();
        assert_eq!(depths, vec![0, 0], "a cycle draws every member as a root");
    }

    #[test]
    fn a_dateless_parent_is_fitted_and_shown_with_a_coarse_date() {
        let mut chapter = note("act2", "Act Two");
        chapter.span = None;
        let mut siege = dated("siege", "Siege", at(1206));
        siege.event_parent = Some("act2".into());
        let spine = input(&[chapter, siege]);
        let Row::Card(top) = &spine.rows[0] else { panic!("expected a card") };
        assert_eq!(top.id, "act2");
        assert_eq!(top.depth, 0);
        assert!(top.when.contains("1206"), "{}", top.when);
    }

    #[test]
    fn clamp_shows_a_cut_at_parent_badge_and_free_shows_outside_parent() {
        let siege = dated("siege", "Siege", span(10, 20));
        let mut breach = dated("breach", "Breach", span(15, 25));
        breach.event_parent = Some("siege".into());
        let notes = [siege, breach];

        let spine = layout(&SpineInput {
            notes: &notes,
            filter: &Filter::default(),
            calendar: &cal(),
            selected: None,
            rule: SpanRule::Clamp,
        });
        let Row::Card(child) = &spine.rows[1] else { panic!() };
        assert_eq!(child.badge, Some(Badge::Clamped));
        assert_eq!(child.badge.unwrap().label(), "cut at parent");

        let spine = layout(&SpineInput {
            notes: &notes,
            filter: &Filter::default(),
            calendar: &cal(),
            selected: None,
            rule: SpanRule::Free,
        });
        let Row::Card(child) = &spine.rows[1] else { panic!() };
        assert_eq!(child.badge, Some(Badge::Escapes));
        assert_eq!(child.badge.unwrap().label(), "outside parent");

        // Fit never clips or escapes: the parent grows to contain the child.
        let spine = input(&notes);
        let Row::Card(child) = &spine.rows[1] else { panic!() };
        assert_eq!(child.badge, None);
    }

    #[test]
    fn overlapping_siblings_are_one_simultaneous_row_and_nested_ones_are_not() {
        // The parley and Maera's move overlap and share no parent: paired.
        let parley = dated("parley", "Parley", span(10, 12));
        let maera = dated("maera", "Maera takes the Needle", span(11, 13));
        // A third, non-overlapping event stays solo.
        let later = dated("later", "Later", span(30, 31));
        let spine = input(&[parley, maera, later]);
        assert_eq!(spine.rows.len(), 2);
        assert!(matches!(&spine.rows[0], Row::Simultaneous { items, .. } if items.len() == 2));
        assert!(matches!(&spine.rows[1], Row::Card(c) if c.id == "later"));
    }

    #[test]
    fn three_way_overlap_chains_into_one_group() {
        // A(0-5) overlaps B(3-8) overlaps C(7-12); A and C do not overlap each other.
        let a = dated("a", "A", span(0, 5));
        let b = dated("b", "B", span(3, 8));
        let c = dated("c", "C", span(7, 12));
        let spine = input(&[a, b, c]);
        assert_eq!(spine.rows.len(), 1);
        assert!(matches!(&spine.rows[0], Row::Simultaneous { items, .. } if items.len() == 3));
    }

    #[test]
    fn nested_events_are_never_folded_into_a_simultaneous_row() {
        // The breach happens entirely inside the siege's span — contained, not paired.
        let siege = dated("siege", "Siege", span(0, 100));
        let mut breach = dated("breach", "Breach", span(10, 20));
        breach.event_parent = Some("siege".into());
        let spine = input(&[siege, breach]);
        assert!(spine.rows.iter().all(|r| matches!(r, Row::Card(_))), "no pairing across nesting levels");
    }

    #[test]
    fn a_relative_note_sits_after_its_targets_whole_subtree() {
        let siege = dated("siege", "Siege", span(1206, 1207));
        let mut breach = dated("breach", "Breach", at(1206));
        breach.event_parent = Some("siege".into());
        let aftermath = Note { relative: Some(RelativeTime { relation: TimeRelation::After, note_id: "siege".into() }), ..note("aftermath", "Aftermath") };
        let spine = input(&[siege, breach, aftermath]);
        // siege, breach (nested), aftermath — after the whole subtree, not wedged
        // between the siege and its own child.
        assert_eq!(card_ids(&spine), vec![("siege".into(), 0), ("breach".into(), 1), ("aftermath".into(), 0)]);
    }

    #[test]
    fn a_before_note_sits_right_before_its_target() {
        let siege = dated("siege", "Siege", at(1206));
        let prelude = Note { relative: Some(RelativeTime { relation: TimeRelation::Before, note_id: "siege".into() }), ..note("prelude", "Prelude") };
        let spine = input(&[siege, prelude]);
        assert_eq!(card_ids(&spine), vec![("prelude".into(), 0), ("siege".into(), 0)]);
    }

    #[test]
    fn during_is_placed_like_after() {
        let siege = dated("siege", "Siege", span(1206, 1207));
        let scene = Note { relative: Some(RelativeTime { relation: TimeRelation::During, note_id: "siege".into() }), ..note("scene", "A scene") };
        let spine = input(&[siege, scene]);
        assert_eq!(card_ids(&spine), vec![("siege".into(), 0), ("scene".into(), 0)]);
    }

    #[test]
    fn undated_refd_notes_go_to_not_yet_dated_and_plain_lore_is_excluded() {
        let mut refd = note("refd", "A scene with Vess");
        let vess = entity("vess", "Vess");
        refs(&mut refd, &["vess"]);
        let lore = note("lore", "Plain lore");
        let spine = input(&[refd, vess, lore]);
        assert_eq!(spine.not_yet_dated.len(), 1);
        assert_eq!(spine.not_yet_dated[0].id, "refd");
        assert_eq!(spine.not_yet_dated[0].who_titles, vec!["Vess".to_string()]);
    }

    #[test]
    fn a_relative_note_whose_target_is_not_drawn_falls_back_to_not_yet_dated() {
        // "after" a note that is itself plain lore — never drawn — has nothing to
        // anchor to.
        let lore = note("lore", "Plain lore");
        let orphan = Note { relative: Some(RelativeTime { relation: TimeRelation::After, note_id: "lore".into() }), ..note("orphan", "Orphan") };
        let spine = input(&[lore, orphan]);
        assert!(spine.rows.is_empty());
        assert_eq!(spine.not_yet_dated.len(), 1);
        assert_eq!(spine.not_yet_dated[0].id, "orphan");
    }

    #[test]
    fn the_filter_narrows_events_and_mutes_ancestors_exactly_as_the_ribbon_does() {
        let mut siege = dated("siege", "Siege", span(1206, 1207));
        siege.links.tags = vec!["war".into()];
        let mut breach = dated("breach", "Breach", at(1206));
        breach.event_parent = Some("siege".into());
        let notes = [siege, breach];
        let filter = Filter::default().cycled(&Term::Tag("war".into()));
        let spine = layout(&SpineInput { notes: &notes, filter: &filter, calendar: &cal(), selected: None, rule: SpanRule::Fit });
        assert_eq!(spine.rows.len(), 1, "the breach does not match #war");
        let Row::Card(siege_card) = &spine.rows[0] else { panic!() };
        assert!(!siege_card.muted, "the siege itself matches");
    }

    #[test]
    fn selecting_an_entity_highlights_the_cards_it_is_in() {
        let vess = entity("vess", "Vess");
        let mut siege = dated("siege", "Siege", at(1206));
        refs(&mut siege, &["vess"]);
        let notes = [vess, siege];
        let spine = layout(&SpineInput { notes: &notes, filter: &Filter::default(), calendar: &cal(), selected: Some("vess"), rule: SpanRule::Fit });
        let Row::Card(siege_card) = &spine.rows[0] else { panic!() };
        assert!(siege_card.on);
        assert_eq!(siege_card.who_titles, vec!["Vess".to_string()]);
    }

    #[test]
    fn nothing_dated_is_reported_undated() {
        let spine = input(&[note("lore", "Just lore")]);
        assert!(spine.undated);
        assert!(spine.rows.is_empty());
    }
}
