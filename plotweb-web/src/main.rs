pub mod web_document;
pub mod api;
pub mod store;
pub mod router;
pub mod pages;
pub mod components;
pub mod fonts;
pub mod ws;

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use rinch::prelude::*;
use rinch_core::dom::*;
use rinch_core::element::ThemeProviderProps;
use rinch_core::events;

use store::{AppStore, Route};

// ── Event delegation ────────────────────────────────────────────────────────

fn utf16_offset_to_utf8_bytes(text: &str, utf16_offset: u32) -> usize {
    let mut utf16_count = 0u32;
    for (byte_idx, ch) in text.char_indices() {
        if utf16_count >= utf16_offset {
            return byte_idx;
        }
        utf16_count += ch.len_utf16() as u32;
    }
    text.len()
}

fn resolve_text_hit(
    browser_doc: &web_sys::Document,
    client_x: f32,
    client_y: f32,
) -> Option<events::TextHitInfo> {
    let func = js_sys::Reflect::get(browser_doc, &"caretRangeFromPoint".into()).ok()?;
    let func: js_sys::Function = func.dyn_into().ok()?;
    let range_val = func
        .call2(
            browser_doc,
            &JsValue::from(client_x),
            &JsValue::from(client_y),
        )
        .ok()?;
    if range_val.is_null() || range_val.is_undefined() {
        return None;
    }
    let range: web_sys::Range = range_val.dyn_into().ok()?;
    let start_container = range.start_container().ok()?;
    let start_offset = range.start_offset().ok()?;

    let mut current: Option<web_sys::Node> = Some(start_container.clone());
    let mut block_el: Option<web_sys::Element> = None;
    while let Some(node) = current {
        if let Ok(el) = node.clone().dyn_into::<web_sys::Element>()
            && el.has_attribute("data-block-index")
        {
            block_el = Some(el);
            break;
        }
        current = node.parent_node();
    }
    let block_el = block_el?;
    let block_index: usize = block_el.get_attribute("data-block-index")?.parse().ok()?;

    let byte_offset = compute_byte_offset_in_block(&block_el, &start_container, start_offset);

    Some(events::TextHitInfo {
        block_index,
        byte_offset,
        inline_root_node_id: 0,
        valid: true,
    })
}

fn compute_byte_offset_in_block(
    block_el: &web_sys::Element,
    target_text_node: &web_sys::Node,
    utf16_offset_in_target: u32,
) -> usize {
    let mut byte_offset = 0usize;
    walk_text_nodes_for_offset(
        &block_el.clone().into(),
        target_text_node,
        utf16_offset_in_target,
        &mut byte_offset,
    );
    byte_offset
}

fn walk_text_nodes_for_offset(
    node: &web_sys::Node,
    target: &web_sys::Node,
    utf16_offset: u32,
    byte_offset: &mut usize,
) -> bool {
    if node.node_type() == web_sys::Node::TEXT_NODE {
        if node == target {
            let text = node.text_content().unwrap_or_default();
            *byte_offset += utf16_offset_to_utf8_bytes(&text, utf16_offset);
            return true;
        }
        let text = node.text_content().unwrap_or_default();
        *byte_offset += text.len();
        return false;
    }
    let children = node.child_nodes();
    for i in 0..children.length() {
        if let Some(child) = children.item(i)
            && walk_text_nodes_for_offset(&child, target, utf16_offset, byte_offset)
        {
            return true;
        }
    }
    false
}

