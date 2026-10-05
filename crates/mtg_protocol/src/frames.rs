//! Server → client frames: `{"t": <type>, ...}`.

use serde::{Deserialize, Serialize};

use crate::{Card, CardId, Game, PlayerId, PromptFrame, Stops};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ServerFrame {
    Hello {
        #[serde(default)]
        match_id: String,
        #[serde(default)]
        human_deck: String,
        #[serde(default)]
        ai_deck: String,
        /// v2: "constructed" | "commander"
        #[serde(default)]
        format: Option<String>,
        /// v2: protocol version; absent on v1 servers.
        #[serde(default)]
        protocol: Option<u32>,
    },
    State(StateFrame),
    Input(InputFrame),
    Prompt(PromptFrame),
    Log {
        #[serde(default)]
        entries: Vec<LogEntry>,
    },
    Message {
        #[serde(default)]
        title: String,
        #[serde(default)]
        text: String,
        #[serde(default)]
        error: bool,
    },
    GameOver {
        #[serde(default)]
        winner: Option<String>,
        #[serde(default)]
        you_won: bool,
    },
    /// v2: animation cues from Forge's game event bus.
    Events {
        #[serde(default)]
        seq: i64,
        #[serde(default)]
        items: Vec<EventCue>,
    },
    /// The engine rejected the last action.
    Flash,
    /// Your attention is needed.
    Alert,
    Pong,
    Error {
        #[serde(default)]
        text: String,
    },
    #[serde(other)]
    Unknown,
}

impl ServerFrame {
    pub fn parse(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct StateFrame {
    pub seq: i64,
    /// The viewing seat's player id.
    pub you: Option<PlayerId>,
    pub stops: Stops,
    pub auto_yields: Vec<String>,
    pub game: Game,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct InputFrame {
    /// Changes whenever a NEW engine Input becomes active (same content ≠ same prompt!).
    pub seq: i64,
    pub message: String,
    /// Card the input is about (e.g. targeting source).
    pub card: Option<CardId>,
    pub card_view: Option<Card>,
    pub ok: ButtonState,
    pub cancel: ButtonState,
    pub selectable: Vec<CardId>,
    pub highlighted: Vec<CardId>,
    /// e.g. lands "Auto" would tap.
    pub weak: Vec<CardId>,
    pub select_min: i64,
    pub select_max: i64,
    pub selectable_players: Vec<PlayerId>,
    /// Selectable cards the table has nowhere to draw (library digs, graveyard picks, …).
    pub tray_cards: Vec<Card>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ButtonState {
    pub label: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LogEntry {
    pub i: i64,
    #[serde(rename = "type")]
    pub kind: String,
    pub text: String,
}

/// One animation cue (protocol v2 `events` frame). `kind` is one of:
/// `damage` (source → targetCard/targetPlayer, amount), `lifeChanged` (targetPlayer, amount = delta),
/// `zoneChange` (card, zoneFrom, zoneTo), `spellCast` (card, source = activator's card), `attack`
/// (card → targetPlayer/targetCard), `block` (card → targetCard), `counters` (card, amount, label),
/// `tap` / `untap` (card), `tokenCreated` (card), `destroyed` / `sacrificed` (card), `turnBegan` (targetPlayer).
/// Unknown kinds must be ignored.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct EventCue {
    pub kind: String,
    pub card: Option<CardId>,
    pub source: Option<CardId>,
    pub target_card: Option<CardId>,
    pub target_player: Option<PlayerId>,
    pub amount: Option<i64>,
    pub zone_from: Option<String>,
    pub zone_to: Option<String>,
    pub label: Option<String>,
}
