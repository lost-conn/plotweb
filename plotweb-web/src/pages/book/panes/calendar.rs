//! The calendar screen (notes card 4): a book's base unit and its divisor stack.
//!
//! An eighth permanently-mounted pane, toggled by `display:none` like the other seven
//! (`panes/mod.rs`). It is reached from the notes surface's header and from a note's
//! time field — not from the tools strip, because a book that keeps the default,
//! Gregorian-shaped calendar never needs to see it.
//!
//! Opening it from a note is an **exit** from the note editor, so it goes through
//! [`open_calendar`], which flushes the pending edit before the pane moves.

use rinch::prelude::*;
use rinch_tabler_icons::{TablerIcon, TablerIconStyle, render_tabler_icon};
use plotweb_common::{Calendar, SpanRule, UpdateBookRequest};

use crate::api;
use crate::store::AppStore;

use super::super::calendar_form::{self, UnitDraft};
use super::super::state::BookState;
use super::super::BookPane;

/// The calendar this book's dates are read through: its own if it has a usable one,
/// otherwise the default.
pub(in crate::pages::book) fn book_calendar(store: AppStore) -> Calendar {
    Calendar::effective(store.current_book.get().and_then(|b| b.calendar).as_ref())
}

/// Open the calendar screen. **An exit**: the note editor's pending edit is written out
/// before the pane changes (`pages/book/flush.rs`), and the back arrow returns to
/// whichever pane this was opened from.
pub(in crate::pages::book) fn open_calendar(state: BookState, store: AppStore) {
    super::super::flush::flush_pending_edits(state, store);
    load_form(state, &book_calendar(store));
    state.calendar_status.set(None);
    let from = state.active_pane.get();
    state.calendar_return.set(match from {
        BookPane::NoteEditor(_) => from,
        _ => BookPane::Notes,
    });
    state.active_pane.set(BookPane::Calendar);
    store.sidebar_open.set(false);
}

fn load_form(state: BookState, calendar: &Calendar) {
    let rows = calendar_form::drafts_from(calendar);
    state.calendar_row_ids.set((0..rows.len() as u32).collect());
    state.calendar_rows.set(rows);
    state.calendar_name_draft.set(calendar.name.clone());
}

fn row_heading(index: usize) -> String {
    if index == 0 {
        "Base unit".to_string()
    } else {
        format!("Unit {}", index + 1)
    }
}

fn back(state: BookState, store: AppStore) {
    super::super::flush::flush_pending_edits(state, store);
    state.active_pane.set(state.calendar_return.get());
}

/// What the form currently says, read as a calendar.
fn form_calendar(state: BookState) -> Result<Calendar, String> {
    calendar_form::calendar_from(&state.calendar_name_draft.get(), &state.calendar_rows.get())
}

/// Write the calendar — or `None`, the default — everywhere a book's structure goes: the
/// local `book:` document, the page's copy of the book, and REST.
fn save(state: BookState, store: AppStore, calendar: Option<Calendar>) {
    let bid = state.bid_signal.get();
    crate::local_book::set_calendar(&bid, calendar.as_ref());
    store.current_book.update(|book| {
        if let Some(b) = book {
            b.calendar = calendar.clone();
        }
    });
    let req = UpdateBookRequest {
        calendar: Some(calendar),
        ..Default::default()
    };
    let status = state.calendar_status;
    status.set(Some("Saving…".to_string()));
    api::put::<_, serde_json::Value>(&format!("/api/books/{}", bid), &req, move |result| {
        status.set(Some(match result {
            Ok(_) => "Saved".to_string(),
            Err(e) => format!("Not saved: {}", e.message),
        }));
    });
}

