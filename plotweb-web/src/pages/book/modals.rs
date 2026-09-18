//! The book page's overlays, tiered by weight (see `design/01-language.html#overlays`).
//!
//! Nine `rinch::Modal`s used to live here, each re-declaring its own
//! `Space + Group + Cancel/Confirm` footer at one uniform volume — a rename
//! and an import wizard got the same centred dialog treatment. They're now
//! three tiers:
//!
//! - **Tier 1 — in place.** Rename and Add-chapter no longer open anything;
//!   they edit the chapters-pane row directly (`panes/chapters.rs`).
//! - **Tier 2 — [`Dialog`](crate::components::dialog::Dialog).** Small,
//!   centred, reserved for destructive/irreversible confirms: restore a
//!   version, delete a chapter.
//! - **Tier 3 — [`Sheet`](crate::components::sheet::Sheet).** A right-hand
//!   slide-over (bottom sheet under 768px) for long or multi-step work:
//!   Import, Export, Book settings, Create/Edit beta link.
//!
//! Add Note keeps Tier 2's chrome but not its weight-class reasoning — the
//! task was chrome-only, since Notes as a surface is being rebuilt later.
//!
//! Create/Edit beta link were ~95% identical, including duplicated one-off
//! inline styles; `beta_link_sheet_fields` is now the one body both render,
//! parametrized on which signal set backs it rather than duplicated per mode.

use rinch::prelude::*;
use wasm_bindgen::JsCast;
use rinch_tabler_icons::{TablerIcon, TablerIconStyle, render_tabler_icon};
use plotweb_common::{Book, Chapter, CommitInfo, ImportPreviewResponse};

use crate::api;
use crate::components::dialog::Dialog;
use crate::components::sheet::Sheet;
use crate::store::AppStore;

use super::state::BookState;
use super::BookPane;

/// The max-chapters + pin-version + username fields shared by Create and Edit
/// beta link — previously duplicated verbatim (including the same hand-rolled
/// checkbox markup) between the two modals. Takes the signals directly rather
/// than a value+setter pair per field: every field here is read live inside
/// reactive closures anyway (`value_fn`/`checked_fn`), so a `Signal` is what
/// both call sites already have on hand.
#[allow(clippy::too_many_arguments)]
fn beta_link_sheet_fields(
    __scope: &mut RenderScope,
    reader_name: Signal<String>,
    max_chapter_text: Signal<String>,
    pin_version: Signal<bool>,
    username: Signal<String>,
    error: Signal<Option<String>>,
    max_chapter_input_id: &'static str,
    onsubmit: impl Fn() + 'static + Copy,
) -> NodeHandle {
    rsx! {
        Fragment {
            TextInput {
                label: "Reader Name",
                placeholder: "e.g. Alice, Book Club, etc.",
                value_fn: move || reader_name.get(),
                oninput: move |v: String| reader_name.set(v),
                onsubmit: onsubmit,
            }
            Space { h: "md" }
            Text { size: "sm", color: "dimmed",
                "Optionally restrict how many chapters the reader can access:"
            }
            Space { h: "xs" }
            div {
                style: "display: flex; align-items: center; gap: 8px;",
                Text { size: "sm", "Max chapters:" }
                input {
                    id: max_chapter_input_id,
                    r#type: "number",
                    min: "1",
                    placeholder: "All",
                    value: {move || max_chapter_text.get()},
                    oninput: move |v: String| max_chapter_text.set(v),
                    style: "width: 80px; padding: 4px 8px; border: 1px solid var(--rinch-color-border); border-radius: 4px; background: var(--rinch-color-surface); color: var(--rinch-color-text); font-size: 13px;",
                }
            }
            Space { h: "md" }
            div {
                style: "display: flex; align-items: center; gap: 8px; cursor: pointer;",
                onclick: move || pin_version.update(|v| *v = !*v),
                div {
                    style: "width: 20px; height: 20px; border: 1px solid var(--rinch-color-border); border-radius: 3px; display: flex; align-items: center; justify-content: center; font-size: 14px;",
                    {move || if pin_version.get() { "\u{2713}" } else { "" }}
                }
                Text { size: "sm", "Pin to current version" }
            }
            Text { size: "xs", color: "dimmed",
                "If pinned, the reader sees a snapshot of the book as it is now. Otherwise, they always see the latest version."
            }
            Space { h: "md" }
            TextInput {
                label: "PlotWeb Username (optional)",
                placeholder: "Attach to a registered user",
                value_fn: move || username.get(),
                oninput: move |v: String| username.set(v),
            }
            Text { size: "xs", color: "dimmed",
                "If set, this user will see the book on their dashboard."
            }
            if let Some(ref err) = error.get() {
                Space { h: "xs" }
                Text { size: "xs", color: "red", {err.clone()} }
            }
        }
    }
}

