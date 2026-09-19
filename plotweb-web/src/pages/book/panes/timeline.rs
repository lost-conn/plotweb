//! The notes timeline — entity lanes (notes revamp, card 5).
//!
//! Draws a [`timeline_layout::Layout`] as SVG and wires its three gestures:
//!
//! * **follow** an entity (the chip row): selects it, which lights its lane and every
//!   event it takes part in. Selection is `BookState::notes_selected`, the same one the
//!   tree highlights, so it survives switching view.
//! * **open** a note (an event's band or caption, a lane's name, a held note): an
//!   **exit** from whatever was being edited, through [`super::notes::open_note_row`],
//!   which calls `flush_pending_edits` before `active_pane` moves.
//! * **date** a held note by dragging it from the holding rail onto the line, which
//!   writes a span through [`super::note_editor::write_note_time`] — the time field's own
//!   write path, tombstone-aware, not a second one.
//!
//! Every rule of the drawing lives in `timeline_layout`, host-tested; nothing here
//! decides anything, it only places what the layout says. The layout reads the
//! projected note list — structure-document data — and never a note body.
//!
//! **Wide drawing, narrow screen.** The SVG has a minimum width and sits in its own
//! `overflow-x: auto` box, so at 390px the drawing scrolls sideways inside itself and
//! the page body never does. Card 7 replaces this with the phone spine.

use rinch::prelude::*;
use plotweb_common::TimeSpan;

use crate::store::AppStore;

use super::super::state::BookState;
use super::super::timeline_layout::{self as tl, Layout};

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

