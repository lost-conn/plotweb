//! Structure writes reaching the canonical `book:` document (phase E).
//!
//! Bodies were the first half of cutover; the structure — chapter order and titles, the
//! notes tree, book metadata — is the other. What these check is that every route that
//! changes it records the change *into* the canonical document rather than beside it,
//! and that doing so leaves a device holding that document still able to converge.

mod common;

use automerge::{sync::State as SyncState, sync::SyncDoc, AutoCommit};
use axum::http::StatusCode;
use common::TestApp;
use plotweb_crdt::BookStructure;
use serde_json::json;

/// A device speaking the real sync protocol for a structure document.
struct Device {
    doc: AutoCommit,
}

impl Device {
    /// One poll cycle: fresh sync state (the server keeps none), round-tripped until we
    /// have nothing further to send.
    async fn sync(&mut self, app: &mut TestApp, uri: &str) {
        use automerge::sync::Message as SyncMessage;
        let mut state = SyncState::new();
        let mut rounds = 0;
        loop {
            let outgoing = self.doc.sync().generate_sync_message(&mut state);
            let Some(msg) = outgoing else { break };
            let (status, reply) = app.post_bytes(uri, &msg.encode()).await;
            assert_eq!(status, StatusCode::OK, "sync round failed");
            rounds += 1;
            assert!(rounds < 20, "sync did not converge in {rounds} rounds");
            if reply.is_empty() {
                break;
            }
            let reply = SyncMessage::decode(&reply).expect("decodable reply");
            self.doc
                .sync()
                .receive_sync_message(&mut state, reply)
                .expect("integrate reply");
        }
    }
}

/// The canonical structure as stored, or `None` when the server holds no copy.
fn canonical(app: &TestApp, book_id: &str) -> Option<BookStructure> {
    let bytes =
        plotweb_server::sync::canonical_snapshot(app.crdt_dir(), &format!("book:{book_id}"))
            .expect("store read")?;
    Some(plotweb_crdt::materialize_book_structure(&bytes).expect("materialize"))
}

/// What git says the structure is — what the canonical copy is supposed to agree with.
async fn from_git(app: &TestApp, book_id: &str) -> BookStructure {
    plotweb_server::structure::read_structure_input(&app.state().books, book_id)
        .await
        .expect("structure in git")
        .structure()
}

async fn assert_agrees(app: &TestApp, book_id: &str) {
    let git = from_git(app, book_id).await;
    let stored = canonical(app, book_id).expect("a canonical structure");
    assert_eq!(stored, git, "the canonical structure must match git's");
}

