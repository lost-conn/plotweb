use rinch::prelude::*;
use rinch_core::use_store;
use rinch_tabler_icons::{TablerIcon, TablerIconStyle, render_tabler_icon};
use plotweb_common::{Book, CreateBookRequest, SharedBook};

use crate::api;
use crate::components::book_jacket::{BOOK_JACKET_CSS, BookJacket};
use crate::components::card::{Card, card_styles};
use crate::router;
use crate::store::{AppStore, Route};

const DASHBOARD_CSS: &str = r#"
.dash-topbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--pw-space-sm) var(--pw-space-lg);
    border-bottom: 1px solid var(--rinch-color-border);
    background: var(--pw-color-deep);
    flex-shrink: 0;
}

.dash-topbar-right {
    display: flex;
    align-items: center;
    gap: var(--pw-space-xs);
}

/* The page owns its full-height root, like `.book-workspace` does. The route
   wrapper in app_shell.rs is a 100dvh flex column, but the component mounts
   inside an unstyled block container between that wrapper and this markup, so
   nothing below it inherited a constrained height: `.dash-body` was
   content-tall, its `overflow-y: auto` never had anything to scroll, and the
   wrapper's `overflow: hidden` clipped the shelf instead. */
.dash-page {
    height: 100dvh;
    display: flex;
    flex-direction: column;
    overflow: hidden;
}

.dash-body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: var(--pw-space-xl) var(--pw-space-2xl);
}

.shelf {
    display: flex;
    flex-direction: row;
    align-items: flex-start;
    gap: var(--pw-space-lg);
    flex-wrap: wrap;
}

.bk {
    width: 180px;
    position: relative;
}

.bk-meta {
    padding-top: var(--pw-space-sm);
}

.bk-meta-title {
    font-size: var(--pw-text-sm);
    font-weight: 600;
    line-height: 1.3;
    color: var(--rinch-color-text);
    word-break: break-word;
}

.bk-meta-stats {
    font-size: var(--pw-text-xs);
    color: var(--rinch-color-dimmed);
    margin-top: 2px;
}

.bk-meta-edited {
    font-size: var(--pw-text-2xs);
    color: var(--rinch-color-placeholder);
    margin-top: 3px;
}

.bk-new {
    width: 180px;
    height: 252px;
}

.shelf-rule {
    display: flex;
    align-items: center;
    gap: var(--pw-space-sm);
    margin: var(--pw-space-2xl) 0 var(--pw-space-md);
}

.shelf-rule-label {
    font-size: var(--pw-text-2xs);
    letter-spacing: .08em;
    text-transform: uppercase;
    color: var(--rinch-color-dimmed);
    white-space: nowrap;
}

.shelf-rule-line {
    flex: 1;
    height: 1px;
    background: var(--pw-hairline);
}

.dash-empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 80px 0;
    text-align: center;
}

.bk-delete-confirm-body {
    font-size: var(--pw-text-sm);
    color: var(--rinch-color-text);
}

.bk-delete-confirm-warning {
    font-size: var(--pw-text-xs);
    color: var(--rinch-color-dimmed);
    margin-top: var(--pw-space-xs);
}

@media (max-width: 640px) {
    .dash-body {
        padding: var(--pw-space-lg) var(--pw-space-md);
    }
    .shelf {
        justify-content: center;
    }
    .bk, .bk-new {
        width: 140px;
    }
    .bk-jacket {
        width: 140px;
        height: 196px;
    }
    .bk-meta-title {
        font-size: var(--pw-text-xs);
    }
}
"#;

fn format_word_count(count: u64) -> String {
    if count >= 1_000_000 {
        format!("{:.1}M", count as f64 / 1_000_000.0)
    } else if count >= 1_000 {
        format!("{:.1}k", count as f64 / 1_000.0)
    } else {
        count.to_string()
    }
}

/// "51,800 words · 12 chapters" — the jacket's second meta line.
fn format_book_stats(word_count: Option<u64>, chapter_count: Option<i64>) -> String {
    let words = word_count.map(format_word_count).unwrap_or_else(|| "0".to_string());
    let chapters = chapter_count.unwrap_or(0);
    format!("{words} words · {chapters} chapters")
}

