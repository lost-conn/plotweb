//! Notes revamp, card 4: a book's calendar, and the five states a note's time can be in.
//!
//! Both have to survive every hop the UI will send them through — the REST write, git,
//! the canonical `book:` document, a restart and the read back — for a book cut over
//! and for one that is not, because the two read from different stores.

mod common;

use axum::http::StatusCode;
use common::TestApp;
use plotweb_common::Calendar;
use serde_json::{json, Value};

/// The design's invented calendar: a year of four named seasons and 320 days, a day of
/// sixteen bells.
fn accord() -> Value {
    json!({
        "name": "The Accord",
        "units": [
            { "name": "Year", "per": 1, "of": 0, "format": "yr {n}", "first": 0 },
            { "name": "Season", "per": 4, "of": 0, "names": ["wet", "dry", "high", "low"],
              "format": ", {name}", "first": 1, "shown_below": { "count": 40, "unit": 0 } },
            { "name": "Day", "per": 320, "of": 0, "format": ", day {n}", "first": 1,
              "shown_below": { "count": 2, "unit": 0 } },
            { "name": "Bell", "per": 16, "of": 2, "format": ", bell {n}", "first": 1,
              "shown_below": { "count": 20, "unit": 2 } }
        ]
    })
}

fn canonical_structure(app: &TestApp, book_id: &str) -> plotweb_crdt::BookStructure {
    let bytes =
        plotweb_server::sync::canonical_snapshot(app.crdt_dir(), &format!("book:{book_id}"))
            .expect("store read")
            .expect("a canonical structure");
    plotweb_crdt::materialize_book_structure(&bytes).expect("materialize")
}

async fn book_calendar(app: &mut TestApp, book: &str) -> Option<Calendar> {
    let got = app.get(&format!("/api/books/{book}")).await;
    assert_eq!(got.status, StatusCode::OK, "{}", got.json);
    got.json
        .get("calendar")
        .map(|c| serde_json::from_value(c.clone()).expect("a calendar"))
}

