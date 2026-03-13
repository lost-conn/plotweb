pub mod web_document;
pub mod api;
pub mod store;
pub mod router;
pub mod pages;
pub mod components;
pub mod fonts;

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
    let browser_doc_for_click = browser_doc.clone();

    // Mousedown delegation
    let mousedown_closure = Closure::wrap(Box::new(move |event: web_sys::MouseEvent| {
        if let Some(target) = event.target()
            && let Ok(el) = target.dyn_into::<web_sys::Element>()
        {
            // Clear render surface focus if click is outside any surface
            if rinch::render_surface::focused_surface_id().is_some()
                && el.closest("[data-render-surface]").ok().flatten().is_none()
            {
                rinch::render_surface::set_focused_surface(None);
            }

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
                });

                event.prevent_default();
                events::dispatch_event(events::EventHandlerId(rid));
            }
        }
    }) as Box<dyn FnMut(_)>);
    browser_doc
        .add_event_listener_with_callback("mousedown", mousedown_closure.as_ref().unchecked_ref())
        .unwrap();
    mousedown_closure.forget();

    // Mousemove delegation
    let mousemove_closure = Closure::wrap(Box::new(move |event: web_sys::MouseEvent| {
        if rinch_core::update_drag(event.client_x() as f32, event.client_y() as f32) {
            event.prevent_default();
        }
    }) as Box<dyn FnMut(_)>);
    browser_doc
        .add_event_listener_with_callback("mousemove", mousemove_closure.as_ref().unchecked_ref())
        .unwrap();
    mousemove_closure.forget();

    // Mouseup delegation
    let mouseup_closure = Closure::wrap(Box::new(move |_event: web_sys::MouseEvent| {
        rinch_core::Drag::cancel();
    }) as Box<dyn FnMut(_)>);
    browser_doc
        .add_event_listener_with_callback("mouseup", mouseup_closure.as_ref().unchecked_ref())
        .unwrap();
    mouseup_closure.forget();

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
        } else if event.key() == "Enter" {
            if let Some(target) = event.target()
                && let Ok(el) = target.dyn_into::<web_sys::Element>()
            {
                let mut current: Option<web_sys::Element> = Some(el);
                while let Some(el) = current {
                    if let Some(handler_str) = el.get_attribute("data-onsubmit")
                        && let Ok(handler_id) = handler_str.parse::<usize>()
                    {
                        events::dispatch_event(events::EventHandlerId(handler_id));
                        break;
                    }
                    current = el.parent_element();
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

    // Check URL hash for #theme to allow unauthenticated theme preview
    let is_theme_preview = web_sys::window()
        .and_then(|w| w.location().hash().ok())
        .is_some_and(|h| h == "#theme");

    if is_theme_preview {
        store.current_route.set(Route::ThemePreview);
        store.loading.set(false);
    } else {
        // Check session on start
        wasm_bindgen_futures::spawn_local(async move {
            match api::get::<plotweb_common::User>("/api/auth/me").await {
                Ok(user) => {
                    store.current_user.set(Some(user));
                    store.current_route.set(Route::Dashboard);
                }
                Err(_) => {
                    store.current_route.set(Route::Login);
                }
            }
            store.loading.set(false);
        });
    }

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
