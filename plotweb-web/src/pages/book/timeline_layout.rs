//! The timeline's entity lanes, laid out — every rule of the drawing, none of the DOM.
//!
//! `design/04-notes-wireframes.html`, take C, as the *lanes* zone of "the chosen shape":
//! one horizontal lane per entity, time on the x axis, and each event a vertical tie
//! joining the lanes of everyone it `$ref`s, with a dot per participant. The event
//! ribbon that stacks above it is card 6; nothing here draws nesting or relates a child
//! event's span to its parent's.
//!
//! Pure functions over the note list, host-tested like [`super::notes_filter`],
//! [`super::sigils`] and [`super::time_entry`]. `panes::timeline` turns a [`Layout`]
//! into SVG and does nothing else, so every decision below — which ticks, which lanes
//! in which order, which caption tier, which events go in the unassigned row — is one a
//! test can pin.
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

use plotweb_common::{fold_token, Calendar, LinkTarget, Note, TimePoint, TimeSpan};

use super::notes_filter::Filter;

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
/// The unassigned row's height, and the gap below it.
const UNASSIGNED_H: f32 = 28.0;
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
    /// The tie's extent — the topmost and bottommost participating lane, or the
    /// unassigned row for an event that names no entity.
    pub y0: f32,
    pub y1: f32,
    pub fuzzy: bool,
    pub open: bool,
    /// The selected note is this event, or takes part in it.
    pub on: bool,
    pub dots: Vec<Dot>,
    /// Names no entity with a lane — drawn in the unassigned row.
    pub unassigned: bool,
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
    /// The unassigned row's y, when any drawn event names no entity.
    pub unassigned_y: Option<f32>,
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