fn setup_event_delegation(doc: &web_document::WebDocument) {
    let browser_doc = doc.browser_document().clone();

    // ── Drag state machine ──────────────────────────────────────────────────
    // Shared between pointerdown/pointermove/pointerup/pointercancel closures.
    // Rinch's native renderer has a pending-drag system with a 5px threshold
    // that separates clicks from drags. The web DOM bridge replicates it, but
    // driven by Pointer Events (not mouse) so it works for touch + pen too:
    //   * mouse: pending on pointerdown inside a draggable, activate at 5px.
    //   * touch/pen: activate on a ~350ms long-press. Movement past 10px before
    //     the hold completes is treated as a scroll and abandons the pending
    //     drag, so a list of draggable rows still scrolls normally.
    // On activation we setPointerCapture (moves keep flowing to us even off the
    // source) and preventDefault during the active drag to suppress scrolling.
    // A single primary pointer owns the drag; other pointers are ignored.
    const DRAG_THRESHOLD: f32 = 5.0;
    const TOUCH_LONG_PRESS_MS: f64 = 350.0;
    const TOUCH_MOVE_SLOP: f32 = 10.0;

    #[derive(Clone, Copy, PartialEq)]
    enum PtrKind {
        Mouse,
        Touch,
    }
    impl PtrKind {
        fn from_event(e: &web_sys::PointerEvent) -> Self {
            // Pen is treated like touch (long-press to drag).
            if e.pointer_type() == "mouse" {
                PtrKind::Mouse
            } else {
                PtrKind::Touch
            }
        }
    }

    enum DragPhase {
        /// Pointerdown happened on a draggable element, waiting to see if it
        /// becomes a drag (mouse: 5px; touch: 350ms hold) or a click/tap.
        Pending {
            pointer_id: i32,
            kind: PtrKind,
            click_rid: usize,
            click_el: web_sys::Element,
            draggable_el: web_sys::Element,
            start_x: f32,
            start_y: f32,
            /// Pointerdown timestamp (ms) — for the touch long-press.
            start_time: f64,
        },
        /// Activation reached — drag is live.
        Active {
            pointer_id: i32,
            draggable_el: web_sys::Element,
            over_el: Option<web_sys::Element>,
        },
    }

    let drag_phase: Rc<RefCell<Option<DragPhase>>> = Rc::new(RefCell::new(None));

    fn phase_pointer_id(p: &DragPhase) -> i32 {
        match p {
            DragPhase::Pending { pointer_id, .. } | DragPhase::Active { pointer_id, .. } => {
                *pointer_id
            }
        }
    }

    // ── Pointerdown ─────────────────────────────────────────────────────────
    let browser_doc_for_click = browser_doc.clone();
    let drag_phase_down = drag_phase.clone();
    let pointerdown_closure = Closure::wrap(Box::new(move |event: web_sys::PointerEvent| {
        // A drag is owned by a single pointer; ignore additional pointers (e.g.
        // a second finger) while one is in progress.
        if drag_phase_down.borrow().is_some() {
            return;
        }
        // Only the primary button / primary pointer starts a drag or click.
        if event.button() > 0 {
            return;
        }
        if let Some(target) = event.target()
            && let Ok(el) = target.dyn_into::<web_sys::Element>()
        {
            // Clear render surface focus if click is outside any surface
            if rinch::render_surface::focused_surface_id().is_some()
                && el.closest("[data-render-surface]").ok().flatten().is_none()
            {
                rinch::render_surface::set_focused_surface(None);
            }

            // Check if click is inside a draggable element
            if let Ok(Some(draggable_el)) = el.closest("[draggable='true']") {
                if let Ok(Some(rid_el)) = el.closest("[data-rid]")
                    && let Some(rid_str) = rid_el.get_attribute("data-rid")
                    && let Ok(rid) = rid_str.parse::<usize>()
                {
                    // Enter pending state — don't fire click yet. (preventDefault
                    // on pointerdown suppresses the compat mouse/click events; it
                    // does not block scrolling — that's gated by activation below.)
                    event.prevent_default();
                    *drag_phase_down.borrow_mut() = Some(DragPhase::Pending {
                        pointer_id: event.pointer_id(),
                        kind: PtrKind::from_event(&event),
                        click_rid: rid,
                        click_el: rid_el,
                        draggable_el,
                        start_x: event.client_x() as f32,
                        start_y: event.client_y() as f32,
                        start_time: event.time_stamp(),
                    });
                    return;
                }
            }

            // Not inside a draggable — fire click immediately (original behavior)
            if let Ok(Some(rid_el)) = el.closest("[data-rid]")
                && let Some(rid_str) = rid_el.get_attribute("data-rid")
                && let Ok(rid) = rid_str.parse::<usize>()
            {
                let rect = rid_el.get_bounding_client_rect();
                let text_hit = resolve_text_hit(
                    &browser_doc_for_click,
                    event.client_x() as f32,
                    event.client_y() as f32,
                )
                .unwrap_or_default();

                events::set_click_context(events::ClickContext {
                    mouse_x: event.client_x() as f32,
                    mouse_y: event.client_y() as f32,
                    element_x: rect.x() as f32,
                    element_y: rect.y() as f32,
                    element_width: rect.width() as f32,
                    element_height: rect.height() as f32,
                    text_hit,
                    viewport_width: 0.0,
                    viewport_height: 0.0,
                    button: Default::default(),
                    modifiers: Default::default(),
                });

                // Don't prevent default on native form elements so they can
                // receive focus and handle clicks normally.
                let tag = el.tag_name();
                let is_form_el = tag.eq_ignore_ascii_case("INPUT")
                    || tag.eq_ignore_ascii_case("TEXTAREA")
                    || tag.eq_ignore_ascii_case("SELECT");
                if !is_form_el {
                    event.prevent_default();
                }
                events::dispatch_event(events::EventHandlerId(rid));
            }
        }
    }) as Box<dyn FnMut(_)>);
    browser_doc
        .add_event_listener_with_callback(
            "pointerdown",
            pointerdown_closure.as_ref().unchecked_ref(),
        )
        .unwrap();
    pointerdown_closure.forget();

    // ── Pointermove ─────────────────────────────────────────────────────────
    let browser_doc_for_move = browser_doc.clone();
    let drag_phase_move = drag_phase.clone();
    let pointermove_closure = Closure::wrap(Box::new(move |event: web_sys::PointerEvent| {
        let mx = event.client_x() as f32;
        let my = event.client_y() as f32;
        let evt_pointer_id = event.pointer_id();

        // Take phase out so we don't hold a borrow during event dispatch
        let phase = drag_phase_move.borrow_mut().take();
        let new_phase = match phase {
            // A move from a non-owning pointer leaves the drag untouched.
            Some(p @ (DragPhase::Pending { .. } | DragPhase::Active { .. }))
                if phase_pointer_id(&p) != evt_pointer_id =>
            {
                Some(p)
            }
            Some(DragPhase::Pending {
                pointer_id,
                kind,
                click_rid,
                click_el,
                draggable_el,
                start_x,
                start_y,
                start_time,
            }) => {
                let dx = mx - start_x;
                let dy = my - start_y;
                let dist = (dx * dx + dy * dy).sqrt();
                let activate = match kind {
                    PtrKind::Mouse => dist >= DRAG_THRESHOLD,
                    // Touch/pen: hold ~350ms. Moving past the slop before then is
                    // a scroll, not a drag (handled in the else branch below).
                    PtrKind::Touch => event.time_stamp() - start_time >= TOUCH_LONG_PRESS_MS,
                };
                if activate {
                    // Activate drag: capture the pointer, suppress scrolling.
                    event.prevent_default();
                    draggable_el.set_pointer_capture(pointer_id).ok();
                    if let Some(handler_str) = draggable_el.get_attribute("data-ondragstart")
                        && let Ok(handler_id) = handler_str.parse::<usize>()
                    {
                        events::dispatch_event(events::EventHandlerId(handler_id));
                    }
                    Some(DragPhase::Active {
                        pointer_id,
                        draggable_el,
                        over_el: None,
                    })
                } else if kind == PtrKind::Touch && dist > TOUCH_MOVE_SLOP {
                    // Moved before the long-press completed — it's a scroll.
                    // Abandon the pending drag and let the browser scroll.
                    None
                } else {
                    // Still waiting (below mouse threshold, or holding for touch).
                    Some(DragPhase::Pending {
                        pointer_id,
                        kind,
                        click_rid,
                        click_el,
                        draggable_el,
                        start_x,
                        start_y,
                        start_time,
                    })
                }
            }
            Some(DragPhase::Active {
                pointer_id,
                draggable_el,
                mut over_el,
            }) => {
                event.prevent_default();
                // Hit-test for drop targets under cursor
                if let Some(el_under) = browser_doc_for_move.element_from_point(mx, my) {
                    let new_over = el_under.closest("[data-ondragenter]").ok().flatten();
                    // Fire ondragleave on the old target / ondragenter on the new
                    // one when the target changes.
                    let changed = match (&new_over, &over_el) {
                        (Some(a), Some(b)) => a != b,
                        (None, None) => false,
                        _ => true,
                    };
                    if changed {
                        // Leaving the previous target
                        if let Some(ref prev) = over_el
                            && let Some(handler_str) = prev.get_attribute("data-ondragleave")
                            && let Ok(handler_id) = handler_str.parse::<usize>()
                        {
                            events::dispatch_event(events::EventHandlerId(handler_id));
                        }
                        // Entering the new target
                        if let Some(ref target) = new_over
                            && let Some(handler_str) = target.get_attribute("data-ondragenter")
                            && let Ok(handler_id) = handler_str.parse::<usize>()
                        {
                            let rect = target.get_bounding_client_rect();
                            events::set_click_context(events::ClickContext {
                                mouse_x: mx,
                                mouse_y: my,
                                element_x: rect.x() as f32,
                                element_y: rect.y() as f32,
                                element_width: rect.width() as f32,
                                element_height: rect.height() as f32,
                                text_hit: Default::default(),
                                viewport_width: 0.0,
                                viewport_height: 0.0,
                                button: Default::default(),
                                modifiers: Default::default(),
                            });
                            events::dispatch_event(events::EventHandlerId(handler_id));
                        }
                    }
                    // Fire ondragover on the current target every move, passing the
                    // target's bounds + cursor so the handler can compute whether
                    // the drop is before / after / into the element.
                    if let Some(ref target) = new_over
                        && let Some(handler_str) = target.get_attribute("data-ondragover")
                        && let Ok(handler_id) = handler_str.parse::<usize>()
                    {
                        let rect = target.get_bounding_client_rect();
                        events::set_click_context(events::ClickContext {
                            mouse_x: mx,
                            mouse_y: my,
                            element_x: rect.x() as f32,
                            element_y: rect.y() as f32,
                            element_width: rect.width() as f32,
                            element_height: rect.height() as f32,
                            text_hit: Default::default(),
                            viewport_width: 0.0,
                            viewport_height: 0.0,
                            button: Default::default(),
                            modifiers: Default::default(),
                        });
                        events::dispatch_event(events::EventHandlerId(handler_id));
                    }
                    over_el = new_over;
                }
                // Fire ondragmove on the source every move, passing the cursor
                // position so the handler can position its own drag ghost.
                if let Some(handler_str) = draggable_el.get_attribute("data-ondragmove")
                    && let Ok(handler_id) = handler_str.parse::<usize>()
                {
                    events::set_click_context(events::ClickContext {
                        mouse_x: mx,
                        mouse_y: my,
                        ..Default::default()
                    });
                    events::dispatch_event(events::EventHandlerId(handler_id));
                }
                Some(DragPhase::Active {
                    pointer_id,
                    draggable_el,
                    over_el,
                })
            }
            None => None,
        };
        *drag_phase_move.borrow_mut() = new_phase;

        // Also handle Drag builder system (sliders, panels)
        if rinch_core::update_drag(mx, my).0 {
            event.prevent_default();
        }
    }) as Box<dyn FnMut(_)>);
    browser_doc
        .add_event_listener_with_callback(
            "pointermove",
            pointermove_closure.as_ref().unchecked_ref(),
        )
        .unwrap();
    pointermove_closure.forget();

    // ── Pointerup / Pointercancel ───────────────────────────────────────────
    // Shared release path. `fire_click` distinguishes a normal release (a
    // never-activated pending drag becomes a click) from a cancel (pointercancel
    // / the system stealing the gesture — no click). Only the owning pointer can
    // end the drag. An active drag always fires ondragend so the app clears its
    // own drag ghost.
    let browser_doc_for_up = browser_doc.clone();
    let drag_phase_up = drag_phase.clone();
    let on_release: Rc<dyn Fn(i32, bool)> = Rc::new(move |pid: i32, fire_click: bool| {
        // Take the phase only if this pointer owns it (drop the borrow before
        // dispatching handlers, which may run app code).
        let phase = {
            let mut slot = drag_phase_up.borrow_mut();
            let owns = matches!(
                slot.as_ref(),
                Some(DragPhase::Pending { pointer_id, .. } | DragPhase::Active { pointer_id, .. })
                    if *pointer_id == pid
            );
            if !owns {
                return;
            }
            slot.take()
        };
        match phase {
            Some(DragPhase::Pending {
                click_rid,
                click_el,
                start_x,
                start_y,
                ..
            }) => {
                if fire_click {
                    // Never activated — it was a click/tap, not a drag.
                    let rect = click_el.get_bounding_client_rect();
                    let text_hit = resolve_text_hit(&browser_doc_for_up, start_x, start_y)
                        .unwrap_or_default();
                    events::set_click_context(events::ClickContext {
                        mouse_x: start_x,
                        mouse_y: start_y,
                        element_x: rect.x() as f32,
                        element_y: rect.y() as f32,
                        element_width: rect.width() as f32,
                        element_height: rect.height() as f32,
                        text_hit,
                        viewport_width: 0.0,
                        viewport_height: 0.0,
                        button: Default::default(),
                        modifiers: Default::default(),
                    });
                    events::dispatch_event(events::EventHandlerId(click_rid));
                }
            }
            Some(DragPhase::Active { draggable_el, .. }) => {
                // Drag ended — fire ondragend on the source element.
                if let Some(handler_str) = draggable_el.get_attribute("data-ondragend")
                    && let Ok(handler_id) = handler_str.parse::<usize>()
                {
                    events::dispatch_event(events::EventHandlerId(handler_id));
                }
            }
            None => {}
        }
        // Also handle Drag builder cancel
        rinch_core::Drag::cancel();
    });

    let on_release_up = on_release.clone();
    let pointerup_closure = Closure::wrap(Box::new(move |event: web_sys::PointerEvent| {
        on_release_up(event.pointer_id(), true);
    }) as Box<dyn FnMut(_)>);
    browser_doc
        .add_event_listener_with_callback("pointerup", pointerup_closure.as_ref().unchecked_ref())
        .unwrap();
    pointerup_closure.forget();

    let on_release_cancel = on_release.clone();
    let pointercancel_closure = Closure::wrap(Box::new(move |event: web_sys::PointerEvent| {
        on_release_cancel(event.pointer_id(), false);
    }) as Box<dyn FnMut(_)>);
    browser_doc
        .add_event_listener_with_callback(
            "pointercancel",
            pointercancel_closure.as_ref().unchecked_ref(),
        )
        .unwrap();
    pointercancel_closure.forget();

    // Keyboard delegation
    let keydown_closure = Closure::wrap(Box::new(move |event: web_sys::KeyboardEvent| {
        if let Some(surface_id) = rinch::render_surface::focused_surface_id() {
            let key_data = rinch::render_surface::SurfaceKeyData {
                key: event.key(),
                code: event.code(),
                ctrl: event.ctrl_key() || event.meta_key(),
                shift: event.shift_key(),
                alt: event.alt_key(),
                meta: event.meta_key(),
            };
            rinch::render_surface::dispatch_surface_event(
                surface_id,
                rinch::render_surface::SurfaceEvent::KeyDown(key_data),
            );
            let key = event.key();
            if key.len() == 1 && !event.ctrl_key() && !event.meta_key() && !event.alt_key() {
                rinch::render_surface::dispatch_surface_event(
                    surface_id,
                    rinch::render_surface::SurfaceEvent::TextInput(key),
                );
            }
            event.prevent_default();
            event.stop_propagation();
            return;
        }

        let key_data = events::KeyEventData {
            key: event.key(),
            code: event.code(),
            ctrl: event.ctrl_key() || event.meta_key(),
            shift: event.shift_key(),
            alt: event.alt_key(),
            meta: event.meta_key(),
        };
        if events::dispatch_keyboard_event(&key_data) {
            event.prevent_default();
            event.stop_propagation();
        } else if event.key() == "Enter" && !event.shift_key() {
            if let Some(target) = event.target()
                && let Ok(el) = target.dyn_into::<web_sys::Element>()
            {
                let is_textarea = el.tag_name().eq_ignore_ascii_case("TEXTAREA");
                let mut found = false;
                let mut current: Option<web_sys::Element> = Some(el.clone());
                while let Some(ref cur) = current {
                    if let Some(handler_str) = cur.get_attribute("data-onsubmit")
                        && let Ok(handler_id) = handler_str.parse::<usize>()
                    {
                        event.prevent_default();
                        events::dispatch_event(events::EventHandlerId(handler_id));
                        found = true;
                        break;
                    }
                    current = cur.parent_element();
                }
                // For textareas inside .feedback-reply-input, find the send button
                if !found && is_textarea {
                    if let Ok(Some(container)) = el.closest(".feedback-reply-input") {
                        if let Ok(Some(btn)) = container.query_selector("[data-rid]") {
                            if let Some(rid_str) = btn.get_attribute("data-rid")
                                && let Ok(rid) = rid_str.parse::<usize>()
                            {
                                event.prevent_default();
                                events::dispatch_event(events::EventHandlerId(rid));
                            }
                        }
                    }
                }
            }
        }
    }) as Box<dyn FnMut(_)>);
    browser_doc
        .add_event_listener_with_callback("keydown", keydown_closure.as_ref().unchecked_ref())
        .unwrap();
    keydown_closure.forget();

    // Input delegation
    let browser_doc2 = browser_doc.clone();
    let input_closure = Closure::wrap(Box::new(move |event: web_sys::Event| {
        if let Some(target) = event.target() {
            let value = if let Ok(input) = target.clone().dyn_into::<web_sys::HtmlInputElement>() {
                Some(input.value())
            } else if let Ok(textarea) = target.clone().dyn_into::<web_sys::HtmlTextAreaElement>() {
                Some(textarea.value())
            } else {
                None
            };

            if let Some(value) = value {
                if let Ok(el) = target.dyn_into::<web_sys::Element>() {
                    let mut current: Option<web_sys::Element> = Some(el);
                    while let Some(el) = current {
                        if let Some(handler_str) = el.get_attribute("data-oninput")
                            && let Ok(handler_id) = handler_str.parse::<usize>()
                        {
                            events::dispatch_input_event(
                                events::EventHandlerId(handler_id),
                                value,
                            );
                            break;
                        }
                        current = el.parent_element();
                    }
                }
            }
        }
    }) as Box<dyn FnMut(_)>);
    browser_doc2
        .add_event_listener_with_callback("input", input_closure.as_ref().unchecked_ref())
        .unwrap();
    input_closure.forget();
}

