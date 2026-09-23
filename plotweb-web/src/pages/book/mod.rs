use std::cell::Cell;
use wasm_bindgen::JsCast;
use rinch::prelude::*;
use rinch_core::use_store;
use rinch_tabler_icons::{TablerIcon, TablerIconStyle, render_tabler_icon};
use plotweb_common::{
    BetaFeedback, BetaReaderLink, Book, Chapter, CommitInfo, CreateBetaLinkRequest,
    CreateBetaReplyRequest, CreateChapterRequest, CreateNoteRequest,
    Note, NoteTree, NotesResponse,
    ReorderChaptersRequest, UpdateBetaLinkRequest, UpdateBookRequest, UpdateChapterRequest,
};

use crate::api;
use crate::components::pane_header::PANE_HEADER_CSS;
use crate::components::section_header::{SectionHeader, SECTION_HEADER_CSS};
use crate::fonts;
use crate::pages::editor_utils;
use crate::router;
use crate::store::{AppStore, Route};

mod calendar_form;
mod chrome_zone;
mod css;
mod feedback;
mod flush;
mod modals;
mod notes_filter;
mod panes;
mod sidebar;
pub(crate) mod sigils;
mod spine_layout;
mod stall;
mod state;
mod time_entry;
mod ribbon;
mod timeline_layout;

use state::BookState;

thread_local! {
    /// Incremented each time `book_page()` is created so that leaked timer
    /// closures from a previous page instance can detect they are stale.
    pub(crate) static PAGE_GEN: Cell<u32> = Cell::new(0);
}

/// What the main pane shows.
#[derive(Clone, PartialEq)]
enum BookPane {
    Chapters,
    Editor(String),
    Typography,
    BetaReaders,
    Notes,
    NoteEditor(String),
    History,
    /// The book's calendar (notes card 4). Reached from the notes surface and from a
    /// note's time field, never from the tools strip: a book that keeps the default
    /// calendar need never see it.
    Calendar,
}

/// Whether the caret is in one of the find panel's own fields.
///
/// The panel's query and replace boxes are raw `<input>`s — rsx gives a raw
/// element no `onkeydown` prop (see `panes::chapters::focus_inline_input` for
/// the full list of what it does give one) — so Enter and Shift+Enter are read
/// off the window listener instead. This is what stops that read taking Enter
/// away from the prose editor: the panel only gets the key when it is the thing
/// being typed into.
///
/// Always `false` on native, where there is no document to ask. That is the same
/// documented gap the inline chapter-title input has, and it fails safe — Enter
/// goes to the editor, and the Prev/Next buttons still work.
fn find_field_focused() -> bool {
    crate::platform::document()
        .and_then(|d| d.active_element())
        .and_then(|el| el.get_attribute("id"))
        .is_some_and(|id| {
            id == panes::find::QUERY_INPUT_ID || id == panes::find::REPLACE_INPUT_ID
        })
}

