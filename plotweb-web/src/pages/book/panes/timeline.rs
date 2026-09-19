//! The notes timeline — the event ribbon over the entity lanes (notes cards 5 and 6).
//!
//! Draws a [`timeline_layout::Layout`] as SVG and wires its gestures:
//!
//! * **follow** an entity (the chip row): selects it, which lights its lane, every event
//!   it takes part in, and those events' bars on the ribbon. Selection is
//!   `BookState::notes_selected`, the same one the tree highlights, so it survives
//!   switching view.
//! * **open** a note (a bar or its label, a tie, a caption, a lane's name, a held note):
//!   an **exit** from whatever was being edited, through [`super::notes::open_note_row`],
//!   which calls `flush_pending_edits` before `active_pane` moves.
//! * **nest** an event by dragging its bar's handle onto another bar, which sets its
//!   `event_parent`; dragging it onto empty ribbon takes it out. Written through
//!   [`super::note_editor::write_event_parent`], which touches nothing else — never the
//!   tree parent, never a span. A drop that would make a loop is refused.
//! * **date** a held note by dragging it from the holding rail onto the line, which
//!   writes a span through [`super::note_editor::write_note_time`] — the time field's own
//!   write path, tombstone-aware, not a second one.
//!
//! Every rule of the drawing lives in `timeline_layout` and `ribbon`, host-tested;
//! nothing here decides anything, it only places what the layout says. The layout reads
//! the projected note list — structure-document data — and never a note body.
//!
//! **Wide drawing, narrow screen.** The SVG has a minimum width and sits in its own
//! `overflow-x: auto` box, so at 390px the drawing scrolls sideways inside itself and
//! the page body never does. Card 7 replaces this with the phone spine.

use std::collections::HashMap;

use rinch::prelude::*;
use plotweb_common::{SpanRule, TimeSpan};

use crate::store::AppStore;

use super::super::ribbon::{self, Nest};
use super::super::state::BookState;
use super::super::timeline_layout::{self as tl, Layout};

/// The book's span rule — its own, or auto-fit.
pub(in crate::pages::book) fn book_span_rule(store: AppStore) -> SpanRule {
    SpanRule::effective(store.current_book.get().and_then(|b| b.span_rule))
}

/// The layout for what is on screen now. Recomputed per read — the same reasoning as
/// `notes::tree_filter`: a walk over a few hundred notes, and a cache would be one more
/// thing that could disagree with them.
fn current(state: BookState, store: AppStore) -> Layout {
    let notes = store.notes.get();
    let filter = state.notes_filter.get();
    let calendar = super::calendar::book_calendar(store);
    let selected = state.notes_selected.get();
    tl::layout(&tl::Input {
        notes: &notes,
        filter: &filter,
        calendar: &calendar,
        order: state.timeline_order.get(),
        selected: selected.as_deref(),
        rule: book_span_rule(store),
        ribbon: state.timeline_ribbon.get(),
        lanes: state.timeline_lanes.get(),
    })
}

fn n(v: f32) -> String {
    format!("{:.1}", v)
}

/// The lanes an event ties — what a caption needs to know to light up with it.
fn who_of(state: BookState, store: AppStore, id: &str) -> Vec<String> {
    current(state, store)
        .marks
        .into_iter()
        .find(|m| m.id == id)
        .map(|m| m.dots.into_iter().map(|d| d.lane).collect())
        .unwrap_or_default()
}

/// Follow (or stop following) an entity. Not an exit — nothing opens.
fn toggle_follow(state: BookState, id: &str) {
    if state.notes_selected.get().as_deref() == Some(id) {
        state.notes_selected.set(None);
    } else {
        state.notes_selected.set(Some(id.to_string()));
    }
}

/// Open a note from the drawing. An exit: see the module header.
fn open(state: BookState, store: AppStore, id: &str) {
    super::notes::open_note_row(state, store, id.to_string());
}

/// Date the dragged held note at the hovered point.
fn drop_on_line(state: BookState, store: AppStore) {
    let (Some(id), Some(frac)) = (state.timeline_dragging.get(), state.timeline_drop.get()) else {
        return;
    };
    let calendar = super::calendar::book_calendar(store);
    let point = current(state, store).point_at(&calendar, frac);
    let bid = state.bid_signal.get();
    // Only the span: a relative constraint the note already carries ("after the
    // siege") is the author's and stays — a note may hold both.
    super::note_editor::write_note_time(
        state,
        store,
        &bid,
        &id,
        Some(Some(TimeSpan::at(point))),
        None,
    );
    state.notes_selected.set(Some(id));
    // End the drag here rather than waiting for `ondragend`: the write above takes the
    // note out of the holding rail, which removes the drag *source* — and with it the
    // handler that would have cleared the drag and hidden the ghost.
    state.timeline_drop.set(None);
    state.timeline_dragging.set(None);
    state.ghost_visible.set(false);
}

/// Every note's stored `event_parent` — raw, loops and all: whether a drop is refused is
/// a question about what would be *stored*.
fn raw_parents(store: AppStore) -> HashMap<String, String> {
    store
        .notes
        .get()
        .into_iter()
        .filter_map(|n| n.event_parent.map(|p| (n.id, p)))
        .collect()
}

