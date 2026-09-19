//! The event ribbon's rules — nesting, the span rule, packing, labels (notes card 6).
//!
//! `design/04-notes-wireframes.html`, "The chosen shape — A and C stacked": event bars
//! above the entity lanes, each nested event in a sub-row inside its parent's
//! containment band. Everything here is pure and host-tested; `timeline_layout` places
//! the result on the shared x axis and `panes::timeline` only draws it.
//!
//! # Nesting is `event_parent`, and only `event_parent`
//!
//! Containment comes from the explicit `Note::event_parent` and is never inferred from
//! two spans overlapping — the parley and the siege coincide without one holding the
//! other. It is a separate hierarchy from the tree (`NoteTree`), which nothing here reads.
//!
//! A corrupt `event_parent` loop (two devices each nesting one note under the other,
//! merged) must not hang the page. [`break_cycles`] drops the parent link of every note
//! on a loop, so each of them draws as a root, and everything after it walks a forest.
//!
//! # The span rule
//!
//! A per-book setting (`plotweb_common::SpanRule`), applied **when drawing**. No rule
//! ever rewrites a typed span: unpinning, re-parenting or deleting a child changes the
//! drawing and nothing else.
//!
//! * **Fit** — a parent is drawn across its own dates plus its children's fitted dates,
//!   recursively, unless it is pinned. A pinned parent is drawn as typed, and a child
//!   running past it is flagged exactly as under Free: pinning says "these dates are
//!   right", so a child outside them is worth pointing at.
//! * **Clamp** — a parent's drawn dates win; a child running past them is cut off at
//!   the parent's edge and marked. Transitively: a grandparent's dates clamp the
//!   grandchildren too, through whatever the parent was clamped to.
//! * **Free** — drawn as typed; a child escaping its parent pokes out and is flagged.
//!
//! **A parent with no dates of its own** is fitted from its children under every rule.
//! It has nothing typed to be authoritative about, so there is nothing to clamp against
//! and nothing to escape from — and without a fitted extent its band would have nowhere
//! to be drawn. Pinning one changes nothing, for the same reason.

use std::collections::{BTreeMap, HashMap, HashSet};

use plotweb_common::SpanRule;

/// A time extent in ticks: `(start, end)`, `end = None` for open-ended (runs to the
/// right edge of whatever is on screen).
pub type Extent = (i64, Option<i64>);

/// The later of two ends, where `None` (open-ended) is later than any tick.
fn max_end(a: Option<i64>, b: Option<i64>) -> Option<i64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.max(b)),
        _ => None,
    }
}

/// The earlier of two ends, where `None` is later than any tick.
fn min_end(a: Option<i64>, b: Option<i64>) -> Option<i64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) | (None, Some(a)) => Some(a),
        (None, None) => None,
    }
}

pub fn union(a: Extent, b: Extent) -> Extent {
    (a.0.min(b.0), max_end(a.1, b.1))
}

/// One event as the ribbon sees it, before any rule applies.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Node {
    /// Its own typed extent, if it has dates.
    pub typed: Option<Extent>,
    pub pinned: bool,
}

