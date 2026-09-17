//! Chapters pane (the book's table of contents) plus the chapter-switch and
//! chapter-body-save machinery it and the editor pane share.

use rinch::prelude::*;
// `web_sys`/`wasm_bindgen` types compile on both targets (only *calling into
// the browser* panics off-wasm, per `crate::platform`'s docs) — `JsCast` is
// used both inside and outside the `web_only!` block below, so unlike the
// wasm32-gated import this file used to have, this one is used on native too
// (to no-op safely once `crate::platform::document()` answers `None`).
use wasm_bindgen::JsCast;
use rinch_tabler_icons::{TablerIcon, TablerIconStyle, render_tabler_icon};
use plotweb_common::{Chapter, SaveReceipt, UpdateChapterRequest};

use crate::api;
use crate::components::pane_header::PaneHeader;
use crate::pages::editor_utils;
use crate::rinch_backend::EditorHandle;
use crate::store::AppStore;

use super::super::state::BookState;
use super::super::BookPane;
#[cfg(target_arch = "wasm32")]
use super::editor::scroll_to_text_in_editor;

/// "3,140" — full comma-grouped count for the pane's rows, where there's room
/// for the exact number. The sidebar's abbreviated "3.1k" is a different
/// helper ([`abbreviate_word_count`]) because the two surfaces have different
/// space budgets, not because the underlying count differs.
pub(in crate::pages::book) fn format_word_count_full(count: u64) -> String {
    let s = count.to_string();
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(*b as char);
    }
    out
}

/// "3.1k" — abbreviated count for the sidebar row, which has one line and no
/// room for "3,140". Mirrors `pages/dashboard.rs`'s private `format_word_count`
/// (same rounding rule) rather than sharing it, since that one isn't public.
pub(in crate::pages::book) fn abbreviate_word_count(count: u64) -> String {
    if count >= 1_000_000 {
        format!("{:.1}M", count as f64 / 1_000_000.0)
    } else if count >= 1_000 {
        format!("{:.1}k", count as f64 / 1_000.0)
    } else {
        count.to_string()
    }
}

/// Apply a save response to the editor's status and the author-facing alert.
///
/// A `200` is not by itself success. The server answers with a [`SaveReceipt`] saying
/// where the content actually went — git, the canonical document, or deliberately
/// neither because a healthy sync engine is carrying it — and a write that reached
/// none of those, or that only landed after the server overruled how this client
/// believed the body was being carried, is precisely the case that used to read as
/// "Saved" while two days of writing went nowhere.
/// PUT a chapter body, and take the write back if the server could not place it.
///
/// One writer decides who carries a cut-over book's body, and the server is the one who
/// knows: it lets sync own the body only while the canonical document is one it can
/// actually read and write (`canonical_is_authoritative`). When that check fails it
/// intends to take the content to git after all and say so on the receipt — but the
/// client stopped sending content for a cut-over book at all, so there was nothing to
/// take. The save then landed nowhere and the author was told nothing beyond a
/// four-word status: exactly the two-day silence #48 set out to end, arrived at from
/// the other side.
///
/// So a non-durable receipt is answered once, with the content: the canonical copy is
/// broken, sync has already stood down server-side, and git is the only writer left.
/// Once, deliberately — a second refusal is reported rather than retried.
pub(in crate::pages::book) fn save_chapter_body(
    url: String,
    content: String,
    cut_over: bool,
    save_status: Signal<&'static str>,
    save_alert: Signal<Option<String>>,
) {
    let req = UpdateChapterRequest {
        title: None,
        content: (!cut_over).then(|| content.clone()),
    };
    let retry_url = url.clone();
    api::put::<_, SaveReceipt>(&url, &req, move |result| match result {
        Ok(receipt) if cut_over && !receipt.is_durable() => {
            let req = UpdateChapterRequest {
                title: None,
                content: Some(content),
            };
            api::put::<_, SaveReceipt>(&retry_url, &req, move |result| {
                apply_save_receipt(result, save_status, save_alert)
            });
        }
        other => apply_save_receipt(other, save_status, save_alert),
    });
}

