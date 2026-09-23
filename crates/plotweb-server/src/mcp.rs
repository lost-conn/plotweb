//! `/api/mcp` — the Model Context Protocol endpoint an author's AI agent connects to.
//!
//! An agent (Claude Code, Claude Desktop, any MCP client) authenticates with one of
//! the author's personal access tokens (`Authorization: Bearer pw_…`, see
//! [`crate::token_auth`]) and gets a small set of **assistant** tools: read the
//! manuscript, search it, count it, review it by leaving comments on exact passages,
//! and read and organise notes.
//!
//! # What an agent can never do
//!
//! Write manuscript prose. That is a product principle, and it is enforced the
//! simplest way there is: **no tool here writes chapter content or titles, reorders
//! chapters, deletes anything, or touches tokens.** `tests/mcp.rs` asserts the exact
//! tool list, so adding one fails CI and has to be argued for. Note *bodies* are also
//! out of reach after creation — see [`PlotwebMcp::update_note`] — because for a
//! cut-over book the sync engine is the only writer of a body, and a REST-shaped write
//! there is silently dropped (see `routes::notes::update_note_inner`).
//!
//! # Transport
//!
//! rmcp's streamable-HTTP server, in **stateless** mode: every POST is a whole
//! exchange, with no session held in memory, so a deploy (which restarts the server)
//! never strands a connected agent. Responses are plain JSON rather than SSE when the
//! tool produces nothing but its result, which is always here.
//!
//! # Authentication and scope
//!
//! [`require_token`] runs before rmcp sees the request: no valid token, `401` with
//! `WWW-Authenticate: Bearer`, and rmcp is never reached. A valid one is put into the
//! request's extensions as a [`TokenAuth`]; rmcp hands the request's `Parts` to each
//! tool, which reads it back from there. Every tool that takes a `book_id` asks
//! [`TokenAuth::can_access_book`] (ownership **and** the token's scope) and answers the
//! same "book not found" whether the book is missing, someone else's, or merely outside
//! the token's scope — a token learns nothing about books it cannot reach.
//!
//! rmcp's DNS-rebinding `Host` allowlist is switched off. It protects unauthenticated
//! servers on loopback; this one is public behind a reverse proxy (so the `Host` is the
//! deployment's own name, which the binary does not know) and every request must carry
//! a bearer token a rebinding page has no way to obtain.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Router;
use plotweb_common::{Note, RelativeTime, TimeSpan, UpdateNoteRequest};
use plotweb_export::text::{self, Legacy, TextBlock};
use rmcp::handler::server::tool::Extension;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerConfig,
};
use rmcp::transport::streamable_http_server::session::never::NeverSessionManager;
use rmcp::transport::{StreamableHttpServerConfig, StreamableHttpService};
use rmcp::{tool, tool_handler, tool_router, ErrorData, ServerHandler};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::rhype::quote;
use crate::routes::{beta, chapters, notes};
use crate::token_auth::TokenAuth;
use crate::AppState;

/// The one answer for a book a token cannot reach, whatever the reason.
const BOOK_NOT_FOUND: &str = "book not found";

/// Default and ceiling for `search_manuscript`'s `max_results`.
pub const SEARCH_MAX_RESULTS: usize = 50;
/// Characters of context on each side of a search hit.
const SNIPPET_RADIUS: usize = 80;
/// Longest query / quote / comment / note body an agent may send. Generous for real
/// use; only here so one call cannot turn into a very large write.
const MAX_QUERY_CHARS: usize = 500;
const MAX_QUOTE_CHARS: usize = 2_000;
const MAX_COMMENT_CHARS: usize = 10_000;
const MAX_NOTE_BODY_CHARS: usize = 100_000;
/// The beta reader stores at most this much of the enclosing paragraph as a comment's
/// `context_block`; agent comments do the same so the rail treats them alike.
const CONTEXT_BLOCK_CHARS: usize = 200;

/// What the agent is told at `initialize`.
pub const INSTRUCTIONS: &str = "\
PlotWeb is this author's fiction-writing workspace: their books, chapters and \
worldbuilding notes. You are connected as the author's reviewer and research \
assistant, not as a co-writer.

What you can do:
- Read: list_books, book_outline, read_chapter, search_manuscript, manuscript_stats, \
list_notes, read_note.
- Review: add_review_comment leaves a comment on an exact passage of a chapter; it \
appears in the author's feedback rail, marked as coming from you. Reply to existing \
reader or agent feedback with reply_to_feedback, and see it with list_feedback.
- Organise notes: create_note, and update_note to rename, recolour, move, pin or set \
timeline facets.

What you cannot and must not do:
- You cannot write, rewrite, retitle, reorder or delete manuscript prose, and there \
is no tool for it. If you think a passage should change, say so in a review comment \
and let the author decide. Do not paste replacement prose into notes as a workaround.
- Note bodies cannot be edited after creation yet; create_note can set a new note's \
initial body only.

How to comment well:
- The quote in add_review_comment must be copied exactly from read_chapter's text, \
including punctuation, curly quotes and dashes; only whitespace differences are \
forgiven. Quote a short, distinctive span (a phrase or a sentence), not a whole page.
- Keep comments specific and actionable, and write them for the author.";

/// The MCP server: one per request (stateless), all state is the app's.
#[derive(Clone)]
pub struct PlotwebMcp {
    state: AppState,
}

