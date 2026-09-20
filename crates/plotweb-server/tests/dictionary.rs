//! The bundled dictionary assets and the per-user custom dictionary.

mod common;

use axum::http::StatusCode;
use common::TestApp;
use serde_json::json;

// ── Bundled assets ───────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bundled_dictionary_is_public_and_cacheable() {
    let mut app = TestApp::new().await;

    // No session: these must be reachable before sign-in, or the spellchecker cannot
    // warm its cache on a cold load.
    for path in ["/api/dictionaries/en_US.aff", "/api/dictionaries/en_US.dic"] {
        let (status, headers) = app.get_bytes_with_headers(path).await;
        assert_eq!(status, StatusCode::OK, "{path}");
        assert_eq!(
            headers.get("content-type").unwrap(),
            "text/plain; charset=utf-8",
            "{path}"
        );
        let cc = headers.get("cache-control").unwrap().to_str().unwrap();
        assert!(cc.contains("max-age=31536000"), "{path}: {cc}");
        assert!(cc.contains("immutable"), "{path}: {cc}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bundled_dictionary_bytes_are_loadable_hunspell() {
    let mut app = TestApp::new().await;

    let (status, aff) = app.get_bytes("/api/dictionaries/en_US.aff").await;
    assert_eq!(status, StatusCode::OK);
    let aff = String::from_utf8(aff).expect("aff is UTF-8");
    assert_eq!(aff.lines().next(), Some("SET UTF-8"));

    let (status, dic) = app.get_bytes("/api/dictionaries/en_US.dic").await;
    assert_eq!(status, StatusCode::OK);
    let dic = String::from_utf8(dic).expect("dic is UTF-8");
    let count: usize = dic
        .lines()
        .next()
        .expect("dic has a first line")
        .trim()
        .parse()
        .expect("dic starts with an entry count");
    assert!(
        count > 50_000,
        "en_US should carry a full word list, got {count}"
    );
}

// ── Per-user custom dictionary ───────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dictionary_requires_a_session() {
    let mut app = TestApp::new().await;

    let r = app.get("/api/me/dictionary").await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);

    let r = app
        .put("/api/me/dictionary", &json!({ "words": ["Elowen"] }))
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn empty_dictionary_reads_as_an_empty_list() {
    let mut app = TestApp::new().await;
    app.register("alice", "hunter2hunter2").await;

    // Never having added a word is the normal state, not a 404.
    let r = app.get("/api/me/dictionary").await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    assert_eq!(r.json["words"], json!([]));
    assert_eq!(r.json["updated_at"], "");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn put_then_get_round_trips() {
    let mut app = TestApp::new().await;
    app.register("alice", "hunter2hunter2").await;

    let r = app
        .put(
            "/api/me/dictionary",
            &json!({ "words": ["Elowen", "Aethelgard", "skyfarer"] }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    assert_eq!(r.json["words"], json!(["Aethelgard", "Elowen", "skyfarer"]));
    assert!(!r.json["updated_at"].as_str().unwrap().is_empty());

    let r = app.get("/api/me/dictionary").await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.json["words"], json!(["Aethelgard", "Elowen", "skyfarer"]));

    // A second PUT replaces rather than appends — the client owns the whole list.
    let r = app
        .put("/api/me/dictionary", &json!({ "words": ["skyfarer"] }))
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    let r = app.get("/api/me/dictionary").await;
    assert_eq!(r.json["words"], json!(["skyfarer"]));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn put_trims_dedupes_drops_empties_and_sorts() {
    let mut app = TestApp::new().await;
    app.register("alice", "hunter2hunter2").await;

    let r = app
        .put(
            "/api/me/dictionary",
            &json!({ "words": ["  zephyr ", "Elowen", "", "elowen", "Elowen", "   "] }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    // Case-sensitive de-duplication: `Elowen` and `elowen` are distinct entries,
    // matching how the speller treats a capitalised proper noun.
    assert_eq!(r.json["words"], json!(["Elowen", "elowen", "zephyr"]));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dictionary_survives_a_restart() {
    let mut app = TestApp::new().await;
    app.register("alice", "hunter2hunter2").await;
    app.put("/api/me/dictionary", &json!({ "words": ["Elowen"] }))
        .await;

    app.restart().await;

    let r = app.get("/api/me/dictionary").await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    assert_eq!(r.json["words"], json!(["Elowen"]));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn one_account_cannot_see_anothers_words() {
    let mut app = TestApp::new().await;

    app.register("alice", "hunter2hunter2").await;
    app.put("/api/me/dictionary", &json!({ "words": ["Elowen"] }))
        .await;
    let lo = app.post("/api/auth/logout", &json!({})).await;
    assert_eq!(lo.status, StatusCode::OK);
    app.logout_local();

    app.register("bob", "hunter2hunter2").await;
    let r = app.get("/api/me/dictionary").await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    assert_eq!(r.json["words"], json!([]), "bob must not see alice's words");

    // And bob's own write leaves alice's alone.
    app.put("/api/me/dictionary", &json!({ "words": ["Corvane"] }))
        .await;
    let lo = app.post("/api/auth/logout", &json!({})).await;
    assert_eq!(lo.status, StatusCode::OK);
    app.logout_local();

    let li = app.login("alice", "hunter2hunter2").await;
    assert_eq!(li.status, StatusCode::OK);
    let r = app.get("/api/me/dictionary").await;
    assert_eq!(r.json["words"], json!(["Elowen"]));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn too_many_words_is_refused_not_truncated() {
    let mut app = TestApp::new().await;
    app.register("alice", "hunter2hunter2").await;

    let words: Vec<String> = (0..10_001).map(|i| format!("word{i}")).collect();
    let r = app
        .put("/api/me/dictionary", &json!({ "words": words }))
        .await;
    assert_eq!(r.status, StatusCode::PAYLOAD_TOO_LARGE, "{}", r.json);

    // Nothing was stored.
    let r = app.get("/api/me/dictionary").await;
    assert_eq!(r.json["words"], json!([]));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_oversized_body_is_refused() {
    let mut app = TestApp::new().await;
    app.register("alice", "hunter2hunter2").await;

    // Well over the 1 MiB route limit, so the body layer rejects it before the
    // handler ever sees a word.
    let words: Vec<String> = (0..40_000)
        .map(|i| format!("averylongishword{i}"))
        .collect();
    let r = app
        .put("/api/me/dictionary", &json!({ "words": words }))
        .await;
    assert_eq!(r.status, StatusCode::PAYLOAD_TOO_LARGE, "{}", r.json);
}