pub(in crate::pages::book) fn apply_save_receipt(
    result: Result<SaveReceipt, api::ApiError>,
    save_status: Signal<&'static str>,
    save_alert: Signal<Option<String>>,
) {
    match result {
        Ok(receipt) => {
            save_status.set(if receipt.is_durable() { "saved" } else { "error" });
            // The warning clears itself on the next clean save, so a condition that
            // resolves stops nagging without the author dismissing anything.
            save_alert.set(receipt.warning);
        }
        Err(e) => {
            save_status.set("error");
            save_alert.set(Some(format!(
                "This save didn't reach the server ({}). Your work is still on this \
                 device — keep this tab open.",
                e.message
            )));
        }
    }
}

/// Switch to editing a chapter. Saves current chapter if editing, clears timers, loads new chapter.
/// All parameters are Copy or Clone so this can be called from rsx! closures without ownership issues.
#[allow(clippy::too_many_arguments)]
pub(in crate::pages::book) fn do_switch_chapter(
    active_pane: Signal<BookPane>,
    save_alert: Signal<Option<String>>,
    auto_save_timer_id: Signal<Option<rinch_core::TimeoutHandle>>,
    chapter_title_save_timer_id: Signal<Option<rinch_core::TimeoutHandle>>,
    save_status: Signal<&'static str>,
    editor_word_count: Signal<u64>,
    loaded_chapter_id: Signal<Option<String>>,
    chapter_dirty: Signal<bool>,
    chapter_handle: Signal<EditorHandle>,
    chapter_title: Signal<String>,
    store: AppStore,
    bid: &str,
    new_chapter_id: &str,
) {
    do_switch_chapter_inner(active_pane, save_alert, auto_save_timer_id, chapter_title_save_timer_id, save_status, editor_word_count, loaded_chapter_id, chapter_dirty, chapter_handle, chapter_title, store, bid, new_chapter_id, None);
}