pub fn layout(input: &Input) -> Layout {
    let Input {
        notes,
        filter,
        calendar,
        order,
        selected,
    } = *input;
    let matches = |n: &Note| filter.matches(n);
    let entities: HashSet<&str> = notes
        .iter()
        .filter(|n| n.is_entity)
        .map(|n| n.id.as_str())
        .collect();

    // Dated, matching events. A dated *entity* is a lifespan on its own lane, not a tie:
    // "Vess, 1181 – 1211" is where her lane is live, not a moment she shares.
    let mut events: Vec<Placed> = Vec::new();
    for n in notes.iter().filter(|n| !n.is_entity && matches(n)) {
        let Some(span) = &n.span else { continue };
        let (start, end) = extent(calendar, span);
        events.push(Placed {
            note: n,
            start,
            end,
            who: participants(n, &entities),
        });
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
    let by_id: HashMap<&str, &Note> = notes.iter().map(|n| (n.id.as_str(), n)).collect();
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
    let lives_src: Vec<(&Note, i64, Option<i64>)> = lane_ids
        .iter()
        .filter_map(|(id, _)| by_id.get(id.as_str()))
        .filter_map(|n| {
            n.span.as_ref().map(|s| {
                let (a, b) = extent(calendar, s);
                (*n, a, b)
            })
        })
        .collect();

    // The visible span: everything drawn, padded a little each side.
    let mut lo = i64::MAX;
    let mut hi = i64::MIN;
    for (start, end) in events
        .iter()
        .map(|e| (e.start, e.end))
        .chain(lives_src.iter().map(|(_, a, b)| (*a, *b)))
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

    // Captions first: how many tiers they need decides where the lanes start.
    let mut order_x: Vec<usize> = (0..events.len()).collect();
    order_x.sort_by(|&a, &b| {
        events[a]
            .start
            .cmp(&events[b].start)
            .then_with(|| events[a].note.title.cmp(&events[b].note.title))
            .then_with(|| events[a].note.id.cmp(&events[b].note.id))
    });
    let mut cap_text: Vec<(usize, String, f32)> = Vec::new();
    for &i in &order_x {
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
    let mut rows_top = cap_top + n_tiers as f32 * TIER_H + 8.0;
    let any_unassigned = events.iter().any(|e| e.who.is_empty());
    let unassigned_y = any_unassigned.then(|| {
        let y = rows_top + 10.0;
        rows_top += UNASSIGNED_H;
        y
    });

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
    let height = rows_top + 14.0 + (lanes.len().max(1) as f32 - 1.0) * LANE_H + 20.0;

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

    let mut marks: Vec<Mark> = Vec::new();
    let mut captions: Vec<Caption> = Vec::new();
    for (k, (i, text, tx)) in cap_text.into_iter().enumerate() {
        let ev = &events[i];
        let (x0, x1) = band(ev.start, ev.end);
        let on = selected.is_some_and(|s| s == ev.note.id || ev.who.iter().any(|w| w == s));
        let (y0, y1, dots) = match unassigned_y {
            Some(uy) if ev.who.is_empty() => (
                uy,
                uy,
                vec![Dot {
                    lane: String::new(),
                    y: uy,
                    on: false,
                }],
            ),
            _ => {
                let ys: Vec<f32> = ev.who.iter().map(|w| lane_y[w]).collect();
                let y0 = ys.iter().copied().fold(f32::MAX, f32::min);
                let y1 = ys.iter().copied().fold(f32::MIN, f32::max);
                let dots = ev
                    .who
                    .iter()
                    .map(|w| Dot {
                        lane: w.clone(),
                        y: lane_y[w],
                        on: selected == Some(w.as_str()),
                    })
                    .collect();
                (y0, y1, dots)
            }
        };
        let span = ev.note.span.as_ref();
        marks.push(Mark {
            id: ev.note.id.clone(),
            title: ev.note.title.clone(),
            x0,
            x1,
            cx: (x0 + x1) / 2.0,
            y0,
            y1,
            fuzzy: span.is_some_and(|s| s.approximate),
            open: ev.end.is_none(),
            on,
            dots,
            unassigned: ev.who.is_empty(),
        });
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
            leader_y1: y0 - 10.0,
            on,
        });
    }

    // The holding rail: notes that happen but are not on the line yet — placed only
    // against another note, or naming people with `$` and no date. Plain undated lore
    // stays out, or a lore-heavy book would bury the rail.
    let mut held: Vec<Held> = notes
        .iter()
        .filter(|n| n.span.is_none() && !n.is_entity && matches(n))
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
        unassigned_y,
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

    fn run(notes: &[Note], filter: &Filter, order: LaneOrder, selected: Option<&str>) -> Layout {
        let cal = Calendar::default();
        layout(&Input {
            notes,
            filter,
            calendar: &cal,
            order,
            selected,
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
        // A dated entity is not a tie, and not an unassigned event either.
        assert!(l.marks.iter().all(|m| m.id != "Maera"));
        assert_eq!(l.unassigned_y, None);
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

    // ── The unassigned row ──

    #[test]
    fn an_event_naming_nobody_goes_in_the_unassigned_row_rather_than_vanishing() {
        let mut notes = cast();
        notes.push(event("comet", 1205, &[]));
        // A $ref to something that is not an entity is still nobody.
        notes.push(note("rumour"));
        notes.push(event("fire", 1206, &["rumour"]));
        let l = run(&notes, &Filter::default(), LaneOrder::FirstAppearance, None);
        let uy = l.unassigned_y.expect("an unassigned row");
        let mut un: Vec<&str> = l.marks.iter().filter(|m| m.unassigned).map(|m| m.id.as_str()).collect();
        un.sort();
        assert_eq!(un, vec!["comet", "fire"]);
        for m in l.marks.iter().filter(|m| m.unassigned) {
            assert_eq!((m.y0, m.y1), (uy, uy));
        }
        // It sits above every lane — where card 6's ribbon will go.
        assert!(l.lanes.iter().all(|x| x.y > uy));
        // Its captions are placed like any other.
        assert!(l.captions.iter().any(|c| c.id == "comet"));
    }

    #[test]
    fn no_unassigned_row_when_every_event_names_someone() {
        let l = run(&cast(), &Filter::default(), LaneOrder::FirstAppearance, None);
        assert_eq!(l.unassigned_y, None);
        assert!(l.marks.iter().all(|m| !m.unassigned));
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
}