fn title_of(store: AppStore, id: &str) -> String {
    store
        .notes
        .get()
        .into_iter()
        .find(|n| n.id == id)
        .map(|n| n.title)
        .unwrap_or_default()
}

/// What a drop of the dragged bar onto `target` (a bar, or `None` for the empty ribbon)
/// would do — for the drop itself and for the hover feedback before it.
fn nest_for(state: BookState, store: AppStore, target: Option<&str>) -> Option<Nest> {
    let dragged = state.ribbon_dragging.get()?;
    Some(ribbon::nest_drop(&raw_parents(store), &dragged, target))
}

fn end_ribbon_drag(state: BookState) {
    state.ribbon_dragging.set(None);
    state.ribbon_over.set(None);
    state.ghost_visible.set(false);
}

/// Drop the dragged bar onto `target`: nest it, take it out, or refuse.
fn drop_nest(state: BookState, store: AppStore, target: Option<String>) {
    let Some(dragged) = state.ribbon_dragging.get() else { return };
    let decision = nest_for(state, store, target.as_deref());
    // Ended here, not in `ondragend`: the write below moves the bar, which rebuilds its
    // handle — the drag source — and with it the handler that would have ended the drag.
    end_ribbon_drag(state);
    let bid = state.bid_signal.get();
    match decision {
        Some(Nest::Under(parent)) => {
            super::note_editor::write_event_parent(state, store, &bid, &dragged, Some(parent));
            state.notes_selected.set(Some(dragged));
        }
        Some(Nest::Out) => {
            super::note_editor::write_event_parent(state, store, &bid, &dragged, None);
            state.notes_selected.set(Some(dragged));
        }
        Some(Nest::Refused) => {
            let into = target.map(|t| title_of(store, &t)).unwrap_or_default();
            state.ribbon_refusal.set(Some(format!(
                "\u{201c}{}\u{201d} is inside \u{201c}{}\u{201d} already \u{2014} an event cannot hold what holds it.",
                into,
                title_of(store, &dragged),
            )));
        }
        Some(Nest::Unchanged) | None => {}
    }
}

/// What the drop guide says: the date the note would get, in the book's calendar.
fn drop_label(state: BookState, store: AppStore) -> String {
    let Some(frac) = state.timeline_drop.get() else {
        return String::new();
    };
    let calendar = super::calendar::book_calendar(store);
    let layout = current(state, store);
    let point = layout.point_at(&calendar, frac);
    format!(
        "{} \u{b7} to the {}",
        calendar.format_point(&point),
        tl::drop_unit_name(&calendar, &layout)
    )
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{} {}", n, if n == 1 { one } else { many })
}

fn status(state: BookState, store: AppStore) -> String {
    let l = current(state, store);
    let mut parts: Vec<String> = Vec::new();
    if l.lanes_on {
        parts.push(plural(l.lanes.len(), "lane", "lanes"));
    }
    if l.ribbon_y.is_some() {
        parts.push(plural(l.bars.len(), "event", "events"));
        if l.dropped_labels > 0 {
            parts.push(format!(
                "{} with nowhere to go \u{2014} hover the bar",
                plural(l.dropped_labels, "label", "labels")
            ));
        }
    } else {
        parts.push(plural(l.marks.len(), "event", "events"));
        if l.no_one > 0 {
            parts.push(format!("{} with no one in them \u{2014} on the ribbon", l.no_one));
        }
    }
    if l.crowded > 0 {
        parts.push(format!("{} sharing a row", plural(l.crowded, "caption", "captions")));
    }
    parts.join(" \u{b7} ")
}

fn rule_label(store: AppStore) -> &'static str {
    match book_span_rule(store) {
        SpanRule::Fit => "nesting: auto-fit",
        SpanRule::Clamp => "nesting: clamp",
        SpanRule::Free => "nesting: free",
    }
}

// ── One element per layout item ──────────────────────────────────────────────
//
// Each item gets its own function, handed an owned copy, so every attribute closure
// can own the string it reads: `rsx!` turns a dynamic attribute into a `move`
// closure, and two closures cannot both move the same field out of a loop item.
//
// **Selection is never part of an item's key.** A keyed item is rebuilt whenever its
// key changes, and every click here selects something (opening a note selects it), so
// a key carrying the highlight would rebuild half the drawing per click. The keys below
// are the item *without* its `on` flags, and the highlight is a reactive class reading
// `notes_selected` directly: selecting repaints classes and rebuilds nothing.

/// Whether `id` — or any of `who`, the lanes it ties — is the selected note.
fn lit(state: BookState, id: &str, who: &[String]) -> bool {
    match state.notes_selected.get() {
        Some(sel) => sel == id || who.iter().any(|w| *w == sel),
        None => false,
    }
}

fn lane_key(lane: &tl::Lane) -> String {
    format!("{:?}", tl::Lane { on: false, ..lane.clone() })
}

fn mark_key(m: &tl::Mark) -> String {
    let dots = m.dots.iter().map(|d| tl::Dot { on: false, ..d.clone() }).collect();
    format!("{:?}", tl::Mark { on: false, dots, ..m.clone() })
}

