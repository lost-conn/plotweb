//! Reconciling a diverged, client-owned document (phase D→E).
//!
//! The backfill refuses to touch these — re-projecting would fork a disjoint history —
//! so this is the only path that resolves them, and it resolves them in whichever
//! direction a human chose. Both directions are exercised, plus the dry run, because a
//! tool that rewrites someone's prose should be provably inert until told otherwise.

mod common;

use axum::http::StatusCode;
use common::TestApp;
use plotweb_server::reconcile::{run_all, Prefer};
use serde_json::json;

/// A yrs peer, as the real client is for bodies.
struct BodyDevice {
    doc: yrs::Doc,
}

impl BodyDevice {
    fn new() -> Self {
        BodyDevice { doc: yrs::Doc::new() }
    }
    fn full(&self) -> Vec<u8> {
        use yrs::{ReadTxn, StateVector, Transact};
        self.doc
            .transact()
            .encode_state_as_update_v1(&StateVector::default())
    }
}

/// A book + chapter whose canonical copy a client owns and which disagrees with git.
async fn diverged_chapter(app: &mut TestApp) -> (String, String) {
    let book_id = app.create_book("Reconcile Book").await;
    let chapter_id = app.create_chapter(&book_id, "One").await;
    let r = app
        .put(
            &format!("/api/books/{book_id}/chapters/{chapter_id}"),
            &json!({ "title": "One", "content": "What git believes." }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);

    // A client claims the document with different content.
    let device = BodyDevice::new();
    let bytes = plotweb_crdt::project_body("What the CRDT believes.", plotweb_crdt::BodyKind::Chapter)
        .expect("project");
    let _ = device;
    let (status, _) = app
        .post_bytes(
            &format!("/api/books/{book_id}/sync/chapter:{chapter_id}/adopt"),
            &bytes,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    (book_id, chapter_id)
}

#[tokio::test]
async fn a_dry_run_changes_nothing() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let (book_id, chapter_id) = diverged_chapter(&mut app).await;

    let summary = run_all(
        app.book_dir().to_str().unwrap(),
        app.crdt_dir().to_str().unwrap(),
        Prefer::Git,
        true,
    )
    .await
    .expect("dry run");
    assert_eq!(summary.resolved.len(), 1, "it reports what it would do: {summary:?}");

    // Git untouched...
    let r = app
        .get(&format!("/api/books/{book_id}/chapters/{chapter_id}"))
        .await;
    assert!(
        r.json["content"].as_str().unwrap().contains("What git believes"),
        "git is unchanged by a dry run"
    );
    // ...and the divergence is still there.
    let after = plotweb_server::shadow::run_shadow_pass(
        app.book_dir().to_str().unwrap(),
        app.crdt_dir().to_str().unwrap(),
    )
    .await
    .expect("shadow");
    assert_eq!(after.diverged.len(), 1, "a dry run resolves nothing: {after:?}");
}

#[tokio::test]
async fn preferring_git_replaces_the_canonical_copy_and_releases_ownership() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let (_book_id, chapter_id) = diverged_chapter(&mut app).await;

    let summary = run_all(
        app.book_dir().to_str().unwrap(),
        app.crdt_dir().to_str().unwrap(),
        Prefer::Git,
        false,
    )
    .await
    .expect("reconcile");
    assert_eq!(summary.resolved.len(), 1, "{summary:?}");
    assert!(summary.errors.is_empty(), "{summary:?}");

    let after = plotweb_server::shadow::run_shadow_pass(
        app.book_dir().to_str().unwrap(),
        app.crdt_dir().to_str().unwrap(),
    )
    .await
    .expect("shadow");
    assert!(
        after.diverged.is_empty() && after.stale.is_empty(),
        "the two copies now agree: {after:?}"
    );

    // Ownership was released, so the backfill will maintain this document again — the
    // thing that stops it drifting once more with nothing able to fix it.
    let backfill = plotweb_server::backfill::run_content_backfill(
        app.book_dir().to_str().unwrap(),
        app.crdt_dir().to_str().unwrap(),
    )
    .await
    .expect("backfill");
    assert_eq!(
        backfill.skipped_synced, 0,
        "no document should still be client-owned: {backfill:?}"
    );
    let _ = chapter_id;
}

#[tokio::test]
async fn preferring_the_crdt_writes_the_stored_document_into_git() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let (book_id, chapter_id) = diverged_chapter(&mut app).await;

    let summary = run_all(
        app.book_dir().to_str().unwrap(),
        app.crdt_dir().to_str().unwrap(),
        Prefer::Crdt,
        false,
    )
    .await
    .expect("reconcile");
    assert_eq!(summary.resolved.len(), 1, "{summary:?}");

    // Git now serves the client's text, through the ordinary read path.
    let r = app
        .get(&format!("/api/books/{book_id}/chapters/{chapter_id}"))
        .await;
    let content = r.json["content"].as_str().unwrap().to_string();
    assert!(
        content.contains("What the CRDT believes"),
        "git took the stored document's content: {content}"
    );

    let after = plotweb_server::shadow::run_shadow_pass(
        app.book_dir().to_str().unwrap(),
        app.crdt_dir().to_str().unwrap(),
    )
    .await
    .expect("shadow");
    assert!(after.is_clean(), "and the two copies agree: {after:?}");
}
