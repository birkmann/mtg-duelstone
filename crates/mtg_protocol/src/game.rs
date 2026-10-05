//! Game view carried by `state` frames, redacted for the viewing seat.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{Card, CardId, PlayerId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PhaseType {
    Untap,
    Upkeep,
    Draw,
    Main1,
    CombatBegin,
    CombatDeclareAttackers,
    CombatDeclareBlockers,
    CombatFirstStrikeDamage,
    CombatDamage,
    CombatEnd,
    Main2,
    EndOfTurn,
    Cleanup,
}

impl PhaseType {
    pub const ALL: [PhaseType; 13] = [
        Self::Untap,
        Self::Upkeep,
        Self::Draw,
        Self::Main1,
        Self::CombatBegin,
        Self::CombatDeclareAttackers,
        Self::CombatDeclareBlockers,
        Self::CombatFirstStrikeDamage,
        Self::CombatDamage,
        Self::CombatEnd,
        Self::Main2,
        Self::EndOfTurn,
        Self::Cleanup,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Untap => "Untap",
            Self::Upkeep => "Upkeep",
            Self::Draw => "Draw",
            Self::Main1 => "Main 1",
            Self::CombatBegin => "Begin Combat",
            Self::CombatDeclareAttackers => "Declare Attackers",
            Self::CombatDeclareBlockers => "Declare Blockers",
            Self::CombatFirstStrikeDamage => "First Strike Damage",
            Self::CombatDamage => "Combat Damage",
            Self::CombatEnd => "End Combat",
            Self::Main2 => "Main 2",
            Self::EndOfTurn => "End Step",
            Self::Cleanup => "Cleanup",
        }
    }
    pub fn is_combat(self) -> bool {
        matches!(
            self,
            Self::CombatBegin
                | Self::CombatDeclareAttackers
                | Self::CombatDeclareBlockers
                | Self::CombatFirstStrikeDamage
                | Self::CombatDamage
                | Self::CombatEnd
        )
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Game {
    pub turn: i32,
    pub phase: Option<PhaseType>,
    pub player_turn: Option<PlayerId>,
    pub is_commander: bool,
    pub game_over: bool,
    pub match_over: bool,
    pub storm_count: i32,
    pub winner: Option<String>,
    pub players: Vec<Player>,
    /// Top of stack first.
    pub stack: Vec<StackItem>,
    pub combat: Option<Combat>,
}

impl Game {
    pub fn player(&self, id: PlayerId) -> Option<&Player> {
        self.players.iter().find(|p| p.id == id)
    }
    /// Every card in every player zone (not stack sources).
    pub fn all_cards(&self) -> impl Iterator<Item = &Card> {
        self.players.iter().flat_map(|p| p.zones.iter())
    }
    pub fn card(&self, id: CardId) -> Option<&Card> {
        self.all_cards().find(|c| c.id == id)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Player {
    pub id: PlayerId,
    pub name: String,
    #[serde(rename = "isAI")]
    pub is_ai: bool,
    pub is_viewer: bool,
    pub life: i32,
    pub has_priority: bool,
    pub has_lost: bool,
    pub max_hand_size: i32,
    pub unlimited_hand_size: bool,
    pub lands_played_this_turn: i32,
    pub max_land_plays: i32,
    pub is_extra_turn: bool,
    pub counters: BTreeMap<String, i32>,
    /// Keys W U B R G C.
    pub mana_pool: BTreeMap<String, i32>,
    pub commanders: Vec<CardId>,
    /// commander card id (as string) → times cast
    pub commander_cast: BTreeMap<String, i32>,
    /// commander card id (as string) → damage dealt to this player
    pub commander_damage: BTreeMap<String, i32>,
    pub zones: Zones,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Zones {
    pub battlefield: Vec<Card>,
    pub hand: Vec<Card>,
    pub graveyard: Vec<Card>,
    pub exile: Vec<Card>,
    pub command: Vec<Card>,
    pub library: Vec<Card>,
}

impl Zones {
    pub fn iter(&self) -> impl Iterator<Item = &Card> {
        self.battlefield
            .iter()
            .chain(&self.hand)
            .chain(&self.graveyard)
            .chain(&self.exile)
            .chain(&self.command)
            .chain(&self.library)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct EntityRef {
    #[serde(rename = "type")]
    pub kind: String,
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct StackItem {
    pub id: i64,
    /// Yield key for auto-yield (`yield` frame).
    pub key: String,
    pub text: String,
    pub is_ability: bool,
    pub is_trigger: bool,
    pub is_optional_trigger: bool,
    pub source: Option<Card>,
    pub activator: Option<PlayerId>,
    pub target_cards: Vec<CardId>,
    pub target_players: Vec<PlayerId>,
    pub sub: Option<Box<StackItem>>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Combat {
    pub attackers: Vec<AttackerInfo>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AttackerInfo {
    pub attacker: CardId,
    pub defender: Option<EntityRef>,
    pub blockers: Vec<CardId>,
    pub planned_blockers: Vec<CardId>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Stops {
    pub my_turn: Vec<PhaseType>,
    pub opp_turn: Vec<PhaseType>,
}
