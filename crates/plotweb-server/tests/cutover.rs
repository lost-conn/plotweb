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

    // Cut over first, *then* write: a write under cutover reaches both copies, which
    // is what brings them into agreement. (Writing beforehand would only touch git and
    // the read would rightly be refused — as the locked test below shows.)
    app.cut_over(&book_id).await;
    let r = app
        .put(
            &format!("/api/books/{book_id}/chapters/{chapter_id}"),
            &json!({ "title": "One", "content": doc_json("what both now say") }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);

    let r = app
        .get(&format!("/api/books/{book_id}/chapters/{chapter_id}"))
        .await;
    assert_eq!(r.status, StatusCode::OK);
    let content = r.json["content"].as_str().unwrap().to_string();
    assert!(
        content.contains("what both now say"),
        "the canonical document is the source of truth now: {content}"
    );
}

#[tokio::test]
async fn a_document_whose_copies_disagree_serves_the_canonical_one() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let (book_id, chapter_id) = book_with_divergent_copies(&mut app).await;
    app.cut_over(&book_id).await;

    // This used to answer 409 on the reasoning that neither side was safe to serve.
    // That was written before anything mirrored sync writes into git — when the two
    // differing meant something had gone wrong. It no longer does: sync lands an edit
    // in the canonical copy at once and the mirror commits it up to thirty seconds
    // later, so git being behind is the ordinary state of a book someone is writing in.
    // Refusing there took the chapter away mid-sentence and gave it back when a timer
    // fired, which in production read as "loading the chapter randomly works or not".
    let r = app
        .get(&format!("/api/books/{book_id}/chapters/{chapter_id}"))
        .await;
    assert_eq!(r.status, StatusCode::OK, "the read must not be refused: {}", r.json);
    assert!(
        r.json["content"].as_str().unwrap().contains("what the CRDT says"),
        "cutover means the canonical document is the source of truth — serving git's \
         copy answers a question the flag has already settled: {}",
        r.json["content"]
    );

    // And a reconcile still decides which copy wins, for the case where the difference
    // is a real one rather than the mirror lagging.
    plotweb_server::reconcile::run_all(
        app.book_dir().to_str().unwrap(),
        app.crdt_dir().to_str().unwrap(),
        plotweb_server::reconcile::Prefer::Git,
        false,
    )
    .await
    .expect("reconcile");

    let r = app
        .get(&format!("/api/books/{book_id}/chapters/{chapter_id}"))
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(
        r.json["content"].as_str().unwrap().contains("what git says"),
        "and it serves the copy the reconcile chose: {}",
        r.json["content"]
    );
}

#[tokio::test]
async fn a_write_to_a_cut_over_book_reaches_both_copies() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let (book_id, chapter_id) = book_with_divergent_copies(&mut app).await;
    app.cut_over(&book_id).await;

    // A write reaches both copies whether or not they agreed beforehand — it is the
    // *read* that refuses ambiguity, and this write is what ends it.
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

/// A `sync_owned` write says "sync is carrying this body". It is a declaration, and it
/// only counts where the canonical document is the source of truth.
mod sync_owned {
    use super::*;

    #[tokio::test]
    async fn is_honoured_for_a_cut_over_book() {
        let mut app = TestApp::new().await;
        app.register("author", "password123").await;
        let book_id = app.create_book("Cutover Book").await;
        let chapter_id = app.create_chapter(&book_id, "One").await;
        app.cut_over(&book_id).await;
        let r = app
            .put(
                &format!("/api/books/{book_id}/chapters/{chapter_id}"),
                &json!({ "content": doc_json("what both copies hold") }),
            )
            .await;
        assert_eq!(r.status, StatusCode::OK);

        // Now a save from a client that also syncs. Its content is a stale duplicate of
        // what sync already carried, so neither copy may take it.
        let r = app
            .put(
                &format!("/api/books/{book_id}/chapters/{chapter_id}"),
                &json!({ "content": doc_json("a stale snapshot"), "sync_owned": true }),
            )
            .await;
        assert_eq!(r.status, StatusCode::OK);

        let git = app
            .state()
            .books
            .get_chapter(&book_id, &chapter_id)
            .await
            .expect("chapter")
            .content;
        assert!(
            git.contains("what both copies hold"),
            "the stale snapshot must not reach git: {git}"
        );
        let r = app
            .get(&format!("/api/books/{book_id}/chapters/{chapter_id}"))
            .await;
        assert_eq!(
            r.status,
            StatusCode::OK,
            "and the copies must still agree — a write to one of them alone locks the \
             document on the next read"
        );
        assert!(r.json["content"].as_str().unwrap().contains("what both copies hold"));
    }

    #[tokio::test]
    async fn is_ignored_for_a_book_that_is_not_cut_over() {
        // The regression this exists for. Deciding client-side alone dropped the write
        // for every book that had *not* been cut over — where git is still the source
        // of truth and this is the only write that reaches it. The edit landed in the
        // canonical store, never reached git, and vanished on the next read.
        let mut app = TestApp::new().await;
        app.register("author", "password123").await;
        let book_id = app.create_book("Ordinary Book").await;
        let chapter_id = app.create_chapter(&book_id, "One").await;

        let r = app
            .put(
                &format!("/api/books/{book_id}/chapters/{chapter_id}"),
                &json!({ "content": doc_json("typed with sync on"), "sync_owned": true }),
            )
            .await;
        assert_eq!(r.status, StatusCode::OK);

        let r = app
            .get(&format!("/api/books/{book_id}/chapters/{chapter_id}"))
            .await;
        assert!(
            r.json["content"].as_str().unwrap().contains("typed with sync on"),
            "git is the truth here, so the write must land: {}",
            r.json["content"]
        );
    }

    #[tokio::test]
    async fn never_covers_a_title_or_a_note_colour() {
        // The flag is about the body. Structure is not carried by sync and must still
        // be written, cut over or not.
        let mut app = TestApp::new().await;
        app.register("author", "password123").await;
        let book_id = app.create_book("Cutover Book").await;
        let chapter_id = app.create_chapter(&book_id, "One").await;
        app.cut_over(&book_id).await;

        let r = app
            .put(
                &format!("/api/books/{book_id}/chapters/{chapter_id}"),
                &json!({ "title": "One, renamed", "sync_owned": true }),
            )
            .await;
        assert_eq!(r.status, StatusCode::OK);

        let list = app.get(&format!("/api/books/{book_id}/chapters")).await;
        assert_eq!(list.json.as_array().unwrap()[0]["title"], "One, renamed");
    }
}

/// A client cannot tell the author the truth about where their writing goes unless it
/// knows whether this book is cut over. For a cut-over book sync is how an edit reaches
/// the server, so a device with sync off is writing only to itself — and "Saved" meaning
/// two different things depending on a flag nobody can see is the shape of every loss in
/// this arc.
#[tokio::test]
async fn a_book_reports_whether_it_is_cut_over() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let book_id = app.create_book("Ordinary Novel").await;

    let r = app.get(&format!("/api/books/{book_id}")).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(
        r.json["cutover"], false,
        "a book that is not cut over must say so"
    );

    app.cut_over(&book_id).await;

    let r = app.get(&format!("/api/books/{book_id}")).await;
    assert_eq!(r.json["cutover"], true);

    // And in the shelf listing, which is where the dashboard reads from.
    let list = app.get("/api/books").await;
    let books = list.json.as_array().expect("a book list");
    let entry = books
        .iter()
        .find(|b| b["id"] == book_id.as_str())
        .expect("the book is listed");
    assert_eq!(entry["cutover"], true);
}
