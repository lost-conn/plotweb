mod common;

use axum::http::StatusCode;
use common::TestApp;
use serde_json::json;

/// Set up an author with a book and two chapters, plus a beta link limited to
/// the first chapter (max_chapter_index = 0). Returns (book, ch1, ch2, token).
async fn setup_beta(app: &mut TestApp) -> (String, String, String, String) {
    app.register("author", "password123").await;
    let book = app.create_book("Shared Novel").await;
    let ch1 = app.create_chapter(&book, "Chapter One").await;
    let ch2 = app.create_chapter(&book, "Chapter Two").await;

    let link = app
        .post(
            &format!("/api/books/{book}/beta-links"),
            &json!({ "reader_name": "Reader", "max_chapter_index": 0 }),
        )
        .await;
    assert_eq!(link.status, StatusCode::CREATED, "create link: {}", link.json);
    let token = link.json["token"].as_str().unwrap().to_string();
    (book, ch1, ch2, token)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reader_view_respects_max_chapter_index() {
    let mut app = TestApp::new().await;
    let (_book, _ch1, _ch2, token) = setup_beta(&mut app).await;

    let view = app.get(&format!("/api/beta/{token}")).await;
    assert_eq!(view.status, StatusCode::OK, "{}", view.json);
    // Only the first chapter (sort_order 0) is visible.
    assert_eq!(view.json["chapters"].as_array().unwrap().len(), 1);
    assert_eq!(view.json["chapters"][0]["title"], "Chapter One");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reader_can_read_allowed_chapter_but_not_restricted() {
    let mut app = TestApp::new().await;
    let (_book, ch1, ch2, token) = setup_beta(&mut app).await;

    let ok = app.get(&format!("/api/beta/{token}/chapters/{ch1}")).await;
    assert_eq!(ok.status, StatusCode::OK);

    let blocked = app.get(&format!("/api/beta/{token}/chapters/{ch2}")).await;
    assert_eq!(blocked.status, StatusCode::FORBIDDEN);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn full_feedback_roundtrip() {
    let mut app = TestApp::new().await;
    let (book, ch1, _ch2, token) = setup_beta(&mut app).await;

    // Reader submits feedback on the allowed chapter.
    let fb = app
        .post(
            &format!("/api/beta/{token}/feedback"),
            &json!({
                "chapter_id": ch1,
                "selected_text": "some prose",
                "context_block": "the paragraph",
                "comment": "I love this line"
            }),
        )
        .await;
    assert_eq!(fb.status, StatusCode::CREATED, "{}", fb.json);
    let fb_id = fb.json["id"].as_str().unwrap().to_string();

    // Author sees it.
    let list = app.get(&format!("/api/books/{book}/feedback")).await;
    assert_eq!(list.status, StatusCode::OK);
    assert!(list
        .json
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["id"] == fb_id && f["comment"] == "I love this line"));

    // Author replies.
    let reply = app
        .post(
            &format!("/api/books/{book}/feedback/{fb_id}/replies"),
            &json!({ "content": "Thank you!" }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.json);

    // Resolve it.
    let res = app
        .put(
            &format!("/api/books/{book}/feedback/{fb_id}/resolve"),
            &json!({}),
        )
        .await;
    assert_eq!(res.status, StatusCode::OK);
}
