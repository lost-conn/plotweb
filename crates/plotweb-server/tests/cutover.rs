//! Reading from and writing to the canonical document for a cut-over book (phase E).
//!
//! Cutover inverts the direction: the CRDT becomes the source of truth and git becomes
//! the mirror. What these check is that the inversion is real (reads come from the
//! canonical copy), that the mirror keeps working (writes still reach git), and — the
//! part that decides whether cutover is survivable — that it degrades to git rather
//! than to an error when the canonical copy isn't there.

mod common;

use axum::http::StatusCode;
use common::TestApp;
use plotweb_crdt::BodyKind;
use serde_json::json;

fn doc_json(text: &str) -> String {
    format!(
        r#"{{"type":"doc","content":[{{"type":"paragraph","content":[{{"type":"text","text":"{text}"}}]}}]}}"#
    )
}

/// A book whose git copy and canonical copy deliberately say different things, so a
/// read proves which one it came from.
async fn book_with_divergent_copies(app: &mut TestApp) -> (String, String) {
    let book_id = app.create_book("Cutover Book").await;
    let chapter_id = app.create_chapter(&book_id, "One").await;
    let r = app
        .put(
            &format!("/api/books/{book_id}/chapters/{chapter_id}"),
            &json!({ "title": "One", "content": doc_json("what git says") }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);

    let canonical = plotweb_crdt::project_body(&doc_json("what the CRDT says"), BodyKind::Chapter)
        .expect("project");
    let (status, _) = app
        .post_bytes(
            &format!("/api/books/{book_id}/sync/chapter:{chapter_id}/adopt"),
            &canonical,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    (book_id, chapter_id)
}

#[tokio::test]
async fn a_book_not_cut_over_still_reads_from_git() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let (book_id, chapter_id) = book_with_divergent_copies(&mut app).await;

    let r = app
        .get(&format!("/api/books/{book_id}/chapters/{chapter_id}"))
        .await;
    assert!(
        r.json["content"].as_str().unwrap().contains("what git says"),
        "nothing changes for a book that hasn't been cut over"
    );
}

#[tokio::test]
async fn a_cut_over_book_reads_from_the_canonical_document() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let (book_id, chapter_id) = book_with_divergent_copies(&mut app).await;

    app.cut_over(&book_id).await;

    let r = app
        .get(&format!("/api/books/{book_id}/chapters/{chapter_id}"))
        .await;
    let content = r.json["content"].as_str().unwrap().to_string();
    assert!(
        content.contains("what the CRDT says"),
        "the canonical document is the source of truth now: {content}"
    );
}

#[tokio::test]
async fn a_write_to_a_cut_over_book_reaches_both_copies() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let (book_id, chapter_id) = book_with_divergent_copies(&mut app).await;
    app.cut_over(&book_id).await;

    let r = app
        .put(
            &format!("/api/books/{book_id}/chapters/{chapter_id}"),
            &json!({ "title": "One", "content": doc_json("written after cutover") }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);

    // Read back: the canonical copy took the write.
    let r = app
        .get(&format!("/api/books/{book_id}/chapters/{chapter_id}"))
        .await;
    assert!(
        r.json["content"]
            .as_str()
            .unwrap()
            .contains("written after cutover"),
        "the canonical copy took the write"
    );

    // And git took it too — which is what makes the flag reversible to *current*
    // content rather than to whatever git held on cutover day.
    let from_git = plotweb_server::shadow::run_shadow_pass(
        app.book_dir().to_str().unwrap(),
        app.crdt_dir().to_str().unwrap(),
    )
    .await
    .expect("shadow");
    assert!(
        from_git.is_clean(),
        "git mirrors the canonical copy after a cut-over write: {from_git:?}"
    );
}

#[tokio::test]
async fn a_missing_canonical_copy_degrades_to_git_rather_than_failing() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let book_id = app.create_book("Cutover Book").await;
    let chapter_id = app.create_chapter(&book_id, "One").await;
    let r = app
        .put(
            &format!("/api/books/{book_id}/chapters/{chapter_id}"),
            &json!({ "title": "One", "content": doc_json("only in git") }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);

    // Cut over with nothing in the canonical store for this chapter at all.
    app.cut_over(&book_id).await;

    let r = app
        .get(&format!("/api/books/{book_id}/chapters/{chapter_id}"))
        .await;
    assert_eq!(r.status, StatusCode::OK, "a missing copy is not an error");
    assert!(
        r.json["content"].as_str().unwrap().contains("only in git"),
        "serving slightly older content is recoverable; serving nothing looks like data loss"
    );
}
