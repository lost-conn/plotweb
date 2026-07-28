//! Cross-platform HTTP API helpers over [`rinch_http`].
//!
//! The request/response calls (`get`/`post`/`put`/`delete_req`) are **callback-
//! based** and work on both web (wasm, `web_sys::fetch`) and native (`ureq`) —
//! `rinch_http` invokes the callback on the UI thread, so the closure can update
//! rinch Signals directly. JSON (de)serialization and error extraction stay here.
//!
//! File upload/download (`upload_file`/`upload_image`/`download_file`) are still
//! web-only (they use `web_sys` File/Blob/anchor); native file I/O is a later
//! phase. They are `#[cfg(target_arch = "wasm32")]`.

use serde::de::DeserializeOwned;
use serde::Serialize;

use rinch_http::{fetch, HttpError, Request, Response};

#[derive(Debug)]
pub struct ApiError {
    pub message: String,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

// Mirrors `rinch_http::HttpCallback`: invoked on the UI thread on both targets and
// never crosses a thread boundary, so it need not be `Send` — letting callbacks
// capture `!Send` UI state such as the Rc-based editor handles.
pub trait ApiCallback<T>: FnOnce(Result<T, ApiError>) + 'static {}
impl<T, F> ApiCallback<T> for F where F: FnOnce(Result<T, ApiError>) + 'static {}

/// The origin prepended to API paths.
///
/// Empty on web — relative `/api/...` paths resolve against the page origin. On
/// native there is no page origin and `ureq` needs an absolute URL, so we prepend
/// the hosted server's address (override with `PLOTWEB_SERVER`).
#[cfg(target_arch = "wasm32")]
fn base_url() -> String {
    String::new()
}

#[cfg(not(target_arch = "wasm32"))]
fn base_url() -> String {
    std::env::var("PLOTWEB_SERVER").unwrap_or_else(|_| "http://127.0.0.1:3000".to_string())
}

/// Resolve an API path to the full URL for the current platform.
fn full_url(path: &str) -> String {
    format!("{}{}", base_url(), path)
}

/// Turn a raw [`rinch_http`] result into the deserialized `T` or an [`ApiError`].
///
/// Runs inside the fetch callback (on the UI thread). A 4xx/5xx response carries
/// its body, so we surface the server's `{ "error": ... }` message when present.
fn parse<T: DeserializeOwned>(result: Result<Response, HttpError>) -> Result<T, ApiError> {
    let resp = result.map_err(|e| ApiError {
        message: e.to_string(),
    })?;
    let status = resp.status;
    let text = resp.text();

    if status >= 400 {
        if let Ok(err) = serde_json::from_str::<plotweb_common::ApiError>(&text) {
            return Err(ApiError { message: err.error });
        }
        return Err(ApiError {
            message: format!("HTTP {}: {}", status, text),
        });
    }

    serde_json::from_str(&text).map_err(|e| ApiError {
        message: format!(
            "JSON parse error: {} (body: {})",
            e,
            text.get(..200).unwrap_or(&text)
        ),
    })
}

/// `GET url`, deserializing the JSON body into `T`.
pub fn get<T: DeserializeOwned + 'static>(url: &str, on_done: impl ApiCallback<T>) {
    let req = Request::get(&full_url(url)).header("Content-Type", "application/json");
    fetch(req, move |res| on_done(parse::<T>(res)));
}

/// `POST url` with a JSON `body`, deserializing the JSON response into `T`.
pub fn post<B: Serialize, T: DeserializeOwned + 'static>(
    url: &str,
    body: &B,
    on_done: impl ApiCallback<T>,
) {
    let json = serde_json::to_string(body).unwrap_or_default();
    let req = Request::post(&full_url(url))
        .header("Content-Type", "application/json")
        .body_str(&json);
    fetch(req, move |res| on_done(parse::<T>(res)));
}

/// `PUT url` with a JSON `body`, deserializing the JSON response into `T`.
pub fn put<B: Serialize, T: DeserializeOwned + 'static>(
    url: &str,
    body: &B,
    on_done: impl ApiCallback<T>,
) {
    let json = serde_json::to_string(body).unwrap_or_default();
    let req = Request::put(&full_url(url))
        .header("Content-Type", "application/json")
        .body_str(&json);
    fetch(req, move |res| on_done(parse::<T>(res)));
}

