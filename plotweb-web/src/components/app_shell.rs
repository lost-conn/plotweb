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

/// Paragraphs of the live chapter/note this rescue belongs to, if it is open.
///
/// The comparison base. Deliberately *only* what is already loaded: a rescue viewer
/// that fetches is a viewer that can fail, and this has to work on a device that cannot
/// reach the server at all — which is the device most likely to be holding one.
fn live_paragraphs(store: &AppStore, doc_id: &str) -> Vec<String> {
    match store.open_body.get() {
        Some((open_id, text)) if open_id == doc_id => paragraphs_of(&text),
        // A different chapter is open, or none. Comparing against the wrong document
        // would mark every paragraph as missing, which is worse than not comparing.
        _ => Vec::new(),
    }
}

/// Split a stored `DocNode` JSON (or legacy HTML) into comparable paragraphs.
fn paragraphs_of(content: &str) -> Vec<String> {
    rescue_preview(content)
        .split("\n\n")
        .map(|p| p.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|p| !p.is_empty())
        .collect()
}

/// The rescued copy as rows: each paragraph, and whether it is missing from the live
/// document. `None` for the flag when there is nothing to compare against.
fn rescue_rows(store: &AppStore) -> Vec<(String, bool)> {
    let Some(json) = store.rescue_text.get() else {
        return Vec::new();
    };
    let live = store
        .rescue_open
        .get()
        .map(|(doc_id, _)| live_paragraphs(store, &doc_id))
        .unwrap_or_default();
    paragraphs_of(&json)
        .into_iter()
        .map(|para| {
            let only_here = !live.is_empty() && !live.iter().any(|l| l.contains(para.as_str()));
            (para, only_here)
        })
        .collect()
}

