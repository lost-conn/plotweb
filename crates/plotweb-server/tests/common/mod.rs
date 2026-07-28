//! Shared harness for in-process HTTP integration tests.
//!
//! Builds the real Axum app (via `plotweb_server::test_router`) over tempdir
//! SQLite + rhypedb + git stores, and drives it with `tower::ServiceExt::oneshot`.
//! A `TestApp` keeps a cookie jar so session auth works across requests.
//!
//! Not every test binary uses every helper, so silence per-binary dead-code.
#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

use plotweb_server::{build_state, test_router};

// Unique per-test SQLite filename so parallel tests never share a DB.
static SEQ: AtomicU64 = AtomicU64::new(0);

pub struct TestApp {
    router: axum::Router,
    cookie: Option<String>,
    // On-disk store paths, kept so the app can be rebuilt over the SAME stores
    // to simulate a server restart (see `restart`).
    db_url: String,
    book_dir: PathBuf,
    rhype_dir: PathBuf,
    // Held for the lifetime of the test so the tempdir isn't deleted early.
    _dir: tempfile::TempDir,
}

pub struct Resp {
    pub status: StatusCode,
    pub json: Value,
}

impl Resp {
    /// Convenience: the `id` field of a JSON object response.
    pub fn id(&self) -> String {
        self.json
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("response has no `id`: {}", self.json))
            .to_string()
    }
}

impl TestApp {
    pub async fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let db_path = dir.path().join(format!("test_{n}.db"));
        let db_url = format!("sqlite:{}", db_path.display());
        let book_dir = dir.path().join("books");
        let rhype_dir = dir.path().join("rhype");

        let state = build_state(&db_url, book_dir.clone(), &rhype_dir, None).await;
        TestApp {
            router: test_router(state).await,
            cookie: None,
            db_url,
            book_dir,
            rhype_dir,
            _dir: dir,
        }
    }

    /// The git `DATA_DIR` these stores live under — so a test can point the
    /// lock-free migration backfill/audit at the same books it created over HTTP.
    pub fn book_dir(&self) -> &PathBuf {
        &self.book_dir
    }

    /// Rebuild the app over the SAME on-disk stores (simulating a server
    /// restart), keeping the client's session cookie. The SQLite-backed session
    /// store must return the still-valid session afterwards.
    pub async fn restart(&mut self) {
        // Drop the old app first: its `RhypeStore` holds an exclusive lock on the
        // rhype data dir, so the new `build_state` can't open it until the old
        // one is released.
        self.router = axum::Router::new();
        let state = build_state(&self.db_url, self.book_dir.clone(), &self.rhype_dir, None).await;
        self.router = test_router(state).await;
    }

    async fn send(&mut self, mut req: Request<Body>) -> Resp {
        if let Some(c) = &self.cookie {
            req.headers_mut()
                .insert("cookie", c.parse().expect("cookie header"));
        }
        let resp = self.router.clone().oneshot(req).await.expect("response");

        // Capture session cookie for subsequent requests.
        if let Some(sc) = resp.headers().get("set-cookie") {
            if let Ok(s) = sc.to_str() {
                // Keep just the `name=value` pair (drop attributes after `;`).
                let pair = s.split(';').next().unwrap_or(s).to_string();
                self.cookie = Some(pair);
            }
        }

        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let json = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(Value::Null)
        };
        Resp { status, json }
    }

    pub async fn get(&mut self, uri: &str) -> Resp {
        let req = Request::builder()
            .method("GET")
            .uri(uri)
            .body(Body::empty())
            .unwrap();
        self.send(req).await
    }

    pub async fn post(&mut self, uri: &str, body: &Value) -> Resp {
        self.body_req("POST", uri, body).await
    }

    pub async fn put(&mut self, uri: &str, body: &Value) -> Resp {
        self.body_req("PUT", uri, body).await
    }

    pub async fn delete(&mut self, uri: &str) -> Resp {
        let req = Request::builder()
            .method("DELETE")
            .uri(uri)
            .body(Body::empty())
            .unwrap();
        self.send(req).await
    }

    async fn body_req(&mut self, method: &str, uri: &str, body: &Value) -> Resp {
        let req = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(body).unwrap()))
            .unwrap();
        self.send(req).await
    }

    /// POST a multipart file upload (single `file` field).
    pub async fn post_multipart(&mut self, uri: &str, filename: &str, data: &[u8]) -> Resp {
        let boundary = "----plotwebtestboundary";
        let mut body = Vec::new();
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(
            format!(
                "Content-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(b"Content-Type: application/octet-stream\r\n\r\n");
        body.extend_from_slice(data);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

        let req = Request::builder()
            .method("POST")
            .uri(uri)
            .header(
                "content-type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(Body::from(body))
            .unwrap();
        self.send(req).await
    }

    /// Drop any stored session cookie (simulate a fresh, unauthenticated client).
    pub fn logout_local(&mut self) {
        self.cookie = None;
    }

    // ── High-level helpers ──────────────────────────────────────────

    /// Register a user and leave the session authenticated. Returns the user id.
    pub async fn register(&mut self, username: &str, password: &str) -> String {
        let r = self
            .post(
                "/api/auth/register",
                &json!({
                    "username": username,
                    "email": format!("{username}@example.com"),
                    "password": password,
                }),
            )
            .await;
        assert_eq!(r.status, StatusCode::CREATED, "register: {}", r.json);
        r.id()
    }

    pub async fn login(&mut self, username: &str, password: &str) -> Resp {
        self.post(
            "/api/auth/login",
            &json!({ "username": username, "password": password }),
        )
        .await
    }

    /// Create a book, returning its id (session must be authenticated).
    pub async fn create_book(&mut self, title: &str) -> String {
        let r = self
            .post(
                "/api/books",
                &json!({ "title": title, "description": "desc" }),
            )
            .await;
        assert_eq!(r.status, StatusCode::CREATED, "create_book: {}", r.json);
        r.id()
    }

    /// Create a chapter, returning its id.
    pub async fn create_chapter(&mut self, book_id: &str, title: &str) -> String {
        let r = self
            .post(
                &format!("/api/books/{book_id}/chapters"),
                &json!({ "title": title }),
            )
            .await;
        assert_eq!(r.status, StatusCode::CREATED, "create_chapter: {}", r.json);
        r.id()
    }
}