/// Set the book's span rule (notes card 6) — how the timeline's ribbon draws a nested
/// event against the event it is in. Saved on the spot, like a toggle, and written the
/// way the calendar is: the local `book:` document, the page's copy of the book (so the
/// timeline redraws without a reload), then REST.
///
/// Auto-fit is the default, and choosing it stores **nothing** — the same absence a book
/// that never touched the setting has.
fn save_span_rule(state: BookState, store: AppStore, rule: SpanRule) {
    let stored = (rule != SpanRule::Fit).then_some(rule);
    let bid = state.bid_signal.get();
    crate::local_book::set_span_rule(&bid, stored);
    store.current_book.update(|book| {
        if let Some(b) = book {
            b.span_rule = stored;
        }
    });
    let req = UpdateBookRequest {
        span_rule: Some(stored),
        ..Default::default()
    };
    let status = state.span_rule_status;
    status.set(Some("Saving…".to_string()));
    api::put::<_, serde_json::Value>(&format!("/api/books/{}", bid), &req, move |result| {
        status.set(Some(match result {
            Ok(_) => "Saved".to_string(),
            Err(e) => format!("Not saved: {}", e.message),
        }));
    });
}

/// One choice of span rule.
fn rule_option(
    __scope: &mut RenderScope,
    state: BookState,
    store: AppStore,
    rule: SpanRule,
    name: &'static str,
    says: &'static str,
) -> NodeHandle {
    rsx! {
        div {
            class: {move || {
                let on = super::timeline::book_span_rule(store) == rule;
                if on { "span-rule-option is-on" } else { "span-rule-option" }
            }},
            id: {format!("span-rule-{}", rule.as_str())},
            aria-pressed: {move || (super::timeline::book_span_rule(store) == rule).to_string()},
            onclick: move || {
                if super::timeline::book_span_rule(store) != rule {
                    save_span_rule(state, store, rule);
                }
            },
            span { class: "span-rule-name", {name} }
            span { class: "span-rule-says", {says} }
        }
    }
}

/// One labelled text field of one unit row.
fn field(
    __scope: &mut RenderScope,
    state: BookState,
    index: usize,
    key: &'static str,
    label: &'static str,
    get: fn(&UnitDraft) -> &String,
    set: fn(&mut UnitDraft, String),
) -> NodeHandle {
    let rows = state.calendar_rows;
    rsx! {
        label { class: "cal-field",
            span { class: "cal-field-label", {label} }
            input {
                r#type: "text",
                autocomplete: "off",
                data-field: key,
                value: {move || rows.get().get(index).map(|r| get(r).clone()).unwrap_or_default()},
                oninput: move |v: String| {
                    rows.update(|rows| {
                        if let Some(r) = rows.get_mut(index) {
                            set(r, v);
                        }
                    });
                    state.calendar_status.set(None);
                },
            }
        }
    }
}