#[tokio::test]
async fn every_kind_of_structure_change_reaches_the_canonical_document() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let book_id = app.create_book("Structure Book").await;
    app.cut_over(&book_id).await;

    let c1 = app.create_chapter(&book_id, "One").await;
    let c2 = app.create_chapter(&book_id, "Two").await;
    assert_agrees(&app, &book_id).await;
    assert_eq!(
        canonical(&app, &book_id).unwrap().chapters,
        vec![(c1.clone(), "One".into()), (c2.clone(), "Two".into())]
    );

    // Rename.
    let r = app
        .put(
            &format!("/api/books/{book_id}/chapters/{c1}"),
            &json!({ "title": "One, revised" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_agrees(&app, &book_id).await;

    // Reorder.
    let r = app
        .put(
            &format!("/api/books/{book_id}/chapters/reorder"),
            &json!({ "chapter_ids": [c2.clone(), c1.clone()] }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(
        canonical(&app, &book_id).unwrap().chapters[0].0,
        c2,
        "the reorder must be visible in the canonical copy, not just git"
    );
    assert_agrees(&app, &book_id).await;

    // Delete — removal from the index is the deletion (§D7).
    let r = app.delete(&format!("/api/books/{book_id}/chapters/{c1}")).await;
    assert_eq!(r.status, StatusCode::OK);
    let stored = canonical(&app, &book_id).unwrap();
    assert_eq!(stored.chapters, vec![(c2.clone(), "Two".into())]);
    assert!(!stored.chapters.iter().any(|(id, _)| id == &c1));
    assert_agrees(&app, &book_id).await;

    // Book metadata.
    let r = app
        .put(
            &format!("/api/books/{book_id}"),
            &json!({ "title": "Structure Book, Revised", "description": "now with a description" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(
        canonical(&app, &book_id).unwrap().title,
        "Structure Book, Revised"
    );
    assert_agrees(&app, &book_id).await;
}

#[tokio::test]
async fn the_notes_tree_reaches_it_too() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let book_id = app.create_book("Structure Book").await;
    app.cut_over(&book_id).await;

    let r = app
        .post(
            &format!("/api/books/{book_id}/notes"),
            &json!({ "title": "Characters", "parent_id": null, "color": "teal" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED);
    let n1 = r.id();
    let r = app
        .post(
            &format!("/api/books/{book_id}/notes"),
            &json!({ "title": "Places", "parent_id": null, "color": null }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED);
    let n2 = r.id();
    assert_agrees(&app, &book_id).await;

    // Retitle + recolour.
    let r = app
        .put(
            &format!("/api/books/{book_id}/notes/{n2}"),
            &json!({ "title": "Settings", "color": "red" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    let stored = canonical(&app, &book_id).unwrap();
    assert_eq!(stored.note_titles.get(&n2).map(String::as_str), Some("Settings"));
    assert_eq!(stored.note_colors.get(&n2).map(String::as_str), Some("red"));

    // Move into a subtree.
    let r = app
        .put(
            &format!("/api/books/{book_id}/notes/move"),
            &json!({ "note_id": n2, "new_parent_id": n1, "index": 0 }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    let stored = canonical(&app, &book_id).unwrap();
    assert_eq!(stored.root_order, vec![n1.clone()]);
    assert_eq!(stored.children.get(&n1), Some(&vec![n2.clone()]));
    assert_agrees(&app, &book_id).await;

    // Collapse, via the whole-tree write the sidebar uses.
    let r = app
        .put(
            &format!("/api/books/{book_id}/notes/tree"),
            &json!({ "tree": { "root_order": [n1.clone()], "children": { n1.clone(): [n2.clone()] }, "collapsed": [n1.clone()] } }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(canonical(&app, &book_id).unwrap().collapsed.contains(&n1));
    assert_agrees(&app, &book_id).await;

    // And deleting a note removes it from the tree.
    let r = app.delete(&format!("/api/books/{book_id}/notes/{n2}")).await;
    assert_eq!(r.status, StatusCode::OK);
    let stored = canonical(&app, &book_id).unwrap();
    assert!(!stored.note_titles.contains_key(&n2));
    assert!(stored.children.get(&n1).map(Vec::is_empty).unwrap_or(true));
    assert_agrees(&app, &book_id).await;
}

#[tokio::test]
async fn a_book_that_is_not_cut_over_gains_no_canonical_structure() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let book_id = app.create_book("Ordinary Book").await;
    app.create_chapter(&book_id, "One").await;

    assert!(
        canonical(&app, &book_id).is_none(),
        "nothing may write the canonical store for a book still served from git — \
         the backfill maintains those, and a write here would take them from it"
    );
}

#[tokio::test]
async fn a_structure_write_leaves_a_synced_device_able_to_converge() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let book_id = app.create_book("Structure Book").await;
    app.cut_over(&book_id).await;
    let c1 = app.create_chapter(&book_id, "One").await;
    let uri = format!("/api/books/{book_id}/sync/book:{book_id}");

    // A device pulls the canonical structure, so from here it holds that history.
    let mut device = Device { doc: AutoCommit::new() };
    device.sync(&mut app, &uri).await;

    // The author adds a chapter from another browser, over REST.
    let c2 = app.create_chapter(&book_id, "Two").await;

    // The decisive check: the device syncs again and *converges* on the new chapter. If
    // the write had replaced the document rather than edited it, the two would share no
    // history and this would either fail to converge or end up holding both copies.
    device.sync(&mut app, &uri).await;
    let seen = plotweb_crdt::materialize_book_structure(&device.doc.save()).expect("materialize");
    assert_eq!(
        seen.chapters,
        vec![(c1, "One".into()), (c2, "Two".into())],
        "the device must see the added chapter once, in order"
    );
    assert_eq!(seen, from_git(&app, &book_id).await);
}

#[tokio::test]
async fn an_autosave_of_a_body_does_not_rewrite_the_structure() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let book_id = app.create_book("Structure Book").await;
    app.cut_over(&book_id).await;
    let c1 = app.create_chapter(&book_id, "One").await;

    let before = plotweb_server::sync::canonical_snapshot(
        app.crdt_dir(),
        &format!("book:{book_id}"),
    )
    .expect("read")
    .expect("stored");

    let r = app
        .put(
            &format!("/api/books/{book_id}/chapters/{c1}"),
            &json!({ "content": r#"{"type":"doc","content":[{"type":"paragraph"}]}"# }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);

    let after = plotweb_server::sync::canonical_snapshot(
        app.crdt_dir(),
        &format!("book:{book_id}"),
    )
    .expect("read")
    .expect("stored");
    assert_eq!(
        before, after,
        "a content-only save must not touch the structure document — every needless \
         change is a sync round for every device and a commit for nobody"
    );
}

// ── Reads ────────────────────────────────────────────────────────────────────
//
// The other half of cutover: once the canonical structure is the truth, the chapter
// list, the notes tree and the book's own metadata have to come from it. What makes
// this observable is a canonical copy git has not caught up with — exactly the window
// between a device syncing a change and the mirror committing it.

/// Move the canonical structure ahead of git, as a device's sync would.
async fn canonical_ahead(
    app: &TestApp,
    book_id: &str,
    edit: impl FnOnce(&mut plotweb_crdt::BookStructureInput),
) {
    let mut input = plotweb_server::structure::read_structure_input(&app.state().books, book_id)
        .await
        .expect("structure in git");
    edit(&mut input);
    plotweb_server::sync::apply_structure(app.crdt_dir(), book_id, &input).expect("apply");
}

#[tokio::test]
async fn the_chapter_list_comes_from_the_canonical_structure() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let book_id = app.create_book("Structure Book").await;
    app.cut_over(&book_id).await;
    let c1 = app.create_chapter(&book_id, "One").await;
    let c2 = app.create_chapter(&book_id, "Two").await;

    let gone = c1.clone();
    canonical_ahead(&app, &book_id, move |input| {
        input.chapters.retain(|(id, _)| id != &gone);
        input.chapters.push(("c-new".into(), "Three".into()));
        input.chapters.reverse();
        input.chapters[1].1 = "Two, revised".into();
    })
    .await;

    let r = app.get(&format!("/api/books/{book_id}/chapters")).await;
    assert_eq!(r.status, StatusCode::OK);
    let listed: Vec<(String, String, i64)> = r.json
        .as_array()
        .expect("a list")
        .iter()
        .map(|c| {
            (
                c["id"].as_str().unwrap().to_string(),
                c["title"].as_str().unwrap().to_string(),
                c["sort_order"].as_i64().unwrap(),
            )
        })
        .collect();

    assert_eq!(
        listed,
        vec![
            ("c-new".to_string(), "Three".to_string(), 0),
            (c2.clone(), "Two, revised".to_string(), 1),
        ],
        "order, titles, an addition git has not seen and a removal it still has — all \
         from the canonical copy"
    );
    assert!(
        !listed.iter().any(|(id, _, _)| id == &c1),
        "a chapter removed canonically must not come back from git"
    );
}

#[tokio::test]
async fn a_chapter_git_still_has_keeps_its_content_and_word_count() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let book_id = app.create_book("Structure Book").await;
    app.cut_over(&book_id).await;
    let c1 = app.create_chapter(&book_id, "One").await;
    let r = app
        .put(
            &format!("/api/books/{book_id}/chapters/{c1}"),
            &json!({ "content": r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"three words here"}]}]}"# }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);

    canonical_ahead(&app, &book_id, |input| {
        input.chapters[0].1 = "One, renamed on a device".into();
    })
    .await;

    let r = app.get(&format!("/api/books/{book_id}/chapters")).await;
    let ch = &r.json.as_array().unwrap()[0];
    assert_eq!(ch["title"], "One, renamed on a device");
    assert_eq!(
        ch["word_count"], 3,
        "the CRDT structure holds no word counts — those stay git's, kept current by \
         the body mirror"
    );
    assert!(ch["content"].as_str().unwrap().contains("three words here"));
}

#[tokio::test]
async fn the_notes_tree_comes_from_the_canonical_structure() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let book_id = app.create_book("Structure Book").await;
    app.cut_over(&book_id).await;
    let r = app
        .post(
            &format!("/api/books/{book_id}/notes"),
            &json!({ "title": "Characters", "parent_id": null, "color": null }),
        )
        .await;
    let n1 = r.id();

    let parent = n1.clone();
    canonical_ahead(&app, &book_id, move |input| {
        input.notes[0].1 = "Cast".into();
        input.notes[0].2 = Some("teal".into());
        input.notes.push(("n-new".into(), "Alice".into(), None));
        input.children.insert(parent.clone(), vec!["n-new".into()]);
        input.collapsed.push(parent);
    })
    .await;

    let r = app.get(&format!("/api/books/{book_id}/notes")).await;
    assert_eq!(r.status, StatusCode::OK);
    let notes = r.json["notes"].as_array().unwrap();
    let cast = notes.iter().find(|n| n["id"] == n1.as_str()).expect("the note");
    assert_eq!(cast["title"], "Cast");
    assert_eq!(cast["color"], "teal");
    assert!(
        notes.iter().any(|n| n["id"] == "n-new"),
        "a note added on a device is listed before the mirror commits it"
    );
    assert_eq!(r.json["tree"]["children"][&n1][0], "n-new");
    assert_eq!(r.json["tree"]["collapsed"][0], n1.as_str());
}

#[tokio::test]
async fn the_books_own_metadata_comes_from_it_too() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let book_id = app.create_book("Structure Book").await;
    app.cut_over(&book_id).await;
    app.create_chapter(&book_id, "One").await;

    canonical_ahead(&app, &book_id, |input| {
        input.title = "Renamed on a device".into();
        input.description = "and described there".into();
        input.chapters.push(("c-new".into(), "Two".into()));
    })
    .await;

    let r = app.get(&format!("/api/books/{book_id}")).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.json["title"], "Renamed on a device");
    assert_eq!(r.json["description"], "and described there");
    assert_eq!(r.json["chapter_count"], 2);
}

#[tokio::test]
async fn a_book_with_no_canonical_structure_still_reads_from_git() {
    // Absence is not evidence: a book cut over before its structure was ever synced or
    // backfilled has no canonical copy, and refusing to list its chapters would look
    // like the book had been emptied.
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let book_id = app.create_book("Structure Book").await;
    let c1 = app.create_chapter(&book_id, "One").await;
    app.cut_over(&book_id).await;

    let r = app.get(&format!("/api/books/{book_id}/chapters")).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.json.as_array().unwrap()[0]["id"], c1.as_str());
    let r = app.get(&format!("/api/books/{book_id}")).await;
    assert_eq!(r.json["title"], "Structure Book");
}

#[tokio::test]
async fn a_book_that_is_not_cut_over_reads_git_even_when_a_canonical_copy_disagrees() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let book_id = app.create_book("Ordinary Book").await;
    let c1 = app.create_chapter(&book_id, "One").await;
    canonical_ahead(&app, &book_id, |input| {
        input.chapters[0].1 = "Not what git says".into();
    })
    .await;

    let r = app.get(&format!("/api/books/{book_id}/chapters")).await;
    let ch = &r.json.as_array().unwrap()[0];
    assert_eq!(ch["id"], c1.as_str());
    assert_eq!(ch["title"], "One", "nothing changes for a book still on git");
}
