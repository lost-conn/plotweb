use plotweb_common::FontSettings;
use rinch_core::Signal;
use serde::{Deserialize, Serialize};

/// A font entry from the Google Fonts catalog.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FontInfo {
    pub family: String,
    pub category: String,
    pub popularity: i64,
}

// Global signal holding the fetched font catalog. Initialized empty; populated
// by `fetch_font_catalog()`.
//
// Created `unowned` on purpose. A `thread_local!` initializes on first touch, and
// the first touch is `fetch_font_catalog()` inside the book page's render — so a
// plain `Signal::new` here was attributed to *that page's* scope, not the app.
// Leaving the book disposed the scope and freed the signal while this handle
// lived on; re-entering any book then read the dead slot and rinch panicked
// (`Signal::get() on a freed signal`), which blanked the page. App lifetime is
// the ownership this cache always meant to have.
thread_local! {
    static FONT_CATALOG: Signal<Vec<FontInfo>> =
        rinch_core::reactive::unowned(|| Signal::new(Vec::new()));
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
    crate::api::get::<Vec<FontInfo>>("/api/fonts", move |result| {
        if let Ok(fonts) = result {
            catalog.set(fonts);
        }
    });
}

/// The app's own typefaces, carried inside the desktop binary.
///
/// On the web `index.html` loads these from Google Fonts; the native shell has
/// no web-font loader (no `<link>`, no `@font-face`), so without them every
/// `font-family` in the theme and `--pw-font-*` stacks fell through to whatever
/// fontconfig had. rinch registers them before the first layout
/// ([`rinch::App::fonts`]), so the first frame is already in the right face.
///
/// Each is reachable by the family name in its file — which matches the CSS
/// names exactly — and claims no generic slot: every stack names its face first,
/// and a script face answering `cursive` or a text face answering `serif` would
/// change elements that never asked for it. Variable files cover the weights the
/// web build requests (Playwrite 100–400, Source Sans 3 400–600).
///
/// Kept in `plotweb-web/fonts/`, not `assets/`, so Trunk's `copy-dir assets`
/// doesn't ship ~1 MB of files the web build never uses. Licences (OFL) sit next
/// to them.
#[cfg(not(target_arch = "wasm32"))]
pub const APP_FONTS: &[rinch::font::AppFont] = &[
    rinch::font::AppFont::new(include_bytes!("../fonts/PlaywriteDEGrund.ttf")),
    rinch::font::AppFont::new(include_bytes!("../fonts/MacondoSwashCaps-Regular.ttf")),
    rinch::font::AppFont::new(include_bytes!("../fonts/SourceSans3.ttf")),
];

/// Fonts already loaded globally — skip these when building the dynamic link.
#[cfg(target_arch = "wasm32")]
const GLOBAL_FONTS: &[&str] = &["Macondo Swash Caps", "Playwrite DE Grund"];

/// Build a Google Fonts CSS URL for the given font names. Web-only (only the DOM
/// `<link>` injectors use it).
#[cfg(target_arch = "wasm32")]
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

/// Make the book's Typography fonts available.
///
/// Web: injects a Google Fonts `<link>` into `<head>` (replacing the previous one).
/// Desktop: there is no DOM, so each family is downloaded as TTF and registered
/// with rinch's text stack (see [`native`]); text already on screen re-shapes in
/// it a frame later.
#[cfg(not(target_arch = "wasm32"))]
pub fn load_book_fonts(settings: &FontSettings) {
    for slot in [
        &settings.h1, &settings.h2, &settings.h3,
        &settings.body, &settings.quote, &settings.code,
    ] {
        if let Some(name) = slot {
            native::ensure(name);
        }
    }
}

#[cfg(target_arch = "wasm32")]
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

/// Load a single font for preview purposes (adds a link tag if not already present;
/// on desktop, downloads and registers it — see [`load_book_fonts`]).
#[cfg(not(target_arch = "wasm32"))]
pub fn load_single_font(family: &str) {
    native::ensure(family);
}

