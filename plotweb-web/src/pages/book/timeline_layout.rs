//! The timeline laid out — every rule of the drawing, none of the DOM.
//!
//! `design/04-notes-wireframes.html`, "the chosen shape": two zones on one x axis.
//!
//! * The **event ribbon** (card 6) on top: a bar per event, nested events in sub-rows
//!   inside their parent's containment band. Its rules — nesting, the book's span rule,
//!   packing, label placement — live in [`super::ribbon`]; this module puts them on the
//!   axis.
//! * The **entity lanes** (card 5) below: one horizontal lane per entity, and each event
//!   a vertical tie joining the lanes of everyone it `$ref`s, with a dot per
//!   participant. With the ribbon on, each tie rises out of its event's bar, so the two
//!   zones read as one drawing.
//!
//! Either zone can be switched off. Lanes alone are card 5's view, captions and all —
//! except that an event naming no one is no longer drawn in a "no one" row: it lives in
//! the ribbon, and the status line counts it while the ribbon is off.
//!
//! Pure functions over the note list, host-tested like [`super::notes_filter`],
//! [`super::sigils`] and [`super::time_entry`]. `panes::timeline` turns a [`Layout`]
//! into SVG and does nothing else, so every decision below — which ticks, which lanes
//! in which order, which caption tier, which row of the ribbon — is one a test can pin.
//!
//! # Drawn from the structure document alone
//!
//! Everything read here — span, relative constraint, entity mark, `$ref` edges, title —
//! is mirrored into the `book:` structure document by card 1 and projected onto
//! `store.notes` by `local_book::project_notes`. A note's `content` is never read, so
//! drawing the timeline never opens a `note:{id}` body document.
//!
//! # The filter, applied the way the tree applies it
//!
//! The tree keeps a non-matching ancestor of a match, dimmed, because without it the
//! match has nowhere to hang. The lanes have the same shape of problem: an event the
//! filter keeps needs the lanes of the people in it, whether or not they match. So:
//!
//! * an **event** is drawn when it matches the filter;
//! * an **entity** gets a lane when it matches, *or* when a drawn event names it — and
//!   in the second case the lane is **muted**, the tree's word for "the path to a
//!   result, not a result".
//!
//! This is also the answer to a large cast (`design/05-notes-build-plan.md`, card 5):
//! lanes are **driven by the shared filter**, not by a separate pinned set. A pinned set
//! would be a second narrowing that the tree does not know about, so the same notes
//! would read as "in scope" in one view and not the other; the filter already exists,
//! already sits above the view switcher, and a `#house-vaun` chip is exactly "the lanes
//! I care about right now".

use std::collections::{HashMap, HashSet};

use std::collections::BTreeMap;

use plotweb_common::{fold_token, Calendar, LinkTarget, Note, SpanRule, TimePoint, TimeSpan};

use super::notes_filter::Filter;
use super::ribbon::{self, LabelSpot};

/// The drawing's own coordinate width. The SVG scales to its container, so this is a
/// unit of layout, not a pixel count on screen.
pub const W: f32 = 900.0;
/// Room left of the plot for lane names.
pub const PAD_L: f32 = 132.0;
pub const PAD_R: f32 = 24.0;
/// The time axis's baseline.
const AXIS_Y: f32 = 24.0;
const TIER_H: f32 = 13.0;
/// Caption tiers before stacking gives up. Past this a caption shares the least
/// crowded tier rather than pushing the lanes off the bottom of the screen.
pub const MAX_TIERS: usize = 6;
const LANE_H: f32 = 30.0;
/// One ribbon row: a bar's height, and the pitch from one row to the next.
pub const ROW_H: f32 = 20.0;
const ROW_PITCH: f32 = 24.0;
/// Where the ribbon starts, under the axis.
const RIBBON_TOP: f32 = AXIS_Y + 12.0;
/// Between the ribbon and the first lane — room for the zone rule.
const ZONE_GAP: f32 = 18.0;
/// The least room between two blocks on one ribbon row.
const PACK_GAP: f32 = 6.0;
/// At most this many ticks across the plot — ~60 drawing units apiece, enough for
/// "Mar" or "day 12" without the labels running together.
pub const MAX_TICKS: usize = 12;
/// A band narrower than this is drawn at this width, so an instant is still a mark.
const MIN_BAND: f32 = 5.0;

/// Rough advance width of one character at the caption size, in drawing units.
/// Captions are fitted, not measured: there is no layout engine to ask on the host, and
/// an estimate that errs wide only costs a little stagger.
const CAP_CH: f32 = 5.6;
/// The same for the monospace tick labels.
const TICK_CH: f32 = 6.0;
/// The same for lane names.
const LANE_CH: f32 = 6.2;

pub fn plot_x0() -> f32 {
    PAD_L
}
pub fn plot_x1() -> f32 {
    W - PAD_R
}

/// How the lanes are stacked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LaneOrder {
    /// By the first moment each entity appears on the line — the reading order of the
    /// book's events, and the default.
    #[default]
    FirstAppearance,
    Alphabetical,
}

impl LaneOrder {
    pub fn toggled(self) -> Self {
        match self {
            LaneOrder::FirstAppearance => LaneOrder::Alphabetical,
            LaneOrder::Alphabetical => LaneOrder::FirstAppearance,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            LaneOrder::FirstAppearance => "order: by first appearance",
            LaneOrder::Alphabetical => "order: alphabetical",
        }
    }
}

/// One tick on the time axis.
#[derive(Debug, Clone, PartialEq)]
pub struct Tick {
    pub x: f32,
    /// Empty when the label would collide with the one before it — the tick line is
    /// still drawn.
    pub label: String,
    /// The tick where a coarser part changes ("1817 Jan" after "Dec"): drawn a shade
    /// stronger, and always labelled in full.
    pub major: bool,
}

/// One entity's lane.
#[derive(Debug, Clone, PartialEq)]
pub struct Lane {
    pub id: String,
    pub title: String,
    /// `title`, cut to fit the name column.
    pub label: String,
    pub y: f32,
    /// Here only because a drawn event names this entity; the entity itself does not
    /// match the filter.
    pub muted: bool,
    /// The selected note.
    pub on: bool,
}

/// An entity that is also dated — a character with a lifespan — drawn along its own
/// lane rather than as a tie.
#[derive(Debug, Clone, PartialEq)]
pub struct Life {
    pub id: String,
    pub x0: f32,
    pub x1: f32,
    pub y: f32,
    pub fuzzy: bool,
    pub open: bool,
    pub on: bool,
}

/// A participant's dot on an event's tie.
#[derive(Debug, Clone, PartialEq)]
pub struct Dot {
    pub lane: String,
    pub y: f32,
    pub on: bool,
}

/// One dated event.
#[derive(Debug, Clone, PartialEq)]
pub struct Mark {
    pub id: String,
    pub title: String,
    /// The band: from the start to the end of the event (an instant spans the unit it is
    /// known to — "1206" covers the year, "1206 Mar 4" the day).
    pub x0: f32,
    pub x1: f32,
    /// Where the tie runs.
    pub cx: f32,
    /// The tie's extent: from the bottom of the event's bar when the ribbon is on (the
    /// tie rises into it), else from the topmost participating lane, down to the
    /// bottommost one.
    pub y0: f32,
    pub y1: f32,
    /// The topmost participating lane — where the band behind the tie starts.
    pub lane_y0: f32,
    pub fuzzy: bool,
    pub open: bool,
    /// The selected note is this event, or takes part in it.
    pub on: bool,
    pub dots: Vec<Dot>,
}

/// One event's bar on the ribbon.
#[derive(Debug, Clone, PartialEq)]
pub struct Bar {
    pub id: String,
    pub title: String,
    pub x0: f32,
    pub x1: f32,
    /// The top of the bar.
    pub y: f32,
    pub row: usize,
    /// How deep it is nested (0 = a root).
    pub depth: usize,
    /// Drawn only because an event inside it matches the filter.
    pub muted: bool,
    pub fuzzy: bool,
    pub open: bool,
    /// Drawn wider than its typed dates (auto-fit).
    pub fitted: bool,
    /// No dates of its own; drawn from its children.
    pub derived: bool,
    pub pinned: bool,
    pub clipped_start: bool,
    pub clipped_end: bool,
    pub escapes_start: bool,
    pub escapes_end: bool,
    /// The entities it `$ref`s — what lights it along with a followed lane.
    pub who: Vec<String>,
    pub on: bool,
}

impl Bar {
    /// A few words on how the rule drew it, for the tooltip — empty when it is drawn as
    /// typed.
    pub fn note(&self) -> String {
        let mut out: Vec<&str> = Vec::new();
        if self.derived {
            out.push("no dates of its own \u{2014} drawn across what it contains");
        } else if self.fitted {
            out.push("stretched to fit what it contains");
        }
        if self.clipped_start || self.clipped_end {
            out.push("cut off at the edge of the event it is in");
        }
        if self.escapes_start || self.escapes_end {
            out.push("runs outside the event it is in");
        }
        if self.pinned {
            out.push("pinned");
        }
        out.join("; ")
    }
}

/// A parent's containment band, behind its children's sub-rows.
#[derive(Debug, Clone, PartialEq)]
pub struct Band {
    pub id: String,
    pub x0: f32,
    pub x1: f32,
    pub y0: f32,
    pub y1: f32,
    pub depth: usize,
}