// ── App component ───────────────────────────────────────────────────────────

#[component]
fn app() -> NodeHandle {
    let store = AppStore::new();
    rinch_core::create_store(store);

    // Parse the current URL to determine initial route
    let initial_route = web_sys::window()
        .and_then(|w| w.location().pathname().ok())
        .map(|p| Route::from_path(&p))
        .unwrap_or(Route::Dashboard);

    if matches!(initial_route, Route::Reader(_)) {
        store.current_route.set(initial_route.clone());
        router::replace_state(&initial_route);
        store.loading.set(false);
    } else if initial_route == Route::ThemePreview {
        store.current_route.set(Route::ThemePreview);
        router::replace_state(&Route::ThemePreview);
        store.loading.set(false);
    } else {
        let requested = initial_route;
        // Check session on start
        wasm_bindgen_futures::spawn_local(async move {
            match api::get::<plotweb_common::User>("/api/auth/me").await {
                Ok(user) => {
                    store.current_user.set(Some(user));
                    let route = match &requested {
                        Route::Login | Route::Register => Route::Dashboard,
                        other => other.clone(),
                    };
                    router::replace_state(&route);
                    store.current_route.set(route);
                }
                Err(_) => {
                    let route = match &requested {
                        Route::Register => Route::Register,
                        _ => Route::Login,
                    };
                    router::replace_state(&route);
                    store.current_route.set(route);
                }
            }
            store.loading.set(false);
        });
    }

    // Listen for back/forward navigation
    let popstate_closure = Closure::wrap(Box::new(move |_event: web_sys::Event| {
        if let Some(window) = web_sys::window() {
            if let Ok(pathname) = window.location().pathname() {
                let route = Route::from_path(&pathname);
                store.current_route.set(route);
            }
        }
    }) as Box<dyn FnMut(_)>);
    web_sys::window()
        .unwrap()
        .add_event_listener_with_callback("popstate", popstate_closure.as_ref().unchecked_ref())
        .unwrap();
    popstate_closure.forget();

    rsx! {
        ThemeProvider {
            primary_color_fn: Rc::new(|| "teal"),
            default_radius: "xs",
            dark_mode_fn: Rc::new(move || store.dark_mode.get()),

            {components::app_shell::app_shell(__scope)}
        }
    }
}

