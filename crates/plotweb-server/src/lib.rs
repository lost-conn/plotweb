//! PlotWeb server library.
//!
//! The Axum app is constructed here (rather than inline in `main`) so it can be
//! built in-process by integration tests. `main.rs` is a thin wrapper that wires
//! up real env-based state, adds static-file serving, and serves.

pub mod auth;
pub mod db;
pub mod email;
pub mod rhype;
pub mod rhype_migrate;
pub mod routes;
pub mod ws;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{DefaultBodyLimit, Path as AxumPath, State, WebSocketUpgrade};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{delete, get, post, put};
use axum::Router;
use plotweb_git::BookStore;
use sqlx::SqlitePool;
use tower_sessions::cookie::SameSite;
use tower_sessions::{Expiry, SessionManagerLayer};
use tower_sessions_sqlx_store::SqliteStore;

use crate::auth::AuthSession;
use crate::email::EmailService;
use crate::ws::FeedbackBroadcaster;

#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    /// Embedded rhypedb metadata store. Added alongside `db` during the SQLite→
    /// rhypedb migration; query sites move over incrementally, then `db` is
    /// removed.
    pub rhype: rhype::RhypeStore,
    pub books: Arc<BookStore>,
    pub broadcaster: Arc<FeedbackBroadcaster>,
    pub email: Option<Arc<EmailService>>,
}

/// Build `AppState` from explicit paths. Used by `main` (via env) and by tests
/// (via tempdirs). Runs the same migration sequence as production minus the
/// one-time data migrations, which are no-ops on an empty store.
pub async fn build_state(
    db_url: &str,
    book_dir: PathBuf,
    rhype_dir: impl AsRef<Path>,
    email: Option<Arc<EmailService>>,
) -> AppState {
    let pool = db::init_db_with(db_url).await;
    let books = Arc::new(BookStore::new(book_dir.clone()));

    // One-time data migrations are no-ops on an empty/fresh store, but the
    // SQLite→git pass needs the legacy tables to exist (they do, from 001).
    if let Err(e) = plotweb_git::migrate::migrate_sqlite_to_git(&pool, &book_dir).await {
        eprintln!("Warning: data migration failed: {e}");
    }
    if let Err(e) = plotweb_git::migrate::migrate_to_split_repos(&book_dir) {
        eprintln!("Warning: split repos migration failed: {e}");
    }
    db::run_migration_003(&pool).await;

    let rhype = rhype::RhypeStore::open(rhype_dir).expect("failed to open rhypedb store");
    rhype_migrate::migrate_sqlite_to_rhype(&pool, &rhype).await;

    AppState {
        db: pool,
        rhype,
        books,
        broadcaster: Arc::new(FeedbackBroadcaster::new()),
        email,
    }
}

/// The session layer (cookie-based, SQLite-backed store over the shared DB
/// pool). Sessions persist to disk, so a signed-in user stays signed in across
/// server restarts. `migrate()` creates the session table if it doesn't exist.
pub async fn session_layer(pool: sqlx::SqlitePool) -> SessionManagerLayer<SqliteStore> {
    let store = SqliteStore::new(pool);
    store
        .migrate()
        .await
        .expect("failed to migrate session store");
    SessionManagerLayer::new(store)
        .with_same_site(SameSite::Lax)
        .with_expiry(Expiry::OnInactivity(
            tower_sessions::cookie::time::Duration::days(30),
        ))
}

