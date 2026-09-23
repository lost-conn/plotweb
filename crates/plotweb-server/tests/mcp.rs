//! `/api/mcp`: the author's AI agent over the Model Context Protocol.
//!
//! Every call here is a plain HTTP POST of a JSON-RPC message with the headers the
//! streamable-HTTP transport requires, driven through the real router with a real
//! personal access token — exactly what an MCP client sends.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::TestApp;
use plotweb_crdt::BodyKind;
use serde_json::{json, Value};

/// The complete tool set. If this changes, the change is a product decision: in
/// particular, **no tool may write chapter content or titles, reorder chapters,
/// delete anything, or touch tokens.** Adding one must fail here first.
const EXPECTED_TOOLS: &[&str] = &[
    "add_review_comment",
    "book_outline",
    "create_note",
    "list_books",
    "list_feedback",
    "list_notes",
    "manuscript_stats",
    "read_chapter",
    "read_note",
    "reply_to_feedback",
    "search_manuscript",
    "update_note",
];

const READ_ONLY_TOOLS: &[&str] = &[
    "book_outline",
    "list_books",
    "list_feedback",
    "list_notes",
    "manuscript_stats",
    "read_chapter",
    "read_note",
    "search_manuscript",
];

/// Mint a token over the session. `book_ids` null = all books.
async fn mint(app: &mut TestApp, label: &str, book_ids: Value) -> String {
    let r = app
        .post("/api/tokens", &json!({ "label": label, "book_ids": book_ids }))
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "create token: {}", r.json);
    r.json["token"].as_str().unwrap().to_string()
}

/// POST one JSON-RPC message to `/api/mcp`. Returns the HTTP status and, when there
/// is one, the JSON-RPC response — read from a JSON body or from an SSE stream,
/// whichever the server chose.
async fn rpc_raw(app: &mut TestApp, auth: Option<&str>, body: Value) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/api/mcp")
        .header("host", "plotweb.example")
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream");
    if let Some(a) = auth {
        builder = builder.header("authorization", a);
    }
    let req = builder
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let (status, headers, bytes) = app.send_raw(req).await;
    let ctype = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let text = String::from_utf8_lossy(&bytes).to_string();
    let json = if ctype.starts_with("text/event-stream") {
        // The response is the `data:` of the event carrying our id.
        text.lines()
            .filter_map(|l| l.strip_prefix("data:"))
            .filter_map(|d| serde_json::from_str::<Value>(d.trim()).ok())
            .find(|v| v.get("id") == body.get("id"))
            .unwrap_or(Value::Null)
    } else if text.trim().is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&text).unwrap_or(Value::String(text))
    };
    (status, json)
}

