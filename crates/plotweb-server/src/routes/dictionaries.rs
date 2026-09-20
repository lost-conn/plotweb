//! Bundled Hunspell dictionary assets.
//!
//! The en_US affix + word list are embedded in the binary (`include_bytes!`) rather
//! than read from disk at runtime, so the Docker/jkbase release image needs nothing
//! beside the executable — the build context already carries `dictionaries/` (see
//! that directory's README for provenance and license).
//!
//! Both endpoints are **public**: a dictionary is not user data, and requiring a
//! session would mean the spellchecker could not warm its cache before sign-in.
//!
//! The bytes for a given path never change — a newer word list ships as a new file
//! under a new client cache key (`dict/en_US/v1/...`), never as a mutation of this
//! one — so they are served immutable with a one-year `max-age`.

use axum::http::{StatusCode, header};
use axum::response::IntoResponse;

/// Affix file. Declares `SET UTF-8` on its first line; stored byte-identical to the
/// Debian `hunspell-en-us` original, so no transcoding happens here or on the client.
const EN_US_AFF: &[u8] = include_bytes!("../../dictionaries/en_US.aff");

/// Word list (~79k entries), likewise verbatim UTF-8.
const EN_US_DIC: &[u8] = include_bytes!("../../dictionaries/en_US.dic");

/// `text/plain` because that is what these are — line-oriented UTF-8 text the client
/// hands to `spellbook` as a `&str`.
const CONTENT_TYPE: &str = "text/plain; charset=utf-8";

/// One year, immutable. See the module docs for why this is safe.
const CACHE_CONTROL: &str = "public, max-age=31536000, immutable";

fn serve(bytes: &'static [u8]) -> impl IntoResponse {
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, CONTENT_TYPE),
            (header::CACHE_CONTROL, CACHE_CONTROL),
        ],
        bytes,
    )
}

/// GET /api/dictionaries/en_US.aff
pub async fn en_us_aff() -> impl IntoResponse {
    serve(EN_US_AFF)
}

/// GET /api/dictionaries/en_US.dic
pub async fn en_us_dic() -> impl IntoResponse {
    serve(EN_US_DIC)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The client passes these to `spellbook` as `&str`. If either ever stopped being
    /// UTF-8 the failure would surface as a dictionary that silently refuses to load
    /// in the browser, so assert it where it is cheap to see.
    #[test]
    fn bundled_files_are_utf8() {
        let aff = std::str::from_utf8(EN_US_AFF).expect("aff is UTF-8");
        assert!(
            aff.lines().next() == Some("SET UTF-8"),
            "aff must declare SET UTF-8 (first line was {:?})",
            aff.lines().next()
        );
        let dic = std::str::from_utf8(EN_US_DIC).expect("dic is UTF-8");
        // First line of a .dic is the entry count.
        assert!(
            dic.lines()
                .next()
                .is_some_and(|l| l.trim().parse::<u32>().is_ok()),
            "dic must start with an entry count"
        );
    }
}
