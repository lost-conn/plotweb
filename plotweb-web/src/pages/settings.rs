//! Account settings (`/settings`). For now one section: **Agent access** —
//! personal access tokens an AI assistant uses to read the author's books,
//! organise notes and leave review comments.
//!
//! The raw token is shown exactly once, straight out of the create response, and
//! is held only in a component-local signal until the author presses Done. It is
//! never written to local storage or the store.

use std::collections::HashSet;

use plotweb_common::{ApiTokenInfo, Book, CreateApiTokenRequest, CreateApiTokenResponse};
use rinch::prelude::*;
use rinch_core::use_store;
use rinch_tabler_icons::{TablerIcon, TablerIconStyle, render_tabler_icon};

use crate::api;
use crate::components::dialog::{DIALOG_CSS, Dialog};
use crate::router;
use crate::store::{AppStore, Route};

const SETTINGS_CSS: &str = r#"
.settings-page {
    height: 100dvh;
    display: flex;
    flex-direction: column;
    overflow: hidden;
}

.settings-topbar {
    display: flex;
    align-items: center;
    gap: var(--pw-space-xs);
    padding: var(--pw-space-sm) var(--pw-space-lg);
    border-bottom: 1px solid var(--rinch-color-border);
    background: var(--pw-color-deep);
    flex-shrink: 0;
}

.settings-body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: var(--pw-space-xl) var(--pw-space-2xl);
}

.settings-col {
    max-width: var(--pw-pane-max);
    margin: 0 auto;
    font-family: var(--pw-font-ui);
}

.settings-h {
    font-family: var(--pw-font-display);
    font-size: var(--pw-text-xl);
    line-height: var(--pw-lh-tight);
    color: var(--rinch-color-text);
    font-weight: 400;
}

.settings-lede {
    margin-top: var(--pw-space-xs);
    font-size: var(--pw-text-md);
    line-height: var(--pw-lh-ui);
    color: var(--rinch-color-dimmed);
}

.settings-rule {
    display: flex;
    align-items: center;
    gap: var(--pw-space-sm);
    margin: var(--pw-space-xl) 0 var(--pw-space-sm);
}

.settings-rule-label {
    font-size: var(--pw-text-2xs);
    letter-spacing: .08em;
    text-transform: uppercase;
    color: var(--rinch-color-dimmed);
    white-space: nowrap;
}

.settings-rule-line {
    flex: 1;
    height: 1px;
    background: var(--pw-hairline);
}

.token-list {
    display: flex;
    flex-direction: column;
    border: 1px solid var(--rinch-color-border);
    border-radius: var(--pw-radius-md);
    background: var(--rinch-color-surface);
}

.token-row {
    display: flex;
    align-items: center;
    gap: var(--pw-space-md);
    padding: var(--pw-space-sm) var(--pw-space-md);
    border-top: 1px solid var(--pw-hairline);
}

.token-row:first-child {
    border-top: none;
}

.token-main {
    flex: 1;
    min-width: 0;
}

.token-label {
    font-size: var(--pw-text-md);
    font-weight: 600;
    color: var(--rinch-color-text);
    word-break: break-word;
}

.token-prefix {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: var(--pw-text-xs);
    color: var(--rinch-color-dimmed);
    margin-left: var(--pw-space-xs);
    font-weight: 400;
}

.token-meta {
    margin-top: var(--pw-space-3xs);
    font-size: var(--pw-text-xs);
    color: var(--rinch-color-dimmed);
    line-height: var(--pw-lh-ui);
}

.token-empty {
    font-size: var(--pw-text-sm);
    color: var(--rinch-color-dimmed);
}

.token-reveal {
    margin-top: var(--pw-space-lg);
    padding: var(--pw-space-md);
    border: 1px solid var(--rinch-color-teal-6);
    border-radius: var(--pw-radius-md);
    background: var(--rinch-color-surface);
    box-shadow: var(--pw-shadow-1);
}

.token-reveal-title {
    font-size: var(--pw-text-md);
    font-weight: 600;
    color: var(--rinch-color-text);
}

.token-reveal-warning {
    margin-top: var(--pw-space-2xs);
    font-size: var(--pw-text-sm);
    color: var(--rinch-color-dimmed);
}

