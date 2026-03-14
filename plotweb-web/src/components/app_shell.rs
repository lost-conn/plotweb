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
    height: 100vh;
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
    height: 100vh;
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

#[component]
pub fn app_shell() -> NodeHandle {
    let store = use_store::<AppStore>();

    rsx! {
        Fragment {
            style { {|| warm_overrides(store.dark_mode.get())} }
            style { {APP_SHELL_CSS} }

            match store.current_route.get() {
                Route::Login => div {
                    {crate::pages::login::login_page(__scope)}
                },
                Route::Register => div {
                    {crate::pages::register::register_page(__scope)}
                },
                Route::ThemePreview => div {
                    style: "height: 100vh; overflow-y: auto; padding: 40px 48px; background: var(--rinch-color-body);",
                    {crate::pages::theme_preview::theme_preview_page(__scope)}
                },
                Route::Dashboard => div {
                    style: "height: 100vh; display: flex; flex-direction: column; overflow: hidden;",
                    {crate::pages::dashboard::dashboard_page(__scope)}
                },
                Route::Book(_id) => div {
                    style: "height: 100vh; overflow: hidden;",
                    {crate::pages::book::book_page(__scope, _id)}
                },
                Route::Reader(_token) => div {
                    style: "height: 100vh; overflow: hidden;",
                    {crate::pages::reader::reader_page(__scope, _token)}
                },
            }
        }
    }
}
