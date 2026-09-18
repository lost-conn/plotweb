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
use crate::fonts;
use crate::pages::editor_utils;
use crate::router;
use crate::store::{AppStore, Route};

mod css;
mod feedback;
mod modals;
mod panes;
mod sidebar;
mod state;

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
        chapter_handle,
        note_handle,
        bid_signal,
        show_chapter_modal,
        new_chapter_title,
        show_rename_chapter_modal,
        rename_chapter_id,
        rename_chapter_title,
        chapter_title_save_timer_id,
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
        note_editor_title,
        note_editor_color,
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

    PAGE_GEN.with(|g| g.set(g.get().wrapping_add(1)));
    let page_gen = PAGE_GEN.with(|g| g.get());

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
        if PAGE_GEN.with(|g| g.get()) != page_gen {
            return;
        }
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
        if let Some(h) = auto_save_timer_id.get() {
            rinch_core::clear_timeout(h);
        }
        let captured_cid = match active_pane.get() {
            BookPane::Editor(cid) => cid,
            _ => return,
        };
        auto_save_timer_id.set(Some(rinch_core::reactive::unowned(|| rinch_core::set_timeout(3000, move || {
            if PAGE_GEN.with(|g| g.get()) != page_gen { return; }
            // Recompute the word count from the model (debounced, not per-keystroke).
            editor_word_count.set(editor_utils::editor_word_count(&chapter_handle.get()));
            if let BookPane::Editor(ref current_cid) = active_pane.get() {
                if *current_cid == captured_cid {
                    save_content(captured_cid.clone());
                }
            }
        }))));
    };

    // ── Set up Ctrl+S and auto-save (once, on mount) ────────────
    if let Some(window) = crate::platform::window() {
        // Ctrl+S — immediate save of the current chapter.
        let keydown = wasm_bindgen::closure::Closure::wrap(Box::new(move |event: web_sys::KeyboardEvent| {
            if (event.ctrl_key() || event.meta_key()) && event.key() == "s" {
                event.prevent_default();
                if let BookPane::Editor(ref cid) = active_pane.get() {
                    save_content(cid.clone());
                }
            }
        }) as Box<dyn FnMut(_)>);
        window.add_event_listener_with_callback("keydown", keydown.as_ref().unchecked_ref()).ok();
        keydown.forget();
    }

    // Auto-save on edit. Cross-platform: the editor notifies us after any local edit
    // (typing/paste/IME/commands), and deliberately not for selection-only changes or
    // for `load_doc`, so opening a chapter can't re-trigger a save. Registered outside
    // the `window` guard above so it runs on native too. Gate on the chapter editor
    // being active — the note editor has its own hook.
    chapter_handle.get().on_change(move || {
        if PAGE_GEN.with(|g| g.get()) != page_gen { return; }
        if matches!(active_pane.get(), BookPane::Editor(_)) {
            schedule_chapter_autosave();
        }
    });

    // ── Chapter actions ─────────────────────────────────────────
    let add_chapter = move || {
        let title = new_chapter_title.get();
        if title.trim().is_empty() {
            return;
        }
        show_chapter_modal.set(false);
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

    // ── Chapter rename save ───────────────────────────────────────
    let save_rename_chapter = move || {
        let title = rename_chapter_title.get();
        if title.trim().is_empty() {
            return;
        }
        show_rename_chapter_modal.set(false);
        let bid = bid_signal.get();
        let bid_sync = bid.clone();
        let cid = rename_chapter_id.get();
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
            if PAGE_GEN.with(|g| g.get()) != page_gen { return; }
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

    let move_chapter = move |chapter_id: String, direction: i32| {
        move || {
            let mut chapters = store.chapters.get();
            let Some(idx) = chapters.iter().position(|c| c.id == chapter_id) else {
                return;
            };
            let new_idx = idx as i32 + direction;
            if new_idx < 0 || (new_idx as usize) >= chapters.len() {
                return;
            }
            chapters.swap(idx, new_idx as usize);

            let bid = bid_signal.get();
            crate::local_book::sync_chapters(&bid, &chapters);

            // Hold the DOM-visible swap (and the REST PUT) until the local-doc
            // write above has actually landed. Flipping `store.chapters` first
            // would let an immediate reload race that write — it's a real
            // IndexedDB round trip, not same-tick — and losing that race means
            // the reload's local-doc-wins projection silently reverts this
            // reorder even though the REST call below reliably lands. See
            // `local_book::on_settled`.
            let ids: Vec<String> = chapters.iter().map(|c| c.id.clone()).collect();
            let bid_put = bid.clone();
            let moved = chapter_id.clone();
            crate::local_book::on_settled(&bid, move || {
                // Re-apply the swap to whatever the list holds *now* rather than
                // writing back the copy captured before the wait. A projection can
                // land during that window — a sync integration re-projecting the
                // book doc — and putting a pre-wait snapshot over it would revert
                // it, which is the failure this whole change exists to stop.
                store.chapters.update(|chapters| {
                    let Some(idx) = chapters.iter().position(|c| c.id == moved) else {
                        return;
                    };
                    let new_idx = idx as i32 + direction;
                    if new_idx >= 0 && (new_idx as usize) < chapters.len() {
                        chapters.swap(idx, new_idx as usize);
                    }
                });
                let req = ReorderChaptersRequest { chapter_ids: ids };
                api::put::<_, serde_json::Value>(&format!("/api/books/{}/chapters/reorder", bid_put), &req, move |_result| {});
            });
        }
    };

    let delete_chapter = move |chapter_id: String| {
        move || {
            let cid = chapter_id.clone();
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
        }
    };

    let go_dashboard = move || {
        router::navigate(Route::Dashboard);
    };

    let logout = move || {
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
        move || panes::chapters::do_switch_chapter(active_pane, save_alert, auto_save_timer_id, chapter_title_save_timer_id, save_status, editor_word_count, loaded_chapter_id, chapter_dirty, chapter_handle, chapter_title, store, &bid_signal.get(), &chapter_id)
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
            // Otherwise switch chapter and scroll after load
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

    // Synchronously flush a pending (debounced) chapter autosave before leaving
    // the editor pane. Without this, navigating away via the back-arrow or any
    // sidebar button discards edits made in the last ~3s, because the pending
    // debounce timer later sees active_pane != Editor and skips the save.
    let flush_editor_if_active = move || {
        if let BookPane::Editor(ref current_id) = active_pane.get() {
            let current_id = current_id.clone();
            // Clear the pending debounce timer so it doesn't fire a redundant/stale save.
            if let Some(h) = auto_save_timer_id.get() {
                rinch_core::clear_timeout(h);
                auto_save_timer_id.set(None);
            }
            // Don't flush while the chapter is still loading: the model holds the
            // previous chapter's content, so saving it to `current_id` would overwrite
            // this chapter with another chapter's content.
            if loaded_chapter_id.get().as_deref() != Some(current_id.as_str()) {
                return;
            }
            let bid = bid_signal.get();
            if let Some(content) = editor_utils::editor_content_json(&chapter_handle.get()) {
                save_status.set("saving");
                panes::chapters::save_chapter_body(
                    format!("/api/books/{}/chapters/{}", bid, current_id),
                    content,
                    !sends_body_content(),
                    save_status,
                    save_alert,
                );
            }
        }
    };

    // ── Sidebar click handlers ──────────────────────────────────
    let _open_chapters_pane = move || {
        flush_editor_if_active();
        active_pane.set(BookPane::Chapters);
        store.sidebar_open.set(false);
    };

    let open_typography_pane = move || {
        flush_editor_if_active();
        active_pane.set(BookPane::Typography);
        store.sidebar_open.set(false);
    };

    let open_beta_pane = move || {
        flush_editor_if_active();
        active_pane.set(BookPane::BetaReaders);
        store.sidebar_open.set(false);
    };

    let open_history_pane = move || {
        flush_editor_if_active();
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

    let open_notes_pane = move || {
        flush_editor_if_active();
        active_pane.set(BookPane::Notes);
        store.sidebar_open.set(false);
        // Fetch notes
        let bid = bid_signal.get();
        api::get::<NotesResponse>(&format!("/api/books/{}/notes", bid), move |result| {
            if let Ok(resp) = result {
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
                            crate::local_book::sync_notes(&bid_sync, &resp.notes, &resp.tree);
                            store.notes.set(resp.notes);
                            store.note_tree.set(Some(resp.tree));
                        }
                    });
                }
                Err(e) => eprintln!("Failed to create note: {}", e.message),
            }
        });
    };

    let save_note_content = move || {
        // Nothing was edited: writing here could only ever overwrite the stored note
        // with whatever the editor was handed.
        if !note_dirty.get() {
            return;
        }
        note_dirty.set(false);
        if let BookPane::NoteEditor(ref nid) = active_pane.get() {
            let nid = nid.clone();
            let bid = bid_signal.get();
            let title_val = note_editor_title.get();
            let color_val = note_editor_color.get();

            // Serialize the durable save shape (DocNode JSON) from the note editor model.
            let content = editor_utils::editor_content_json(&note_handle.get()).unwrap_or_default();

            // Local-first: mirror the rename/recolor into the `book:` doc's note
            // titles/colors Maps (structure decoupled), beside the REST PUT below.
            crate::local_book::note_meta(&bid, &nid, Some(&title_val), color_val.as_deref());

            note_save_status.set("saving");
            panes::note_editor::save_note_body(
                format!("/api/books/{}/notes/{}", bid, nid),
                title_val,
                content,
                color_val,
                !sends_body_content(),
                note_save_status,
                save_alert,
            );
        }
    };

    let schedule_note_save = move || {
        note_dirty.set(true);
        note_save_status.set("unsaved");
        if let Some(h) = note_save_timer_id.get() {
            rinch_core::clear_timeout(h);
        }
        note_save_timer_id.set(Some(rinch_core::reactive::unowned(|| rinch_core::set_timeout(800, move || {
            save_note_content();
        }))));
    };

    // Note-editor autosave on edit. Like the chapter editor, the model-first note
    // editor emits no DOM `input` event, so edits are detected via the cross-platform
    // `EditorHandle::on_change` hook, gated on the note editor being active. (Toolbar
    // formatting and color changes call `schedule_note_save` directly.)
    note_handle.get().on_change(move || {
        if PAGE_GEN.with(|g| g.get()) != page_gen { return; }
        if matches!(active_pane.get(), BookPane::NoteEditor(_)) {
            schedule_note_save();
        }
    });

    let go_back_to_notes = move || {
        // Save before navigating back
        save_note_content();
        active_pane.set(BookPane::Notes);
        // Refresh notes list
        let bid = bid_signal.get();
        api::get::<NotesResponse>(&format!("/api/books/{}/notes", bid), move |result| {
            if let Ok(resp) = result {
                store.notes.set(resp.notes);
                store.note_tree.set(Some(resp.tree));
                crate::local_book::project_notes(store);
            }
        });
    };

    let go_back_to_chapters = move || {
        flush_editor_if_active();
        active_pane.set(BookPane::Chapters);
    };

    rsx! {
        Fragment {
            style { {css::TYPOGRAPHY_CSS} }
            style { {css::NOTES_CSS} }
            style { {css::BOOK_WORKSPACE_CSS} }
            style { {editor_utils::EDITOR_CSS} }
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

            div { class: "book-workspace",
                // ── Backdrop for mobile sidebar ──
                div {
                    class: {move || if store.sidebar_open.get() { "sidebar-backdrop open" } else { "sidebar-backdrop" }},
                    onclick: move || store.sidebar_open.set(false),
                }

                // ── Sidebar ──
                div {
                    class: {move || if store.sidebar_open.get() { "book-sidebar open" } else { "book-sidebar" }},

                    // Book title
                    div { class: "book-sidebar-title",
                        span {
                            style: "flex: 1; overflow: hidden; text-overflow: ellipsis;",
                            {move || store.current_book.get().map(|b| b.title.clone()).unwrap_or_default()}
                        }
                        ActionIcon {
                            variant: "subtle",
                            size: "xs",
                            onclick: move || {
                                let book = store.current_book.get();
                                if let Some(b) = book {
                                    edit_book_title.set(b.title.clone());
                                    edit_book_desc.set(b.description.clone());
                                    edit_book_cover.set(b.cover_image.clone());
                                    show_book_settings_modal.set(true);
                                }
                            },
                            {render_tabler_icon(__scope, TablerIcon::Pencil, TablerIconStyle::Outline)}
                        }
                    }
                    Space { h: "sm" }

                    div { class: "book-sidebar-nav",
                        // Chapters section header
                        div {
                            class: {move || if matches!(active_pane.get(), BookPane::Chapters) { "sidebar-section-header active" } else { "sidebar-section-header" }},
                            div {
                                style: "display: flex; align-items: center; cursor: pointer;",
                                onclick: move || chapters_collapsed.update(|c| *c = !*c),
                                span {
                                    style: "margin-right: 6px; display: inline-flex; align-items: center; font-size: 10px; line-height: 1; position: relative; top: -1px;",
                                    {move || if chapters_collapsed.get() { "\u{25b8}" } else { "\u{25be}" }}
                                }
                                "Chapters"
                            }
                            div {
                                style: "display: flex; align-items: center; gap: 2px;",
                                ActionIcon {
                                    variant: "subtle",
                                    size: "xs",
                                    onclick: move || {
                                        new_chapter_title.set(String::new());
                                        show_chapter_modal.set(true);
                                    },
                                    {render_tabler_icon(__scope, TablerIcon::Plus, TablerIconStyle::Outline)}
                                }
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
                                    active_pane,
                                    open_chapter,
                                    move_chapter,
                                )}
                            }
                        }

                        // Notes section header
                        div {
                            class: {move || if matches!(active_pane.get(), BookPane::Notes | BookPane::NoteEditor(_)) { "sidebar-section-header active" } else { "sidebar-section-header" }},
                            div {
                                style: "display: flex; align-items: center; cursor: pointer; flex: 1;",
                                onclick: open_notes_pane,
                                "Notes"
                            }
                            div {
                                style: "display: flex; align-items: center; gap: 2px;",
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

                        // Typography section header
                        div {
                            class: {move || if matches!(active_pane.get(), BookPane::Typography) { "sidebar-section-header active" } else { "sidebar-section-header" }},
                            onclick: open_typography_pane,
                            "Typography"
                        }

                        // Beta Readers section header
                        div {
                            class: {move || if matches!(active_pane.get(), BookPane::BetaReaders) { "sidebar-section-header active" } else { "sidebar-section-header" }},
                            onclick: open_beta_pane,
                            div {
                                style: "display: flex; align-items: center; justify-content: space-between; width: 100%;",
                                "Beta Readers"
                                if !beta_links.get().is_empty() {
                                    Badge {
                                        variant: "light",
                                        size: "xs",
                                        {move || format!("{}", beta_links.get().len())}
                                    }
                                }
                            }
                        }

                        // History section header
                        div {
                            class: {move || if matches!(active_pane.get(), BookPane::History) { "sidebar-section-header active" } else { "sidebar-section-header" }},
                            onclick: open_history_pane,
                            "History"
                        }
                    }

                    // Footer
                    div { class: "book-sidebar-footer",
                        Button {
                            variant: "subtle",
                            size: "xs",
                            onclick: move || router::navigate(Route::ReaderPreview(bid_signal.get())),
                            {render_tabler_icon(__scope, TablerIcon::Eye, TablerIconStyle::Outline)}
                            " Preview as reader"
                        }
                        Button {
                            variant: "subtle",
                            size: "xs",
                            onclick: go_dashboard,
                            "\u{2190} Dashboard"
                        }
                        div { class: "book-sidebar-footer-row",
                            ActionIcon {
                                variant: "subtle",
                                size: "sm",
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
                            ActionIcon {
                                variant: "subtle",
                                size: "sm",
                                onclick: logout,
                                {render_tabler_icon(__scope, TablerIcon::Logout, TablerIconStyle::Outline)}
                            }
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
                        if !beta_feedback.get().is_empty() && matches!(active_pane.get(), BookPane::Editor(_)) {
                            ActionIcon {
                                variant: {move || if show_feedback_sidebar.get() { "filled".to_string() } else { "subtle".to_string() }},
                                size: "sm",
                                onclick: move || show_feedback_sidebar.update(|v| *v = !*v),
                                {render_tabler_icon(__scope, TablerIcon::MessageCircle, TablerIconStyle::Outline)}
                            }
                        }
                    }

                    // Chapters pane (CSS toggle, always in DOM)
                    {panes::chapters::render(__scope, state, store, open_chapter, move_chapter, delete_chapter)}

                    // Editor pane (CSS toggle, always in DOM — preserves undo history)
                    {panes::editor::render(
                        __scope,
                        state,
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
                        book_id.clone(),
                        saved_here_only,
                        go_back_to_notes,
                        schedule_note_save,
                    )}

                    // History pane (CSS toggle)
                    {panes::history::render(__scope, state)}
                }
            }

            {modals::render(
                __scope,
                state,
                store,
                book_id.clone(),
                add_note,
                add_chapter,
                save_book_settings,
                save_rename_chapter,
                add_beta_link,
                update_beta_link,
            )}
        }
    }
}