.token-reveal-value {
    margin-top: var(--pw-space-sm);
    padding: var(--pw-space-xs) var(--pw-space-sm);
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: var(--pw-text-sm);
    color: var(--rinch-color-text);
    background: var(--pw-color-deepest);
    border-radius: var(--pw-radius-sm);
    word-break: break-all;
    user-select: all;
}

.token-reveal-actions {
    margin-top: var(--pw-space-sm);
    display: flex;
    align-items: center;
    gap: var(--pw-space-xs);
}

.token-reveal-hint {
    font-size: var(--pw-text-xs);
    color: var(--rinch-color-dimmed);
}

.token-form {
    display: flex;
    flex-direction: column;
    gap: var(--pw-space-sm);
}

.token-books {
    display: flex;
    flex-direction: column;
    gap: var(--pw-space-2xs);
    max-height: 260px;
    overflow-y: auto;
    padding: var(--pw-space-xs) var(--pw-space-sm);
    border: 1px solid var(--rinch-color-border);
    border-radius: var(--pw-radius-sm);
}

.token-form-error {
    font-size: var(--pw-text-sm);
    color: var(--rinch-color-red-6);
}

@media (max-width: 640px) {
    .settings-body {
        padding: var(--pw-space-lg) var(--pw-space-md);
    }
    .token-row {
        flex-wrap: wrap;
    }
}
"#;

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// `"2026-09-23 14:05:00"` → `"23 Sep 2026"`; anything unparseable is shown as-is.
fn format_date(ts: &str) -> String {
    let date = ts.split(' ').next().unwrap_or(ts);
    let mut parts = date.split('-');
    let (Some(y), Some(m), Some(d)) = (parts.next(), parts.next(), parts.next()) else {
        return ts.to_string();
    };
    let (Ok(m), Ok(d)) = (m.parse::<usize>(), d.parse::<u32>()) else {
        return ts.to_string();
    };
    match MONTHS.get(m.wrapping_sub(1)) {
        Some(month) => format!("{d} {month} {y}"),
        None => ts.to_string(),
    }
}

/// "All books", or the scoped books' titles. A book deleted since the token was
/// made is no longer reachable by it, so it is simply left out of the list.
fn scope_label(book_ids: &Option<Vec<String>>, books: &[Book]) -> String {
    let Some(ids) = book_ids else {
        return "All books".to_string();
    };
    let titles: Vec<String> = ids
        .iter()
        .filter_map(|id| books.iter().find(|b| &b.id == id).map(|b| b.title.clone()))
        .collect();
    if titles.is_empty() {
        "No books (they have been deleted)".to_string()
    } else {
        titles.join(", ")
    }
}

/// One rendered row: the token plus its display strings, computed up front so the
/// row itself reads no signals.
#[derive(Clone, PartialEq)]
struct TokenRow {
    id: String,
    label: String,
    prefix: String,
    meta: String,
}

fn token_rows(tokens: &[ApiTokenInfo], books: &[Book]) -> Vec<TokenRow> {
    let now = crate::platform::now_epoch_secs();
    tokens
        .iter()
        .map(|t| {
            let used = match &t.last_used_at {
                Some(ts) => format!(
                    "last used {}",
                    crate::pages::dashboard::relative_time(ts, now)
                ),
                None => "never used".to_string(),
            };
            TokenRow {
                id: t.id.clone(),
                label: t.label.clone(),
                prefix: format!("pw_{}\u{2026}", t.prefix),
                meta: format!(
                    "{} \u{00B7} created {} \u{00B7} {}",
                    scope_label(&t.book_ids, books),
                    format_date(&t.created_at),
                    used
                ),
            }
        })
        .collect()
}

