//! Regression tests for the security/correctness bugs found in the audit.
//! Each test fails on the pre-fix code (panic, IDOR, or access-control bypass).

mod common;

use axum::http::StatusCode;
use common::TestApp;
use serde_json::json;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn chapter_path_traversal_is_rejected() {
    // Pre-fix: chapter_id was interpolated into a filesystem path with no
    // validation. A non-UUID / traversal id must now be a clean 404, never a
    // panic or a read outside the book directory.
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let book = app.create_book("Novel").await;

    for bad in [
        "..%2F..%2F..%2Fetc%2Fpasswd",
        "not-a-uuid",
        "%2e%2e%2fsecret",
    ] {
        let get = app.get(&format!("/api/books/{book}/chapters/{bad}")).await;
        assert_eq!(
            get.status,
            StatusCode::NOT_FOUND,
            "GET traversal `{bad}` should be 404, got {}",
            get.status
        );

        let upd = app
            .put(
                &format!("/api/books/{book}/chapters/{bad}"),
                &json!({ "content": "x" }),
            )
            .await;
        assert_ne!(
            upd.status,
            StatusCode::OK,
            "PUT traversal `{bad}` must not succeed"
        );

        let del = app.delete(&format!("/api/books/{book}/chapters/{bad}")).await;
        assert_ne!(
            del.status,
            StatusCode::OK,
            "DELETE traversal `{bad}` must not succeed"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn note_path_traversal_is_rejected() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let book = app.create_book("Novel").await;

    let get = app
        .get(&format!("/api/books/{book}/notes/..%2F..%2Fsecret"))
        .await;
    assert_eq!(get.status, StatusCode::NOT_FOUND);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn beta_reader_cannot_submit_feedback_beyond_max_chapter() {
    // Pre-fix: reader_create_feedback stored chapter_id verbatim with no range
    // check, so a restricted reader could comment on a hidden chapter.
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let book = app.create_book("Novel").await;
    let _ch1 = app.create_chapter(&book, "One").await;
    let ch2 = app.create_chapter(&book, "Two").await; // sort_order 1, restricted

    let link = app
        .post(
            &format!("/api/books/{book}/beta-links"),
            &json!({ "reader_name": "Reader", "max_chapter_index": 0 }),
        )
        .await;
    let token = link.json["token"].as_str().unwrap().to_string();

    let fb = app
        .post(
            &format!("/api/beta/{token}/feedback"),
            &json!({
                "chapter_id": ch2,
                "selected_text": "hidden",
                "context_block": "",
                "comment": "should be blocked"
            }),
        )
        .await;
    assert_eq!(
        fb.status,
        StatusCode::FORBIDDEN,
        "feedback on a restricted chapter must be 403, got {}: {}",
        fb.status,
        fb.json
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn author_cannot_reply_to_another_books_feedback() {
    // Pre-fix: author_reply_to_feedback checked book ownership but not that the
    // feedback belonged to that book — so an author could inject replies onto a
    // different book's feedback.
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;

    // Book B with a chapter, a beta link, and one piece of reader feedback.
    let book_b = app.create_book("Book B").await;
    let ch_b = app.create_chapter(&book_b, "B1").await;
    let link = app
        .post(
            &format!("/api/books/{book_b}/beta-links"),
            &json!({ "reader_name": "Reader" }),
        )
        .await;
    let token = link.json["token"].as_str().unwrap().to_string();
    let fb = app
        .post(
            &format!("/api/beta/{token}/feedback"),
            &json!({
                "chapter_id": ch_b,
                "selected_text": "x",
                "context_block": "",
                "comment": "real feedback on B"
            }),
        )
        .await;
    let fb_id = fb.json["id"].as_str().unwrap().to_string();

    // Book A, owned by the same author.
    let book_a = app.create_book("Book A").await;

    // Replying via book A's path to book B's feedback must be rejected.
    let cross = app
        .post(
            &format!("/api/books/{book_a}/feedback/{fb_id}/replies"),
            &json!({ "content": "smuggled reply" }),
        )
        .await;
    assert_eq!(
        cross.status,
        StatusCode::NOT_FOUND,
        "cross-book reply must be 404, got {}: {}",
        cross.status,
        cross.json
    );

    // The legitimate path (book B) still works.
    let ok = app
        .post(
            &format!("/api/books/{book_b}/feedback/{fb_id}/replies"),
            &json!({ "content": "legit reply" }),
        )
        .await;
    assert_eq!(ok.status, StatusCode::CREATED);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn import_preview_handles_multibyte_utf8() {
    // Pre-fix: the preview built `&content[..200]`, panicking when byte 200 split
    // a multibyte char. Use prose whose 200th byte lands mid-character.
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let book = app.create_book("Novel").await;

    // "é" is 2 bytes; a long run guarantees byte 200 falls inside a char.
    let body = format!("# Chapter\n\n{}", "é".repeat(300));
    let r = app
        .post_multipart(&format!("/api/books/{book}/import/preview"), "manuscript.md", body.as_bytes())
        .await;
    assert_eq!(r.status, StatusCode::OK, "import preview: {}", r.json);
    assert!(r.json["chapters"].as_array().map(|c| !c.is_empty()).unwrap_or(false));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn author_feedback_ws_requires_auth() {
    // The author feedback WebSocket gained an AuthSession + ownership gate.
    // A plain (non-WebSocket) unauthenticated request must not reach the handler;
    // it is rejected by the extractor (401) rather than upgrading.
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let book = app.create_book("Novel").await;
    app.logout_local();

    let r = app.get(&format!("/api/books/{book}/feedback/ws")).await;
    assert!(
        r.status == StatusCode::UNAUTHORIZED || r.status == StatusCode::NOT_FOUND,
        "unauthenticated author WS must be 401/404, got {}",
        r.status
    );
}