/// A failed binary request: the HTTP status (0 = transport failure, i.e. offline)
/// plus a message. The sync engine branches on `status` — 401 is a *state*
/// (signed out), not an error to retry.
#[derive(Debug, Clone)]
pub struct BinError {
    pub status: u16,
    pub message: String,
}

/// `POST url` with a raw byte body, yielding the raw response bytes.
///
/// The sync endpoints speak `application/octet-stream` in both directions
/// (Automerge sync messages), so neither side of this goes through JSON.
pub fn post_bytes(
    url: &str,
    body: Vec<u8>,
    on_done: impl FnOnce(Result<Vec<u8>, BinError>) + 'static,
) {
    let req = Request::post(&full_url(url))
        .header("Content-Type", "application/octet-stream")
        .body(body);
    fetch(req, move |res| {
        on_done(match res {
            // A transport error means we could not reach the server at all.
            Err(e) => Err(BinError {
                status: 0,
                message: e.to_string(),
            }),
            Ok(resp) if resp.status >= 400 => Err(BinError {
                status: resp.status,
                message: resp.text(),
            }),
            Ok(resp) => Ok(resp.body),
        })
    });
}

/// `DELETE url`, deserializing the JSON response into `T`.
pub fn delete_req<T: DeserializeOwned + 'static>(url: &str, on_done: impl ApiCallback<T>) {
    let req = Request::delete(&full_url(url)).header("Content-Type", "application/json");
    fetch(req, move |res| on_done(parse::<T>(res)));
}

// ── Web-only file transfer ───────────────────────────────────────────────────
// These use web_sys File/Blob/anchor. Native file dialogs / filesystem I/O are a
// later phase; the call sites are web-only UI flows for now.

#[cfg(target_arch = "wasm32")]
mod web_files {
    use super::ApiError;
    use serde::de::DeserializeOwned;
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;
    use web_sys::{Request, RequestCredentials, RequestInit, Response};

    async fn parse_response<T: DeserializeOwned>(resp: Response) -> Result<T, ApiError> {
        let status = resp.status();
        let text = JsFuture::from(resp.text().unwrap())
            .await
            .map_err(|e| ApiError {
                message: format!("{:?}", e),
            })?;
        let text = text.as_string().unwrap_or_default();

        if status >= 400 {
            if let Ok(err) = serde_json::from_str::<plotweb_common::ApiError>(&text) {
                return Err(ApiError { message: err.error });
            }
            return Err(ApiError {
                message: format!("HTTP {}: {}", status, text),
            });
        }

        serde_json::from_str(&text).map_err(|e| ApiError {
            message: format!(
                "JSON parse error: {} (body: {})",
                e,
                text.get(..200).unwrap_or(&text)
            ),
        })
    }

    /// Trigger a browser download of a binary GET endpoint (session credentials),
    /// saving via a temporary object URL + synthetic `<a download>` click. The
    /// filename comes from `Content-Disposition`, falling back to `fallback_name`.
    pub async fn download_file(url: &str, fallback_name: &str) -> Result<(), ApiError> {
        let init = RequestInit::new();
        init.set_method("GET");
        init.set_credentials(RequestCredentials::SameOrigin);
        let request = Request::new_with_str_and_init(url, &init).map_err(|e| ApiError {
            message: format!("{:?}", e),
        })?;

        let window = web_sys::window().unwrap();
        let resp_value = JsFuture::from(window.fetch_with_request(&request))
            .await
            .map_err(|e| ApiError {
                message: format!("{:?}", e),
            })?;
        let resp: Response = resp_value.dyn_into().map_err(|_| ApiError {
            message: "response is not a Response".into(),
        })?;

        let status = resp.status();
        if status >= 400 {
            let text = JsFuture::from(resp.text().unwrap())
                .await
                .ok()
                .and_then(|t| t.as_string())
                .unwrap_or_default();
            if let Ok(err) = serde_json::from_str::<plotweb_common::ApiError>(&text) {
                return Err(ApiError { message: err.error });
            }
            return Err(ApiError {
                message: format!("HTTP {}", status),
            });
        }

        let filename = resp
            .headers()
            .get("Content-Disposition")
            .ok()
            .flatten()
            .and_then(|cd| content_disposition_filename(&cd))
            .unwrap_or_else(|| fallback_name.to_string());

        let blob_promise = resp.blob().map_err(|e| ApiError {
            message: format!("{:?}", e),
        })?;
        let blob_value = JsFuture::from(blob_promise).await.map_err(|e| ApiError {
            message: format!("{:?}", e),
        })?;
        let blob: web_sys::Blob = blob_value.dyn_into().map_err(|_| ApiError {
            message: "response body is not a Blob".into(),
        })?;

        let object_url =
            web_sys::Url::create_object_url_with_blob(&blob).map_err(|e| ApiError {
                message: format!("{:?}", e),
            })?;

        let document = web_sys::window().unwrap().document().unwrap();
        let anchor: web_sys::HtmlAnchorElement = document
            .create_element("a")
            .map_err(|e| ApiError {
                message: format!("{:?}", e),
            })?
            .dyn_into()
            .map_err(|_| ApiError {
                message: "failed to create anchor".into(),
            })?;
        anchor.set_href(&object_url);
        anchor.set_download(&filename);
        anchor.click();
        web_sys::Url::revoke_object_url(&object_url).ok();

        Ok(())
    }