/// Drop the parent link of every note on an `event_parent` loop, and of every note whose
/// parent is itself. Links into a loop from outside it survive: those notes nest under
/// a loop member, which now draws as a root.
///
/// Each note is walked once overall, so this is linear however the links are tangled.
pub fn break_cycles(parent: &HashMap<String, String>) -> HashMap<String, String> {
    let mut on_cycle: HashSet<&str> = HashSet::new();
    let mut done: HashSet<&str> = HashSet::new();
    let mut starts: Vec<&String> = parent.keys().collect();
    starts.sort();
    for start in starts {
        let mut path: Vec<&str> = Vec::new();
        let mut at: HashMap<&str, usize> = HashMap::new();
        let mut cur: &str = start.as_str();
        loop {
            if done.contains(cur) {
                break;
            }
            if let Some(&i) = at.get(cur) {
                on_cycle.extend(path[i..].iter().copied());
                break;
            }
            at.insert(cur, path.len());
            path.push(cur);
            match parent.get(cur) {
                Some(p) => cur = p.as_str(),
                None => break,
            }
        }
        done.extend(path);
    }
    parent
        .iter()
        .filter(|(k, _)| !on_cycle.contains(k.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

/// Whether nesting `child` under `new_parent` would make a loop: `new_parent` is
/// `child`, or already sits (at any depth) inside it. Walks `parent` with a visited set,
/// so a loop already in the data cannot spin it.
pub fn would_cycle(parent: &HashMap<String, String>, child: &str, new_parent: &str) -> bool {
    let mut seen: HashSet<&str> = HashSet::new();
    let mut cur = new_parent;
    loop {
        if cur == child {
            return true;
        }
        if !seen.insert(cur) {
            return false;
        }
        match parent.get(cur) {
            Some(p) => cur = p.as_str(),
            None => return false,
        }
    }
}

/// What dropping a dragged bar should do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Nest {
    /// Set `event_parent` to this note.
    Under(String),
    /// Clear `event_parent` (a tombstone).
    Out,
    /// Already so; write nothing.
    Unchanged,
    /// The drop would make a loop; refuse it.
    Refused,
}

/// The drop of `dragged` onto `target` — a bar's id, or `None` for the empty ribbon.
/// `parent` is the **raw** `event_parent` map: refusing is about what would be stored.
pub fn nest_drop(parent: &HashMap<String, String>, dragged: &str, target: Option<&str>) -> Nest {
    match target {
        None => {
            if parent.contains_key(dragged) {
                Nest::Out
            } else {
                Nest::Unchanged
            }
        }
        Some(t) if t == dragged => Nest::Unchanged,
        Some(t) => {
            if parent.get(dragged).map(String::as_str) == Some(t) {
                Nest::Unchanged
            } else if would_cycle(parent, dragged, t) {
                Nest::Refused
            } else {
                Nest::Under(t.to_string())
            }
        }
    }
}

/// How one event is drawn once the book's rule is applied.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Drawn {
    pub start: i64,
    pub end: Option<i64>,
    /// Drawn wider than its own typed dates — the auto-fit at work.
    pub fitted: bool,
    /// Has no dates of its own; drawn entirely from its children.
    pub derived: bool,
    /// Cut off at its parent's edge (Clamp).
    pub clipped_start: bool,
    pub clipped_end: bool,
    /// Runs past its parent's drawn edge (Free, or a pinned parent under Fit).
    pub escapes_start: bool,
    pub escapes_end: bool,
}

/// Apply `rule` to every event.
///
/// `nodes` are the events; `parent` must already be cycle-free ([`break_cycles`]) and
/// name only keys of `nodes`. An event with no dates and no dated descendant has no
/// extent under any rule and is absent from the result.
pub fn resolve(
    nodes: &BTreeMap<String, Node>,
    parent: &HashMap<String, String>,
    rule: SpanRule,
) -> BTreeMap<String, Drawn> {
    let mut children: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (c, p) in parent {
        if nodes.contains_key(c) && nodes.contains_key(p) {
            children.entry(p.as_str()).or_default().push(c.as_str());
        }
    }

    // Bottom-up: a post-order over the forest, iterative so a long chain cannot
    // overflow the stack. `natural` is typed-or-derived; `fit` is the auto-fit extent.
    let order = post_order(nodes, &children, parent);
    let mut natural: HashMap<&str, Extent> = HashMap::new();
    let mut fit: HashMap<&str, Extent> = HashMap::new();
    for id in &order {
        let node = &nodes[*id];
        let kids = children.get(id).map(Vec::as_slice).unwrap_or(&[]);
        let kid_nat = kids.iter().filter_map(|k| natural.get(k).copied()).reduce(union);
        let kid_fit = kids.iter().filter_map(|k| fit.get(k).copied()).reduce(union);
        if let Some(n) = node.typed.or(kid_nat) {
            natural.insert(id, n);
        }
        let f = match (node.typed, node.pinned) {
            (Some(t), true) => Some(t),
            (Some(t), false) => Some(kid_fit.map_or(t, |k| union(t, k))),
            (None, _) => kid_fit,
        };
        if let Some(f) = f {
            fit.insert(id, f);
        }
    }

    // Top-down: clamping, and the escape flags every rule reports.
    let mut out: BTreeMap<String, Drawn> = BTreeMap::new();
    for id in order.iter().rev() {
        let node = &nodes[*id];
        let own = match rule {
            SpanRule::Fit => fit.get(id).copied(),
            SpanRule::Clamp | SpanRule::Free => natural.get(id).copied(),
        };
        let Some((mut start, mut end)) = own else { continue };
        let mut d = Drawn {
            derived: node.typed.is_none(),
            ..Default::default()
        };
        if let Some(t) = node.typed {
            d.fitted = start < t.0 || match (end, t.1) {
                (None, Some(_)) => true,
                (Some(e), Some(te)) => e > te,
                _ => false,
            };
        }
        let up = parent.get(*id).and_then(|p| out.get(p.as_str()));
        if let Some(p) = up {
            let (ps, pe) = (p.start, p.end);
            let before = start < ps;
            let after = match (end, pe) {
                (_, None) => false,
                (None, Some(_)) => true,
                (Some(e), Some(pe)) => e > pe,
            };
            if rule == SpanRule::Clamp {
                if before || after {
                    d.clipped_start = before;
                    d.clipped_end = after;
                    start = start.max(ps);
                    end = min_end(end, pe);
                    // Wholly outside: a sliver at the edge it ran off.
                    if let Some(e) = end
                        && e <= start
                    {
                        if before && !after {
                            start = ps;
                            end = Some(ps + 1);
                        } else {
                            let edge = pe.unwrap_or(start);
                            start = edge - 1;
                            end = Some(edge);
                        }
                    }
                }
            } else {
                d.escapes_start = before;
                d.escapes_end = after;
            }
        }
        d.start = start;
        d.end = end;
        out.insert(id.to_string(), d);
    }
    out
}

/// Every node, children before their parent. Roots in key order, children likewise, so
/// the order — and everything drawn from it — is deterministic.
fn post_order<'a>(
    nodes: &'a BTreeMap<String, Node>,
    children: &BTreeMap<&'a str, Vec<&'a str>>,
    parent: &HashMap<String, String>,
) -> Vec<&'a str> {
    let mut out = Vec::with_capacity(nodes.len());
    let mut seen: HashSet<&str> = HashSet::new();
    let roots = nodes
        .keys()
        .map(String::as_str)
        .filter(|id| !parent.get(*id).is_some_and(|p| nodes.contains_key(p)));
    for root in roots {
        // (node, whether its children have been pushed)
        let mut stack: Vec<(&str, bool)> = vec![(root, false)];
        while let Some((id, expanded)) = stack.pop() {
            if expanded {
                out.push(id);
                continue;
            }
            if !seen.insert(id) {
                continue;
            }
            stack.push((id, true));
            if let Some(kids) = children.get(id) {
                let mut kids = kids.clone();
                kids.sort();
                for k in kids.into_iter().rev() {
                    stack.push((k, false));
                }
            }
        }
    }
    out
}

