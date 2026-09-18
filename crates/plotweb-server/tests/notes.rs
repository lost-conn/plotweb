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

// ── Notes revamp, card 1: spans, facets, `event_parent`, link index ──────────
//
// Everything below is model plumbing with no UI on it yet. What it has to prove is
// that a facet survives every hop it will take once there *is* a UI — the REST write,
// git, the canonical `book:` document, and the read back — and that adding the fields
// changed nothing for a note that carries none of them.

/// The canonical `book:` structure the timeline will eventually be drawn from.
fn canonical_structure(app: &TestApp, book_id: &str) -> plotweb_crdt::BookStructure {
    let bytes =
        plotweb_server::sync::canonical_snapshot(app.crdt_dir(), &format!("book:{book_id}"))
            .expect("store read")
            .expect("a canonical structure");
    plotweb_crdt::materialize_book_structure(&bytes).expect("materialize")
}

fn doc_body(text: &str) -> String {
    format!(
        r#"{{"type":"doc","content":[{{"type":"paragraph","content":[{{"type":"text","text":"{text}"}}]}}]}}"#
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_span_an_entity_mark_and_an_event_parent_round_trip_through_the_canonical_document() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let book = app.create_book("Novel").await;
    app.cut_over(&book).await;

    let siege = create_note(&mut app, &book, "The siege").await;
    let vess = create_note(&mut app, &book, "Vess").await;

    // A span in ticks — one number in base calendar units plus a precision level, so
    // the timeline can sort without knowing what a "year" is in this book.
    let span = json!({
        "start": { "tick": 126_144_000_i64, "precision": 0 },
        "end": { "tick": 157_680_000_i64, "precision": 2 },
        "approximate": true,
        "open_ended": false
    });
    let upd = app
        .put(
            &format!("/api/books/{book}/notes/{vess}"),
            &json!({
                "span": span,
                "is_entity": true,
                "event_parent": siege,
                "relative": { "relation": "after", "note_id": siege },
                "content": doc_body("$Vess held the wall. #siege @Karel"),
            }),
        )
        .await;
    assert_eq!(upd.status, StatusCode::OK, "{}", upd.json);

    // Read back over REST.
    let got = app.get(&format!("/api/books/{book}/notes/{vess}")).await;
    assert_eq!(got.json["span"]["start"]["tick"], 126_144_000_i64);
    assert_eq!(got.json["span"]["end"]["precision"], 2);
    assert_eq!(got.json["span"]["approximate"], true);
    assert_eq!(got.json["is_entity"], true);
    assert_eq!(got.json["event_parent"], siege);
    assert_eq!(got.json["relative"]["relation"], "after");

    // And in the canonical document, which is where the timeline will read them —
    // including the link index derived from the body, so drawing it never has to open
    // a single `note:{id}` document.
    let structure = canonical_structure(&app, &book);
    assert_eq!(
        structure.note_spans.get(&vess).map(|s| s.start.tick),
        Some(126_144_000)
    );
    assert!(structure.note_entities.contains(&vess));
    assert_eq!(structure.note_event_parents.get(&vess), Some(&siege));
    let links = structure.note_links.get(&vess).expect("link edges");
    let texts = |edges: &[plotweb_common::NoteLink]| -> Vec<String> {
        edges.iter().map(|e| e.text.clone()).collect()
    };
    assert_eq!(texts(&links.refs), vec!["Vess".to_string()]);
    assert_eq!(links.tags, vec!["siege".to_string()]);
    assert_eq!(texts(&links.mentions), vec!["Karel".to_string()]);

    // Survives a restart: the facets are stored, not held in memory.
    app.restart().await;
    app.login("author", "password123").await;
    let after = app.get(&format!("/api/books/{book}/notes/{vess}")).await;
    assert_eq!(after.json["span"]["start"]["tick"], 126_144_000_i64);
    assert_eq!(after.json["is_entity"], true);
    assert_eq!(after.json["event_parent"], siege);

    // And a span can be taken off again — a `null` clears, an absent field would not.
    let cleared = app
        .put(
            &format!("/api/books/{book}/notes/{vess}"),
            &json!({ "span": null }),
        )
        .await;
    assert_eq!(cleared.status, StatusCode::OK);
    let got = app.get(&format!("/api/books/{book}/notes/{vess}")).await;
    assert!(got.json.get("span").is_none(), "{}", got.json);
    assert_eq!(
        got.json["is_entity"], true,
        "undating a note must not un-entity it — the facets are independent"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_note_carrying_no_facets_is_exactly_what_it_was_before() {
    // The migration promise: every existing note becomes lore, and nothing about it
    // changes. An old stored note file has none of the new keys at all.
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let book = app.create_book("Novel").await;
    let note = create_note(&mut app, &book, "Characters").await;

    // Rewrite the note's file as a pre-revamp writer would have left it.
    let path = app
        .book_dir()
        .join(&book)
        .join("notes")
        .join(format!("{note}.json"));
    std::fs::write(
        &path,
        json!({
            "title": "Characters",
            "content": "<p>the old shape</p>",
            "color": "teal",
            "created_at": "2026-01-01 00:00:00"
        })
        .to_string(),
    )
    .expect("write the old-shaped note");

    let got = app.get(&format!("/api/books/{book}/notes/{note}")).await;
    assert_eq!(got.status, StatusCode::OK, "{}", got.json);
    assert_eq!(got.json["title"], "Characters");
    assert_eq!(got.json["color"], "teal");
    assert_eq!(got.json["content"], "<p>the old shape</p>");
    for absent in ["span", "relative", "event_parent", "is_entity"] {
        assert!(
            got.json.get(absent).is_none(),
            "a lore note must serialize exactly as it did before, but carried {absent}"
        );
    }

    // And it still lists, with the same shape.
    let list = app.get(&format!("/api/books/{book}/notes")).await;
    let listed = &list.json["notes"][0];
    assert_eq!(listed["title"], "Characters");
    assert!(listed.get("span").is_none());
    assert!(listed.get("is_entity").is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_event_parent_is_independent_of_the_tree_parent() {
    // Two hierarchies that must never be confused: `NoteTree` is where the author filed
    // a note, `event_parent` is what contains it in time. The parley and the siege can
    // coincide without one owning the other, and a note filed under "Characters" can
    // still nest inside an event.
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let book = app.create_book("Novel").await;
    app.cut_over(&book).await;

    let characters = create_note(&mut app, &book, "Characters").await;
    let siege = create_note(&mut app, &book, "The siege").await;
    let vess = create_note(&mut app, &book, "Vess").await;

    // Filed under Characters in the tree…
    let moved = app
        .put(
            &format!("/api/books/{book}/notes/move"),
            &json!({ "note_id": vess, "new_parent_id": characters, "index": 0 }),
        )
        .await;
    assert_eq!(moved.status, StatusCode::OK, "{}", moved.json);

    // …and contained by the siege in time.
    let upd = app
        .put(
            &format!("/api/books/{book}/notes/{vess}"),
            &json!({ "event_parent": siege }),
        )
        .await;
    assert_eq!(upd.status, StatusCode::OK, "{}", upd.json);

    let structure = canonical_structure(&app, &book);
    assert_eq!(
        structure.children.get(&characters),
        Some(&vec![vess.clone()]),
        "the tree parent is Characters"
    );
    assert_eq!(
        structure.note_event_parents.get(&vess),
        Some(&siege),
        "and the event parent is the siege — neither implies the other"
    );
    assert!(
        !structure.children.contains_key(&siege),
        "setting an event parent must not have moved the note in the tree"
    );

    // Refiling in the tree leaves containment in time alone.
    let moved = app
        .put(
            &format!("/api/books/{book}/notes/move"),
            &json!({ "note_id": vess, "new_parent_id": null, "index": 0 }),
        )
        .await;
    assert_eq!(moved.status, StatusCode::OK);
    let structure = canonical_structure(&app, &book);
    assert!(structure.root_order.contains(&vess), "the move happened");
    assert_eq!(
        structure.note_event_parents.get(&vess),
        Some(&siege),
        "and the event parent survived it untouched"
    );

    // And clearing containment in time leaves the tree alone.
    let upd = app
        .put(
            &format!("/api/books/{book}/notes/{vess}"),
            &json!({ "event_parent": null }),
        )
        .await;
    assert_eq!(upd.status, StatusCode::OK);
    let structure = canonical_structure(&app, &book);
    assert!(!structure.note_event_parents.contains_key(&vess));
    assert!(
        structure.root_order.contains(&vess),
        "the note is still exactly where the author filed it"
    );
}

// ── Notes revamp, card 2: what a sigil points at ─────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_mention_records_whether_it_found_a_chapter_or_a_note() {
    // `@` reaches both, so the edge has to say which — a reader that worked it out
    // from the id would have to search two lists and would answer differently once a
    // title moved between them. Decided on write, stored, and never derived again.
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let book = app.create_book("Novel").await;
    app.cut_over(&book).await;

    let chapter = app.create_chapter(&book, "Three Doors").await;
    let karel = create_note(&mut app, &book, "Karel").await;
    let siege = create_note(&mut app, &book, "The siege").await;

    // A token cannot hold a space, so the editor writes the title's spaces as `-` and
    // resolution folds both sides back together.
    let upd = app
        .put(
            &format!("/api/books/{book}/notes/{siege}"),
            &json!({
                "content": doc_body("@Three-Doors dramatises it. $Karel was there. @Nobody was not."),
            }),
        )
        .await;
    assert_eq!(upd.status, StatusCode::OK, "{}", upd.json);

    let expect = |links: &plotweb_common::NoteLinks| {
        assert_eq!(
            links.mentions,
            vec![
                plotweb_common::NoteLink {
                    text: "Three-Doors".into(),
                    target: plotweb_common::LinkTarget::Chapter,
                    id: Some(chapter.clone()),
                },
                // A name nothing answers to is still an edge: the author may be
                // pointing at something they have not written yet, and dropping it
                // would lose the intent instead of showing it as missing.
                plotweb_common::NoteLink::unresolved("Nobody"),
            ]
        );
        assert_eq!(
            links.refs,
            vec![plotweb_common::NoteLink {
                text: "Karel".into(),
                target: plotweb_common::LinkTarget::Note,
                id: Some(karel.clone()),
            }]
        );
    };

    expect(
        canonical_structure(&app, &book)
            .note_links
            .get(&siege)
            .expect("link edges"),
    );

    // Served on the note itself, so the editor's rail draws the graph from the notes
    // list rather than opening every `note:{id}` document.
    let got = app.get(&format!("/api/books/{book}/notes/{siege}")).await;
    assert_eq!(got.json["links"]["mentions"][0]["target"], "chapter");
    assert_eq!(got.json["links"]["mentions"][0]["id"], chapter);
    assert_eq!(
        got.json["links"]["mentions"][1], "Nobody",
        "an unresolved note edge stays the bare string it has always been"
    );
    assert_eq!(got.json["links"]["refs"][0]["id"], karel);

    // And it survives a restart, which is the only proof that the kind was stored
    // rather than recomputed on the way out.
    app.restart().await;
    app.login("author", "password123").await;
    expect(
        canonical_structure(&app, &book)
            .note_links
            .get(&siege)
            .expect("link edges after restart"),
    );
    let list = app.get(&format!("/api/books/{book}/notes")).await;
    let listed = list.json["notes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == siege.as_str())
        .expect("the siege");
    assert_eq!(listed["links"]["mentions"][0]["target"], "chapter");
}
