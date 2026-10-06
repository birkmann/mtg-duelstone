//! The one HTTP primitive [`crate::Api`] is built on, implemented per platform.
//!
//! Why a tiny hand-rolled layer: natively we already have `ureq` (blocking, run on a worker thread by
//! [`crate::spawn`]); in the browser the only transport is `fetch`, which is async and must run on the
//! page's event loop. Both sit behind the same `async fn request`, so every typed endpoint is written once.

use std::time::Duration;

use anyhow::{Result, anyhow};

pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

impl HttpResponse {
    /// `Ok(self)` for 2xx; otherwise an error carrying the engine's `{error}` text when there is one.
    pub fn ok(self, what: &str) -> Result<Self> {
        if (200..300).contains(&self.status) {
            return Ok(self);
        }
        let body = String::from_utf8_lossy(&self.body).into_owned();
        let msg = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| v.get("error").and_then(|e| e.as_str().map(str::to_owned)))
            .unwrap_or(body);
        Err(anyhow!("{what}: HTTP {}: {}", self.status, msg.chars().take(300).collect::<String>()))
    }
    pub fn json<T: serde::de::DeserializeOwned>(&self, what: &str) -> Result<T> {
        serde_json::from_slice(&self.body).map_err(|e| anyhow!("{what}: bad JSON: {e}"))
    }
}

/// Percent-encode a query string (`application/x-www-form-urlencoded` rules).
pub fn query(pairs: &[(&str, &str)]) -> String {
    fn enc(s: &str, out: &mut String) {
        for b in s.bytes() {
            match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
                b' ' => out.push('+'),
                _ => out.push_str(&format!("%{b:02X}")),
            }
        }
    }
    let mut out = String::new();
    for (i, (k, v)) in pairs.iter().enumerate() {
        out.push(if i == 0 { '?' } else { '&' });
        enc(k, &mut out);
        out.push('=');
        enc(v, &mut out);
    }
    out
}

/// `method` is GET / POST / PUT / DELETE; `json_body` is sent as `application/json`.
#[cfg(not(target_arch = "wasm32"))]
pub async fn request(method: &str, url: &str, json_body: Option<String>, timeout: Duration) -> Result<HttpResponse> {
    use ureq::Agent;
    // Non-2xx statuses are inspected by the caller so errors can carry the engine's `{error}` text.
    let agent: Agent = Agent::config_builder().timeout_global(Some(timeout)).http_status_as_error(false).build().into();
    let resp = match (method, json_body) {
        ("GET", _) => agent.get(url).call(),
        ("DELETE", _) => agent.delete(url).call(),
        ("POST", body) => agent.post(url).header("Content-Type", "application/json").send(body.unwrap_or_default()),
        ("PUT", body) => agent.put(url).header("Content-Type", "application/json").send(body.unwrap_or_default()),
        (m, _) => return Err(anyhow!("unsupported method {m}")),
    };
    let mut resp = resp.map_err(|e| anyhow!("{method} {url}: {e}"))?;
    let status = resp.status().as_u16();
    let body = resp.body_mut().with_config().limit(64 * 1024 * 1024).read_to_vec().map_err(|e| anyhow!("read body: {e}"))?;
    Ok(HttpResponse { status, body })
}

#[cfg(target_arch = "wasm32")]
pub async fn request(method: &str, url: &str, json_body: Option<String>, timeout: Duration) -> Result<HttpResponse> {
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;
    use web_sys::{Request, RequestInit, RequestMode, Response};

    let js = |e: wasm_bindgen::JsValue| anyhow!("{method} {url}: {}", e.as_string().unwrap_or_else(|| format!("{e:?}")));
    let init = RequestInit::new();
    init.set_method(method);
    init.set_mode(RequestMode::Cors);
    if let Some(b) = json_body {
        init.set_body(&wasm_bindgen::JsValue::from_str(&b));
    }
    init.set_signal(Some(&web_sys::AbortSignal::timeout_with_u32(timeout.as_millis().min(u32::MAX as u128) as u32)));
    let req = Request::new_with_str_and_init(url, &init).map_err(js)?;
    if method == "POST" || method == "PUT" {
        req.headers().set("Content-Type", "application/json").map_err(js)?;
    }
    let window = web_sys::window().ok_or_else(|| anyhow!("no window"))?;
    let resp: Response = JsFuture::from(window.fetch_with_request(&req)).await.map_err(js)?.dyn_into().map_err(js)?;
    let status = resp.status();
    let buf = JsFuture::from(resp.array_buffer().map_err(js)?).await.map_err(js)?;
    Ok(HttpResponse { status, body: js_sys::Uint8Array::new(&buf).to_vec() })
}
