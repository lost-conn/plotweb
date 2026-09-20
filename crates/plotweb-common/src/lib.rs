use serde::{Deserialize, Deserializer, Serialize};

pub mod markdown;
pub use markdown::markdown_to_html;

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

/// Body for `POST /api/auth/forgot-password` — request a reset link by email.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgotPasswordRequest {
    pub email: String,
}

/// Body for `POST /api/auth/reset-password` — redeem a token for a new password.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResetPasswordRequest {
    pub token: String,
    pub new_password: String,
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
    #[serde(default)]
    pub cover_image: Option<String>,
    /// Whether this book reads from the canonical CRDT rather than git.
    ///
    /// The client needs this to tell the author the truth about where their writing
    /// goes: for a cut-over book, sync is how an edit reaches the server, so a device
    /// with sync off is writing only to itself. Nothing in the UI used to say that, and
    /// "Saved" meant two different things depending on a flag the author could not see.
    #[serde(default)]
    pub cutover: bool,
    /// The book's calendar — how a note's time reads. `None` for every book that never
    /// set one, which reads through [`Calendar::default`]; so a book that never touches
    /// the calendar serialises exactly as it did before there was one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calendar: Option<Calendar>,
    /// How the timeline's event ribbon draws a parent event against its children —
    /// see [`SpanRule`]. `None` for every book that never chose, which reads as
    /// [`SpanRule::Fit`]; stored beside the calendar and absent in exactly the same way,
    /// so a book that never touches it serialises as it did before there was one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span_rule: Option<SpanRule>,
}

/// The per-book rule for drawing a nested event against its parent on the ribbon.
///
/// Every rule is applied **when drawing** only. None of them ever rewrites a typed span:
/// a fitted parent's dates are what the ribbon computes, not what the note stores.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum SpanRule {
    /// A parent is drawn across its own dates plus its children's (fitted, recursively),
    /// unless the parent is [`Note::pinned`]. The default.
    #[default]
    Fit,
    /// A parent's typed dates win; a child running past them is drawn cut off at the
    /// parent's edge, with a marker. A parent with no dates of its own clamps nothing.
    Clamp,
    /// Everything is drawn as typed; a child escaping its parent pokes out and is flagged.
    Free,
}

impl SpanRule {
    /// The rule a book actually draws with: its own, or [`SpanRule::Fit`].
    pub fn effective(rule: Option<SpanRule>) -> SpanRule {
        rule.unwrap_or_default()
    }

    /// The word stored for it — `meta.span_rule` in the `book:` document, and the JSON.
    pub fn as_str(self) -> &'static str {
        match self {
            SpanRule::Fit => "fit",
            SpanRule::Clamp => "clamp",
            SpanRule::Free => "free",
        }
    }

    /// Read a stored word back. Anything else is `None` — which draws as
    /// [`SpanRule::Fit`], never as an error.
    pub fn parse(word: &str) -> Option<SpanRule> {
        match word {
            "fit" => Some(SpanRule::Fit),
            "clamp" => Some(SpanRule::Clamp),
            "free" => Some(SpanRule::Free),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateBookRequest {
    pub title: String,
    pub description: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateBookRequest {
    pub title: Option<String>,
    pub description: Option<String>,
    pub font_settings: Option<FontSettings>,
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub cover_image: Option<Option<String>>,
    /// A patch, like `cover_image`: absent leaves the calendar alone, `null` returns the
    /// book to the default calendar, a value replaces it whole.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_double_option"
    )]
    pub calendar: Option<Option<Calendar>>,
    /// A patch, like `calendar`: absent leaves the rule alone, `null` returns the book to
    /// the default ([`SpanRule::Fit`]), a value sets it.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_double_option"
    )]
    pub span_rule: Option<Option<SpanRule>>,
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