/// All `/api/*` routes plus `/health`, with state attached. No session layer and
/// no static-file serving — callers add those.
pub fn api_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/api/auth/register", post(routes::auth::register))
        .route("/api/auth/login", post(routes::auth::login))
        .route("/api/auth/logout", post(routes::auth::logout))
        .route("/api/auth/me", get(routes::auth::me))
        .route(
            "/api/auth/forgot-password",
            post(routes::auth::forgot_password),
        )
        .route(
            "/api/auth/reset-password",
            post(routes::auth::reset_password),
        )
        .route("/api/fonts", get(routes::fonts::list))
        .route("/api/books", get(routes::books::list))
        .route("/api/books", post(routes::books::create))
        .route("/api/books/{id}", get(routes::books::get))
        .route("/api/books/{id}", put(routes::books::update))
        .route("/api/books/{id}", delete(routes::books::delete))
        .route("/api/books/{book_id}/chapters", get(routes::chapters::list))
        .route("/api/books/{book_id}/chapters", post(routes::chapters::create))
        .route(
            "/api/books/{book_id}/chapters/reorder",
            put(routes::chapters::reorder),
        )
        .route("/api/books/{book_id}/chapters/{id}", get(routes::chapters::get))
        .route("/api/books/{book_id}/chapters/{id}", put(routes::chapters::update))
        .route(
            "/api/books/{book_id}/chapters/{id}",
            delete(routes::chapters::delete),
        )
        .route("/api/books/{book_id}/notes", get(routes::notes::list))
        .route("/api/books/{book_id}/notes", post(routes::notes::create))
        .route("/api/books/{book_id}/notes/move", put(routes::notes::move_note))
        .route("/api/books/{book_id}/notes/tree", put(routes::notes::update_tree))
        .route("/api/books/{book_id}/notes/{id}", get(routes::notes::get))
        .route("/api/books/{book_id}/notes/{id}", put(routes::notes::update))
        .route("/api/books/{book_id}/notes/{id}", delete(routes::notes::delete))
        .route("/api/books/{book_id}/history", get(routes::history::list))
        .route(
            "/api/books/{book_id}/history/{commit}/chapters",
            get(routes::history::list_chapters),
        )
        .route(
            "/api/books/{book_id}/history/{commit}/chapters/{id}",
            get(routes::history::get_chapter),
        )
        .route(
            "/api/books/{book_id}/history/{commit}/restore",
            post(routes::history::restore),
        )
        .route(
            "/api/books/{book_id}/history/{commit}/diff",
            get(routes::history::diff),
        )
        .route(
            "/api/books/{book_id}/images",
            post(routes::images::upload).layer(DefaultBodyLimit::max(10 * 1024 * 1024)),
        )
        .route(
            "/api/books/{book_id}/images/{filename}",
            get(routes::images::serve),
        )
        .route(
            "/api/books/{book_id}/images/{filename}",
            delete(routes::images::delete),
        )
        .route(
            "/api/beta/{token}/images/{filename}",
            get(routes::images::serve_beta),
        )
        .route("/api/books/{book_id}/export", get(routes::export::export_book))
        .route(
            "/api/books/{book_id}/import/preview",
            post(routes::import::preview),
        )
        .route(
            "/api/books/{book_id}/import/confirm",
            post(routes::import::confirm),
        )
        .route("/api/shared-books", get(routes::beta::list_shared_books))
        .route("/api/books/{book_id}/beta-links", get(routes::beta::list_links))
        .route("/api/books/{book_id}/beta-links", post(routes::beta::create_link))
        .route(
            "/api/books/{book_id}/beta-links/{id}",
            put(routes::beta::update_link),
        )
        .route(
            "/api/books/{book_id}/beta-links/{id}",
            delete(routes::beta::delete_link),
        )
        .route(
            "/api/books/{book_id}/feedback",
            get(routes::beta::list_book_feedback),
        )
        .route(
            "/api/books/{book_id}/feedback/{id}/resolve",
            put(routes::beta::resolve_feedback),
        )
        .route(
            "/api/books/{book_id}/feedback/{id}",
            delete(routes::beta::delete_feedback),
        )
        .route(
            "/api/books/{book_id}/feedback/{id}/replies",
            post(routes::beta::author_reply_to_feedback),
        )
        .route("/api/beta/{token}/claim", post(routes::beta::claim_link))
        .route("/api/beta/{token}", get(routes::beta::reader_view))
        .route(
            "/api/beta/{token}/chapters/{id}",
            get(routes::beta::reader_chapter),
        )
        .route(
            "/api/beta/{token}/feedback",
            get(routes::beta::reader_list_feedback),
        )
        .route(
            "/api/beta/{token}/feedback",
            post(routes::beta::reader_create_feedback),
        )
        .route(
            "/api/beta/{token}/feedback/{id}/replies",
            post(routes::beta::reader_reply_to_feedback),
        )
        .route(
            "/api/beta/{token}/progress",
            put(routes::beta::reader_update_progress),
        )
        .route(
            "/api/beta/{token}/bookmarks",
            get(routes::beta::reader_list_bookmarks),
        )
        .route(
            "/api/beta/{token}/bookmarks",
            post(routes::beta::reader_create_bookmark),
        )
        .route(
            "/api/beta/{token}/bookmarks/{id}",
            delete(routes::beta::reader_delete_bookmark),
        )
        .route("/api/books/{book_id}/feedback/ws", get(ws_author_feedback))
        .route("/api/beta/{token}/feedback/ws", get(ws_reader_feedback))
        .with_state(state)
}

/// A complete, ready-to-serve router for tests: api routes + session layer, no
/// static-file serving.
pub async fn test_router(state: AppState) -> Router {
    let layer = session_layer(state.db.clone()).await;
    api_router(state).layer(layer)
}

/// Liveness probe — returns 200 OK. Used by the jkbase health check.
pub async fn health() -> StatusCode {
    StatusCode::OK
}

/// WebSocket endpoint for author feedback (authenticated via session).
pub async fn ws_author_feedback(
    State(state): State<AppState>,
    AuthSession(user_id): AuthSession,
    AxumPath(book_id): AxumPath<String>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    if !crate::routes::verify_book_ownership(&state, &book_id, &user_id).await {
        return StatusCode::NOT_FOUND.into_response();
    }
    ws.on_upgrade(move |socket| handle_feedback_ws(socket, state, book_id))
        .into_response()
}

/// WebSocket endpoint for reader feedback (public, token-based).
pub async fn ws_reader_feedback(
    State(state): State<AppState>,
    AxumPath(token): AxumPath<String>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    // Look up book_id from token (active links only)
    let book_id = state
        .rhype
        .find_one(format!(
            "BetaLink.filter(.token == {} && .active == true).limit(1)",
            rhype::quote(&token)
        ))
        .await
        .ok()
        .flatten()
        .and_then(|o| o.string("book_id"))
        .unwrap_or_default();

    ws.on_upgrade(move |socket| handle_feedback_ws(socket, state, book_id))
}

async fn handle_feedback_ws(mut socket: WebSocket, state: AppState, book_id: String) {
    if book_id.is_empty() {
        return;
    }

    let mut rx = state.broadcaster.subscribe(&book_id);

    // Keepalive ping so dead half-open connections are detected.
    let mut ping = tokio::time::interval(std::time::Duration::from_secs(30));
    ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Ok(text) => {
                        if socket.send(Message::Text(text.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {} // Ignore other incoming messages
                }
            }
            _ = ping.tick() => {
                if socket.send(Message::Ping(Vec::new().into())).await.is_err() {
                    break;
                }
            }
        }
    }

    // Drop our subscription and prune the channel if no readers remain.
    drop(rx);
    state.broadcaster.cleanup(&book_id);
}
