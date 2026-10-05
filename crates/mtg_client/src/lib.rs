//! Engine transport — no Bevy. Used by the `arena` app (via its `net` plugin), the headless
//! `autopilot` soak bot, and tests.
//!
//! CONTRACT (the arena app codes against exactly these signatures; extend, don't break):
//! - [`EngineLink::connect`] / [`EngineLink::replay`] → a thread-backed link; poll `rx` each frame,
//!   push commands with [`EngineLink::send`].
//! - [`Rest`] — blocking REST helpers (call from a task pool / thread, never the main schedule).
//! - [`autoplay::AutoPolicy`] — decide a reply to an input/prompt (autopilot bin + arena `--autoplay`).
//! - [`launcher::ensure_engine`] — start the Java engine if `/api/health` doesn't answer.
//! - [`deckstats`] — pure deck analysis for the builder (curve, colours, roles, odds, goldfish).
//! - [`scryfall::Showcase`] — curated card artworks + artist facts for the splash screens (cached).

pub mod autoplay;
pub mod deckstats;
pub mod launcher;
pub mod record;
pub mod rest;
pub mod scryfall;
pub mod ws;

pub use launcher::{EngineProcess, LaunchConfig, ensure_engine};
pub use rest::{ImageRequest, ImageVersion, Rest};
pub use ws::{EngineLink, LinkEvent};
