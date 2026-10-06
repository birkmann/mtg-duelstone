//! Serde mirror of the engine WebSocket protocol (`engine/docs/protocol.md`).
//!
//! The engine (Forge behind `forge-ws-server`) is the single source of truth: it decides legality,
//! the client only renders `state`, highlights `input.selectable`, and answers `prompt`s by index.
//! Every struct is `#[serde(default)]`-tolerant because the server omits optional fields, and both
//! frame enums have an `Unknown` fallback so a newer server never crashes an older client.

mod avatar;
mod card;
mod cmd;
mod deck;
mod event;
mod frames;
mod game;
mod prompt;

pub use avatar::*;
pub use card::*;
pub use cmd::*;
pub use deck::*;
pub use event::*;
pub use frames::*;
pub use game::*;
pub use prompt::*;

/// Engine trackable id of a card — stable for the whole game.
pub type CardId = i64;
/// Engine trackable id of a player.
pub type PlayerId = i64;

/// Wire protocol version this crate implements (`hello.protocol`; servers without the field are v1).
pub const PROTOCOL_VERSION: u32 = 2;
