use rinch::prelude::*;
use rinch_core::use_store;
use crate::store::AppStore;

const PREVIEW_CSS: &str = r#"
.theme-preview {
    max-width: 960px;
    margin: 0 auto;
    padding-bottom: 60px;
}

.theme-preview h1 {
    margin-bottom: 8px;
}

.swatch-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(140px, 1fr));
    gap: 12px;
    margin-bottom: 32px;
}

.swatch {
    border-radius: var(--rinch-radius-xs);
    border: 1px solid var(--rinch-color-border);
    overflow: hidden;
}

.swatch-color {
    height: 60px;
}

.swatch-label {
    padding: 6px 8px;
    font-size: 12px;
    color: var(--rinch-color-dimmed);
    background: var(--rinch-color-surface);
}

.typography-section {
    margin-bottom: 32px;
}

.typography-section p {
    margin-bottom: 8px;
    line-height: 1.7;
}

.component-row {
    display: flex;
    flex-wrap: wrap;
    gap: 12px;
    align-items: center;
    margin-bottom: 16px;
}

.editor-mockup {
    background: var(--rinch-color-surface);
    border: 1px solid var(--rinch-color-border);
    border-radius: var(--rinch-radius-xs);
    padding: 24px;
    margin-bottom: 32px;
    font-size: 16px;
    line-height: 1.8;
    color: var(--rinch-color-text);
    outline: none;
    min-height: 200px;
}

.editor-mockup blockquote {
    border-left: 3px solid var(--rinch-color-teal-8);
    padding-left: 16px;
    margin: 16px 0;
    color: var(--rinch-color-dimmed);
}

.editor-mockup pre {
    background: var(--pw-color-deep);
    border: 1px solid var(--rinch-color-border);
    border-radius: var(--rinch-radius-xs);
    padding: 12px 16px;
    margin: 16px 0;
    font-family: monospace;
    font-size: 14px;
}

.editor-mockup code {
    background: var(--rinch-color-surface);
    border: 1px solid var(--rinch-color-border);
    padding: 2px 5px;
    border-radius: 3px;
    font-size: 0.9em;
    color: var(--rinch-color-teal-4);
}

.editor-mockup pre code {
    background: none;
    border: none;
    padding: 0;
    color: inherit;
}

.editor-mockup ul {
    margin: 8px 0;
    padding-left: 24px;
}

.editor-mockup li {
    margin: 4px 0;
}
"#;

