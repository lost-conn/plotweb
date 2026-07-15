//! Phase-0 spike: mount rinch's model-first rich-text editor
//! (`rinch-editor-view` `Editor`/`EditorHandle`, re-exported by `rinch-web`) inside
//! the PlotWeb web build — **no contenteditable**, no `set_inner_html`. Reachable
//! at `/editor-spike`. Loads a sample chapter (PlotWeb markdown → HTML → the
//! editor's `load_html`), and drives formatting through `handle.command(...)` — the
//! same API desktop uses. This validates the editor swap before Phase 1.
//!
//! Not wired into auth/nav; it's a dev surface to eyeball parity (marks, headings,
//! lists, blockquote, code, undo/redo). Text-alignment has no editor command yet
//! (schema gap — see the plan doc); links/images need arg-based handle calls.

use rinch::prelude::*;
use crate::rinch_backend::{Editor, create_editor};

use crate::pages::editor_utils::markdown_to_html;

const SAMPLE_MD: &str = "# Chapter One\n\nThe lantern **guttered** against the *fog* while the harbour bell counted out the hours. She pressed on, `notebook` in hand.\n\n## A short list\n\n- First beat of the scene\n- Second beat, with **weight**\n- Third\n\n1. Rising\n2. Falling\n\n> \"Everything flows through the same invertible steps as desktop.\"\n\nA final paragraph with ~~struck~~ text and a closing line.\n";

const SPIKE_CSS: &str = r#"
.editor-spike { max-width: 820px; margin: 0 auto; padding: 24px; }
.editor-spike-toolbar { display: flex; flex-wrap: wrap; gap: 6px; margin: 12px 0; padding: 8px; border: 1px solid var(--rinch-color-border); border-radius: 8px; }
.editor-spike-toolbar button { padding: 4px 10px; border: 1px solid var(--rinch-color-border); background: var(--rinch-color-surface); color: var(--rinch-color-text); border-radius: 6px; cursor: pointer; font-size: 13px; }
.editor-spike-host { border: 1px solid var(--rinch-color-border); border-radius: 8px; padding: 8px 12px; min-height: 320px; background: var(--rinch-color-body); }
"#;

#[component]
pub fn editor_spike_page() -> NodeHandle {
    // One handle drives the toolbar and the mounted editor (cheap Rc clone).
    let editor = create_editor();
    let (bold, italic, underline, strike, code, hl) = (
        editor.clone(), editor.clone(), editor.clone(),
        editor.clone(), editor.clone(), editor.clone(),
    );
    let (h1, h2, para, ul, ol, task) = (
        editor.clone(), editor.clone(), editor.clone(),
        editor.clone(), editor.clone(), editor.clone(),
    );
    let (quote, codeblock, hr, undo, redo) = (
        editor.clone(), editor.clone(), editor.clone(),
        editor.clone(), editor.clone(),
    );

    let content = markdown_to_html(SAMPLE_MD);

    rsx! {
        Fragment {
            style { {SPIKE_CSS} }
            div { class: "editor-spike",
                h2 { "Editor spike — rinch-editor-view (model-first, no contenteditable)" }
                p { style: "color: var(--rinch-color-dimmed);",
                    "Loaded from PlotWeb markdown via markdown_to_html → the editor's load_html."
                }
                div { class: "editor-spike-toolbar", id: "spike-toolbar",
                    button { id: "sp-bold",      onclick: move || { bold.command("toggleBold"); },            "B" }
                    button { id: "sp-italic",    onclick: move || { italic.command("toggleItalic"); },        "I" }
                    button { id: "sp-underline", onclick: move || { underline.command("toggleUnderline"); },  "U" }
                    button { id: "sp-strike",    onclick: move || { strike.command("toggleStrike"); },        "S" }
                    button { id: "sp-code",      onclick: move || { code.command("toggleCode"); },            "Code" }
                    button { id: "sp-hl",        onclick: move || { hl.command("toggleHighlight"); },         "Highlight" }
                    button { id: "sp-h1",        onclick: move || { h1.command("setHeading1"); },             "H1" }
                    button { id: "sp-h2",        onclick: move || { h2.command("setHeading2"); },             "H2" }
                    button { id: "sp-p",         onclick: move || { para.command("setParagraph"); },          "P" }
                    button { id: "sp-ul",        onclick: move || { ul.command("toggleBulletList"); },        "• List" }
                    button { id: "sp-ol",        onclick: move || { ol.command("toggleOrderedList"); },       "1. List" }
                    button { id: "sp-task",      onclick: move || { task.command("toggleTaskList"); },        "☑ Tasks" }
                    button { id: "sp-quote",     onclick: move || { quote.command("wrapInBlockquote"); },     "Quote" }
                    button { id: "sp-codeblock", onclick: move || { codeblock.command("setCodeBlock"); },     "Code block" }
                    button { id: "sp-hr",        onclick: move || { hr.command("insertHorizontalRule"); },    "—" }
                    button { id: "sp-undo",      onclick: move || { undo.command("undo"); },                  "Undo" }
                    button { id: "sp-redo",      onclick: move || { redo.command("redo"); },                  "Redo" }
                }
                div { class: "editor-spike-host", id: "spike-editor-host",
                    Editor {
                        editor: editor.clone(),
                        content: content,
                    }
                }
            }
        }
    }
}
