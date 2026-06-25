use serde::de::DeserializeOwned;
use serde::Serialize;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Request, RequestCredentials, RequestInit, Response};

#[derive(Debug)]
pub struct ApiError {
    pub message: String,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

async fn do_fetch(url: &str, init: &RequestInit) -> Result<Response, ApiError> {
    let request =
        Request::new_with_str_and_init(url, init).map_err(|e| ApiError { message: format!("{:?}", e) })?;
    request
        .headers()
        .set("Content-Type", "application/json")
        .ok();

    let window = web_sys::window().unwrap();
    let resp_value = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(|e| ApiError { message: format!("{:?}", e) })?;

    let resp: Response = resp_value.dyn_into().map_err(|_| ApiError {
        message: "response is not a Response".into(),
    })?;

    Ok(resp)
}

async fn parse_response<T: DeserializeOwned>(resp: Response) -> Result<T, ApiError> {
    let status = resp.status();
    let text = JsFuture::from(resp.text().unwrap())
        .await
        .map_err(|e| ApiError { message: format!("{:?}", e) })?;
    let text = text.as_string().unwrap_or_default();

    if status >= 400 {
        // Try to extract error message from JSON
        if let Ok(err) = serde_json::from_str::<plotweb_common::ApiError>(&text) {
            return Err(ApiError { message: err.error });
        }
        return Err(ApiError {
            message: format!("HTTP {}: {}", status, text),
        });
    }

    serde_json::from_str(&text).map_err(|e| ApiError {
        message: format!("JSON parse error: {} (body: {})", e, &text[..text.len().min(200)]),
    })
}

fn make_init(method: &str) -> RequestInit {
    let init = RequestInit::new();
    init.set_method(method);
    init.set_credentials(RequestCredentials::SameOrigin);
    init
}

pub async fn get<T: DeserializeOwned>(url: &str) -> Result<T, ApiError> {
    let init = make_init("GET");
    let resp = do_fetch(url, &init).await?;
    parse_response(resp).await
}

pub async fn post<B: Serialize, T: DeserializeOwned>(url: &str, body: &B) -> Result<T, ApiError> {
    let init = make_init("POST");
    let json = serde_json::to_string(body).unwrap();
    init.set_body(&JsValue::from_str(&json));
    let resp = do_fetch(url, &init).await?;
    parse_response(resp).await
}

pub async fn put<B: Serialize, T: DeserializeOwned>(url: &str, body: &B) -> Result<T, ApiError> {
    let init = make_init("PUT");
    let json = serde_json::to_string(body).unwrap();
    init.set_body(&JsValue::from_str(&json));
    let resp = do_fetch(url, &init).await?;
    parse_response(resp).await
}

pub async fn delete_req<T: DeserializeOwned>(url: &str) -> Result<T, ApiError> {
    let init = make_init("DELETE");
    let resp = do_fetch(url, &init).await?;
    parse_response(resp).await
}

/// Trigger a browser download of a binary GET endpoint.
///
/// Fetches `url` with session credentials, then saves the response body via a
/// temporary object URL + synthetic `<a download>` click — the download mirror
/// of [`upload_file`]. The saved filename comes from the response's
/// `Content-Disposition`, falling back to `fallback_name`.
pub async fn download_file(url: &str, fallback_name: &str) -> Result<(), ApiError> {
    let init = make_init("GET");
    let resp = do_fetch(url, &init).await?;

    let status = resp.status();
    if status >= 400 {
        // Error responses are JSON; surface the message if we can.
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

    let blob_promise = resp
        .blob()
        .map_err(|e| ApiError { message: format!("{:?}", e) })?;
    let blob_value = JsFuture::from(blob_promise)
        .await
        .map_err(|e| ApiError { message: format!("{:?}", e) })?;
    let blob: web_sys::Blob = blob_value.dyn_into().map_err(|_| ApiError {
        message: "response body is not a Blob".into(),
    })?;

    let object_url = web_sys::Url::create_object_url_with_blob(&blob)
        .map_err(|e| ApiError { message: format!("{:?}", e) })?;

    let document = web_sys::window().unwrap().document().unwrap();
    let anchor: web_sys::HtmlAnchorElement = document
        .create_element("a")
        .map_err(|e| ApiError { message: format!("{:?}", e) })?
        .dyn_into()
        .map_err(|_| ApiError { message: "failed to create anchor".into() })?;
    anchor.set_href(&object_url);
    anchor.set_download(&filename);
    anchor.click();
    web_sys::Url::revoke_object_url(&object_url).ok();

    Ok(())
}

/// Extract the `filename="..."` value from a Content-Disposition header.
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

/// Upload a file via multipart form data. Does NOT set Content-Type header
/// (the browser sets it with the boundary automatically for FormData).
pub async fn upload_file<T: DeserializeOwned>(
    url: &str,
    file: &web_sys::File,
) -> Result<T, ApiError> {
    let form_data = web_sys::FormData::new()
        .map_err(|e| ApiError { message: format!("{:?}", e) })?;
    form_data
        .append_with_blob_and_filename("file", file, &file.name())
        .map_err(|e| ApiError { message: format!("{:?}", e) })?;

    let init = RequestInit::new();
    init.set_method("POST");
    init.set_credentials(RequestCredentials::SameOrigin);
    init.set_body(&form_data);

    let request = Request::new_with_str_and_init(url, &init)
        .map_err(|e| ApiError { message: format!("{:?}", e) })?;
    // Do NOT set Content-Type — browser will set multipart/form-data with boundary

    let window = web_sys::window().unwrap();
    let resp_value = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(|e| ApiError { message: format!("{:?}", e) })?;

    let resp: Response = resp_value.dyn_into().map_err(|_| ApiError {
        message: "response is not a Response".into(),
    })?;

    parse_response(resp).await
}