#[allow(clippy::too_many_arguments)]
pub(in crate::pages::book) fn do_switch_chapter_inner(
    active_pane: Signal<BookPane>,
    save_alert: Signal<Option<String>>,
    auto_save_timer_id: Signal<Option<rinch_core::TimeoutHandle>>,
    chapter_title_save_timer_id: Signal<Option<rinch_core::TimeoutHandle>>,
    save_status: Signal<&'static str>,
    editor_word_count: Signal<u64>,
    loaded_chapter_id: Signal<Option<String>>,
    chapter_dirty: Signal<bool>,
    chapter_handle: Signal<EditorHandle>,
    chapter_title: Signal<String>,
    store: AppStore,
    bid: &str,
    new_chapter_id: &str,
    pending_scroll: Option<Signal<Option<(String, String)>>>,
) {
    // Clear any pending auto-save timer
    if let Some(h) = auto_save_timer_id.get() {
        rinch_core::clear_timeout(h);
        auto_save_timer_id.set(None);
    }

    // Flush, then clear, any pending chapter-title save. The debounced title PUT
    // (save_chapter_title_inline) hasn't fired yet; cancelling its timer without
    // saving would drop a title edit made just before switching. Persist the
    // leaving chapter's current title now via its own PUT (mirroring the debounce).
    if let Some(h) = chapter_title_save_timer_id.get() {
        rinch_core::clear_timeout(h);
        chapter_title_save_timer_id.set(None);
        if let BookPane::Editor(leaving_cid) = active_pane.get() {
            let title = chapter_title.get();
            store.chapters.update(|chapters| {
                if let Some(ch) = chapters.iter_mut().find(|c| c.id == leaving_cid) {
                    ch.title = title.clone();
                }
            });
            let bid = bid.to_string();
            crate::local_book::sync_chapters(&bid, &store.chapters.get());
            let req = UpdateChapterRequest {
                title: Some(title),
                content: None,
            };
            api::put::<_, serde_json::Value>(
                &format!("/api/books/{}/chapters/{}", bid, leaving_cid),
                &req,
                move |_result| {},
            );
        }
    }

    // Save the chapter we're leaving — but ONLY if its content is the one currently
    // loaded in the editor model. While a switch is still loading (loaded_chapter_id
    // != the pane's chapter), the model holds the *previous* chapter's content;
    // reading it here and writing it to `current_id` would overwrite that chapter
    // with another chapter's content (the chapter-overwrite bug when switching
    // quickly between chapters).
    let leaving_id = match active_pane.get() {
        // ...and only if it was actually edited. Without that, leaving a chapter you
        // merely looked at rewrites it with whatever the editor was handed — which is
        // how a divergent or mis-loaded document overwrites the stored one.
        BookPane::Editor(id)
            if loaded_chapter_id.get().as_deref() == Some(id.as_str()) && chapter_dirty.get() =>
        {
            Some(id)
        }
        _ => None,
    };
    if let Some(current_id) = leaving_id {
        let bid = bid.to_string();
        // Read the durable save shape straight from the editor model (synchronous,
        // no DOM race): the model always reflects the loaded chapter.
        if let Some(content) = editor_utils::editor_content_json(&chapter_handle.get()) {
            chapter_dirty.set(false);
            save_status.set("saving");
            // Same rule as the autosave's — this path builds its own PUT rather than
            // going through `save_content`. A cut-over book takes body edits through
            // sync only, so this write carries structure and not content.
            let cut_over = store.current_book.get().map(|b| b.cutover).unwrap_or(false);
            // The just-left chapter's save is reported like any other: an optimistic
            // "saved" is set below for the chapter being opened, and a receipt that
            // says this write went nowhere has to win over it.
            save_chapter_body(
                format!("/api/books/{}/chapters/{}", bid, current_id),
                content,
                cut_over,
                save_status,
                save_alert,
            );
        }
    }

    // Switch pane
    let new_cid = new_chapter_id.to_string();
    active_pane.set(BookPane::Editor(new_cid.clone()));
    save_status.set("saved");
    store.sidebar_open.set(false);

    // Mark the editor as not-yet-loaded for the new chapter so a save can't fire
    // against `new_cid` while the model still holds the previous chapter.
    loaded_chapter_id.set(None);
    let bid = bid.to_string();
    api::get::<Chapter>(
        &format!("/api/books/{}/chapters/{}", bid, new_cid),
        move |result| {
        if let Ok(chapter) = result {
            // Bail if the user switched away while this fetch was in flight, so a
            // slow stale response can't clobber the now-current chapter's editor.
            if active_pane.get() != BookPane::Editor(new_cid.clone()) {
                return;
            }
            chapter_title.set(chapter.title.clone());
            editor_word_count.set(chapter.word_count);

            // Load into the editor model (synchronous). Legacy-tolerant: DocNode JSON
            // if it parses, else the legacy Markdown → HTML path.
            let handle = chapter_handle.get();
            editor_utils::load_chapter_content(&handle, &chapter.content);
            handle.set_dark_mode(store.dark_mode.get());

            // The model now reflects the current chapter — save-on-leave / autosave
            // may safely persist it.
            loaded_chapter_id.set(Some(new_cid.clone()));
            // What a rescued copy is compared against (see `store.open_body`).
            store.open_body.set(Some((
                format!("chapter:{new_cid}"),
                chapter.content.clone(),
            )));
            // Freshly loaded: nothing to save until the author changes something.
            chapter_dirty.set(false);

            // Local-first (Phase 2 · Slice 1 · deliverable 1): back this chapter body
            // with a durable Automerge doc in rinch-storage. Additive — the REST
            // load above and the REST autosave elsewhere are unchanged (dual-write).
            // If a local doc exists it is adopted (guest + delta replay); otherwise
            // the just-loaded REST content seeds a fresh host doc. Every subsequent
            // keystroke's change delta is persisted locally.
            crate::local_store::attach_chapter(
                handle.clone(),
                bid.clone(),
                new_cid.clone(),
                chapter.content.clone(),
            );

            // Execute pending feedback scroll if any (walks the editor's rendered
            // text nodes under #editor-main). Delay a tick to let the view settle.
            if let Some(scroll_signal) = pending_scroll {
                if let Some((selected_text, context_block)) = scroll_signal.get() {
                    // Clearing the pending scroll is target-independent.
                    scroll_signal.set(None);
                    // Web-only: `scroll_to_text_in_editor` walks the DOM editor's
                    // rendered text nodes, so the scroll itself can't run natively.
                    let _ = (&selected_text, &context_block);
                    crate::web_only! {
                        let closure2 = wasm_bindgen::closure::Closure::once(move || {
                            scroll_to_text_in_editor(&selected_text, &context_block);
                        });
                        if let Some(w) = crate::platform::window() {
                            w.set_timeout_with_callback_and_timeout_and_arguments_0(
                                closure2.as_ref().unchecked_ref(),
                                50,
                            ).ok();
                        }
                        closure2.forget();
                    }
                }
            }
        }
        },
    );
}