/// Render the book page's overlays: two `Dialog`s (restore, delete-chapter),
/// one chrome-only `Dialog` (add note), and four `Sheet`s (import, export,
/// book settings, beta link — the last covering both create and edit).
///
/// Takes the handful of closures the overlays invoke on submit — everything
/// else comes off `state`/`store`. `add_chapter`/`save_rename_chapter` moved
/// to `panes::chapters` with Tier 1; this only keeps what still opens an
/// overlay.
#[allow(clippy::too_many_arguments)]
pub(super) fn render<AN, SBS, CDC, ABL, UBL>(
    __scope: &mut RenderScope,
    state: BookState,
    store: AppStore,
    book_id: String,
    add_note: AN,
    save_book_settings: SBS,
    confirm_delete_chapter: CDC,
    add_beta_link: ABL,
    update_beta_link: UBL,
) -> NodeHandle
where
    AN: Fn() + 'static + Copy,
    SBS: Fn() + 'static + Copy,
    CDC: Fn() + 'static + Copy,
    ABL: Fn() + 'static + Copy,
    UBL: Fn() + 'static + Copy,
{
    let BookState {
        show_restore_confirm,
        bid_signal,
        history_commits,
        history_preview_commit,
        history_preview_chapters,
        history_preview_content,
        history_diff,
        active_pane,
        delete_chapter_target,
        show_note_modal,
        new_note_title,
        new_note_color,
        show_book_settings_modal,
        edit_book_title,
        edit_book_desc,
        edit_book_cover,
        show_beta_link_modal,
        new_beta_reader_name,
        new_beta_max_chapter_text,
        new_beta_max_chapter,
        new_beta_pin_version,
        new_beta_username,
        beta_link_error,
        editing_beta_link,
        edit_beta_reader_name,
        edit_beta_max_chapter_text,
        edit_beta_max_chapter,
        edit_beta_pinned,
        edit_beta_username,
        show_import_modal,
        import_preview,
        import_full_chapters,
        import_error,
        import_filename,
        import_file,
        import_loading,
        show_export_modal,
        export_error,
        export_format,
        export_selected,
        export_loading,
        ..
    } = state;

    let submit_new_beta_link = move || {
        // Parse the max chapter input from its bound signal.
        match new_beta_max_chapter_text.get().parse::<i64>() {
            Ok(n) => new_beta_max_chapter.set(Some(n - 1)), // 0-indexed
            Err(_) => new_beta_max_chapter.set(None),
        }
        add_beta_link();
    };

    let submit_edit_beta_link = move || {
        match edit_beta_max_chapter_text.get().parse::<i64>() {
            Ok(n) => edit_beta_max_chapter.set(Some(n - 1)), // 0-indexed
            Err(_) => edit_beta_max_chapter.set(None),
        }
        update_beta_link();
    };

    rsx! {
        Fragment {
            // ── Tier 2 — Dialog: restore a version ──────────────────────
            Dialog {
                opened_fn: move || show_restore_confirm.get().is_some(),
                onclose: move || show_restore_confirm.set(None),
                title: "Restore this version?",
                danger: false,
                confirm_label: "Restore",
                onconfirm: move || {
                    if let Some(oid) = show_restore_confirm.get() {
                        let bid = bid_signal.get();
                        show_restore_confirm.set(None);
                        let bid_book = bid.clone();
                        let bid_ch = bid.clone();
                        let bid_rest = bid.clone();
                        let bid_hist = bid.clone();
                        api::post::<_, serde_json::Value>(
                            &format!("/api/books/{}/history/{}/restore", bid, oid),
                            &serde_json::json!({}),
                            move |result| {
                                if result.is_ok() {
                                    // Refresh book data
                                    api::get::<Book>(&format!("/api/books/{}", bid_book), move |book_result| {
                                        if let Ok(book) = book_result {
                                            store.current_book.set(Some(book));
                                        }
                                        api::get::<Vec<Chapter>>(&format!("/api/books/{}/chapters", bid_ch), move |ch_result| {
                                            if let Ok(chapters) = ch_result {
                                                // A restore is a fresh REST answer about which
                                                // chapters exist, so the projection's snapshot
                                                // takes it too — otherwise the next projection
                                                // paints the pre-restore list back.
                                                crate::local_book::rest_chapters(&bid_rest, |ch| {
                                                    *ch = chapters.clone()
                                                });
                                                store.chapters.set(chapters);
                                            }
                                            // Refresh history
                                            api::get::<Vec<CommitInfo>>(&format!("/api/books/{}/history", bid_hist), move |commits_result| {
                                                if let Ok(commits) = commits_result {
                                                    history_commits.set(commits);
                                                }
                                                history_preview_commit.set(None);
                                                history_preview_chapters.set(Vec::new());
                                                history_preview_content.set(None);
                                                history_diff.set(None);
                                                active_pane.set(BookPane::Chapters);
                                            });
                                        });
                                    });
                                }
                            },
                        );
                    }
                },

                Text { size: "sm",
                    "Your current version will be preserved in history."
                }
            }

            // ── Tier 2 — Dialog: delete a chapter ───────────────────────
            // Previously had no confirmation at all — one misclick removed a
            // chapter outright. Matches the dashboard's book-delete confirm:
            // names the thing, states the word count, "This cannot be undone."
            Dialog {
                opened_fn: move || delete_chapter_target.get().is_some(),
                onclose: move || delete_chapter_target.set(None),
                title: {move || {
                    delete_chapter_target.get()
                        .map(|(_, title, _)| format!("Delete \"{title}\"?"))
                        .unwrap_or_default()
                }},
                danger: true,
                confirm_label: "Delete",
                onconfirm: confirm_delete_chapter,

                div {
                    {move || {
                        delete_chapter_target.get()
                            .map(|(_, _, wc)| format!("{} words.", super::panes::chapters::format_word_count_full(wc)))
                            .unwrap_or_default()
                    }}
                }
                span { class: "pw-dialog-warning", "This cannot be undone." }
            }

            // ── Tier 2 — Dialog (chrome only): Add Note ─────────────────
            // Notes as a surface is being rebuilt later — this keeps its
            // current behaviour and adopts Dialog's chrome, nothing else.
            Dialog {
                opened_fn: move || show_note_modal.get(),
                onclose: move || show_note_modal.set(false),
                title: "Add Note",
                confirm_label: "Add",
                onconfirm: add_note,

                TextInput {
                    label: "Note Title",
                    placeholder: "Enter note title",
                    value_fn: move || new_note_title.get(),
                    oninput: move |v: String| new_note_title.set(v),
                    onsubmit: add_note,
                }
                Space { h: "sm" }
                Text { size: "sm", weight: "500", "Color" }
                Space { h: "xs" }
                div { class: "note-color-picker",
                    for color in ["teal", "blue", "violet", "pink", "red", "orange", "yellow", "green", "gray"] {
                        div {
                            key: color,
                            class: {
                                let c = color.to_string();
                                move || if new_note_color.get() == c { "note-color-dot selected" } else { "note-color-dot" }
                            },
                            style: {
                                let c = color.to_string();
                                move || format!("background: var(--rinch-color-{}-6);", c)
                            },
                            onclick: {
                                let c = color.to_string();
                                move || new_note_color.set(c.clone())
                            },
                        }
                    }
                }
            }

            // ── Tier 3 — Sheet: Book settings ───────────────────────────
            Sheet {
                opened_fn: move || show_book_settings_modal.get(),
                onclose: move || show_book_settings_modal.set(false),
                title: "Book Settings",

                TextInput {
                    label: "Title",
                    placeholder: "Book title",
                    value_fn: move || edit_book_title.get(),
                    oninput: move |v: String| edit_book_title.set(v),
                    onsubmit: save_book_settings,
                }
                Space { h: "md" }
                Textarea {
                    label: "Description",
                    placeholder: "What's this book about?",
                    value_fn: move || edit_book_desc.get(),
                    oninput: move |v: String| edit_book_desc.set(v),
                }
                Space { h: "md" }
                Text { size: "sm", weight: "500", "Cover Image" }
                Space { h: "xs" }
                if edit_book_cover.get().is_some() {
                    div {
                        style: "display: flex; align-items: flex-start; gap: 12px;",
                        img {
                            src: {move || edit_book_cover.get().unwrap_or_default()},
                            style: "max-width: 120px; max-height: 160px; border-radius: var(--rinch-radius-sm); object-fit: cover; border: 1px solid var(--rinch-color-border);",
                        }
                        Button {
                            variant: "subtle",
                            size: "xs",
                            onclick: move || edit_book_cover.set(None),
                            "Remove"
                        }
                    }
                }
                Space { h: "xs" }
                Button {
                    variant: "light",
                    size: "sm",
                    onclick: {
                        let bid_for_cover = book_id.clone();
                        move || {
                            let bid = bid_for_cover.clone();
                            let Some(doc) = crate::platform::window().and_then(|w| w.document()) else { return; };
                            let Ok(input) = doc.create_element("input") else { return; };
                            let input: web_sys::HtmlInputElement = input.unchecked_into();
                            input.set_type("file");
                            input.set_accept("image/*");
                            input.set_id("__pw_cover_input");
                            let onchange = wasm_bindgen::closure::Closure::wrap(Box::new(move |_: web_sys::Event| {
                                let input: web_sys::HtmlInputElement = crate::platform::window()
                                    .and_then(|w| w.document())
                                    .and_then(|d| d.get_element_by_id("__pw_cover_input"))
                                    .map(|e| e.unchecked_into())
                                    .unwrap();
                                let Some(files) = input.files() else { return; };
                                let Some(file) = files.get(0) else { return; };
                                let bid = bid.clone();
                                wasm_bindgen_futures::spawn_local(async move {
                                    if let Ok(resp) = crate::api::upload_image(&bid, &file).await {
                                        edit_book_cover.set(Some(resp.url));
                                    }
                                });
                                input.remove();
                            }) as Box<dyn FnMut(_)>);
                            input.set_onchange(Some(onchange.as_ref().unchecked_ref()));
                            onchange.forget();
                            doc.body().unwrap().append_child(&input).ok();
                            input.click();
                        }
                    },
                    {move || if edit_book_cover.get().is_some() { "Change Cover" } else { "Upload Cover" }}
                }
                Space { h: "lg" }
                Group {
                    justify: "flex-end",
                    Button {
                        variant: "subtle",
                        onclick: move || show_book_settings_modal.set(false),
                        "Cancel"
                    }
                    Button {
                        onclick: save_book_settings,
                        "Save"
                    }
                }
            }

            // ── Tier 3 — Sheet: Create beta link ────────────────────────
            Sheet {
                opened_fn: move || show_beta_link_modal.get(),
                onclose: move || show_beta_link_modal.set(false),
                title: "Create Beta Reader Link",

                {beta_link_sheet_fields(
                    __scope,
                    new_beta_reader_name,
                    new_beta_max_chapter_text,
                    new_beta_pin_version,
                    new_beta_username,
                    beta_link_error,
                    "beta-max-chapter-input",
                    submit_new_beta_link,
                )}
                Space { h: "lg" }
                Group {
                    justify: "flex-end",
                    Button {
                        variant: "subtle",
                        onclick: move || show_beta_link_modal.set(false),
                        "Cancel"
                    }
                    Button {
                        onclick: submit_new_beta_link,
                        "Create"
                    }
                }
            }

            // ── Tier 3 — Sheet: Edit beta link ───────────────────────────
            // Same body as Create (`beta_link_sheet_fields`), bound to the
            // `edit_beta_*` signal set instead of `new_beta_*` — this and the
            // sheet above were ~95% identical, including duplicated one-off
            // inline styles, before that helper existed.
            Sheet {
                opened_fn: move || editing_beta_link.get().is_some(),
                onclose: move || editing_beta_link.set(None),
                title: "Edit Beta Reader Link",

                {beta_link_sheet_fields(
                    __scope,
                    edit_beta_reader_name,
                    edit_beta_max_chapter_text,
                    edit_beta_pinned,
                    edit_beta_username,
                    beta_link_error,
                    "beta-edit-max-chapter-input",
                    submit_edit_beta_link,
                )}
                Space { h: "lg" }
                Group {
                    justify: "flex-end",
                    Button {
                        variant: "subtle",
                        onclick: move || editing_beta_link.set(None),
                        "Cancel"
                    }
                    Button {
                        onclick: submit_edit_beta_link,
                        "Save"
                    }
                }
            }

            // ── Tier 3 — Sheet: Import manuscript ───────────────────────
            Sheet {
                opened_fn: move || show_import_modal.get(),
                onclose: move || {
                    show_import_modal.set(false);
                    import_preview.set(Vec::new());
                    import_full_chapters.set(Vec::new());
                    import_error.set(None);
                    import_filename.set(String::new());
                    import_file.set(None);
                },
                title: "Import Manuscript",

                if import_preview.get().is_empty() && !import_loading.get() {
                    // File picker step
                    Text { size: "sm", color: "dimmed",
                        "Upload a .md, .txt, or .docx file. Chapters will be detected automatically from headings, \"Chapter\" markers, or ALL-CAPS titles."
                    }
                    Space { h: "md" }
                    div {
                        style: "display: flex; flex-direction: column; align-items: center; padding: 32px; border: 2px dashed var(--rinch-color-border); border-radius: 8px; cursor: pointer;",
                        onclick: move || {
                            // Create a temporary file input, attach a change
                            // listener, and click it.  rinch's onchange passes
                            // a String (the input value) which is useless for
                            // file inputs, so we use the raw DOM API instead.
                            let doc = match crate::platform::window().and_then(|w| w.document()) {
                                Some(d) => d,
                                None => return,
                            };
                            let input: web_sys::HtmlInputElement = doc
                                .create_element("input").unwrap()
                                .dyn_into().unwrap();
                            input.set_type("file");
                            input.set_accept(".md,.txt,.docx,.markdown");
                            input.style().set_property("display", "none").ok();
                            doc.body().unwrap().append_child(&input).ok();

                            let input_clone = input.clone();
                            let change_handler = wasm_bindgen::closure::Closure::wrap(Box::new(move |_event: web_sys::Event| {
                                if let Some(files) = input_clone.files() {
                                    if let Some(file) = files.get(0) {
                                        import_loading.set(true);
                                        import_error.set(None);
                                        import_file.set(Some(file.clone()));
                                        let bid = bid_signal.get();
                                        let input_remove = input_clone.clone();
                                        wasm_bindgen_futures::spawn_local(async move {
                                            match api::upload_file::<ImportPreviewResponse>(
                                                &format!("/api/books/{}/import/preview", bid),
                                                &file,
                                            ).await {
                                                Ok(resp) => {
                                                    import_filename.set(resp.filename);
                                                    import_preview.set(resp.chapters);
                                                }
                                                Err(e) => {
                                                    import_error.set(Some(e.message));
                                                }
                                            }
                                            import_loading.set(false);
                                            input_remove.remove();
                                        });
                                    }
                                }
                            }) as Box<dyn FnMut(_)>);
                            input.add_event_listener_with_callback("change", change_handler.as_ref().unchecked_ref()).ok();
                            change_handler.forget();
                            input.click();
                        },
                        {render_tabler_icon(__scope, TablerIcon::Upload, TablerIconStyle::Outline)}
                        Space { h: "xs" }
                        Text { size: "sm", color: "dimmed", "Click to select file" }
                    }
                }

                if import_loading.get() {
                    Space { h: "lg" }
                    Center {
                        Text { color: "dimmed", "Analyzing manuscript..." }
                    }
                    Space { h: "lg" }
                }

                if let Some(err) = import_error.get() {
                    Space { h: "sm" }
                    Text { size: "sm", color: "red", {err} }
                }

                if !import_preview.get().is_empty() {
                    // Preview step
                    Text { size: "sm", color: "dimmed",
                        {move || format!("Found {} chapters in {}", import_preview.get().len(), import_filename.get())}
                    }
                    Space { h: "md" }
                    div {
                        style: "max-height: 400px; overflow-y: auto; display: flex; flex-direction: column; gap: 6px;",
                        for (i, ch) in import_preview.get().into_iter().enumerate() {
                            Paper {
                                key: format!("import-ch-{}", i),
                                p: "sm",
                                radius: "sm",
                                shadow: "xs",
                                div {
                                    style: "display: flex; align-items: center; gap: 8px; margin-bottom: 4px;",
                                    Badge { variant: "light", size: "sm", {format!("{}", i + 1)} }
                                    Text { weight: "600", size: "sm", {ch.title.clone()} }
                                    Text { size: "xs", color: "dimmed", {format!("{} words", ch.word_count)} }
                                }
                                Text { size: "xs", color: "dimmed",
                                    {ch.content_preview.clone()}
                                }
                            }
                        }
                    }
                    Space { h: "lg" }
                    Group {
                        justify: "flex-end",
                        Button {
                            variant: "subtle",
                            onclick: move || {
                                import_preview.set(Vec::new());
                                import_full_chapters.set(Vec::new());
                                import_error.set(None);
                                import_filename.set(String::new());
                            },
                            "Back"
                        }
                        Button {
                            onclick: move || {
                                let bid = bid_signal.get();
                                let file = match import_file.get() {
                                    Some(f) => f,
                                    None => return,
                                };
                                import_loading.set(true);
                                wasm_bindgen_futures::spawn_local(async move {
                                    match api::upload_file::<Vec<Chapter>>(
                                        &format!("/api/books/{}/import/confirm", bid),
                                        &file,
                                    ).await {
                                        Ok(new_chapters) => {
                                            crate::local_book::rest_chapters(&bid, |ch| {
                                                ch.extend(new_chapters.clone())
                                            });
                                            store.chapters.update(|ch| ch.extend(new_chapters));
                                            show_import_modal.set(false);
                                            import_preview.set(Vec::new());
                                            import_full_chapters.set(Vec::new());
                                            import_error.set(None);
                                            import_filename.set(String::new());
                                            import_file.set(None);
                                        }
                                        Err(e) => {
                                            import_error.set(Some(e.message));
                                        }
                                    }
                                    import_loading.set(false);
                                });
                            },
                            {move || format!("Import {} Chapters", import_preview.get().len())}
                        }
                    }
                }
            }

            // ── Tier 3 — Sheet: Export manuscript ───────────────────────
            Sheet {
                opened_fn: move || show_export_modal.get(),
                onclose: move || {
                    show_export_modal.set(false);
                    export_error.set(None);
                },
                title: "Export Manuscript",

                Text { size: "sm", color: "dimmed",
                    "Choose a format and which chapters to include, then download your manuscript."
                }
                Space { h: "md" }

                Text { size: "xs", weight: "600", color: "dimmed", "FORMAT" }
                Space { h: "xs" }
                Group {
                    gap: "xs",
                    Button {
                        size: "sm",
                        variant: {move || if export_format.get() == "md" { "filled".to_string() } else { "outline".to_string() }},
                        onclick: move || export_format.set("md"),
                        "Markdown"
                    }
                    Button {
                        size: "sm",
                        variant: {move || if export_format.get() == "docx" { "filled".to_string() } else { "outline".to_string() }},
                        onclick: move || export_format.set("docx"),
                        "DOCX"
                    }
                    Button {
                        size: "sm",
                        variant: {move || if export_format.get() == "epub" { "filled".to_string() } else { "outline".to_string() }},
                        onclick: move || export_format.set("epub"),
                        "EPUB"
                    }
                    Button { size: "sm", variant: "subtle", disabled: true, onclick: move || {}, "PDF · soon" }
                }
                Space { h: "md" }

                Group {
                    justify: "space-between",
                    Text { size: "xs", weight: "600", color: "dimmed", "CHAPTERS" }
                    Group {
                        gap: "xs",
                        Button {
                            size: "xs",
                            variant: "subtle",
                            onclick: move || {
                                let all: std::collections::HashSet<String> =
                                    store.chapters.get().iter().map(|c| c.id.clone()).collect();
                                export_selected.set(all);
                            },
                            "All"
                        }
                        Button {
                            size: "xs",
                            variant: "subtle",
                            onclick: move || export_selected.set(std::collections::HashSet::new()),
                            "None"
                        }
                    }
                }
                Space { h: "xs" }
                div {
                    style: "max-height: 320px; overflow-y: auto; display: flex; flex-direction: column; gap: 4px;",
                    for ch in store.chapters.get() {
                        Checkbox {
                            key: ch.id.clone(),
                            label: ch.title.clone(),
                            checked_fn: {
                                let id = ch.id.clone();
                                move || export_selected.get().contains(&id)
                            },
                            onchange: {
                                let id = ch.id.clone();
                                move || {
                                    let id = id.clone();
                                    export_selected.update(|set| {
                                        if !set.remove(&id) {
                                            set.insert(id);
                                        }
                                    });
                                }
                            },
                        }
                    }
                }

                if let Some(err) = export_error.get() {
                    Space { h: "sm" }
                    Text { size: "sm", color: "red", {err} }
                }

                Space { h: "lg" }
                Group {
                    justify: "flex-end",
                    Button {
                        variant: "subtle",
                        onclick: move || {
                            show_export_modal.set(false);
                            export_error.set(None);
                        },
                        "Cancel"
                    }
                    Button {
                        onclick: move || {
                            let sel = export_selected.get();
                            if sel.is_empty() {
                                export_error.set(Some("Select at least one chapter.".to_string()));
                                return;
                            }
                            let bid = bid_signal.get();
                            let fmt = export_format.get();
                            let total = store.chapters.get().len();
                            // Whole book → omit the chapters param entirely.
                            let url = if sel.len() >= total {
                                format!("/api/books/{}/export?format={}", bid, fmt)
                            } else {
                                let ids: Vec<String> = store
                                    .chapters
                                    .get()
                                    .iter()
                                    .filter(|c| sel.contains(&c.id))
                                    .map(|c| c.id.clone())
                                    .collect();
                                format!(
                                    "/api/books/{}/export?format={}&chapters={}",
                                    bid,
                                    fmt,
                                    ids.join(",")
                                )
                            };
                            export_loading.set(true);
                            export_error.set(None);
                            let fallback = format!("manuscript.{}", fmt);
                            wasm_bindgen_futures::spawn_local(async move {
                                match api::download_file(&url, &fallback).await {
                                    Ok(()) => {
                                        show_export_modal.set(false);
                                    }
                                    Err(e) => {
                                        export_error.set(Some(e.message));
                                    }
                                }
                                export_loading.set(false);
                            });
                        },
                        {move || if export_loading.get() {
                            "Exporting...".to_string()
                        } else {
                            format!("Export {} chapters", export_selected.get().len())
                        }}
                    }
                }
            }
        }
    }
}
