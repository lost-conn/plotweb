use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;
use tokio::sync::RwLock;

/// Cached font list with timestamp.
struct FontCache {
    fonts: Vec<FontInfo>,
    fetched_at: std::time::Instant,
}

static FONT_CACHE: LazyLock<RwLock<Option<FontCache>>> = LazyLock::new(|| RwLock::new(None));

/// 24 hours cache duration.
const CACHE_DURATION: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

/// What we return to the client — slim per-font info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FontInfo {
    pub family: String,
    pub category: String,
    pub popularity: i64,
}

/// Shape of the Google Fonts metadata response (only fields we need).
#[derive(Deserialize)]
struct GoogleMetadataResponse {
    #[serde(rename = "familyMetadataList")]
    family_metadata_list: Vec<GoogleFontEntry>,
}

#[derive(Deserialize)]
struct GoogleFontEntry {
    family: String,
    category: Option<String>,
    popularity: Option<i64>,
}

async fn fetch_google_fonts() -> Result<Vec<FontInfo>, String> {
    let resp = reqwest::get("https://fonts.google.com/metadata/fonts")
        .await
        .map_err(|e| format!("fetch failed: {}", e))?;

    let metadata: GoogleMetadataResponse = resp
        .json()
        .await
        .map_err(|e| format!("parse failed: {}", e))?;

    let mut fonts: Vec<FontInfo> = metadata
        .family_metadata_list
        .into_iter()
        .map(|f| FontInfo {
            family: f.family,
            category: f.category.unwrap_or_default().to_lowercase().replace(' ', "-"),
            popularity: f.popularity.unwrap_or(9999),
        })
        .collect();

    // Sort by popularity (lower rank = more popular)
    fonts.sort_by_key(|f| f.popularity);
    Ok(fonts)
}

pub async fn list() -> impl IntoResponse {
    // Check cache
    {
        let cache = FONT_CACHE.read().await;
        if let Some(ref c) = *cache {
            if c.fetched_at.elapsed() < CACHE_DURATION {
                return (StatusCode::OK, Json(serde_json::to_value(&c.fonts).unwrap()));
            }
        }
    }

    // Fetch and cache
    match fetch_google_fonts().await {
        Ok(fonts) => {
            let value = serde_json::to_value(&fonts).unwrap();
            let mut cache = FONT_CACHE.write().await;
            *cache = Some(FontCache {
                fonts,
                fetched_at: std::time::Instant::now(),
            });
            (StatusCode::OK, Json(value))
        }
        Err(msg) => {
            // If fetch fails but we have stale cache, use it
            let cache = FONT_CACHE.read().await;
            if let Some(ref c) = *cache {
                return (StatusCode::OK, Json(serde_json::to_value(&c.fonts).unwrap()));
            }
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "error": msg })),
            )
        }
    }
}