#[component]
pub fn settings_page() -> NodeHandle {
    let store = use_store::<AppStore>();

    let books: Signal<Vec<Book>> = Signal::new(store.books.get());
    let tokens: Signal<Vec<ApiTokenInfo>> = Signal::new(Vec::new());
    let loaded = Signal::new(false);
    let load_error: Signal<Option<String>> = Signal::new(None);

    let label = Signal::new(String::new());
    let all_books = Signal::new(true);
    let selected: Signal<HashSet<String>> = Signal::new(HashSet::new());
    let form_error: Signal<Option<String>> = Signal::new(None);
    let creating = Signal::new(false);

    // The one-time reveal of a freshly minted token.
    let revealed: Signal<Option<String>> = Signal::new(None);
    let copy_state: Signal<Option<bool>> = Signal::new(None);

    let revoke_target: Signal<Option<(String, String)>> = Signal::new(None);

    // Books first, so a scoped token's titles are known when its row renders.
    api::get::<Vec<Book>>("/api/books", move |books_result| {
        match books_result {
            Ok(list) => books.set(list),
            Err(e) => load_error.set(Some(e.message)),
        }
        api::get::<Vec<ApiTokenInfo>>("/api/tokens", move |result| {
            match result {
                Ok(list) => tokens.set(list),
                Err(e) => load_error.set(Some(e.message)),
            }
            loaded.set(true);
        });
    });

    let back = move || router::navigate(Route::Dashboard);

    let create = move || {
        if creating.get() {
            return;
        }
        let name = label.get().trim().to_string();
        if name.is_empty() {
            form_error.set(Some("Give the token a name, so you can tell it apart later.".into()));
            return;
        }
        let book_ids = if all_books.get() {
            None
        } else {
            let chosen: Vec<String> = books
                .get()
                .iter()
                .filter(|b| selected.get().contains(&b.id))
                .map(|b| b.id.clone())
                .collect();
            if chosen.is_empty() {
                form_error.set(Some("Tick at least one book, or allow all books.".into()));
                return;
            }
            Some(chosen)
        };
        form_error.set(None);
        creating.set(true);
        let req = CreateApiTokenRequest { label: name, book_ids };
        api::post::<_, CreateApiTokenResponse>("/api/tokens", &req, move |result| {
            creating.set(false);
            match result {
                Ok(resp) => {
                    tokens.update(|list| list.insert(0, resp.info));
                    copy_state.set(None);
                    revealed.set(Some(resp.token));
                    label.set(String::new());
                    all_books.set(true);
                    selected.set(HashSet::new());
                }
                Err(e) => form_error.set(Some(e.message)),
            }
        });
    };

    let copy = move || {
        if let Some(raw) = revealed.get() {
            copy_state.set(Some(crate::platform::copy_text(&raw)));
        }
    };

    let done = move || {
        revealed.set(None);
        copy_state.set(None);
    };

    let confirm_revoke = move || {
        let Some((id, _)) = revoke_target.get() else {
            return;
        };
        revoke_target.set(None);
        api::delete_req::<serde_json::Value>(&format!("/api/tokens/{id}"), move |result| {
            match result {
                Ok(_) => tokens.update(|list| list.retain(|t| t.id != id)),
                Err(e) => load_error.set(Some(e.message)),
            }
        });
    };

    rsx! {
        div { class: "settings-page",
            style { {SETTINGS_CSS} }
            style { {DIALOG_CSS} }

            div { class: "settings-topbar",
                ActionIcon {
                    variant: "subtle",
                    size: "sm",
                    onclick: back,
                    {render_tabler_icon(__scope, TablerIcon::ArrowLeft, TablerIconStyle::Outline)}
                }
                Button {
                    variant: "subtle",
                    size: "xs",
                    onclick: back,
                    "Library"
                }
                span { style: "flex: 1;" }
                Title { order: 3, "Settings" }
            }

            div { class: "settings-body",
                div { class: "settings-col",
                    h2 { class: "settings-h", "Agent access" }
                    p { class: "settings-lede",
                        "Access tokens let an AI assistant read your books, organise your notes and leave review comments. They can never edit your manuscript. Give each assistant its own token, and revoke it when you stop using it."
                    }

                    if let Some(raw) = revealed.get() {
                        div { class: "token-reveal",
                            div { class: "token-reveal-title", "Your new token" }
                            div { class: "token-reveal-warning",
                                "Copy it now and paste it into your assistant's settings. You won't see it again."
                            }
                            div { class: "token-reveal-value", {raw} }
                            div { class: "token-reveal-actions",
                                Button {
                                    size: "xs",
                                    variant: "light",
                                    onclick: copy,
                                    {|| match copy_state.get() {
                                        Some(true) => "Copied",
                                        _ => "Copy",
                                    }}
                                }
                                Button {
                                    size: "xs",
                                    onclick: done,
                                    "Done"
                                }
                                span { class: "token-reveal-hint",
                                    {|| match copy_state.get() {
                                        Some(false) => "Couldn't reach the clipboard here: select the token and copy it by hand.",
                                        _ => "",
                                    }}
                                }
                            }
                        }
                    }

                    div { class: "settings-rule",
                        span { class: "settings-rule-label", "Your tokens" }
                        span { class: "settings-rule-line" }
                    }

                    if let Some(err) = load_error.get() {
                        Text { size: "sm", color: "red", {err} }
                        Space { h: "xs" }
                    }

                    if loaded.get() && tokens.get().is_empty() {
                        div { class: "token-empty", "No tokens yet." }
                    }

                    if !tokens.get().is_empty() {
                        div { class: "token-list",
                            for row in token_rows(&tokens.get(), &books.get()) {
                                div {
                                    key: row.id.clone(),
                                    class: "token-row",
                                    div { class: "token-main",
                                        div { class: "token-label",
                                            {row.label.clone()}
                                            span { class: "token-prefix", {row.prefix.clone()} }
                                        }
                                        div { class: "token-meta", {row.meta.clone()} }
                                    }
                                    Button {
                                        size: "xs",
                                        variant: "subtle",
                                        color: "red",
                                        onclick: {
                                            let id = row.id.clone();
                                            let label = row.label.clone();
                                            move || revoke_target.set(Some((id.clone(), label.clone())))
                                        },
                                        "Revoke"
                                    }
                                }
                            }
                        }
                    }

                    div { class: "settings-rule",
                        span { class: "settings-rule-label", "New token" }
                        span { class: "settings-rule-line" }
                    }

                    div { class: "token-form",
                        TextInput {
                            label: "Name",
                            placeholder: "e.g. Claude on my laptop",
                            value_fn: move || label.get(),
                            oninput: move |v: String| label.set(v),
                            onsubmit: create,
                        }
                        Switch {
                            label: "All books",
                            description: "Include every book you have now and any you create later.",
                            checked_fn: move || all_books.get(),
                            onchange: move || all_books.update(|v| *v = !*v),
                        }
                        if !all_books.get() {
                            if books.get().is_empty() {
                                div { class: "token-empty", "You don't have any books yet." }
                            }
                            if !books.get().is_empty() {
                                div { class: "token-books",
                                    for book in books.get() {
                                        Checkbox {
                                            key: book.id.clone(),
                                            label: book.title.clone(),
                                            checked_fn: {
                                                let id = book.id.clone();
                                                move || selected.get().contains(&id)
                                            },
                                            onchange: {
                                                let id = book.id.clone();
                                                move || {
                                                    let id = id.clone();
                                                    selected.update(|set| {
                                                        if !set.remove(&id) {
                                                            set.insert(id);
                                                        }
                                                    });
                                                }
                                            },
                                        }
                                    }
                                }
                            }
                        }
                        if let Some(err) = form_error.get() {
                            div { class: "token-form-error", {err} }
                        }
                        div {
                            Button {
                                onclick: create,
                                "Create token"
                            }
                        }
                    }
                }
            }

            Dialog {
                opened_fn: move || revoke_target.get().is_some(),
                onclose: move || revoke_target.set(None),
                title: {move || {
                    revoke_target.get()
                        .map(|(_, label)| format!("Revoke \"{label}\"?"))
                        .unwrap_or_default()
                }},
                danger: true,
                confirm_label: "Revoke",
                onconfirm: confirm_revoke,
                "Any assistant using this token loses access straight away."
                span { class: "pw-dialog-warning", "This cannot be undone." }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book(id: &str, title: &str) -> Book {
        serde_json::from_value(serde_json::json!({
            "id": id, "title": title, "description": "", "created_at": "", "updated_at": "",
            "chapter_count": null,
        }))
        .unwrap()
    }

    #[test]
    fn dates_read_as_day_month_year() {
        assert_eq!(format_date("2026-09-23 14:05:00"), "23 Sep 2026");
        assert_eq!(format_date("2026-01-01 00:00:00"), "1 Jan 2026");
        assert_eq!(format_date("nonsense"), "nonsense");
    }

    #[test]
    fn scope_names_books_or_says_all() {
        let books = vec![book("a", "Alpha"), book("b", "Beta")];
        assert_eq!(scope_label(&None, &books), "All books");
        assert_eq!(scope_label(&Some(vec!["b".into()]), &books), "Beta");
        assert_eq!(
            scope_label(&Some(vec!["a".into(), "gone".into(), "b".into()]), &books),
            "Alpha, Beta"
        );
        assert_eq!(
            scope_label(&Some(vec!["gone".into()]), &books),
            "No books (they have been deleted)"
        );
    }
}