fn caption_key(c: &tl::Caption) -> String {
    format!("{:?}", tl::Caption { on: false, ..c.clone() })
}

fn life_key(l: &tl::Life) -> String {
    format!("{:?}", tl::Life { on: false, ..l.clone() })
}

fn bar_key(b: &tl::Bar) -> String {
    format!("{:?}", tl::Bar { on: false, ..b.clone() })
}

fn follow_chip(__scope: &mut RenderScope, state: BookState, lane: tl::Lane) -> NodeHandle {
    let id = lane.id.clone();
    let open_id = lane.id.clone();
    let title = lane.title.clone();
    let lit_id = lane.id.clone();
    rsx! {
        div {
            class: {if lit(state, &lit_id, &[]) { "tl-chip is-on" } else { "tl-chip" }},
            data-id: {id.clone()},
            onclick: move || toggle_follow(state, &open_id),
            {title.clone()}
        }
    }
}

fn tick_node(__scope: &mut RenderScope, t: tl::Tick) -> NodeHandle {
    let label = t.label.clone();
    let (x, major) = (t.x, t.major);
    rsx! {
        g {
            class: {if major { "tl-tick is-major" } else { "tl-tick" }},
            line { x1: {n(x)}, y1: "24", x2: {n(x)}, y2: "10000" }
            text { x: {n(x + 3.0)}, y: "16", {label.clone()} }
        }
    }
}

/// The containment band behind a parent's sub-rows.
fn contain_node(__scope: &mut RenderScope, b: tl::Band) -> NodeHandle {
    let (x0, x1, y0, y1) = (b.x0, b.x1, b.y0, b.y1);
    let id = b.id.clone();
    rsx! {
        rect {
            class: "tl-contain",
            data-id: {id.clone()},
            x: {n(x0)}, y: {n(y0)},
            width: {n(x1 - x0)}, height: {n(y1 - y0)},
            rx: "3",
        }
    }
}

/// A zigzag at a bar's edge: it was cut off there (Clamp).
fn clip_path(x: f32, y: f32) -> String {
    let mut d = format!("M{:.1},{:.1}", x, y);
    for i in 0..5 {
        let dx = if i % 2 == 0 { 3.0 } else { -3.0 };
        d.push_str(&format!(" l{:.1},4", dx));
    }
    d
}

/// One event's bar on the ribbon, with its rule markers.
fn bar_node(__scope: &mut RenderScope, state: BookState, b: tl::Bar) -> NodeHandle {
    let class = format!(
        "tl-event depth-{}{}{}{}{}{}{}{}{}",
        b.depth.min(3),
        if b.muted { " is-muted" } else { "" },
        if b.fuzzy { " is-fuzzy" } else { "" },
        if b.open { " is-open" } else { "" },
        if b.fitted { " is-fitted" } else { "" },
        if b.derived { " is-derived" } else { "" },
        if b.pinned { " is-pinned" } else { "" },
        if b.clipped_start || b.clipped_end { " is-clipped" } else { "" },
        if b.escapes_start || b.escapes_end { " is-escaping" } else { "" },
    );
    let lit_id = b.id.clone();
    let who = b.who.clone();
    let id = b.id.clone();
    let title = b.title.clone();
    let (x0, x1, y) = (b.x0, b.x1, b.y);
    let (cs, ce, es, ee) = (b.clipped_start, b.clipped_end, b.escapes_start, b.escapes_end);
    rsx! {
        g {
            class: {format!("{}{}", class, if lit(state, &lit_id, &who) { " is-on" } else { "" })},
            data-id: {id.clone()},
            data-title: {title.clone()},
            rect { class: "tl-event-bar", x: {n(x0)}, y: {n(y)}, width: {n(x1 - x0)}, height: {n(tl::ROW_H)}, rx: "2" }
            if cs {
                path { class: "tl-clip tl-clip-start", d: {clip_path(x0 - 1.5, y)} }
            }
            if ce {
                path { class: "tl-clip tl-clip-end", d: {clip_path(x1 - 1.5, y)} }
            }
            if es {
                g { class: "tl-escape tl-escape-start",
                    circle { cx: {n(x0)}, cy: {n(y + tl::ROW_H / 2.0)}, r: "6" }
                    text { x: {n(x0)}, y: {n(y + tl::ROW_H / 2.0 + 3.5)}, text-anchor: "middle", "!" }
                }
            }
            if ee {
                g { class: "tl-escape tl-escape-end",
                    circle { cx: {n(x1)}, cy: {n(y + tl::ROW_H / 2.0)}, r: "6" }
                    text { x: {n(x1)}, y: {n(y + tl::ROW_H / 2.0 + 3.5)}, text-anchor: "middle", "!" }
                }
            }
        }
    }
}

