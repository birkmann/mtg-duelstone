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
    /// Lobby: change your own portrait.
    LobbyAvatar { avatar: crate::Avatar },
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
    /// Event: take this card from the current pack.
    Pick { id: i64 },
    /// Event: submit (or update) your limited deck.
    SubmitDeck { deck: crate::LimitedDeck },
    /// Event: ask the engine's limited deck builder for a suggestion (answered with `deckSuggestion`).
    SuggestDeck,
    /// Event (host): start drafting / opening (fills open seats with AI drafters).
    StartEvent,
    /// Event (host): fill or reopen a seat.
    EventSeat { seat: i64, kind: crate::SeatKind },
    /// Event (host): deal games from the submitted decks.
    DealGames {
        pairing: crate::Pairing,
        /// players per table for `table` pairing (2..=4)
        table_size: i64,
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
    /// 60-card singleton with a commander, Standard card pool (v3).
    Brawl,
    /// 60-card singleton led by a planeswalker plus its signature spell (v3).
    Oathbreaker,
    /// 40-card decks built from a draft/sealed pool; only reachable through an event (v3).
    Limited,
}

impl Format {
    /// Formats a player picks a deck for (Limited decks come from events).
    pub const PICKABLE: [Format; 4] = [Format::Constructed, Format::Commander, Format::Brawl, Format::Oathbreaker];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Constructed => "constructed",
            Self::Commander => "commander",
            Self::Brawl => "brawl",
            Self::Oathbreaker => "oathbreaker",
            Self::Limited => "limited",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "constructed" => Self::Constructed,
            "commander" => Self::Commander,
            "brawl" => Self::Brawl,
            "oathbreaker" => Self::Oathbreaker,
            "limited" => Self::Limited,
            _ => return None,
        })
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Constructed => "Constructed",
            Self::Commander => "Commander",
            Self::Brawl => "Brawl",
            Self::Oathbreaker => "Oathbreaker",
            Self::Limited => "Limited",
        }
    }
    pub fn starting_life(self) -> i32 {
        match self {
            Self::Commander => 40,
            Self::Brawl => 25,
            Self::Constructed | Self::Oathbreaker | Self::Limited => 20,
        }
    }
    /// Has a command zone with commanders (or an oathbreaker + signature spell).
    pub fn has_commander(self) -> bool {
        matches!(self, Self::Commander | Self::Brawl | Self::Oathbreaker)
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
    /// Card whose art stands for the deck (engine's pick; the client may override it per deck).
    pub cover: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateMatch {
    pub human_deck: String,
    pub ai_deck: String,
    pub format: Format,
    /// v3: legality / variants
    #[serde(flatten)]
    pub setup: crate::GameSetup,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub human_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub human_avatar: Option<crate::Avatar>,
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
    /// v3: legality / variants
    #[serde(flatten)]
    pub setup: crate::GameSetup,
    /// Table size including the host, 2..=4.
    pub players: i64,
    pub name: String,
    pub deck: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub avatar: Option<crate::Avatar>,
}

/// `POST /api/lobby/{id}/join`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JoinLobby {
    pub name: String,
    pub deck: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub avatar: Option<crate::Avatar>,
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
