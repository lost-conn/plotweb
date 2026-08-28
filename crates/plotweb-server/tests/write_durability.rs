//! A save must never be dropped by both writers.
//!
//! Under cutover a client can declare `sync_owned` — "my sync engine is carrying this
//! body, treat my content as a duplicate" — and the server withholds the write from
//! git. That is correct only while the canonical document is one the server can
//! actually read and write. When it isn't, honouring the declaration leaves git
//! standing down for a sync engine that cannot deliver, and the edit survives nowhere
//! but the author's browser: two days of writing reached neither store in production
//! before anything said so.
//!
//! These pin the rule (git takes the write whenever the canonical copy can't), the
//! exception (a healthy canonical copy still defers), and the reporting that made the
//! original failure invisible — a `200` that persisted nothing.

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

/// `FsStore` percent-encodes `:` and `/` in a key to get a flat filename.
fn blob_path(app: &TestApp, key: &str) -> std::path::PathBuf {
    app.crdt_dir()
        .join(key.replace(':', "%3A").replace('/', "%2F"))
}

/// A cut-over book whose chapter has been claimed by a syncing client.
async fn cut_over_with_claimed_chapter(app: &mut TestApp) -> (String, String) {
    let book_id = app.create_book("Cutover Book").await;
    let chapter_id = app.create_chapter(&book_id, "One").await;
    let r = app
        .put(
            &format!("/api/books/{book_id}/chapters/{chapter_id}"),
            &json!({ "content": doc_json("what git says") }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);

    let canonical =
        plotweb_crdt::project_body(&doc_json("what the CRDT says"), BodyKind::Chapter)
            .expect("project");
    let (status, _) = app
        .post_bytes(
            &format!("/api/books/{book_id}/sync/chapter:{chapter_id}/adopt"),
            &canonical,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    app.cut_over(&book_id).await;
    (book_id, chapter_id)
}

/// Leave the document in place but make its bytes unreadable to this build — the shape
/// of the production failure, where every canonical body was an older projection the
/// current collab seam refuses to load.
fn corrupt_canonical(app: &TestApp, doc_id: &str) {
    let manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(blob_path(app, &format!("{doc_id}/manifest"))).expect("manifest"),
    )
    .expect("manifest json");
    let generation = manifest["generation"].as_str().unwrap_or_default().to_string();
    let key = if generation.is_empty() {
        format!("{doc_id}/snapshot")
    } else {
        format!("{doc_id}/{generation}/snapshot")
    };
    std::fs::write(blob_path(app, &key), b"not a projection this build can read")
        .expect("overwrite snapshot");
}

#[tokio::test]
async fn a_claimed_write_still_reaches_git_when_the_canonical_copy_is_unreadable() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let (book_id, chapter_id) = cut_over_with_claimed_chapter(&mut app).await;
    corrupt_canonical(&app, &format!("chapter:{chapter_id}"));

    let r = app
        .put(
            &format!("/api/books/{book_id}/chapters/{chapter_id}"),
            &json!({ "content": doc_json("written while the canonical copy was broken"),
                     "sync_owned": true }),
        )
        .await;

    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(
        r.json["git"], true,
        "git must take the write when the canonical copy cannot: {}",
        r.json
    );
    assert_eq!(
        r.json["deferred_to_sync"], false,
        "the claim must not be honoured for a document the server cannot read"
    );
    assert!(
        r.json["warning"].is_string(),
        "a degraded write has to say so, or it stays invisible: {}",
        r.json
    );

    let read = app
        .get(&format!("/api/books/{book_id}/chapters/{chapter_id}"))
        .await;
    assert!(
        read.json["content"]
            .as_str()
            .unwrap()
            .contains("written while the canonical copy was broken"),
        "the edit has to be readable back, not just accepted"
    );
}

#[tokio::test]
async fn an_overridden_claim_clears_the_stale_ownership() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let (book_id, chapter_id) = cut_over_with_claimed_chapter(&mut app).await;
    let doc_id = format!("chapter:{chapter_id}");
    corrupt_canonical(&app, &doc_id);

    let heads = app.get(&format!("/api/books/{book_id}/sync/heads")).await;
    assert!(
        heads.json.get(&doc_id).is_some(),
        "precondition: the document starts out claimed"
    );

    app.put(
        &format!("/api/books/{book_id}/chapters/{chapter_id}"),
        &json!({ "content": doc_json("anything"), "sync_owned": true }),
    )
    .await;

    let heads = app.get(&format!("/api/books/{book_id}/sync/heads")).await;
    assert!(
        heads.json.get(&doc_id).is_none(),
        "an overridden claim must be cleared, or the next write stands down too: {}",
        heads.json
    );
}

#[tokio::test]
async fn a_claimed_write_is_still_deferred_when_the_canonical_copy_is_healthy() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let (book_id, chapter_id) = cut_over_with_claimed_chapter(&mut app).await;

    let r = app
        .put(
            &format!("/api/books/{book_id}/chapters/{chapter_id}"),
            &json!({ "content": doc_json("a duplicate of what sync already carries"),
                     "sync_owned": true }),
        )
        .await;

    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(
        r.json["deferred_to_sync"], true,
        "the reappearing-sentence guard still holds for a readable document: {}",
        r.json
    );
    assert_eq!(r.json["git"], false, "git must stay out of a deferred write");
    assert!(
        r.json["warning"].is_null(),
        "an ordinary deferral is not a degraded write"
    );

    let read = app
        .get(&format!("/api/books/{book_id}/chapters/{chapter_id}"))
        .await;
    assert!(
        read.json["content"].as_str().unwrap().contains("what the CRDT says"),
        "the canonical copy is still the source of truth"
    );
}

#[tokio::test]
async fn an_ordinary_write_reports_both_stores() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let (book_id, chapter_id) = cut_over_with_claimed_chapter(&mut app).await;

    let r = app
        .put(
            &format!("/api/books/{book_id}/chapters/{chapter_id}"),
            &json!({ "content": doc_json("an ordinary save") }),
        )
        .await;

    assert_eq!(r.json["git"], true);
    assert_eq!(
        r.json["canonical"], true,
        "a cut-over write lands in both stores: {}",
        r.json
    );
}
