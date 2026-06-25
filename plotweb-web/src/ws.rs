use plotweb_common::{BetaFeedback, BetaFeedbackReply};
use serde::{Deserialize, Serialize};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

/// Holds the live WebSocket plus its event closures so they stay alive without
/// `.forget()` (which leaked a fresh set of closures on every reconnect). When a
/// new connection is established, the previous `WsConn` is dropped, releasing the
/// old closures and socket.
struct WsConn {
    _ws: web_sys::WebSocket,
    _on_msg: Closure<dyn FnMut(web_sys::MessageEvent)>,
    _on_open: Closure<dyn FnMut(web_sys::Event)>,
    _on_close: Closure<dyn FnMut(web_sys::CloseEvent)>,
    _on_err: Closure<dyn FnMut(web_sys::Event)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WsMessage {
    NewFeedback(BetaFeedback),
    NewReply {
        feedback_id: String,
        reply: BetaFeedbackReply,
    },
    FeedbackResolved {
        feedback_id: String,
        resolved: bool,
    },
    FeedbackDeleted {
        feedback_id: String,
    },
}

/// Connect to a feedback WebSocket and call `on_message` for each incoming message.
/// Automatically reconnects with exponential backoff on disconnection.
pub fn connect_feedback_ws(
    url: &str,
    on_message: impl Fn(WsMessage) + 'static,
) {
    let url = url.to_string();
    let on_message = Rc::new(on_message);
    // Shared current-backoff cell: reset to the initial delay on every successful
    // open, and grown (exponential, capped) on each close.
    let backoff = Rc::new(Cell::new(1000));
    // Slot that owns the current connection's closures. Replacing its contents on
    // reconnect drops the previous closure set instead of leaking it via forget().
    let slot: Rc<RefCell<Option<WsConn>>> = Rc::new(RefCell::new(None));
    do_connect(url, on_message, backoff, slot);
}

fn do_connect(
    url: String,
    on_message: Rc<dyn Fn(WsMessage)>,
    backoff: Rc<Cell<i32>>,
    slot: Rc<RefCell<Option<WsConn>>>,
) {
    let ws = match web_sys::WebSocket::new(&url) {
        Ok(ws) => ws,
        Err(e) => {
            log::warn!("WebSocket connect failed: {:?}", e);
            return;
        }
    };
    ws.set_binary_type(web_sys::BinaryType::Arraybuffer);

    // onmessage
    let on_msg_cb = on_message.clone();
    let on_msg = Closure::wrap(Box::new(move |event: web_sys::MessageEvent| {
        if let Some(text) = event.data().as_string() {
            if let Ok(msg) = serde_json::from_str::<WsMessage>(&text) {
                on_msg_cb(msg);
            }
        }
    }) as Box<dyn FnMut(_)>);
    ws.set_onmessage(Some(on_msg.as_ref().unchecked_ref()));

    // onopen — reset backoff to the initial delay so a brief blip doesn't leave
    // us stuck at the maximum reconnect interval.
    let backoff_for_open = backoff.clone();
    let on_open = Closure::wrap(Box::new(move |_: web_sys::Event| {
        log::info!("WebSocket connected");
        backoff_for_open.set(1000);
    }) as Box<dyn FnMut(_)>);
    ws.set_onopen(Some(on_open.as_ref().unchecked_ref()));

    // onclose — reconnect with exponential backoff
    let url_for_close = url.clone();
    let on_message_for_close = on_message.clone();
    let backoff_for_close = backoff.clone();
    let slot_for_close = slot.clone();
    let on_close = Closure::wrap(Box::new(move |_: web_sys::CloseEvent| {
        // Current delay drives this reconnect; the next one doubles (capped).
        let cur = backoff_for_close.get();
        backoff_for_close.set((cur * 2).min(30_000));
        log::info!("WebSocket closed, reconnecting in {}ms", cur);
        let url2 = url_for_close.clone();
        let cb2 = on_message_for_close.clone();
        let backoff2 = backoff_for_close.clone();
        let slot2 = slot_for_close.clone();
        let closure = Closure::once(move || {
            do_connect(url2, cb2, backoff2, slot2);
        });
        web_sys::window()
            .unwrap()
            .set_timeout_with_callback_and_timeout_and_arguments_0(
                closure.as_ref().unchecked_ref(),
                cur,
            )
            .ok();
        closure.forget();
    }) as Box<dyn FnMut(_)>);
    ws.set_onclose(Some(on_close.as_ref().unchecked_ref()));

    // onerror
    let on_err = Closure::wrap(Box::new(move |_: web_sys::Event| {
        log::warn!("WebSocket error");
    }) as Box<dyn FnMut(_)>);
    ws.set_onerror(Some(on_err.as_ref().unchecked_ref()));

    // Take ownership of this connection's closures, dropping the previous set
    // (and its socket) instead of leaking them via forget().
    *slot.borrow_mut() = Some(WsConn {
        _ws: ws,
        _on_msg: on_msg,
        _on_open: on_open,
        _on_close: on_close,
        _on_err: on_err,
    });
}

/// Build the WebSocket URL from the current page location.
/// In dev mode (Trunk on :8080), connects directly to the API server on :3000
/// since Trunk's HTTP proxy doesn't support WebSocket upgrades.
pub fn ws_url(path: &str) -> String {
    let window = web_sys::window().unwrap();
    let location = window.location();
    let protocol = location.protocol().unwrap_or_default();
    let hostname = location.hostname().unwrap_or_default();
    let port = location.port().unwrap_or_default();
    let ws_proto = if protocol == "https:" { "wss:" } else { "ws:" };

    // If running on Trunk dev server (port 8080), route WS to API server (port 3000)
    let ws_port = if port == "8080" { "3000".to_string() } else { port };

    if ws_port.is_empty() {
        format!("{}//{}{}", ws_proto, hostname, path)
    } else {
        format!("{}//{}:{}{}", ws_proto, hostname, ws_port, path)
    }
}