/// Plain text out of stored content, for a read-only preview.
///
/// Handles both shapes on purpose: the editor's `DocNode` JSON, and the legacy
/// HTML/Markdown that predates it and is still in older chapters. A comparison base
/// that silently returned nothing for legacy content would report "nothing to compare
/// against" for exactly the oldest, most valuable chapters.
fn rescue_preview(content: &str) -> String {
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
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(content) {
        let mut out = String::new();
        walk(&v, &mut out);
        return out.trim().to_string();
    }
    // Legacy: block tags become paragraph breaks, everything else is stripped.
    let mut out = String::new();
    let mut in_tag = false;
    let mut tag = String::new();
    for c in content.chars() {
        match c {
            '<' => {
                in_tag = true;
                tag.clear();
            }
            '>' => {
                in_tag = false;
                let name = tag.trim_start_matches('/').trim();
                if name.starts_with('p')
                    || name.starts_with("br")
                    || name.starts_with('h')
                    || name.starts_with("div")
                    || name.starts_with("li")
                {
                    out.push_str("\n\n");
                }
            }
            _ if in_tag => tag.push(c),
            _ => out.push(c),
        }
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
                    style: "padding: 6px 18px 0; font-size: 13px; color: var(--rinch-color-dimmed);",
                    {move || {
                        let Some(json) = store.rescue_text.get() else {
                            return String::new();
                        };
                        let Some((doc_id, _)) = store.rescue_open.get() else {
                            return String::new();
                        };
                        let live = live_paragraphs(&store, &doc_id);
                        if live.is_empty() {
                            return "The chapter this came from is not open, so there is \
                                    nothing to compare it against — every paragraph is shown."
                                .to_string();
                        }
                        let only_here = paragraphs_of(&json)
                            .into_iter()
                            .filter(|p| !live.iter().any(|l| l.contains(p.as_str())))
                            .count();
                        match only_here {
                            0 => "Everything in this copy is already in the chapter — \
                                  nothing here is missing from it."
                                .to_string(),
                            1 => "1 paragraph here is not in the chapter (highlighted).".to_string(),
                            n => format!("{n} paragraphs here are not in the chapter (highlighted)."),
                        }
                    }}
                }

                div {
                    style: "padding: 12px 18px 16px; overflow-y: auto; line-height: 1.55; flex: 1;",
                    // A reactive node swap has to be a match in the macro — a closure
                    // returning a node renders as its Debug text here.
                    match store.rescue_text.get() {
                        None => div { "Reading…" },
                        Some(json) if json.is_empty() => div {
                            "This copy could not be projected back to text. The bytes are still stored on this device."
                        },
                        Some(_) => div {
                            for (para, only_here) in rescue_rows(&store) {
                                div {
                                    key: para.clone(),
                                    // The only-here paragraphs are the reason this
                                    // viewer exists; the rest is context for them.
                                    style: {
                                        if only_here {
                                            "margin-bottom: 12px; padding: 6px 10px; border-left: 3px solid #C97B4A; background: rgba(201,123,74,0.12);"
                                        } else {
                                            "margin-bottom: 12px; padding: 6px 10px; opacity: 0.55;"
                                        }
                                    },
                                    {para}
                                }
                            }
                        },
                    }
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

/// Says plainly that this device is writing only to itself.
///
/// A cut-over book takes edits through sync; with sync off, a save reaches this
/// device's storage and stops there. That was true before this banner existed and
/// nothing said it, which is the shape of every loss in this arc: the app reporting
/// success for something that only half happened.
#[cfg(test)]
mod rescue_comparison {
    use super::*;

    /// The viewer's job is to show what the chapter is missing, not to dump text. A
    /// rescue whose every paragraph is already in the chapter has nothing to act on;
    /// one that carries a paragraph the chapter lacks is the whole reason it was kept.
    #[test]
    fn a_paragraph_the_chapter_lacks_is_the_one_worth_showing() {
        let doc = r#"{"type":"doc","content":[
            {"type":"paragraph","content":[{"type":"text","text":"The bus section is dug in."}]},
            {"type":"paragraph","content":[{"type":"text","text":"He looks up at the night sky."}]}
        ]}"#;
        let paras = paragraphs_of(doc);
        assert_eq!(paras.len(), 2);

        // The chapter as the server has it — it stops before the second paragraph.
        let live = paragraphs_of(
            r#"{"type":"doc","content":[
                {"type":"paragraph","content":[{"type":"text","text":"The bus section is dug in."}]}
            ]}"#,
        );

        let missing: Vec<&String> = paras
            .iter()
            .filter(|p| !live.iter().any(|l| l.contains(p.as_str())))
            .collect();
        assert_eq!(missing.len(), 1);
        assert!(missing[0].contains("night sky"));
    }

    /// Legacy chapters store HTML, not `DocNode` JSON. A comparison base that returned
    /// nothing for those would report "nothing to compare against" for exactly the
    /// oldest chapters — which is where a rescue matters most.
    #[test]
    fn legacy_html_content_still_yields_paragraphs() {
        let paras = paragraphs_of("<p>First paragraph.</p><p>Second paragraph.</p>");
        assert_eq!(paras.len(), 2, "got {paras:?}");
        assert!(paras[0].contains("First paragraph."));
        assert!(paras[1].contains("Second paragraph."));
    }

    /// Whitespace differences are not content differences — the projection and the
    /// stored copy will not agree on them, and a viewer that highlighted every
    /// paragraph would be no more useful than the raw dump it replaced.
    #[test]
    fn spacing_alone_is_not_a_difference() {
        let a = paragraphs_of(
            r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"One   two\n three"}]}]}"#,
        );
        let b = paragraphs_of(
            r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"One two three"}]}]}"#,
        );
        assert_eq!(a, b, "paragraphs compare on words, not on spacing");
    }
}

#[component]
/// Shown when a cut-over book is open on a device where sync cannot carry it.
///
/// Cutover now switches sync on by itself, so this is reachable only where the flag
/// says `plotweb_sync=0` — the kill switch. It stays because that state still has to
/// be visible: a save that reaches nothing must not look like any other save.
fn sync_off_banner() -> NodeHandle {
    rsx! {
        div {
            style: "background: #4A3B22; color: #FFF4DF; padding: 8px 16px; font-size: 13px; display: flex; align-items: center; gap: 10px;",
            span { "This book syncs, and sync is switched off on this device — anything you write here stays on this device until it is turned back on." }
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
            if store
                .current_book
                .get()
                .is_some_and(|b| b.cutover && !crate::sync::enabled_for_book(&b.id))
            {
                {sync_off_banner(__scope)}
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
