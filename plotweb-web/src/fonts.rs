use plotweb_common::FontSettings;
use rinch_core::Signal;
use serde::{Deserialize, Serialize};

/// A font entry from the Google Fonts catalog.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FontInfo {
    pub family: String,
    pub category: String,
    pub popularity: i64,
}

/// Global signal holding the fetched font catalog.
/// Initialized empty; populated by `fetch_font_catalog()`.
thread_local! {
    static FONT_CATALOG: Signal<Vec<FontInfo>> = Signal::new(Vec::new());
}

pub fn font_catalog() -> Signal<Vec<FontInfo>> {
    FONT_CATALOG.with(|s| *s)
}

/// Fetch the font catalog from the server proxy and store it in the global signal.
pub fn fetch_font_catalog() {
    let catalog = font_catalog();
    // Don't re-fetch if already loaded
    if !catalog.get().is_empty() {
        return;
    }
    wasm_bindgen_futures::spawn_local(async move {
        if let Ok(fonts) = crate::api::get::<Vec<FontInfo>>("/api/fonts").await {
            catalog.set(fonts);
        }
    });
}

/// Fonts already loaded globally — skip these when building the dynamic link.
const GLOBAL_FONTS: &[&str] = &["Macondo Swash Caps", "Playwrite DE Grund"];

/// Build a Google Fonts CSS URL for the given font names.
pub fn google_fonts_url(fonts: &[&str]) -> String {
    let families: Vec<String> = fonts
        .iter()
        .map(|f| format!("family={}", f.replace(' ', "+")))
        .collect();
    format!(
        "https://fonts.googleapis.com/css2?{}&display=swap",
        families.join("&")
    )
}

/// Inject a <link> tag for the book's custom fonts into <head>.
/// Removes any previous book-fonts link first.
pub fn load_book_fonts(settings: &FontSettings) {
    let document = match web_sys::window().and_then(|w| w.document()) {
        Some(d) => d,
        None => return,
    };

    // Remove previous book-fonts link
    if let Ok(Some(old)) = document.query_selector("link[data-book-fonts]") {
        if let Some(parent) = old.parent_node() {
            parent.remove_child(&old).ok();
        }
    }

    // Collect unique fonts that aren't globally loaded
    let mut fonts: Vec<&str> = Vec::new();
    let slots: [&Option<String>; 6] = [
        &settings.h1, &settings.h2, &settings.h3,
        &settings.body, &settings.quote, &settings.code,
    ];
    for slot in &slots {
        if let Some(name) = slot {
            let name_str = name.as_str();
            if !GLOBAL_FONTS.contains(&name_str) && !fonts.contains(&name_str) {
                fonts.push(name_str);
            }
        }
    }

    if fonts.is_empty() {
        return;
    }

    let url = google_fonts_url(&fonts);
    if let Ok(link) = document.create_element("link") {
        link.set_attribute("rel", "stylesheet").ok();
        link.set_attribute("href", &url).ok();
        link.set_attribute("data-book-fonts", "").ok();
        if let Some(head) = document.head() {
            head.append_child(&link).ok();
        }
    }
}

/// Load a single font for preview purposes (adds a link tag if not already present).
pub fn load_single_font(family: &str) {
    if family.is_empty() || GLOBAL_FONTS.contains(&family) {
        return;
    }
    let document = match web_sys::window().and_then(|w| w.document()) {
        Some(d) => d,
        None => return,
    };
    // Check if already loaded via a data-preview-font link
    let selector = format!("link[data-preview-font='{}']", family);
    if let Ok(Some(_)) = document.query_selector(&selector) {
        return; // Already loaded
    }
    // Also check if loaded via book-fonts link (check href)
    if let Ok(Some(book_link)) = document.query_selector("link[data-book-fonts]") {
        if let Some(href) = book_link.get_attribute("href") {
            if href.contains(&family.replace(' ', "+")) {
                return;
            }
        }
    }
    let url = google_fonts_url(&[family]);
    if let Ok(link) = document.create_element("link") {
        link.set_attribute("rel", "stylesheet").ok();
        link.set_attribute("href", &url).ok();
        link.set_attribute("data-preview-font", family).ok();
        if let Some(head) = document.head() {
            head.append_child(&link).ok();
        }
    }
}
