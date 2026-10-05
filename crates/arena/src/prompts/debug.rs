//! Dev-only: `ARENA_PROMPT=<kind>` injects a synthetic prompt built from the live game's cards so
//! every modal can be screenshotted without waiting for the engine to ask. Kinds: confirm, option,
//! ability, choices, entity, order, damage, amount, manip, text, mulligan, tray.

use bevy::prelude::*;
use mtg_protocol::{ButtonState, Card, InputFrame, PromptFrame};
use serde_json::json;

use crate::shared::*;

pub struct PromptDebugPlugin;

impl Plugin for PromptDebugPlugin {
    fn build(&self, app: &mut App) {
        if std::env::var("ARENA_PROMPT").is_ok() {
            app.add_systems(Update, inject.run_if(in_state(AppState::InMatch)));
        }
    }
}

fn opt(i: i64, c: &Card) -> serde_json::Value {
    json!({"i": i, "type": "card", "label": c.display_name(), "id": c.id, "card": c})
}

fn inject(time: Res<Time>, view: Res<GameView>, mut pending: ResMut<PendingPrompt>, mut input: ResMut<CurrentInput>) {
    if time.elapsed_secs() < 9.0 {
        return;
    }
    let kind = std::env::var("ARENA_PROMPT").unwrap_or_default();
    let Some(me) = view.me() else { return };
    let hand: Vec<Card> = me.zones.hand.iter().filter(|c| c.is_visible()).cloned().collect();
    if hand.len() < 4 {
        return;
    }
    if kind == "mulligan" {
        input.frame = Some(InputFrame {
            seq: 777,
            message: "Do you want to keep your hand?".into(),
            ok: ButtonState { label: "Keep".into(), enabled: true },
            cancel: ButtonState { label: "Mulligan".into(), enabled: true },
            ..Default::default()
        });
        return;
    }
    if kind == "tray" {
        input.frame = Some(InputFrame {
            seq: 778,
            message: "Choose a card to put into your hand".into(),
            ok: ButtonState { label: "OK".into(), enabled: false },
            cancel: ButtonState { label: "Cancel".into(), enabled: true },
            selectable: hand.iter().map(|c| c.id).collect(),
            highlighted: vec![hand[1].id],
            tray_cards: hand.iter().map(|c| Card { zone: Some("Library".into()), ..c.clone() }).collect(),
            select_min: 1,
            select_max: 1,
            ..Default::default()
        });
        return;
    }
    if pending.0.is_some() {
        return;
    }
    let v = match kind.as_str() {
        "confirm" => json!({"id": 9001, "kind":"confirm","title":"Optional Trigger","message":"Do you want to pay {2}{G} to draw a card?","card":hand[0],"yes":"Pay","no":"Decline","defaultYes":true}),
        "option" => json!({"id": 9002, "kind":"option","title":"Choose a mode","message":"Choose one","options":["Draw two cards","Deal 3 damage to any target","Gain 5 life"],"default":0}),
        "ability" => json!({"id": 9003, "kind":"chooseAbility","hostCard":hand[0],"options":[
            {"i":0,"type":"ability","label":"Cast","ability":{"id":1,"description":"Cast Overgrown Battlement","isSpell":true,"canPlay":true}},
            {"i":1,"type":"ability","label":"Ability","ability":{"id":2,"description":"{T}: Add {G}.","canPlay":true}},
            {"i":2,"type":"ability","label":"Ability","ability":{"id":3,"description":"{2}{G}: Put a +1/+1 counter on target creature.","canPlay":false}}]}),
        "choices" => json!({"id": 9004, "kind":"choices","title":"Discard Cards","message":"Choose 2 cards to discard","min":2,"max":2,"selected":[],"options":[opt(0,&hand[0]),opt(1,&hand[1]),opt(2,&hand[2]),opt(3,&hand[3])]}),
        "entity" => json!({"id": 9005, "kind":"chooseEntity","title":"Search Your Library","message":"Choose a creature card","min":0,"max":1,"options":[opt(0,&hand[0]),opt(1,&hand[1]),opt(2,&hand[2]),opt(3,&hand[3]),
            {"i":4,"type":"player","label":"Forge AI","id":2}]}),
        "order" => json!({"id": 9006, "kind":"order","title":"Order Triggers","message":"Choose the order in which these triggers go on the stack","top":"Top of stack first","remainMin":0,"remainMax":-1,"sourceCount":3,"showRemember":true,
            "options":[{"i":0,"type":"text","label":"Whenever a creature enters, draw a card"},{"i":1,"type":"text","label":"At the beginning of your end step, scry 1"},{"i":2,"type":"text","label":"Whenever you gain life, put a +1/+1 counter"}]}),
        "damage" => json!({"id": 9007, "kind":"assignCombatDamage","title":"Assign Combat Damage","attacker":hand[0],"blockers":[hand[1],hand[2]],"damage":6,"defender":{"type":"player","id":2,"name":"Forge AI"},"overrideOrder":false,"maySkip":true}),
        "amount" => json!({"id": 9008, "kind":"assignAmount","title":"Divide Damage","source":hand[0],"amount":5,"atLeastOne":true,"label":"damage","options":[opt(0,&hand[1]),opt(1,&hand[2]),{"i":2,"type":"player","label":"Forge AI","id":2}]}),
        "manip" => json!({"id": 9009, "kind":"manipulateCards","title":"Scry 3","message":"Arrange the cards","cards":[hand[0],hand[1],hand[2],hand[3]],"manipulable":[hand[0].id,hand[1].id,hand[2].id],"toTop":true,"toBottom":true,"toAnywhere":false}),
        "text" => json!({"id": 9010, "kind":"text","title":"Choose X","message":"Announce a value for X","initial":"2","options":[],"numeric":true}),
        _ => return,
    };
    if let Ok(p) = serde_json::from_value::<PromptFrame>(v) {
        pending.0 = Some(p);
    } else {
        warn!("ARENA_PROMPT: synthetic frame failed to parse");
    }
}
