//! Limited events (cube draft, set draft, sealed), cubes, and the richer game setup (format legality,
//! Brawl/Oathbreaker, casual variants) — protocol v3 additions, documented in `engine/docs/protocol.md`.
//!
//! Why a separate module: an event is its own WebSocket session (`/ws/event/{id}`) with its own frame
//! (`event`) — the pod drafts and builds decks there, and only when the host deals games does each human
//! get a seat ticket for an ordinary match socket (`eventMatch`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{Format, SeatKind};

// ---- game setup ------------------------------------------------------------------------------

/// Constructed legality (`format: constructed` only); None = casual (anything Forge can play).
pub const LEGALITIES: &[(&str, &str)] = &[
    ("standard", "Standard"),
    ("pioneer", "Pioneer"),
    ("modern", "Modern"),
    ("legacy", "Legacy"),
    ("vintage", "Vintage"),
    ("pauper", "Pauper"),
];

/// Casual variants layered on a match (`variants: [...]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Variant {
    /// Planar decks (auto-generated) and the planar die.
    Planechase,
    /// One player (seat 0) is the archenemy with a scheme deck, everyone else plays as a team.
    Archenemy,
    /// Decks are 60 basic lands; discard a card to make a random creature of that mana value.
    Momir,
    /// Four players in two teams of two (seats 0+2 vs 1+3): a team wins together. Forge has no
    /// shared-life Two-Headed Giant rules, so teammates keep their own life totals and turns.
    Teams,
}

impl Variant {
    pub const ALL: [Variant; 4] = [Variant::Planechase, Variant::Archenemy, Variant::Momir, Variant::Teams];
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Planechase => "planechase",
            Self::Archenemy => "archenemy",
            Self::Momir => "momir",
            Self::Teams => "teams",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Planechase => "Planechase",
            Self::Archenemy => "Archenemy",
            Self::Momir => "Momir Basic",
            Self::Teams => "2v2 Teams",
        }
    }
    /// `planechase` / `archenemy` / `momir` / `teams` (case-insensitive).
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.as_str().eq_ignore_ascii_case(s.trim()))
    }
    /// Momir decks are generated: the player's deck choice is irrelevant.
    pub fn ignores_decks(self) -> bool {
        matches!(self, Self::Momir)
    }
}

/// What kind of game to set up — sent with `POST /api/match` and `POST /api/lobby` (flattened).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct GameSetup {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub legality: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub variants: Vec<Variant>,
}

/// `GET /api/formats`: everything the engine can set up.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct FormatsInfo {
    pub formats: Vec<NamedId>,
    pub legalities: Vec<NamedId>,
    pub variants: Vec<NamedId>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct NamedId {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
}

// ---- cubes -----------------------------------------------------------------------------------

/// `GET /api/cubes` row.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CubeInfo {
    /// `bundled:<name>` (Forge's res/cube) or `user:<name>` (~/.forge/cube)
    #[serde(rename = "ref")]
    pub cube_ref: String,
    pub name: String,
    pub size: i64,
    pub user: bool,
}

/// One printing in a cube or a search result (`CardCatalog.cardJson`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CubeCard {
    pub name: String,
    pub mana_cost: Option<String>,
    pub cmc: i32,
    #[serde(rename = "type")]
    pub type_line: String,
    /// `WUBRG` letters, empty = colourless
    pub colors: String,
    pub color_identity: String,
    pub oracle_text: Option<String>,
    pub set: Option<String>,
    pub rarity: Option<String>,
    pub is_land: bool,
    pub is_creature: bool,
    pub power: Option<String>,
    pub toughness: Option<String>,
    /// copies (cubes are usually singleton)
    pub qty: i32,
}

/// `GET /api/cube?ref=` and the body of `POST /api/cube`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Cube {
    #[serde(rename = "ref")]
    pub cube_ref: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub cards: Vec<CubeCard>,
    /// pack layout used when drafting it
    pub packs: i32,
    pub pack_size: i32,
    /// set when saving under a new name (rename)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_name: Option<String>,
}

/// `GET /api/sets?draftable=1` row (set drafts / set sealed).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SetInfo {
    pub code: String,
    pub name: String,
    pub date: Option<String>,
}

// ---- events ----------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EventKind {
    #[default]
    CubeDraft,
    SetDraft,
    /// Sealed from a cube (`source` = cube ref) or a set (`source` = set code).
    Sealed,
}

impl EventKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::CubeDraft => "Cube Draft",
            Self::SetDraft => "Set Draft",
            Self::Sealed => "Sealed",
        }
    }
}

/// `POST /api/event` — opens a pod; the answer is the host's [`crate::SeatTicket`] (seat 0).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateEvent {
    pub kind: EventKind,
    /// cube ref (`bundled:…` / `user:…`) or set code
    pub source: String,
    /// drafters in the pod including AI, 2..=8
    pub pod_size: i64,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub avatar: Option<crate::Avatar>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EventPhase {
    /// Waiting for players; the host fills seats and starts.
    #[default]
    Lobby,
    Drafting,
    /// Everyone has a pool; build a 40-card deck and submit it.
    Building,
    /// Decks are in: the host deals games (and can deal again after they end).
    Ready,
}