#[component]
pub fn book_page(book_id: String) -> NodeHandle {
    let store = use_store::<AppStore>();
    // For a cut-over book the canonical document is the source of truth and sync is how
    // an edit reaches it, so with sync off this device is writing only to itself. The
    // save indicator must not claim otherwise — "Saved" meaning two different things
    // depending on a flag the author cannot see is how a lost session looks like a
    // finished one.
    // For a cut-over book the canonical document is the source of truth and sync
    // carries the edits; a whole-state PUT can only be a stale duplicate of what the
    // ops already said, so this device does not send one. Where the book is not cut
    // over, git is the truth and this write is the only thing that reaches it.
    let sends_body_content = move || {
        !store
            .current_book
            .get()
            .map(|b| b.cutover)
            .unwrap_or(false)
    };
    let saved_here_only = move || {
        store
            .current_book
            .get()
            .is_some_and(|b| b.cutover && !crate::sync::enabled_for_book(&b.id))
    };
    // All of book_page's ~89 signals live on one `Copy` struct (see `state.rs`) so
    // pane/modal render functions take `state: BookState` instead of a long, drifting
    // parameter list. Destructured back into plain locals immediately below so every
    // closure and rsx block downstream is unchanged from before the split.
    let state = BookState::new(&book_id);
    // Only the fields `book_page`'s own closures (not a pane/modal file) touch directly
    // are destructured here; the rest ride along on `state`, passed whole to the
    // `panes::*::render` / `modals::render` calls at the bottom of this function.
    let BookState {
        active_pane,
        chapters_collapsed,
        chapter_title,
        save_status,
        save_alert,
        editor_word_count,
        loaded_chapter_id,
        chapter_dirty,
        note_dirty,
        auto_save_timer_id,
        editor_writing,
        chapter_handle,
        note_handle,
        bid_signal,
        editing_chapter_id,
        editing_chapter_title,
        pending_new_chapter,
        chapter_title_save_timer_id,
        delete_chapter_target,
        font_settings,
        beta_links,
        beta_feedback,
        show_beta_link_modal,
        new_beta_reader_name,
        new_beta_max_chapter,
        show_feedback_sidebar,
        _beta_reply_text,
        reply_drafts,
        pending_feedback_scroll,
        editing_beta_link,
        edit_beta_reader_name,
        edit_beta_max_chapter,
        new_beta_pin_version,
        edit_beta_pinned,
        new_beta_username,
        edit_beta_username,
        beta_link_error,
        show_note_modal,
        new_note_title,
        new_note_parent_id,
        new_note_color,
        note_save_status,
        note_save_timer_id,
        history_commits,
        show_book_settings_modal,
        edit_book_title,
        edit_book_desc,
        edit_book_cover,
        ..
    } = state;

    // Keep the editors' color scheme in sync with the app theme. `set_dark_mode` is a
    // no-op before mount; the load paths (chapter/note open) also set it, so the
    // initial state is applied once an editor is actually shown.
    __scope.create_effect(move || {
        let dark = store.dark_mode.get();
        chapter_handle.get().set_dark_mode(dark);
        note_handle.get().set_dark_mode(dark);
    });

    // ── Keep the spellchecker's word lists current ───────────────
    // The book's entity notes are its proper nouns; feeding their titles to the
    // speller is what stops a character's name being underlined on every page.
    // Re-run whenever the notes change (a note created, renamed, or given the
    // entity facet), because each of those changes the set of names.
    //
    // All three effects repaint both prose surfaces: one speller serves them, a
    // note's body is prose too, and a word added from the chapter editor should
    // stop being underlined in the note editor as well.
    __scope.create_effect(move || {
        let words = crate::spell::entity_words_from_notes(&store.notes.get());
        crate::spell::plugin::set_entity_words(&chapter_handle.get(), words);
        crate::spell::plugin::force_redraw(&note_handle.get());
    });

    // The account's own words. `local_dictionary` hands them to the plugin as they
    // arrive (from local storage, then the server, then an "Add to dictionary")
    // and then sets this signal; this is the half that makes the editor redraw.
    __scope.create_effect(move || {
        let _ = store.user_dictionary.get();
        crate::spell::plugin::user_words_changed(&chapter_handle.get());
        crate::spell::plugin::force_redraw(&note_handle.get());
    });

    // The switch. Held in the store so the Typography pane can flip it from a
    // pane that knows nothing about editors.
    __scope.create_effect(move || {
        crate::spell::plugin::set_enabled(&chapter_handle.get(), store.spellcheck_enabled.get());
        crate::spell::plugin::force_redraw(&note_handle.get());
    });

    // Whether the chapter currently open in the editor has any feedback — the
    // mobile hamburger bar's message-circle toggle only makes sense to show when
    // there's something for it to open. (The desktop editor topbar has its own
    // copy of this same check — see `current_chapter_feedback` in
    // `panes::editor::render` — because that one also needs the filtered list
    // itself, not just whether it's non-empty.)
    let current_chapter_has_feedback = move || {
        match active_pane.get() {
            BookPane::Editor(ref cid) => beta_feedback.get().iter().any(|f| f.chapter_id == *cid),
            _ => false,
        }
    };

    PAGE_GEN.with(|g| g.set(g.get().wrapping_add(1)));
    let page_gen = PAGE_GEN.with(|g| g.get());

    // Return early unless this particular mount of the book page is still live.
    //
    // Deferred work — debounced saves, the writing-idle timer, in-flight fetch
    // callbacks — can outlive the scope that owns the signals it touches, and
    // `Signal::get()` *panics* on a freed slot (`set`/`update` merely warn). Two
    // different things can make a callback stale, and each guard alone lets the
    // other through:
    //
    // * `PAGE_GEN` catches a *remount of this same page* — opening another book
    //   bumps the counter while the old timers are still armed. It cannot catch
    //   navigation *away*, because leaving for the reader or the dashboard never
    //   re-runs this setup and so never bumps it.
    // * `is_alive` catches *scope disposal*, which is exactly the cross-page
    //   case: the signals are freed even though the generation still matches.
    //
    // `active_pane` is the canary — every signal here belongs to the same scope,
    // so if it is gone they all are.
    macro_rules! bail_if_stale {
        () => {
            if PAGE_GEN.with(|g| g.get()) != page_gen || !active_pane.is_alive() {
                return;
            }
        };
    }

    // Trigger font catalog fetch
    fonts::fetch_font_catalog();

    // Fetch book and chapters
    let bid = book_id.clone();
    api::get::<Book>(&format!("/api/books/{}", bid), move |book_result| {
        let book_opt = match book_result {
            Ok(book) => {
                let fs = book.font_settings.clone().unwrap_or_default();
                fonts::load_book_fonts(&fs);
                font_settings.set(fs);
                // Before the signal that renders the page, not after: the gate is read
                // from plain state rather than a signal (as the flag always was), so
                // the render triggered by `current_book` has to see the answer already
                // recorded. It also has to be recorded before an editor can attach — a
                // body registers for sync only if its book's gate is open by then.
                crate::sync::note_cutover(&book.id, book.cutover, store);
                store.current_book.set(Some(book.clone()));
                Some(book)
            }
            Err(_) => None,
        };
        api::get::<Vec<Chapter>>(&format!("/api/books/{}/chapters", bid), move |ch_result| {
            // Whether this was a real answer or a failure. A failed fetch used to
            // become an empty list, and that empty list then *seeded the local
            // `book:` document* — which is authoritative, so the book was left
            // genuinely empty until something happened to refill it. The store can
            // take an empty list harmlessly; the document cannot.
            let loaded = ch_result.is_ok();
            let chapters = ch_result.unwrap_or_default();
            store.chapters.set(chapters.clone());

            // Local-first book structure (Phase 2 · Slice 1 · deliverable 2): also
            // fetch the notes, then seed-or-load the hand-projected `book:` Automerge
            // doc and project it back over the REST signals (chapter order/titles +
            // notes tree). On divergence the local doc wins. Additive — the REST
            // loads/PUTs stay intact (dual-write).
            let bid_book = bid.clone();
            api::get::<NotesResponse>(&format!("/api/books/{}/notes", bid), move |notes_result| {
                let (notes, tree) = match notes_result {
                    Ok(resp) => (resp.notes, resp.tree),
                    Err(_) => (
                        Vec::new(),
                        NoteTree {
                            root_order: Vec::new(),
                            children: Default::default(),
                            collapsed: Vec::new(),
                        },
                    ),
                };
                store.notes.set(notes.clone());
                store.note_tree.set(Some(tree.clone()));
                if let Some(book) = book_opt
                    && loaded
                {
                    crate::local_book::enter(bid_book, book, chapters, notes, tree, store);
                }
            });

            api::get::<Vec<BetaReaderLink>>(&format!("/api/books/{}/beta-links", bid), move |links_result| {
                if let Ok(links) = links_result {
                    beta_links.set(links);
                }
                api::get::<Vec<BetaFeedback>>(&format!("/api/books/{}/feedback", bid), move |fb_result| {
                    if let Ok(fb) = fb_result {
                        beta_feedback.set(fb);
                    }
                });
            });
        });
    });

    // Connect WebSocket for real-time feedback
    {
        let bid = book_id.clone();
        let ws_url = crate::ws::ws_url(&format!("/api/books/{}/feedback/ws", bid));
        crate::ws::connect_feedback_ws(&ws_url, move |msg| {
            match msg {
                crate::ws::WsMessage::NewFeedback(fb) => {
                    beta_feedback.update(|list| {
                        if !list.iter().any(|f| f.id == fb.id) {
                            list.insert(0, fb);
                        }
                    });
                }
                crate::ws::WsMessage::NewReply { feedback_id, reply } => {
                    beta_feedback.update(|list| {
                        if let Some(fb) = list.iter_mut().find(|f| f.id == feedback_id) {
                            if !fb.replies.iter().any(|r| r.id == reply.id) {
                                fb.replies.push(reply);
                            }
                        }
                    });
                }
                crate::ws::WsMessage::FeedbackResolved { feedback_id, resolved } => {
                    beta_feedback.update(|list| {
                        if let Some(fb) = list.iter_mut().find(|f| f.id == feedback_id) {
                            fb.resolved = resolved;
                        }
                    });
                }
                crate::ws::WsMessage::FeedbackDeleted { feedback_id } => {
                    beta_feedback.update(|list| list.retain(|f| f.id != feedback_id));
                }
            }
        });
    }

    // ── Save helper for the editor ──────────────────────────────
    let save_content = move |chapter_id_to_save: String| {
        // Bail if this page instance is no longer current (user navigated away and back)
        bail_if_stale!();
        // Only save if this chapter's content is the one currently loaded in the
        // editor model — otherwise the model still holds a previous chapter and we'd
        // overwrite the wrong one.
        if loaded_chapter_id.get().as_deref() != Some(chapter_id_to_save.as_str()) {
            return;
        }
        // The edits are about to be written: anything typed from here re-marks the
        // surface through `schedule_chapter_autosave`. Without clearing, the flag stays
        // set for the life of the page and the next switch re-saves content nobody
        // touched — which is the very write this guard exists to prevent.
        chapter_dirty.set(false);
        // Serialize the durable save shape (DocNode JSON) straight from the model.
        let Some(content) = editor_utils::editor_content_json(&chapter_handle.get()) else {
            return;
        };
        let bid = bid_signal.get();
        // Sync is the only writer of this body in a cut-over book, so if it is stalled
        // the PUT below would come back durable having carried nothing and flip the
        // indicator to "Saved". Re-mark the surface dirty on the way out: it was
        // cleared above for a write that is now refused, and leaving it clean would
        // hide this text from the later flush that runs once the unsupported content
        // is gone — the same rule `flush.rs` calls "a refused write never marks the
        // surface clean".
        if stall::veto_if_stalled(
            crate::local_store::BodyKind::Chapter,
            &chapter_handle.get(),
            &bid,
            save_status,
        ) {
            chapter_dirty.set(true);
            return;
        }
        save_status.set("saving");
        // Declare that sync is carrying this body rather than withholding the write.
        // Two writers on one body is how deleted text comes back — but *which* writer
        // should stand down depends on whether the book is cut over, and only the
        // server knows that. Withholding it here regardless is how an edit reaches the
        // canonical store, never reaches git, and vanishes from a book whose reads
        // still come from git.
        panes::chapters::save_chapter_body(
            format!("/api/books/{}/chapters/{}", bid, chapter_id_to_save),
            content,
            !sends_body_content(),
            save_status,
            save_alert,
        );
    };

    // Schedule a debounced chapter autosave (3s). Called on editor edits and
    // after toolbar formatting actions (via the toolbar's `on_edit`). The model-first
    // editor emits no DOM `input` event (no `contenteditable`), so edits are detected
    // via `EditorHandle::on_change` below.
    let schedule_chapter_autosave = move || {
        chapter_dirty.set(true);
        if !matches!(active_pane.get(), BookPane::Editor(_)) {
            return;
        }
        save_status.set("unsaved");
        // Asked here, per edit, because this is the one function every chapter edit
        // reaches — the `on_change` hook below *and* the toolbar's `on_edit` (which is
        // how a blockquote arrives). Doing it now rather than at the end of the 3s
        // debounce means the footer stops claiming anything the moment the document
        // goes out of scope, instead of three seconds later.
        stall::veto_if_stalled(
            crate::local_store::BodyKind::Chapter,
            &chapter_handle.get(),
            &bid_signal.get(),
            save_status,
        );
        if let Some(h) = auto_save_timer_id.get() {
            rinch_core::clear_timeout(h);
        }
        let captured_cid = match active_pane.get() {
            BookPane::Editor(cid) => cid,
            _ => return,
        };
        auto_save_timer_id.set(Some(rinch_core::reactive::unowned(|| rinch_core::set_timeout(3000, move || {
            bail_if_stale!();
            // Recompute the word count from the model (debounced, not per-keystroke).
            editor_word_count.set(editor_utils::editor_word_count(&chapter_handle.get()));
            if let BookPane::Editor(ref current_cid) = active_pane.get() {
                if *current_cid == captured_cid {
                    save_content(captured_cid.clone());
                }
            }
        }))));
    };

    // ── Chrome collapse while typing ─────────────────────────────
    // Sidebar, editor header/toolbar and feedback rail fade (and on desktop
    // the sidebar and feedback rail also collapse to zero width — CSS on
    // `editor_writing`, see EDITOR_CSS / book/css.rs) while the author is
    // actively typing, and return only on Escape or a pointer move into a
    // reveal zone (`chrome_zone::in_reveal_zone` — a mousemove over the prose
    // itself is a no-op, see that module) — no idle timer brings it back on
    // its own, so the chrome stays out of the way for as long as the author
    // keeps their eyes on the page. Driven off real edits (`EditorHandle::
    // on_change`), not DOM `keydown`: `#editor-main` is rinch's own
    // editor-view, not a `contenteditable`, so there is no native
    // `input`/`keydown` to hang this on at the DOM level the way the
    // reference mockup does.
    let stop_writing = move || {
        editor_writing.set(false);
    };
    let start_writing = move || {
        // `store.chrome_fade_enabled` is the per-device off switch (Typography
        // pane, "Fade chrome while writing") — off means typing never collapses
        // the chrome in the first place, so there's nothing for a reveal zone or
        // Escape to bring back.
        if store.chrome_fade_enabled.get() {
            editor_writing.set(true);
        }
    };

    // ── Set up Ctrl+S, Escape-to-return, and auto-save (once, on mount) ──
    if let Some(window) = crate::platform::window() {
        // Ctrl+S — immediate save of the current chapter. Escape — bring the
        // collapsed chrome back immediately.
        // Both listeners below are `forget()`-ed, so they stay bound to `window`
        // for the life of the tab — including after this page's scope is gone.
        // Every signal they touch is freed at that point and `Signal::get()`
        // panics on a freed slot, so each one has to check first. The mousemove
        // handler is the sharp edge: it runs on *any* pointer movement, so a
        // single unguarded read takes the whole WASM instance down the moment
        // the reader moves their mouse on the next page.
        let keydown = wasm_bindgen::closure::Closure::wrap(Box::new(move |event: web_sys::KeyboardEvent| {
            bail_if_stale!();
            let accel = event.ctrl_key() || event.meta_key();
            if accel && event.key() == "s" {
                event.prevent_default();
                if let BookPane::Editor(ref cid) = active_pane.get() {
                    save_content(cid.clone());
                }
            } else if accel && event.key().eq_ignore_ascii_case("f") {
                // Ctrl+F searches the open chapter, Ctrl+Shift+F the whole book
                // — the same panel either way, with its scope switch preset.
                // `eq_ignore_ascii_case` because Shift makes `key()` report "F".
                // `prevent_default` is what keeps the browser's own find bar
                // out of the way; without it two find UIs open at once and only
                // one of them knows what a chapter is.
                event.prevent_default();
                panes::find::open(state, store, event.shift_key());
            } else if panes::find::is_open(state)
                && event.key() == "Enter"
                && find_field_focused()
            {
                // Enter / Shift+Enter cycle the hits, but only from inside the
                // panel's own fields: a raw `input` gets no `onkeydown` prop
                // from rsx (see `panes::chapters::focus_inline_input` on why),
                // and an unscoped Enter here would fire while the author is
                // typing a paragraph.
                event.prevent_default();
                panes::find::step(state, store, if event.shift_key() { -1 } else { 1 });
            } else if event.key() == "Escape" {
                // Escape belongs to the find panel while it is open — closing it
                // clears the highlights, which is the thing the author wants
                // back first. The chrome-collapse escape still runs underneath.
                if panes::find::is_open(state) {
                    panes::find::close(state);
                }
                stop_writing();
            }
        }) as Box<dyn FnMut(_)>);
        window.add_event_listener_with_callback("keydown", keydown.as_ref().unchecked_ref()).ok();
        keydown.forget();

        // Pointer move — brings the chrome back, but only when it's a reach for
        // the chrome itself (see `chrome_zone::in_reveal_zone`): a strip down the
        // left edge (the sidebar), a band across the top (topbar/toolbar/"Notes
        // here"), a strip down the right edge (the feedback rail). A move that
        // stays over the prose in the middle of the screen — which is most of
        // them, while reading back what was just typed — is a no-op. Only acts
        // while collapsed (the `editor_writing.get()` check), so a mousemove
        // while the chrome is already showing is a no-op rather than a signal
        // write on every pixel of cursor travel. A layout shift from the
        // collapse itself never fires this — `mousemove` only follows real
        // pointer motion.
        let mousemove = wasm_bindgen::closure::Closure::wrap(Box::new(move |event: web_sys::MouseEvent| {
            bail_if_stale!();
            if !editor_writing.get() {
                return;
            }
            // `inner_width` is read fresh on every move rather than cached: a
            // window resize/rotate between two moves must not leave the zone
            // computed against a stale width.
            let Some(viewport_w) = crate::platform::window()
                .and_then(|w| w.inner_width().ok())
                .and_then(|v| v.as_f64())
            else {
                return;
            };
            if chrome_zone::in_reveal_zone(event.client_x() as f64, event.client_y() as f64, viewport_w) {
                stop_writing();
            }
        }) as Box<dyn FnMut(_)>);
        window.add_event_listener_with_callback("mousemove", mousemove.as_ref().unchecked_ref()).ok();
        mousemove.forget();
    }

    // Auto-save on edit, and mark the editor "writing" so the chrome collapses.
    // Cross-platform: the editor notifies us after any local edit (typing/paste/
    // IME/commands), and deliberately not for selection-only changes or for
    // `load_doc`, so opening a chapter can't re-trigger a save or a collapse.
    // Registered outside the `window` guard above so the autosave half runs on
    // native too (the collapse itself is a web-only visual affordance — native
    // has no mouse-leaves-then-returns chrome to hide).
    chapter_handle.get().on_change(move || {
        bail_if_stale!();
        if matches!(active_pane.get(), BookPane::Editor(_)) {
            schedule_chapter_autosave();
            start_writing();
        }
        // An open find panel is a question about the document, so an edit is a
        // new answer: typing (or undoing a replace) has to move the counts.
        // Debounced, like the autosave beside it.
        //
        // Not while a whole-book replace is walking, though — it is switching
        // chapters underneath this, and its own final re-search is the one that
        // should have the last word. The empty transaction the highlighter uses
        // to repaint changes no document, so it never reaches here and there is
        // no loop to break.
        if panes::find::is_open(state) && state.find_busy.get().is_none() {
            panes::find::schedule_search(state, store);
        }
    });

    // ── Chapter actions ─────────────────────────────────────────
    // Tier 1 (design/01-language.html#overlays): "Add chapter" appends an
    // empty row with focus already in its title field rather than opening a
    // dialog to ask for one. `title` comes off `editing_chapter_title`, the
    // same draft signal the inline-rename row uses — see `panes::chapters`.
    let add_chapter = move |title: String| {
        let title = title.trim().to_string();
        pending_new_chapter.set(false);
        if title.is_empty() {
            return;
        }
        let bid = bid_signal.get();
        let bid_sync = bid.clone();
        let req = CreateChapterRequest { title };
        api::post::<_, Chapter>(
            &format!("/api/books/{}/chapters", bid),
            &req,
            move |result| {
                if let Ok(chapter) = result {
                    // The snapshot the projection reads, beside the REST call that
                    // changed which chapters exist. See `local_book::rest_chapters`.
                    crate::local_book::rest_chapters(&bid_sync, |ch| ch.push(chapter.clone()));
                    store.chapters.update(|ch| ch.push(chapter));
                    crate::local_book::sync_chapters(&bid_sync, &store.chapters.get());
                }
            },
        );
    };

    // ── Book settings save ──────────────────────────────────────
    let save_book_settings = move || {
        let title = edit_book_title.get();
        if title.trim().is_empty() {
            return;
        }
        show_book_settings_modal.set(false);
        let bid = bid_signal.get();
        let desc = edit_book_desc.get();
        let cover = edit_book_cover.get();
        let req = UpdateBookRequest {
            title: Some(title.clone()),
            description: Some(desc.clone()),
            font_settings: None,
            cover_image: Some(cover.clone()),
            calendar: None,
            span_rule: None,
        };
        api::put::<_, serde_json::Value>(
            &format!("/api/books/{}", bid),
            &req,
            move |result| {
                if result.is_ok() {
                    // Dual-write: update the dashboard's cached title/cover for this
                    // book in the local `user:` doc (updated_at preserved — the PUT
                    // returns no body, so there is no fresh server timestamp).
                    if let Some(user) = store.current_user.get() {
                        crate::local_user::update_book(&user.id, &bid, &title, cover.as_deref());
                    }
                    store.current_book.update(|book| {
                        if let Some(b) = book {
                            b.title = title;
                            b.description = desc;
                            b.cover_image = cover;
                        }
                    });
                }
            },
        );
    };

    // ── Chapter rename save (Tier 1 — inline row, no overlay) ─────
    let save_rename_chapter = move |cid: String, title: String| {
        editing_chapter_id.set(None);
        let title = title.trim().to_string();
        if title.is_empty() {
            return;
        }
        let bid = bid_signal.get();
        let bid_sync = bid.clone();
        let req = UpdateChapterRequest {
            title: Some(title.clone()),
            content: None,
        };
        api::put::<_, serde_json::Value>(
            &format!("/api/books/{}/chapters/{}", bid, cid),
            &req,
            move |result| {
                if result.is_ok() {
                    store.chapters.update(|chapters| {
                        if let Some(ch) = chapters.iter_mut().find(|c| c.id == cid) {
                            ch.title = title.clone();
                        }
                    });
                    crate::local_book::sync_chapters(&bid_sync, &store.chapters.get());
                    // Update editor title if this chapter is currently open
                    if let BookPane::Editor(ref open_cid) = active_pane.get() {
                        if *open_cid == cid {
                            chapter_title.set(title);
                        }
                    }
                }
            },
        );
    };

    // ── Inline chapter title save (debounced) ─────────────────────
    let save_chapter_title_inline = move |new_title: String| {
        chapter_title.set(new_title.clone());
        if let Some(h) = chapter_title_save_timer_id.get() {
            rinch_core::clear_timeout(h);
        }
        // Capture context now, validate when timer fires
        let captured_bid = bid_signal.get();
        let captured_cid = if let BookPane::Editor(ref cid) = active_pane.get() {
            cid.clone()
        } else {
            return;
        };
        chapter_title_save_timer_id.set(Some(rinch_core::reactive::unowned(|| rinch_core::set_timeout(1000, move || {
            // Bail if this page instance is stale (user navigated away and back)
            bail_if_stale!();
            if let BookPane::Editor(ref current_cid) = active_pane.get() {
                if *current_cid != captured_cid { return; }
            } else { return; }
            let cid = captured_cid;
            let title = new_title.clone();
            let bid_sync = captured_bid.clone();
            let req = UpdateChapterRequest {
                title: Some(title.clone()),
                content: None,
            };
            api::put::<_, serde_json::Value>(
                &format!("/api/books/{}/chapters/{}", captured_bid, cid),
                &req,
                move |result| {
                    if result.is_ok() {
                        store.chapters.update(|chapters| {
                            if let Some(ch) = chapters.iter_mut().find(|c| c.id == cid) {
                                ch.title = title;
                            }
                        });
                        crate::local_book::sync_chapters(&bid_sync, &store.chapters.get());
                    }
                },
            );
        }))));
    };

    // Drag-to-reorder: move `dragged_id` to sit at `target_index` in the list
    // (post-removal indexing, i.e. the index it should occupy once it's pulled
    // out of its old slot). Same local-first sequencing the old arrow-key move
    // used — sync the local doc first, then hold the DOM-visible move and the
    // REST PUT until that write has settled — so a drag can't race a reload.
    let reorder_chapter = move |dragged_id: String, target_index: usize| {
        let mut chapters = store.chapters.get();
        let Some(from) = chapters.iter().position(|c| c.id == dragged_id) else {
            return;
        };
        let target_index = target_index.min(chapters.len().saturating_sub(1));
        if from == target_index {
            return;
        }
        let chapter = chapters.remove(from);
        chapters.insert(target_index, chapter);

        let bid = bid_signal.get();
        crate::local_book::sync_chapters(&bid, &chapters);

        let ids: Vec<String> = chapters.iter().map(|c| c.id.clone()).collect();
        let bid_put = bid.clone();
        let moved = dragged_id.clone();
        crate::local_book::on_settled(&bid, move || {
            // Re-apply the move to whatever the list holds *now* rather than
            // writing back the copy captured before the wait — a projection
            // that lands during the wait must not be clobbered by a stale one.
            store.chapters.update(|chapters| {
                let Some(from) = chapters.iter().position(|c| c.id == moved) else {
                    return;
                };
                let target_index = target_index.min(chapters.len().saturating_sub(1));
                if from != target_index {
                    let chapter = chapters.remove(from);
                    chapters.insert(target_index, chapter);
                }
            });
            let req = ReorderChaptersRequest { chapter_ids: ids };
            api::put::<_, serde_json::Value>(&format!("/api/books/{}/chapters/reorder", bid_put), &req, move |_result| {});
        });
    };

    // Tier 2 (design/01-language.html#overlays): deleting a chapter had no
    // confirmation at all before this stage — one misclick removed a chapter
    // outright. `request_delete_chapter` opens the confirm `Dialog`;
    // `confirm_delete_chapter` is what its "Delete" button actually runs,
    // mirroring the dashboard's book-delete confirm (`pages/dashboard.rs`).
    let request_delete_chapter = move |chapter_id: String, title: String, word_count: u64| {
        move || {
            delete_chapter_target.set(Some((chapter_id.clone(), title.clone(), word_count)));
        }
    };

    let confirm_delete_chapter = move || {
        let Some((cid, _title, _wc)) = delete_chapter_target.get() else {
            return;
        };
        delete_chapter_target.set(None);
        let bid = bid_signal.get();
        let bid_sync = bid.clone();
        api::delete_req::<serde_json::Value>(
            &format!("/api/books/{}/chapters/{}", bid, cid),
            move |result| {
                if result.is_ok() {
                    if active_pane.get() == BookPane::Editor(cid.clone()) {
                        active_pane.set(BookPane::Chapters);
                    }
                    let cid_visible = cid.clone();
                    let mut chapters = store.chapters.get();
                    chapters.retain(|c| c.id != cid);
                    // Before the document write, so no projection in between can
                    // find the chapter still listed here and put it back.
                    crate::local_book::rest_chapters(&bid_sync, |ch| {
                        ch.retain(|c| c.id != cid)
                    });
                    crate::local_book::sync_chapters(&bid_sync, &chapters);
                    // Same reload race as `move_chapter`: don't flip the
                    // DOM-visible list until the local-doc write above lands.
                    crate::local_book::on_settled(&bid_sync, move || {
                        // Removal by id, for the same reason as `move_chapter`:
                        // a pre-wait snapshot would clobber anything that landed
                        // during the wait.
                        store.chapters.update(|ch| ch.retain(|c| c.id != cid_visible));
                    });
                }
            },
        );
    };

    // Synchronously flush whichever prose surface is active before leaving it — see
    // `flush.rs` for why every exit must do this and what each guard is for. Was
    // `flush_editor_if_active` (chapter-only); the note editor had no equivalent, so
    // every exit but its back arrow discarded the last ~800ms of a note.
    let flush_pending_edits = move || flush::flush_pending_edits(state, store);

    // Leaving the book page entirely has to flush for the same reason switching
    // panes does — these three were the only exits that didn't. An edit made in
    // the last ~3s was silently discarded, and the still-armed debounce timer
    // went on to read signals belonging to the disposed page scope.
    let go_dashboard = move || {
        flush_pending_edits();
        router::navigate(Route::Dashboard);
    };

    let logout = move || {
        flush_pending_edits();
        api::post::<_, serde_json::Value>("/api/auth/logout", &serde_json::json!({}), move |_result| {
            store.current_user.set(None);
            router::navigate(Route::Login);
        });
    };

    let toggle_dark = move || {
        store.dark_mode.update(|d| *d = !*d);
    };

    let toggle_sidebar = move || {
        store.sidebar_open.update(|o| *o = !*o);
    };

    // Factory closures capture only Copy types (Signals) so they are Copy themselves.
    // This lets the rsx macro use them in multiple for-loops without move issues.
    let open_chapter = move |chapter_id: String| {
        move || {
            // Opening a chapter is an exit from whatever was open before — including a
            // note. `do_switch_chapter` saves the *chapter* it is leaving but knows
            // nothing about the note editor, so without this a note edit made in the
            // last ~800ms died on the way to a chapter. When a chapter is what we're
            // leaving this is idempotent: it clears `chapter_dirty`, so the switch's own
            // save-on-leave then finds nothing to do and only one PUT goes out.
            flush_pending_edits();
            panes::chapters::do_switch_chapter(active_pane, save_alert, auto_save_timer_id, chapter_title_save_timer_id, save_status, editor_word_count, loaded_chapter_id, chapter_dirty, chapter_handle, chapter_title, store, &bid_signal.get(), &chapter_id)
        }
    };

    // Navigate to a feedback item: switch to the chapter editor, open the feedback
    // sidebar, and scroll to the quoted text after content loads.
    let navigate_to_feedback = move |chapter_id: String, selected_text: String, context_block: String| {
        move || {
            // If already viewing this chapter, just scroll directly
            if let BookPane::Editor(ref cid) = active_pane.get() {
                if *cid == chapter_id {
                    panes::editor::scroll_to_text_in_editor(&selected_text, &context_block);
                    show_feedback_sidebar.set(true);
                    return;
                }
            }
            // Otherwise switch chapter and scroll after load — same exit, same flush as
            // `open_chapter` above.
            flush_pending_edits();
            pending_feedback_scroll.set(Some((selected_text.clone(), context_block.clone())));
            show_feedback_sidebar.set(true);
            panes::chapters::do_switch_chapter_inner(
                active_pane, save_alert, auto_save_timer_id, chapter_title_save_timer_id, save_status, editor_word_count, loaded_chapter_id, chapter_dirty, chapter_handle,
                chapter_title, store, &bid_signal.get(), &chapter_id,
                Some(pending_feedback_scroll),
            );
        }
    };

    // ── Beta reader actions ────────────────────────────────────────
    let add_beta_link = move || {
        let name = new_beta_reader_name.get();
        if name.trim().is_empty() { return; }
        beta_link_error.set(None);
        let bid = bid_signal.get();
        let max_ch = new_beta_max_chapter.get();
        let pinned = if new_beta_pin_version.get() { Some("HEAD".to_string()) } else { None };
        let username_val = new_beta_username.get();
        let username = if username_val.trim().is_empty() { None } else { Some(username_val) };
        let req = CreateBetaLinkRequest { reader_name: name, max_chapter_index: max_ch, pinned_commit: pinned, username };
        api::post::<_, BetaReaderLink>(
            &format!("/api/books/{}/beta-links", bid), &req,
            move |result| {
                match result {
                    Ok(link) => {
                        show_beta_link_modal.set(false);
                        beta_links.update(|l| l.insert(0, link));
                    }
                    Err(e) => {
                        beta_link_error.set(Some(format!("{}", e)));
                    }
                }
            },
        );
    };

    let delete_beta_link = move |link_id: String| {
        move || {
            let bid = bid_signal.get();
            let lid = link_id.clone();
            api::delete_req::<serde_json::Value>(
                &format!("/api/books/{}/beta-links/{}", bid, lid),
                move |result| {
                    if result.is_ok() {
                        beta_links.update(|l| l.retain(|x| x.id != lid));
                    }
                },
            );
        }
    };

    let update_beta_link = move || {
        let link = match editing_beta_link.get() {
            Some(l) => l,
            None => return,
        };
        let name = edit_beta_reader_name.get();
        if name.trim().is_empty() { return; }
        let max_ch = edit_beta_max_chapter.get();
        let is_pinned = edit_beta_pinned.get();
        let was_pinned = link.pinned_commit.is_some();
        let pinned_commit = if is_pinned && !was_pinned {
            Some(Some("HEAD".to_string()))
        } else if !is_pinned && was_pinned {
            Some(None)
        } else {
            None
        };
        let new_username = edit_beta_username.get();
        let old_username = link.username.clone().unwrap_or_default();
        let username = if new_username.trim() != old_username.trim() {
            if new_username.trim().is_empty() {
                Some(None) // Detach
            } else {
                Some(Some(new_username)) // Attach new user
            }
        } else {
            None // No change
        };
        let bid = bid_signal.get();
        let lid = link.id.clone();
        beta_link_error.set(None);
        let req = UpdateBetaLinkRequest {
            reader_name: Some(name.clone()),
            max_chapter_index: Some(max_ch),
            active: None,
            pinned_commit,
            username,
        };
        let bid_refresh = bid.clone();
        api::put::<_, serde_json::Value>(
            &format!("/api/books/{}/beta-links/{}", bid, lid), &req,
            move |result| {
                match result {
                    Ok(_) => {
                        editing_beta_link.set(None);
                        // Refresh links to get resolved data
                        api::get::<Vec<BetaReaderLink>>(
                            &format!("/api/books/{}/beta-links", bid_refresh),
                            move |links_result| {
                                if let Ok(links) = links_result {
                                    beta_links.set(links);
                                }
                            },
                        );
                    }
                    Err(e) => {
                        beta_link_error.set(Some(format!("{}", e)));
                    }
                }
            },
        );
    };

    let toggle_beta_link_active = move |link_id: String, currently_active: bool| {
        move || {
            let bid = bid_signal.get();
            let lid = link_id.clone();
            let req = UpdateBetaLinkRequest {
                reader_name: None,
                max_chapter_index: None,
                active: Some(!currently_active),
                pinned_commit: None,
                username: None,
            };
            api::put::<_, serde_json::Value>(
                &format!("/api/books/{}/beta-links/{}", bid, lid), &req,
                move |result| {
                    if result.is_ok() {
                        beta_links.update(|l| {
                            if let Some(link) = l.iter_mut().find(|x| x.id == lid) {
                                link.active = !currently_active;
                            }
                        });
                    }
                },
            );
        }
    };

    let resolve_feedback = move |feedback_id: String| {
        move || {
            let bid = bid_signal.get();
            let fid = feedback_id.clone();
            api::put::<_, serde_json::Value>(
                &format!("/api/books/{}/feedback/{}/resolve", bid, fid),
                &serde_json::json!({}),
                move |result| {
                    if result.is_ok() {
                        beta_feedback.update(|fb| {
                            if let Some(f) = fb.iter_mut().find(|x| x.id == fid) {
                                f.resolved = !f.resolved;
                            }
                        });
                    }
                },
            );
        }
    };

    let delete_feedback = move |feedback_id: String| {
        move || {
            let bid = bid_signal.get();
            let fid = feedback_id.clone();
            api::delete_req::<serde_json::Value>(
                &format!("/api/books/{}/feedback/{}", bid, fid),
                move |result| {
                    if result.is_ok() {
                        beta_feedback.update(|fb| fb.retain(|x| x.id != fid));
                    }
                },
            );
        }
    };

    let author_reply = move |feedback_id: String| {
        move || {
            let content = reply_drafts
                .get()
                .get(&feedback_id)
                .cloned()
                .unwrap_or_default();
            if content.trim().is_empty() { return; }
            let clear_key = feedback_id.clone();
            reply_drafts.update(|m| { m.remove(&clear_key); });
            let bid = bid_signal.get();
            let fid = feedback_id.clone();
            let req = CreateBetaReplyRequest { content };
            let bid_refresh = bid.clone();
            api::post::<_, serde_json::Value>(
                &format!("/api/books/{}/feedback/{}/replies", bid, fid), &req,
                move |result| {
                    if result.is_ok() {
                        // Refresh feedback
                        api::get::<Vec<BetaFeedback>>(&format!("/api/books/{}/feedback", bid_refresh), move |fb_result| {
                            if let Ok(fb) = fb_result {
                                beta_feedback.set(fb);
                            }
                        });
                    }
                },
            );
        }
    };

    let copy_beta_link = move |token: String| {
        move || {
            let origin = crate::platform::window()
                .and_then(|w| w.location().origin().ok())
                .unwrap_or_default();
            let url = format!("{}/read/{}", origin, token);
            if let Some(window) = crate::platform::window() {
                if let Ok(clipboard) = js_sys::Reflect::get(&window.navigator(), &"clipboard".into()) {
                    let clipboard: web_sys::Clipboard = clipboard.unchecked_into();
                    let _ = clipboard.write_text(&url);
                }
            }
        }
    };


    // ── Sidebar click handlers ──────────────────────────────────
    let _open_chapters_pane = move || {
        flush_pending_edits();
        active_pane.set(BookPane::Chapters);
        store.sidebar_open.set(false);
    };

    let open_typography_pane = move || {
        flush_pending_edits();
        active_pane.set(BookPane::Typography);
        store.sidebar_open.set(false);
    };

    let open_beta_pane = move || {
        flush_pending_edits();
        active_pane.set(BookPane::BetaReaders);
        store.sidebar_open.set(false);
    };

    let open_history_pane = move || {
        flush_pending_edits();
        active_pane.set(BookPane::History);
        store.sidebar_open.set(false);
        let bid = bid_signal.get();
        api::get::<Vec<CommitInfo>>(
            &format!("/api/books/{}/history", bid),
            move |result| {
                if let Ok(commits) = result {
                    history_commits.set(commits);
                }
            },
        );
    };

    // The sidebar's "Manuscript" header opens the chapters pane — the manuscript
    // page (chapter list, counts, import/export) — the same way the "Notes" header
    // opens the notes surface. It used to be a bare label: the caret beside it
    // collapsed the list and the "+" added a chapter, but the word itself did
    // nothing, and there was no other way back to that pane from an open chapter.
    let open_chapters_pane = move || {
        flush_pending_edits();
        active_pane.set(BookPane::Chapters);
        store.sidebar_open.set(false);
    };

    let open_notes_pane = move || {
        flush_pending_edits();
        active_pane.set(BookPane::Notes);
        store.sidebar_open.set(false);
        // Fetch notes
        let bid = bid_signal.get();
        let bid_rest = bid.clone();
        api::get::<NotesResponse>(&format!("/api/books/{}/notes", bid), move |result| {
            if let Ok(resp) = result {
                // What REST said, kept apart from the projection's output — the
                // fallback for any facet the local document has not heard of yet.
                crate::local_book::rest_notes(&bid_rest, &resp.notes);
                store.notes.set(resp.notes);
                store.note_tree.set(Some(resp.tree));
                // Read path: re-project the local `book:` doc's structure over the
                // REST notes so the local tree/titles/colors win on divergence.
                crate::local_book::project_notes(store);
            }
        });
    };

    let add_note = move || {
        let title = new_note_title.get().trim().to_string();
        if title.is_empty() {
            return;
        }
        let parent_id = new_note_parent_id.get();
        let color = new_note_color.get();
        let bid = bid_signal.get();
        show_note_modal.set(false);
        // Creating a note from inside an open note leaves that note: flush before the
        // pane moves, or the text typed just before reaching for "+" is discarded.
        flush_pending_edits();
        // Switch to Notes pane so the new note is visible
        active_pane.set(BookPane::Notes);
        let req = CreateNoteRequest {
            title,
            parent_id: parent_id.clone(),
            color: Some(color),
        };
        let bid_refresh = bid.clone();
        let bid_sync = bid.clone();
        api::post::<_, Note>(&format!("/api/books/{}/notes", bid), &req, move |result| {
            match result {
                Ok(_note) => {
                    // Refresh tree
                    api::get::<NotesResponse>(&format!("/api/books/{}/notes", bid_refresh), move |resp_result| {
                        if let Ok(resp) = resp_result {
                            crate::local_book::rest_notes(&bid_sync, &resp.notes);
                            crate::local_book::sync_notes(&bid_sync, &resp.notes, &resp.tree);
                            store.notes.set(resp.notes);
                            store.note_tree.set(Some(resp.tree));
                            // The list is REST's; what the author sees is the document's over it.
                            crate::local_book::project_notes(store);
                        }
                    });
                }
                Err(e) => eprintln!("Failed to create note: {}", e.message),
            }
        });
    };

    // The debounced note save and the save-on-leave are the same write, so they are the
    // same code (`flush::flush_note`). Keeping them separate is what let them disagree:
    // this one used to clear `note_dirty` *before* checking the pane, so once any exit
    // had moved the pane on, the 800ms timer would mark the note clean and save nothing
    // — the edit was gone, and no later flush could recover it because the surface now
    // looked untouched.
    let save_note_content = move || flush::flush_pending_edits(state, store);

    let schedule_note_save = move || {
        note_dirty.set(true);
        note_save_status.set("unsaved");
        // Same question as the chapter autosave above, same reason — a note body in a
        // cut-over book travels by sync alone too.
        stall::veto_if_stalled(
            crate::local_store::BodyKind::Note,
            &note_handle.get(),
            &bid_signal.get(),
            note_save_status,
        );
        if let Some(h) = note_save_timer_id.get() {
            rinch_core::clear_timeout(h);
        }
        note_save_timer_id.set(Some(rinch_core::reactive::unowned(|| rinch_core::set_timeout(800, move || {
            // Same guard as the chapter autosave timer (PR #62): this callback can
            // outlive the page scope, and the first thing the save does is read
            // `note_dirty` — `Signal::get()` panics on a freed slot.
            bail_if_stale!();
            save_note_content();
        }))));
    };

    // Note-editor autosave on edit. Like the chapter editor, the model-first note
    // editor emits no DOM `input` event, so edits are detected via the cross-platform
    // `EditorHandle::on_change` hook, gated on the note editor being active. (Toolbar
    // formatting and color changes call `schedule_note_save` directly.)
    note_handle.get().on_change(move || {
        bail_if_stale!();
        if matches!(active_pane.get(), BookPane::NoteEditor(_)) {
            schedule_note_save();
            // A sigil only ever arrives by being typed, so the edit notification is
            // also the completion menu's trigger — rinch has no selection-change
            // callback, and this is the one moment it would need.
            panes::note_editor::refresh_sigil_menu(state, store);
        }
    });

    // Jump from the chapter editor's "Notes here" strip to the note that named it —
    // the same load the notes tree does, so the note arrives with its body attached.
    let open_note_by_id = move |note_id: String| {
        panes::note_editor::open_note_by_id(state, store, note_id);
    };

    let go_back_to_notes = move || {
        // The back arrow was already the one exit that saved — it is now the same
        // `flush_pending_edits` every other exit calls, rather than its own private copy
        // of the logic. (This is the path `notes-sigils.spec.ts` was routed through to
        // dodge the sidebar bug.)
        panes::note_editor::close_sigil_menu(state);
        flush_pending_edits();
        active_pane.set(BookPane::Notes);
        // Refresh notes list
        let bid = bid_signal.get();
        let bid_rest = bid.clone();
        api::get::<NotesResponse>(&format!("/api/books/{}/notes", bid), move |result| {
            if let Ok(resp) = result {
                // What REST said, kept apart from the projection's output — the
                // fallback for any facet the local document has not heard of yet.
                crate::local_book::rest_notes(&bid_rest, &resp.notes);
                store.notes.set(resp.notes);
                store.note_tree.set(Some(resp.tree));
                crate::local_book::project_notes(store);
            }
        });
    };

    let go_back_to_chapters = move || {
        flush_pending_edits();
        active_pane.set(BookPane::Chapters);
    };

    rsx! {
        Fragment {
            style { {css::TYPOGRAPHY_CSS} }
            style { {css::NOTES_CSS} }
            style { {css::BOOK_WORKSPACE_CSS} }
            style { {SECTION_HEADER_CSS} }
            style { {PANE_HEADER_CSS} }
            style { {crate::components::dialog::DIALOG_CSS} }
            style { {crate::components::sheet::SHEET_CSS} }
            style { {editor_utils::EDITOR_CSS} }
            style { {css::FIND_CSS} }
            // Editor font styles
            style {
                {move || {
                    let fs = store.current_book.get()
                        .and_then(|b| b.font_settings.clone())
                        .unwrap_or_default();

                    let h1 = fs.h1.as_deref().unwrap_or("Macondo Swash Caps");
                    let h2 = fs.h2.as_deref().unwrap_or("Macondo Swash Caps");
                    let h3 = fs.h3.as_deref().unwrap_or("Macondo Swash Caps");
                    let body = fs.body.as_deref().unwrap_or("Playwrite DE Grund");
                    let quote = fs.quote.as_deref().unwrap_or("inherit");
                    let code = fs.code.as_deref().unwrap_or("monospace");
                    let p_spacing = fs.paragraph_spacing.unwrap_or(8.0);
                    let p_indent = fs.paragraph_indent.unwrap_or(0.0);
                    let h_indent = fs.heading_indent.unwrap_or(0.0);

                    format!(
                        ".book-workspace {{ --rinch-font-family: '{body}', serif; font-family: '{body}', serif; }}
                         .book-workspace h1, .book-workspace h2,
                         .book-workspace h3, .book-workspace h4,
                         .book-workspace h5, .book-workspace h6,
                         .book-workspace .rinch-title,
                         .book-workspace .book-sidebar-title {{ font-family: '{h1}', cursive; }}
                         .book-workspace .chapter-item .rinch-text,
                         .book-workspace .editor-topbar .rinch-text,
                         .book-workspace .editor-topbar .editor-title-input input {{ font-family: '{h1}', cursive; }}
                         .book-workspace .sidebar-chapter-item,
                         .book-workspace .chapter-item {{ font-family: '{body}', serif; }}
                         /* The book's body font sits on the wrapper; the editor's
                            own container inherits it (EDITOR_CSS sets
                            `font-family: inherit` there). Everything below has to
                            out-specify rinch-editor-view's injected defaults,
                            which style `[data-pm-editor]` directly and would
                            otherwise win — hence the wrapper ids. */
                         .editor-content {{ font-family: '{body}', serif; }}
                         #editor-main [data-pm-editor] p,
                         #note-editor-main [data-pm-editor] p {{ margin: 0 0 {p_spacing}px 0; text-indent: {p_indent}px; }}
                         #editor-main [data-pm-editor] h1,
                         #note-editor-main [data-pm-editor] h1 {{ font-family: '{h1}', cursive; text-indent: {h_indent}px; }}
                         #editor-main [data-pm-editor] h2,
                         #note-editor-main [data-pm-editor] h2 {{ font-family: '{h2}', cursive; text-indent: {h_indent}px; }}
                         #editor-main [data-pm-editor] h3, #editor-main [data-pm-editor] h4,
                         #editor-main [data-pm-editor] h5, #editor-main [data-pm-editor] h6,
                         #note-editor-main [data-pm-editor] h3, #note-editor-main [data-pm-editor] h4,
                         #note-editor-main [data-pm-editor] h5, #note-editor-main [data-pm-editor] h6 {{ font-family: '{h3}', cursive; text-indent: {h_indent}px; }}
                         #editor-main [data-pm-editor] blockquote,
                         #note-editor-main [data-pm-editor] blockquote {{ font-family: '{quote}', serif; }}
                         #editor-main [data-pm-editor] code, #editor-main [data-pm-editor] pre,
                         #note-editor-main [data-pm-editor] code, #note-editor-main [data-pm-editor] pre {{ font-family: '{code}', monospace; }}"
                    )
                }}
            }

            div {
                class: {move || if editor_writing.get() { "book-workspace is-writing" } else { "book-workspace" }},
                // ── Backdrop for mobile sidebar ──
                div {
                    class: {move || if store.sidebar_open.get() { "sidebar-backdrop open" } else { "sidebar-backdrop" }},
                    onclick: move || store.sidebar_open.set(false),
                }

                // ── Sidebar ──
                div {
                    class: {move || if store.sidebar_open.get() { "book-sidebar open" } else { "book-sidebar" }},

                    // Book header — mini jacket, title, quiet word-count line.
                    // Content up, tools down (#chapters mockup): this replaces the
                    // old flat title-plus-edit-pencil row with the object the rest
                    // of the app treats a book as (see `components/book_jacket.rs`).
                    div {
                        class: "ws-book",
                        onclick: move || {
                            let book = store.current_book.get();
                            if let Some(b) = book {
                                edit_book_title.set(b.title.clone());
                                edit_book_desc.set(b.description.clone());
                                edit_book_cover.set(b.cover_image.clone());
                                show_book_settings_modal.set(true);
                            }
                        },
                        div { class: "ws-book-mini" }
                        div {
                            style: "min-width: 0; flex: 1;",
                            div {
                                class: "ws-book-title",
                                {move || store.current_book.get().map(|b| b.title.clone()).unwrap_or_default()}
                            }
                            div {
                                class: "ws-book-words",
                                {move || {
                                    let words: u64 = store.chapters.get().iter().map(|c| c.word_count).sum();
                                    format!("{} words", panes::chapters::format_word_count_full(words))
                                }}
                            }
                        }
                    }

                    div { class: "book-sidebar-nav",
                        SectionHeader {
                            label: "Manuscript",
                            count: {move || Some(store.chapters.get().len() as i64)},
                            active: {move || matches!(active_pane.get(), BookPane::Chapters)},
                            onclick: open_chapters_pane,
                            span {
                                style: "display: inline-flex; align-items: center; margin-right: 2px;",
                                onclick: move || chapters_collapsed.update(|c| *c = !*c),
                                {move || if chapters_collapsed.get() { "\u{25b8}" } else { "\u{25be}" }}
                            }
                            ActionIcon {
                                variant: "subtle",
                                size: "xs",
                                onclick: move || {
                                    flush_pending_edits();
                                    active_pane.set(BookPane::Chapters);
                                    store.sidebar_open.set(false);
                                    editing_chapter_id.set(None);
                                    editing_chapter_title.set(String::new());
                                    pending_new_chapter.set(true);
                                },
                                {render_tabler_icon(__scope, TablerIcon::Plus, TablerIconStyle::Outline)}
                            }
                        }

                        // Chapter list — collapsible
                        div {
                            class: "sidebar-chapter-list",
                            style: {move || if chapters_collapsed.get() { "display: none;" } else { "" }},
                            for chapter in store.chapters.get() {
                                {sidebar::sidebar_chapter_item(
                                    __scope,
                                    chapter.id.clone(),
                                    chapter.title.clone(),
                                    chapter.word_count,
                                    active_pane,
                                    open_chapter,
                                )}
                            }
                        }

                        SectionHeader {
                            label: "Notes",
                            count: {move || Some(store.notes.get().len() as i64)},
                            active: {move || matches!(active_pane.get(), BookPane::Notes | BookPane::NoteEditor(_) | BookPane::Calendar)},
                            style: "margin-top: var(--pw-space-sm);",
                            onclick: open_notes_pane,
                            ActionIcon {
                                variant: "subtle",
                                size: "xs",
                                onclick: move || {
                                    new_note_title.set(String::new());
                                    new_note_parent_id.set(None);
                                    new_note_color.set("teal".to_string());
                                    show_note_modal.set(true);
                                },
                                {render_tabler_icon(__scope, TablerIcon::Plus, TablerIconStyle::Outline)}
                            }
                        }
                    }

                    // Tools footer strip — Typography / Beta readers / History /
                    // Preview, demoted from equal-weight section headers to a row
                    // of icon buttons: these open roughly once a week, unlike the
                    // manuscript above. Sits above the theme/dashboard/logout row.
                    div { class: "ws-tools",
                        div {
                            class: "tool",
                            data-tip: "Typography",
                            onclick: open_typography_pane,
                            {render_tabler_icon(__scope, TablerIcon::Typography, TablerIconStyle::Outline)}
                        }
                        div {
                            class: "tool",
                            data-tip: "Beta readers",
                            onclick: open_beta_pane,
                            {render_tabler_icon(__scope, TablerIcon::Users, TablerIconStyle::Outline)}
                            if !beta_links.get().is_empty() {
                                span { class: "badge", {move || format!("{}", beta_links.get().len())} }
                            }
                        }
                        div {
                            class: "tool",
                            data-tip: "History",
                            onclick: open_history_pane,
                            {render_tabler_icon(__scope, TablerIcon::History, TablerIconStyle::Outline)}
                        }
                        div {
                            class: "tool",
                            data-tip: "Preview as reader",
                            onclick: move || {
                                flush_pending_edits();
                                router::navigate(Route::ReaderPreview(bid_signal.get()));
                            },
                            {render_tabler_icon(__scope, TablerIcon::Eye, TablerIconStyle::Outline)}
                        }
                        span { class: "sp" }
                        div {
                            class: "tool",
                            data-tip: "Theme",
                            onclick: toggle_dark,
                            // Reactive icon: an rsx `if` block re-renders the child
                            // node when dark_mode toggles. A bare `{expr}` block is
                            // captured once; a `{|| ...}` child closure is treated as
                            // reactive text by rinch, not a node.
                            if store.dark_mode.get() {
                                {render_tabler_icon(__scope, TablerIcon::Sun, TablerIconStyle::Outline)}
                            } else {
                                {render_tabler_icon(__scope, TablerIcon::Moon, TablerIconStyle::Outline)}
                            }
                        }
                        div {
                            class: "tool",
                            data-tip: "All books",
                            onclick: go_dashboard,
                            {render_tabler_icon(__scope, TablerIcon::Home, TablerIconStyle::Outline)}
                        }
                        div {
                            class: "tool",
                            data-tip: "Sign out",
                            onclick: logout,
                            {render_tabler_icon(__scope, TablerIcon::Logout, TablerIconStyle::Outline)}
                        }
                    }
                }

                // ── Main Pane ──
                div { class: "book-main-pane",
                    // Mobile hamburger bar
                    div { class: "mobile-topbar",
                        ActionIcon {
                            variant: "subtle",
                            onclick: toggle_sidebar,
                            {render_tabler_icon(__scope, TablerIcon::Menu2, TablerIconStyle::Outline)}
                        }
                        if current_chapter_has_feedback() {
                            ActionIcon {
                                variant: {move || if show_feedback_sidebar.get() { "filled".to_string() } else { "subtle".to_string() }},
                                size: "sm",
                                onclick: move || show_feedback_sidebar.update(|v| *v = !*v),
                                {render_tabler_icon(__scope, TablerIcon::MessageCircle, TablerIconStyle::Outline)}
                            }
                        }
                    }

                    // Chapters pane (CSS toggle, always in DOM)
                    {panes::chapters::render(
                        __scope,
                        state,
                        store,
                        open_chapter,
                        reorder_chapter,
                        request_delete_chapter,
                        add_chapter,
                        save_rename_chapter,
                    )}

                    // Editor pane (CSS toggle, always in DOM — preserves undo history)
                    {panes::editor::render(
                        __scope,
                        state,
                        store,
                        open_note_by_id,
                        book_id.clone(),
                        saved_here_only,
                        go_back_to_chapters,
                        save_chapter_title_inline,
                        schedule_chapter_autosave,
                        save_content,
                        author_reply,
                        resolve_feedback,
                        delete_feedback,
                    )}

                    // Typography pane (CSS toggle)
                    {panes::typography::render(__scope, state, store)}

                    // Beta Readers pane (CSS toggle)
                    {panes::beta_readers::render(
                        __scope,
                        state,
                        store,
                        copy_beta_link,
                        toggle_beta_link_active,
                        delete_beta_link,
                        author_reply,
                        resolve_feedback,
                        delete_feedback,
                        navigate_to_feedback,
                    )}

                    // Notes pane (CSS toggle)
                    {panes::notes::render(__scope, state, store)}

                    // Note Editor pane (CSS toggle)
                    {panes::note_editor::render(
                        __scope,
                        state,
                        store,
                        book_id.clone(),
                        saved_here_only,
                        go_back_to_notes,
                        schedule_note_save,
                    )}

                    // History pane (CSS toggle)
                    {panes::history::render(__scope, state)}

                    // Calendar pane (CSS toggle) — notes card 4
                    {panes::calendar::render(__scope, state, store)}
                }
            }

            {modals::render(
                __scope,
                state,
                store,
                book_id.clone(),
                add_note,
                save_book_settings,
                confirm_delete_chapter,
                add_beta_link,
                update_beta_link,
            )}

            // Find and replace — a floating overlay, not a pane: it stays open
            // over whichever pane is showing, which is the whole point of a
            // book-wide search.
            {panes::find::render(__scope, state, store)}
        }
    }
}
