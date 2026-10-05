//! Engine transport — no Bevy. Used by the `arena` app (via its `net` plugin), the headless
//! `autopilot` soak bot, and tests. Builds for desktop and for the browser (wasm32).
//!
//! CONTRACT (the arena app codes against exactly these signatures; extend, don't break):
//! - [`EngineLink::connect`] (both platforms) / `EngineLink::replay` (native) → poll `rx` each frame,
//!   push commands with [`EngineLink::send`].
//! - [`Api`] — async REST for the game; run calls with [`spawn`] (thread natively, event loop in the browser).
//! - `Rest` — blocking REST (native tools only: autopilot, launcher).
//! - [`autoplay::AutoPolicy`] — decide a reply to an input/prompt (autopilot bin + arena `--autoplay`).
//! - `launcher::ensure_engine` — start the Java engine if `/api/health` doesn't answer (native).
//! - `scryfall::Showcase` — curated card artworks + artist facts for the splash screens (native, cached).

pub mod api;
pub mod autoplay;
pub mod curated;
pub mod http;
pub mod task;

#[cfg(not(target_arch = "wasm32"))]
pub mod launcher;
#[cfg(not(target_arch = "wasm32"))]
pub mod record;
#[cfg(not(target_arch = "wasm32"))]
pub mod rest;
#[cfg(not(target_arch = "wasm32"))]
pub mod scryfall;
#[cfg(not(target_arch = "wasm32"))]
pub mod ws;
#[cfg(target_arch = "wasm32")]
#[path = "ws_web.rs"]
pub mod ws;

pub use api::{Api, ImageRequest, ImageVersion, parse_invite};
#[cfg(not(target_arch = "wasm32"))]
pub use launcher::{EngineProcess, LaunchConfig, ensure_engine};
#[cfg(not(target_arch = "wasm32"))]
pub use rest::Rest;
pub use task::{now_millis, spawn};
pub use ws::{EngineLink, LinkEvent};