/// The chapters pane only ever has one row in edit mode at a time (an
/// existing chapter's rename, or the draft row from "Add chapter"), so both
/// share this one stable id rather than a per-chapter id — there is never a
/// collision, and it gives this helper (and the Escape handler below it) a
/// single, simple target.
const INLINE_TITLE_INPUT_ID: &str = "chapter-inline-title";

/// Focus the just-mounted inline chapter-title input (cursor lands at the end
/// of its text — the browser default for a focused, already-valued input),
/// and wire real `keydown` (Enter commits, Escape cancels) and `blur`
/// (commits) listeners.
///
/// None of the three can be an rsx event prop on a raw element. `oninput` /
/// `onchange` / `onclick` (via `data-rid`) are the only ones rinch-web's
/// delegator gives a raw HTML element a real DOM listener for; `onsubmit` on
/// a raw element is not one of them despite compiling — the macro accepts
/// any `on`-prefixed name and falls through to `data-rid`, so an `onsubmit:`
/// prop on a plain `<input>` silently becomes a second `onclick`, not an
/// Enter handler (only rinch's own `TextInput` component wires `onsubmit` to
/// `data-onsubmit`, inside its own `Component::render()` — not something the
/// generic macro does for any element). `onblur`/`onkeydown` don't exist as
/// rsx event props at all. So all three are wired directly with real DOM
/// listeners here instead, which only exist on the web backend (`web_only!`
/// is the only safe home for a `wasm_bindgen::closure::Closure` — see that
/// module's docs on why a runtime guard isn't enough). On native, this whole
/// function is therefore a no-op beyond the focus call — a known gap with no
/// working commit/cancel path for this input at all there yet.
///
/// Called from an effect watching `editing_chapter_id`/`pending_new_chapter`
/// (see `render`, below) rather than directly from the pencil/"Add chapter"
/// click handlers: the sidebar's own "+" (see `mod.rs`) only sets
/// `pending_new_chapter` and switches pane, so an effect is the one place
/// that covers every entry point without each one re-scheduling the same
/// tick-then-focus dance. Still needs `set_timeout(0, ..)` even from an
/// effect — the input doesn't exist until the reactive `if` branch that
/// renders it has actually mounted, which happens synchronously after the
/// signal set but the effect body runs as part of that same update.
fn focus_inline_input(on_commit: impl Fn() + 'static + Copy, on_cancel: impl Fn() + 'static + Copy) {
    // Both closures are only actually invoked from inside the `web_only!`
    // block below, which compiles to nothing on native — without this,
    // `on_commit`/`on_cancel` would be unused params there.
    let _ = (&on_commit, &on_cancel);

    // `web_sys` types are unconditional deps (compile on both targets — only
    // *calling into the browser* panics off-wasm, per `crate::platform`'s
    // docs), and `crate::platform::document()` is `None` on native, so this
    // branch is simply never entered there. Only the `Closure`s below need
    // `web_only!`.
    if let Some(doc) = crate::platform::document() {
        if let Ok(Some(el)) = doc.query_selector(&format!("#{INLINE_TITLE_INPUT_ID}")) {
            if let Some(html_el) = el.dyn_ref::<web_sys::HtmlElement>() {
                html_el.focus().ok();
            }
        }
    }
    crate::web_only! {
        // Enter and Escape both resolve the field and must suppress the blur
        // their own `.blur()`/focus-loss is about to cause — otherwise Enter
        // would commit and then immediately commit again (empty, post-clear)
        // via the blur listener below, and Escape would cancel then
        // re-commit the just-discarded text the same way. Shared between all
        // three closures so whichever already ran marks it for the others.
        let resolved = std::rc::Rc::new(std::cell::Cell::new(false));

        let resolved_for_keydown = resolved.clone();
        let keydown = wasm_bindgen::closure::Closure::wrap(Box::new(move |event: web_sys::KeyboardEvent| {
            let is_enter = event.key() == "Enter" && !event.shift_key() && !event.is_composing();
            let is_escape = event.key() == "Escape";
            if !is_enter && !is_escape {
                return;
            }
            event.prevent_default();
            resolved_for_keydown.set(true);
            if is_enter {
                on_commit();
            } else {
                on_cancel();
            }
            if let Some(el) = crate::platform::document()
                .and_then(|d| d.active_element())
                .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok())
            {
                el.blur().ok();
            }
        }) as Box<dyn FnMut(_)>);

        let resolved_for_blur = resolved.clone();
        let blur = wasm_bindgen::closure::Closure::wrap(Box::new(move |_event: web_sys::FocusEvent| {
            if !resolved_for_blur.replace(false) {
                on_commit();
            }
        }) as Box<dyn FnMut(_)>);

        if let Some(doc) = crate::platform::document() {
            if let Ok(Some(el)) = doc.query_selector(&format!("#{INLINE_TITLE_INPUT_ID}")) {
                el.add_event_listener_with_callback("keydown", keydown.as_ref().unchecked_ref()).ok();
                el.add_event_listener_with_callback("blur", blur.as_ref().unchecked_ref()).ok();
            }
        }
        keydown.forget();
        blur.forget();
    }
}