/// A card in a pack or a pool. `id` is unique within the event (picks refer to it).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DraftCard {
    pub id: i64,
    #[serde(flatten)]
    pub card: CubeCard,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct EventSeat {
    pub seat: i64,
    pub kind: SeatKind,
    pub name: Option<String>,
    pub avatar: Option<crate::Avatar>,
    pub connected: bool,
    pub you: bool,
    /// cards drafted so far (or pool size in sealed)
    pub picked: i64,
    /// waiting on this seat's pick in the current round
    pub thinking: bool,
    pub deck_ready: bool,
}

/// A submitted limited deck: pool card ids in the main deck plus basic lands by colour letter.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LimitedDeck {
    pub main: Vec<i64>,
    /// "W"/"U"/"B"/"R"/"G" → count
    pub basics: BTreeMap<String, i32>,
}

impl LimitedDeck {
    pub fn size(&self) -> i32 {
        self.main.len() as i32 + self.basics.values().sum::<i32>()
    }
}

/// The `event` frame: the whole pod as this seat sees it (sent on every change).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct EventFrame {
    pub id: String,
    pub kind: EventKind,
    pub source: String,
    pub source_name: String,
    pub phase: EventPhase,
    pub host: i64,
    pub you: Option<i64>,
    pub invite: Option<String>,
    pub seats: Vec<EventSeat>,
    /// 1-based pack round and pick within it (drafting)
    pub round: i64,
    pub rounds: i64,
    pub pick: i64,
    /// passing direction this round: true = left
    pub pass_left: bool,
    /// cards to pick from right now (empty while waiting for the next pack)
    pub pack: Vec<DraftCard>,
    pub pool: Vec<DraftCard>,
    /// your submitted deck (building / ready)
    pub deck: Option<LimitedDeck>,
    pub can_start: bool,
    /// games currently running from this event
    pub games: Vec<EventGame>,
    pub min_deck: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct EventGame {
    pub match_id: String,
    pub players: Vec<String>,
    pub over: bool,
    pub winner: Option<String>,
}

/// How the host deals games once decks are in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Pairing {
    /// Every human (plus AI drafters up to `tableSize`) at one free-for-all table.
    #[default]
    Table,
    /// One-on-one games: humans paired with each other, the odd one out against an AI drafter.
    Duels,
}

pub fn format_for_event() -> Format {
    Format::Limited
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ClientFrame, ServerFrame};

    #[test]
    fn event_frame_from_engine_json() {
        let raw = r#"{"t":"event","id":"2UXJFG","kind":"cubeDraft","source":"bundled:MTGO Legacy Cube","sourceName":"MTGO Legacy Cube",
            "phase":"drafting","host":0,"you":0,"invite":null,"minDeck":40,"round":1,"rounds":3,"passLeft":true,"pick":2,"canStart":false,
            "seats":[{"seat":0,"kind":"human","name":"Ana","connected":true,"you":true,"picked":1,"thinking":true,"deckReady":false},
                     {"seat":1,"kind":"ai","name":"AI Drafter 2","connected":true,"you":false,"picked":2,"thinking":false,"deckReady":false}],
            "pack":[{"name":"Lightning Bolt","manaCost":"{R}","cmc":1,"type":"Instant","colors":"R","colorIdentity":"R","set":"LEA","isLand":false,"isCreature":false,"id":3001,"qty":1}],
            "pool":[],"games":[]}"#;
        let ServerFrame::Event(f) = ServerFrame::parse(raw).unwrap() else { panic!("not an event frame") };
        assert_eq!(f.kind, EventKind::CubeDraft);
        assert_eq!(f.phase, EventPhase::Drafting);
        assert_eq!(f.pack[0].id, 3001);
        assert_eq!(f.pack[0].card.name, "Lightning Bolt");
        assert_eq!(f.pack[0].card.type_line, "Instant");
        assert!(f.seats[0].thinking && f.pass_left);
    }

    #[test]
    fn event_commands_and_setup_serialize_like_the_engine_expects() {
        assert_eq!(ClientFrame::Pick { id: 7 }.to_json(), r#"{"t":"pick","id":7}"#);
        let d = LimitedDeck { main: vec![1, 2], basics: [("R".to_string(), 17)].into_iter().collect() };
        assert_eq!(d.size(), 19);
        assert_eq!(ClientFrame::SubmitDeck { deck: d }.to_json(), r#"{"t":"submitDeck","deck":{"main":[1,2],"basics":{"R":17}}}"#);
        assert_eq!(ClientFrame::DealGames { pairing: Pairing::Duels, table_size: 2 }.to_json(), r#"{"t":"dealGames","pairing":"duels","tableSize":2}"#);
        let m = crate::CreateMatch {
            format: crate::Format::Constructed,
            setup: GameSetup { legality: Some("pauper".into()), variants: vec![Variant::Planechase] },
            ..Default::default()
        };
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(v["legality"], "pauper");
        assert_eq!(v["variants"][0], "planechase");
        assert_eq!(crate::Format::parse("Oathbreaker"), Some(crate::Format::Oathbreaker));
    }
}