async fn calendar_round_trip(cut_over: bool) {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let book = app.create_book("The Accord Cycle").await;
    if cut_over {
        app.cut_over(&book).await;
    }

    // A book that never touches the calendar has none — it reads as the default, and
    // its JSON is exactly what it was before calendars existed.
    assert_eq!(book_calendar(&mut app, &book).await, None);

    let put = app
        .put(&format!("/api/books/{book}"), &json!({ "calendar": accord() }))
        .await;
    assert_eq!(put.status, StatusCode::OK, "{}", put.json);
    let want: Calendar = serde_json::from_value(accord()).unwrap();
    assert_eq!(book_calendar(&mut app, &book).await, Some(want.clone()));

    if cut_over {
        let structure = canonical_structure(&app, &book);
        let stored: Calendar =
            serde_json::from_str(structure.calendar_json.as_deref().expect("in the document"))
                .unwrap();
        assert_eq!(stored, want);
    }

    // An unrelated book edit leaves it alone (the calendar field is a patch).
    let put = app
        .put(&format!("/api/books/{book}"), &json!({ "title": "Renamed" }))
        .await;
    assert_eq!(put.status, StatusCode::OK);
    app.restart().await;
    app.login("author", "password123").await;
    if cut_over {
        app.cut_over(&book).await;
    }
    assert_eq!(book_calendar(&mut app, &book).await, Some(want.clone()));

    // A calendar the arithmetic cannot use is refused, and the stored one is untouched.
    let mut broken = accord();
    broken["units"][2]["of"] = json!(3);
    let put = app
        .put(&format!("/api/books/{book}"), &json!({ "calendar": broken }))
        .await;
    assert_eq!(put.status, StatusCode::BAD_REQUEST, "{}", put.json);
    assert_eq!(book_calendar(&mut app, &book).await, Some(want));

    // `null` returns the book to the default calendar.
    let put = app
        .put(&format!("/api/books/{book}"), &json!({ "calendar": null }))
        .await;
    assert_eq!(put.status, StatusCode::OK);
    assert_eq!(book_calendar(&mut app, &book).await, None);
    if cut_over {
        assert_eq!(canonical_structure(&app, &book).calendar_json, None);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_calendar_round_trips_and_resets() {
    calendar_round_trip(false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_calendar_round_trips_and_resets_on_a_cut_over_book() {
    calendar_round_trip(true).await;
}

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

fn point(tick: i64, precision: u8) -> Value {
    json!({ "tick": tick, "precision": precision })
}

/// Read one note's `(span, relative)` out of the list — the read the tree and the
/// client's projection seed from.
async fn time_of(app: &mut TestApp, book: &str, id: &str) -> (Value, Value) {
    let list = app.get(&format!("/api/books/{book}/notes")).await;
    assert_eq!(list.status, StatusCode::OK);
    let note = list.json["notes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == id)
        .expect("listed")
        .clone();
    (
        note.get("span").cloned().unwrap_or(Value::Null),
        note.get("relative").cloned().unwrap_or(Value::Null),
    )
}

async fn five_states(cut_over: bool) {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let book = app.create_book("Novel").await;
    if cut_over {
        app.cut_over(&book).await;
    }
    let cal: Calendar = serde_json::from_value(accord()).unwrap();
    app.put(&format!("/api/books/{book}"), &json!({ "calendar": accord() }))
        .await;

    let exact = create_note(&mut app, &book, "The breach").await;
    let approx = create_note(&mut app, &book, "The first frost").await;
    let open = create_note(&mut app, &book, "The exile").await;
    let relative = create_note(&mut app, &book, "The parley").await;
    let undated = create_note(&mut app, &book, "Amberwork").await;

    // Exactly what the UI sends: a whole span, typed through the book's calendar.
    let breach = cal.parse_point("yr 1206, dry, day 12, bell 9").unwrap();
    let breach_end = cal.parse_point("yr 1206, dry, day 13").unwrap();
    let frost = cal.parse_point("yr 1206, low").unwrap();
    let exile = cal.parse_point("yr 1198").unwrap();

    let states = [
        (&exact, json!({
            "span": { "start": point(breach.tick, 3), "end": point(breach_end.tick, 2),
                      "approximate": false, "open_ended": false },
            "relative": null
        })),
        (&approx, json!({
            "span": { "start": point(frost.tick, 1), "approximate": true, "open_ended": false },
            "relative": null
        })),
        (&open, json!({
            "span": { "start": point(exile.tick, 0), "approximate": false, "open_ended": true },
            "relative": null
        })),
        (&relative, json!({
            "span": null,
            "relative": { "relation": "after", "note_id": exact }
        })),
    ];
    for (id, body) in &states {
        let r = app.put(&format!("/api/books/{book}/notes/{id}"), body).await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    }

    app.restart().await;
    app.login("author", "password123").await;
    if cut_over {
        app.cut_over(&book).await;
    }

    let (span, rel) = time_of(&mut app, &book, &exact).await;
    assert_eq!(span["start"], point(breach.tick, 3));
    assert_eq!(span["end"], point(breach_end.tick, 2));
    assert_eq!(rel, Value::Null);

    let (span, _) = time_of(&mut app, &book, &approx).await;
    assert_eq!(span["approximate"], true);
    assert_eq!(span["start"]["precision"], 1);

    let (span, _) = time_of(&mut app, &book, &open).await;
    assert_eq!(span["open_ended"], true);
    assert!(span.get("end").is_none(), "{span}");

    let (span, rel) = time_of(&mut app, &book, &relative).await;
    assert_eq!(span, Value::Null);
    assert_eq!(rel["relation"], "after");
    assert_eq!(rel["note_id"], json!(exact));

    assert_eq!(time_of(&mut app, &book, &undated).await, (Value::Null, Value::Null));

    // Undating a dated note takes it back to the fifth state, in every store.
    let r = app
        .put(
            &format!("/api/books/{book}/notes/{approx}"),
            &json!({ "span": null, "relative": null }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(time_of(&mut app, &book, &approx).await, (Value::Null, Value::Null));
    if cut_over {
        let structure = canonical_structure(&app, &book);
        assert!(!structure.note_spans.contains_key(&approx));
        assert!(structure.note_spans.contains_key(&exact));
        assert!(structure.note_relatives.contains_key(&relative));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn all_five_time_states_round_trip() {
    five_states(false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn all_five_time_states_round_trip_on_a_cut_over_book() {
    five_states(true).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_child_event_dated_outside_its_parent_is_stored_exactly_as_typed() {
    // The span rule for nested events is still open (card 6). Until it is settled the
    // server must neither clamp, nor stretch, nor refuse: it stores what was typed.
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let book = app.create_book("Novel").await;
    app.cut_over(&book).await;
    let siege = create_note(&mut app, &book, "The siege").await;
    let breach = create_note(&mut app, &book, "The breach").await;

    let year = plotweb_common::TICKS_PER_BASE_UNIT;
    app.put(
        &format!("/api/books/{book}/notes/{siege}"),
        &json!({ "span": { "start": point(1206 * year, 0), "end": point(1207 * year, 0),
                           "approximate": false, "open_ended": false } }),
    )
    .await;
    let r = app
        .put(
            &format!("/api/books/{book}/notes/{breach}"),
            &json!({ "event_parent": siege,
                     "span": { "start": point(1300 * year, 0),
                               "approximate": false, "open_ended": false } }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);

    let (span, _) = time_of(&mut app, &book, &breach).await;
    assert_eq!(span["start"], point(1300 * year, 0));
    let (span, _) = time_of(&mut app, &book, &siege).await;
    assert_eq!(span["end"], point(1207 * year, 0), "the parent did not grow to fit");
}