/// A JSON-RPC request with the token; asserts HTTP 200 and returns `result`, or
/// panics with the JSON-RPC error.
async fn rpc(app: &mut TestApp, token: &str, method: &str, params: Value) -> Value {
    let (status, resp) = rpc_raw(
        app,
        Some(&format!("Bearer {token}")),
        json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{method}: {resp}");
    resp.get("result")
        .cloned()
        .unwrap_or_else(|| panic!("{method} returned no result: {resp}"))
}

/// The raw `tools/call` result.
async fn call(app: &mut TestApp, token: &str, tool: &str, args: Value) -> Value {
    rpc(app, token, "tools/call", json!({ "name": tool, "arguments": args })).await
}

fn is_error(result: &Value) -> bool {
    result["isError"] == json!(true)
}

/// The text of each content block.
fn texts(result: &Value) -> Vec<String> {
    result["content"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|c| c["text"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// A successful call's first content block, parsed as JSON.
async fn call_ok(app: &mut TestApp, token: &str, tool: &str, args: Value) -> Value {
    let r = call(app, token, tool, args).await;
    assert!(!is_error(&r), "{tool} failed: {r}");
    serde_json::from_str(&texts(&r)[0]).unwrap_or_else(|_| panic!("{tool}: not JSON: {r}"))
}

/// A call that must fail as a tool error; returns its message.
async fn call_err(app: &mut TestApp, token: &str, tool: &str, args: Value) -> String {
    let r = call(app, token, tool, args).await;
    assert!(is_error(&r), "{tool} should have failed: {r}");
    texts(&r).join("\n")
}

fn doc_json(paragraphs: &[&str]) -> String {
    let content: Vec<Value> = paragraphs
        .iter()
        .map(|p| json!({ "type": "paragraph", "content": [{ "type": "text", "text": p }] }))
        .collect();
    json!({ "type": "doc", "content": content }).to_string()
}

async fn set_chapter(app: &mut TestApp, book: &str, chapter: &str, content: &str) {
    let r = app
        .put(
            &format!("/api/books/{book}/chapters/{chapter}"),
            &json!({ "content": content }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "save chapter: {}", r.json);
}

/// A book with one chapter holding known prose. Returns (book, chapter).
async fn book_with_prose(app: &mut TestApp, title: &str) -> (String, String) {
    let book = app.create_book(title).await;
    let ch = app.create_chapter(&book, "The Lighthouse").await;
    set_chapter(
        app,
        &book,
        &ch,
        &doc_json(&[
            "The lantern guttered against the fog, and Mira counted the bells.",
            "Nobody came up the stairs that night.",
        ]),
    )
    .await;
    (book, ch)
}

async fn beta_link(app: &mut TestApp, book: &str, reader: &str) -> String {
    let r = app
        .post(
            &format!("/api/books/{book}/beta-links"),
            &json!({ "reader_name": reader }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
    r.json["token"].as_str().unwrap().to_string()
}

async fn reader_feedback(app: &mut TestApp, link: &str, chapter: &str, comment: &str) -> String {
    let r = app
        .post(
            &format!("/api/beta/{link}/feedback"),
            &json!({
                "chapter_id": chapter,
                "selected_text": "Nobody came",
                "context_block": "Nobody came up the stairs that night.",
                "comment": comment,
            }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
    r.id()
}

// ── Transport and auth ──────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_token_or_a_bad_one_is_401() {
    let mut app = TestApp::new().await;
    app.register("alice", "hunter2hunter2").await;
    let init = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {} });

    for auth in [
        None,
        Some("Bearer pw_not-a-real-token"),
        Some("Bearer something-else"),
        Some("Basic dXNlcjpwYXNz"),
    ] {
        let mut builder = Request::builder()
            .method("POST")
            .uri("/api/mcp")
            .header("host", "localhost")
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream");
        if let Some(a) = auth {
            builder = builder.header("authorization", a);
        }
        let (status, headers, _) = app
            .send_raw(builder.body(Body::from(init.to_string())).unwrap())
            .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "auth {auth:?}");
        assert_eq!(
            headers.get("www-authenticate").and_then(|v| v.to_str().ok()),
            Some("Bearer"),
            "auth {auth:?}"
        );
    }

    // A session cookie is not a token: the endpoint only reads Authorization.
    let r = app.post("/api/mcp", &init).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);

    // A revoked token stops working.
    let token = mint(&mut app, "agent", Value::Null).await;
    let id = app.get("/api/tokens").await.json[0]["id"].as_str().unwrap().to_string();
    let _ = rpc(&mut app, &token, "tools/list", json!({})).await;
    assert_eq!(app.delete(&format!("/api/tokens/{id}")).await.status, StatusCode::OK);
    let (status, _) = rpc_raw(&mut app, Some(&format!("Bearer {token}")), init).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn initialize_then_the_exact_tool_set() {
    let mut app = TestApp::new().await;
    app.register("alice", "hunter2hunter2").await;
    let token = mint(&mut app, "Claude", Value::Null).await;

    let init = rpc(
        &mut app,
        &token,
        "initialize",
        json!({
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": { "name": "test", "version": "0" }
        }),
    )
    .await;
    assert_eq!(init["serverInfo"]["name"], "plotweb");
    assert!(init["capabilities"]["tools"].is_object(), "{init}");
    let instructions = init["instructions"].as_str().expect("instructions");
    assert!(instructions.contains("cannot write"), "{instructions}");
    assert!(instructions.contains("exact"), "{instructions}");
    assert!(instructions.contains("Note bodies cannot be edited"), "{instructions}");

    // Stateless: no session id handed out, and none needed for the next call.
    let listed = rpc(&mut app, &token, "tools/list", json!({})).await;
    let tools = listed["tools"].as_array().expect("tools");
    let mut names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    names.sort();
    assert_eq!(
        names, EXPECTED_TOOLS,
        "the tool set changed. No tool may write chapter content or titles, reorder \
         chapters, delete anything, or touch tokens"
    );

    for t in tools {
        let name = t["name"].as_str().unwrap();
        assert!(
            t["description"].as_str().is_some_and(|d| d.len() > 20),
            "{name} needs a description"
        );
        let ann = &t["annotations"];
        if READ_ONLY_TOOLS.contains(&name) {
            assert_eq!(ann["readOnlyHint"], true, "{name}: {ann}");
        } else {
            assert_eq!(ann["readOnlyHint"], false, "{name}: {ann}");
            assert_eq!(ann["destructiveHint"], false, "{name}: {ann}");
        }
        // Nothing an agent can send is a chapter body or a note body edit.
        let props = t["inputSchema"]["properties"].as_object().cloned().unwrap_or_default();
        assert!(!props.contains_key("content"), "{name} takes content: {t}");
        if name != "create_note" {
            assert!(!props.contains_key("body"), "{name} takes a body: {t}");
        }
    }
}

// ── Scope ───────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tokens_reach_only_their_books() {
    let mut app = TestApp::new().await;
    app.register("alice", "hunter2hunter2").await;
    let (book_a, ch_a) = book_with_prose(&mut app, "Book A").await;
    let (book_b, ch_b) = book_with_prose(&mut app, "Book B").await;
    let scoped = mint(&mut app, "scoped", json!([book_a])).await;
    let all = mint(&mut app, "all", Value::Null).await;

    let listed = call_ok(&mut app, &scoped, "list_books", json!({})).await;
    let ids: Vec<&str> = listed["books"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec![book_a.as_str()]);
    assert_eq!(listed["books"][0]["title"], "Book A");
    assert_eq!(listed["books"][0]["chapter_count"], 1);

    let listed = call_ok(&mut app, &all, "list_books", json!({})).await;
    assert_eq!(listed["books"].as_array().unwrap().len(), 2);

    // Out of scope reads exactly like nonexistent.
    let out_of_scope = call_err(
        &mut app,
        &scoped,
        "read_chapter",
        json!({ "book_id": book_b, "chapter_id": ch_b }),
    )
    .await;
    let missing = call_err(
        &mut app,
        &scoped,
        "read_chapter",
        json!({ "book_id": "no-such-book", "chapter_id": ch_b }),
    )
    .await;
    assert_eq!(out_of_scope, "book not found");
    assert_eq!(missing, out_of_scope);
    for tool in ["book_outline", "manuscript_stats", "list_notes"] {
        let msg = call_err(&mut app, &scoped, tool, json!({ "book_id": book_b })).await;
        assert_eq!(msg, "book not found", "{tool}");
    }
    let msg = call_err(
        &mut app,
        &scoped,
        "add_review_comment",
        json!({ "book_id": book_b, "chapter_id": ch_b, "quote": "Nobody came", "comment": "x" }),
    )
    .await;
    assert_eq!(msg, "book not found");
    assert!(!call(&mut app, &scoped, "read_chapter", json!({ "book_id": book_a, "chapter_id": ch_a }))
        .await["isError"]
        .as_bool()
        .unwrap_or(false));

    // An all-books token is still only its owner's books.
    app.logout_local();
    app.register("bob", "hunter2hunter2").await;
    let (book_bob, ch_bob) = book_with_prose(&mut app, "Bob's").await;
    let msg = call_err(
        &mut app,
        &all,
        "read_chapter",
        json!({ "book_id": book_bob, "chapter_id": ch_bob }),
    )
    .await;
    assert_eq!(msg, "book not found");
    let listed = call_ok(&mut app, &all, "list_books", json!({})).await;
    assert!(!listed.to_string().contains(&book_bob));
}

// ── Reading ────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn read_chapter_is_plain_text_with_entities_decoded() {
    let mut app = TestApp::new().await;
    app.register("alice", "hunter2hunter2").await;
    let book = app.create_book("Entities").await;
    let ch = app.create_chapter(&book, "One").await;
    // Legacy Markdown with the entities the old editor baked in.
    set_chapter(
        &mut app,
        &book,
        &ch,
        "# Arrival\n\nShe said&comma; &ldquo;don&rsquor;t&rdquo;&period; Tom &amp; Jerry&#39;s **boat**.\n\nSecond paragraph.",
    )
    .await;
    let token = mint(&mut app, "agent", Value::Null).await;

    let r = call(&mut app, &token, "read_chapter", json!({ "book_id": book, "chapter_id": ch })).await;
    assert!(!is_error(&r), "{r}");
    let t = texts(&r);
    let meta: Value = serde_json::from_str(&t[0]).unwrap();
    assert_eq!(meta["title"], "One");
    assert_eq!(meta["id"], ch);
    assert_eq!(
        t[1],
        "# Arrival\n\nShe said, \u{201c}don\u{2019}t\u{201d}. Tom & Jerry's boat.\n\nSecond paragraph."
    );

    let missing = call_err(
        &mut app,
        &token,
        "read_chapter",
        json!({ "book_id": book, "chapter_id": "nope" }),
    )
    .await;
    assert_eq!(missing, "chapter not found");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn search_finds_snippets_and_clamps_max_results() {
    let mut app = TestApp::new().await;
    app.register("alice", "hunter2hunter2").await;
    let (book, ch) = book_with_prose(&mut app, "Searchable").await;
    let ch2 = app.create_chapter(&book, "Echoes").await;
    let many: Vec<String> = (0..60).map(|i| format!("Echo number {i} rang out.")).collect();
    let refs: Vec<&str> = many.iter().map(String::as_str).collect();
    set_chapter(&mut app, &book, &ch2, &doc_json(&refs)).await;
    let token = mint(&mut app, "agent", Value::Null).await;

    let r = call_ok(
        &mut app,
        &token,
        "search_manuscript",
        json!({ "book_id": book, "query": "THE FOG" }),
    )
    .await;
    assert_eq!(r["total_hits"], 1);
    assert_eq!(r["results"][0]["chapter_id"], ch);
    assert_eq!(r["results"][0]["chapter_title"], "The Lighthouse");
    let snip = r["results"][0]["snippet"].as_str().unwrap();
    assert!(snip.contains("guttered against the fog, and Mira"), "{snip}");

    let r = call_ok(
        &mut app,
        &token,
        "search_manuscript",
        json!({ "book_id": book, "query": "echo", "max_results": 500 }),
    )
    .await;
    assert_eq!(r["total_hits"], 60);
    assert_eq!(r["returned"], 50, "clamped to the maximum");
    assert_eq!(r["truncated"], true);

    let r = call_ok(
        &mut app,
        &token,
        "search_manuscript",
        json!({ "book_id": book, "query": "echo", "max_results": 3 }),
    )
    .await;
    assert_eq!(r["returned"], 3);

    let r = call_ok(
        &mut app,
        &token,
        "search_manuscript",
        json!({ "book_id": book, "query": "echo" }),
    )
    .await;
    assert_eq!(r["returned"], 50, "default cap");

    let msg = call_err(
        &mut app,
        &token,
        "search_manuscript",
        json!({ "book_id": book, "query": "   " }),
    )
    .await;
    assert!(msg.contains("empty"), "{msg}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn outline_and_stats() {
    let mut app = TestApp::new().await;
    app.register("alice", "hunter2hunter2").await;
    let (book, ch) = book_with_prose(&mut app, "Outlined").await;
    let parent = app
        .post(&format!("/api/books/{book}/notes"), &json!({ "title": "People" }))
        .await
        .id();
    let child = app
        .post(
            &format!("/api/books/{book}/notes"),
            &json!({ "title": "Mira", "parent_id": parent }),
        )
        .await
        .id();
    app.put(
        &format!("/api/books/{book}/notes/{child}"),
        &json!({ "is_entity": true }),
    )
    .await;
    let token = mint(&mut app, "agent", Value::Null).await;

    let stats = call_ok(&mut app, &token, "manuscript_stats", json!({ "book_id": book })).await;
    // 11 + 7 words.
    assert_eq!(stats["total_words"], 18, "{stats}");
    assert_eq!(stats["chapters"][0]["id"], ch);

    let outline = call_ok(&mut app, &token, "book_outline", json!({ "book_id": book })).await;
    assert_eq!(outline["chapters"][0]["title"], "The Lighthouse");
    assert_eq!(outline["chapters"][0]["word_count"], 18);
    assert_eq!(outline["notes"][0]["title"], "People");
    assert_eq!(outline["notes"][0]["children"][0]["id"], child);
    assert_eq!(outline["notes"][0]["children"][0]["facets"], json!(["entity"]));
}

// ── Review comments ────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_review_comment_needs_a_real_quote_and_reaches_the_author() {
    let mut app = TestApp::new().await;
    app.register("alice", "hunter2hunter2").await;
    let (book, ch) = book_with_prose(&mut app, "Reviewed").await;
    let token = mint(&mut app, "Claude Code", Value::Null).await;

    // Author's rail listens on the author channel; readers on the book's.
    let mut author_rx = app
        .state()
        .broadcaster
        .subscribe(&plotweb_server::ws::author_channel(&book));
    let mut shared_rx = app.state().broadcaster.subscribe(&book);

    // Made up: refused, nothing stored.
    let msg = call_err(
        &mut app,
        &token,
        "add_review_comment",
        json!({ "book_id": book, "chapter_id": ch, "quote": "the fog rolled in", "comment": "?" }),
    )
    .await;
    assert!(msg.contains("exact"), "{msg}");
    let list = app.get(&format!("/api/books/{book}/feedback")).await;
    assert_eq!(list.json, json!([]));
    assert!(author_rx.try_recv().is_err());

    // Real (whitespace may differ).
    let r = call_ok(
        &mut app,
        &token,
        "add_review_comment",
        json!({
            "book_id": book,
            "chapter_id": ch,
            "quote": "guttered   against\nthe fog",
            "comment": "Lovely image; consider cutting 'and Mira counted the bells'."
        }),
    )
    .await;
    let fb_id = r["feedback_id"].as_str().unwrap().to_string();

    let list = app.get(&format!("/api/books/{book}/feedback")).await;
    assert_eq!(list.status, StatusCode::OK);
    let fb = &list.json[0];
    assert_eq!(fb["id"], fb_id);
    assert_eq!(fb["source"], "agent");
    assert_eq!(fb["reader_name"], "Claude Code");
    assert_eq!(fb["link_id"], "");
    assert_eq!(fb["chapter_id"], ch);
    assert_eq!(fb["selected_text"], "guttered against the fog");
    assert_eq!(
        fb["context_block"],
        "The lantern guttered against the fog, and Mira counted the bells."
    );

    // Broadcast to the author only.
    let msg: Value = serde_json::from_str(&author_rx.try_recv().expect("author told")).unwrap();
    assert_eq!(msg["type"], "NewFeedback");
    assert_eq!(msg["source"], "agent");
    assert!(shared_rx.try_recv().is_err(), "readers' channel must not hear it");

    // A quote spanning two paragraphs anchors to where it starts, as the rail does.
    let r = call_ok(
        &mut app,
        &token,
        "add_review_comment",
        json!({
            "book_id": book,
            "chapter_id": ch,
            "quote": "counted the bells. Nobody came",
            "comment": "Nice transition."
        }),
    )
    .await;
    assert!(r["feedback_id"].is_string());

    // list_feedback sees both, newest first, marked as agent.
    let listed = call_ok(&mut app, &token, "list_feedback", json!({ "book_id": book })).await;
    let items = listed["feedback"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert!(items.iter().all(|i| i["source"] == "agent" && i["author"] == "Claude Code"));
    assert_eq!(items[0]["chapter_title"], "The Lighthouse");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn beta_readers_never_see_agent_feedback() {
    let mut app = TestApp::new().await;
    app.register("alice", "hunter2hunter2").await;
    let (book, ch) = book_with_prose(&mut app, "Shared").await;
    let link = beta_link(&mut app, &book, "Rae").await;
    let reader_fb = reader_feedback(&mut app, &link, &ch, "Spooky!").await;
    let token = mint(&mut app, "Claude", Value::Null).await;

    let r = call_ok(
        &mut app,
        &token,
        "add_review_comment",
        json!({ "book_id": book, "chapter_id": ch, "quote": "Nobody came", "comment": "agent note" }),
    )
    .await;
    let agent_fb = r["feedback_id"].as_str().unwrap().to_string();
    // The agent also replies on the reader's own comment.
    call_ok(
        &mut app,
        &token,
        "reply_to_feedback",
        json!({ "book_id": book, "feedback_id": reader_fb, "comment": "agent reply to reader" }),
    )
    .await;

    let r = app.get(&format!("/api/beta/{link}/feedback")).await;
    assert_eq!(r.status, StatusCode::OK);
    let body = r.json.to_string();
    let items = r.json.as_array().unwrap();
    assert_eq!(items.len(), 1, "{body}");
    assert_eq!(items[0]["id"], reader_fb);
    assert!(!body.contains(&agent_fb), "{body}");
    assert!(!body.contains("agent note"), "{body}");
    assert!(!body.contains("agent reply to reader"), "{body}");

    // A reader cannot reply into an agent comment either.
    let r = app
        .post(
            &format!("/api/beta/{link}/feedback/{agent_fb}/replies"),
            &json!({ "content": "hi" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);

    // The author sees everything, including the agent's reply.
    let r = app.get(&format!("/api/books/{book}/feedback")).await;
    let items = r.json.as_array().unwrap();
    assert_eq!(items.len(), 2);
    let on_reader = items.iter().find(|f| f["id"] == reader_fb).unwrap();
    assert_eq!(on_reader["source"], "reader");
    assert_eq!(on_reader["reader_name"], "Rae");
    assert_eq!(on_reader["replies"][0]["author_type"], "agent");
    assert_eq!(on_reader["replies"][0]["author_name"], "Claude");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_author_resolves_replies_to_and_deletes_agent_feedback() {
    let mut app = TestApp::new().await;
    app.register("alice", "hunter2hunter2").await;
    let (book, ch) = book_with_prose(&mut app, "Managed").await;
    let token = mint(&mut app, "Claude", Value::Null).await;
    let fb = call_ok(
        &mut app,
        &token,
        "add_review_comment",
        json!({ "book_id": book, "chapter_id": ch, "quote": "Nobody came", "comment": "tense?" }),
    )
    .await["feedback_id"]
        .as_str()
        .unwrap()
        .to_string();

    let r = app
        .post(
            &format!("/api/books/{book}/feedback/{fb}/replies"),
            &json!({ "content": "Intentional." }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
    let r = app
        .put(&format!("/api/books/{book}/feedback/{fb}/resolve"), &json!({}))
        .await;
    assert_eq!(r.status, StatusCode::OK);
    let list = app.get(&format!("/api/books/{book}/feedback")).await;
    assert_eq!(list.json[0]["resolved"], true);
    assert_eq!(list.json[0]["replies"][0]["author_type"], "owner");
    assert_eq!(list.json[0]["replies"][0]["content"], "Intentional.");

    // Resolved feedback drops out of list_feedback unless asked for.
    let l = call_ok(&mut app, &token, "list_feedback", json!({ "book_id": book })).await;
    assert_eq!(l["feedback"], json!([]));
    let l = call_ok(
        &mut app,
        &token,
        "list_feedback",
        json!({ "book_id": book, "include_resolved": true, "chapter_id": ch }),
    )
    .await;
    assert_eq!(l["feedback"].as_array().unwrap().len(), 1);

    // Another book's route cannot touch it.
    let other = app.create_book("Other").await;
    app.delete(&format!("/api/books/{other}/feedback/{fb}")).await;
    assert_eq!(app.get(&format!("/api/books/{book}/feedback")).await.json.as_array().unwrap().len(), 1);

    let r = app.delete(&format!("/api/books/{book}/feedback/{fb}")).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(app.get(&format!("/api/books/{book}/feedback")).await.json, json!([]));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn replies_stay_within_the_book() {
    let mut app = TestApp::new().await;
    app.register("alice", "hunter2hunter2").await;
    let (book_a, ch_a) = book_with_prose(&mut app, "A").await;
    let (book_b, ch_b) = book_with_prose(&mut app, "B").await;
    let link_a = beta_link(&mut app, &book_a, "Rae").await;
    let link_b = beta_link(&mut app, &book_b, "Sam").await;
    let fb_a = reader_feedback(&mut app, &link_a, &ch_a, "on A").await;
    let fb_b = reader_feedback(&mut app, &link_b, &ch_b, "on B").await;
    let token = mint(&mut app, "Claude", Value::Null).await;

    let r = call_ok(
        &mut app,
        &token,
        "reply_to_feedback",
        json!({ "book_id": book_a, "feedback_id": fb_a, "comment": "Agreed." }),
    )
    .await;
    assert_eq!(r["author"], "Claude");

    // B's feedback through A's id: not found, nothing written.
    let msg = call_err(
        &mut app,
        &token,
        "reply_to_feedback",
        json!({ "book_id": book_a, "feedback_id": fb_b, "comment": "Sneaky." }),
    )
    .await;
    assert_eq!(msg, "feedback not found");
    let b = app.get(&format!("/api/books/{book_b}/feedback")).await;
    assert_eq!(b.json[0]["replies"], json!([]));

    let msg = call_err(
        &mut app,
        &token,
        "reply_to_feedback",
        json!({ "book_id": book_a, "feedback_id": fb_a, "comment": "  " }),
    )
    .await;
    assert!(msg.contains("empty"), "{msg}");

    let a = app.get(&format!("/api/books/{book_a}/feedback")).await;
    assert_eq!(a.json[0]["replies"][0]["author_type"], "agent");
    assert_eq!(a.json[0]["replies"][0]["content"], "Agreed.");
}

/// Feedback written before agents existed has no `book_id` / `source` /
/// `author_name`. It must still list for the author (as a reader's) and for its reader.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn feedback_rows_written_the_old_way_still_list() {
    let mut app = TestApp::new().await;
    app.register("alice", "hunter2hunter2").await;
    let (book, ch) = book_with_prose(&mut app, "Old").await;
    let link = beta_link(&mut app, &book, "Old Reader").await;
    let link_id = app.get(&format!("/api/books/{book}/beta-links")).await.json[0]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Exactly the fields the pre-agent code wrote.
    let fields = plotweb_server::rhype::Fields::new()
        .str("uuid", "old-feedback-1")
        .str("link_id", &link_id)
        .str("chapter_id", &ch)
        .str("selected_text", "Nobody came")
        .str("context_block", "Nobody came up the stairs that night.")
        .str("comment", "from before")
        .bool("resolved", false)
        .str("created_at", "2025-01-01 00:00:00")
        .render();
    app.state()
        .rhype
        .create(format!("BetaFeedback.create({fields})"))
        .await
        .expect("old-shape row");

    // And an agent comment beside it, so the merge is exercised.
    let token = mint(&mut app, "Claude", Value::Null).await;
    call_ok(
        &mut app,
        &token,
        "add_review_comment",
        json!({ "book_id": book, "chapter_id": ch, "quote": "Mira", "comment": "new" }),
    )
    .await;

    let r = app.get(&format!("/api/books/{book}/feedback")).await;
    let items = r.json.as_array().unwrap();
    assert_eq!(items.len(), 2, "{}", r.json);
    assert_eq!(items[0]["source"], "agent", "newest first");
    assert_eq!(items[1]["id"], "old-feedback-1");
    assert_eq!(items[1]["source"], "reader");
    assert_eq!(items[1]["reader_name"], "Old Reader");

    let r = app.get(&format!("/api/beta/{link}/feedback")).await;
    let items = r.json.as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], "old-feedback-1");

    // The author can still resolve it through its link.
    let r = app
        .put(&format!("/api/books/{book}/feedback/old-feedback-1/resolve"), &json!({}))
        .await;
    assert_eq!(r.status, StatusCode::OK);
    let r = app.get(&format!("/api/books/{book}/feedback")).await;
    let old = r.json.as_array().unwrap().iter().find(|f| f["id"] == "old-feedback-1").unwrap().clone();
    assert_eq!(old["resolved"], true);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deleting_a_book_removes_its_agent_feedback() {
    let mut app = TestApp::new().await;
    app.register("alice", "hunter2hunter2").await;
    let (book, ch) = book_with_prose(&mut app, "Doomed").await;
    let token = mint(&mut app, "Claude", Value::Null).await;
    let fb = call_ok(
        &mut app,
        &token,
        "add_review_comment",
        json!({ "book_id": book, "chapter_id": ch, "quote": "Mira", "comment": "x" }),
    )
    .await["feedback_id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(app.delete(&format!("/api/books/{book}")).await.status, StatusCode::OK);
    let left = app
        .state()
        .rhype
        .find(format!("BetaFeedback.filter(.uuid == {})", plotweb_server::rhype::quote(&fb)))
        .await
        .unwrap();
    assert!(left.is_empty());
}

// ── Notes ──────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn update_note_changes_structure_on_a_cut_over_book_and_never_a_body() {
    let mut app = TestApp::new().await;
    app.register("alice", "hunter2hunter2").await;
    let book = app.create_book("Cut").await;
    app.create_chapter(&book, "One").await;
    let mira = app
        .post(&format!("/api/books/{book}/notes"), &json!({ "title": "Mira" }))
        .await
        .id();
    let places = app
        .post(&format!("/api/books/{book}/notes"), &json!({ "title": "Places" }))
        .await
        .id();
    let siege = app
        .post(&format!("/api/books/{book}/notes"), &json!({ "title": "The Siege" }))
        .await
        .id();
    app.cut_over(&book).await;
    let token = mint(&mut app, "Claude", Value::Null).await;

    let r = call_ok(
        &mut app,
        &token,
        "update_note",
        json!({
            "book_id": book,
            "note_id": mira,
            "title": "Mira Vale",
            "color": "#c0392b",
            "pinned": true,
            "facets": {
                "is_entity": true,
                "span": { "start": { "tick": 31536000, "precision": 0 } },
                "event_parent": siege
            },
            "parent_id": places
        }),
    )
    .await;
    assert_eq!(r["updated"]["title"], "Mira Vale");
    assert_eq!(r["updated"]["parent_id"], places);

    // Visible afterwards through the same reads the app uses.
    let n = app.get(&format!("/api/books/{book}/notes/{mira}")).await;
    assert_eq!(n.json["title"], "Mira Vale");
    assert_eq!(n.json["is_entity"], true);
    assert_eq!(n.json["pinned"], true);
    assert_eq!(n.json["event_parent"], siege);
    assert_eq!(n.json["span"]["start"]["tick"], 31536000);
    let list = app.get(&format!("/api/books/{book}/notes")).await;
    let listed = list.json["notes"].as_array().unwrap().iter().find(|n| n["id"] == mira).unwrap().clone();
    assert_eq!(listed["title"], "Mira Vale");
    assert_eq!(listed["color"], "#c0392b");
    assert_eq!(listed["is_entity"], true);
    assert_eq!(list.json["tree"]["children"][&places], json!([mira]));

    // Clearing a facet with null.
    call_ok(
        &mut app,
        &token,
        "update_note",
        json!({ "book_id": book, "note_id": mira, "facets": { "span": null } }),
    )
    .await;
    let n = app.get(&format!("/api/books/{book}/notes/{mira}")).await;
    assert!(n.json.get("span").is_none(), "{}", n.json);

    // Back to the top level, first.
    call_ok(
        &mut app,
        &token,
        "update_note",
        json!({ "book_id": book, "note_id": mira, "parent_id": null, "position": 0 }),
    )
    .await;
    let list = app.get(&format!("/api/books/{book}/notes")).await;
    assert_eq!(list.json["tree"]["root_order"][0], mira);

    // No body: an attempt is refused outright, and nothing changes.
    let before = app.get(&format!("/api/books/{book}/notes/{mira}")).await.json["content"].clone();
    for field in ["content", "body"] {
        let (status, resp) = rpc_raw(
            &mut app,
            Some(&format!("Bearer {token}")),
            json!({
                "jsonrpc": "2.0", "id": 7, "method": "tools/call",
                "params": { "name": "update_note", "arguments": {
                    "book_id": book, "note_id": mira, "title": "Hijacked", field: "new prose"
                }}
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            resp.get("error").is_some() || resp["result"]["isError"] == true,
            "a {field} must be refused: {resp}"
        );
    }
    let after = app.get(&format!("/api/books/{book}/notes/{mira}")).await.json;
    assert_eq!(after["content"], before);
    assert_eq!(after["title"], "Mira Vale", "the whole call was refused");

    // Bad references are refused.
    let msg = call_err(
        &mut app,
        &token,
        "update_note",
        json!({ "book_id": book, "note_id": mira, "facets": { "event_parent": mira } }),
    )
    .await;
    assert!(msg.contains("event_parent"), "{msg}");
    let msg = call_err(
        &mut app,
        &token,
        "update_note",
        json!({ "book_id": book, "note_id": "nope", "title": "x" }),
    )
    .await;
    assert_eq!(msg, "note not found");
}

/// The body gate: `create_note` may take an initial body only because it provably
/// lands for a cut-over book — readable through the REST read, present in the
/// canonical document a syncing client pulls, in git, across a restart, and still
/// there after a client claims the provisional document the way the editor does.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn create_note_body_is_durable_on_a_cut_over_book() {
    let mut app = TestApp::new().await;
    app.register("alice", "hunter2hunter2").await;
    let book = app.create_book("Cut").await;
    app.create_chapter(&book, "One").await;
    let places = app
        .post(&format!("/api/books/{book}/notes"), &json!({ "title": "Places" }))
        .await
        .id();
    app.cut_over(&book).await;
    let token = mint(&mut app, "Claude", Value::Null).await;

    let r = call_ok(
        &mut app,
        &token,
        "create_note",
        json!({
            "book_id": book,
            "title": "The Lighthouse",
            "parent_id": places,
            "facets": { "is_entity": true },
            "body": "A **granite** tower on the point.\n\nMira keeps the lamp."
        }),
    )
    .await;
    let id = r["created"]["id"].as_str().unwrap().to_string();
    assert_eq!(r["body"], "A granite tower on the point.\n\nMira keeps the lamp.");

    let check = |content: &str| {
        let text = plotweb_export::text::readable_text(content, plotweb_export::text::Legacy::Html);
        assert_eq!(text, "A granite tower on the point.\n\nMira keeps the lamp.", "{content}");
    };

    // 1. The REST read every client uses.
    let n = app.get(&format!("/api/books/{book}/notes/{id}")).await;
    assert_eq!(n.status, StatusCode::OK);
    check(n.json["content"].as_str().unwrap());
    assert_eq!(n.json["title"], "The Lighthouse");
    assert_eq!(n.json["is_entity"], true);
    let list = app.get(&format!("/api/books/{book}/notes")).await;
    assert!(list.json["notes"].as_array().unwrap().iter().any(|n| n["id"] == id));
    assert_eq!(list.json["tree"]["children"][&places], json!([id]));

    // 2. The canonical document a syncing client pulls.
    let (status, bytes) = app.get_bytes(&format!("/api/books/{book}/sync/note:{id}")).await;
    assert_eq!(status, StatusCode::OK);
    check(&plotweb_crdt::materialize_body(&bytes).expect("canonical body loads"));
    let (status, _) = app.get_bytes(&format!("/api/books/{book}/sync/heads")).await;
    assert_eq!(status, StatusCode::OK);

    // 3. Git, the mirror every read falls back to.
    let git = app.state().books.get_note(&book, &id).await.expect("git note").content;
    check(&git);

    // 4. A restart.
    app.restart().await;
    app.cut_over(&book).await;
    check(app.get(&format!("/api/books/{book}/notes/{id}")).await.json["content"].as_str().unwrap());

    // 5. A client claims the provisional document the way the editor does: seeded
    //    from the REST read, posted to /adopt. The body survives, because the REST read
    //    served it.
    let seeded = app.get(&format!("/api/books/{book}/notes/{id}")).await.json["content"]
        .as_str()
        .unwrap()
        .to_string();
    let claim = plotweb_crdt::project_body(&seeded, BodyKind::Note).expect("project");
    let (status, resp) = app
        .post_bytes(&format!("/api/books/{book}/sync/note:{id}/adopt"), &claim)
        .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&resp));
    check(app.get(&format!("/api/books/{book}/notes/{id}")).await.json["content"].as_str().unwrap());

    // 6. And it is now sync's to carry: a later REST body write is dropped, which is
    //    exactly why update_note has no body parameter.
    let r = app
        .put(&format!("/api/books/{book}/notes/{id}"), &json!({ "content": "<p>overwritten</p>" }))
        .await;
    assert_eq!(r.json["deferred_to_sync"], true, "{}", r.json);
    check(app.get(&format!("/api/books/{book}/notes/{id}")).await.json["content"].as_str().unwrap());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn create_note_without_a_body_and_on_a_git_book() {
    let mut app = TestApp::new().await;
    app.register("alice", "hunter2hunter2").await;
    let book = app.create_book("Git").await;
    let token = mint(&mut app, "Claude", Value::Null).await;

    let r = call_ok(&mut app, &token, "create_note", json!({ "book_id": book, "title": "Empty" })).await;
    let empty = r["created"]["id"].as_str().unwrap().to_string();
    let r = call_ok(
        &mut app,
        &token,
        "create_note",
        json!({ "book_id": book, "title": "Full", "body": "Some lore.", "pinned": true }),
    )
    .await;
    let full = r["created"]["id"].as_str().unwrap().to_string();

    let n = app.get(&format!("/api/books/{book}/notes/{full}")).await;
    assert_eq!(
        plotweb_export::text::readable_text(n.json["content"].as_str().unwrap(), plotweb_export::text::Legacy::Html),
        "Some lore."
    );
    assert_eq!(n.json["pinned"], true);
    let n = app.get(&format!("/api/books/{book}/notes/{empty}")).await;
    assert_eq!(n.json["content"], "");

    let read = call(&mut app, &token, "read_note", json!({ "book_id": book, "note_id": full })).await;
    assert!(!is_error(&read));
    assert_eq!(texts(&read)[1], "Some lore.");

    let listed = call_ok(&mut app, &token, "list_notes", json!({ "book_id": book })).await;
    assert_eq!(listed["notes"].as_array().unwrap().len(), 2);

    let msg = call_err(&mut app, &token, "create_note", json!({ "book_id": book, "title": " " })).await;
    assert!(msg.contains("title"), "{msg}");
    let msg = call_err(
        &mut app,
        &token,
        "create_note",
        json!({ "book_id": book, "title": "Orphan", "parent_id": "nope" }),
    )
    .await;
    assert!(msg.contains("parent_id"), "{msg}");
}