fn label_node(__scope: &mut RenderScope, state: BookState, l: tl::BarLabel) -> NodeHandle {
    let class = format!(
        "tl-event-label{}{}",
        if l.inside { " is-inside" } else { " is-beside" },
        if l.muted { " is-muted" } else { "" },
    );
    let lit_id = l.id.clone();
    let who = l.who.clone();
    let id = l.id.clone();
    let text = l.text.clone();
    let (x, y) = (l.x, l.y);
    let anchor = if l.end { "end" } else { "start" };
    rsx! {
        text {
            class: {format!("{}{}", class, if lit(state, &lit_id, &who) { " is-on" } else { "" })},
            data-id: {id.clone()},
            x: {n(x)}, y: {n(y)},
            text-anchor: {anchor},
            {text.clone()}
        }
    }
}

fn band_node(__scope: &mut RenderScope, state: BookState, m: tl::Mark) -> NodeHandle {
    let class = format!(
        "tl-band{}{}",
        if m.fuzzy { " is-fuzzy" } else { "" },
        if m.open { " is-open" } else { "" },
    );
    let lit_id = m.id.clone();
    let who: Vec<String> = m.dots.iter().map(|d| d.lane.clone()).collect();
    let (x0, x1, y0, y1) = (m.x0, m.x1, m.lane_y0, m.y1);
    rsx! {
        rect {
            class: {format!("{}{}", class, if lit(state, &lit_id, &who) { " is-on" } else { "" })},
            x: {n(x0)}, y: {n(y0 - 10.0)},
            width: {n(x1 - x0)}, height: {n(y1 - y0 + 20.0)},
            rx: "2",
        }
    }
}

fn caption_node(
    __scope: &mut RenderScope,
    state: BookState,
    c: tl::Caption,
    who: Vec<String>,
) -> NodeHandle {
    let id = c.id.clone();
    let lit_id = c.id.clone();
    let text = c.text.clone();
    let (x, y) = (c.x, c.y);
    let (lx, ly0, ly1) = (c.leader_x, c.leader_y0, c.leader_y1);
    rsx! {
        g {
            class: {if lit(state, &lit_id, &who) { "tl-caption is-on" } else { "tl-caption" }},
            data-id: {id.clone()},
            line { class: "tl-leader", x1: {n(lx)}, y1: {n(ly0)}, x2: {n(lx)}, y2: {n(ly1)} }
            text { x: {n(x)}, y: {n(y)}, {text.clone()} }
        }
    }
}

fn lane_node(__scope: &mut RenderScope, state: BookState, lane: tl::Lane) -> NodeHandle {
    let class = format!("tl-lane{}", if lane.muted { " is-muted" } else { "" });
    let lit_id = lane.id.clone();
    let id = lane.id.clone();
    let title = lane.title.clone();
    let label = lane.label.clone();
    let y = lane.y;
    rsx! {
        g {
            class: {format!("{}{}", class, if lit(state, &lit_id, &[]) { " is-on" } else { "" })},
            data-id: {id.clone()},
            data-title: {title.clone()},
            line { x1: {n(tl::plot_x0())}, y1: {n(y)}, x2: {n(tl::plot_x1())}, y2: {n(y)} }
            g {
                class: "tl-lane-name",
                text {
                    x: {n(tl::plot_x0() - 10.0)}, y: {n(y + 4.0)},
                    text-anchor: "end",
                    {label.clone()}
                }
            }
        }
    }
}

fn life_node(__scope: &mut RenderScope, state: BookState, life: tl::Life) -> NodeHandle {
    let class = format!(
        "tl-life{}{}",
        if life.fuzzy { " is-fuzzy" } else { "" },
        if life.open { " is-open" } else { "" },
    );
    let lit_id = life.id.clone();
    let (x0, x1, y) = (life.x0, life.x1, life.y);
    rsx! {
        line { class: {format!("{}{}", class, if lit(state, &lit_id, &[]) { " is-on" } else { "" })}, x1: {n(x0)}, y1: {n(y)}, x2: {n(x1)}, y2: {n(y)} }
    }
}

fn mark_node(__scope: &mut RenderScope, state: BookState, m: tl::Mark) -> NodeHandle {
    let lit_id = m.id.clone();
    let who: Vec<String> = m.dots.iter().map(|d| d.lane.clone()).collect();
    let id = m.id.clone();
    let title = m.title.clone();
    let dots = m.dots.clone();
    let (cx, y0, y1) = (m.cx, m.y0, m.y1);
    rsx! {
        g {
            class: {if lit(state, &lit_id, &who) { "tl-mark is-on" } else { "tl-mark" }},
            data-id: {id.clone()},
            data-title: {title.clone()},
            line { class: "tl-tie", x1: {n(cx)}, y1: {n(y0)}, x2: {n(cx)}, y2: {n(y1)} }
            for d in dots.clone() {
                g { key: {format!("{:?}", tl::Dot { on: false, ..d.clone() })}, {dot_node(__scope, state, cx, d.clone())} }
            }
        }
    }
}

/// A click target over part of the drawing.
///
/// **Clicks land on HTML, never on the SVG.** rinch's click dispatch walks the clicked
/// element's ancestors reading `className`, which on an SVG element is an
/// `SVGAnimatedString`, not a string — a click handler on anything inside an `<svg>`
/// panics inside wasm-bindgen before it runs. So the drawing is inert
/// (`pointer-events: none`) and these transparent boxes, laid over it in the same
/// proportions, take the clicks.
#[derive(Debug, Clone, PartialEq)]
struct Hit {
    /// `bar`, `label`, `mark`, `caption` or `lane` — the CSS modifier and what e2e keys
    /// off.
    kind: &'static str,
    id: String,
    title: String,
    /// The tooltip: the full name, and how the span rule drew it.
    tip: String,
    /// Percentages of the canvas, so the box scales with the SVG.
    left: f32,
    top: f32,
    width: f32,
    height: f32,
}

