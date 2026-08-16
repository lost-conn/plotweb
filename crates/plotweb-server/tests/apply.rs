//! Server-side application of a REST write into the canonical document (phase E
//! groundwork).
//!
//! The property these exist to protect is not "the content changed" — that is easy —
//! but "the document a synced device holds is still a peer of ours afterwards". Get
//! that wrong and cutover silently orphans every client on its first write.

mod common;

use axum::http::StatusCode;
use common::TestApp;
use plotweb_crdt::BodyKind;
use plotweb_server::sync::{apply_body_content, body_exchange, BodyExchange};
use serde_json::json;

fn doc_json(text: &str) -> String {
    format!(
        r#"{{"type":"doc","content":[{{"type":"paragraph","content":[{{"type":"text","text":"{text}"}}]}}]}}"#
    )
}

struct Peer {
    doc: yrs::Doc,
}

impl Peer {
    fn holding(update: &[u8]) -> Self {
        use yrs::updates::decoder::Decode;
        use yrs::Transact;
        let doc = yrs::Doc::new();
        doc.transact_mut()
            .apply_update(yrs::Update::decode_v1(update).expect("decodable"))
            .expect("apply");
        Peer { doc }
    }
    fn state_vector(&self) -> Vec<u8> {
        use yrs::updates::encoder::Encode;
        use yrs::{ReadTxn, Transact};
        self.doc.transact().state_vector().encode_v1()
    }
}

/// A book + chapter whose canonical copy a client owns.
async fn owned_chapter(app: &mut TestApp) -> (String, String, Vec<u8>) {
    let book_id = app.create_book("Apply Book").await;
    let chapter_id = app.create_chapter(&book_id, "One").await;
    let bytes = plotweb_crdt::project_body(&doc_json("as the client has it"), BodyKind::Chapter)
        .expect("project");
    let (status, _) = app
        .post_bytes(
            &format!("/api/books/{book_id}/sync/chapter:{chapter_id}/adopt"),
            &bytes,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    (book_id, chapter_id, bytes)
}

#[tokio::test]
async fn an_applied_write_leaves_a_synced_client_able_to_converge() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let (book_id, chapter_id, held) = owned_chapter(&mut app).await;
    let doc_id = format!("chapter:{chapter_id}");
    let peer = Peer::holding(&held);

    let changed = apply_body_content(
        app.crdt_dir(),
        &doc_id,
        "chapter",
        &doc_json("as a REST save changed it"),
        BodyKind::Chapter,
    )
    .expect("apply");
    assert!(changed);

    // The decisive check: the peer's exchange is still a diff, not a refusal. A
    // replacement would have made these two documents unrelated, and the server would
    // (correctly) tell the peer to throw its copy away — losing anything it had offline.
    let outcome = body_exchange(app.crdt_dir(), &doc_id, &peer.state_vector()).expect("exchange");
    match outcome {
        BodyExchange::Diff { diff, .. } => {
            assert!(!diff.is_empty(), "the peer is owed the server's edit");
        }
        BodyExchange::Unrelated => {
            panic!("an applied write must not orphan a client that holds the document")
        }
    }
    let _ = book_id;
}

#[tokio::test]
async fn applying_a_write_does_not_take_ownership() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let book_id = app.create_book("Apply Book").await;
    let chapter_id = app.create_chapter(&book_id, "One").await;
    let r = app
        .put(
            &format!("/api/books/{book_id}/chapters/{chapter_id}"),
            &json!({ "title": "One", "content": doc_json("first") }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);

    // Backfill it, so the document exists but belongs to no client.
    plotweb_server::backfill::run_content_backfill(
        app.book_dir().to_str().unwrap(),
        app.crdt_dir().to_str().unwrap(),
    )
    .await
    .expect("backfill");

    apply_body_content(
        app.crdt_dir(),
        &format!("chapter:{chapter_id}"),
        "chapter",
        &doc_json("second"),
        BodyKind::Chapter,
    )
    .expect("apply");

    // Still un-owned, so the backfill keeps maintaining it. If applying a write took
    // ownership, ordinary saves would quietly remove documents from its care.
    let again = plotweb_server::backfill::run_content_backfill(
        app.book_dir().to_str().unwrap(),
        app.crdt_dir().to_str().unwrap(),
    )
    .await
    .expect("backfill");
    assert_eq!(
        again.skipped_synced, 0,
        "a server-applied write is not a client taking the document: {again:?}"
    );
}

#[tokio::test]
async fn an_applied_write_makes_the_canonical_copy_agree_with_git() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let (book_id, chapter_id, _held) = owned_chapter(&mut app).await;

    // git and the canonical copy disagree to begin with (the adopt above).
    let before = plotweb_server::shadow::run_shadow_pass(
        app.book_dir().to_str().unwrap(),
        app.crdt_dir().to_str().unwrap(),
    )
    .await
    .expect("shadow");
    assert_eq!(before.diverged.len(), 1, "{before:?}");

    // Save through REST, then apply that same content into the canonical document —
    // the pair of writes cutover will perform.
    let content = doc_json("what both should say");
    let r = app
        .put(
            &format!("/api/books/{book_id}/chapters/{chapter_id}"),
            &json!({ "title": "One", "content": content }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    apply_body_content(
        app.crdt_dir(),
        &format!("chapter:{chapter_id}"),
        "chapter",
        &content,
        BodyKind::Chapter,
    )
    .expect("apply");

    let after = plotweb_server::shadow::run_shadow_pass(
        app.book_dir().to_str().unwrap(),
        app.crdt_dir().to_str().unwrap(),
    )
    .await
    .expect("shadow");
    assert!(after.is_clean(), "the two copies now agree: {after:?}");
}
