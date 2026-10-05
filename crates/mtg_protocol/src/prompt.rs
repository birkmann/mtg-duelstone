//! Blocking `prompt` frames — answered with `ClientFrame::Answer { id, value }`.
//! See the table in protocol.md for the shape of each `value`.

use serde::{Deserialize, Serialize};

use crate::{Card, CardId, EntityRef, PlayerId};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptFrame {
    pub id: i64,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(flatten)]
    pub body: PromptBody,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum PromptBody {
    /// Answer: option index, or null to cancel.
    ChooseAbility {
        #[serde(default)]
        host_card: Option<Card>,
        #[serde(default)]
        options: Vec<ChoiceOption>,
    },
    /// Answer: bool.
    Confirm {
        #[serde(default)]
        card: Option<Card>,
        #[serde(default)]
        yes: String,
        #[serde(default)]
        no: String,
        #[serde(default)]
        default_yes: bool,
    },
    /// Answer: index.
    Option {
        #[serde(default)]
        options: Vec<String>,
        #[serde(default)]
        default: i64,
    },
    /// Answer: string.
    Text {
        #[serde(default)]
        initial: Option<String>,
        #[serde(default)]
        options: Vec<String>,
        #[serde(default)]
        numeric: bool,
    },
    /// Answer: `[i, ...]`. min/max < 0 = informational list.
    Choices {
        #[serde(default)]
        min: i64,
        #[serde(default)]
        max: i64,
        #[serde(default)]
        options: Vec<ChoiceOption>,
        #[serde(default)]
        selected: Vec<i64>,
    },
    /// Answer: `[i, ...]` (empty = none).
    ChooseEntity {
        #[serde(default)]
        min: i64,
        #[serde(default)]
        max: i64,
        #[serde(default)]
        options: Vec<ChoiceOption>,
        #[serde(default)]
        reveal: Option<Reveal>,
    },
    /// Answer: `{ order: [i...], remember: bool }` — indices of everything moved to the ordered/dest list.
    Order {
        #[serde(default)]
        top: String,
        #[serde(default)]
        remain_min: i64,
        #[serde(default)]
        remain_max: i64,
        #[serde(default)]
        options: Vec<ChoiceOption>,
        #[serde(default)]
        source_count: i64,
        #[serde(default)]
        ref_card: Option<Card>,
        #[serde(default)]
        show_remember: bool,
    },
    /// Answer: `{ "<blockerCardId>": n, "defender"?: n }` summing to `damage`.
    /// `null` ONLY when `may_skip` and meaning "assign another attacker first" — never "auto".
    AssignCombatDamage {
        #[serde(default)]
        attacker: Option<Card>,
        #[serde(default)]
        blockers: Vec<Card>,
        #[serde(default)]
        damage: i64,
        #[serde(default)]
        defender: Option<EntityRef>,
        #[serde(default)]
        override_order: bool,
        #[serde(default)]
        may_skip: bool,
    },
    /// Answer: `{ "<i>": n }`.
    AssignAmount {
        #[serde(default)]
        source: Option<Card>,
        #[serde(default)]
        amount: i64,
        #[serde(default)]
        at_least_one: bool,
        #[serde(default)]
        label: String,
        #[serde(default)]
        options: Vec<ChoiceOption>,
    },
    /// Answer: `[cardId...]` new order.
    ManipulateCards {
        #[serde(default)]
        cards: Vec<Card>,
        #[serde(default)]
        manipulable: Vec<CardId>,
        #[serde(default)]
        to_top: bool,
        #[serde(default)]
        to_bottom: bool,
        #[serde(default)]
        to_anywhere: bool,
    },
    /// A prompt kind this client doesn't know yet.
    #[serde(other)]
    Unknown,
}

impl PromptBody {
    pub fn kind_name(&self) -> &'static str {
        match self {
            Self::ChooseAbility { .. } => "chooseAbility",
            Self::Confirm { .. } => "confirm",
            Self::Option { .. } => "option",
            Self::Text { .. } => "text",
            Self::Choices { .. } => "choices",
            Self::ChooseEntity { .. } => "chooseEntity",
            Self::Order { .. } => "order",
            Self::AssignCombatDamage { .. } => "assignCombatDamage",
            Self::AssignAmount { .. } => "assignAmount",
            Self::ManipulateCards { .. } => "manipulateCards",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ChoiceOption {
    pub i: i64,
    /// `card` | `player` | `ability` | `stackItem` | `entity` | `number` | `color` | `text`
    #[serde(rename = "type")]
    pub kind: String,
    pub label: String,
    pub id: Option<i64>,
    pub card: Option<Card>,
    pub ability: Option<AbilityInfo>,
    pub value: Option<serde_json::Value>,
    pub current: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AbilityInfo {
    pub id: i64,
    pub description: String,
    pub is_spell: bool,
    pub can_play: bool,
    pub host_card: Option<CardId>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Reveal {
    pub cards: Vec<Card>,
    pub zones: Vec<String>,
    pub owner: Option<PlayerId>,
    pub message: Option<String>,
}
