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
        /// v2: display names of the decks (`humanDeck`/`aiDeck` are refs).
        #[serde(default)]
        human_deck_name: Option<String>,
        #[serde(default)]
        ai_deck_name: Option<String>,
        /// v2: "constructed" | "commander"
        #[serde(default)]
        format: Option<String>,
        /// v2: protocol version; absent on v1 servers.
        #[serde(default)]
        protocol: Option<u32>,
        /// Multiplayer: the seat this socket speaks for (0 = host / the classic human seat).
        #[serde(default)]
        seat: Option<i64>,
        /// Multiplayer: number of players at the table (humans + AI).
        #[serde(default)]
        players: Option<i64>,
        /// Every filled seat with its portrait (the engine picks one from the deck for seats
        /// that didn't choose). Players are matched by `name`.
        #[serde(default)]
        seats: Vec<SeatInfo>,
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
    /// Multiplayer: the lobby before the host starts the game (sent on every seat change).
    Lobby(LobbyFrame),
    /// v3 events: the pod's state as this seat sees it.
    Event(crate::EventFrame),
    /// v3 events: a game was dealt — connect to it like a lobby seat (`id` = match id).
    EventMatch(crate::SeatTicket),
    /// v3 events: the engine's deck suggestion for your pool.
    DeckSuggestion { deck: crate::LimitedDeck },
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

/// A multiplayer table before (and, via `GET /api/lobby/{id}`, around) the game. Also the body of
/// `GET /api/lobby/{id}` and part of the host ticket from `POST /api/lobby`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LobbyFrame {
    /// Lobby code = match id (6 letters); also the `?join=` value of the invite link.
    pub id: String,
    pub format: Option<crate::Format>,
    pub max_players: i64,
    /// Seat index of the host (who may change seats and start).
    pub host: i64,
    pub started: bool,
    /// `http://<host lan ip>:<port>/?join=<id>` — absent when the engine has no public URL.
    pub invite: Option<String>,
    /// Seat index of the receiving socket (absent in REST answers).
    pub you: Option<i64>,
    pub seats: Vec<SeatInfo>,
    /// ≥ 2 filled seats and every seated human connected.
    pub can_start: bool,
    /// v3: the table's legality / variants (joiners pick a deck for these)
    #[serde(flatten)]
    pub setup: crate::GameSetup,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SeatInfo {
    pub seat: i64,
    pub kind: SeatKind,
    pub name: Option<String>,
    /// deck ref (`random`, `starter:…`, …)
    pub deck: Option<String>,
    pub deck_name: Option<String>,
    pub connected: bool,
    pub you: bool,
    pub avatar: Option<crate::Avatar>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SeatKind {
    Human,
    Ai,
    #[default]
    Open,
}

#[cfg(test)]
mod lobby_tests {
    use super::*;
    use crate::{ClientFrame, Format};

    #[test]
    fn lobby_frame_from_engine_json() {
        let raw = r#"{"t":"lobby","id":"K7QF2M","format":"commander","maxPlayers":4,"host":0,"started":false,
            "invite":"http://10.0.0.5:8080/?join=K7QF2M","you":1,"canStart":false,
            "seats":[{"seat":0,"kind":"human","name":"Ana","deck":"random","deckName":"Random deck","connected":true,"you":false},
                     {"seat":1,"kind":"human","name":"Bo","deck":"precon:X","deckName":"X","connected":true,"you":true},
                     {"seat":2,"kind":"ai","name":"Forge AI","deck":"random","deckName":"Random deck","connected":true,"you":false},
                     {"seat":3,"kind":"open","name":null,"deck":null,"deckName":null,"connected":false,"you":false}]}"#;
        let ServerFrame::Lobby(l) = ServerFrame::parse(raw).unwrap() else { panic!("not a lobby frame") };
        assert_eq!(l.format, Some(Format::Commander));
        assert_eq!(l.you, Some(1));
        assert_eq!(l.seats.len(), 4);
        assert_eq!(l.seats[2].kind, SeatKind::Ai);
        assert_eq!(l.seats[3].kind, SeatKind::Open);
        assert!(l.seats[1].you);
    }

    #[test]
    fn hello_carries_seat() {
        let f = ServerFrame::parse(r#"{"t":"hello","matchId":"K7QF2M","format":"constructed","protocol":2,"seat":2,"players":4}"#).unwrap();
        assert!(matches!(f, ServerFrame::Hello { seat: Some(2), players: Some(4), .. }));
        let f = ServerFrame::parse(r#"{"t":"hello","seats":[{"seat":0,"kind":"human","name":"Ann","avatar":{"card":"Griselbrand","x":0.3,"y":0.2,"zoom":1.5}},{"seat":1,"kind":"ai","name":"Forge AI"}]}"#).unwrap();
        let ServerFrame::Hello { seats, .. } = f else { panic!() };
        assert_eq!(seats[0].avatar.as_ref().map(|a| (a.card.as_str(), a.zoom)), Some(("Griselbrand", 1.5)));
        assert_eq!(seats[1].avatar, None);
        assert_eq!(
            ClientFrame::LobbyAvatar { avatar: crate::Avatar::of("Griselbrand") }.to_json(),
            r#"{"t":"lobbyAvatar","avatar":{"card":"Griselbrand","x":0.5,"y":0.4,"zoom":1.0}}"#
        );
    }

    #[test]
    fn lobby_commands_serialize_like_the_engine_expects() {
        assert_eq!(ClientFrame::LobbySeat { seat: 3, kind: SeatKind::Ai, deck: None }.to_json(), r#"{"t":"lobbySeat","seat":3,"kind":"ai"}"#);
        assert_eq!(ClientFrame::Start { seed: None }.to_json(), r#"{"t":"start"}"#);
        assert_eq!(ClientFrame::LobbyDeck { deck: "random".into() }.to_json(), r#"{"t":"lobbyDeck","deck":"random"}"#);
    }
}
