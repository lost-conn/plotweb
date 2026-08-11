//! Phase D: does the canonical store still say what git says?
//!
//! The pass exists to catch the one thing the audit cannot. The audit projects git
//! content fresh and checks the projection is lossless — a property of the code. This
//! checks the *stored* document, which clients now write to, against git. The tests
//! below therefore care most about the case where those two legitimately drift apart:
//! a device moves the CRDT through sync while git keeps the older text.

mod common;

use axum::http::StatusCode;
use common::TestApp;
use plotweb_server::shadow::run_shadow_pass;
use serde_json::json;

/// Single-poll block-on for the `FsStore` futures (they resolve on first poll).
fn block_on<F: std::future::Future>(fut: F) -> F::Output {
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
    fn noop(_: *const ()) {}
    fn clone_raw(_: *const ()) -> RawWaker {
        RawWaker::new(std::ptr::null(), &VTABLE)
    }
    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone_raw, noop, noop, noop);
    let waker = unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) };
    let mut cx = Context::from_waker(&waker);
    let mut fut = Box::pin(fut);
    match fut.as_mut().poll(&mut cx) {
        Poll::Ready(v) => v,
        Poll::Pending => panic!("fs store future pended"),
    }
}

async fn shadow(app: &TestApp) -> plotweb_server::shadow::ShadowSummary {
    run_shadow_pass(
        app.book_dir().to_str().unwrap(),
        app.crdt_dir().to_str().unwrap(),
    )
    .await
    .expect("shadow pass")
}

#[tokio::test]
async fn a_freshly_backfilled_corpus_matches_git() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let book_id = app.create_book("Shadow Book").await;
    let chapter_id = app.create_chapter(&book_id, "One").await;
    let r = app
        .put(
            &format!("/api/books/{book_id}/chapters/{chapter_id}"),
            &json!({ "title": "One", "content": "The lantern guttered." }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);

    // Nothing is stored yet: no canonical copy is not a divergence.
    let before = shadow(&app).await;
    assert_eq!(before.diverged.len(), 0);
    assert!(before.absent > 0, "nothing backfilled yet: {before:?}");

    plotweb_server::backfill::run_content_backfill(
        app.book_dir().to_str().unwrap(),
        app.crdt_dir().to_str().unwrap(),
    )
    .await
    .expect("backfill");

    let after = shadow(&app).await;
    assert!(after.compared > 0, "documents were compared: {after:?}");
    assert!(
        after.is_clean(),
        "a corpus straight from the backfill must agree with git: {after:?}"
    );
    assert_eq!(after.matched, after.compared);
}

#[tokio::test]
async fn a_body_a_client_changed_without_git_is_reported_as_diverged() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let book_id = app.create_book("Shadow Book").await;
    let chapter_id = app.create_chapter(&book_id, "One").await;
    let r = app
        .put(
            &format!("/api/books/{book_id}/chapters/{chapter_id}"),
            &json!({ "title": "One", "content": "The lantern guttered." }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);

    // A device claims the document and stores prose git has never seen — the shape of
    // an offline edit whose REST dual-write never landed. The bytes are a *real* body
    // projection (otherwise the pass would rightly call them unreadable rather than
    // diverged); only the text differs from git.
    let drifted = plotweb_crdt::project_body(
        "text only the CRDT knows",
        plotweb_crdt::BodyKind::Chapter,
    )
    .expect("project the drifted body");
    let (status, _) = app
        .post_bytes(
            &format!("/api/books/{book_id}/sync/chapter:{chapter_id}/adopt"),
            &drifted,
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    let summary = shadow(&app).await;
    let flagged: Vec<&String> = summary.diverged.iter().map(|(id, _)| id).collect();
    assert!(
        flagged.iter().any(|id| id.contains(&chapter_id)),
        "the drifted chapter must be reported: {summary:?}"
    );
    assert!(
        !summary.is_clean(),
        "an unclean soak must say so — this is what holds the cutover"
    );
}

#[tokio::test]
async fn a_stored_copy_that_cannot_be_read_is_separated_from_a_divergence() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let book_id = app.create_book("Shadow Book").await;
    let chapter_id = app.create_chapter(&book_id, "One").await;

    // Write junk where a canonical body belongs — the shape of a blob left by an older
    // CRDT. That is "no signal", not "the content disagrees", and the report must not
    // conflate the two. Written through the store, since `FsStore` encodes a key into a
    // flat filename rather than a directory path.
    {
        use rinch_storage::{FsStore, Store};
        let store = FsStore::open(app.crdt_dir().clone()).unwrap();
        block_on(store.put(&format!("chapter:{chapter_id}/snapshot"), b"not a document")).unwrap();
    }

    let summary = shadow(&app).await;
    assert!(
        summary
            .unreadable
            .iter()
            .any(|(id, _)| id.contains(&chapter_id)),
        "unreadable stored copy is reported: {summary:?}"
    );
    assert!(
        summary.diverged.is_empty(),
        "and is not counted as a content divergence: {summary:?}"
    );
}