/// A bar's label, where [`ribbon::place_labels`] put it.
#[derive(Debug, Clone, PartialEq)]
pub struct BarLabel {
    pub id: String,
    pub text: String,
    pub x: f32,
    /// Baseline.
    pub y: f32,
    /// Anchored at its end (the gap to the left of the bar).
    pub end: bool,
    /// Inside the bar rather than beside it.
    pub inside: bool,
    pub muted: bool,
    /// The bar's people — the label lights with its bar.
    pub who: Vec<String>,
}

/// A caption above the lanes, with its leader line down to the mark it names.
#[derive(Debug, Clone, PartialEq)]
pub struct Caption {
    pub id: String,
    pub text: String,
    /// Where the text starts.
    pub x: f32,
    /// Its baseline.
    pub y: f32,
    pub tier: usize,
    /// The leader: a vertical at `leader_x` from just under the caption down to the mark.
    pub leader_x: f32,
    pub leader_y0: f32,
    pub leader_y1: f32,
    pub on: bool,
}

/// A note waiting in the holding rail: it happens, but not yet anywhere on the line.
#[derive(Debug, Clone, PartialEq)]
pub struct Held {
    pub id: String,
    pub title: String,
    /// Why it is here: the relative constraint as typed ("after The siege"), or
    /// "undated".
    pub why: String,
}

/// The whole drawing.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Layout {
    pub height: f32,
    /// The visible span, in ticks.
    pub lo: i64,
    pub hi: i64,
    /// Which calendar unit the ticks are (0 = the base unit), and every how many.
    pub tick_level: usize,
    pub tick_step: i64,
    pub ticks: Vec<Tick>,
    pub lanes: Vec<Lane>,
    pub lives: Vec<Life>,
    pub marks: Vec<Mark>,
    pub captions: Vec<Caption>,
    /// Captions that found no free tier and share one. Drawn anyway — a caption that
    /// overprints is legible on hover and in the note list, one that vanishes is not.
    pub crowded: usize,
    /// The ribbon: bars, containment bands, labels. Empty when the ribbon is off.
    pub bars: Vec<Bar>,
    pub bands: Vec<Band>,
    pub labels: Vec<BarLabel>,
    /// Bars whose label had nowhere to go. Counted aloud rather than hidden.
    pub dropped_labels: usize,
    /// The ribbon zone's top and bottom, when it is on — the "take it out" drop target.
    pub ribbon_y: Option<(f32, f32)>,
    /// Whether the lanes zone is drawn. The lanes are laid out regardless — they are
    /// also the follow chips.
    pub lanes_on: bool,
    /// Drawn events that name no entity. Only the ribbon has somewhere to put them.
    pub no_one: usize,
    pub held: Vec<Held>,
    /// Nothing matched is dated: the axis is a placeholder, there to drop onto.
    pub undated: bool,
}

impl Layout {
    /// The time under `frac` of the plot's width (0 = left edge), snapped to the start
    /// of the tick unit — a drop is exactly as precise as the ruler it lands on.
    pub fn point_at(&self, calendar: &Calendar, frac: f32) -> TimePoint {
        let frac = frac.clamp(0.0, 1.0) as f64;
        let tick = self.lo as f64 + frac * (self.hi - self.lo) as f64;
        let level = self.tick_level;
        let n = calendar.index_at(level, tick.round() as i64);
        TimePoint {
            tick: calendar.start_tick(level, n),
            precision: level as u8,
        }
    }

    #[cfg(test)]
    pub fn x_of(&self, tick: i64) -> f32 {
        x_of(self.lo, self.hi, tick)
    }
}

fn x_of(lo: i64, hi: i64, tick: i64) -> f32 {
    let w = (hi - lo).max(1) as f64;
    plot_x0() + (((tick - lo) as f64 / w) as f32) * (plot_x1() - plot_x0())
}

/// Everything the layout reads.
pub struct Input<'a> {
    pub notes: &'a [Note],
    pub filter: &'a Filter,
    pub calendar: &'a Calendar,
    pub order: LaneOrder,
    pub selected: Option<&'a str>,
    /// The book's span rule.
    pub rule: SpanRule,
    /// Which zones are on. At least one always is: both off reads as both on.
    pub ribbon: bool,
    pub lanes: bool,
}

// ── Time extents ─────────────────────────────────────────────────────────────

/// The first tick after the unit `point` is known to — the end of "1206" is the start
/// of 1207. An instant is drawn across that whole unit rather than as a hairline at its
/// first tick, which would read as a date nobody typed.
fn end_of_unit(calendar: &Calendar, point: &TimePoint) -> i64 {
    let level = (point.precision as usize).min(calendar.units.len().saturating_sub(1));
    let n = calendar.index_at(level, point.tick);
    calendar.start_tick(level, n.saturating_add(1)).max(point.tick + 1)
}

/// A span's extent in ticks: `(start, end)`, with `end = None` for open-ended.
fn extent(calendar: &Calendar, span: &TimeSpan) -> (i64, Option<i64>) {
    let start = span.start.tick;
    let end = match (&span.end, span.open_ended) {
        (Some(end), _) => Some(end_of_unit(calendar, end).max(start + 1)),
        (None, true) => None,
        (None, false) => Some(end_of_unit(calendar, &span.start)),
    };
    (start, end)
}

// ── Ticks ────────────────────────────────────────────────────────────────────

/// The next "nice" step at or above `raw`: 1, 2, 5, 10, 20, 50, …
fn nice_step(raw: f64) -> i64 {
    let mut base = 1i64;
    loop {
        for m in [1, 2, 5] {
            let s = base.saturating_mul(m);
            if s as f64 >= raw {
                return s.max(1);
            }
        }
        if base > i64::MAX / 10 {
            return i64::MAX;
        }
        base *= 10;
    }
}

/// Which calendar unit the axis ticks in, and every how many.
///
/// Each unit's "shown in timeline when" rule ([`Calendar::shown_at`]) decides whether it
/// may appear at all for the span on screen; of those, the **finest** one that fits
/// [`MAX_TICKS`] across the plot is used. A rule that allows a unit is permission, not
/// an order: the default calendar shows Days below a one-year span, but eleven months
/// of days would be 330 ticks, so the Month is used until the span is short enough.
/// If even the coarsest allowed unit is too dense (five centuries of Years), it is
/// stepped by 1/2/5 × 10ⁿ.
pub fn choose_tick_unit(calendar: &Calendar, span_ticks: f64) -> (usize, i64) {
    let levels = calendar.units.len();
    let shown: Vec<usize> = (0..levels).filter(|&l| calendar.shown_at(l, span_ticks)).collect();
    for &level in shown.iter().rev() {
        if span_ticks / calendar.unit_ticks(level) <= MAX_TICKS as f64 {
            return (level, 1);
        }
    }
    let level = shown.first().copied().unwrap_or(0);
    let raw = span_ticks / calendar.unit_ticks(level) / MAX_TICKS as f64;
    (level, nice_step(raw))
}

/// The ticks for `lo..=hi`, labelled through the calendar.
///
/// A tick is labelled with only its own part ("Mar", "day 12") except where a coarser
/// part changes — and at the first tick — where it reads in full ("1817 Jan"), so the
/// axis can always be read from its left edge. A label that would run into the one
/// before it is dropped; the tick line stays.
pub fn ticks(calendar: &Calendar, lo: i64, hi: i64) -> (usize, i64, Vec<Tick>) {
    let (level, step) = choose_tick_unit(calendar, (hi - lo) as f64);
    let mut out = Vec::new();
    let mut n = calendar.index_at(level, lo);
    if calendar.start_tick(level, n) < lo {
        n += 1;
    }
    let rem = n.rem_euclid(step);
    if rem != 0 {
        n = n.saturating_add(step - rem);
    }
    let mut prev_prefix: Option<Vec<i64>> = None;
    let mut label_end = f32::MIN;
    // Bounded: a malformed span must not spin here.
    for _ in 0..(MAX_TICKS * 4 + 4) {
        let tick = calendar.start_tick(level, n);
        if tick > hi {
            break;
        }
        let point = TimePoint {
            tick,
            precision: level as u8,
        };
        let parts = calendar.components(&point);
        let prefix = parts[..parts.len().saturating_sub(1)].to_vec();
        let major = level == 0 || prev_prefix.as_ref().is_some_and(|p| *p != prefix);
        let full = prev_prefix.is_none() || major;
        let text = if full {
            calendar.format_point(&point)
        } else {
            calendar.format_part(&point)
        };
        let x = x_of(lo, hi, tick);
        let width = text.chars().count() as f32 * TICK_CH;
        let label = if x >= label_end + 6.0 && x + width <= W {
            label_end = x + 3.0 + width;
            text
        } else {
            String::new()
        };
        out.push(Tick { x, label, major });
        prev_prefix = Some(prefix);
        n = match n.checked_add(step) {
            Some(n) => n,
            None => break,
        };
    }
    (level, step, out)
}

// ── Lanes ────────────────────────────────────────────────────────────────────

/// The entities an event puts on the line: every `$ref` that resolves to an entity,
/// in the order the body names them, once each. `@` mentions stay quiet here — only
/// `$` is participation (`design/05-notes-build-plan.md`, "Decisions taken").
pub fn participants(note: &Note, entities: &HashSet<&str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for link in &note.links.refs {
        if link.target != LinkTarget::Note {
            continue;
        }
        let Some(id) = link.id.as_deref() else { continue };
        if id == note.id || !entities.contains(id) || out.iter().any(|o| o == id) {
            continue;
        }
        out.push(id.to_string());
    }
    out
}