pub(in crate::pages::book) fn render(__scope: &mut RenderScope, state: BookState, store: AppStore) -> NodeHandle {
    let BookState {
        active_pane,
        calendar_rows,
        calendar_row_ids,
        calendar_name_draft,
        calendar_status,
        ..
    } = state;
    rsx! {
        div {
            class: "book-main-scroll",
            id: "calendar-pane",
            style: {move || if matches!(active_pane.get(), BookPane::Calendar) { "" } else { "display:none;" }},

            div { class: "chapters-pane calendar-pane",
                div { class: "calendar-head",
                    ActionIcon {
                        variant: "subtle",
                        onclick: move || back(state, store),
                        {render_tabler_icon(__scope, TablerIcon::ArrowLeft, TablerIconStyle::Outline)}
                    }
                    Title { order: 3, "Calendar" }
                }
                p { class: "calendar-lede",
                    "A base unit and the units it divides into. Every date in this book's notes is read through it; nothing needs setting up for an ordinary contemporary calendar."
                }

                label { class: "cal-field calendar-name",
                    span { class: "cal-field-label", "Name" }
                    input {
                        r#type: "text",
                        autocomplete: "off",
                        id: "calendar-name",
                        placeholder: "The Accord",
                        value: {move || calendar_name_draft.get()},
                        oninput: move |v: String| {
                            calendar_name_draft.set(v);
                            calendar_status.set(None);
                        },
                    }
                }

                div { class: "cal-rows",
                    for (index, id) in calendar_row_ids.get().into_iter().enumerate() {
                        div {
                            key: {id.to_string()},
                            class: "cal-row",
                            div { class: "cal-row-head",
                                {row_heading(index)}
                            }
                            div { class: "cal-row-fields",
                                {field(__scope, state, index, "name", "Unit", |r| &r.name, |r, v| r.name = v)}
                                {field(__scope, state, index, "defined_as", "Defined as", |r| &r.defined_as, |r, v| r.defined_as = v)}
                                {field(__scope, state, index, "written_as", "Written as", |r| &r.written_as, |r, v| r.written_as = v)}
                                {field(__scope, state, index, "names", "Value names", |r| &r.names, |r, v| r.names = v)}
                                {field(__scope, state, index, "counts_from", "Counts from", |r| &r.counts_from, |r, v| r.counts_from = v)}
                                {field(__scope, state, index, "shown_when", "Shown in timeline when", |r| &r.shown_when, |r, v| r.shown_when = v)}
                            }
                        }
                    }
                }

                div { class: "cal-row-actions",
                    div { id: "calendar-add-unit", style: "display: contents;",
                        Button {
                            variant: "subtle",
                            size: "xs",
                            onclick: move || {
                                let mut rows = calendar_rows.get();
                                let row = calendar_form::new_row(rows.last());
                                rows.push(row);
                                calendar_rows.set(rows);
                                calendar_row_ids.update(|ids| {
                                    let next = ids.iter().max().map_or(0, |m| m + 1);
                                    ids.push(next);
                                });
                            },
                            "Add a unit"
                        }
                    }
                    if calendar_row_ids.get().len() > 1 {
                        div { id: "calendar-remove-unit", style: "display: contents;",
                            Button {
                                variant: "subtle",
                                size: "xs",
                                color: "gray",
                                onclick: move || {
                                    calendar_rows.update(|rows| {
                                        rows.pop();
                                    });
                                    calendar_row_ids.update(|ids| {
                                        ids.pop();
                                    });
                                },
                                "Remove the last unit"
                            }
                        }
                    }
                }

                div {
                    class: {move || if form_calendar(state).is_ok() { "calendar-preview" } else { "calendar-preview is-error" }},
                    {move || match form_calendar(state) {
                        Ok(cal) => format!("Dates read like: {}", calendar_form::example(&cal)),
                        Err(e) => e,
                    }}
                }
                p { class: "calendar-note",
                    "Every unit is an exact fraction of the base, so there are no leap years and no special cases. A day of a 1/365 year is exactly that; a 1/12 month then comes out at 30 or 31 days by arithmetic rather than by rule."
                }

                div { class: "calendar-actions",
                    div { id: "calendar-save", style: "display: contents;",
                        Button {
                            size: "sm",
                            onclick: move || match form_calendar(state) {
                                Ok(cal) => save(state, store, Some(cal)),
                                Err(e) => calendar_status.set(Some(e)),
                            },
                            "Save calendar"
                        }
                    }
                    div { id: "calendar-reset", style: "display: contents;",
                        Button {
                            size: "sm",
                            variant: "subtle",
                            color: "gray",
                            onclick: move || {
                                load_form(state, &Calendar::default());
                                save(state, store, None);
                            },
                            "Reset to default"
                        }
                    }
                    span { class: "calendar-status", {move || calendar_status.get().unwrap_or_default()} }
                }

                div { class: "span-rule", id: "span-rule",
                    Title { order: 4, "Nested events on the timeline" }
                    p { class: "calendar-note",
                        "When an event sits inside another on the timeline's ribbon and their dates disagree. Only the drawing changes: nobody's dates are ever rewritten."
                    }
                    div { class: "span-rule-options",
                        {rule_option(__scope, state, store, SpanRule::Fit, "Auto-fit", "The outer event stretches to hold everything inside it, unless its dates are pinned. Give it no dates at all and it is drawn from what it holds.")}
                        {rule_option(__scope, state, store, SpanRule::Clamp, "Clamp", "The outer event's dates win. Anything running past them is cut off at its edge, and marked.")}
                        {rule_option(__scope, state, store, SpanRule::Free, "Free", "Everything is drawn as typed. An inner event that runs outside its outer one pokes out, flagged.")}
                    }
                    span { class: "span-rule-status", id: "span-rule-status", {move || state.span_rule_status.get().unwrap_or_default()} }
                }
            }
        }
    }
}