/// Desktop loading of Google Fonts families chosen in the Typography panel.
///
/// The browser gets these from a CSS `<link>`; rinch's native text stack has no
/// web-font loader, so we do its job: ask the Google Fonts CSS API for the family
/// (a non-browser user agent is served plain `.ttf` URLs, one static file per
/// style), download each file and hand it to `rinch::font::register_font`, which
/// adds it to every window's font context and repaints text already drawn in a
/// fallback.
///
/// Downloads are kept under `<data dir>/fonts/<family>/` so a book opened offline
/// (the local-first case) still has its faces; a `complete` marker is written only
/// once every file of a family arrived, so a half-finished download is retried.
/// Each family is requested at most once per run; a failed request forgets it so a
/// later call (reopening the book) tries again.
#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::cell::RefCell;
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};
    use std::rc::Rc;

    use rinch::font::{register_font, AppFont};
    use rinch_http::{fetch, Request};

    /// Families already in the binary ([`super::APP_FONTS`]).
    const BUNDLED: &[&str] = &["Playwrite DE Grund", "Macondo Swash Caps", "Source Sans 3"];

    /// The styles asked for. Google answers with whichever of them the family
    /// has (a single-style display face gets just its regular).
    const STYLES: &str = "ital,wght@0,400;0,700;1,400;1,700";

    thread_local! {
        static REQUESTED: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
    }

    pub(super) fn ensure(family: &str) {
        let family = family.trim();
        if family.is_empty() || BUNDLED.contains(&family) {
            return;
        }
        if !REQUESTED.with(|r| r.borrow_mut().insert(family.to_string())) {
            return;
        }
        let dir = cache_dir(family);
        if let Some(faces) = dir.as_deref().and_then(read_cached) {
            faces.into_iter().for_each(register);
            return;
        }
        request_css(family.to_string(), dir, true);
    }

    fn forget(family: &str) {
        REQUESTED.with(|r| r.borrow_mut().remove(family));
    }

    fn register(bytes: Vec<u8>) {
        register_font(AppFont::from_data(bytes));
    }

    fn css_url(family: &str, with_styles: bool) -> String {
        let name = family.replace(' ', "+");
        if with_styles {
            format!("https://fonts.googleapis.com/css2?family={name}:{STYLES}")
        } else {
            format!("https://fonts.googleapis.com/css2?family={name}")
        }
    }

    /// Fetch the family's CSS. A family whose axes reject the style list gets a
    /// second, plain request.
    fn request_css(family: String, dir: Option<PathBuf>, with_styles: bool) {
        fetch(Request::get(css_url(&family, with_styles)), move |res| {
            let urls = match res {
                Ok(r) if r.status == 200 => ttf_urls(&r.text()),
                Ok(r) if with_styles && r.status == 400 => {
                    return request_css(family, dir, false);
                }
                Ok(r) => {
                    log::warn!("fonts: {family}: Google Fonts answered HTTP {}", r.status);
                    Vec::new()
                }
                Err(e) => {
                    log::warn!("fonts: {family}: {e}");
                    Vec::new()
                }
            };
            if urls.is_empty() {
                forget(&family);
                return;
            }
            download(family, dir, urls);
        });
    }

    /// Download every file, registering each as it lands; cache the set once all
    /// of them arrived.
    fn download(family: String, dir: Option<PathBuf>, urls: Vec<String>) {
        struct Batch {
            family: String,
            dir: Option<PathBuf>,
            remaining: usize,
            failed: bool,
            files: Vec<Vec<u8>>,
        }
        let batch = Rc::new(RefCell::new(Batch {
            family,
            dir,
            remaining: urls.len(),
            failed: false,
            files: Vec::new(),
        }));
        for url in urls {
            let batch = batch.clone();
            fetch(Request::get(url), move |res| {
                let mut b = batch.borrow_mut();
                match res {
                    Ok(r) if r.status == 200 && is_font(&r.body) => {
                        register(r.body.clone());
                        b.files.push(r.body);
                    }
                    other => {
                        log::warn!("fonts: {}: a file failed: {:?}", b.family, other.map(|r| r.status));
                        b.failed = true;
                    }
                }
                b.remaining -= 1;
                if b.remaining > 0 {
                    return;
                }
                if b.failed && b.files.is_empty() {
                    forget(&b.family);
                } else if !b.failed {
                    if let Some(dir) = &b.dir {
                        if let Err(e) = write_cache(dir, &b.files) {
                            log::warn!("fonts: {}: could not cache: {e}", b.family);
                        }
                    }
                }
            });
        }
    }

    /// The `.ttf` URLs in a Google Fonts stylesheet, in order, without repeats
    /// (a family's subsets can share a file).
    pub(super) fn ttf_urls(css: &str) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for part in css.split("url(").skip(1) {
            let Some(end) = part.find(')') else { continue };
            let url = part[..end].trim_matches(|c| c == '\'' || c == '"');
            if url.starts_with("https://") && url.ends_with(".ttf") && !out.iter().any(|u| u == url) {
                out.push(url.to_string());
            }
        }
        out
    }

    /// TrueType, OpenType or a collection, by its first four bytes — so an error
    /// page served with a 200 is never handed to the text stack or cached.
    pub(super) fn is_font(bytes: &[u8]) -> bool {
        matches!(
            bytes.get(..4),
            Some([0, 1, 0, 0]) | Some(b"OTTO") | Some(b"true") | Some(b"ttcf")
        )
    }

    fn cache_dir(family: &str) -> Option<PathBuf> {
        let slug: String = family
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
            .collect();
        crate::platform::native_data_dir().map(|d| d.join("fonts").join(slug))
    }

    fn read_cached(dir: &Path) -> Option<Vec<Vec<u8>>> {
        if !dir.join("complete").is_file() {
            return None;
        }
        let mut faces = Vec::new();
        for entry in std::fs::read_dir(dir).ok()?.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "ttf") {
                if let Ok(bytes) = std::fs::read(&path) {
                    if is_font(&bytes) {
                        faces.push(bytes);
                    }
                }
            }
        }
        (!faces.is_empty()).then_some(faces)
    }

    fn write_cache(dir: &Path, files: &[Vec<u8>]) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        for (i, bytes) in files.iter().enumerate() {
            let tmp = dir.join(format!("{i}.ttf.part"));
            std::fs::write(&tmp, bytes)?;
            std::fs::rename(&tmp, dir.join(format!("{i}.ttf")))?;
        }
        std::fs::write(dir.join("complete"), b"")
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn picks_ttf_urls_in_order_without_repeats() {
            let css = "@font-face { src: url(https://fonts.gstatic.com/a.ttf) format('truetype'); }\n\
                       @font-face { src: url('https://fonts.gstatic.com/b.ttf'); }\n\
                       @font-face { src: url(https://fonts.gstatic.com/a.ttf); }\n\
                       @font-face { src: url(https://fonts.gstatic.com/c.woff2); }";
            assert_eq!(
                ttf_urls(css),
                ["https://fonts.gstatic.com/a.ttf", "https://fonts.gstatic.com/b.ttf"]
            );
        }

        #[test]
        fn recognises_font_files_only() {
            assert!(is_font(&[0, 1, 0, 0, 9]));
            assert!(is_font(b"OTTO...."));
            assert!(!is_font(b"<!DOCTYPE html>"));
            assert!(!is_font(b"ab"));
        }

        #[test]
        fn cache_round_trips_and_needs_the_marker() {
            let dir = std::env::temp_dir().join(format!("pw-font-cache-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            let face = vec![0u8, 1, 0, 0, 42];
            write_cache(&dir, &[face.clone()]).unwrap();
            assert_eq!(read_cached(&dir), Some(vec![face]));
            std::fs::remove_file(dir.join("complete")).unwrap();
            assert_eq!(read_cached(&dir), None, "no marker: download again");
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}

#[cfg(target_arch = "wasm32")]
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
