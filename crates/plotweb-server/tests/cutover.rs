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

    // The canonical copy is what a cut-over book reads, and under one writer it is the
    // only thing a body edit can move — REST no longer carries body content here.
    let r = app
        .get(&format!("/api/books/{book_id}/chapters/{chapter_id}"))
        .await;
    assert_eq!(r.status, StatusCode::OK);
    let content = r.json["content"].as_str().unwrap().to_string();
    assert!(
        content.contains("what the CRDT says"),
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

/// One writer: a body edit sent over REST to a cut-over book is **not** applied.
///
/// It can only be a stale whole-state copy of what the ops already carried — applying
/// it re-inserts text the author has since deleted, which is the reappearing-sentence
/// bug. Current clients do not send it; an older one might, and it is dropped rather
/// than trusted. Structure on the same request still lands, because REST is still the
/// writer for structure.
#[tokio::test]
async fn a_cut_over_book_ignores_a_body_write_sent_over_rest() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let (book_id, chapter_id) = book_with_divergent_copies(&mut app).await;
    app.cut_over(&book_id).await;

    let r = app
        .put(
            &format!("/api/books/{book_id}/chapters/{chapter_id}"),
            &json!({ "title": "One, renamed", "content": doc_json("a stale whole-state copy") }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);

    let r = app
        .get(&format!("/api/books/{book_id}/chapters/{chapter_id}"))
        .await;
    assert!(
        !r.json["content"]
            .as_str()
            .unwrap()
            .contains("a stale whole-state copy"),
        "a whole-state body write must not reach a cut-over book: {}",
        r.json["content"]
    );
    assert_eq!(
        r.json["title"].as_str(),
        Some("One, renamed"),
        "structure on the same request still lands — REST still writes that"
    );
}

/// The counterpart: where the book is **not** cut over, git is the truth and this write
/// is the only thing that reaches it, so it must still carry content.
#[tokio::test]
async fn a_book_that_is_not_cut_over_still_takes_a_rest_body_write() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let book_id = app.create_book("Ordinary Novel").await;
    let chapter_id = app.create_chapter(&book_id, "One").await;

    let r = app
        .put(
            &format!("/api/books/{book_id}/chapters/{chapter_id}"),
            &json!({ "content": doc_json("written over REST") }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);

    let r = app
        .get(&format!("/api/books/{book_id}/chapters/{chapter_id}"))
        .await;
    assert!(
        r.json["content"].as_str().unwrap().contains("written over REST"),
        "git is still the writer for a book that has not been cut over: {}",
        r.json["content"]
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

/// What replaced the `sync_owned` declaration.
///
/// There used to be a flag on the wire saying "sync is carrying this body", and the
/// server honoured it only where the book was cut over *and* the canonical document was
/// readable. It was a negotiation between two writers over which should stand down, and
/// its failures cost — in order — two days of silently dropped writes, the
/// reappearing-sentence bug, and a writing session. There is one writer now, decided
/// from state the server can verify, so there is nothing to declare and nothing to
/// mistrust.
mod one_writer {
    use super::*;

    /// An older client still sending the old declaration changes nothing: the field is
    /// gone, and the decision never depended on the client anyway.
    #[tokio::test]
    async fn an_old_clients_declaration_is_simply_ignored() {
        let mut app = TestApp::new().await;
        app.register("author", "password123").await;
        let book_id = app.create_book("Ordinary Book").await;
        let chapter_id = app.create_chapter(&book_id, "One").await;

        // Not cut over: git is the truth, so the write lands whatever the old flag said.
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

    /// Structure is not a body. REST writes it for every book, cut over or not — sync
    /// does not carry titles or note colours.
    #[tokio::test]
    async fn structure_is_still_written_for_a_cut_over_book() {
        let mut app = TestApp::new().await;
        app.register("author", "password123").await;
        let book_id = app.create_book("Cutover Book").await;
        let chapter_id = app.create_chapter(&book_id, "One").await;
        app.cut_over(&book_id).await;

        let r = app
            .put(
                &format!("/api/books/{book_id}/chapters/{chapter_id}"),
                &json!({ "title": "One, renamed" }),
            )
            .await;
        assert_eq!(r.status, StatusCode::OK);

        let r = app
            .get(&format!("/api/books/{book_id}/chapters/{chapter_id}"))
            .await;
        assert_eq!(r.json["title"].as_str(), Some("One, renamed"));
    }

    /// The check two days of dropped writes bought, kept: a body is only sync's to
    /// carry if the canonical document is one this build can read. When it is not, the
    /// content is taken after all — dropping it would leave the edit nowhere.
    #[tokio::test]
    async fn an_unreadable_canonical_copy_means_git_takes_the_write() {
        let mut app = TestApp::new().await;
        app.register("author", "password123").await;
        let book_id = app.create_book("Cutover Book").await;
        let chapter_id = app.create_chapter(&book_id, "One").await;
        // Cut over with nothing in the canonical store for this chapter at all.
        app.cut_over(&book_id).await;

        let r = app
            .put(
                &format!("/api/books/{book_id}/chapters/{chapter_id}"),
                &json!({ "content": doc_json("nowhere else to go") }),
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
            git.contains("nowhere else to go"),
            "with no canonical copy to carry it, the write must still land somewhere: {git}"
        );
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