/// Order lanes. `first` is each entity's first appearance on the line, if it has one;
/// entities that never appear go last. Ties — and the whole of the alphabetical
/// order — fall back to the title, case-folded, and then the id so the order is total.
pub fn order_lanes(
    lanes: &mut [(String, String)],
    first: &HashMap<String, i64>,
    order: LaneOrder,
) {
    lanes.sort_by(|(a_id, a_title), (b_id, b_title)| {
        let by_title = fold_token(a_title)
            .cmp(&fold_token(b_title))
            .then_with(|| a_id.cmp(b_id));
        match order {
            LaneOrder::Alphabetical => by_title,
            LaneOrder::FirstAppearance => {
                let fa = first.get(a_id);
                let fb = first.get(b_id);
                match (fa, fb) {
                    (Some(a), Some(b)) => a.cmp(b).then(by_title),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => by_title,
                }
            }
        }
    });
}

// ── Captions ─────────────────────────────────────────────────────────────────

/// Staggered caption tiers. `spans` are `(start, end)` in x, **in left-to-right
/// order**; each goes into the lowest tier whose last caption ends before it starts.
/// When all [`MAX_TIERS`] are taken at that x, it shares the tier that frees up
/// soonest — overprinting — and is counted in the second return value.
///
/// The wireframe's technique: a dense moment (six events in one season) stacks upward
/// into readable rows with leader lines instead of printing six titles on one spot.
pub fn assign_tiers(spans: &[(f32, f32)]) -> (Vec<usize>, usize) {
    let mut ends: Vec<f32> = Vec::new();
    let mut out = Vec::with_capacity(spans.len());
    let mut crowded = 0;
    for &(start, end) in spans {
        let free = ends.iter().position(|&e| e <= start);
        let tier = match free {
            Some(t) => t,
            None if ends.len() < MAX_TIERS => {
                ends.push(f32::MIN);
                ends.len() - 1
            }
            None => {
                crowded += 1;
                ends.iter()
                    .enumerate()
                    .min_by(|a, b| a.1.total_cmp(b.1))
                    .map(|(i, _)| i)
                    .unwrap_or(0)
            }
        };
        ends[tier] = end;
        out.push(tier);
    }
    (out, crowded)
}

/// `text`, cut with an ellipsis to fit `room` at `ch` units a character.
pub fn fit(text: &str, room: f32, ch: f32) -> String {
    let max = (room / ch).floor().max(0.0) as usize;
    let count = text.chars().count();
    if count <= max {
        return text.to_string();
    }
    if max <= 1 {
        return String::new();
    }
    let mut out: String = text.chars().take(max - 1).collect();
    out = out.trim_end().to_string();
    out.push('\u{2026}');
    out
}

// ── The layout ───────────────────────────────────────────────────────────────

/// A dated event, before it has a place on screen.
struct Placed<'a> {
    note: &'a Note,
    start: i64,
    end: Option<i64>,
    who: Vec<String>,
}

/// The ribbon's view of the book: every event under the book's rule, before the filter.
///
/// Which notes are events here: every non-entity note with a span, and every non-entity
/// note that contains one through `event_parent` (a parent with no dates of its own is
/// fitted from its children). A dated entity is a lifespan on its lane, not a bar, and
/// an `event_parent` pointing at an entity — or at nothing — is not drawn as nesting.
pub struct Ribbon {
    pub drawn: BTreeMap<String, ribbon::Drawn>,
    /// Cycle-free, and naming only events.
    pub parent: HashMap<String, String>,
}

pub fn ribbon_of(notes: &[Note], calendar: &Calendar, rule: SpanRule) -> Ribbon {
    let by_id: HashMap<&str, &Note> = notes.iter().map(|n| (n.id.as_str(), n)).collect();
    let eligible = |id: &str| by_id.get(id).is_some_and(|n| !n.is_entity);
    let raw: HashMap<String, String> = notes
        .iter()
        .filter(|n| !n.is_entity)
        .filter_map(|n| {
            let p = n.event_parent.as_ref()?;
            eligible(p).then(|| (n.id.clone(), p.clone()))
        })
        .collect();
    let parent = ribbon::break_cycles(&raw);

    // The dated events, then every ancestor of one.
    let mut nodes: BTreeMap<String, ribbon::Node> = BTreeMap::new();
    for n in notes.iter().filter(|n| !n.is_entity) {
        if let Some(span) = &n.span {
            nodes.insert(
                n.id.clone(),
                ribbon::Node {
                    typed: Some(extent(calendar, span)),
                    pinned: n.pinned,
                },
            );
        }
    }
    let dated: Vec<String> = nodes.keys().cloned().collect();
    for id in dated {
        let mut cur = id;
        // `parent` is a forest now, so this ends; the bound is belt and braces.
        for _ in 0..notes.len() {
            let Some(p) = parent.get(&cur) else { break };
            if !nodes.contains_key(p) {
                let pinned = by_id.get(p.as_str()).is_some_and(|n| n.pinned);
                nodes.insert(p.clone(), ribbon::Node { typed: None, pinned });
            }
            cur = p.clone();
        }
    }
    let parent: HashMap<String, String> =
        parent.into_iter().filter(|(c, _)| nodes.contains_key(c)).collect();
    let drawn = ribbon::resolve(&nodes, &parent, rule);
    Ribbon { drawn, parent }
}

