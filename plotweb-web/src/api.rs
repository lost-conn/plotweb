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
