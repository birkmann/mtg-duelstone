//! Card views. Hidden cards (opponent hand, libraries, face-down) arrive as `{id, zone, owner,
//! controller, hidden: true, faceDown}` — a card back with an id; everything else is optional.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{CardId, PlayerId};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Card {
    pub id: CardId,
    /// Forge `ZoneType` name: `Battlefield`, `Hand`, `Library`, `Graveyard`, `Exile`, `Stack`, `Command`, …
    pub zone: Option<String>,
    pub owner: Option<PlayerId>,
    pub controller: Option<PlayerId>,
    pub hidden: bool,
    pub face_down: bool,

    // ---- visible-only fields ----
    pub name: String,
    pub oracle_name: String,
    pub tapped: bool,
    pub sick: bool,
    pub attacking: bool,
    pub blocking: bool,
    pub flipped: bool,
    pub phased_out: bool,
    pub token: bool,
    pub commander: bool,
    pub damage: i32,
    pub assigned_damage: i32,
    pub shields: i32,
    pub counters: BTreeMap<String, i32>,
    pub attached_to: Option<CardId>,
    pub attached_cards: Vec<CardId>,
    pub chosen_type: Option<String>,
    pub chosen_colors: Option<Vec<String>>,
    pub named_card: Option<Vec<String>>,
    pub text: Option<String>,
    pub state: Option<CardState>,
    pub alt_state: Option<CardState>,
}

impl Card {
    pub fn is_visible(&self) -> bool {
        !self.hidden && self.state.is_some()
    }
    pub fn zone_kind(&self) -> ZoneKind {
        self.zone.as_deref().map(ZoneKind::from_wire).unwrap_or(ZoneKind::None)
    }
    /// Display name: current face name, falling back to the card name.
    pub fn display_name(&self) -> &str {
        self.state.as_ref().map(|s| s.name.as_str()).filter(|s| !s.is_empty()).unwrap_or(&self.name)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CardState {
    pub name: String,
    pub state_name: String,
    /// Full type line, e.g. "Legendary Creature — Elf Druid".
    #[serde(rename = "type")]
    pub type_line: Option<String>,
    /// e.g. "{2}{G}"
    pub mana_cost: Option<String>,
    /// Color letters, e.g. "WG"; empty = colorless.
    pub colors: String,
    pub oracle_text: Option<String>,
    pub rules_text: Option<String>,
    pub set_code: Option<String>,
    pub image_key: Option<String>,
    /// Creatures, Vehicles, and any card with a printed P/T (a God below devotion has one but
    /// `is_creature == false`).
    pub power: Option<i32>,
    pub toughness: Option<i32>,
    pub loyalty: Option<String>,
    pub defense: Option<String>,
    pub is_land: bool,
    pub is_creature: bool,
    pub is_artifact: bool,
    pub is_enchantment: bool,
    pub is_planeswalker: bool,
    pub is_instant: bool,
    pub is_sorcery: bool,
    pub keywords: Vec<String>,
}

/// Zones the client lays out. Parsed from the Forge `ZoneType` name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize)]
pub enum ZoneKind {
    Battlefield,
    Hand,
    Library,
    Graveyard,
    Exile,
    Stack,
    Command,
    #[default]
    None,
}

impl ZoneKind {
    pub fn from_wire(s: &str) -> Self {
        match s {
            "Battlefield" => Self::Battlefield,
            "Hand" => Self::Hand,
            "Library" => Self::Library,
            "Graveyard" => Self::Graveyard,
            "Exile" => Self::Exile,
            "Stack" => Self::Stack,
            "Command" => Self::Command,
            _ => Self::None,
        }
    }
}