pub fn layout(input: &Input) -> Layout {
    let Input {
        notes,
        filter,
        calendar,
        order,
        selected,
        rule,
        ribbon: ribbon_on,
        lanes: lanes_on,
    } = *input;
    // Both zones off is not a state the drawing has: it reads as both on.
    let (ribbon_on, lanes_on) = if ribbon_on || lanes_on { (ribbon_on, lanes_on) } else { (true, true) };
    let matches = |n: &Note| filter.matches(n);
    let entities: HashSet<&str> = notes
        .iter()
        .filter(|n| n.is_entity)
        .map(|n| n.id.as_str())
        .collect();
    let by_id: HashMap<&str, &Note> = notes.iter().map(|n| (n.id.as_str(), n)).collect();

    // The book's events under its span rule. Computed over every note, not the filtered
    // ones: how long the siege is drawn is a fact about the book, and a filter that
    // hides a scene must not shrink the siege around what is left.
    let rib = ribbon_of(notes, calendar, rule);

    // Dated, matching events — the ties. A dated *entity* is a lifespan on its own lane,
    // not a tie: "Vess, 1181 – 1211" is where her lane is live, not a moment she shares.
    // Each is drawn where the ribbon draws it, so a tie sits under its own bar.
    let mut events: Vec<Placed> = Vec::new();
    for n in notes.iter().filter(|n| !n.is_entity && matches(n)) {
        if n.span.is_none() {
            continue;
        }
        let Some(d) = rib.drawn.get(&n.id) else { continue };
        events.push(Placed {
            note: n,
            start: d.start,
            end: d.end,
            who: participants(n, &entities),
        });
    }
    let no_one = events.iter().filter(|e| e.who.is_empty()).count();

    // The ribbon's bars: matching events (dated, or fitted from what they contain), and
    // — muted — every ancestor of one, the tree's rule for the path to a match.
    let mut shown: BTreeMap<String, bool> = BTreeMap::new(); // id -> muted
    if ribbon_on {
        for id in rib.drawn.keys() {
            let Some(n) = by_id.get(id.as_str()) else { continue };
            if !matches(n) {
                continue;
            }
            shown.insert(id.clone(), false);
            let mut cur = id.clone();
            for _ in 0..notes.len() {
                let Some(p) = rib.parent.get(&cur) else { break };
                shown.entry(p.clone()).or_insert(true);
                cur = p.clone();
            }
        }
    }

    // Lanes: every matching entity, plus — muted — anyone a drawn event names.
    let mut lane_ids: Vec<(String, String)> = Vec::new();
    let mut muted: HashSet<String> = HashSet::new();
    let mut seen: HashSet<String> = HashSet::new();
    for n in notes.iter().filter(|n| n.is_entity && matches(n)) {
        if seen.insert(n.id.clone()) {
            lane_ids.push((n.id.clone(), n.title.clone()));
        }
    }
    for ev in &events {
        for id in &ev.who {
            if seen.insert(id.clone()) {
                let title = by_id.get(id.as_str()).map(|n| n.title.clone()).unwrap_or_default();
                lane_ids.push((id.clone(), title));
                muted.insert(id.clone());
            }
        }
    }

    // Lifespans of the entities that have lanes.
    let lives_src: Vec<(&Note, i64, Option<i64>)> = if lanes_on {
        lane_ids
            .iter()
            .filter_map(|(id, _)| by_id.get(id.as_str()))
            .filter_map(|n| {
                n.span.as_ref().map(|s| {
                    let (a, b) = extent(calendar, s);
                    (*n, a, b)
                })
            })
            .collect()
    } else {
        Vec::new()
    };

    // The visible span: everything drawn, padded a little each side.
    let mut lo = i64::MAX;
    let mut hi = i64::MIN;
    for (start, end) in events
        .iter()
        .map(|e| (e.start, e.end))
        .chain(lives_src.iter().map(|(_, a, b)| (*a, *b)))
        .chain(shown.keys().filter_map(|id| rib.drawn.get(id)).map(|d| (d.start, d.end)))
    {
        lo = lo.min(start);
        hi = hi.max(end.unwrap_or(start + 1));
    }
    let undated = lo > hi;
    if undated {
        // Nothing to measure: a placeholder decade from the epoch, so the holding rail
        // still has a line to drop onto.
        lo = 0;
        hi = calendar.start_tick(0, 10);
    }
    let pad = ((hi - lo) as f64 * 0.04) as i64;
    let (lo, hi) = (lo.saturating_sub(pad), hi.saturating_add(pad.max(1)));

    // First appearance: the earliest event an entity takes part in, or its own birth.
    let mut first: HashMap<String, i64> = HashMap::new();
    for ev in &events {
        for id in &ev.who {
            let e = first.entry(id.clone()).or_insert(ev.start);
            *e = (*e).min(ev.start);
        }
    }
    for (n, start, _) in &lives_src {
        let e = first.entry(n.id.clone()).or_insert(*start);
        *e = (*e).min(*start);
    }
    order_lanes(&mut lane_ids, &first, order);

    let (tick_level, tick_step, ticks) = ticks(calendar, lo, hi);
    let x = |t: i64| x_of(lo, hi, t);
    let band = |start: i64, end: Option<i64>| {
        let x0 = x(start);
        let x1 = end.map(x).unwrap_or(plot_x1()).max(x0 + MIN_BAND);
        (x0, x1)
    };

    // ── Zone 1: the ribbon ──
    let lit = |id: &str, who: &[String]| selected.is_some_and(|s| s == id || who.iter().any(|w| w == s));
    let mut bars: Vec<Bar> = Vec::new();
    let mut bands: Vec<Band> = Vec::new();
    let mut labels: Vec<BarLabel> = Vec::new();
    let mut dropped_labels = 0;
    let mut ribbon_rows = 0usize;
    if ribbon_on && !shown.is_empty() {
        // Each shown event's own x extent. A bar cut off at its parent's end keeps the
        // minimum width by growing leftward, so it never pokes past the edge it was cut
        // at.
        let bar_x: HashMap<&str, (f32, f32)> = shown
            .keys()
            .filter_map(|id| {
                let d = rib.drawn.get(id)?;
                let (mut x0, x1) = band(d.start, d.end);
                if d.clipped_end && x1 - x0 <= MIN_BAND {
                    let edge = d.end.map(x).unwrap_or(plot_x1());
                    x0 = edge - MIN_BAND;
                    return Some((id.as_str(), (x0, edge)));
                }
                Some((id.as_str(), (x0, x1)))
            })
            .collect();
        let title = |id: &str| by_id.get(id).map(|n| n.title.as_str()).unwrap_or("");
        let mut kids: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        let mut roots: Vec<&str> = Vec::new();
        for id in shown.keys() {
            match rib.parent.get(id) {
                Some(p) if shown.contains_key(p) => kids.entry(p.as_str()).or_default().push(id),
                _ => roots.push(id),
            }
        }
        let by_x = |a: &&str, b: &&str| {
            bar_x[a].0
                .total_cmp(&bar_x[b].0)
                .then_with(|| title(a).cmp(title(b)))
                .then_with(|| a.cmp(b))
        };

        // Blocks, bottom-up: a block is a bar with its children packed into sub-rows
        // beneath it. Iterative post-order, so a deep chain cannot overflow the stack.
        struct Block<'a> {
            x0: f32,
            x1: f32,
            height: usize,
            /// Children and the row each starts at, relative to this block's own row.
            kids: Vec<(&'a str, usize)>,
        }
        let mut blocks: HashMap<&str, Block> = HashMap::new();
        let mut stack: Vec<(&str, bool)> = roots.iter().map(|r| (*r, false)).collect();
        while let Some((id, expanded)) = stack.pop() {
            let mine: Vec<&str> = kids.get(id).cloned().unwrap_or_default();
            if !expanded {
                stack.push((id, true));
                for k in &mine {
                    stack.push((k, false));
                }
                continue;
            }
            let mut mine = mine;
            mine.sort_by(|a, b| {
                blocks[a].x0.total_cmp(&blocks[b].x0).then_with(|| by_x(a, b))
            });
            let items: Vec<(f32, f32, usize)> =
                mine.iter().map(|k| (blocks[k].x0, blocks[k].x1, blocks[k].height)).collect();
            let rows = ribbon::pack_rows(&items, PACK_GAP);
            let (mut x0, mut x1) = bar_x[id];
            let mut height = 1;
            for (k, r) in mine.iter().zip(&rows) {
                let b = &blocks[k];
                x0 = x0.min(b.x0);
                x1 = x1.max(b.x1);
                height = height.max(1 + r + b.height);
            }
            let placed = mine.iter().zip(rows).map(|(k, r)| (*k, 1 + r)).collect();
            blocks.insert(id, Block { x0, x1, height, kids: placed });
        }
        roots.sort_by(|a, b| blocks[a].x0.total_cmp(&blocks[b].x0).then_with(|| by_x(a, b)));
        let items: Vec<(f32, f32, usize)> =
            roots.iter().map(|r| (blocks[r].x0, blocks[r].x1, blocks[r].height)).collect();
        let root_rows = ribbon::pack_rows(&items, PACK_GAP);

        // Top-down: absolute rows.
        let row_y = |r: usize| RIBBON_TOP + r as f32 * ROW_PITCH;
        let mut place: Vec<(&str, usize, usize)> =
            roots.iter().zip(root_rows).map(|(r, row)| (*r, row, 0)).collect();
        while let Some((id, row, depth)) = place.pop() {
            let b = &blocks[id];
            ribbon_rows = ribbon_rows.max(row + b.height);
            for (k, off) in &b.kids {
                place.push((k, row + off, depth + 1));
            }
            let (x0, x1) = bar_x[id];
            if !b.kids.is_empty() {
                bands.push(Band {
                    id: id.to_string(),
                    x0: x0 - 3.0,
                    x1: x1 + 3.0,
                    y0: row_y(row) - 3.0,
                    y1: row_y(row) + b.height as f32 * ROW_PITCH - (ROW_PITCH - ROW_H) + 3.0,
                    depth,
                });
            }
            let n = by_id[id];
            let d = &rib.drawn[id];
            let who = participants(n, &entities);
            bars.push(Bar {
                id: id.to_string(),
                title: n.title.clone(),
                x0,
                x1,
                y: row_y(row),
                row,
                depth,
                muted: shown[id],
                fuzzy: n.span.as_ref().is_some_and(|s| s.approximate),
                open: d.end.is_none(),
                fitted: d.fitted,
                derived: d.derived,
                pinned: n.pinned && !d.derived,
                clipped_start: d.clipped_start,
                clipped_end: d.clipped_end,
                escapes_start: d.escapes_start,
                escapes_end: d.escapes_end,
                on: lit(id, &who),
                who,
            });
        }
        bars.sort_by(|a, b| a.row.cmp(&b.row).then(a.x0.total_cmp(&b.x0)).then_with(|| a.id.cmp(&b.id)));
        bands.sort_by(|a, b| a.depth.cmp(&b.depth).then(a.y0.total_cmp(&b.y0)).then_with(|| a.id.cmp(&b.id)));

        // Labels, a row at a time.
        let mut i = 0;
        while i < bars.len() {
            let row = bars[i].row;
            let j = bars[i..].iter().position(|b| b.row != row).map_or(bars.len(), |k| i + k);
            let on_row: Vec<(f32, f32, &str)> =
                bars[i..j].iter().map(|b| (b.x0, b.x1, b.title.as_str())).collect();
            let spots = ribbon::place_labels(&on_row, plot_x0(), plot_x1(), |t, room| fit(t, room, CAP_CH));
            for (b, spot) in bars[i..j].iter().zip(spots) {
                let y = b.y + ROW_H / 2.0 + 4.0;
                let (x, end, inside, text) = match spot {
                    LabelSpot::Inside { x, text } => (x, false, true, text),
                    LabelSpot::Beside { x, end, text } => (x, end, false, text),
                    LabelSpot::Dropped => {
                        dropped_labels += 1;
                        continue;
                    }
                };
                labels.push(BarLabel {
                    id: b.id.clone(),
                    text,
                    x,
                    y,
                    end,
                    inside,
                    muted: b.muted,
                    who: b.who.clone(),
                });
            }
            i = j;
        }
    }
    let ribbon_y = (ribbon_on).then(|| {
        (
            RIBBON_TOP - 8.0,
            RIBBON_TOP + ribbon_rows.max(1) as f32 * ROW_PITCH + 2.0,
        )
    });

    // ── Zone 2: the lanes ──

    // A book with no entities has no lanes to draw: under the ribbon that is an empty
    // zone, so it folds away. (Alone, the lanes zone still shows its empty axis.)
    let lanes_on = lanes_on && (!lane_ids.is_empty() || !ribbon_on);

    // Captions only without the ribbon: with it, every event's name is on its bar.
    let mut order_x: Vec<usize> = (0..events.len()).collect();
    order_x.sort_by(|&a, &b| {
        events[a]
            .start
            .cmp(&events[b].start)
            .then_with(|| events[a].note.title.cmp(&events[b].note.title))
            .then_with(|| events[a].note.id.cmp(&events[b].note.id))
    });
    let captioned: Vec<usize> = if ribbon_on {
        Vec::new()
    } else {
        order_x.iter().copied().filter(|&i| !events[i].who.is_empty()).collect()
    };
    let mut cap_text: Vec<(usize, String, f32)> = Vec::new();
    for &i in &captioned {
        let (x0, _) = band(events[i].start, events[i].end);
        let text = fit(&events[i].note.title, plot_x1() - plot_x0(), CAP_CH);
        let width = text.chars().count() as f32 * CAP_CH;
        // Pull a caption at the right edge back inside the drawing.
        let tx = (x0 + 3.0).min(plot_x1() - width).max(plot_x0());
        cap_text.push((i, text, tx));
    }
    let spans: Vec<(f32, f32)> = cap_text
        .iter()
        .map(|(_, t, tx)| (*tx, tx + t.chars().count() as f32 * CAP_CH + 8.0))
        .collect();
    let (tiers, crowded) = assign_tiers(&spans);
    let n_tiers = tiers.iter().max().map(|t| t + 1).unwrap_or(0);

    let cap_top = AXIS_Y + 8.0;
    let rows_top = match ribbon_y {
        Some((_, bottom)) => bottom + ZONE_GAP - 8.0,
        None => cap_top + n_tiers as f32 * TIER_H + 8.0,
    };

    let lane_y: HashMap<String, f32> = lane_ids
        .iter()
        .enumerate()
        .map(|(i, (id, _))| (id.clone(), rows_top + 14.0 + i as f32 * LANE_H))
        .collect();
    let lanes: Vec<Lane> = lane_ids
        .iter()
        .map(|(id, title)| Lane {
            id: id.clone(),
            title: title.clone(),
            label: fit(title, PAD_L - 14.0, LANE_CH),
            y: lane_y[id],
            muted: muted.contains(id),
            on: selected == Some(id.as_str()),
        })
        .collect();
    let height = if lanes_on {
        rows_top + 14.0 + (lanes.len().max(1) as f32 - 1.0) * LANE_H + 20.0
    } else {
        ribbon_y.map_or(RIBBON_TOP, |(_, b)| b) + 12.0
    };

    let lives: Vec<Life> = lives_src
        .iter()
        .map(|(n, start, end)| {
            let (x0, x1) = band(*start, *end);
            Life {
                id: n.id.clone(),
                x0,
                x1,
                y: lane_y[&n.id],
                fuzzy: n.span.as_ref().is_some_and(|s| s.approximate),
                open: end.is_none(),
                on: selected == Some(n.id.as_str()),
            }
        })
        .collect();

    // Ties: an event with people in it, while the lanes are on. With the ribbon on it
    // rises from the bottom of the event's own bar.
    let bar_at: HashMap<&str, &Bar> = bars.iter().map(|b| (b.id.as_str(), b)).collect();
    let mut marks: Vec<Mark> = Vec::new();
    if lanes_on {
        for &i in &order_x {
            let ev = &events[i];
            if ev.who.is_empty() {
                continue;
            }
            let (x0, x1) = match bar_at.get(ev.note.id.as_str()) {
                Some(b) => (b.x0, b.x1),
                None => band(ev.start, ev.end),
            };
            let ys: Vec<f32> = ev.who.iter().map(|w| lane_y[w]).collect();
            let lane_y0 = ys.iter().copied().fold(f32::MAX, f32::min);
            let y1 = ys.iter().copied().fold(f32::MIN, f32::max);
            let y0 = bar_at.get(ev.note.id.as_str()).map_or(lane_y0, |b| b.y + ROW_H);
            let dots = ev
                .who
                .iter()
                .map(|w| Dot {
                    lane: w.clone(),
                    y: lane_y[w],
                    on: selected == Some(w.as_str()),
                })
                .collect();
            let span = ev.note.span.as_ref();
            marks.push(Mark {
                id: ev.note.id.clone(),
                title: ev.note.title.clone(),
                x0,
                x1,
                cx: (x0 + x1) / 2.0,
                y0,
                y1,
                lane_y0,
                fuzzy: span.is_some_and(|s| s.approximate),
                open: ev.end.is_none(),
                on: lit(&ev.note.id, &ev.who),
                dots,
            });
        }
    }

    let mut captions: Vec<Caption> = Vec::new();
    if lanes_on {
        for (k, (i, text, tx)) in cap_text.into_iter().enumerate() {
            let ev = &events[i];
            let (x0, _) = band(ev.start, ev.end);
            let top = ev.who.iter().map(|w| lane_y[w]).fold(f32::MAX, f32::min);
            let tier = tiers[k];
            let y = cap_top + (n_tiers - 1 - tier) as f32 * TIER_H + 10.0;
            captions.push(Caption {
                id: ev.note.id.clone(),
                text,
                x: tx,
                y,
                tier,
                leader_x: x0 + 1.0,
                leader_y0: y + 3.0,
                leader_y1: top - 10.0,
                on: lit(&ev.note.id, &ev.who),
            });
        }
    }

    // The holding rail: notes that happen but are not on the line yet — placed only
    // against another note, or naming people with `$` and no date. Plain undated lore
    // stays out, or a lore-heavy book would bury the rail; so does a note the ribbon
    // already draws, fitted from the events inside it.
    let mut held: Vec<Held> = notes
        .iter()
        .filter(|n| n.span.is_none() && !n.is_entity && matches(n))
        .filter(|n| !rib.drawn.contains_key(&n.id))
        .filter(|n| n.relative.is_some() || !participants(n, &entities).is_empty())
        .map(|n| {
            let why = super::time_entry::format_entry(None, n.relative.as_ref(), calendar, notes);
            Held {
                id: n.id.clone(),
                title: n.title.clone(),
                why: if why.is_empty() { "undated".to_string() } else { why },
            }
        })
        .collect();
    held.sort_by(|a, b| fold_token(&a.title).cmp(&fold_token(&b.title)).then(a.id.cmp(&b.id)));

    Layout {
        height,
        lo,
        hi,
        tick_level,
        tick_step,
        ticks,
        lanes,
        lives,
        marks,
        captions,
        crowded,
        bars,
        bands,
        labels,
        dropped_labels,
        ribbon_y,
        lanes_on,
        no_one,
        held,
        undated,
    }
}