fn pct(l: &Layout, x: f32, y: f32, w: f32, hh: f32) -> (f32, f32, f32, f32) {
    let h = l.height.max(1.0);
    (x / tl::W * 100.0, y / h * 100.0, w / tl::W * 100.0, hh / h * 100.0)
}

/// Everything clickable except the bars, which are also drop targets ([`bar_hits`]).
fn hits(l: &Layout) -> Vec<Hit> {
    let mut out = Vec::new();
    for c in &l.captions {
        let w = (c.text.chars().count() as f32 * 5.6 + 4.0).max(12.0);
        let (left, top, width, height) = pct(l, c.x - 2.0, c.y - 10.0, w, 13.0);
        out.push(Hit { kind: "caption", id: c.id.clone(), title: c.text.clone(), tip: c.text.clone(), left, top, width, height });
    }
    if l.lanes_on {
        for lane in &l.lanes {
            let (left, top, width, height) = pct(l, 0.0, lane.y - 12.0, tl::plot_x0() - 4.0, 24.0);
            out.push(Hit { kind: "lane", id: lane.id.clone(), title: lane.title.clone(), tip: lane.title.clone(), left, top, width, height });
        }
    }
    for m in &l.marks {
        // The whole band over the lanes, never narrower than a fingertip's worth of
        // drawing. With the ribbon on, the bar above is its own target.
        let x = m.x0.min(m.cx - 6.0);
        let w = (m.x1 - m.x0).max(12.0);
        let (left, top, width, height) = pct(l, x, m.lane_y0 - 10.0, w, m.y1 - m.lane_y0 + 20.0);
        out.push(Hit { kind: "mark", id: m.id.clone(), title: m.title.clone(), tip: m.title.clone(), left, top, width, height });
    }
    for lb in l.labels.iter().filter(|lb| !lb.inside) {
        let title = l.bars.iter().find(|b| b.id == lb.id).map(|b| b.title.clone()).unwrap_or_default();
        let w = lb.text.chars().count() as f32 * 5.6 + 4.0;
        let x = if lb.end { lb.x - w } else { lb.x - 2.0 };
        let (left, top, width, height) = pct(l, x, lb.y - 11.0, w, 14.0);
        out.push(Hit { kind: "label", id: lb.id.clone(), title: title.clone(), tip: title, left, top, width, height });
    }
    out
}

fn bar_hits(l: &Layout) -> Vec<Hit> {
    l.bars
        .iter()
        .map(|b| {
            let x = b.x0.min((b.x0 + b.x1) / 2.0 - 6.0);
            let w = (b.x1 - b.x0).max(12.0);
            let (left, top, width, height) = pct(l, x, b.y, w, tl::ROW_H);
            let note = b.note();
            let tip = if note.is_empty() { b.title.clone() } else { format!("{} \u{2014} {}", b.title, note) };
            Hit { kind: "bar", id: b.id.clone(), title: b.title.clone(), tip, left, top, width, height }
        })
        .collect()
}

fn hit_node(__scope: &mut RenderScope, state: BookState, store: AppStore, hit: Hit) -> NodeHandle {
    let class = format!("tl-hit tl-hit-{}", hit.kind);
    let id = hit.id.clone();
    let open_id = hit.id.clone();
    let title = hit.title.clone();
    let tip = hit.tip.clone();
    let style = format!(
        "left:{:.3}%; top:{:.3}%; width:{:.3}%; height:{:.3}%;",
        hit.left, hit.top, hit.width, hit.height
    );
    rsx! {
        div {
            class: {class.clone()},
            data-id: {id.clone()},
            data-title: {title.clone()},
            title: {tip.clone()},
            style: {style.clone()},
            onclick: move || open(state, store, &open_id),
        }
    }
}

/// A bar's click target, which is also where another bar is dropped to nest inside it.
fn bar_hit_node(__scope: &mut RenderScope, state: BookState, store: AppStore, hit: Hit) -> NodeHandle {
    let BookState { ribbon_over, .. } = state;
    let id = hit.id.clone();
    let open_id = hit.id.clone();
    let over_id = hit.id.clone();
    let class_id = hit.id.clone();
    let drop_id = hit.id.clone();
    let title = hit.title.clone();
    let tip = hit.tip.clone();
    let style = format!(
        "left:{:.3}%; top:{:.3}%; width:{:.3}%; height:{:.3}%;",
        hit.left, hit.top, hit.width, hit.height
    );
    rsx! {
        div {
            class: {move || {
                let mut c = "tl-hit tl-hit-bar".to_string();
                if ribbon_over.get() == Some(Some(class_id.clone())) {
                    c.push_str(match nest_for(state, store, Some(&class_id)) {
                        Some(Nest::Under(_)) => " is-drop-into",
                        Some(Nest::Refused) => " is-drop-refused",
                        _ => "",
                    });
                }
                c
            }},
            data-id: {id.clone()},
            data-title: {title.clone()},
            title: {tip.clone()},
            style: {style.clone()},
            onclick: move || open(state, store, &open_id),
            ondragover: move || {
                let want = Some(Some(over_id.clone()));
                if ribbon_over.get() != want {
                    ribbon_over.set(want);
                }
            },
            ondragleave: move || ribbon_over.set(None),
            ondrop: move || drop_nest(state, store, Some(drop_id.clone())),
        }
    }
}

