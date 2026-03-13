mod auth;
mod db;
mod routes;

use std::path::PathBuf;
use std::sync::Arc;

use axum::routing::{delete, get, post, put};
use axum::Router;
use plotweb_git::BookStore;
use sqlx::SqlitePool;
use tower_http::services::{ServeDir, ServeFile};
use tower_sessions::cookie::SameSite;
use tower_sessions::{Expiry, MemoryStore, SessionManagerLayer};

#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    pub books: Arc<BookStore>,
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

    // Now slim down the SQLite schema
    db::run_migration_003(&pool).await;

    let state = AppState {
        db: pool,
        books: book_store,
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
        .with_state(state);

    // Static files — serve the built frontend, with SPA fallback to index.html
    let dist_path = std::env::var("DIST_DIR").unwrap_or_else(|_| "../plotweb-web/dist".into());
    let index_path = format!("{}/index.html", dist_path);
    let serve_dir = ServeDir::new(&dist_path).not_found_service(ServeFile::new(&index_path));

    let app = Router::new()
        .merge(api)
        .fallback_service(serve_dir)
        .layer(session_layer);

    let addr = "0.0.0.0:3000";
    println!("PlotWeb server running on http://{}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