#[component]
pub fn theme_preview_page() -> NodeHandle {
    rsx! {
        Fragment {
            style { {PREVIEW_CSS} }
            div { class: "theme-preview",
                // ── Header with dark mode toggle ──
                Title { order: 1, "Theme Preview" }
                Space { h: "xs" }
                div { class: "component-row",
                    Button {
                        variant: "outline",
                        onclick: move || {
                            let store = use_store::<AppStore>();
                            store.dark_mode.update(|d| *d = !*d);
                        },
                        {|| {
                            let store = use_store::<AppStore>();
                            if store.dark_mode.get() { "Switch to Light Mode" } else { "Switch to Dark Mode" }
                        }}
                    }
                }
                Space { h: "md" }
                Title { order: 3, "Color Swatches" }
                Space { h: "xs" }
                div { class: "swatch-grid",
                    // Semantic colors
                    div { class: "swatch",
                        div { class: "swatch-color", style: "background: var(--rinch-color-body);" }
                        div { class: "swatch-label", "body" }
                    }
                    div { class: "swatch",
                        div { class: "swatch-color", style: "background: var(--rinch-color-surface);" }
                        div { class: "swatch-label", "surface" }
                    }
                    div { class: "swatch",
                        div { class: "swatch-color", style: "background: var(--rinch-color-text);" }
                        div { class: "swatch-label", "text" }
                    }
                    div { class: "swatch",
                        div { class: "swatch-color", style: "background: var(--rinch-color-dimmed);" }
                        div { class: "swatch-label", "dimmed" }
                    }
                    div { class: "swatch",
                        div { class: "swatch-color", style: "background: var(--rinch-color-border);" }
                        div { class: "swatch-label", "border" }
                    }
                    div { class: "swatch",
                        div { class: "swatch-color", style: "background: var(--rinch-color-placeholder);" }
                        div { class: "swatch-label", "placeholder" }
                    }
                    div { class: "swatch",
                        div { class: "swatch-color", style: "background: var(--pw-color-deep);" }
                        div { class: "swatch-label", "deep" }
                    }
                    div { class: "swatch",
                        div { class: "swatch-color", style: "background: var(--pw-color-deepest);" }
                        div { class: "swatch-label", "deepest" }
                    }
                    // Teal accent shades
                    div { class: "swatch",
                        div { class: "swatch-color", style: "background: var(--rinch-color-teal-1);" }
                        div { class: "swatch-label", "teal-1" }
                    }
                    div { class: "swatch",
                        div { class: "swatch-color", style: "background: var(--rinch-color-teal-4);" }
                        div { class: "swatch-label", "teal-4" }
                    }
                    div { class: "swatch",
                        div { class: "swatch-color", style: "background: var(--rinch-color-teal-6);" }
                        div { class: "swatch-label", "teal-6" }
                    }
                    div { class: "swatch",
                        div { class: "swatch-color", style: "background: var(--rinch-color-teal-8);" }
                        div { class: "swatch-label", "teal-8" }
                    }
                    div { class: "swatch",
                        div { class: "swatch-color", style: "background: var(--rinch-color-teal-9);" }
                        div { class: "swatch-label", "teal-9" }
                    }
                }

                // ── Typography ──
                Title { order: 3, "Typography" }
                Space { h: "xs" }
                div { class: "typography-section",
                    Title { order: 1, "Heading 1 — Macondo Swash Caps" }
                    Title { order: 2, "Heading 2 — Macondo Swash Caps" }
                    Title { order: 3, "Heading 3 — Macondo Swash Caps" }
                    Title { order: 4, "Heading 4 — Macondo Swash Caps" }
                    Title { order: 5, "Heading 5 — Macondo Swash Caps" }
                    Title { order: 6, "Heading 6 — Macondo Swash Caps" }
                    Space { h: "sm" }
                    Text { "This is body text in Andada Pro. It should feel warm and readable, suitable for long-form fiction writing. The quick brown fox jumps over the lazy dog." }
                    Space { h: "xs" }
                    Text { "Here is some " }
                    Text { weight: "700", "bold text" }
                    Text { ", some " }
                    Text { style: "font-style: italic;", "italic text" }
                    Text { ", and " }
                    Text { size: "sm", color: "dimmed", "dimmed small text" }
                    Text { "." }
                }

                // ── Components ──
                Title { order: 3, "Components" }
                Space { h: "xs" }
                div { class: "component-row",
                    Button { variant: "filled", "Filled" }
                    Button { variant: "outline", "Outline" }
                    Button { variant: "subtle", "Subtle" }
                    Button { variant: "light", "Light" }
                }
                div { class: "component-row",
                    Badge { "Default" }
                    Badge { variant: "outline", "Outline" }
                    Badge { color: "red", "Red" }
                    Badge { color: "green", "Green" }
                }
                Space { h: "sm" }
                TextInput { label: "Text Input", placeholder: "Type something..." }
                Space { h: "sm" }
                Alert { title: "Success", color: "green", "This is a success alert." }
                Space { h: "xs" }
                Alert { title: "Error", color: "red", "This is an error alert." }
                Space { h: "sm" }

                Paper { shadow: "sm", p: "md",
                    Title { order: 4, "Card / Paper" }
                    Space { h: "xs" }
                    Text { color: "dimmed", "This is a Paper component with shadow and padding, sitting on the surface color." }
                }

                // ── Editor Mockup ──
                Space { h: "lg" }
                Title { order: 3, "Editor Mockup" }
                Space { h: "xs" }
                div {
                    class: "editor-mockup",
                    contenteditable: "true",
                    h2 { "Chapter One" }
                    p { "The evening sun cast long shadows across the cobblestone streets. She adjusted her coat and stepped into the alley, where the scent of " em { "old books" } " and " strong { "rain-soaked stone" } " hung in the air." }
                    blockquote { p { "\"Not everything that glitters is gold,\" he whispered, \"but some things shine brighter in the dark.\"" } }
                    pre { code { "fn main() {\n    println!(\"Hello, PlotWeb!\");\n}" } }
                    ul {
                        li { "Plot point A — the discovery" }
                        li { "Plot point B — the confrontation" }
                        li { "Plot point C — the resolution" }
                    }
                }
            }
        }
    }
}