    fn content_disposition_filename(cd: &str) -> Option<String> {
        let start = cd.find("filename=")? + "filename=".len();
        let rest = cd[start..].trim();
        let name = rest
            .split(';')
            .next()
            .unwrap_or(rest)
            .trim()
            .trim_matches('"');
        if name.is_empty() {
            None
        } else {
            Some(name.to_string())
        }
    }

    /// Upload an image for a book via multipart form data.
    pub async fn upload_image(
        book_id: &str,
        file: &web_sys::File,
    ) -> Result<plotweb_common::ImageUploadResponse, ApiError> {
        upload_file(&format!("/api/books/{}/images", book_id), file).await
    }

    /// Upload a file via multipart form data. Does NOT set Content-Type (the
    /// browser sets it with the boundary automatically for FormData).
    pub async fn upload_file<T: DeserializeOwned>(
        url: &str,
        file: &web_sys::File,
    ) -> Result<T, ApiError> {
        let form_data = web_sys::FormData::new().map_err(|e| ApiError {
            message: format!("{:?}", e),
        })?;
        form_data
            .append_with_blob_and_filename("file", file, &file.name())
            .map_err(|e| ApiError {
                message: format!("{:?}", e),
            })?;

        let init = RequestInit::new();
        init.set_method("POST");
        init.set_credentials(RequestCredentials::SameOrigin);
        init.set_body(&form_data);

        let request = Request::new_with_str_and_init(url, &init).map_err(|e| ApiError {
            message: format!("{:?}", e),
        })?;

        let window = web_sys::window().unwrap();
        let resp_value = JsFuture::from(window.fetch_with_request(&request))
            .await
            .map_err(|e| ApiError {
                message: format!("{:?}", e),
            })?;
        let resp: Response = resp_value.dyn_into().map_err(|_| ApiError {
            message: "response is not a Response".into(),
        })?;

        parse_response(resp).await
    }
}

#[cfg(target_arch = "wasm32")]
pub use web_files::{download_file, upload_file, upload_image};

// Native stubs: file upload/download need OS file dialogs / filesystem I/O
// (Phase 3, "fonts + images on native"). For now they fail cleanly so the call
// sites compile for desktop.
#[cfg(not(target_arch = "wasm32"))]
mod native_files {
    use super::ApiError;
    use serde::de::DeserializeOwned;

    pub async fn download_file(_url: &str, _fallback_name: &str) -> Result<(), ApiError> {
        Err(ApiError {
            message: "file download is not supported on native yet".into(),
        })
    }

    pub async fn upload_image(
        _book_id: &str,
        _file: &web_sys::File,
    ) -> Result<plotweb_common::ImageUploadResponse, ApiError> {
        Err(ApiError {
            message: "image upload is not supported on native yet".into(),
        })
    }

    pub async fn upload_file<T: DeserializeOwned>(
        _url: &str,
        _file: &web_sys::File,
    ) -> Result<T, ApiError> {
        Err(ApiError {
            message: "file upload is not supported on native yet".into(),
        })
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use native_files::{download_file, upload_file, upload_image};
