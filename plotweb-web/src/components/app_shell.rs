use rinch::prelude::*;
use rinch_core::use_store;

use crate::store::{AppStore, Route};

/// Returns a `:root { ... }` CSS block with warm color overrides for the given mode.
fn warm_overrides(dark: bool) -> String {
    let font = "--rinch-font-family: 'Playwrite DE Grund', Georgia, 'Times New Roman', serif;";
    if dark {
        format!(":root {{
  {font}
  --rinch-color-body: #1C1917;
  --rinch-color-surface: #262220;
  --rinch-color-text: #E7E0D8;
  --rinch-color-dimmed: #9C9489;
  --rinch-color-border: #3D3733;
  --rinch-color-placeholder: #6B6359;
  --pw-color-deep: #1A1714;
  --pw-color-deepest: #14120F;
}}")
    } else {
        format!(":root {{
  {font}
  --rinch-color-body: #FAF8F5;
  --rinch-color-surface: #FFFFFF;
  --rinch-color-text: #2C2520;
  --rinch-color-dimmed: #8C8278;
  --rinch-color-border: #E0D8CF;
  --rinch-color-placeholder: #A89E94;
  --pw-color-deep: #F3F0EC;
  --pw-color-deepest: #EDE9E3;
}}")
    }
}

/// CSS for the app shell layout.
const APP_SHELL_CSS: &str = r#"
* {
    box-sizing: border-box;
    margin: 0;
    padding: 0;
}

html, body {
    height: 100dvh;
    font-family: var(--rinch-font-family);
    background: var(--rinch-color-body);
    color: var(--rinch-color-text);
    overflow: hidden;
    -webkit-font-smoothing: antialiased;
    -moz-osx-font-smoothing: grayscale;
}

h1, h2, h3, h4, h5, h6, .rinch-title {
    font-family: 'Macondo Swash Caps', cursive;
}

/* ── Auth Pages ────────────────────────────────────────── */

.auth-page {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100dvh;
    background: linear-gradient(
        160deg,
        var(--pw-color-deepest) 0%,
        var(--rinch-color-body) 50%,
        var(--pw-color-deep) 100%
    );
}

.auth-page .rinch-paper {
    border: 1px solid var(--rinch-color-border);
    background: var(--rinch-color-surface);
}

.auth-page h2 {
    color: var(--rinch-color-teal-4);
}

/* ── Scrollbar ─────────────────────────────────────────── */

::-webkit-scrollbar {
    width: 8px;
}

::-webkit-scrollbar-track {
    background: transparent;
}

::-webkit-scrollbar-thumb {
    background: var(--rinch-color-border);
    border-radius: 4px;
}

::-webkit-scrollbar-thumb:hover {
    background: var(--rinch-color-dimmed);
}

/* ── Selection ─────────────────────────────────────────── */

::selection {
    background: var(--rinch-color-teal-8);
    color: var(--rinch-color-teal-1);
}

/* ── Auth responsive ───────────────────────────────────── */

@media (max-width: 480px) {
    .auth-page .rinch-paper {
        width: 100% !important;
        margin: 0 16px;
    }
}
"#;

/// A label an author can recognise: the chapter's title if we know it, else the id.
fn rescue_label(store: &AppStore, doc_id: &str) -> String {
    if let Some(id) = doc_id.strip_prefix("chapter:") {
        if let Some(ch) = store.chapters.get().iter().find(|c| c.id == id) {
            return format!("chapter \"{}\"", ch.title);
        }
        return format!("a chapter ({})", &id[..id.len().min(8)]);
    }
    if let Some(id) = doc_id.strip_prefix("note:") {
        return format!("a note ({})", &id[..id.len().min(8)]);
    }
    doc_id.to_string()
}

/// Plain text out of the stored `DocNode` JSON, for a read-only preview.
fn rescue_preview(json: &str) -> String {
    fn walk(v: &serde_json::Value, out: &mut String) {
        match v {
            serde_json::Value::Object(map) => {
                if map.get("type").and_then(|t| t.as_str()) == Some("text") {
                    if let Some(t) = map.get("text").and_then(|t| t.as_str()) {
                        out.push_str(t);
                    }
                }
                if let Some(kids) = map.get("content") {
                    walk(kids, out);
                    // Paragraph boundaries the author will expect to see.
                    if map.get("type").and_then(|t| t.as_str()) == Some("paragraph") {
                        out.push_str("\n\n");
                    }
                }
            }
            serde_json::Value::Array(items) => items.iter().for_each(|i| walk(i, out)),
            _ => {}
        }
    }
    let mut out = String::new();
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(json) {
        walk(&v, &mut out);
    }
    out.trim().to_string()
}

/// Tells the author this device kept work the server replaced, and lets them read it.
///
/// The §D8 resolution used to discard that work silently; it is preserved now
/// (`local_store::preserve_local_copy`), but a rescue nothing surfaces is only
/// marginally better than no rescue at all — the 2026-08-29 loss was invisible until
/// the word count dropped the next morning.
#[component]
fn rescue_banner(store: AppStore) -> NodeHandle {
    let open_rescue = move |doc_id: String, slot: String| {
        move || {
            let (d, s) = (doc_id.clone(), slot.clone());
            store.rescue_open.set(Some((d.clone(), s.clone())));
            store.rescue_text.set(None);
            crate::local_store::spawn(async move {
                match crate::local_store::materialize_rescued_copy(&d, &s).await {
                    Ok(Some(json)) => store.rescue_text.set(Some(json)),
                    Ok(None) => store.rescue_text.set(Some(String::new())),
                    Err(e) => {
                        log::warn!("local-first: {d}: {e}");
                        store.rescue_text.set(Some(String::new()));
                    }
                }
            });
        }
    };

    rsx! {
        div {
            style: "background: #7C4A2D; color: #FFF6EC; padding: 10px 16px; display: flex; align-items: center; gap: 12px; font-size: 14px; flex-wrap: wrap;",

            span {
                {move || {
                    let n = store.rescued.get().len();
                    let what = if n == 1 { "a copy" } else { "copies" };
                    format!("This device kept {what} of work that never reached the server ({n}).")
                }}
            }

            for (doc_id, slot) in store.rescued.get() {
                button {
                    key: format!("{doc_id}/{slot}"),
                    style: "background: transparent; color: #FFF6EC; border: 1px solid #FFF6EC; border-radius: 4px; padding: 3px 10px; cursor: pointer; font-size: 13px;",
                    onclick: open_rescue(doc_id.clone(), slot.clone()),
                    {format!("View {}", rescue_label(&store, &doc_id))}
                }
            }
        }
    }
}

