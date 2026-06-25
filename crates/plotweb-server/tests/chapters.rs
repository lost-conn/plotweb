mod common;

use axum::http::StatusCode;
use common::TestApp;
use serde_json::json;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn chapter_crud_and_reorder() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let book = app.create_book("Novel").await;

    let c1 = app.create_chapter(&book, "Chapter One").await;
    let c2 = app.create_chapter(&book, "Chapter Two").await;
    let c3 = app.create_chapter(&book, "Chapter Three").await;

    // List returns them in sort order.
    let list = app.get(&format!("/api/books/{book}/chapters")).await;
    assert_eq!(list.status, StatusCode::OK);
    let ids: Vec<String> = list
        .json
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(ids, vec![c1.clone(), c2.clone(), c3.clone()]);

    // Update content + word count.
    let upd = app
        .put(
            &format!("/api/books/{book}/chapters/{c1}"),
            &json!({ "content": "one two three four five" }),
        )
        .await;
    assert_eq!(upd.status, StatusCode::OK);
    let got = app.get(&format!("/api/books/{book}/chapters/{c1}")).await;
    assert_eq!(got.json["content"], "one two three four five");
    assert_eq!(got.json["word_count"], 5);

    // Reorder: reverse.
    let reorder = app
        .put(
            &format!("/api/books/{book}/chapters/reorder"),
            &json!({ "chapter_ids": [c3, c2, c1] }),
        )
        .await;
    assert_eq!(reorder.status, StatusCode::OK);
    let list2 = app.get(&format!("/api/books/{book}/chapters")).await;
    let ids2: Vec<String> = list2
        .json
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(ids2.first().unwrap(), list2.json[0]["id"].as_str().unwrap());
    assert_eq!(list2.json[0]["title"], "Chapter Three");

    // Delete one.
    let del = app
        .delete(&format!("/api/books/{book}/chapters/{}", list2.json[0]["id"].as_str().unwrap()))
        .await;
    assert_eq!(del.status, StatusCode::OK);
    let list3 = app.get(&format!("/api/books/{book}/chapters")).await;
    assert_eq!(list3.json.as_array().unwrap().len(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn empty_chapter_title_rejected() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let book = app.create_book("Novel").await;
    let r = app
        .post(
            &format!("/api/books/{book}/chapters"),
            &json!({ "title": "  " }),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unicode_chapter_content_roundtrips() {
    // Multibyte content must survive store + retrieve (regression-adjacent to the
    // UTF-8 byte-slice panics fixed in the audit).
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let book = app.create_book("Novel").await;
    let ch = app.create_chapter(&book, "Ch").await;
    let content = "“Smart quotes,” em—dashes, ellipsis… café résumé. ".repeat(20);
    let upd = app
        .put(
            &format!("/api/books/{book}/chapters/{ch}"),
            &json!({ "content": content }),
        )
        .await;
    assert_eq!(upd.status, StatusCode::OK);
    let got = app.get(&format!("/api/books/{book}/chapters/{ch}")).await;
    assert_eq!(got.json["content"], content);
}