// ── Packing ──────────────────────────────────────────────────────────────────

/// Place blocks into rows. Each item is `(x0, x1, height)` in **left-to-right order**;
/// it takes the lowest row `r` such that rows `r .. r + height` are all free at `x0`
/// (their last occupant ended at least `gap` before it). Returns each item's row.
///
/// The same packer runs at the top level and inside every containment band, which is
/// what makes a parent's children sub-rows of that parent rather than of the ribbon.
pub fn pack_rows(items: &[(f32, f32, usize)], gap: f32) -> Vec<usize> {
    let mut ends: Vec<f32> = Vec::new();
    let mut out = Vec::with_capacity(items.len());
    for &(x0, x1, height) in items {
        let height = height.max(1);
        let mut row = 0;
        loop {
            let clash = (row..row + height).any(|r| ends.get(r).is_some_and(|&e| e + gap > x0));
            if !clash {
                break;
            }
            row += 1;
        }
        if ends.len() < row + height {
            ends.resize(row + height, f32::MIN);
        }
        for e in &mut ends[row..row + height] {
            *e = x1;
        }
        out.push(row);
    }
    out
}

// ── Labels ───────────────────────────────────────────────────────────────────

/// Where a bar's label went.
#[derive(Debug, Clone, PartialEq)]
pub enum LabelSpot {
    /// Inside the bar, starting at `x`.
    Inside { x: f32, text: String },
    /// In the gap beside it on the same row: starting at `x` (`end = false`) or ending
    /// at `x` (`end = true`, the gap to its left).
    Beside { x: f32, end: bool, text: String },
    /// Nowhere to go. The full name is still the bar's tooltip, and the count of these
    /// is shown rather than hidden.
    Dropped,
}

