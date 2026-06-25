mod common;

use axum::http::StatusCode;
use common::TestApp;
use serde_json::json;

async fn create_note(app: &mut TestApp, book: &str, title: &str) -> String {
    let r = app
        .post(
            &format!("/api/books/{book}/notes"),
            &json!({ "title": title, "parent_id": null, "color": null }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "create_note: {}", r.json);
    r.id()
}

async fn root_order(app: &mut TestApp, book: &str) -> Vec<String> {
    let list = app.get(&format!("/api/books/{book}/notes")).await;
    assert_eq!(list.status, StatusCode::OK);
    list.json["tree"]["root_order"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn note_crud_and_tree() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let book = app.create_book("Novel").await;

    let n1 = create_note(&mut app, &book, "Note 1").await;
    let _n2 = create_note(&mut app, &book, "Note 2").await;

    // Update content.
    let upd = app
        .put(
            &format!("/api/books/{book}/notes/{n1}"),
            &json!({ "content": "body text" }),
        )
        .await;
    assert_eq!(upd.status, StatusCode::OK);
    let got = app.get(&format!("/api/books/{book}/notes/{n1}")).await;
    assert_eq!(got.json["content"], "body text");

    // Both appear in the tree root order.
    assert_eq!(root_order(&mut app, &book).await.len(), 2);

    // Delete one.
    let del = app.delete(&format!("/api/books/{book}/notes/{n1}")).await;
    assert_eq!(del.status, StatusCode::OK);
    assert_eq!(root_order(&mut app, &book).await.len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn note_move_downward_is_not_off_by_one() {
    // Audit fix: moving a note DOWN within the same list must land it exactly at
    // the requested index (remove-then-insert previously shifted it one slot up).
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let book = app.create_book("Novel").await;

    let a = create_note(&mut app, &book, "A").await;
    let b = create_note(&mut app, &book, "B").await;
    let c = create_note(&mut app, &book, "C").await;
    assert_eq!(root_order(&mut app, &book).await, vec![a.clone(), b.clone(), c.clone()]);

    // Move A down to index 2 in the live list [A,B,C] — i.e. drop it before C.
    // Correct result is [B, A, C]. The pre-fix code (remove-then-insert at the
    // raw index) produced [B, C, A] — one slot too far.
    let mv = app
        .put(
            &format!("/api/books/{book}/notes/move"),
            &json!({ "note_id": a, "new_parent_id": null, "index": 2 }),
        )
        .await;
    assert_eq!(mv.status, StatusCode::OK, "move: {}", mv.json);
    assert_eq!(
        root_order(&mut app, &book).await,
        vec![b.clone(), a.clone(), c.clone()],
        "downward same-list move landed at the wrong index"
    );

    // Now move C (currently last) up to index 0. Upward moves take no decrement.
    let mv2 = app
        .put(
            &format!("/api/books/{book}/notes/move"),
            &json!({ "note_id": c, "new_parent_id": null, "index": 0 }),
        )
        .await;
    assert_eq!(mv2.status, StatusCode::OK, "move2: {}", mv2.json);
    assert_eq!(
        root_order(&mut app, &book).await,
        vec![c.clone(), b.clone(), a.clone()],
        "upward same-list move landed at the wrong index"
    );
}