// ── Entry point ─────────────────────────────────────────────────────────────

#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
    console_log::init_with_level(log::Level::Info).ok();

    events::clear_handlers();
    rinch_core::clear_context();

    let theme = ThemeProviderProps {
        primary_color: Some("teal".into()),
        default_radius: Some("xs".into()),
        font_family: Some(
            "'Playwrite DE Grund', Georgia, 'Times New Roman', serif".into(),
        ),
        dark_mode: true,
        ..Default::default()
    };
    rinch::setup_theme_css(&theme);

    let browser_doc = web_sys::window().unwrap().document().unwrap();
    let web_doc = Rc::new(RefCell::new(web_document::WebDocument::new(browser_doc)));

    let doc_as_dom: Rc<RefCell<dyn DomDocument>> = web_doc.clone();
    let body_id = web_doc.borrow().body();
    let scope = Rc::new(RefCell::new(RenderScope::new(doc_as_dom, body_id)));

    set_render_scope(scope.clone());

    let root = {
        let mut scope_ref = scope.borrow_mut();
        app(&mut scope_ref)
    };

    web_doc
        .borrow_mut()
        .append_child(body_id, root.node_id());

    clear_render_scope();

    if let Some(css) = rinch_core::get_current_theme_css() {
        web_doc.borrow().inject_style(&css);
    }

    setup_event_delegation(&web_doc.borrow());

    let doc_for_signal = web_doc.clone();
    rinch_core::set_on_signal_change(move || {
        if let Some(css) = rinch_core::get_current_theme_css() {
            doc_for_signal.borrow().update_theme_style(&css);
        }
    });

    std::mem::forget(scope);
    std::mem::forget(web_doc);

    log::info!("PlotWeb mounted");
}

fn main() {}