/// Room the label needs inside a bar before it goes there rather than beside it, in
/// drawing units — the wireframe's threshold. Below it an inside label is three letters
/// and an ellipsis, which reads worse than the name beside the bar.
pub const INSIDE_MIN: f32 = 34.0;
const PAD: f32 = 5.0;
const GAP: f32 = 6.0;

/// Place a label for every bar on one row. `bars` are `(x0, x1, title)`, **sorted by
/// x0**; `left`/`right` bound the row. In order of preference:
///
/// 1. inside the bar, whole;
/// 2. whole, in the wider of the gaps beside it — gaps already narrowed by the bars and
///    by labels placed before it on this row;
/// 3. inside, cut short, when the bar has [`INSIDE_MIN`] of room;
/// 4. cut short in that gap;
/// 5. nowhere.
///
/// `fit` cuts a title to a width (`timeline_layout::fit`).
pub fn place_labels(
    bars: &[(f32, f32, &str)],
    left: f32,
    right: f32,
    fit: impl Fn(&str, f32) -> String,
) -> Vec<LabelSpot> {
    // What is already taken on the row, as intervals: every bar, then placed labels.
    let mut taken: Vec<(f32, f32)> = bars.iter().map(|(a, b, _)| (*a, *b)).collect();
    let mut out = Vec::with_capacity(bars.len());
    for (i, &(x0, x1, title)) in bars.iter().enumerate() {
        let inner = x1 - x0 - 2.0 * PAD + 2.0;
        let whole = |room: f32| fit(title, room) == title;
        if whole(inner) {
            out.push(LabelSpot::Inside { x: x0 + PAD, text: title.to_string() });
            continue;
        }
        let others = || taken.iter().enumerate().filter(move |(j, _)| *j != i).map(|(_, t)| *t);
        let next = others().filter(|(a, _)| *a >= x1).map(|(a, _)| a).fold(right, f32::min);
        let prev = others().filter(|(_, b)| *b <= x0).map(|(_, b)| b).fold(left, f32::max);
        let right_room = next - (x1 + PAD) - GAP;
        let left_room = (x0 - PAD) - prev - GAP;
        let (spot, room) = if right_room >= left_room {
            ((x1 + PAD, false), right_room)
        } else {
            ((x0 - PAD, true), left_room)
        };
        if !whole(room) && inner >= INSIDE_MIN {
            let text = fit(title, inner);
            out.push(if text.is_empty() {
                LabelSpot::Dropped
            } else {
                LabelSpot::Inside { x: x0 + PAD, text }
            });
            continue;
        }
        let text = fit(title, room);
        if text.is_empty() {
            out.push(LabelSpot::Dropped);
            continue;
        }
        let w = room.min(text.chars().count() as f32 * 5.6);
        taken.push(if spot.1 { (spot.0 - w, spot.0) } else { (spot.0, spot.0 + w) });
        out.push(LabelSpot::Beside {
            x: spot.0,
            end: spot.1,
            text,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nodes(list: &[(&str, Option<Extent>, bool)]) -> BTreeMap<String, Node> {
        list.iter()
            .map(|(id, typed, pinned)| {
                (
                    id.to_string(),
                    Node {
                        typed: *typed,
                        pinned: *pinned,
                    },
                )
            })
            .collect()
    }

    fn parents(list: &[(&str, &str)]) -> HashMap<String, String> {
        list.iter().map(|(c, p)| (c.to_string(), p.to_string())).collect()
    }

    fn ext(d: &Drawn) -> Extent {
        (d.start, d.end)
    }

    /// The siege (10–20) holds the breach (15–25), which runs past it.
    fn siege() -> (BTreeMap<String, Node>, HashMap<String, String>) {
        (
            nodes(&[("siege", Some((10, Some(20))), false), ("breach", Some((15, Some(25))), false)]),
            parents(&[("breach", "siege")]),
        )
    }

    // ── The three rules ──

    #[test]
    fn fit_stretches_the_parent_over_its_child_and_never_touches_the_child() {
        let (n, p) = siege();
        let d = resolve(&n, &p, SpanRule::Fit);
        assert_eq!(ext(&d["siege"]), (10, Some(25)));
        assert!(d["siege"].fitted);
        assert_eq!(ext(&d["breach"]), (15, Some(25)), "the child is drawn as typed");
        assert!(!d["breach"].escapes_end, "a fitted parent always contains its children");
        // The typed span is an input, not an output: nothing here writes it.
        assert_eq!(n["siege"].typed, Some((10, Some(20))));
    }

    #[test]
    fn clamp_cuts_the_child_at_the_parents_edge_and_marks_it() {
        let (n, p) = siege();
        let d = resolve(&n, &p, SpanRule::Clamp);
        assert_eq!(ext(&d["siege"]), (10, Some(20)));
        assert!(!d["siege"].fitted);
        assert_eq!(ext(&d["breach"]), (15, Some(20)));
        assert!(d["breach"].clipped_end && !d["breach"].clipped_start);
        assert!(!d["breach"].escapes_end);
    }

    #[test]
    fn free_draws_as_typed_and_flags_the_escape() {
        let (n, p) = siege();
        let d = resolve(&n, &p, SpanRule::Free);
        assert_eq!(ext(&d["siege"]), (10, Some(20)));
        assert_eq!(ext(&d["breach"]), (15, Some(25)));
        assert!(d["breach"].escapes_end && !d["breach"].escapes_start);
        assert!(!d["breach"].clipped_end);
    }

    #[test]
    fn a_child_inside_its_parent_is_left_alone_by_every_rule() {
        let n = nodes(&[("siege", Some((10, Some(20))), false), ("parley", Some((12, Some(13))), false)]);
        let p = parents(&[("parley", "siege")]);
        for rule in [SpanRule::Fit, SpanRule::Clamp, SpanRule::Free] {
            let d = resolve(&n, &p, rule);
            assert_eq!(ext(&d["siege"]), (10, Some(20)), "{rule:?}");
            assert_eq!(ext(&d["parley"]), (12, Some(13)), "{rule:?}");
            let f = &d["parley"];
            assert!(!(f.clipped_start || f.clipped_end || f.escapes_start || f.escapes_end), "{rule:?}");
            assert!(!d["siege"].fitted);
        }
    }

    #[test]
    fn clamp_a_child_wholly_outside_leaves_a_sliver_at_the_edge_it_ran_off() {
        let n = nodes(&[("siege", Some((10, Some(20))), false), ("later", Some((30, Some(40))), false)]);
        let p = parents(&[("later", "siege")]);
        let d = resolve(&n, &p, SpanRule::Clamp);
        assert_eq!(ext(&d["later"]), (19, Some(20)));
        assert!(d["later"].clipped_end);
        let n = nodes(&[("siege", Some((10, Some(20))), false), ("earlier", Some((0, Some(5))), false)]);
        let d = resolve(&n, &parents(&[("earlier", "siege")]), SpanRule::Clamp);
        assert_eq!(ext(&d["earlier"]), (10, Some(11)));
        assert!(d["earlier"].clipped_start);
    }

    #[test]
    fn an_open_ended_parent_contains_everything_after_its_start() {
        let n = nodes(&[("reign", Some((10, None)), false), ("war", Some((50, Some(90))), false)]);
        let p = parents(&[("war", "reign")]);
        for rule in [SpanRule::Clamp, SpanRule::Free, SpanRule::Fit] {
            let d = resolve(&n, &p, rule);
            assert_eq!(ext(&d["reign"]), (10, None), "{rule:?}");
            assert!(!d["war"].clipped_end && !d["war"].escapes_end, "{rule:?}");
        }
        // And an open-ended child under a closed parent runs past it.
        let n = nodes(&[("siege", Some((10, Some(20))), false), ("exile", Some((15, None)), false)]);
        let p = parents(&[("exile", "siege")]);
        assert_eq!(ext(&resolve(&n, &p, SpanRule::Fit)["siege"]), (10, None));
        assert_eq!(ext(&resolve(&n, &p, SpanRule::Clamp)["exile"]), (15, Some(20)));
        assert!(resolve(&n, &p, SpanRule::Free)["exile"].escapes_end);
    }

    // ── Pinned parents ──

    #[test]
    fn a_pinned_parent_keeps_its_dates_under_fit_and_its_child_is_flagged() {
        let (mut n, p) = siege();
        n.get_mut("siege").unwrap().pinned = true;
        let d = resolve(&n, &p, SpanRule::Fit);
        assert_eq!(ext(&d["siege"]), (10, Some(20)));
        assert!(!d["siege"].fitted);
        assert!(d["breach"].escapes_end);
        // Unpinning undoes it by itself: nothing was written.
        n.get_mut("siege").unwrap().pinned = false;
        assert_eq!(ext(&resolve(&n, &p, SpanRule::Fit)["siege"]), (10, Some(25)));
    }

    #[test]
    fn re_parenting_or_deleting_a_child_undoes_the_fit() {
        let (n, _) = siege();
        let d = resolve(&n, &HashMap::new(), SpanRule::Fit);
        assert_eq!(ext(&d["siege"]), (10, Some(20)));
        let mut gone = n.clone();
        gone.remove("breach");
        // A parent link to a note no longer present is ignored.
        let d = resolve(&gone, &parents(&[("breach", "siege")]), SpanRule::Fit);
        assert_eq!(ext(&d["siege"]), (10, Some(20)));
    }

    // ── Date-less parents ──

    #[test]
    fn a_dateless_parent_is_fitted_from_its_children_under_every_rule() {
        let n = nodes(&[
            ("act two", None, false),
            ("siege", Some((10, Some(20))), false),
            ("winter", Some((30, Some(35))), false),
        ]);
        let p = parents(&[("siege", "act two"), ("winter", "act two")]);
        for rule in [SpanRule::Fit, SpanRule::Clamp, SpanRule::Free] {
            let d = resolve(&n, &p, rule);
            assert_eq!(ext(&d["act two"]), (10, Some(35)), "{rule:?}");
            assert!(d["act two"].derived);
            for k in ["siege", "winter"] {
                let f = &d[k];
                assert!(!(f.clipped_start || f.clipped_end || f.escapes_start || f.escapes_end), "{rule:?} {k}");
            }
        }
        // Pinning one changes nothing — there are no dates to pin it to.
        let mut pinned = n.clone();
        pinned.get_mut("act two").unwrap().pinned = true;
        assert_eq!(ext(&resolve(&pinned, &p, SpanRule::Fit)["act two"]), (10, Some(35)));
    }

    #[test]
    fn a_dateless_note_with_no_dated_descendant_is_not_drawn() {
        let n = nodes(&[("lore", None, false), ("more lore", None, false)]);
        let d = resolve(&n, &parents(&[("more lore", "lore")]), SpanRule::Fit);
        assert!(d.is_empty());
    }

    // ── Deep nesting ──

    #[test]
    fn fit_climbs_every_level_and_clamp_reaches_down_every_level() {
        // war ⊃ siege ⊃ breach ⊃ the gate falls; only the innermost runs late.
        let n = nodes(&[
            ("war", Some((0, Some(100))), false),
            ("siege", Some((10, Some(50))), false),
            ("breach", Some((20, Some(40))), false),
            ("gate", Some((30, Some(120))), false),
        ]);
        let p = parents(&[("siege", "war"), ("breach", "siege"), ("gate", "breach")]);
        let fit = resolve(&n, &p, SpanRule::Fit);
        for id in ["war", "siege", "breach", "gate"] {
            assert_eq!(fit[id].end, Some(120), "{id} fits the gate");
        }
        let clamp = resolve(&n, &p, SpanRule::Clamp);
        assert_eq!(ext(&clamp["gate"]), (30, Some(40)), "cut at the breach, the nearest edge");
        assert!(clamp["gate"].clipped_end);
        let free = resolve(&n, &p, SpanRule::Free);
        assert!(free["gate"].escapes_end);
        assert!(!free["breach"].escapes_end);
    }

    #[test]
    fn clamp_is_transitive_through_a_dateless_middle() {
        let n = nodes(&[
            ("siege", Some((10, Some(20))), false),
            ("the night", None, false),
            ("fire", Some((18, Some(30))), false),
        ]);
        let p = parents(&[("the night", "siege"), ("fire", "the night")]);
        let d = resolve(&n, &p, SpanRule::Clamp);
        assert_eq!(ext(&d["the night"]), (18, Some(20)));
        assert!(d["the night"].clipped_end);
        assert_eq!(ext(&d["fire"]), (18, Some(20)));
    }

    #[test]
    fn a_long_chain_does_not_overflow_the_stack() {
        let mut list = Vec::new();
        let ids: Vec<String> = (0..5000).map(|i| format!("n{i:05}")).collect();
        for (i, id) in ids.iter().enumerate() {
            list.push((id.clone(), Node { typed: Some((i as i64, Some(i as i64 + 1))), pinned: false }));
        }
        let n: BTreeMap<String, Node> = list.into_iter().collect();
        let p: HashMap<String, String> =
            ids.windows(2).map(|w| (w[1].clone(), w[0].clone())).collect();
        let d = resolve(&n, &p, SpanRule::Fit);
        assert_eq!(ext(&d["n00000"]), (0, Some(5000)));
    }

    // ── Cycles ──

    #[test]
    fn a_corrupt_loop_is_broken_so_its_members_draw_as_roots() {
        let p = parents(&[("a", "b"), ("b", "c"), ("c", "a"), ("tail", "a"), ("x", "x"), ("ok", "root")]);
        let clean = break_cycles(&p);
        assert!(!clean.contains_key("a") && !clean.contains_key("b") && !clean.contains_key("c"));
        assert!(!clean.contains_key("x"), "a self-parent is a loop of one");
        assert_eq!(clean.get("tail").map(String::as_str), Some("a"), "a link into the loop survives");
        assert_eq!(clean.get("ok").map(String::as_str), Some("root"));

        let n = nodes(&[
            ("a", Some((0, Some(1))), false),
            ("b", Some((5, Some(6))), false),
            ("c", Some((9, Some(10))), false),
            ("tail", Some((20, Some(30))), false),
        ]);
        let d = resolve(&n, &clean, SpanRule::Fit);
        assert_eq!(ext(&d["b"]), (5, Some(6)));
        assert_eq!(ext(&d["a"]), (0, Some(30)), "a fits its surviving child");
    }

    #[test]
    fn a_drop_that_would_make_a_loop_is_refused() {
        let p = parents(&[("breach", "siege"), ("gate", "breach")]);
        assert_eq!(nest_drop(&p, "siege", Some("gate")), Nest::Refused);
        assert_eq!(nest_drop(&p, "siege", Some("breach")), Nest::Refused);
        assert_eq!(nest_drop(&p, "siege", Some("siege")), Nest::Unchanged);
        assert_eq!(nest_drop(&p, "gate", Some("siege")), Nest::Under("siege".into()));
        assert_eq!(nest_drop(&p, "gate", Some("breach")), Nest::Unchanged);
        assert_eq!(nest_drop(&p, "gate", None), Nest::Out);
        assert_eq!(nest_drop(&p, "siege", None), Nest::Unchanged, "a root has nothing to leave");
        // Walking a loop already in the data terminates.
        let looped = parents(&[("a", "b"), ("b", "a")]);
        assert_eq!(nest_drop(&looped, "c", Some("a")), Nest::Under("a".into()));
        assert!(would_cycle(&looped, "a", "b"));
    }

    // ── Packing ──

    #[test]
    fn packing_stacks_only_what_overlaps() {
        let rows = pack_rows(&[(0.0, 50.0, 1), (10.0, 20.0, 1), (60.0, 90.0, 1), (15.0, 40.0, 1)], 3.0);
        assert_eq!(rows, vec![0, 1, 0, 2]);
    }

    #[test]
    fn a_tall_block_needs_every_row_it_spans_free() {
        // A two-row block (a parent with one row of children) cannot slot into a gap
        // one row deep: row 0 is free at x=40, row 1 is not.
        let rows = pack_rows(&[(0.0, 30.0, 1), (0.0, 100.0, 1), (40.0, 80.0, 2)], 3.0);
        assert_eq!(rows, vec![0, 1, 2]);
        let rows = pack_rows(&[(0.0, 30.0, 2), (40.0, 80.0, 2)], 3.0);
        assert_eq!(rows, vec![0, 0]);
    }

    // ── Labels ──

    fn cut(t: &str, room: f32) -> String {
        super::super::timeline_layout::fit(t, room, 5.6)
    }

    #[test]
    fn a_label_goes_inside_when_it_fits_else_beside_else_nowhere() {
        let bars = [(0.0, 200.0, "The siege of Harrowgate"), (210.0, 216.0, "Breach"), (300.0, 306.0, "Parley")];
        let spots = place_labels(&bars, 0.0, 400.0, cut);
        assert!(matches!(&spots[0], LabelSpot::Inside { text, .. } if text == "The siege of Harrowgate"));
        assert!(matches!(&spots[1], LabelSpot::Beside { end: false, text, .. } if text == "Breach"));
        assert!(matches!(&spots[2], LabelSpot::Beside { end: false, .. }));

        // Hemmed in on both sides: nowhere to go.
        let bars = [(0.0, 100.0, "Left"), (103.0, 106.0, "Squeezed"), (109.0, 300.0, "Right")];
        let spots = place_labels(&bars, 0.0, 300.0, cut);
        assert_eq!(spots[1], LabelSpot::Dropped);
    }

    #[test]
    fn a_name_that_fits_whole_beside_its_bar_goes_there_rather_than_cut_short_inside() {
        // Room inside for "Winter qua…", room beside for all of it.
        let bars = [(0.0, 70.0, "Winter quarters"), (300.0, 310.0, "Next")];
        let spots = place_labels(&bars, 0.0, 400.0, cut);
        assert!(matches!(&spots[0], LabelSpot::Beside { end: false, text, .. } if text == "Winter quarters"));
        // Hemmed in beside: then it is cut short inside, rather than dropped.
        let bars = [(0.0, 70.0, "Winter quarters"), (80.0, 400.0, "Wall")];
        let spots = place_labels(&bars, 0.0, 400.0, cut);
        assert!(matches!(&spots[0], LabelSpot::Inside { text, .. } if text.ends_with('\u{2026}')));
    }

    #[test]
    fn a_label_takes_the_wider_gap_and_the_next_label_sees_it() {
        // Room on the left only.
        let bars = [(200.0, 204.0, "Breach"), (215.0, 400.0, "Winter quarters")];
        let spots = place_labels(&bars, 0.0, 400.0, cut);
        assert!(matches!(&spots[0], LabelSpot::Beside { end: true, .. }));
        // Two narrow bars side by side: the first label fills the gap the second wanted.
        let bars = [(0.0, 4.0, "Alpha event"), (80.0, 84.0, "Beta"), (90.0, 400.0, "Wall")];
        let spots = place_labels(&bars, 0.0, 400.0, cut);
        assert!(matches!(&spots[0], LabelSpot::Beside { end: false, .. }));
        match &spots[1] {
            LabelSpot::Beside { end: true, x, .. } => assert!(*x <= 80.0),
            other => assert_eq!(other, &LabelSpot::Dropped),
        }
    }
}
