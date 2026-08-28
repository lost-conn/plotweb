mod common;

use axum::http::StatusCode;
use common::TestApp;
use serde_json::json;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn book_crud_lifecycle() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;

    // Create
    let id = app.create_book("My Novel").await;

    // List shows it
    let list = app.get("/api/books").await;
    assert_eq!(list.status, StatusCode::OK);
    assert!(list.json.as_array().unwrap().iter().any(|b| b["id"] == id));

    // Get
    let got = app.get(&format!("/api/books/{id}")).await;
    assert_eq!(got.status, StatusCode::OK);
    assert_eq!(got.json["title"], "My Novel");

    // Update title
    let upd = app
        .put(
            &format!("/api/books/{id}"),
            &json!({ "title": "Renamed Novel" }),
        )
        .await;
    assert_eq!(upd.status, StatusCode::OK);
    let got2 = app.get(&format!("/api/books/{id}")).await;
    assert_eq!(got2.json["title"], "Renamed Novel");

    // Delete
    let del = app.delete(&format!("/api/books/{id}")).await;
    assert_eq!(del.status, StatusCode::OK);
    let gone = app.get(&format!("/api/books/{id}")).await;
    assert_eq!(gone.status, StatusCode::NOT_FOUND);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn create_book_requires_auth() {
    let mut app = TestApp::new().await;
    let r = app
        .post("/api/books", &json!({ "title": "X", "description": "" }))
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn blank_title_update_rejected() {
    // Audit fix: update must reject blank titles (was: silently blanked listing).
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let id = app.create_book("Keep Me").await;
    let r = app
        .put(&format!("/api/books/{id}"), &json!({ "title": "   " }))
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    let got = app.get(&format!("/api/books/{id}")).await;
    assert_eq!(got.json["title"], "Keep Me");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cannot_access_another_users_book() {
    // IDOR: a second user must not read/update/delete the first user's book.
    let mut app = TestApp::new().await;
    app.register("owner", "password123").await;
    let book = app.create_book("Private").await;

    // Switch to a different user.
    app.logout_local();
    app.register("intruder", "password123").await;

    let get = app.get(&format!("/api/books/{book}")).await;
    assert_eq!(get.status, StatusCode::NOT_FOUND);

    let upd = app
        .put(&format!("/api/books/{book}"), &json!({ "title": "Hacked" }))
        .await;
    assert_eq!(upd.status, StatusCode::NOT_FOUND);

    let del = app.delete(&format!("/api/books/{book}")).await;
    assert_eq!(del.status, StatusCode::NOT_FOUND);

    // The owner's view is untouched.
    app.logout_local();
    app.login("owner", "password123").await;
    let got = app.get(&format!("/api/books/{book}")).await;
    assert_eq!(got.json["title"], "Private");
}

/// The legacy SQLite→git pass must not run against a post-003 database.
///
/// It reads the pre-003 `books` schema, so on a migrated database its query cannot
/// parse and it reported `data migration failed: no such column: description` on every
/// boot — a warning with nothing behind it, on the exact line a genuine migration
/// failure would use.
#[tokio::test]
async fn the_legacy_schema_check_tracks_migration_003() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_url = format!("sqlite:{}/legacy.db", dir.path().display());
    let pool = plotweb_server::db::init_db_with(&db_url).await;

    // 001 creates `books` with `description`, so a fresh database looks legacy until
    // 003 has run — which is what makes the one-time pass worth attempting exactly once.
    assert!(
        plotweb_server::db::has_legacy_books_schema(&pool).await,
        "a database that has not been through 003 still carries the legacy schema"
    );

    plotweb_server::db::run_migration_003(&pool).await;

    assert!(
        !plotweb_server::db::has_legacy_books_schema(&pool).await,
        "after 003 the legacy pass must be skipped, not attempted and reported as failed"
    );
}
