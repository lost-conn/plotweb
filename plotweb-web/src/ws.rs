use plotweb_common::{BetaFeedback, BetaFeedbackReply};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

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
    let on_message = std::rc::Rc::new(on_message);
    do_connect(url, on_message, 1000);
}

fn do_connect(
    url: String,
    on_message: std::rc::Rc<dyn Fn(WsMessage)>,
    backoff_ms: i32,
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
    on_msg.forget();

    // onopen — reset backoff
    let on_open = Closure::wrap(Box::new(move |_: web_sys::Event| {
        log::info!("WebSocket connected");
    }) as Box<dyn FnMut(_)>);
    ws.set_onopen(Some(on_open.as_ref().unchecked_ref()));
    on_open.forget();

    // onclose — reconnect with backoff
    let url_for_close = url.clone();
    let on_message_for_close = on_message.clone();
    let on_close = Closure::wrap(Box::new(move |_: web_sys::CloseEvent| {
        let next_backoff = (backoff_ms * 2).min(30_000);
        let url2 = url_for_close.clone();
        let cb2 = on_message_for_close.clone();
        log::info!("WebSocket closed, reconnecting in {}ms", backoff_ms);
        let closure = Closure::once(move || {
            do_connect(url2, cb2, next_backoff);
        });
        web_sys::window()
            .unwrap()
            .set_timeout_with_callback_and_timeout_and_arguments_0(
                closure.as_ref().unchecked_ref(),
                backoff_ms,
            )
            .ok();
        closure.forget();
    }) as Box<dyn FnMut(_)>);
    ws.set_onclose(Some(on_close.as_ref().unchecked_ref()));
    on_close.forget();

    // onerror
    let on_err = Closure::wrap(Box::new(move |_: web_sys::Event| {
        log::warn!("WebSocket error");
    }) as Box<dyn FnMut(_)>);
    ws.set_onerror(Some(on_err.as_ref().unchecked_ref()));
    on_err.forget();
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