fn status(state: BookState, store: AppStore) -> String {
    let l = current(state, store);
    let mut parts: Vec<String> = Vec::new();
    let lanes = l.lanes.len();
    parts.push(format!("{} lane{}", lanes, if lanes == 1 { "" } else { "s" }));
    let events = l.marks.len();
    parts.push(format!("{} event{}", events, if events == 1 { "" } else { "s" }));
    let unassigned = l.marks.iter().filter(|m| m.unassigned).count();
    if unassigned > 0 {
        parts.push(format!("{} with no one in them", unassigned));
    }
    if l.crowded > 0 {
        parts.push(format!(
            "{} caption{} sharing a row",
            l.crowded,
            if l.crowded == 1 { "" } else { "s" }
        ));
    }
    parts.join(" \u{b7} ")
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

fn band_node(__scope: &mut RenderScope, state: BookState, m: tl::Mark) -> NodeHandle {
    let class = format!(
        "tl-band{}{}{}",
        if m.fuzzy { " is-fuzzy" } else { "" },
        if m.open { " is-open" } else { "" },
        if m.unassigned { " is-unassigned" } else { "" },
    );
    let lit_id = m.id.clone();
    let who: Vec<String> = m.dots.iter().map(|d| d.lane.clone()).collect();
    let (x0, x1, y0, y1) = (m.x0, m.x1, m.y0, m.y1);
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
    let class = format!("tl-mark{}", if m.unassigned { " is-unassigned" } else { "" });
    let lit_id = m.id.clone();
    let who: Vec<String> = m.dots.iter().map(|d| d.lane.clone()).collect();
    let id = m.id.clone();
    let title = m.title.clone();
    let dots = m.dots.clone();
    let (cx, y0, y1) = (m.cx, m.y0, m.y1);
    rsx! {
        g {
            class: {format!("{}{}", class, if lit(state, &lit_id, &who) { " is-on" } else { "" })},
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
    /// `mark`, `caption` or `lane` — the CSS modifier and what e2e keys off.
    kind: &'static str,
    id: String,
    title: String,
    /// Percentages of the canvas, so the box scales with the SVG.
    left: f32,
    top: f32,
    width: f32,
    height: f32,
}

fn hits(l: &Layout) -> Vec<Hit> {
    let h = l.height.max(1.0);
    let pct = |x: f32, y: f32, w: f32, hh: f32| {
        (x / tl::W * 100.0, y / h * 100.0, w / tl::W * 100.0, hh / h * 100.0)
    };
    let mut out = Vec::new();
    for c in &l.captions {
        let w = (c.text.chars().count() as f32 * 5.6 + 4.0).max(12.0);
        let (left, top, width, height) = pct(c.x - 2.0, c.y - 10.0, w, 13.0);
        out.push(Hit { kind: "caption", id: c.id.clone(), title: c.text.clone(), left, top, width, height });
    }
    for lane in &l.lanes {
        let (left, top, width, height) = pct(0.0, lane.y - 12.0, tl::plot_x0() - 4.0, 24.0);
        out.push(Hit { kind: "lane", id: lane.id.clone(), title: lane.title.clone(), left, top, width, height });
    }
    for m in &l.marks {
        // The whole band, and never narrower than a fingertip's worth of drawing.
        let x = m.x0.min(m.cx - 6.0);
        let w = (m.x1 - m.x0).max(12.0);
        let (left, top, width, height) = pct(x, m.y0 - 10.0, w, m.y1 - m.y0 + 20.0);
        out.push(Hit { kind: "mark", id: m.id.clone(), title: m.title.clone(), left, top, width, height });
    }
    out
}

fn hit_node(__scope: &mut RenderScope, state: BookState, store: AppStore, hit: Hit) -> NodeHandle {
    let class = format!("tl-hit tl-hit-{}", hit.kind);
    let id = hit.id.clone();
    let open_id = hit.id.clone();
    let title = hit.title.clone();
    let tip = hit.title.clone();
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

// ── The view ─────────────────────────────────────────────────────────────────

pub(in crate::pages::book) fn render(__scope: &mut RenderScope, state: BookState, store: AppStore) -> NodeHandle {
    let BookState {
        timeline_order,
        timeline_dragging,
        timeline_drop,
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
                div {
                    class: "tl-order",
                    id: "tl-order",
                    onclick: move || timeline_order.set(timeline_order.get().toggled()),
                    {move || timeline_order.get().label()}
                }
            }
            div { class: "tl-status", {move || status(state, store)} }

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

                        // The unassigned row: events that name no entity, kept on screen
                        // until card 6's ribbon gives them a home. Where the ribbon
                        // will go — above the lanes.
                        if current(state, store).unassigned_y.is_some() {
                            g { class: "tl-unassigned",
                                line {
                                    x1: {n(tl::plot_x0())},
                                    y1: {move || n(current(state, store).unassigned_y.unwrap_or(0.0))},
                                    x2: {n(tl::plot_x1())},
                                    y2: {move || n(current(state, store).unassigned_y.unwrap_or(0.0))},
                                }
                                text {
                                    class: "tl-unassigned-name",
                                    x: {n(tl::plot_x0() - 10.0)},
                                    y: {move || n(current(state, store).unassigned_y.unwrap_or(0.0) + 4.0)},
                                    text-anchor: "end",
                                    "no one"
                                }
                            }
                        }

                        // Event bands, behind everything that follows.
                        g { class: "tl-bands",
                            for m in current(state, store).marks {
                                g { key: {mark_key(&m)}, {band_node(__scope, state, m.clone())} }
                            }
                        }
                        // Captions, staggered in tiers, each with a leader to its band.
                        g { class: "tl-captions",
                            for c in current(state, store).captions {
                                g { key: {caption_key(&c)}, {caption_node(__scope, state, c.clone(), who_of(state, store, &c.id))} }
                            }
                        }
                        g { class: "tl-lanes",
                            for lane in current(state, store).lanes {
                                g { key: {lane_key(&lane)}, {lane_node(__scope, state, lane.clone())} }
                            }
                        }
                        g { class: "tl-lives",
                            for life in current(state, store).lives {
                                g { key: {life_key(&life)}, {life_node(__scope, state, life.clone())} }
                            }
                        }
                        // Ties and dots on top of the lanes.
                        g { class: "tl-marks",
                            for m in current(state, store).marks {
                                g { key: {mark_key(&m)}, {mark_node(__scope, state, m.clone())} }
                            }
                        }
                    }

                    div { class: "tl-hits",
                        for hit in hits(&current(state, store)) {
                            div { key: {format!("{:?}", hit)}, style: "display: contents;",
                                {hit_node(__scope, state, store, hit.clone())}
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
