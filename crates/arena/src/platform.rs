//! The few things that differ between the desktop app and the browser build, behind one small API.
//!
//! Why a module and not `cfg`s sprinkled around: in the browser there is no command line, no
//! filesystem and no engine to launch — the page *is* served by the engine. Everything that needs to
//! know that asks here: where the engine is (the page origin), what the user asked for (URL query
//! instead of argv), and where settings live (localStorage instead of `~/.config`).

/// True in the browser build.
pub const WEB: bool = cfg!(target_arch = "wasm32");

/// `http(s)://host:port` of the page (the engine that served us); None on desktop.
pub fn page_origin() -> Option<String> {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window().and_then(|w| w.location().origin().ok())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        None
    }
}

/// `?name=value` from the page URL (desktop: always None).
pub fn query_param(name: &str) -> Option<String> {
    #[cfg(target_arch = "wasm32")]
    {
        let search = web_sys::window()?.location().search().ok()?;
        web_sys::UrlSearchParams::new_with_str(&search).ok()?.get(name)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = name;
        None
    }
}

/// Drop the query string from the address bar (so a reload doesn't re-join a finished table).
pub fn clear_query() {
    #[cfg(target_arch = "wasm32")]
    if let Some(w) = web_sys::window()
        && let Ok(h) = w.history()
        && let Ok(path) = w.location().pathname()
    {
        let _ = h.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(&path));
    }
}

/// Persistent key/value storage: localStorage in the browser, unused on desktop (settings.rs owns files).
pub fn storage_get(key: &str) -> Option<String> {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window()?.local_storage().ok()??.get_item(key).ok()?
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = key;
        None
    }
}

pub fn storage_set(key: &str, value: &str) -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window()
            .and_then(|w| w.local_storage().ok().flatten())
            .is_some_and(|s| s.set_item(key, value).is_ok())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = (key, value);
        false
    }
}

/// Whether `Clipboard::set_text` can reach the OS clipboard. Browsers only expose
/// `navigator.clipboard` to secure contexts (https / localhost) — an invite page opened over
/// plain-http LAN has none, and touching it there would throw.
pub fn clipboard_usable() -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window().is_some_and(|w| w.is_secure_context())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        true
    }
}