/// Render the Chapters pane (CSS toggle, always in DOM).
#[allow(clippy::too_many_arguments)]
pub(in crate::pages::book) fn render<OC, OCO, RC, DC, DCO, AC, SRC>(
    __scope: &mut RenderScope,
    state: BookState,
    store: AppStore,
    open_chapter: OC,
    reorder_chapter: RC,
    request_delete_chapter: DC,
    add_chapter: AC,
    save_rename_chapter: SRC,
) -> NodeHandle
where
    OC: Fn(String) -> OCO + 'static + Copy,
    RC: Fn(String, usize) + 'static + Copy,
    DC: Fn(String, String, u64) -> DCO + 'static + Copy,
    DCO: Fn() + 'static,
    AC: Fn(String) + 'static + Copy,
    SRC: Fn(String, String) + 'static + Copy,
    OCO: Fn() + 'static,
{
    let BookState {
        active_pane,
        show_import_modal,
        import_preview,
        import_full_chapters,
        import_error,
        import_filename,
        export_selected,
        export_format,
        export_loading,
        export_error,
        show_export_modal,
        editing_chapter_id,
        editing_chapter_title,
        pending_new_chapter,
        dragging_chapter_id,
        chapter_drop_target,
        show_chapters_menu,
        ..
    } = state;

    let chapter_count = move || store.chapters.get().len();
    let total_words = move || store.chapters.get().iter().map(|c| c.word_count).sum::<u64>();

    // Cancel whatever inline edit is in progress — the existing-row rename, or
    // the not-yet-created draft row from "Add chapter". Escape's handler.
    let cancel_inline_edit = move || {
        editing_chapter_id.set(None);
        pending_new_chapter.set(false);
        editing_chapter_title.set(String::new());
    };

    // Commit the draft row from "Add chapter": create the chapter for real.
    let commit_new_chapter = move || {
        let title = editing_chapter_title.get();
        editing_chapter_title.set(String::new());
        pending_new_chapter.set(false);
        add_chapter(title);
    };

    // Commit an existing row's inline rename.
    let commit_rename = move |cid: String| {
        let title = editing_chapter_title.get();
        editing_chapter_title.set(String::new());
        editing_chapter_id.set(None);
        save_rename_chapter(cid, title);
    };

    // Blur's handler doesn't know which of the two edit modes is live — the
    // effect below wires it once for whichever the DOM currently has.
    let commit_whichever_is_active = move || {
        if let Some(cid) = editing_chapter_id.get() {
            commit_rename(cid);
        } else if pending_new_chapter.get() {
            commit_new_chapter();
        }
    };

    // Focus the inline title input whenever a row enters edit mode — covers
    // both entry points (this pane's own pencil/"Add chapter", and the
    // sidebar's "+", which only sets `pending_new_chapter` and switches pane
    // from `mod.rs`) with one reactive rule instead of duplicating the
    // schedule-a-tick-then-focus dance at every call site. Fires once, right
    // after the `if` branch that renders the input mounts it, since the
    // input doesn't exist in the DOM until this same signal-set is applied.
    __scope.create_effect(move || {
        if editing_chapter_id.get().is_some() || pending_new_chapter.get() {
            rinch_core::set_timeout(0, move || {
                focus_inline_input(commit_whichever_is_active, cancel_inline_edit);
            });
        }
    });

    rsx! {
        div {
            class: "book-main-scroll",
            style: {move || if matches!(active_pane.get(), BookPane::Chapters) { "" } else { "display:none;" }},

            div { class: "chapters-pane",
                PaneHeader {
                    title: "Manuscript",
                    subtitle: {|| format!(
                        "{} chapter{} · {} words",
                        chapter_count(),
                        if chapter_count() == 1 { "" } else { "s" },
                        format_word_count_full(total_words()),
                    )},

                    Popover {
                        opened_fn: move || show_chapters_menu.get(),
                        onclose: move || show_chapters_menu.set(false),
                        position: "bottom-end",
                        close_on_click_outside: true,
                        close_on_escape: true,
                        shadow: "md",
                        radius: "sm",

                        PopoverTarget {
                            ActionIcon {
                                variant: "subtle",
                                size: "sm",
                                onclick: move || show_chapters_menu.update(|v| *v = !*v),
                                {render_tabler_icon(__scope, TablerIcon::DotsVertical, TablerIconStyle::Outline)}
                            }
                        }
                        PopoverDropdown {
                            div { class: "chapters-menu",
                                button {
                                    class: "chapters-menu-item",
                                    onclick: move || {
                                        show_chapters_menu.set(false);
                                        show_import_modal.set(true);
                                        import_preview.set(Vec::new());
                                        import_full_chapters.set(Vec::new());
                                        import_error.set(None);
                                        import_filename.set(String::new());
                                    },
                                    {render_tabler_icon(__scope, TablerIcon::Upload, TablerIconStyle::Outline)}
                                    "Import"
                                }
                                button {
                                    class: "chapters-menu-item",
                                    onclick: move || {
                                        show_chapters_menu.set(false);
                                        // Default to exporting the whole book.
                                        let all: std::collections::HashSet<String> =
                                            store.chapters.get().iter().map(|c| c.id.clone()).collect();
                                        export_selected.set(all);
                                        export_format.set("md");
                                        export_error.set(None);
                                        export_loading.set(false);
                                        show_export_modal.set(true);
                                    },
                                    {render_tabler_icon(__scope, TablerIcon::Download, TablerIconStyle::Outline)}
                                    "Export"
                                }
                            }
                        }
                    }

                    Button {
                        size: "sm",
                        onclick: move || {
                            editing_chapter_id.set(None);
                            editing_chapter_title.set(String::new());
                            pending_new_chapter.set(true);
                        },
                        {render_tabler_icon(__scope, TablerIcon::Plus, TablerIconStyle::Outline)}
                        "Add chapter"
                    }
                }

                if store.chapters.get().is_empty() {
                    Center {
                        style: "padding: 40px 0;",
                        Text { color: "dimmed", "No chapters yet. Add one to start writing!" }
                    }
                }

                div {
                    class: "chapter-rows",
                    for (_i, chapter) in ({
                        store.chapters.get().into_iter().enumerate()
                    }) {
                        // Wrapped in Signals (as `panes/notes.rs`'s `nid` does for the
                        // same reason): the `for` body's prop closures each want their
                        // own copy of the id/title, and a plain `String` moved into the
                        // first closure that captures it is gone for the rest — a
                        // Signal is `Copy`, so every closure below can read it freely.
                        let cid = Signal::new(chapter.id.clone());
                        let ctitle = Signal::new(chapter.title.clone());
                        let cwords = chapter.word_count;
                        div {
                            key: chapter.id.clone(),
                            class: {move || {
                                let mut cls = "crow".to_string();
                                if dragging_chapter_id.get().as_deref() == Some(cid.get().as_str()) {
                                    cls.push_str(" dragging");
                                }
                                if chapter_drop_target.get() == Some(_i) && dragging_chapter_id.get().is_some() {
                                    cls.push_str(" drop-target");
                                }
                                cls
                            }},
                            draggable: "true",
                            ondragstart: move || dragging_chapter_id.set(Some(cid.get())),
                            ondragover: move || {
                                chapter_drop_target.set(Some(_i));
                            },
                            ondrop: move || {
                                if let Some(dragged) = dragging_chapter_id.get() {
                                    reorder_chapter(dragged, _i);
                                }
                                dragging_chapter_id.set(None);
                                chapter_drop_target.set(None);
                            },
                            ondragend: move || {
                                dragging_chapter_id.set(None);
                                chapter_drop_target.set(None);
                            },

                            span { class: "grip",
                                {render_tabler_icon(__scope, TablerIcon::GripVertical, TablerIconStyle::Outline)}
                            }
                            span { class: "n", {format!("{}", _i + 1)} }
                            // Tier 1 (design/01-language.html#overlays): renaming edits
                            // the row in place instead of opening a dialog. A reactive
                            // node swap has to be an `if`/`match` in the macro (a
                            // closure returning a node renders as its Debug text here).
                            if editing_chapter_id.get().as_deref() == Some(cid.get().as_str()) {
                                input {
                                    id: "chapter-inline-title",
                                    class: "t crow-edit-input",
                                    value: {move || editing_chapter_title.get()},
                                    oninput: move |v: String| editing_chapter_title.set(v),
                                    // Enter/Escape/blur are wired by
                                    // `focus_inline_input` with real DOM listeners —
                                    // see its doc comment for why a raw element's
                                    // `onsubmit:`/`onblur:` rsx props can't do this.
                                }
                            } else {
                                span {
                                    class: "t",
                                    onclick: open_chapter(cid.get()),
                                    {move || ctitle.get()}
                                }
                            }
                            span { class: "m", {format_word_count_full(cwords)} }
                            span { class: "acts",
                                ActionIcon {
                                    variant: "subtle",
                                    size: "sm",
                                    onclick: move || {
                                        pending_new_chapter.set(false);
                                        editing_chapter_title.set(ctitle.get());
                                        editing_chapter_id.set(Some(cid.get()));
                                    },
                                    {render_tabler_icon(
                                        __scope,
                                        TablerIcon::Pencil,
                                        TablerIconStyle::Outline,
                                    )}
                                }
                                ActionIcon {
                                    variant: "subtle",
                                    color: "red",
                                    size: "sm",
                                    onclick: request_delete_chapter(cid.get(), ctitle.get(), cwords),
                                    {render_tabler_icon(
                                        __scope,
                                        TablerIcon::Trash,
                                        TablerIconStyle::Outline,
                                    )}
                                }
                            }
                        }
                    }

                    // Tier 1: the draft row appended by "Add chapter" — an empty
                    // title field with focus already in it, instead of a dialog
                    // asking for a title up front.
                    if pending_new_chapter.get() {
                        div {
                            key: "pending-new-chapter",
                            class: "crow",
                            span { class: "grip" }
                            span { class: "n", {format!("{}", chapter_count() + 1)} }
                            input {
                                id: "chapter-inline-title",
                                class: "t crow-edit-input",
                                placeholder: "Chapter title",
                                value: {move || editing_chapter_title.get()},
                                oninput: move |v: String| editing_chapter_title.set(v),
                                // Enter/Escape/blur come from `focus_inline_input`'s
                                // real DOM listeners — see the rename row's comment
                                // above for why rsx props can't do this here.
                            }
                            span { class: "m" }
                            span { class: "acts" }
                        }
                    }
                }
            }
        }
    }
}