impl PlotwebMcp {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }
}

/// The `/api/mcp` route, authenticated. Merged into the app by `api_router`.
pub fn router(state: AppState) -> Router<AppState> {
    let config = StreamableHttpServerConfig::default()
        .with_legacy_session_mode(false)
        .with_json_response(true)
        .with_sse_keep_alive(None)
        .disable_allowed_hosts();
    let for_factory = state.clone();
    let service = StreamableHttpService::new(
        move || Ok(PlotwebMcp::new(for_factory.clone())),
        Arc::new(NeverSessionManager::default()),
        config,
    );
    Router::new()
        .route_service("/api/mcp", service)
        .route_layer(axum::middleware::from_fn_with_state(state, require_token))
}

/// Reject anything without a valid personal access token before rmcp sees it, and
/// hand the authenticated [`TokenAuth`] on to the tools through the request.
pub async fn require_token(State(state): State<AppState>, mut req: Request, next: Next) -> Response {
    let header = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok());
    match TokenAuth::authenticate(&state, header).await {
        Ok(auth) => {
            req.extensions_mut().insert(auth);
            next.run(req).await
        }
        Err(e) => e.into_response(),
    }
}

// ── Tool parameters ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BookParams {
    /// The book's id (from list_books).
    pub book_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChapterParams {
    /// The book's id.
    pub book_id: String,
    /// The chapter's id (from book_outline).
    pub chapter_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchParams {
    /// The book's id.
    pub book_id: String,
    /// Text to find. Case-insensitive; matched within a paragraph.
    pub query: String,
    /// Most hits to return (default and maximum 50).
    pub max_results: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoteParams {
    /// The book's id.
    pub book_id: String,
    /// The note's id (from list_notes or book_outline).
    pub note_id: String,
}

/// Timeline facets of a note. Each is a patch: leave it out to keep the current
/// value, send null to clear it.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FacetsParam {
    /// When the note happens, which makes it an event. Shape:
    /// {"start": {"tick": <i64>, "precision": <0 = year, 1 = month, 2 = day, …>},
    ///  "end": <same, optional>, "approximate": <bool>, "open_ended": <bool>}.
    /// A tick is one second in the default calendar (31536000 ticks per year,
    /// no leap years; tick 0 is the start of year 0).
    #[serde(default, deserialize_with = "double_option")]
    #[schemars(with = "Option<Value>")]
    pub span: Option<Option<Value>>,
    /// Placement relative to another note:
    /// {"relation": "after" | "before" | "during", "note_id": "<note id>"}.
    #[serde(default, deserialize_with = "double_option")]
    #[schemars(with = "Option<Value>")]
    pub relative: Option<Option<Value>>,
    /// Whether this note is an entity (a person, place or thing that gets a
    /// lane on the timeline).
    pub is_entity: Option<bool>,
    /// The note that contains this one in time (e.g. the siege that holds the
    /// breach). Not the same as the note's parent in the tree.
    #[serde(default, deserialize_with = "double_option")]
    #[schemars(with = "Option<String>")]
    pub event_parent: Option<Option<String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateNoteParams {
    /// The book's id.
    pub book_id: String,
    /// The new note's title.
    pub title: String,
    /// File it under this note in the notes tree; leave out for the top level.
    pub parent_id: Option<String>,
    /// A colour for the note, as a CSS colour string (e.g. "#c0392b").
    pub color: Option<String>,
    /// Timeline facets to set on the new note.
    pub facets: Option<FacetsParam>,
    /// Pin it in time (the timeline draws it across its own dates only).
    pub pinned: Option<bool>,
    /// The new note's initial body, as Markdown. Only at creation: note bodies
    /// cannot be edited afterwards through this server.
    pub body: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateNoteParams {
    /// The book's id.
    pub book_id: String,
    /// The note to change.
    pub note_id: String,
    /// A new title.
    pub title: Option<String>,
    /// A new colour (CSS colour string).
    pub color: Option<String>,
    /// Timeline facets to change (each a patch; null clears).
    pub facets: Option<FacetsParam>,
    /// Pin or unpin it in time.
    pub pinned: Option<bool>,
    /// Move it under this note in the tree; null moves it to the top level. Leave
    /// out to keep its parent.
    #[serde(default, deserialize_with = "double_option")]
    #[schemars(with = "Option<String>")]
    pub parent_id: Option<Option<String>>,
    /// Its 0-based position among its (new) siblings. Leave out to keep it where
    /// it is, or to append it when moving to a new parent.
    pub position: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReviewCommentParams {
    /// The book's id.
    pub book_id: String,
    /// The chapter the passage is in.
    pub chapter_id: String,
    /// The exact passage you are commenting on, copied from read_chapter's text
    /// (whitespace differences are forgiven, nothing else is). Keep it short: a
    /// phrase or a sentence.
    pub quote: String,
    /// Your comment for the author.
    pub comment: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListFeedbackParams {
    /// The book's id.
    pub book_id: String,
    /// Only feedback on this chapter.
    pub chapter_id: Option<String>,
    /// Include feedback the author has marked resolved (default false).
    #[serde(default)]
    pub include_resolved: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReplyParams {
    /// The book's id.
    pub book_id: String,
    /// The feedback item to reply to (from list_feedback).
    pub feedback_id: String,
    /// Your reply.
    pub comment: String,
}

/// `Option<Option<T>>` that tells an absent field (`None`) from an explicit `null`
/// (`Some(None)`) — with `#[serde(default)]` supplying the absent case.
fn double_option<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

// ── Helpers ────────────────────────────────────────────────────────────────

fn tool_error(msg: impl Into<String>) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(msg.into())])
}

fn json_result(value: Value) -> CallToolResult {
    CallToolResult::success(vec![ContentBlock::text(
        serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string()),
    )])
}

/// The authenticated token, put there by [`require_token`].
fn token(parts: &Parts) -> Result<TokenAuth, ErrorData> {
    parts
        .extensions
        .get::<TokenAuth>()
        .cloned()
        .ok_or_else(|| ErrorData::internal_error("request reached a tool unauthenticated", None))
}

/// What an agent's comments and replies are signed with: its token's label now.
fn agent_name(auth: &TokenAuth) -> String {
    let label = auth.label.trim();
    if label.is_empty() {
        "AI agent".to_string()
    } else {
        label.to_string()
    }
}

/// Collapse every run of whitespace to one space and trim — how quotes are matched,
/// and how the author's rail matches them when it scrolls to one.
fn normalize_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn check_len(what: &str, value: &str, max: usize) -> Result<(), CallToolResult> {
    if value.chars().count() > max {
        return Err(tool_error(format!("{what} is too long (at most {max} characters)")));
    }
    Ok(())
}

/// A chapter as the tools see it: served content (canonical for a cut-over book),
/// as plain-text blocks.
struct ChapterText {
    id: String,
    title: String,
    blocks: Vec<TextBlock>,
    word_count: u64,
}

/// Case-insensitive substring search over `haystack`, returning the char offsets
/// (start, end) of every non-overlapping hit in the original string.
fn find_ci(haystack: &str, needle: &str) -> Vec<(usize, usize)> {
    // Lower-case per char, remembering which original char each lowered char came
    // from, so a hit maps back exactly even where lower-casing changes the length.
    let mut lowered: Vec<char> = Vec::new();
    let mut origin: Vec<usize> = Vec::new();
    for (i, c) in haystack.chars().enumerate() {
        for l in c.to_lowercase() {
            lowered.push(l);
            origin.push(i);
        }
    }
    let needle: Vec<char> = needle.chars().flat_map(char::to_lowercase).collect();
    let mut hits = Vec::new();
    if needle.is_empty() || needle.len() > lowered.len() {
        return hits;
    }
    let mut i = 0;
    while i + needle.len() <= lowered.len() {
        if lowered[i..i + needle.len()] == needle[..] {
            let start = origin[i];
            let end = origin[i + needle.len() - 1] + 1;
            hits.push((start, end));
            i += needle.len();
        } else {
            i += 1;
        }
    }
    hits
}

/// About [`SNIPPET_RADIUS`] characters either side of a hit, marked with `…` where cut.
fn snippet(text: &str, start: usize, end: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    let from = start.saturating_sub(SNIPPET_RADIUS);
    let to = (end + SNIPPET_RADIUS).min(chars.len());
    let mut out = String::new();
    if from > 0 {
        out.push('…');
    }
    out.extend(chars[from..to].iter().map(|c| if *c == '\n' { ' ' } else { *c }));
    if to < chars.len() {
        out.push('…');
    }
    out
}

fn facet_labels(n: &Note) -> Vec<&'static str> {
    let mut out = Vec::new();
    if n.is_event() {
        out.push("event");
    }
    if n.is_entity {
        out.push("entity");
    }
    if n.pinned {
        out.push("pinned");
    }
    out
}

fn note_facets_json(n: &Note) -> Value {
    json!({
        "span": n.span,
        "relative": n.relative,
        "is_entity": n.is_entity,
        "event_parent": n.event_parent,
        "pinned": n.pinned,
    })
}

impl PlotwebMcp {
    /// `Ok` if the token may reach the book; otherwise the tool's answer.
    async fn check_book(&self, auth: &TokenAuth, book_id: &str) -> Result<(), CallToolResult> {
        if auth.can_access_book(&self.state, book_id).await {
            Ok(())
        } else {
            Err(tool_error(BOOK_NOT_FOUND))
        }
    }

    /// Every chapter of a book, in order, with its served body as plain text.
    async fn manuscript(&self, book_id: &str) -> Result<Vec<ChapterText>, CallToolResult> {
        let listed = chapters::list_chapters_inner(&self.state, book_id)
            .await
            .map_err(|e| {
                eprintln!("[mcp] list chapters for {book_id}: {e}");
                tool_error("could not read the book's chapters")
            })?;
        // Bodies for a cut-over book come from the canonical store — a file read and a
        // CRDT materialize per chapter — so do the lot off the async reactor.
        let state = self.state.clone();
        let book = book_id.to_string();
        tokio::task::spawn_blocking(move || {
            listed
                .into_iter()
                .map(|ch| {
                    let content = match crate::routes::cutover_body(
                        &state,
                        &book,
                        &format!("chapter:{}", ch.id),
                        &ch.content,
                        plotweb_crdt::BodyKind::Chapter,
                    ) {
                        crate::routes::CutoverRead::Git => ch.content,
                        crate::routes::CutoverRead::Canonical(c) => c,
                    };
                    ChapterText {
                        word_count: plotweb_git::chapter::count_words(&content),
                        blocks: text::text_blocks(&content, Legacy::Markdown),
                        id: ch.id,
                        title: ch.title,
                    }
                })
                .collect()
        })
        .await
        .map_err(|e| {
            eprintln!("[mcp] manuscript read worker failed: {e}");
            tool_error("could not read the book's chapters")
        })
    }

    /// Validate a facets patch against the book's notes and turn it into the REST
    /// update shape (no title, colour or content).
    fn facets_patch(
        facets: FacetsParam,
        note_id: Option<&str>,
        existing: &HashSet<String>,
    ) -> Result<UpdateNoteRequest, CallToolResult> {
        let mut req = UpdateNoteRequest::default();
        if let Some(span) = facets.span {
            req.span = Some(match span {
                None => None,
                Some(v) => Some(serde_json::from_value::<TimeSpan>(v).map_err(|e| {
                    tool_error(format!(
                        "span is not valid ({e}); expected {{\"start\": {{\"tick\": <int>, \"precision\": <int>}}, \"end\": …optional, \"approximate\": bool, \"open_ended\": bool}}"
                    ))
                })?),
            });
        }
        if let Some(rel) = facets.relative {
            req.relative = Some(match rel {
                None => None,
                Some(v) => {
                    let rel = serde_json::from_value::<RelativeTime>(v).map_err(|e| {
                        tool_error(format!(
                            "relative is not valid ({e}); expected {{\"relation\": \"after\"|\"before\"|\"during\", \"note_id\": \"…\"}}"
                        ))
                    })?;
                    if !existing.contains(&rel.note_id) || Some(rel.note_id.as_str()) == note_id {
                        return Err(tool_error("relative.note_id must be another note in this book"));
                    }
                    Some(rel)
                }
            });
        }
        if let Some(parent) = facets.event_parent {
            if let Some(p) = &parent {
                if !existing.contains(p) || Some(p.as_str()) == note_id {
                    return Err(tool_error("event_parent must be another note in this book"));
                }
            }
            req.event_parent = Some(parent);
        }
        req.is_entity = facets.is_entity;
        Ok(req)
    }

    /// A note's place in the git notes tree: (parent, index among its siblings).
    async fn tree_position(&self, book_id: &str, note_id: &str) -> Option<(Option<String>, usize)> {
        let (_, tree) = self.state.books.list_notes(book_id).await.ok()?;
        if let Some(i) = tree.root_order.iter().position(|id| id == note_id) {
            return Some((None, i));
        }
        tree.children.iter().find_map(|(parent, kids)| {
            kids.iter()
                .position(|id| id == note_id)
                .map(|i| (Some(parent.clone()), i))
        })
    }

    async fn sibling_count(&self, book_id: &str, parent: Option<&str>) -> usize {
        let Ok((_, tree)) = self.state.books.list_notes(book_id).await else {
            return 0;
        };
        match parent {
            None => tree.root_order.len(),
            Some(p) => tree.children.get(p).map(Vec::len).unwrap_or(0),
        }
    }

    /// The note as `read_note` / the create and update tools report it.
    fn note_summary(n: &Note, parent: Option<&str>) -> Value {
        json!({
            "id": n.id,
            "title": n.title,
            "parent_id": parent,
            "color": n.color,
            "facets": note_facets_json(n),
            "links": n.links,
        })
    }
}

#[tool_router]
impl PlotwebMcp {
    #[tool(
        description = "List the author's books this token can reach: id, title and chapter count.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn list_books(&self, Extension(parts): Extension<Parts>) -> Result<CallToolResult, ErrorData> {
        let auth = token(&parts)?;
        let mut rows = self
            .state
            .rhype
            .find(format!("Book.filter(.user_id == {})", quote(&auth.user_id)))
            .await
            .unwrap_or_default();
        rows.sort_by(|a, b| b.string("created_at").cmp(&a.string("created_at")));

        let mut books = Vec::new();
        for row in rows {
            let id = row.string("uuid").unwrap_or_default();
            if !auth.in_scope(&id) {
                continue;
            }
            let git = self.state.books.get_book(&id).await.ok();
            let chapter_count = match crate::routes::cutover_structure(&self.state, &id).await {
                Some(structure) => structure.chapters.len(),
                None => git.as_ref().map(|b| b.chapter_order.len()).unwrap_or(0),
            };
            let title = git
                .map(|b| b.title)
                .unwrap_or_else(|| row.string("title").unwrap_or_default());
            books.push(json!({ "id": id, "title": title, "chapter_count": chapter_count }));
        }
        Ok(json_result(json!({ "books": books })))
    }

    #[tool(
        description = "A book's structure: its chapters in reading order (id, title, word count) and its notes tree (id, title, facets such as event/entity/pinned, children).",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn book_outline(
        &self,
        Parameters(p): Parameters<BookParams>,
        Extension(parts): Extension<Parts>,
    ) -> Result<CallToolResult, ErrorData> {
        let auth = token(&parts)?;
        if let Err(e) = self.check_book(&auth, &p.book_id).await {
            return Ok(e);
        }
        let manuscript = match self.manuscript(&p.book_id).await {
            Ok(m) => m,
            Err(e) => return Ok(e),
        };
        let chapters: Vec<Value> = manuscript
            .iter()
            .enumerate()
            .map(|(i, c)| json!({ "id": c.id, "title": c.title, "position": i, "word_count": c.word_count }))
            .collect();

        let notes = notes::list_notes_inner(&self.state, &p.book_id).await;
        let by_id: HashMap<&str, &Note> = notes.notes.iter().map(|n| (n.id.as_str(), n)).collect();
        fn node(
            id: &str,
            by_id: &HashMap<&str, &Note>,
            children: &HashMap<String, Vec<String>>,
            seen: &mut HashSet<String>,
        ) -> Option<Value> {
            let n = by_id.get(id)?;
            if !seen.insert(id.to_string()) {
                return None;
            }
            let kids: Vec<Value> = children
                .get(id)
                .map(|ks| ks.iter().filter_map(|k| node(k, by_id, children, seen)).collect())
                .unwrap_or_default();
            Some(json!({
                "id": n.id,
                "title": n.title,
                "facets": facet_labels(n),
                "children": kids,
            }))
        }
        let mut seen = HashSet::new();
        let mut tree: Vec<Value> = notes
            .tree
            .root_order
            .iter()
            .filter_map(|id| node(id, &by_id, &notes.tree.children, &mut seen))
            .collect();
        // Anything the tree does not reach is still the author's note.
        for n in &notes.notes {
            if !seen.contains(&n.id) {
                if let Some(v) = node(&n.id, &by_id, &notes.tree.children, &mut seen) {
                    tree.push(v);
                }
            }
        }

        Ok(json_result(json!({ "chapters": chapters, "notes": tree })))
    }

    #[tool(
        description = "Read one chapter: its title and full text as plain text (paragraphs separated by blank lines, headings as '#' lines). Quote from this text when commenting.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn read_chapter(
        &self,
        Parameters(p): Parameters<ChapterParams>,
        Extension(parts): Extension<Parts>,
    ) -> Result<CallToolResult, ErrorData> {
        let auth = token(&parts)?;
        if let Err(e) = self.check_book(&auth, &p.book_id).await {
            return Ok(e);
        }
        let Some(ch) = chapters::get_chapter_inner(&self.state, &p.book_id, &p.chapter_id).await
        else {
            return Ok(tool_error("chapter not found"));
        };
        let body = text::readable_text(&ch.content, Legacy::Markdown);
        let meta = json!({
            "id": ch.id,
            "title": ch.title,
            "word_count": plotweb_git::chapter::count_words(&ch.content),
        });
        Ok(CallToolResult::success(vec![
            ContentBlock::text(meta.to_string()),
            ContentBlock::text(body),
        ]))
    }

    #[tool(
        description = "Search the whole manuscript for text (case-insensitive, within a paragraph). Returns each hit's chapter id and title with about 80 characters of context either side. max_results defaults to 50, which is also the maximum.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn search_manuscript(
        &self,
        Parameters(p): Parameters<SearchParams>,
        Extension(parts): Extension<Parts>,
    ) -> Result<CallToolResult, ErrorData> {
        let auth = token(&parts)?;
        if let Err(e) = self.check_book(&auth, &p.book_id).await {
            return Ok(e);
        }
        let query = p.query.trim();
        if query.is_empty() {
            return Ok(tool_error("query must not be empty"));
        }
        if let Err(e) = check_len("query", query, MAX_QUERY_CHARS) {
            return Ok(e);
        }
        let limit = p
            .max_results
            .map(|n| (n as usize).clamp(1, SEARCH_MAX_RESULTS))
            .unwrap_or(SEARCH_MAX_RESULTS);
        let manuscript = match self.manuscript(&p.book_id).await {
            Ok(m) => m,
            Err(e) => return Ok(e),
        };

        let mut results = Vec::new();
        let mut total = 0usize;
        for ch in &manuscript {
            for block in &ch.blocks {
                for (start, end) in find_ci(&block.text, query) {
                    total += 1;
                    if results.len() < limit {
                        results.push(json!({
                            "chapter_id": ch.id,
                            "chapter_title": ch.title,
                            "snippet": snippet(&block.text, start, end),
                        }));
                    }
                }
            }
        }
        Ok(json_result(json!({
            "query": query,
            "total_hits": total,
            "returned": results.len(),
            "truncated": total > results.len(),
            "results": results,
        })))
    }

    #[tool(
        description = "Word counts: per chapter (in reading order) and for the whole manuscript.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn manuscript_stats(
        &self,
        Parameters(p): Parameters<BookParams>,
        Extension(parts): Extension<Parts>,
    ) -> Result<CallToolResult, ErrorData> {
        let auth = token(&parts)?;
        if let Err(e) = self.check_book(&auth, &p.book_id).await {
            return Ok(e);
        }
        let manuscript = match self.manuscript(&p.book_id).await {
            Ok(m) => m,
            Err(e) => return Ok(e),
        };
        let total: u64 = manuscript.iter().map(|c| c.word_count).sum();
        let chapters: Vec<Value> = manuscript
            .iter()
            .map(|c| json!({ "id": c.id, "title": c.title, "word_count": c.word_count }))
            .collect();
        Ok(json_result(json!({
            "chapter_count": chapters.len(),
            "total_words": total,
            "chapters": chapters,
        })))
    }

    #[tool(
        description = "List a book's notes (no bodies): id, title, parent in the notes tree, colour, facets (span, relative, is_entity, event_parent, pinned) and what each links to.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn list_notes(
        &self,
        Parameters(p): Parameters<BookParams>,
        Extension(parts): Extension<Parts>,
    ) -> Result<CallToolResult, ErrorData> {
        let auth = token(&parts)?;
        if let Err(e) = self.check_book(&auth, &p.book_id).await {
            return Ok(e);
        }
        let resp = notes::list_notes_inner(&self.state, &p.book_id).await;
        let parent_of: HashMap<&str, &str> = resp
            .tree
            .children
            .iter()
            .flat_map(|(p, kids)| kids.iter().map(move |k| (k.as_str(), p.as_str())))
            .collect();
        let listed: Vec<Value> = resp
            .notes
            .iter()
            .map(|n| Self::note_summary(n, parent_of.get(n.id.as_str()).copied()))
            .collect();
        Ok(json_result(json!({ "notes": listed, "root_order": resp.tree.root_order })))
    }

    #[tool(
        description = "Read one note: title, colour, facets, links, and its body as plain text.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn read_note(
        &self,
        Parameters(p): Parameters<NoteParams>,
        Extension(parts): Extension<Parts>,
    ) -> Result<CallToolResult, ErrorData> {
        let auth = token(&parts)?;
        if let Err(e) = self.check_book(&auth, &p.book_id).await {
            return Ok(e);
        }
        let Some(n) = notes::get_note_inner(&self.state, &p.book_id, &p.note_id).await else {
            return Ok(tool_error("note not found"));
        };
        let parent = self
            .tree_position(&p.book_id, &p.note_id)
            .await
            .and_then(|(parent, _)| parent);
        Ok(CallToolResult::success(vec![
            ContentBlock::text(Self::note_summary(&n, parent.as_deref()).to_string()),
            ContentBlock::text(text::readable_text(&n.content, Legacy::Html)),
        ]))
    }

    #[tool(
        description = "Create a note, optionally under a parent note, with facets and an initial Markdown body. The body can only be set here, at creation; it cannot be edited afterwards through this server.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = false, open_world_hint = false)
    )]
    async fn create_note(
        &self,
        Parameters(p): Parameters<CreateNoteParams>,
        Extension(parts): Extension<Parts>,
    ) -> Result<CallToolResult, ErrorData> {
        let auth = token(&parts)?;
        if let Err(e) = self.check_book(&auth, &p.book_id).await {
            return Ok(e);
        }
        let title = p.title.trim();
        if title.is_empty() {
            return Ok(tool_error("title is required"));
        }
        let existing: HashSet<String> = notes::list_notes_inner(&self.state, &p.book_id)
            .await
            .notes
            .into_iter()
            .map(|n| n.id)
            .collect();
        if let Some(parent) = &p.parent_id {
            if !existing.contains(parent) {
                return Ok(tool_error("parent_id is not a note in this book"));
            }
        }
        let facets = match Self::facets_patch(p.facets.unwrap_or_default(), None, &existing) {
            Ok(f) => f,
            Err(e) => return Ok(e),
        };
        let body = match p.body.as_deref().map(str::trim).filter(|b| !b.is_empty()) {
            None => None,
            Some(md) => {
                if let Err(e) = check_len("body", md, MAX_NOTE_BODY_CHARS) {
                    return Ok(e);
                }
                match text::markdown_to_docnode_json(md) {
                    Some(json) => Some(json),
                    None => return Ok(tool_error("body could not be read as Markdown")),
                }
            }
        };

        let created = match notes::create_note_inner(
            &self.state,
            &p.book_id,
            title,
            p.parent_id.as_deref(),
            p.color.as_deref(),
            body.as_deref(),
        )
        .await
        {
            Ok(n) => n,
            Err(e) => {
                eprintln!("[mcp] create_note in {}: {e}", p.book_id);
                return Ok(tool_error(format!("could not create the note: {e}")));
            }
        };

        let mut patch = facets;
        patch.pinned = p.pinned;
        let has_facets = patch.span.is_some()
            || patch.relative.is_some()
            || patch.is_entity.is_some()
            || patch.event_parent.is_some()
            || patch.pinned.is_some();
        if has_facets {
            if let Err(e) = notes::update_note_inner(&self.state, &p.book_id, &created.id, patch).await {
                eprintln!("[mcp] create_note facets for {}: {e}", created.id);
                return Ok(tool_error(format!(
                    "the note was created (id {}) but its facets could not be set: {e}",
                    created.id
                )));
            }
        }

        let note = notes::get_note_inner(&self.state, &p.book_id, &created.id)
            .await
            .unwrap_or(created);
        Ok(json_result(json!({
            "created": Self::note_summary(&note, p.parent_id.as_deref()),
            "body": text::readable_text(&note.content, Legacy::Html),
        })))
    }

    /// Structure only — title, colour, facets, pin, place in the tree. There is
    /// deliberately no body parameter: for a cut-over book a REST-shaped body write is
    /// dropped because sync is the body's only writer, and a tool that returned success
    /// for text that went nowhere would be worse than no tool.
    #[tool(
        description = "Change a note's title, colour, facets, pin, or place in the notes tree (parent_id and/or position). It cannot change a note's body.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = true, open_world_hint = false)
    )]
    async fn update_note(
        &self,
        Parameters(p): Parameters<UpdateNoteParams>,
        Extension(parts): Extension<Parts>,
    ) -> Result<CallToolResult, ErrorData> {
        let auth = token(&parts)?;
        if let Err(e) = self.check_book(&auth, &p.book_id).await {
            return Ok(e);
        }
        let existing: HashSet<String> = notes::list_notes_inner(&self.state, &p.book_id)
            .await
            .notes
            .into_iter()
            .map(|n| n.id)
            .collect();
        if !existing.contains(&p.note_id) {
            return Ok(tool_error("note not found"));
        }
        if let Some(title) = &p.title {
            if title.trim().is_empty() {
                return Ok(tool_error("title must not be empty"));
            }
        }
        let mut req = match Self::facets_patch(p.facets.unwrap_or_default(), Some(&p.note_id), &existing) {
            Ok(f) => f,
            Err(e) => return Ok(e),
        };
        req.title = p.title.as_deref().map(|t| t.trim().to_string());
        req.color = p.color.clone();
        req.pinned = p.pinned;
        // Never a body. `UpdateNoteParams` has no field for one and rejects unknown
        // fields, so this is only a statement of fact.
        req.content = None;

        let moving = p.parent_id.is_some() || p.position.is_some();
        let structural = req.title.is_some()
            || req.color.is_some()
            || req.span.is_some()
            || req.relative.is_some()
            || req.is_entity.is_some()
            || req.event_parent.is_some()
            || req.pinned.is_some();
        if !moving && !structural {
            return Ok(tool_error(
                "nothing to change: pass title, color, facets, pinned, parent_id or position",
            ));
        }

        if moving {
            let Some((current_parent, current_index)) =
                self.tree_position(&p.book_id, &p.note_id).await
            else {
                return Ok(tool_error("note not found in the notes tree"));
            };
            let target_parent: Option<String> = match &p.parent_id {
                None => current_parent.clone(),
                Some(None) => None,
                Some(Some(id)) => {
                    if !existing.contains(id) {
                        return Ok(tool_error("parent_id is not a note in this book"));
                    }
                    Some(id.clone())
                }
            };
            let same_list = target_parent == current_parent;
            // `move_note` takes a drop index in the list as it stands (the note still
            // in it); translate the final position the agent asked for into that.
            let index = match p.position {
                Some(pos) => {
                    let pos = pos as usize;
                    if same_list && current_index <= pos { pos + 1 } else { pos }
                }
                None if same_list => current_index,
                None => self.sibling_count(&p.book_id, target_parent.as_deref()).await,
            };
            match notes::move_note_inner(
                &self.state,
                &p.book_id,
                &p.note_id,
                target_parent.as_deref(),
                index,
            )
            .await
            {
                Ok(()) => {}
                Err(plotweb_git::error::GitStoreError::CircularReference) => {
                    return Ok(tool_error("cannot move a note into its own subtree"));
                }
                Err(e) => {
                    eprintln!("[mcp] move note {}: {e}", p.note_id);
                    return Ok(tool_error("could not move the note"));
                }
            }
        }

        if structural {
            if let Err(e) = notes::update_note_inner(&self.state, &p.book_id, &p.note_id, req).await {
                eprintln!("[mcp] update note {}: {e}", p.note_id);
                return Ok(tool_error("could not save the note"));
            }
        }

        let Some(note) = notes::get_note_inner(&self.state, &p.book_id, &p.note_id).await else {
            return Ok(tool_error("note not found"));
        };
        let parent = self
            .tree_position(&p.book_id, &p.note_id)
            .await
            .and_then(|(parent, _)| parent);
        Ok(json_result(json!({ "updated": Self::note_summary(&note, parent.as_deref()) })))
    }

    #[tool(
        description = "Leave a review comment on an exact passage of a chapter. It appears in the author's feedback rail, marked as yours. `quote` must be copied exactly from read_chapter's text (only whitespace may differ).",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = false, open_world_hint = false)
    )]
    async fn add_review_comment(
        &self,
        Parameters(p): Parameters<ReviewCommentParams>,
        Extension(parts): Extension<Parts>,
    ) -> Result<CallToolResult, ErrorData> {
        let auth = token(&parts)?;
        if let Err(e) = self.check_book(&auth, &p.book_id).await {
            return Ok(e);
        }
        let comment = p.comment.trim();
        if comment.is_empty() {
            return Ok(tool_error("comment must not be empty"));
        }
        if let Err(e) = check_len("comment", comment, MAX_COMMENT_CHARS) {
            return Ok(e);
        }
        let quote_text = normalize_ws(&p.quote);
        if quote_text.is_empty() {
            return Ok(tool_error("quote must not be empty"));
        }
        if let Err(e) = check_len("quote", &quote_text, MAX_QUOTE_CHARS) {
            return Ok(e);
        }
        let Some(ch) = chapters::get_chapter_inner(&self.state, &p.book_id, &p.chapter_id).await
        else {
            return Ok(tool_error("chapter not found"));
        };

        // Find the passage: inside one block first (the usual case), then across block
        // boundaries the way the rail does — blocks joined by a single space.
        let blocks = text::text_blocks(&ch.content, Legacy::Markdown);
        let normalized: Vec<String> = blocks.iter().map(|b| normalize_ws(&b.text)).collect();
        let containing = normalized.iter().position(|b| b.contains(&quote_text)).or_else(|| {
            let joined = normalized.join(" ");
            let at = joined.find(&quote_text)?;
            let mut offset = 0;
            normalized.iter().position(|b| {
                let end = offset + b.len();
                let hit = at < end;
                offset = end + 1;
                hit
            })
        });
        let Some(block_index) = containing else {
            return Ok(tool_error(
                "quote was not found in this chapter. Copy the passage exactly from read_chapter \
                 (punctuation, curly quotes, apostrophes and dashes must match; only whitespace \
                 may differ) and keep it within the chapter's text.",
            ));
        };
        // What the beta reader stores too: the enclosing block's text, capped.
        let context_block = truncate_chars(&blocks[block_index].text, CONTEXT_BLOCK_CHARS);

        let fb = match beta::create_agent_feedback(
            &self.state,
            &p.book_id,
            &ch.id,
            &quote_text,
            &context_block,
            comment,
            &auth.token_id,
            &agent_name(&auth),
        )
        .await
        {
            Ok(fb) => fb,
            Err(e) => {
                eprintln!("[mcp] add_review_comment in {}: {e}", p.book_id);
                return Ok(tool_error("could not save the comment"));
            }
        };
        Ok(json_result(json!({
            "feedback_id": fb.id,
            "chapter_id": fb.chapter_id,
            "quote": fb.selected_text,
            "author": fb.reader_name,
        })))
    }

    #[tool(
        description = "List feedback on a book — from beta readers and from AI agents — with replies. Unresolved only unless include_resolved is true; optionally one chapter only.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn list_feedback(
        &self,
        Parameters(p): Parameters<ListFeedbackParams>,
        Extension(parts): Extension<Parts>,
    ) -> Result<CallToolResult, ErrorData> {
        let auth = token(&parts)?;
        if let Err(e) = self.check_book(&auth, &p.book_id).await {
            return Ok(e);
        }
        let titles: HashMap<String, String> = chapters::list_chapters_inner(&self.state, &p.book_id)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|c| (c.id, c.title))
            .collect();
        let items: Vec<Value> = beta::book_feedback(&self.state, &p.book_id)
            .await
            .into_iter()
            .filter(|f| p.include_resolved || !f.resolved)
            .filter(|f| p.chapter_id.as_ref().is_none_or(|c| &f.chapter_id == c))
            .map(|f| {
                json!({
                    "id": f.id,
                    "source": f.source,
                    "author": f.reader_name,
                    "chapter_id": f.chapter_id,
                    "chapter_title": titles.get(&f.chapter_id),
                    "quote": f.selected_text,
                    "comment": f.comment,
                    "resolved": f.resolved,
                    "created_at": f.created_at,
                    "replies": f.replies.iter().map(|r| json!({
                        "author_type": r.author_type,
                        "author": r.author_name,
                        "content": r.content,
                        "created_at": r.created_at,
                    })).collect::<Vec<_>>(),
                })
            })
            .collect();
        Ok(json_result(json!({ "feedback": items })))
    }

    #[tool(
        description = "Reply to a feedback item on this book (a beta reader's or an agent's). The reply is shown to the author, marked as yours; beta readers never see it.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = false, open_world_hint = false)
    )]
    async fn reply_to_feedback(
        &self,
        Parameters(p): Parameters<ReplyParams>,
        Extension(parts): Extension<Parts>,
    ) -> Result<CallToolResult, ErrorData> {
        let auth = token(&parts)?;
        if let Err(e) = self.check_book(&auth, &p.book_id).await {
            return Ok(e);
        }
        let comment = p.comment.trim();
        if comment.is_empty() {
            return Ok(tool_error("comment must not be empty"));
        }
        if let Err(e) = check_len("comment", comment, MAX_COMMENT_CHARS) {
            return Ok(e);
        }
        match beta::create_agent_reply(&self.state, &p.book_id, &p.feedback_id, &agent_name(&auth), comment)
            .await
        {
            None => Ok(tool_error("feedback not found")),
            Some(Err(e)) => {
                eprintln!("[mcp] reply_to_feedback in {}: {e}", p.book_id);
                Ok(tool_error("could not save the reply"))
            }
            Some(Ok(reply)) => Ok(json_result(json!({
                "reply_id": reply.id,
                "feedback_id": reply.feedback_id,
                "author": reply.author_name,
            }))),
        }
    }
}

#[tool_handler]
impl ServerHandler for PlotwebMcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("plotweb", env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCTIONS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn case_insensitive_hits_map_back_to_the_original() {
        assert_eq!(find_ci("The Fog and the fog", "fog"), vec![(4, 7), (16, 19)]);
        assert_eq!(find_ci("abc", ""), vec![]);
        assert_eq!(find_ci("ab", "abc"), vec![]);
        // A character whose lower case is longer (İ → i̇) must not shift later hits.
        let hay = "İx fog";
        assert_eq!(find_ci(hay, "fog"), vec![(3, 6)]);
    }

    #[test]
    fn snippets_are_bounded_and_marked_where_cut() {
        let text = format!("{}needle{}", "a".repeat(200), "b".repeat(200));
        let s = snippet(&text, 200, 206);
        assert!(s.starts_with('…') && s.ends_with('…'));
        assert_eq!(s.chars().count(), 1 + 80 + 6 + 80 + 1);
        assert_eq!(snippet("short needle", 6, 12), "short needle");
    }

    #[test]
    fn whitespace_normalization() {
        assert_eq!(normalize_ws("  a\n\n b\tc "), "a b c");
    }
}
