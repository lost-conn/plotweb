//! The phone timeline — a vertical spine (notes card 7).
//!
//! `design/04-notes-wireframes.html`, "Timeline, take B — vertical spine" and
//! "Placement and the phone": at phone width the Timeline tab swaps the ribbon-over-
//! lanes SVG for a plain scrolling list of cards, time running top to bottom instead
//! of left to right.
//!
//! **A flag, not a second renderer.** This pane and [`super::timeline`] are rendered
//! side by side, always both in the DOM; which one is visible is decided purely by CSS
//! (`.tl` / `.tl-spine` in the existing `@media (max-width: 768px)` rule in
//! `pages/book/css.rs`), never by a Rust-side width check. That is why the native
//! desktop build — which has no media queries at all — always shows [`super::timeline`]
//! regardless of window size: `crate::platform`, not `web_sys::window()`, is the only
//! way this crate may ask a browser anything, and this pane asks it nothing.
//!
//! Every rule of *what* is drawn — nesting, the span rule, the filter, selection — is
//! [`super::super::spine_layout`], host-tested. This module only turns a
//! [`spine_layout::Spine`] into plain HTML (no SVG, so none of `panes::timeline`'s
//! `pointer-events: none` + hit-box workaround is needed — a click handler on an
//! ordinary `div` never touches the `SVGAnimatedString` `className` trap that panics
//! wasm-bindgen inside an `<svg>`).
//!
//! Tapping a card opens its note through [`super::notes::open_note_row`] — the same
//! exit the tree row and the desktop timeline use, which flushes whatever was being
//! edited first (`pages/book/flush.rs`). There is no drag here: no holding rail (card 7
//! places relative/undated notes inline instead) and no nest-by-drag (the spine only
//! *reads* `event_parent`, through the ribbon's own rules; it is not a second place to
//! write it).

use rinch::prelude::*;

use crate::store::AppStore;

use super::super::spine_layout::{self as sp, Card, Loose, Row};
use super::super::state::BookState;

fn current(state: BookState, store: AppStore) -> sp::Spine {
    let notes = store.notes.get();
    let filter = state.notes_filter.get();
    let calendar = super::calendar::book_calendar(store);
    let selected = state.notes_selected.get();
    sp::layout(&sp::SpineInput {
        notes: &notes,
        filter: &filter,
        calendar: &calendar,
        selected: selected.as_deref(),
        rule: super::timeline::book_span_rule(store),
    })
}

/// Open a note from the spine. An exit: see the module header.
fn open(state: BookState, store: AppStore, id: &str) {
    super::notes::open_note_row(state, store, id.to_string());
}

fn refs_line(titles: &[String]) -> String {
    titles.iter().map(|t| format!("${t}")).collect::<Vec<_>>().join(" \u{b7} ")
}

fn card_key(c: &Card) -> String {
    format!("{:?}", Card { on: false, ..c.clone() })
}

fn loose_key(l: &Loose) -> String {
    format!("{:?}", Loose { on: false, ..l.clone() })
}

/// One event's card, or one relative note placed inline beside another card — the two
/// are the same shape ([`spine_layout::Card`]) and drawn the same way.
///
/// Every attribute below gets its own freshly-cloned string rather than one shared
/// `let` reused at several sites: `rsx!` turns a dynamic attribute into its own `move`
/// closure, and a `move` closure takes full ownership of whatever it names, so two
/// closures cannot both move the same field out of `c` (the same reasoning
/// `panes::timeline`'s `bar_node`/`label_node` document at their own repeated clones).
fn card_node(__scope: &mut RenderScope, state: BookState, store: AppStore, c: Card, indent: bool) -> NodeHandle {
    let open_id = c.id.clone();
    let data_id = c.id.clone();
    let data_title = c.title.clone();
    let head_title = c.title.clone();
    let when = c.when.clone();
    let refs = refs_line(&c.who_titles);
    let has_refs = !refs.is_empty();
    let (on, muted, fuzzy, depth, badge) = (c.on, c.muted, c.fuzzy, c.depth, c.badge);
    rsx! {
        div {
            class: {format!(
                "spine-card{}{}{}",
                if on { " is-on" } else { "" },
                if muted { " is-muted" } else { "" },
                if fuzzy { " is-fuzzy" } else { "" },
            )},
            style: {if indent { format!("margin-left: {}px;", depth as f32 * 18.0) } else { String::new() }},
            data-id: {data_id.clone()},
            data-title: {data_title.clone()},
            onclick: move || open(state, store, &open_id),
            div { class: "spine-card-head",
                h4 { class: "spine-card-title", {head_title.clone()} }
                span { class: "spine-card-when", {when.clone()} }
            }
            if has_refs {
                div { class: "spine-card-refs", {refs.clone()} }
            }
            if let Some(b) = badge {
                span { class: "spine-card-badge", {b.label()} }
            }
        }
    }
}

fn row_node(__scope: &mut RenderScope, state: BookState, store: AppStore, row: Row) -> NodeHandle {
    match row {
        Row::Card(c) => card_node(__scope, state, store, c, true),
        Row::Simultaneous { depth, items } => {
            rsx! {
                div { class: "spine-simul", style: {format!("margin-left: {}px;", depth as f32 * 18.0)},
                    div { class: "spine-simul-lbl", "at the same time" }
                    for c in items.clone() {
                        div { key: {card_key(&c)}, style: "display: contents;",
                            {card_node(__scope, state, store, c.clone(), false)}
                        }
                    }
                }
            }
        }
    }
}

fn row_key(row: &Row) -> String {
    match row {
        Row::Card(c) => format!("card:{}", card_key(c)),
        Row::Simultaneous { depth, items } => {
            let items: Vec<String> = items.iter().map(card_key).collect();
            format!("simul:{depth}:{}", items.join(","))
        }
    }
}

fn loose_node(__scope: &mut RenderScope, state: BookState, store: AppStore, l: Loose) -> NodeHandle {
    let open_id = l.id.clone();
    let data_id = l.id.clone();
    let data_title = l.title.clone();
    let chip_title = l.title.clone();
    let refs = refs_line(&l.who_titles);
    let has_refs = !refs.is_empty();
    let on = l.on;
    rsx! {
        div {
            class: {if on { "spine-loose-chip is-on" } else { "spine-loose-chip" }},
            data-id: {data_id.clone()},
            data-title: {data_title.clone()},
            onclick: move || open(state, store, &open_id),
            span { class: "spine-loose-title", {chip_title.clone()} }
            if has_refs {
                span { class: "spine-loose-refs", {refs.clone()} }
            }
        }
    }
}

pub(in crate::pages::book) fn render(__scope: &mut RenderScope, state: BookState, store: AppStore) -> NodeHandle {
    rsx! {
        div { class: "tl-spine", id: "notes-timeline-spine",
            if current(state, store).undated && current(state, store).not_yet_dated.is_empty() {
                div { class: "tl-hint",
                    "Nothing on the line is dated yet."
                }
            }
            if !current(state, store).rows.is_empty() {
                div { class: "spine",
                    for row in current(state, store).rows {
                        div { key: {row_key(&row)}, style: "display: contents;",
                            {row_node(__scope, state, store, row.clone())}
                        }
                    }
                }
            }
            if !current(state, store).not_yet_dated.is_empty() {
                div { class: "spine-loose", id: "spine-loose",
                    div { class: "spine-loose-heading", "not yet dated" }
                    for l in current(state, store).not_yet_dated {
                        div { key: {loose_key(&l)}, style: "display: contents;",
                            {loose_node(__scope, state, store, l.clone())}
                        }
                    }
                }
            }
        }
    }
}