/// "3 chapters unread" — omitted entirely (see the `if` around the call site)
/// when there is nothing unread, so a fully-read shared book stays silent
/// rather than announcing "0 chapters unread".
fn format_unread(n: i64) -> String {
    format!("{n} chapter{} unread", if n == 1 { "" } else { "s" })
}

/// Parse `"YYYY-MM-DD HH:MM:SS"` (the format every `updated_at` on the wire
/// uses — always `chrono::Utc::now()`-formatted server-side) into UTC seconds
/// since the epoch, using Howard Hinnant's civil-from-days algorithm so this
/// file doesn't need a date library just to diff two timestamps.
fn parse_timestamp_secs(s: &str) -> Option<i64> {
    let (date, time) = s.split_once(' ')?;
    let mut date_parts = date.split('-');
    let year: i64 = date_parts.next()?.parse().ok()?;
    let month: i64 = date_parts.next()?.parse().ok()?;
    let day: i64 = date_parts.next()?.parse().ok()?;

    let mut time_parts = time.split(':');
    let hour: i64 = time_parts.next()?.parse().ok()?;
    let minute: i64 = time_parts.next()?.parse().ok()?;
    let second: i64 = time_parts.next()?.parse().ok()?;

    // Days since epoch (1970-01-01), civil calendar -> days.
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as i64; // [0, 399]
    let mp = (month + 9) % 12; // [0, 11]
    let doy = (153 * mp + 2) / 5 + day - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    let days = era * 146097 + doe - 719468;

    Some(days * 86400 + hour * 3600 + minute * 60 + second)
}

/// Relative-time label for a book's `updated_at`, e.g. "edited 20 minutes ago".
/// `now_secs` is injected (rather than read internally) so this stays a pure,
/// easily unit-tested function — see the `relative_time_tests` module below.
///
/// Falls back to `"a while ago"` for an `updated_at` that fails to parse
/// (should not happen — the server always writes this format — but a
/// malformed timestamp should degrade quietly rather than panic or show
/// nonsense like "-4000000000 minutes ago").
fn relative_time(updated_at: &str, now_secs: i64) -> String {
    let Some(then) = parse_timestamp_secs(updated_at) else {
        return "a while ago".to_string();
    };
    let delta = (now_secs - then).max(0);

    const MINUTE: i64 = 60;
    const HOUR: i64 = 3600;
    const DAY: i64 = 86400;
    const MONTH: i64 = 30 * DAY;

    if delta < MINUTE {
        "just now".to_string()
    } else if delta < HOUR {
        let n = delta / MINUTE;
        format!("{n} minute{} ago", if n == 1 { "" } else { "s" })
    } else if delta < DAY {
        let n = delta / HOUR;
        format!("{n} hour{} ago", if n == 1 { "" } else { "s" })
    } else if delta < 2 * DAY {
        "yesterday".to_string()
    } else if delta < MONTH {
        let n = delta / DAY;
        format!("{n} days ago")
    } else if delta < 2 * MONTH {
        "last month".to_string()
    } else {
        let n = delta / MONTH;
        format!("{n} months ago")
    }
}