/// The unit a drop lands at, in the calendar's words ("Year", "Day") — shown on the
/// drop guide so the author knows how precisely the note is about to be dated.
pub fn drop_unit_name(calendar: &Calendar, layout: &Layout) -> String {
    calendar.precision_name(layout.tick_level as u8).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pages::book::notes_filter::{ChipState, Facet, Term};
    use plotweb_common::{
        NoteLink, NoteLinks, RelativeTime, TimeRelation, TICKS_PER_BASE_UNIT as YEAR,
    };

    fn note(id: &str) -> Note {
        Note {
            id: id.to_string(),
            book_id: "b".to_string(),
            title: id.to_string(),
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

    fn entity(id: &str) -> Note {
        Note {
            is_entity: true,
            ..note(id)
        }
    }

    fn refs(ids: &[&str]) -> NoteLinks {
        NoteLinks {
            refs: ids
                .iter()
                .map(|i| NoteLink {
                    text: i.to_string(),
                    target: LinkTarget::Note,
                    id: Some(i.to_string()),
                })
                .collect(),
            ..Default::default()
        }
    }

    fn event(id: &str, year: i64, who: &[&str]) -> Note {
        Note {
            span: Some(TimeSpan::at(TimePoint::base_unit(year))),
            links: refs(who),
            ..note(id)
        }
    }

    /// Card 5's view: the lanes alone, ribbon off.
    fn run(notes: &[Note], filter: &Filter, order: LaneOrder, selected: Option<&str>) -> Layout {
        let cal = Calendar::default();
        layout(&Input {
            notes,
            filter,
            calendar: &cal,
            order,
            selected,
            rule: SpanRule::Fit,
            ribbon: false,
            lanes: true,
        })
    }

    /// Both zones, under `rule`.
    fn both(notes: &[Note], rule: SpanRule, selected: Option<&str>) -> Layout {
        zones(notes, &Filter::default(), rule, selected, true, true)
    }

    fn zones(
        notes: &[Note],
        filter: &Filter,
        rule: SpanRule,
        selected: Option<&str>,
        ribbon: bool,
        lanes: bool,
    ) -> Layout {
        let cal = Calendar::default();
        layout(&Input {
            notes,
            filter,
            calendar: &cal,
            order: LaneOrder::FirstAppearance,
            selected,
            rule,
            ribbon,
            lanes,
        })
    }

    fn lane_ids(l: &Layout) -> Vec<&str> {
        l.lanes.iter().map(|l| l.id.as_str()).collect()
    }

    fn cast() -> Vec<Note> {
        vec![
            entity("Vess"),
            entity("Corin"),
            entity("Maera"),
            event("siege", 1206, &["Corin", "Vess"]),
            event("harrowgate", 1204, &["Vess"]),
            event("winter", 1207, &["Maera", "Corin"]),
        ]
    }

    // ── Ticks ──

    #[test]
    fn a_decade_ticks_in_years_because_months_would_crowd_it() {
        let cal = Calendar::default();
        // Months are *allowed* below ten years, but nine years of months is 108 ticks.
        let (level, step) = choose_tick_unit(&cal, 9.0 * YEAR as f64);
        assert_eq!((level, step), (0, 1));
    }

    #[test]
    fn a_year_ticks_in_months_and_a_week_in_days() {
        let cal = Calendar::default();
        assert_eq!(choose_tick_unit(&cal, 0.9 * YEAR as f64), (1, 1));
        assert_eq!(choose_tick_unit(&cal, 7.0 * 86_400.0), (2, 1));
        // A day and a bit: Hours are allowed below three Days and fit at 2h steps? No —
        // 30 hours is past MAX_TICKS, so it stays on Days.
        assert_eq!(choose_tick_unit(&cal, 30.0 * 3_600.0).0, 2);
        assert_eq!(choose_tick_unit(&cal, 10.0 * 3_600.0), (3, 1));
    }

    #[test]
    fn a_unit_the_calendar_hides_is_never_used_even_when_it_would_fit() {
        let mut cal = Calendar::default();
        // Months only below a 6-month span: a ten-month span must not tick in months.
        cal.units[1].shown_below = Some(plotweb_common::ShownBelow { count: 6, unit: 1 });
        assert_eq!(choose_tick_unit(&cal, 10.0 / 12.0 * YEAR as f64).0, 0);
        assert_eq!(choose_tick_unit(&cal, 5.0 / 12.0 * YEAR as f64).0, 1);
    }

    #[test]
    fn centuries_step_the_base_unit_by_a_nice_number() {
        let cal = Calendar::default();
        let (level, step) = choose_tick_unit(&cal, 500.0 * YEAR as f64);
        assert_eq!(level, 0);
        assert_eq!(step, 50);
        let (_, _, ticks) = ticks(&cal, 1000 * YEAR, 1500 * YEAR);
        let labels: Vec<&str> = ticks.iter().map(|t| t.label.as_str()).collect();
        assert_eq!(labels, vec!["1000", "1050", "1100", "1150", "1200", "1250", "1300", "1350", "1400", "1450", "1500"]);
    }

    #[test]
    fn month_ticks_read_in_full_where_the_year_turns() {
        let cal = Calendar::default();
        let lo = cal.parse_point("1817 Oct").unwrap().tick;
        let hi = cal.parse_point("1818 Mar").unwrap().tick;
        let (level, _, ticks) = ticks(&cal, lo, hi);
        assert_eq!(level, 1);
        let labels: Vec<&str> = ticks.iter().map(|t| t.label.as_str()).collect();
        assert_eq!(labels, vec!["1817 Oct", "Nov", "Dec", "1818 Jan", "Feb", "Mar"]);
        let majors: Vec<bool> = ticks.iter().map(|t| t.major).collect();
        assert_eq!(majors, vec![false, false, false, true, false, false]);
    }

    #[test]
    fn the_accord_ticks_in_its_own_seasons() {
        let mut season = plotweb_common::CalendarUnit {
            name: "Season".into(),
            per: 4,
            of: 0,
            names: vec!["wet".into(), "dry".into(), "high".into(), "low".into()],
            format: ", {name}".into(),
            first: 1,
            shown_below: Some(plotweb_common::ShownBelow { count: 40, unit: 0 }),
        };
        season.shown_below = Some(plotweb_common::ShownBelow { count: 40, unit: 0 });
        let cal = Calendar {
            name: "The Accord".into(),
            units: vec![
                plotweb_common::CalendarUnit {
                    name: "Year".into(),
                    per: 1,
                    of: 0,
                    names: vec![],
                    format: "yr {n}".into(),
                    first: 0,
                    shown_below: None,
                },
                season,
            ],
        };
        let (level, _, ticks) = ticks(&cal, 1206 * YEAR, 1208 * YEAR);
        assert_eq!(level, 1);
        let labels: Vec<&str> = ticks.iter().map(|t| t.label.as_str()).collect();
        assert_eq!(labels[..5], ["yr 1206, wet", "dry", "high", "low", "yr 1207, wet"]);
    }

    #[test]
    fn a_tick_label_that_would_collide_is_dropped_but_the_tick_stays() {
        let cal = Calendar::default();
        // Twelve months squeezed so the long "1817 Jan" overlaps its neighbour.
        let lo = cal.parse_point("1817 Jan").unwrap().tick;
        let (_, _, ticks) = ticks(&cal, lo, lo + YEAR - 1);
        assert_eq!(ticks.len(), 12);
        assert!(ticks.iter().all(|t| !t.label.contains("  ")));
        let first_end = ticks[0].x + 3.0 + "1817 Jan".len() as f32 * TICK_CH;
        for t in &ticks[1..] {
            if t.x < first_end + 6.0 {
                assert!(t.label.is_empty(), "{t:?} overprints the first label");
            }
        }
    }

    // ── Lanes ──

    #[test]
    fn lanes_order_by_first_appearance_then_alphabetically_on_request() {
        let notes = cast();
        let l = run(&notes, &Filter::default(), LaneOrder::FirstAppearance, None);
        // Vess at Harrowgate (1204), Corin at the siege (1206), Maera in the winter (1207).
        assert_eq!(lane_ids(&l), vec!["Vess", "Corin", "Maera"]);
        let l = run(&notes, &Filter::default(), LaneOrder::Alphabetical, None);
        assert_eq!(lane_ids(&l), vec!["Corin", "Maera", "Vess"]);
    }

    #[test]
    fn an_entity_that_never_appears_still_gets_a_lane_at_the_bottom() {
        let mut notes = cast();
        notes.push(entity("Aldo"));
        let l = run(&notes, &Filter::default(), LaneOrder::FirstAppearance, None);
        assert_eq!(lane_ids(&l), vec!["Vess", "Corin", "Maera", "Aldo"]);
    }

    #[test]
    fn a_lifespan_counts_as_an_appearance_and_is_drawn_on_its_own_lane() {
        let mut notes = cast();
        notes[2].span = Some(TimeSpan::at(TimePoint::base_unit(1190))); // Maera, born 1190
        let l = run(&notes, &Filter::default(), LaneOrder::FirstAppearance, None);
        assert_eq!(lane_ids(&l), vec!["Maera", "Vess", "Corin"]);
        assert_eq!(l.lives.len(), 1);
        assert_eq!(l.lives[0].y, l.lanes[0].y);
        // A dated entity is not a tie, and not a ribbon event either.
        assert!(l.marks.iter().all(|m| m.id != "Maera"));
        let l = both(&notes, SpanRule::Fit, None);
        assert!(l.bars.iter().all(|b| b.id != "Maera"));
    }

    #[test]
    fn ties_join_only_dollar_refs_to_entities() {
        let mut notes = cast();
        // @mentions are quiet; a $ref to a lore note is not participation.
        notes.push(note("the-map"));
        let mut ev = event("parley", 1206, &["Corin", "the-map"]);
        ev.links.mentions = vec![NoteLink {
            text: "Vess".into(),
            target: LinkTarget::Note,
            id: Some("Vess".into()),
        }];
        notes.push(ev);
        let l = run(&notes, &Filter::default(), LaneOrder::FirstAppearance, None);
        let parley = l.marks.iter().find(|m| m.id == "parley").unwrap();
        let dots: Vec<&str> = parley.dots.iter().map(|d| d.lane.as_str()).collect();
        assert_eq!(dots, vec!["Corin"]);
    }

    #[test]
    fn a_tie_runs_from_its_top_participant_to_its_bottom_one() {
        let notes = cast();
        let l = run(&notes, &Filter::default(), LaneOrder::FirstAppearance, None);
        let y = |id: &str| l.lanes.iter().find(|x| x.id == id).unwrap().y;
        let siege = l.marks.iter().find(|m| m.id == "siege").unwrap();
        assert_eq!((siege.y0, siege.y1), (y("Vess"), y("Corin")));
        assert_eq!(siege.dots.len(), 2);
    }

    // ── Selection ──

    #[test]
    fn selecting_an_entity_lights_its_lane_its_dots_and_every_event_it_is_in() {
        let notes = cast();
        let l = run(&notes, &Filter::default(), LaneOrder::FirstAppearance, Some("Corin"));
        let on_lanes: Vec<&str> = l.lanes.iter().filter(|x| x.on).map(|x| x.id.as_str()).collect();
        assert_eq!(on_lanes, vec!["Corin"]);
        let mut on_marks: Vec<&str> = l.marks.iter().filter(|m| m.on).map(|m| m.id.as_str()).collect();
        on_marks.sort();
        assert_eq!(on_marks, vec!["siege", "winter"]);
        let lit: Vec<&str> = l
            .marks
            .iter()
            .flat_map(|m| m.dots.iter())
            .filter(|d| d.on)
            .map(|d| d.lane.as_str())
            .collect();
        assert_eq!(lit, vec!["Corin", "Corin"]);
        assert!(l.captions.iter().filter(|c| c.on).count() == 2);
    }

    // ── The filter ──

    #[test]
    fn the_filter_keeps_an_events_participants_as_muted_lanes() {
        let mut notes = cast();
        notes[3].links.tags = vec!["siege".into()];
        let f = Filter::default().cycled(&Term::Tag("siege".into()));
        assert_eq!(f.state_of(&Term::Tag("siege".into())), ChipState::Must);
        let l = run(&notes, &f, LaneOrder::FirstAppearance, None);
        let marks: Vec<&str> = l.marks.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(marks, vec!["siege"]);
        // Neither entity is tagged, so both lanes are here only for the siege — and both
        // first appear at it, so they fall back to alphabetical.
        assert_eq!(lane_ids(&l), vec!["Corin", "Vess"]);
        assert!(l.lanes.iter().all(|x| x.muted));
    }

    #[test]
    fn filtering_to_entities_draws_every_lane_and_no_event() {
        let notes = cast();
        let f = Filter::default().cycled(&Term::Facet(Facet::Entity));
        let l = run(&notes, &f, LaneOrder::Alphabetical, None);
        assert_eq!(lane_ids(&l), vec!["Corin", "Maera", "Vess"]);
        assert!(l.marks.is_empty());
        assert!(l.lanes.iter().all(|x| !x.muted));
        assert!(l.undated, "nothing drawn is dated, so the axis is a placeholder");
    }

    #[test]
    fn without_an_entity_keeps_its_lane_off_unless_an_event_needs_it() {
        let notes = cast();
        // "without entity" drops every matched lane — the events still pull theirs in.
        let f = Filter::default()
            .cycled(&Term::Facet(Facet::Entity))
            .cycled(&Term::Facet(Facet::Entity))
            .cycled(&Term::Facet(Facet::Entity));
        let l = run(&notes, &f, LaneOrder::FirstAppearance, None);
        assert_eq!(l.marks.len(), 3);
        assert_eq!(lane_ids(&l).len(), 3);
        assert!(l.lanes.iter().all(|x| x.muted));
    }

    // ── Events that name no one ──

    #[test]
    fn an_event_naming_nobody_lives_in_the_ribbon_and_is_counted_without_it() {
        let mut notes = cast();
        notes.push(event("comet", 1205, &[]));
        // A $ref to something that is not an entity is still nobody.
        notes.push(note("rumour"));
        notes.push(event("fire", 1206, &["rumour"]));
        // Card 5's "no one" row is gone: with the ribbon on, the comet is a bar with no
        // tie, and no lane was made up for it.
        let l = both(&notes, SpanRule::Fit, None);
        assert!(l.bars.iter().any(|b| b.id == "comet"));
        assert!(l.bars.iter().any(|b| b.id == "fire"));
        assert!(l.marks.iter().all(|m| m.id != "comet" && m.id != "fire"));
        assert_eq!(l.no_one, 2);
        // With the ribbon off it has nowhere to be drawn, and the count says so.
        let l = run(&notes, &Filter::default(), LaneOrder::FirstAppearance, None);
        assert!(l.marks.iter().all(|m| m.id != "comet"));
        assert!(l.captions.iter().all(|c| c.id != "comet"));
        assert_eq!(l.no_one, 2);
        assert_eq!(lane_ids(&l).len(), 3);
    }

    // ── Captions ──

    #[test]
    fn captions_stack_into_tiers_only_where_they_would_collide() {
        let (tiers, crowded) = assign_tiers(&[(0.0, 50.0), (60.0, 100.0), (70.0, 120.0), (75.0, 90.0), (130.0, 150.0)]);
        assert_eq!(tiers, vec![0, 0, 1, 2, 0]);
        assert_eq!(crowded, 0);
    }

    #[test]
    fn past_the_last_tier_a_caption_shares_the_one_that_frees_soonest() {
        let spans: Vec<(f32, f32)> = (0..MAX_TIERS + 1).map(|i| (i as f32, 100.0 + i as f32)).collect();
        let (tiers, crowded) = assign_tiers(&spans);
        assert_eq!(tiers[..MAX_TIERS], (0..MAX_TIERS).collect::<Vec<_>>()[..]);
        assert_eq!(tiers[MAX_TIERS], 0, "tier 0 ends first");
        assert_eq!(crowded, 1);
    }

    #[test]
    fn a_dense_moment_staggers_its_captions_with_leaders_down_to_the_lanes() {
        let mut notes = cast();
        for i in 0..4 {
            notes.push(event(&format!("scene {i}"), 1206, &["Corin"]));
        }
        let l = run(&notes, &Filter::default(), LaneOrder::FirstAppearance, None);
        let at_1206: Vec<&Caption> = l
            .captions
            .iter()
            .filter(|c| c.id == "siege" || c.id.starts_with("scene"))
            .collect();
        let mut tiers: Vec<usize> = at_1206.iter().map(|c| c.tier).collect();
        tiers.sort();
        tiers.dedup();
        assert_eq!(tiers.len(), 5, "five captions at one x need five tiers");
        for c in &l.captions {
            let mark = l.marks.iter().find(|m| m.id == c.id).unwrap();
            assert!(c.leader_y0 < c.leader_y1, "{c:?}: the leader runs downward");
            assert!(c.leader_y1 < mark.y0, "{c:?}: the leader stops just above its tie");
            assert_eq!(c.leader_x, mark.x0 + 1.0, "{c:?}: and lands on its band");
        }
    }

    #[test]
    fn fit_cuts_long_names_with_an_ellipsis() {
        assert_eq!(fit("Corin", 100.0, 5.0), "Corin");
        assert_eq!(fit("The breach at Low Gate", 50.0, 5.0), "The breac\u{2026}");
        assert_eq!(fit("anything", 3.0, 5.0), "");
    }

    // ── The holding rail and the drop ──

    #[test]
    fn the_holding_rail_takes_relative_and_undated_participation_but_not_lore() {
        let mut notes = cast();
        notes.push(Note {
            relative: Some(RelativeTime {
                relation: TimeRelation::After,
                note_id: "siege".into(),
            }),
            ..note("aftermath")
        });
        notes.push(Note {
            links: refs(&["Vess"]),
            ..note("a letter")
        });
        notes.push(note("the map of Vaun"));
        let l = run(&notes, &Filter::default(), LaneOrder::FirstAppearance, None);
        let held: Vec<(&str, &str)> = l.held.iter().map(|h| (h.id.as_str(), h.why.as_str())).collect();
        assert_eq!(held, vec![("a letter", "undated"), ("aftermath", "after siege")]);
    }

    #[test]
    fn a_drop_snaps_to_the_tick_unit() {
        let notes = cast();
        let l = run(&notes, &Filter::default(), LaneOrder::FirstAppearance, None);
        assert_eq!(l.tick_level, 0);
        let cal = Calendar::default();
        let mid = l.point_at(&cal, 0.5);
        assert_eq!(mid.precision, 0);
        assert_eq!(mid.tick % YEAR, 0, "the start of a year");
        let x = l.x_of(mid.tick);
        assert!(x <= (plot_x0() + plot_x1()) / 2.0);
        assert_eq!(l.point_at(&cal, -3.0), l.point_at(&cal, 0.0));
    }

    #[test]
    fn an_instant_covers_the_unit_it_is_known_to() {
        let cal = Calendar::default();
        let year = TimeSpan::at(TimePoint::base_unit(1206));
        assert_eq!(extent(&cal, &year), (1206 * YEAR, Some(1207 * YEAR)));
        let day = TimeSpan::at(cal.parse_point("1206 Mar 4").unwrap());
        let (a, b) = extent(&cal, &day);
        assert_eq!(b.unwrap() - a, 86_400);
        let open = TimeSpan {
            open_ended: true,
            ..year.clone()
        };
        assert_eq!(extent(&cal, &open).1, None);
    }

    #[test]
    fn the_layout_never_reads_a_note_body() {
        // The structure-doc guarantee, pinned: a note whose body says something the
        // link index does not must draw from the index.
        let mut notes = cast();
        notes[3].content = "$Maera was also there".into();
        let l = run(&notes, &Filter::default(), LaneOrder::FirstAppearance, None);
        let siege = l.marks.iter().find(|m| m.id == "siege").unwrap();
        assert!(siege.dots.iter().all(|d| d.lane != "Maera"));
    }

    // ── The ribbon (card 6) ──

    fn spanned(id: &str, from: i64, to: i64, who: &[&str]) -> Note {
        Note {
            span: Some(TimeSpan {
                start: TimePoint::base_unit(from),
                end: Some(TimePoint::base_unit(to)),
                approximate: false,
                open_ended: false,
            }),
            links: refs(who),
            ..note(id)
        }
    }

    fn inside(mut n: Note, parent: &str) -> Note {
        n.event_parent = Some(parent.to_string());
        n
    }

    /// The siege (1206–1207) holds the breach (1207–1209), which runs past it, and the
    /// parley (1206) overlaps the siege without being in it.
    fn siege_book() -> Vec<Note> {
        vec![
            entity("Vess"),
            entity("Corin"),
            spanned("siege", 1206, 1207, &["Corin", "Vess"]),
            inside(spanned("breach", 1207, 1209, &["Corin"]), "siege"),
            event("parley", 1206, &["Vess"]),
        ]
    }

    fn bar<'a>(l: &'a Layout, id: &str) -> &'a Bar {
        l.bars.iter().find(|b| b.id == id).unwrap_or_else(|| panic!("no bar {id}"))
    }

    fn band_of<'a>(l: &'a Layout, id: &str) -> &'a Band {
        l.bands.iter().find(|b| b.id == id).unwrap_or_else(|| panic!("no band {id}"))
    }

    #[test]
    fn fit_the_siege_visibly_contains_the_breach() {
        let l = both(&siege_book(), SpanRule::Fit, None);
        let (siege, breach) = (bar(&l, "siege"), bar(&l, "breach"));
        assert_eq!(breach.depth, 1);
        assert!(breach.row > siege.row, "a sub-row beneath its parent");
        assert!(siege.fitted);
        assert!(siege.x0 <= breach.x0 && siege.x1 >= breach.x1, "the parent spans the child");
        let band = band_of(&l, "siege");
        assert!(band.x0 <= breach.x0 && band.x1 >= breach.x1);
        assert!(band.y0 <= siege.y && band.y1 >= breach.y + ROW_H);
        assert!(!breach.escapes_end && !breach.clipped_end);
    }

    #[test]
    fn clamp_cuts_the_breach_at_the_sieges_edge() {
        let l = both(&siege_book(), SpanRule::Clamp, None);
        let (siege, breach) = (bar(&l, "siege"), bar(&l, "breach"));
        assert!(!siege.fitted);
        assert!(breach.clipped_end);
        assert!((breach.x1 - siege.x1).abs() < 0.01, "cut exactly at the parent's edge");
        assert!(breach.note().contains("cut off"));
        // The breach's tie sits under the bar as drawn.
        let m = l.marks.iter().find(|m| m.id == "breach").unwrap();
        assert!((m.cx - (breach.x0 + breach.x1) / 2.0).abs() < 0.01);
    }

    #[test]
    fn free_lets_the_breach_poke_out_of_the_band_and_flags_it() {
        let l = both(&siege_book(), SpanRule::Free, None);
        let (siege, breach) = (bar(&l, "siege"), bar(&l, "breach"));
        assert!(!siege.fitted);
        assert!(breach.escapes_end && !breach.clipped_end);
        assert!(breach.x1 > band_of(&l, "siege").x1, "it pokes out past the band");
        assert!(breach.note().contains("runs outside"));
    }

    #[test]
    fn overlap_is_never_nesting() {
        // The parley coincides with the siege and is not in it.
        let l = both(&siege_book(), SpanRule::Fit, None);
        let parley = bar(&l, "parley");
        assert_eq!(parley.depth, 0);
        assert!(l.bands.iter().all(|b| b.id != "parley"));
        assert_ne!(parley.row, bar(&l, "siege").row, "it overlaps, so it packs into another row");
    }

    #[test]
    fn a_tie_rises_from_the_lanes_into_its_bar() {
        let l = both(&siege_book(), SpanRule::Fit, None);
        let siege = bar(&l, "siege");
        let m = l.marks.iter().find(|m| m.id == "siege").unwrap();
        assert_eq!(m.y0, siege.y + ROW_H, "the tie starts at the bottom of the bar");
        assert!(m.y1 >= m.lane_y0 && m.lane_y0 > m.y0);
        assert!(l.lanes.iter().all(|x| x.y > l.ribbon_y.unwrap().1), "lanes sit below the ribbon");
        // Captions belong to the lanes-only view: with the ribbon, names are on bars.
        assert!(l.captions.is_empty());
        // Without the ribbon the tie runs from the top lane, as in card 5.
        let l = run(&siege_book(), &Filter::default(), LaneOrder::FirstAppearance, None);
        let m = l.marks.iter().find(|m| m.id == "siege").unwrap();
        assert_eq!(m.y0, m.lane_y0);
        assert!(l.bars.is_empty() && l.ribbon_y.is_none());
    }

    #[test]
    fn either_zone_collapses_and_both_off_reads_as_both_on() {
        let notes = siege_book();
        let f = Filter::default();
        let ribbon_only = zones(&notes, &f, SpanRule::Fit, None, true, false);
        assert!(!ribbon_only.lanes_on);
        assert!(ribbon_only.marks.is_empty() && ribbon_only.captions.is_empty());
        assert_eq!(ribbon_only.bars.len(), 3);
        // The lanes are still laid out: they are the follow chips.
        assert_eq!(ribbon_only.lanes.len(), 2);
        let full = both(&notes, SpanRule::Fit, None);
        assert!(ribbon_only.height < full.height);

        let neither = zones(&notes, &f, SpanRule::Fit, None, false, false);
        assert_eq!(neither, full);
    }

    #[test]
    fn the_filter_keeps_an_ancestor_muted_and_does_not_shrink_its_fit() {
        let mut notes = siege_book();
        let full = both(&notes, SpanRule::Fit, None);
        // Only the breach matches.
        notes[3].links.tags = vec!["breach".into()];
        let filter = Filter::default().cycled(&Term::Tag("breach".into()));
        let l = zones(&notes, &filter, SpanRule::Fit, None, true, true);
        let ids: Vec<&str> = l.bars.iter().map(|b| b.id.as_str()).collect();
        assert!(ids.contains(&"breach") && ids.contains(&"siege"), "{ids:?}");
        assert!(!ids.contains(&"parley"));
        assert!(bar(&l, "siege").muted && !bar(&l, "breach").muted);
        // Still fitted over the breach, whatever else the filter hides.
        let (a, b) = (bar(&l, "siege"), bar(&full, "siege"));
        assert!(a.fitted && b.fitted);
        // A muted parent draws no tie and makes no lane of its own.
        assert!(l.marks.iter().all(|m| m.id != "siege"));
    }

    #[test]
    fn selection_lights_bars_in_the_ribbon_as_well_as_the_lanes() {
        let l = both(&siege_book(), SpanRule::Fit, Some("Corin"));
        let mut on: Vec<&str> = l.bars.iter().filter(|b| b.on).map(|b| b.id.as_str()).collect();
        on.sort();
        assert_eq!(on, vec!["breach", "siege"]);
        let l = both(&siege_book(), SpanRule::Fit, Some("parley"));
        assert!(bar(&l, "parley").on && !bar(&l, "siege").on);
    }

    #[test]
    fn a_dateless_parent_is_drawn_from_its_children_and_leaves_the_rail() {
        let mut notes = siege_book();
        // "Act two" is lore, with no date, placed after the parley — a rail candidate
        // until it contains something dated.
        notes.push(Note {
            relative: Some(RelativeTime { relation: TimeRelation::After, note_id: "parley".into() }),
            ..note("act two")
        });
        let l = both(&notes, SpanRule::Clamp, None);
        assert!(l.held.iter().any(|h| h.id == "act two"));
        assert!(l.bars.iter().all(|b| b.id != "act two"));

        notes[2].event_parent = Some("act two".into());
        for rule in [SpanRule::Fit, SpanRule::Clamp, SpanRule::Free] {
            let l = both(&notes, rule, None);
            let act = bar(&l, "act two");
            assert!(act.derived, "{rule:?}");
            let siege = bar(&l, "siege");
            assert!(act.x0 <= siege.x0 + 0.01 && act.x1 + 0.01 >= siege.x1, "{rule:?}");
            assert_eq!((siege.depth, bar(&l, "breach").depth), (1, 2), "{rule:?}");
            assert!(l.held.iter().all(|h| h.id != "act two"), "{rule:?}");
        }
    }

    #[test]
    fn deep_nesting_stacks_bands_inside_bands() {
        let notes = vec![
            spanned("war", 1200, 1220, &[]),
            inside(spanned("siege", 1206, 1208, &[]), "war"),
            inside(spanned("breach", 1207, 1207, &[]), "siege"),
            inside(event("gate", 1207, &[]), "breach"),
        ];
        let l = both(&notes, SpanRule::Fit, None);
        let depths: Vec<usize> = ["war", "siege", "breach", "gate"].iter().map(|id| bar(&l, id).depth).collect();
        assert_eq!(depths, vec![0, 1, 2, 3]);
        let rows: Vec<usize> = ["war", "siege", "breach", "gate"].iter().map(|id| bar(&l, id).row).collect();
        assert!(rows.windows(2).all(|w| w[0] < w[1]), "{rows:?}");
        let (war, siege, breach) = (band_of(&l, "war"), band_of(&l, "siege"), band_of(&l, "breach"));
        assert!(war.y0 < siege.y0 && siege.y0 < breach.y0);
        assert!(war.y1 >= siege.y1 && siege.y1 >= breach.y1);
    }

    #[test]
    fn a_corrupt_nesting_loop_still_draws() {
        let notes = vec![
            inside(event("a", 1206, &[]), "b"),
            inside(event("b", 1207, &[]), "a"),
            inside(event("c", 1208, &[]), "c"),
        ];
        let l = both(&notes, SpanRule::Fit, None);
        assert_eq!(l.bars.len(), 3);
        assert!(l.bars.iter().all(|b| b.depth == 0));
        assert!(l.bands.is_empty());
    }

    #[test]
    fn nesting_under_an_entity_or_a_missing_note_is_not_drawn_as_nesting() {
        let notes = vec![
            entity("Vess"),
            inside(event("birthday", 1206, &["Vess"]), "Vess"),
            inside(event("orphan", 1207, &[]), "deleted-note"),
        ];
        let l = both(&notes, SpanRule::Fit, None);
        assert!(l.bars.iter().all(|b| b.depth == 0));
        assert!(l.bars.iter().all(|b| b.id != "Vess"));
    }

    #[test]
    fn labels_that_have_nowhere_to_go_are_counted() {
        // A crowd of one-day scenes in one year: most labels cannot fit.
        let cal = Calendar::default();
        let mut notes = vec![spanned("siege", 1206, 1206, &[])];
        for d in 0..30 {
            let p = cal.parse_point(&format!("1206 Mar {}", d % 28 + 1)).unwrap();
            notes.push(inside(
                Note { span: Some(TimeSpan::at(p)), ..note(&format!("a rather long scene name {d}")) },
                "siege",
            ));
        }
        let l = both(&notes, SpanRule::Fit, None);
        assert_eq!(l.labels.len() + l.dropped_labels, l.bars.len());
        assert!(l.dropped_labels > 0);
        // Every bar still has its name as a tooltip-ready title.
        assert!(l.bars.iter().all(|b| !b.title.is_empty()));
    }
}
