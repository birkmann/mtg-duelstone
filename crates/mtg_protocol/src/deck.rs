//! Deck-builder REST payloads: catalog cards (`/api/cards/*`), full decks (`GET /api/deck`) and
//! recommendations (`POST /api/deck/recommend`).
//!
//! Why here and not in `mtg_client`: these are wire shapes documented in `engine/docs/protocol.md`,
//! like the frames — the builder UI and tests decode the same structs the engine emits.

use serde::{Deserialize, Serialize};

use crate::Format;

/// One card from Forge's card database (`CardCatalog.cardJson`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CatalogCard {
    pub name: String,
    /// `{2}{G}{U}` style; None for lands / no cost.
    pub mana_cost: Option<String>,
    pub cmc: i32,
    /// Full type line, e.g. `Legendary Creature - Phyrexian Angel Horror`.
    #[serde(rename = "type")]
    pub type_line: String,
    /// `WUBRG` letters, empty = colourless.
    pub colors: String,
    pub color_identity: String,
    pub oracle_text: Option<String>,
    pub power: Option<String>,
    pub toughness: Option<String>,
    pub loyalty: Option<String>,
    pub set: Option<String>,
    pub cn: Option<String>,
    pub rarity: Option<String>,
    pub is_land: bool,
    pub is_creature: bool,
    pub is_basic_land: bool,
    pub can_be_commander: bool,
    pub banned: bool,
    /// Role (`ramp`, `draw`, `removal`, `wipe`, `counter`, `tutor`, `protection`) and theme
    /// (`counters`, `tokens`, `infect`, …) tags derived by the engine from oracle text.
    pub tags: Vec<String>,
    pub subtypes: Vec<String>,
    /// 0..1, how often the card is played across the engine's deck data.
    pub popularity: f32,
}

impl CatalogCard {
    pub fn has_tag(&self, tag: &str) -> bool {
        self.tags.iter().any(|t| t == tag)
    }
    /// Primary card type for grouping a decklist, in the usual deck-list order.
    pub fn primary_type(&self) -> CardKind {
        let t = self.type_line.to_ascii_lowercase();
        // Land first: "Artifact Land" plays like a land; creature before artifact for artifact creatures.
        if self.is_land || (t.contains("land") && !t.contains("creature")) {
            CardKind::Land
        } else if self.is_creature || t.contains("creature") {
            CardKind::Creature
        } else if t.contains("planeswalker") {
            CardKind::Planeswalker
        } else if t.contains("battle") {
            CardKind::Battle
        } else if t.contains("instant") {
            CardKind::Instant
        } else if t.contains("sorcery") {
            CardKind::Sorcery
        } else if t.contains("artifact") {
            CardKind::Artifact
        } else if t.contains("enchantment") {
            CardKind::Enchantment
        } else {
            CardKind::Other
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CardKind {
    Creature,
    Planeswalker,
    Instant,
    Sorcery,
    Artifact,
    Enchantment,
    Battle,
    Other,
    Land,
}

impl CardKind {
    pub const ALL: [CardKind; 9] = [
        CardKind::Creature,
        CardKind::Planeswalker,
        CardKind::Instant,
        CardKind::Sorcery,
        CardKind::Artifact,
        CardKind::Enchantment,
        CardKind::Battle,
        CardKind::Other,
        CardKind::Land,
    ];
    pub fn label(self) -> &'static str {
        match self {
            CardKind::Creature => "Creatures",
            CardKind::Planeswalker => "Planeswalkers",
            CardKind::Instant => "Instants",
            CardKind::Sorcery => "Sorceries",
            CardKind::Artifact => "Artifacts",
            CardKind::Enchantment => "Enchantments",
            CardKind::Battle => "Battles",
            CardKind::Other => "Other",
            CardKind::Land => "Lands",
        }
    }
}

/// `{card, qty}` as returned in `GET /api/deck` sections.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DeckCardEntry {
    pub card: CatalogCard,
    pub qty: u32,
}

/// `GET /api/deck?ref=` — a full deck with card details.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DeckFull {
    #[serde(rename = "ref")]
    pub deck_ref: String,
    pub name: String,
    pub format: Option<Format>,
    pub problem: Option<String>,
    pub commanders: Vec<CatalogCard>,
    pub main: Vec<DeckCardEntry>,
    pub sideboard: Vec<DeckCardEntry>,
}

/// `{name, qty}` line of a deck body sent to the engine (save / validate / recommend).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DeckLine {
    pub name: String,
    pub qty: u32,
}

/// Body of `POST /api/deck` / `POST /api/deck/validate`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeckBody {
    pub name: String,
    pub format: Format,
    pub commanders: Vec<String>,
    pub main: Vec<DeckLine>,
    pub sideboard: Vec<DeckLine>,
    /// Rename: the deck's old name, so the engine replaces it instead of keeping both.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_name: Option<String>,
}

/// Body of `POST /api/deck/recommend`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecommendRequest {
    pub format: Format,
    pub commanders: Vec<String>,
    pub main: Vec<DeckLine>,
    /// Theme keys the user pinned (`counters`, `tribal:Elf`); empty = let the engine pick.
    pub themes: Vec<String>,
    /// Colour filter override (constructed); None = derived from the deck.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub colors: Option<String>,
    pub limit: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Recommendations {
    pub format: Option<Format>,
    pub color_identity: String,
    /// Commander only: decks behind the data for this commander (0 = commander not in the data).
    pub commander_decks: u32,
    pub themes: Vec<ThemeInfo>,
    pub archetypes: Vec<ArchetypeMatch>,
    pub sections: Vec<RecSection>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeInfo {
    pub key: String,
    pub label: String,
    /// Cards in the current deck carrying the theme.
    pub count: u32,
    pub score: f32,
    pub active: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ArchetypeMatch {
    pub name: String,
    pub format: String,
    pub similarity: f32,
    pub decks: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RecSection {
    /// `synergy`, `top`, `theme:<tag>`, `tribal:<Type>`, `role:<tag>`, `lands`
    pub key: String,
    pub title: String,
    pub cards: Vec<RecCard>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RecCard {
    pub card: CatalogCard,
    /// 0..1 share of decks (with this commander / archetype) that run the card.
    pub inclusion: f32,
    /// inclusion minus the card's base rate (EDHREC's "synergy"), -1..1.
    pub synergy: f32,
    pub score: f32,
    pub reason: String,
}

/// Query for `GET /api/cards/search`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CardQuery {
    pub q: String,
    /// Colour identity the card must fit in (`WUBG`), None = any.
    pub ci: Option<String>,
    pub type_line: Option<String>,
    pub text: Option<String>,
    /// Mana value; 7 = 7+.
    pub cmc: Option<i32>,
    pub tag: Option<String>,
    pub commander_only: bool,
    /// `name` | `popular`
    pub sort: Option<String>,
    pub offset: u32,
    pub limit: u32,
}