/// The handle a bar is dragged by. A separate element beside the bar's click target,
/// not a child of it: a click on an interactive child of a `draggable` is dropped, and
/// rinch fires `onclick` on pointerdown, so a bar that was both would open its note at
/// the start of every drag.
fn handle_node(__scope: &mut RenderScope, state: BookState, b: tl::Bar, height: f32) -> NodeHandle {
    let BookState {
        ribbon_dragging,
        ribbon_over,
        ribbon_refusal,
        ghost_visible,
        ghost_pos,
        ghost_label,
        ghost_color,
        ..
    } = state;
    let id = b.id.clone();
    let drag_id = b.id.clone();
    let title = b.title.clone();
    let ghost_title = b.title.clone();
    let h = height.max(1.0);
    let style = format!(
        "left:{:.3}%; top:{:.3}%; height:{:.3}%;",
        (b.x0 - 11.0).max(0.0) / tl::W * 100.0,
        b.y / h * 100.0,
        tl::ROW_H / h * 100.0,
    );
    rsx! {
        div {
            class: {move || if ribbon_dragging.get().as_deref() == Some(drag_id.as_str()) { "tl-handle is-dragging" } else { "tl-handle" }},
            data-id: {id.clone()},
            data-title: {title.clone()},
            title: "Drag onto another event to nest it there, or onto empty ribbon to take it out",
            style: {style.clone()},
            draggable: "true",
            ondragstart: {
                let id = b.id.clone();
                move || {
                    ribbon_dragging.set(Some(id.clone()));
                    ribbon_over.set(None);
                    ribbon_refusal.set(None);
                    ghost_label.set(ghost_title.clone());
                    ghost_color.set("teal".to_string());
                    ghost_visible.set(true);
                }
            },
            ondragmove: move || {
                let ctx = rinch_core::events::get_click_context();
                ghost_pos.set((ctx.mouse_x, ctx.mouse_y));
            },
            ondragend: move || end_ribbon_drag(state),
            "\u{22ee}"
        }
    }
}

fn dot_node(__scope: &mut RenderScope, state: BookState, cx: f32, d: tl::Dot) -> NodeHandle {
    let lane_a = d.lane.clone();
    let lane_b = d.lane.clone();
    let y = d.y;
    rsx! {
        circle {
            class: {if lit(state, &lane_a, &[]) { "tl-dot is-on" } else { "tl-dot" }},
            cx: {n(cx)}, cy: {n(y)},
            r: {if lit(state, &lane_b, &[]) { "5" } else { "3.5" }},
        }
    }
}

/// One held note. The chip is the drag source *and* the open target, like a tree row,
/// and has no interactive children: a click on a control inside a `draggable` is
/// dispatched on the draggable instead.
fn held_node(__scope: &mut RenderScope, state: BookState, store: AppStore, h: tl::Held) -> NodeHandle {
    let BookState {
        timeline_dragging,
        timeline_drop,
        ghost_visible,
        ghost_pos,
        ghost_label,
        ghost_color,
        ..
    } = state;
    let id = h.id.clone();
    let drag_id = h.id.clone();
    let open_id = h.id.clone();
    let title = h.title.clone();
    let title_attr = h.title.clone();
    let ghost_title = h.title.clone();
    let why = h.why.clone();
    rsx! {
        div {
            class: "tl-held",
            data-id: {id.clone()},
            data-title: {title_attr.clone()},
            draggable: "true",
            ondragstart: move || {
                timeline_dragging.set(Some(drag_id.clone()));
                timeline_drop.set(None);
                ghost_label.set(ghost_title.clone());
                ghost_color.set("teal".to_string());
                ghost_visible.set(true);
            },
            ondragmove: move || {
                let ctx = rinch_core::events::get_click_context();
                ghost_pos.set((ctx.mouse_x, ctx.mouse_y));
            },
            ondragend: move || {
                timeline_dragging.set(None);
                timeline_drop.set(None);
                ghost_visible.set(false);
            },
            onclick: move || open(state, store, &open_id),
            span { class: "tl-held-glyph", "\u{25c7}" }
            span { class: "tl-held-title", {title.clone()} }
            span { class: "tl-held-why", {why.clone()} }
        }
    }
}

/// Flip one zone, keeping at least one on: turning off the last one turns the other on.
fn toggle_zone(this: Signal<bool>, other: Signal<bool>) {
    let on = !this.get();
    if !on && !other.get() {
        other.set(true);
    }
    this.set(on);
}

// ── The view ─────────────────────────────────────────────────────────────────

