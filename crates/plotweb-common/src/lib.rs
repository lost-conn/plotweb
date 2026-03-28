use serde::{Deserialize, Deserializer, Serialize};

/// Custom deserializer for `Option<Option<T>>` fields in update requests.
/// Makes JSON `null` deserialize as `Some(None)` (meaning "set to null")
/// rather than `None` (meaning "don't update").
fn deserialize_double_option<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Ok(Some(Option::deserialize(deserializer)?))
}

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
    #[serde(default)]
    pub remember_me: bool,
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
    pub paragraph_spacing: Option<f64>,
    pub paragraph_indent: Option<f64>,
    pub heading_indent: Option<f64>,
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
    #[serde(default)]
    pub word_count: Option<u64>,
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
    #[serde(default)]
    pub word_count: u64,
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

// ── Beta Reader ──

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BetaReaderLink {
    pub id: String,
    pub book_id: String,
    pub token: String,
    pub reader_name: String,
    pub max_chapter_index: Option<i64>,
    pub active: bool,
    pub created_at: String,
    #[serde(default)]
    pub pinned_commit: Option<String>,
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateBetaLinkRequest {
    pub reader_name: String,
    pub max_chapter_index: Option<i64>,
    #[serde(default)]
    pub pinned_commit: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateBetaLinkRequest {
    pub reader_name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub max_chapter_index: Option<Option<i64>>,
    pub active: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub pinned_commit: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub username: Option<Option<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BetaFeedback {
    pub id: String,
    pub link_id: String,
    pub chapter_id: String,
    pub selected_text: String,
    pub context_block: String,
    pub comment: String,
    pub reader_name: String,
    pub resolved: bool,
    pub created_at: String,
    pub replies: Vec<BetaFeedbackReply>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateBetaFeedbackRequest {
    pub chapter_id: String,
    pub selected_text: String,
    pub context_block: String,
    pub comment: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BetaFeedbackReply {
    pub id: String,
    pub feedback_id: String,
    pub author_type: String,
    pub author_name: String,
    pub content: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateBetaReplyRequest {
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BetaReaderView {
    pub book_title: String,
    pub book_description: String,
    pub reader_name: String,
    pub chapters: Vec<BetaChapterSummary>,
    pub font_settings: Option<FontSettings>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BetaChapterSummary {
    pub id: String,
    pub title: String,
    pub sort_order: i64,
}

// ── Shared Books ──

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SharedBook {
    pub book_title: String,
    pub book_description: String,
    pub token: String,
    pub reader_name: String,
    pub author_username: String,
}

// ── Import ──

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportPreviewChapter {
    pub title: String,
    pub content_preview: String,
    pub word_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportPreviewResponse {
    pub chapters: Vec<ImportPreviewChapter>,
    pub filename: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportConfirmRequest {
    pub chapters: Vec<ImportChapter>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportChapter {
    pub title: String,
    pub content: String,
}

// ── Notes ──

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Note {
    pub id: String,
    pub book_id: String,
    pub title: String,
    pub content: String,
    pub color: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NoteTree {
    pub root_order: Vec<String>,
    pub children: std::collections::HashMap<String, Vec<String>>,
    #[serde(default)]
    pub collapsed: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NotesResponse {
    pub notes: Vec<Note>,
    pub tree: NoteTree,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateNoteRequest {
    pub title: String,
    pub parent_id: Option<String>,
    pub color: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateNoteRequest {
    pub title: Option<String>,
    pub content: Option<String>,
    pub color: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MoveNoteRequest {
    pub note_id: String,
    pub new_parent_id: Option<String>,
    pub index: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateNoteTreeRequest {
    pub tree: NoteTree,
}

// ── History ──

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CommitInfo {
    pub oid: String,
    pub message: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CommitDiff {
    pub changed_chapters: Vec<ChapterDiff>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChapterDiff {
    pub chapter_id: String,
    pub chapter_title: String,
    pub change_type: String,
    pub hunks: Vec<DiffHunk>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiffHunk {
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiffLine {
    pub origin: String,
    pub content: String,
}

// ── Error ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiError {
    pub error: String,
}
