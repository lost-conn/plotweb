use serde::{Deserialize, Serialize};

// ── User ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub username: String,
    pub email: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub email: String,
    pub password: String,
}

// ── Font Settings ──

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct FontSettings {
    pub h1: Option<String>,
    pub h2: Option<String>,
    pub h3: Option<String>,
    pub body: Option<String>,
    pub quote: Option<String>,
    pub code: Option<String>,
}

// ── Book ──

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Book {
    pub id: String,
    pub title: String,
    pub description: String,
    pub created_at: String,
    pub updated_at: String,
    pub chapter_count: Option<i64>,
    pub font_settings: Option<FontSettings>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateBookRequest {
    pub title: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateBookRequest {
    pub title: Option<String>,
    pub description: Option<String>,
    pub font_settings: Option<FontSettings>,
}

// ── Chapter ──

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Chapter {
    pub id: String,
    pub book_id: String,
    pub title: String,
    pub content: String,
    pub sort_order: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateChapterRequest {
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateChapterRequest {
    pub title: Option<String>,
    pub content: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReorderChaptersRequest {
    pub chapter_ids: Vec<String>,
}

// ── Error ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiError {
    pub error: String,
}