pub(in crate::pages::book) fn render(__scope: &mut RenderScope, state: BookState, store: AppStore) -> NodeHandle {
    let BookState {
        timeline_order,
        timeline_dragging,
        timeline_drop,
        timeline_ribbon,
        timeline_lanes,
        ribbon_dragging,
        ribbon_over,
        ribbon_refusal,
        ..
    } = state;

    // The drop overlay covers exactly the plot, in the drawing's own proportions.
    let overlay_style = format!(
        "left:{:.4}%; width:{:.4}%;",
        tl::plot_x0() / tl::W * 100.0,
        (tl::plot_x1() - tl::plot_x0()) / tl::W * 100.0,
    );

    rsx! {
        div { class: "tl", id: "notes-timeline",
            div { class: "tl-bar",
                span { class: "tl-bar-lbl", "follow:" }
                div { class: "tl-follow",
                    for lane in current(state, store).lanes {
                        div { key: {format!("{}|{}", lane.id, lane.title)}, style: "display: contents;",
                            {follow_chip(__scope, state, lane.clone())}
                        }
                    }
                }
                div { class: "tl-zones",
                    div {
                        class: {move || if timeline_ribbon.get() { "tl-zone is-on" } else { "tl-zone" }},
                        id: "tl-ribbon",
                        aria-pressed: {move || timeline_ribbon.get().to_string()},
                        onclick: move || toggle_zone(timeline_ribbon, timeline_lanes),
                        "ribbon"
                    }
                    div {
                        class: {move || if timeline_lanes.get() { "tl-zone is-on" } else { "tl-zone" }},
                        id: "tl-lanes",
                        aria-pressed: {move || timeline_lanes.get().to_string()},
                        onclick: move || toggle_zone(timeline_lanes, timeline_ribbon),
                        "lanes"
                    }
                }
                div {
                    class: "tl-order",
                    id: "tl-order",
                    onclick: move || timeline_order.set(timeline_order.get().toggled()),
                    {move || timeline_order.get().label()}
                }
                div {
                    class: "tl-order",
                    id: "tl-rule",
                    title: "How a nested event is drawn against the event it is in \u{2014} set in the book's Calendar",
                    // Leaves the timeline for the calendar pane: an exit, which
                    // `open_calendar` flushes.
                    onclick: move || super::calendar::open_calendar(state, store),
                    {move || rule_label(store)}
                }
            }
            div { class: "tl-status", {move || status(state, store)} }
            if ribbon_refusal.get().is_some() {
                div { class: "tl-refusal", id: "tl-refusal",
                    {move || ribbon_refusal.get().unwrap_or_default()}
                }
            }

            if current(state, store).undated {
                div { class: "tl-hint",
                    "Nothing on the line is dated yet. Give a note a date, or drag one up from the holding rail."
                }
            }

            div { class: "tl-scroll",
                div { class: "tl-canvas",
                    svg {
                        class: "tl-svg",
                        xmlns: "http://www.w3.org/2000/svg",
                        viewBox: {move || format!("0 0 {} {}", tl::W, n(current(state, store).height))},

                        g { class: "tl-ticks",
                            for t in current(state, store).ticks {
                                g { key: {format!("{:?}", t)}, {tick_node(__scope, t.clone())} }
                            }
                        }
                        line {
                            class: "tl-axis",
                            x1: {n(tl::plot_x0())}, y1: "24", x2: {n(tl::plot_x1())}, y2: "24",
                        }

                        // ── Zone 2: the lanes ──
                        if current(state, store).ribbon_y.is_some() && current(state, store).lanes_on {
                            g { class: "tl-zone-rule",
                                line {
                                    x1: {n(tl::plot_x0() - 6.0)},
                                    y1: {move || n(current(state, store).ribbon_y.map_or(0.0, |r| r.1) + 4.0)},
                                    x2: {n(tl::plot_x1())},
                                    y2: {move || n(current(state, store).ribbon_y.map_or(0.0, |r| r.1) + 4.0)},
                                }
                                text {
                                    class: "tl-zone-name",
                                    x: {n(tl::plot_x0() - 10.0)},
                                    y: {move || n(current(state, store).ribbon_y.map_or(0.0, |r| r.1) + 1.0)},
                                    text-anchor: "end",
                                    "who"
                                }
                            }
                        }
                        // Event bands over the lanes, behind everything that follows.
                        g { class: "tl-bands",
                            for m in current(state, store).marks {
                                g { key: {mark_key(&m)}, {band_node(__scope, state, m.clone())} }
                            }
                        }
                        // Captions — only without the ribbon — each with a leader to its band.
                        g { class: "tl-captions",
                            for c in current(state, store).captions {
                                g { key: {caption_key(&c)}, {caption_node(__scope, state, c.clone(), who_of(state, store, &c.id))} }
                            }
                        }
                        if current(state, store).lanes_on {
                            g { class: "tl-lanes",
                                for lane in current(state, store).lanes {
                                    g { key: {lane_key(&lane)}, {lane_node(__scope, state, lane.clone())} }
                                }
                            }
                        }
                        g { class: "tl-lives",
                            for life in current(state, store).lives {
                                g { key: {life_key(&life)}, {life_node(__scope, state, life.clone())} }
                            }
                        }
                        // Ties and dots on top of the lanes, rising into the ribbon.
                        g { class: "tl-marks",
                            for m in current(state, store).marks {
                                g { key: {mark_key(&m)}, {mark_node(__scope, state, m.clone())} }
                            }
                        }

                        // ── Zone 1: the ribbon ──
                        // Drawn after the ties, so a tie rising to its own bar passes
                        // behind any bar it crosses on the way rather than through its
                        // name.
                        if current(state, store).ribbon_y.is_some() {
                            text {
                                class: "tl-zone-name",
                                x: {n(tl::plot_x0() - 10.0)},
                                y: {move || n(current(state, store).ribbon_y.map_or(0.0, |r| r.0) + 20.0)},
                                text-anchor: "end",
                                "events"
                            }
                        }
                        g { class: "tl-contains",
                            for b in current(state, store).bands {
                                g { key: {format!("{:?}", b)}, {contain_node(__scope, b.clone())} }
                            }
                        }
                        g { class: "tl-events",
                            for b in current(state, store).bars {
                                g { key: {bar_key(&b)}, {bar_node(__scope, state, b.clone())} }
                            }
                        }
                        g { class: "tl-event-labels",
                            for lb in current(state, store).labels {
                                g { key: {format!("{:?}", lb)}, {label_node(__scope, state, lb.clone())} }
                            }
                        }

                    }

                    div { class: "tl-hits",
                        // The empty ribbon: dropping a bar here takes it out of whatever
                        // it is in. Beneath the bars' own targets, and armed only while a
                        // bar is being dragged.
                        div {
                            class: {move || {
                                let mut c = "tl-ribbon-drop".to_string();
                                if ribbon_dragging.get().is_some() {
                                    c.push_str(" is-armed");
                                    if ribbon_over.get() == Some(None) {
                                        c.push_str(match nest_for(state, store, None) {
                                            Some(Nest::Out) => " is-drop-out",
                                            _ => " is-drop-idle",
                                        });
                                    }
                                }
                                c
                            }},
                            id: "tl-ribbon-drop",
                            style: {move || {
                                let l = current(state, store);
                                let (y0, y1) = l.ribbon_y.unwrap_or((0.0, 0.0));
                                let h = l.height.max(1.0);
                                format!("top:{:.3}%; height:{:.3}%;", y0 / h * 100.0, (y1 - y0) / h * 100.0)
                            }},
                            ondragover: move || {
                                if ribbon_over.get() != Some(None) {
                                    ribbon_over.set(Some(None));
                                }
                            },
                            ondragleave: move || ribbon_over.set(None),
                            ondrop: move || drop_nest(state, store, None),
                        }
                        for hit in hits(&current(state, store)) {
                            div { key: {format!("{:?}", hit)}, style: "display: contents;",
                                {hit_node(__scope, state, store, hit.clone())}
                            }
                        }
                        for hit in bar_hits(&current(state, store)) {
                            div { key: {format!("{:?}", hit)}, style: "display: contents;",
                                {bar_hit_node(__scope, state, store, hit.clone())}
                            }
                        }
                        for b in current(state, store).bars {
                            div { key: {format!("{}|{:?}|{}", bar_key(&b), current(state, store).height, "h")}, style: "display: contents;",
                                {handle_node(__scope, state, b.clone(), current(state, store).height)}
                            }
                        }
                    }

                    // The drop target: the plot area, armed only while a held note is
                    // being dragged, so it never sits over the drawing's own clicks.
                    div {
                        class: {move || if timeline_dragging.get().is_some() { "tl-drop is-armed" } else { "tl-drop" }},
                        id: "tl-drop",
                        style: {overlay_style.clone()},
                        ondragover: move || {
                            let ctx = rinch_core::events::get_click_context();
                            if ctx.element_width > 0.0 {
                                let frac = (ctx.mouse_x - ctx.element_x) / ctx.element_width;
                                timeline_drop.set(Some(frac.clamp(0.0, 1.0)));
                            }
                        },
                        ondragleave: move || timeline_drop.set(None),
                        ondrop: move || drop_on_line(state, store),
                        if timeline_drop.get().is_some() && timeline_dragging.get().is_some() {
                            div {
                                class: "tl-drop-guide",
                                style: {move || format!("left:{:.3}%;", timeline_drop.get().unwrap_or(0.0) * 100.0)},
                                span { class: "tl-drop-label", {move || drop_label(state, store)} }
                            }
                        }
                    }
                }
            }

            div { class: "tl-rail", id: "tl-rail",
                div { class: "tl-rail-head",
                    span { class: "tl-rail-title", "Holding rail" }
                    span { class: "tl-rail-sub", "placed only against another note, or not yet dated \u{2014} drag one onto the line to date it" }
                }
                if current(state, store).held.is_empty() {
                    div { class: "tl-rail-empty", "Nothing waiting." }
                }
                div { class: "tl-rail-items",
                    for h in current(state, store).held {
                        div { key: {format!("{:?}", h)}, style: "display: contents;",
                            {held_node(__scope, state, store, h.clone())}
                        }
                    }
                }
            }
        }
    }
}
