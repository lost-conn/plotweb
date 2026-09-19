//! Notes revamp, card 6: a book's span rule — how the event ribbon draws a nested
//! event against its parent.
//!
//! Carried exactly like the calendar (`tests/calendar.rs`): absent for a book that
//! never chose one, set and read back by PUT/GET, `null` resets it, and it lands in
//! the canonical `book:` structure once cut over.

mod common;

use axum::http::StatusCode;
use common::TestApp;
use plotweb_common::SpanRule;
use serde_json::json;

fn canonical_structure(app: &TestApp, book_id: &str) -> plotweb_crdt::BookStructure {
    let bytes =
        plotweb_server::sync::canonical_snapshot(app.crdt_dir(), &format!("book:{book_id}"))
            .expect("store read")
            .expect("a canonical structure");
    plotweb_crdt::materialize_book_structure(&bytes).expect("materialize")
}

async fn book_span_rule(app: &mut TestApp, book: &str) -> Option<SpanRule> {
    let got = app.get(&format!("/api/books/{book}")).await;
    assert_eq!(got.status, StatusCode::OK, "{}", got.json);
    got.json
        .get("span_rule")
        .map(|v| serde_json::from_value(v.clone()).expect("a span rule"))
}

async fn span_rule_round_trip(cut_over: bool) {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let book = app.create_book("The Accord Cycle").await;
    if cut_over {
        app.cut_over(&book).await;
    }

    // A book that never touches the span rule has none — it reads as the default
    // (SpanRule::Fit), and its JSON is exactly what it was before span rules existed.
    assert_eq!(book_span_rule(&mut app, &book).await, None);

    let put = app
        .put(&format!("/api/books/{book}"), &json!({ "span_rule": "clamp" }))
        .await;
    assert_eq!(put.status, StatusCode::OK, "{}", put.json);
    assert_eq!(book_span_rule(&mut app, &book).await, Some(SpanRule::Clamp));

    if cut_over {
        let structure = canonical_structure(&app, &book);
        assert_eq!(structure.span_rule.as_deref(), Some("clamp"));
    }

    // An unrelated book edit leaves it alone (the field is a patch, like calendar).
    let put = app
        .put(&format!("/api/books/{book}"), &json!({ "title": "Renamed" }))
        .await;
    assert_eq!(put.status, StatusCode::OK);
    app.restart().await;
    app.login("author", "password123").await;
    if cut_over {
        app.cut_over(&book).await;
    }
    assert_eq!(book_span_rule(&mut app, &book).await, Some(SpanRule::Clamp));

    // Set to the third rule too, so this isn't just a two-state check.
    let put = app
        .put(&format!("/api/books/{book}"), &json!({ "span_rule": "free" }))
        .await;
    assert_eq!(put.status, StatusCode::OK);
    assert_eq!(book_span_rule(&mut app, &book).await, Some(SpanRule::Free));

    // `null` returns the book to the default (auto SpanRule::Fit).
    let put = app
        .put(&format!("/api/books/{book}"), &json!({ "span_rule": null }))
        .await;
    assert_eq!(put.status, StatusCode::OK);
    assert_eq!(book_span_rule(&mut app, &book).await, None);
    if cut_over {
        assert_eq!(canonical_structure(&app, &book).span_rule, None);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_span_rule_round_trips_and_resets() {
    span_rule_round_trip(false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_span_rule_round_trips_and_resets_on_a_cut_over_book() {
    span_rule_round_trip(true).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unrecognised_span_rule_is_rejected_by_the_request_type() {
    // SpanRule has no "other" variant, so JSON that isn't one of the three words fails
    // to deserialize before the handler ever runs — refused, not silently coerced.
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let book = app.create_book("Novel").await;

    let put = app
        .put(&format!("/api/books/{book}"), &json!({ "span_rule": "bogus" }))
        .await;
    assert_eq!(put.status, StatusCode::UNPROCESSABLE_ENTITY, "{}", put.json);
    assert_eq!(book_span_rule(&mut app, &book).await, None);
}