/// What a body write actually did, returned by the chapter/note update routes.
///
/// A save that reaches the server and persists nothing used to answer `200 {"ok":true}`
/// — the client showed "Saved" while the content went nowhere, and it stayed invisible
/// for two days. The write path can legitimately take several routes (git, the
/// canonical document, or deliberately neither when the client's own sync engine is
/// carrying the body), so the response says which one it took rather than asserting
/// success.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SaveReceipt {
    /// The content was written to the git store.
    pub git: bool,
    /// The content was applied to the canonical CRDT document.
    pub canonical: bool,
    /// The server deliberately left this content to the client's sync engine, having
    /// confirmed the canonical document is one it can actually read and write.
    pub deferred_to_sync: bool,
    /// Author-facing explanation of a degraded or refused write. Present when a
    /// cut-over book's canonical document could not carry the edit after all, and when
    /// nothing was persisted at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

impl SaveReceipt {
    /// Whether this request's content is accounted for: written somewhere durable, or
    /// knowingly left to a sync engine that can deliver it.
    pub fn is_durable(&self) -> bool {
        self.git || self.canonical || self.deferred_to_sync
    }
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateChapterRequest {
    pub title: Option<String>,
    /// Absent for a cut-over book: there, sync is the only writer of a body, so a
    /// whole-state copy can only be a stale duplicate of what the ops already said.
    ///
    /// This used to be accompanied by a `sync_owned` declaration, and the two of them
    /// were a negotiation between two writers over who should stand down — a
    /// negotiation whose failures cost, in order, two days of silently dropped writes,
    /// the reappearing-text bug, and a writing session. There is one writer now, so
    /// there is nothing to declare.
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
    #[serde(default)]
    pub cover_image: Option<String>,
    /// Improved reader mode: the chapter/page the reader last left off at, and
    /// their saved bookmarks. `last_chapter_id` is `None`/`last_page` is 0 when
    /// nothing has been read yet.
    #[serde(default)]
    pub last_chapter_id: Option<String>,
    #[serde(default)]
    pub last_page: i64,
    #[serde(default)]
    pub bookmarks: Vec<BetaBookmark>,
}

/// A reader's saved bookmark (chapter + page in the paginated reader).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BetaBookmark {
    pub id: String,
    pub chapter_id: String,
    pub page: i64,
    pub label: String,
    pub created_at: String,
}

/// Body for `PUT /api/beta/{token}/progress` — auto last-page tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateReadingProgressRequest {
    pub chapter_id: String,
    pub page: i64,
}

/// Body for `POST /api/beta/{token}/bookmarks`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateBookmarkRequest {
    pub chapter_id: String,
    pub page: i64,
    pub label: String,
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
    #[serde(default)]
    pub cover_image: Option<String>,
    /// Chapters this link can actually reach (respects `max_chapter_index` /
    /// `pinned_commit`). `None` if it could not be computed.
    #[serde(default)]
    pub chapter_count: Option<i64>,
    /// `chapter_count` minus how far into the accessible list `last_chapter_id`
    /// reaches, clamped at 0. `None` if it could not be computed; everything is
    /// unread when there is no `last_chapter_id` yet.
    #[serde(default)]
    pub unread_count: Option<i64>,
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

pub mod calendar;
pub mod note_links;
pub mod note_time;

pub use calendar::{Calendar, CalendarUnit, ShownBelow};