/// The rescued text, read-only, with the two things an author needs: to read it, and
/// to say they are done with it.
#[component]
fn rescue_viewer(store: AppStore) -> NodeHandle {
    let close_viewer = move || {
        store.rescue_open.set(None);
    };
    let discard = move || {
        let Some((doc_id, slot)) = store.rescue_open.get() else {
            return;
        };
        store.rescue_open.set(None);
        crate::local_store::spawn(async move {
            if let Err(e) = crate::local_store::discard_rescued_copy(&doc_id, &slot).await {
                log::warn!("local-first: {doc_id}: discard failed: {e}");
            }
            match crate::local_store::rescued_copies().await {
                Ok(found) => store.rescued.set(found),
                Err(e) => log::warn!("local-first: could not re-list rescued copies: {e}"),
            }
        });
    };

    rsx! {
        div {
            style: "position: fixed; inset: 0; background: rgba(0,0,0,0.55); z-index: 1000; display: flex; align-items: center; justify-content: center; padding: 24px;",

            div {
                style: "background: var(--rinch-color-surface); color: var(--rinch-color-text); border-radius: 8px; max-width: 760px; width: 100%; max-height: 80dvh; display: flex; flex-direction: column; overflow: hidden;",

                div {
                    style: "padding: 14px 18px; border-bottom: 1px solid var(--rinch-color-border);",
                    div { style: "font-weight: 600;", "Kept on this device" }
                    div {
                        style: "font-size: 13px; color: var(--rinch-color-dimmed); margin-top: 2px;",
                        "This text was in this browser when the server replaced the document. Copy anything you still want, then discard it."
                    }
                }

                div {
                    style: "padding: 16px 18px; overflow-y: auto; white-space: pre-wrap; line-height: 1.55; flex: 1;",
                    {move || match store.rescue_text.get() {
                        None => "Reading…".to_string(),
                        Some(json) if json.is_empty() => {
                            "This copy could not be projected back to text. The bytes are still stored on this device.".to_string()
                        }
                        Some(json) => rescue_preview(&json),
                    }}
                }

                div {
                    style: "padding: 12px 18px; border-top: 1px solid var(--rinch-color-border); display: flex; gap: 10px; justify-content: flex-end;",
                    button {
                        style: "padding: 6px 14px; cursor: pointer;",
                        onclick: close_viewer,
                        "Close"
                    }
                    button {
                        style: "padding: 6px 14px; cursor: pointer; color: #B3452B;",
                        onclick: discard,
                        "Discard this copy"
                    }
                }
            }
        }
    }
}

#[component]
pub fn app_shell() -> NodeHandle {
    let store = use_store::<AppStore>();

    rsx! {
        Fragment {
            style { {|| warm_overrides(store.dark_mode.get())} }
            style { {APP_SHELL_CSS} }

            if !store.rescued.get().is_empty() {
                {rescue_banner(__scope, store)}
            }
            if store.rescue_open.get().is_some() {
                {rescue_viewer(__scope, store)}
            }

            match store.current_route.get() {
                Route::Login => div {
                    {crate::pages::login::login_page(__scope)}
                },
                Route::Register => div {
                    {crate::pages::register::register_page(__scope)}
                },
                Route::ForgotPassword => div {
                    {crate::pages::forgot_password::forgot_password_page(__scope)}
                },
                Route::ResetPassword(_token) => div {
                    {crate::pages::reset_password::reset_password_page(__scope, _token)}
                },
                Route::ThemePreview => div {
                    style: "height: 100dvh; overflow-y: auto; padding: 40px 48px; background: var(--rinch-color-body);",
                    {crate::pages::theme_preview::theme_preview_page(__scope)}
                },
                Route::EditorSpike => div {
                    style: "height: 100dvh; overflow-y: auto; background: var(--rinch-color-body);",
                    {crate::pages::editor_spike::editor_spike_page(__scope)}
                },
                Route::OpfsSpike => div {
                    style: "height: 100dvh; overflow-y: auto; background: var(--rinch-color-body);",
                    {crate::pages::opfs_spike::opfs_spike_page(__scope)}
                },
                Route::Dashboard => div {
                    style: "height: 100dvh; display: flex; flex-direction: column; overflow: hidden;",
                    {crate::pages::dashboard::dashboard_page(__scope)}
                },
                Route::Book(_id) => div {
                    style: "height: 100dvh; overflow: hidden;",
                    {crate::pages::book::book_page(__scope, _id)}
                },
                Route::Reader(_token) => div {
                    style: "height: 100dvh; overflow: hidden;",
                    {crate::pages::reader::reader_page(__scope, _token)}
                },
                Route::ReaderPreview(_book_id) => div {
                    style: "height: 100dvh; overflow: hidden;",
                    {crate::pages::reader::reader_preview_page(__scope, _book_id)}
                },
            }
        }
    }
}
