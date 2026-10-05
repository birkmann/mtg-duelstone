//! Client → server frames, plus REST payloads.

use serde::{Deserialize, Serialize};

use crate::{CardId, PhaseType, PlayerId};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ClientFrame {
    Click {
        action: ClickAction,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        id: Option<i64>,
    },
    /// Reply to a `prompt` (see PromptBody docs for `value` shapes).
    Answer { id: i64, value: serde_json::Value },
    Settings { stops: StopsPatch },
    /// MTGO-style auto-yield for a stack item's yield key.
    Yield { key: String, auto: bool },
    ClearYields,
    Concede,
    Resync,
    Ping,
    /// Lobby: change your own deck.
    LobbyDeck { deck: String },
    /// Lobby (host only): fill seat `seat` with an AI (`kind: Ai`, optional deck) or reopen it (`Open`, kicks a human).
    LobbySeat {
        seat: i64,
        kind: crate::SeatKind,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        deck: Option<String>,
    },
    /// Lobby (host only): start the game with the filled seats.
    Start {
        #[serde(skip_serializing_if = "Option::is_none", default)]
        seed: Option<i64>,
    },
}

impl ClientFrame {
    pub fn ok() -> Self {
        Self::Click { action: ClickAction::Ok, id: None }
    }
    pub fn cancel() -> Self {
        Self::Click { action: ClickAction::Cancel, id: None }
    }
    pub fn click_card(id: CardId) -> Self {
        Self::Click { action: ClickAction::Card, id: Some(id) }
    }
    pub fn click_player(id: PlayerId) -> Self {
        Self::Click { action: ClickAction::Player, id: Some(id) }
    }
    pub fn answer(id: i64, value: serde_json::Value) -> Self {
        Self::Answer { id, value }
    }
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("ClientFrame serializes")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ClickAction {
    Ok,
    Cancel,
    Pass,
    AlphaStrike,
    Undo,
    Card,
    Player,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StopsPatch {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub my_turn: Option<Vec<PhaseType>>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub opp_turn: Option<Vec<PhaseType>>,
}

// ---- REST ----------------------------------------------------------------------------------

/// Game format (protocol v2 `format` on `POST /api/match` and `GET /api/decks?format=`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Format {
    #[default]
    Constructed,
    Commander,
}

impl Format {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Constructed => "constructed",
            Self::Commander => "commander",
        }
    }
    pub fn starting_life(self) -> i32 {
        match self {
            Self::Constructed => 20,
            Self::Commander => 40,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DeckInfo {
    /// `precon:<name>` / `user:<name>` / `file:<path>` / `starter:<name>`
    #[serde(rename = "ref")]
    pub deck_ref: String,
    pub name: String,
    pub commanders: Vec<String>,
    pub main_count: i32,
    pub color_identity: String,
    pub format: Option<Format>,
    pub problem: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateMatch {
    pub human_deck: String,
    pub ai_deck: String,
    pub format: Format,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub human_name: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MatchInfo {
    pub id: String,
    pub human_deck: String,
    pub ai_deck: String,
    pub format: Option<Format>,
    pub started: bool,
    pub over: bool,
    pub sockets: i32,
    pub turn: Option<i32>,
    pub game_over: Option<bool>,
}

/// `GET /api/health`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ServerInfo {
    pub ok: bool,
    pub protocol: Option<u32>,
    pub cards: Option<i64>,
    /// Base URL other machines can reach the engine at (invite links).
    pub public_url: Option<String>,
    /// The engine serves the browser client (invitees can play without installing anything).
    pub web: bool,
    pub max_players: Option<i64>,
}

/// `POST /api/lobby`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateLobby {
    pub format: Format,
    /// Table size including the host, 2..=4.
    pub players: i64,
    pub name: String,
    pub deck: String,
}

/// `POST /api/lobby/{id}/join`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JoinLobby {
    pub name: String,
    pub deck: String,
}

/// Answer to `POST /api/lobby` (host, seat 0) and `POST /api/lobby/{id}/join`: the seat's credential.
/// Connect with `ws://…/ws/match/{id}?token={token}`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SeatTicket {
    pub id: String,
    pub seat: i64,
    pub token: String,
    pub name: Option<String>,
    pub format: Option<Format>,
    pub invite: Option<String>,
}