pub use note_links::{
    extract_note_links, extract_note_links_in, fold_token, note_plain_text, token_for_title,
    LinkIndex, LinkTarget, NoteLink, NoteLinks,
};
pub use note_time::{RelativeTime, TimePoint, TimeRelation, TimeSpan, TICKS_PER_BASE_UNIT};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Note {
    pub id: String,
    pub book_id: String,
    pub title: String,
    pub content: String,
    pub color: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    /// When this note happens, if it happens at all. `Some` is what makes a note act
    /// as an **event**; `None` is lore. See [`note_time`] for the representation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span: Option<TimeSpan>,
    /// A constraint against another note ("after the parley") for a note whose place
    /// in time is known only relative to something else. Independent of [`Note::span`]:
    /// a note may carry both, neither, or either.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relative: Option<RelativeTime>,
    /// The **entity** facet: this note is a person, place or thing that can appear in
    /// events and gets a lane on the timeline. Orthogonal to the event facet — a
    /// character with a lifespan is one note carrying both.
    #[serde(default, skip_serializing_if = "is_false")]
    pub is_entity: bool,
    /// The note that contains this one **in time** — the siege that holds the breach.
    ///
    /// Deliberately **not** the tree parent. [`NoteTree`] says where the author filed
    /// a note; this says what contains it on the timeline. Neither implies the other,
    /// and this is only ever set explicitly — never inferred from two spans
    /// overlapping, because two events can coincide without one containing the other.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_parent: Option<String>,
    /// Pinned in time: under the book's [`SpanRule::Fit`] the ribbon draws this note
    /// across its own typed dates only, never stretched to fit its children. A facet
    /// like [`Note::is_entity`], and carried the same way (a clear is a `false`
    /// tombstone in the `book:` document).
    #[serde(default, skip_serializing_if = "is_false")]
    pub pinned: bool,
    /// What this note's body points at — derived from the body on every write, never
    /// authored. Served alongside the note so the editor's context rail, and later the
    /// timeline, can draw the graph from the notes list alone rather than opening
    /// every `note:{id}` document. See [`note_links`].
    #[serde(default, skip_serializing_if = "NoteLinks::is_empty")]
    pub links: NoteLinks,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl Note {
    /// Whether this note is placed in time at all — by a span, or by a constraint
    /// against another note. The **event** facet; [`Note::is_entity`] is the other.
    pub fn is_event(&self) -> bool {
        self.span.is_some() || self.relative.is_some()
    }
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateNoteRequest {
    pub title: Option<String>,
    pub content: Option<String>,
    /// See [`UpdateChapterRequest::content`]. Title and colour are structure, which
    /// REST still carries for every book.
    pub color: Option<String>,
    /// The facet fields are **patches**: absent leaves the stored value alone, an
    /// explicit `null` clears it, a value sets it. A bare `Option` cannot say "clear
    /// this note's span", which is a gesture card 4 needs.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_double_option"
    )]
    pub span: Option<Option<TimeSpan>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_double_option"
    )]
    pub relative: Option<Option<RelativeTime>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_entity: Option<bool>,
    /// Containment in time. Always stated by the client, never derived here — see
    /// [`Note::event_parent`].
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_double_option"
    )]
    pub event_parent: Option<Option<String>>,
    /// See [`Note::pinned`]. A patch like `is_entity`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned: Option<bool>,
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

// ── Images ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageUploadResponse {
    pub url: String,
    pub filename: String,
}

// ── User dictionary (spellcheck) ──

/// `GET /api/me/dictionary` — the signed-in user's custom spellcheck words.
///
/// `words` is normalised by the server (trimmed, de-duplicated, sorted); an account
/// that has never saved one gets an empty list and an empty `updated_at`, so the
/// client never has to distinguish "no row" from "an empty list".
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct UserDictionary {
    pub words: Vec<String>,
    pub updated_at: String,
}

/// Body for `PUT /api/me/dictionary` — replaces the whole list.
///
/// A replace rather than an append because the client is the local-first owner of
/// this list: it holds the union of what every device has added and pushes that
/// whole set. See `plotweb-web/src/local_dictionary.rs`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UpdateUserDictionaryRequest {
    pub words: Vec<String>,
}

/// The most words one account's custom dictionary may hold. A list beyond this is
/// refused rather than truncated — silently dropping an author's words is worse
/// than telling them the list is full.
pub const MAX_USER_DICTIONARY_WORDS: usize = 10_000;

/// The longest a single custom word may be. Anything longer is not a word.
pub const MAX_USER_DICTIONARY_WORD_LEN: usize = 64;

// ── Error ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiError {
    pub error: String,
}