#[component]
pub fn dashboard_page() -> NodeHandle {
    let store = use_store::<AppStore>();
    let show_modal = Signal::new(false);
    let new_title = Signal::new(String::new());
    let new_desc = Signal::new(String::new());

    // Confirmation modal for delete — (book id, title, word count), or None
    // when closed. A `⋯` menu replaces the old hover-only trash icon, which
    // was one misclick from destroying a manuscript with no confirmation.
    let delete_target: Signal<Option<(String, String, Option<u64>)>> = Signal::new(None);

    // The generated plate's author line for the owner's own shelf. Read once
    // rather than as a `{|| ..}` closure per jacket — the logged-in user does
    // not change while the dashboard is mounted, and a reactive closure prop
    // here forces the whole `BookJacket` call onto rinch's reactive-component
    // path, which does not play well with a `for`-loop body that also moves
    // `book` fields around (each field needs its own `.clone()`, and the
    // combination panics the borrow checker rather than the app).
    let current_username = store
        .current_user
        .get()
        .map(|u| u.username.clone())
        .unwrap_or_default();

    // Fetch books on mount, then back the list with the local-first `user:` doc:
    // seed it from this REST list (first open) or load the local doc (which then
    // wins the projection into `store.books`). See crate::local_user.
    api::get::<Vec<Book>>("/api/books", move |books_result| {
        if let Ok(books) = books_result {
            store.books.set(books.clone());
            if let Some(user) = store.current_user.get() {
                crate::local_user::enter_user(user.id, books, store);
            }
        }
        api::get::<Vec<SharedBook>>("/api/shared-books", move |shared_result| {
            if let Ok(shared) = shared_result {
                store.shared_books.set(shared);
            }
        });
    });

    let logout = move || {
        api::post::<_, serde_json::Value>("/api/auth/logout", &serde_json::json!({}), move |_result| {
            store.current_user.set(None);
            router::navigate(Route::Login);
        });
    };

    let toggle_dark = move || {
        store.dark_mode.update(|d| *d = !*d);
    };

    let open_modal = move || {
        new_title.set(String::new());
        new_desc.set(String::new());
        show_modal.set(true);
    };

    let create_book = move || {
        let title = new_title.get();
        if title.trim().is_empty() {
            return;
        }
        let desc = new_desc.get();
        show_modal.set(false);
        let req = CreateBookRequest {
            title,
            description: desc,
        };
        api::post::<_, Book>("/api/books", &req, move |result| {
            if let Ok(book) = result {
                // Dual-write: cache the new book in the local `user:` doc beside REST.
                if let Some(user) = store.current_user.get() {
                    crate::local_user::add_book(&user.id, &book);
                }
                store.books.update(|books| books.insert(0, book));
            }
        });
    };

    let open_book = move |id: String| {
        move || {
            router::navigate(Route::Book(id.clone()));
        }
    };

    let open_shared_book = move |token: String| {
        move || {
            router::navigate(Route::Reader(token.clone()));
        }
    };

    let request_delete = move |id: String, title: String, word_count: Option<u64>| {
        move || {
            delete_target.set(Some((id.clone(), title.clone(), word_count)));
        }
    };

    let confirm_delete = move || {
        let Some((id, _title, _wc)) = delete_target.get() else {
            return;
        };
        delete_target.set(None);
        api::delete_req::<serde_json::Value>(&format!("/api/books/{}", id), move |result| {
            if result.is_ok() {
                // Dual-write: drop the cached entry from the local `user:` doc.
                if let Some(user) = store.current_user.get() {
                    crate::local_user::remove_book(&user.id, &id);
                }
                store.books.update(|books| books.retain(|b| b.id != id));
            }
        });
    };

    rsx! {
        div { class: "dash-page",
            style { {DASHBOARD_CSS} }
            style { {BOOK_JACKET_CSS} }
            style { {card_styles()} }

            // Top bar
            div { class: "dash-topbar",
                div {
                    style: "display: flex; align-items: center; gap: 10px;",
                    img {
                        src: crate::platform::asset_src("/assets/logo.png"),
                        alt: "PlotWeb",
                        style: "width: 28px; height: 28px;",
                    }
                    Title { order: 3, "PlotWeb" }
                }
                div { class: "dash-topbar-right",
                    ActionIcon {
                        variant: "subtle",
                        size: "sm",
                        onclick: toggle_dark,
                        // Reactive icon: an rsx `if` block re-renders the child node
                        // when `dark_mode` toggles. A bare `{expr}` block is captured
                        // once at render and would never update; a `{|| ...}` child
                        // closure is treated as reactive *text* (ToString) by rinch, not
                        // a node, so it can't return a NodeHandle here.
                        if store.dark_mode.get() {
                            {render_tabler_icon(__scope, TablerIcon::Sun, TablerIconStyle::Outline)}
                        } else {
                            {render_tabler_icon(__scope, TablerIcon::Moon, TablerIconStyle::Outline)}
                        }
                    }
                    Text {
                        size: "sm",
                        color: "dimmed",
                        {|| {
                            let store = use_store::<AppStore>();
                            store.current_user.get()
                                .map(|u| u.username.clone())
                                .unwrap_or_default()
                        }}
                    }
                    ActionIcon {
                        variant: "subtle",
                        size: "sm",
                        onclick: logout,
                        {render_tabler_icon(__scope, TablerIcon::Logout, TablerIconStyle::Outline)}
                    }
                }
            }

            // Body
            div { class: "dash-body",
                if store.books.get().is_empty() {
                    div { class: "dash-empty",
                        Text { size: "lg", color: "dimmed", "No books yet" }
                        Space { h: "sm" }
                        Text { size: "sm", color: "dimmed", "Create your first book to get started" }
                        Space { h: "lg" }
                        Button {
                            onclick: open_modal,
                            "New Book"
                        }
                    }
                }

                if !store.books.get().is_empty() {
                    div { class: "shelf",
                        for book in store.books.get() {
                            div {
                                key: book.id.clone(),
                                class: "bk",

                                BookJacket {
                                    title: book.title.clone(),
                                    author: current_username.clone(),
                                    cover_image: book.cover_image.clone(),
                                    shared: false,
                                    onclick: open_book(book.id.clone()),

                                    ActionIcon {
                                        variant: "subtle",
                                        size: "xs",
                                        style: "background: var(--rinch-color-surface);",
                                        onclick: request_delete(book.id.clone(), book.title.clone(), book.word_count),
                                        "\u{22EF}"
                                    }
                                }

                                div { class: "bk-meta",
                                    div { class: "bk-meta-title", {book.title.clone()} }
                                    div { class: "bk-meta-stats",
                                        {format_book_stats(book.word_count, book.chapter_count)}
                                    }
                                    div { class: "bk-meta-edited",
                                        {format!("edited {}", relative_time(&book.updated_at, crate::platform::now_epoch_secs()))}
                                    }
                                }
                            }
                        }

                        Card {
                            class: "bk-new",
                            interactive: true,
                            dashed: true,
                            onclick: open_modal,
                            {render_tabler_icon(__scope, TablerIcon::Plus, TablerIconStyle::Outline)}
                            Text { size: "sm", "New book" }
                        }
                    }
                }

                if !store.shared_books.get().is_empty() {
                    div { class: "shelf-rule",
                        span { class: "shelf-rule-label", "Shared with me" }
                        span { class: "shelf-rule-line" }
                    }
                    div { class: "shelf",
                        for shared in store.shared_books.get() {
                            div {
                                key: shared.token.clone(),
                                class: "bk",

                                BookJacket {
                                    title: shared.book_title.clone(),
                                    author: shared.author_username.clone(),
                                    cover_image: shared.cover_image.clone(),
                                    shared: true,
                                    onclick: open_shared_book(shared.token.clone()),
                                }

                                div { class: "bk-meta",
                                    div { class: "bk-meta-title", {shared.book_title.clone()} }
                                    div { class: "bk-meta-stats", {format!("by {}", shared.author_username)} }
                                    if shared.unread_count.unwrap_or(0) > 0 {
                                        div { class: "bk-meta-edited",
                                            {format_unread(shared.unread_count.unwrap_or(0))}
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // New Book Modal
            Modal {
                opened_fn: move || show_modal.get(),
                onclose: move || show_modal.set(false),
                title: "Create New Book",

                TextInput {
                    label: "Title",
                    placeholder: "Book title",
                    value_fn: move || new_title.get(),
                    oninput: move |v: String| new_title.set(v),
                    onsubmit: create_book,
                }
                Space { h: "md" }
                Textarea {
                    label: "Description",
                    placeholder: "What's this book about?",
                    value_fn: move || new_desc.get(),
                    oninput: move |v: String| new_desc.set(v),
                }
                Space { h: "lg" }
                Group {
                    justify: "flex-end",
                    Button {
                        variant: "subtle",
                        onclick: move || show_modal.set(false),
                        "Cancel"
                    }
                    Button {
                        onclick: create_book,
                        "Create"
                    }
                }
            }

            // Delete confirmation modal — replaces the old hover-only trash icon.
            Modal {
                opened_fn: move || delete_target.get().is_some(),
                onclose: move || delete_target.set(None),
                title: {|| {
                    delete_target.get()
                        .map(|(_, title, _)| format!("Delete \"{title}\"?"))
                        .unwrap_or_default()
                }},

                div { class: "bk-delete-confirm-body",
                    {|| {
                        delete_target.get()
                            .and_then(|(_, _, wc)| wc)
                            .map(|w| format!("{} words.", format_word_count(w)))
                            .unwrap_or_default()
                    }}
                }
                div { class: "bk-delete-confirm-warning", "This cannot be undone." }
                Space { h: "lg" }
                Group {
                    justify: "flex-end",
                    Button {
                        variant: "subtle",
                        onclick: move || delete_target.set(None),
                        "Cancel"
                    }
                    Button {
                        color: "red",
                        onclick: confirm_delete,
                        "Delete"
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod relative_time_tests {
    use super::*;

    /// A fixed "now" so every case below is a pure function of the delta,
    /// independent of when the test suite actually runs.
    const NOW: &str = "2026-09-17 18:12:23";

    fn now_secs() -> i64 {
        parse_timestamp_secs(NOW).unwrap()
    }

    #[test]
    fn just_now_covers_the_first_minute() {
        assert_eq!(relative_time("2026-09-17 18:12:23", now_secs()), "just now");
        assert_eq!(relative_time("2026-09-17 18:11:59", now_secs()), "just now");
    }

    #[test]
    fn minutes_boundary() {
        assert_eq!(relative_time("2026-09-17 18:11:23", now_secs()), "1 minute ago");
        assert_eq!(relative_time("2026-09-17 17:52:23", now_secs()), "20 minutes ago");
        // 59:59 ago is still minutes, not hours.
        assert_eq!(relative_time("2026-09-17 17:12:24", now_secs()), "59 minutes ago");
    }

    #[test]
    fn hours_boundary() {
        assert_eq!(relative_time("2026-09-17 17:12:23", now_secs()), "1 hour ago");
        assert_eq!(relative_time("2026-09-17 12:12:23", now_secs()), "6 hours ago");
        // 23:59:59 ago is still hours.
        assert_eq!(relative_time("2026-09-16 18:12:24", now_secs()), "23 hours ago");
    }

    #[test]
    fn yesterday_covers_one_to_two_days() {
        assert_eq!(relative_time("2026-09-16 18:12:23", now_secs()), "yesterday");
        // Just under 2 full days is still "yesterday".
        assert_eq!(relative_time("2026-09-15 18:12:24", now_secs()), "yesterday");
    }

    #[test]
    fn days_boundary() {
        assert_eq!(relative_time("2026-09-15 18:12:23", now_secs()), "2 days ago");
        assert_eq!(relative_time("2026-09-04 18:12:23", now_secs()), "13 days ago");
        assert_eq!(relative_time("2026-08-19 18:12:23", now_secs()), "29 days ago");
        // One second short of 30 full days still floors to 29, not "last month".
        assert_eq!(relative_time("2026-08-18 18:12:24", now_secs()), "29 days ago");
    }

    #[test]
    fn last_month_covers_one_to_two_months() {
        assert_eq!(relative_time("2026-08-18 18:12:23", now_secs()), "last month");
        assert_eq!(relative_time("2026-07-20 18:12:24", now_secs()), "last month");
    }

    #[test]
    fn months_boundary() {
        assert_eq!(relative_time("2026-07-19 18:12:23", now_secs()), "2 months ago");
        assert_eq!(relative_time("2026-01-17 18:12:23", now_secs()), "8 months ago");
    }

    #[test]
    fn malformed_timestamp_degrades_quietly() {
        assert_eq!(relative_time("", now_secs()), "a while ago");
        assert_eq!(relative_time("not a date", now_secs()), "a while ago");
    }

    #[test]
    fn parse_timestamp_round_trips_known_epoch_values() {
        // 1970-01-01 00:00:00 UTC is epoch zero.
        assert_eq!(parse_timestamp_secs("1970-01-01 00:00:00"), Some(0));
        // 2000-03-01 00:00:00 UTC — a well-known reference point in the
        // civil-from-days algorithm (951868800), useful to catch a sign/leap
        // error that the round numbers above wouldn't.
        assert_eq!(parse_timestamp_secs("2000-03-01 00:00:00"), Some(951_868_800));
    }
}
