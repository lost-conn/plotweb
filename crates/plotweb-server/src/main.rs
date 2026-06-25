mod auth;
mod db;
mod email;
mod rhype;
mod rhype_migrate;
mod routes;
mod ws;

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{DefaultBodyLimit, Path, State, WebSocketUpgrade};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{delete, get, post, put};
use axum::Router;
use plotweb_git::BookStore;
use sqlx::SqlitePool;
use tower_http::services::{ServeDir, ServeFile};
use tower_sessions::cookie::SameSite;
use tower_sessions::{Expiry, MemoryStore, SessionManagerLayer};

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

#[tokio::main]
async fn main() {
    let pool = db::init_db().await;

    // Book store (git-backed)
    let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "data/books".into());
    let base_dir = PathBuf::from(&data_dir);
    let book_store = Arc::new(BookStore::new(base_dir.clone()));

    // Migrate existing data from SQLite to git repos (idempotent)
    if let Err(e) = plotweb_git::migrate::migrate_sqlite_to_git(&pool, &base_dir).await {
        eprintln!("Warning: data migration failed: {}", e);
    }

    // Migrate old single-repo layout to split repos (manuscript + notes)
    if let Err(e) = plotweb_git::migrate::migrate_to_split_repos(&base_dir) {
        eprintln!("Warning: split repos migration failed: {}", e);
    }

    // Now slim down the SQLite schema
    db::run_migration_003(&pool).await;

    // Embedded rhypedb metadata store (opens/creates its own data dir).
    let rhype = rhype::RhypeStore::from_env().expect("failed to open rhypedb store");

    // One-time idempotent import of existing metadata from SQLite into rhypedb.
    rhype_migrate::migrate_sqlite_to_rhype(&pool, &rhype).await;

    let state = AppState {
        db: pool,
        rhype,
        books: book_store,
        broadcaster: Arc::new(FeedbackBroadcaster::new()),
        email: EmailService::from_env(),
    };

    // Session store (in-memory — sessions lost on restart, fine for dev)
    let session_store = MemoryStore::default();

    let session_layer = SessionManagerLayer::new(session_store)
        .with_same_site(SameSite::Lax)
        .with_expiry(Expiry::OnInactivity(tower_sessions::cookie::time::Duration::days(30)));

    // API routes
    let api = Router::new()
        .route("/api/auth/register", post(routes::auth::register))
        .route("/api/auth/login", post(routes::auth::login))
        .route("/api/auth/logout", post(routes::auth::logout))
        .route("/api/auth/me", get(routes::auth::me))
        .route("/api/fonts", get(routes::fonts::list))
        .route("/api/books", get(routes::books::list))
        .route("/api/books", post(routes::books::create))
        .route("/api/books/{id}", get(routes::books::get))
        .route("/api/books/{id}", put(routes::books::update))
        .route("/api/books/{id}", delete(routes::books::delete))
        .route(
            "/api/books/{book_id}/chapters",
            get(routes::chapters::list),
        )
        .route(
            "/api/books/{book_id}/chapters",
            post(routes::chapters::create),
        )
        .route(
            "/api/books/{book_id}/chapters/reorder",
            put(routes::chapters::reorder),
        )
        .route(
            "/api/books/{book_id}/chapters/{id}",
            get(routes::chapters::get),
        )
        .route(
            "/api/books/{book_id}/chapters/{id}",
            put(routes::chapters::update),
        )
        .route(
            "/api/books/{book_id}/chapters/{id}",
            delete(routes::chapters::delete),
        )
        // Notes endpoints
        .route(
            "/api/books/{book_id}/notes",
            get(routes::notes::list),
        )
        .route(
            "/api/books/{book_id}/notes",
            post(routes::notes::create),
        )
        .route(
            "/api/books/{book_id}/notes/move",
            put(routes::notes::move_note),
        )
        .route(
            "/api/books/{book_id}/notes/tree",
            put(routes::notes::update_tree),
        )
        .route(
            "/api/books/{book_id}/notes/{id}",
            get(routes::notes::get),
        )
        .route(
            "/api/books/{book_id}/notes/{id}",
            put(routes::notes::update),
        )
        .route(
            "/api/books/{book_id}/notes/{id}",
            delete(routes::notes::delete),
        )
        // History endpoints
        .route(
            "/api/books/{book_id}/history",
            get(routes::history::list),
        )
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
        // Image endpoints
        .route(
            "/api/books/{book_id}/images",
            post(routes::images::upload)
                .layer(DefaultBodyLimit::max(10 * 1024 * 1024)), // 10MB
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
        // Export endpoint
        .route(
            "/api/books/{book_id}/export",
            get(routes::export::export_book),
        )
        // Import endpoints
        .route(
            "/api/books/{book_id}/import/preview",
            post(routes::import::preview),
        )
        .route(
            "/api/books/{book_id}/import/confirm",
            post(routes::import::confirm),
        )
        // Shared books (authenticated)
        .route("/api/shared-books", get(routes::beta::list_shared_books))
        // Beta reader link management (authenticated)
        .route(
            "/api/books/{book_id}/beta-links",
            get(routes::beta::list_links),
        )
        .route(
            "/api/books/{book_id}/beta-links",
            post(routes::beta::create_link),
        )
        .route(
            "/api/books/{book_id}/beta-links/{id}",
            put(routes::beta::update_link),
        )
        .route(
            "/api/books/{book_id}/beta-links/{id}",
            delete(routes::beta::delete_link),
        )
        // Author feedback management (authenticated)
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
        // Public beta reader endpoints (no auth, except claim)
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
        // WebSocket endpoints for real-time feedback
        .route(
            "/api/books/{book_id}/feedback/ws",
            get(ws_author_feedback),
        )
        .route(
            "/api/beta/{token}/feedback/ws",
            get(ws_reader_feedback),
        )
        .with_state(state);

    // Static files — serve the built frontend, with SPA fallback to index.html
    let dist_path = std::env::var("DIST_DIR").unwrap_or_else(|_| "../plotweb-web/dist".into());
    let index_path = format!("{}/index.html", dist_path);
    let serve_dir = ServeDir::new(&dist_path).not_found_service(ServeFile::new(&index_path));

    let app = Router::new()
        // Lightweight liveness probe for the jkbase health check (and any LB).
        // No state, no session — just a 200.
        .route("/health", get(health))
        .merge(api)
        .fallback_service(serve_dir)
        .layer(session_layer);

    let addr = "0.0.0.0:3000";
    println!("PlotWeb server running on http://{}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

/// Liveness probe — returns 200 OK. Used by the jkbase health check.
async fn health() -> StatusCode {
    StatusCode::OK
}

/// WebSocket endpoint for author feedback (authenticated via session).
async fn ws_author_feedback(
    State(state): State<AppState>,
    Path(book_id): Path<String>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_feedback_ws(socket, state, book_id))
}

/// WebSocket endpoint for reader feedback (public, token-based).
async fn ws_reader_feedback(
    State(state): State<AppState>,
    Path(token): Path<String>,
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
        }
    }
}
