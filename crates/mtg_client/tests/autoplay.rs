//! Decision tests for `AutoPolicy` on hand-built boards and on real recorded fixtures.

use std::path::PathBuf;

use mtg_client::autoplay::AutoPolicy;
use mtg_protocol::*;
use serde_json::{Value, json};

fn land(id: i64) -> Value {
    json!({"id":id,"zone":"Hand","name":"Forest","state":{"name":"Forest","type":"Basic Land — Forest","isLand":true,"manaCost":null}})
}
fn spell(id: i64, cost: &str) -> Value {
    json!({"id":id,"zone":"Hand","name":"Bear","state":{"name":"Bear","type":"Creature — Bear","manaCost":cost,"isCreature":true,"power":2,"toughness":2}})
}
fn creature(id: i64, sick: bool, tapped: bool, p: i64, t: i64) -> Value {
    json!({"id":id,"zone":"Battlefield","name":"Guy","sick":sick,"tapped":tapped,"state":{"name":"Guy","isCreature":true,"power":p,"toughness":t,"manaCost":"{1}"}})
}
fn bf_land(id: i64, tapped: bool) -> Value {
    json!({"id":id,"zone":"Battlefield","name":"Forest","tapped":tapped,"state":{"name":"Forest","isLand":true}})
}

fn game(my_hand: Vec<Value>, my_bf: Vec<Value>, opp_bf: Vec<Value>, lands_played: i64) -> Game {
    serde_json::from_value(json!({
        "turn":3,"phase":"MAIN1","playerTurn":0,
        "players":[
            {"id":0,"name":"Tester","isViewer":true,"life":20,"maxLandPlays":1,"landsPlayedThisTurn":lands_played,
             "zones":{"hand":my_hand,"battlefield":my_bf}},
            {"id":1,"name":"Forge AI","isAI":true,"life":20,"zones":{"battlefield":opp_bf}}
        ]
    }))
    .unwrap()
}

fn input(seq: i64, msg: &str) -> InputFrame {
    serde_json::from_value(json!({"seq":seq,"message":msg,"ok":{"label":"OK","enabled":true},"cancel":{"label":"End Turn","enabled":true}})).unwrap()
}
const MAIN: &str = "Priority: Tester\nTurn: 3 (Tester)\nPhase: Main phase, precombat\nStack: Empty";

fn as_json(f: Option<ClientFrame>) -> Value {
    serde_json::from_str(&f.expect("policy answered").to_json()).unwrap()
}

#[test]
fn plays_a_land_first() {
    let g = game(vec![spell(10, "{G}"), land(11)], vec![bf_land(20, false)], vec![], 0);
    let mut p = AutoPolicy::new(1);
    assert_eq!(as_json(p.on_input(&input(1, MAIN), &g, Some(0))), json!({"t":"click","action":"card","id":11}));
}

#[test]
fn one_land_per_turn_then_spells() {
    let g = game(vec![land(11), spell(10, "{1}{G}")], vec![bf_land(20, false), bf_land(21, false)], vec![], 1);
    let mut p = AutoPolicy::new(1);
    assert_eq!(as_json(p.on_input(&input(1, MAIN), &g, Some(0))), json!({"t":"click","action":"card","id":10}));
}

#[test]
fn skips_unaffordable_spells_and_passes() {
    let g = game(vec![spell(10, "{4}{G}")], vec![bf_land(20, false), bf_land(21, true)], vec![], 1);
    let mut p = AutoPolicy::new(1);
    assert_eq!(as_json(p.on_input(&input(1, MAIN), &g, Some(0))), json!({"t":"click","action":"ok"}));
}

#[test]
fn most_expensive_affordable_spell_first() {
    let g = game(vec![spell(10, "{1}"), spell(12, "{2}{G}"), spell(13, "{5}")], vec![bf_land(20, false), bf_land(21, false), bf_land(22, false)], vec![], 1);
    let mut p = AutoPolicy::new(1);
    assert_eq!(as_json(p.on_input(&input(1, MAIN), &g, Some(0))), json!({"t":"click","action":"card","id":12}));
}

#[test]
fn flash_blacklists_the_card_and_retries_with_another() {
    let g = game(vec![spell(12, "{2}"), spell(10, "{1}")], vec![bf_land(20, false), bf_land(21, false)], vec![], 1);
    let mut p = AutoPolicy::new(1);
    let i = input(1, MAIN);
    assert_eq!(as_json(p.on_input(&i, &g, Some(0)))["id"], 12);
    assert!(p.on_input(&i, &g, Some(0)).is_none(), "same input repaint must not be answered twice");
    p.on_flash();
    assert_eq!(as_json(p.on_input(&i, &g, Some(0)))["id"], 10);
    p.on_flash();
    assert_eq!(as_json(p.on_input(&i, &g, Some(0))), json!({"t":"click","action":"ok"}));
}

#[test]
fn does_not_play_on_opponents_turn_or_with_stack() {
    let g = game(vec![land(11)], vec![], vec![], 0);
    let mut p = AutoPolicy::new(1);
    let opp = "Priority: Tester\nTurn: 4 (Forge AI)\nPhase: Main phase, precombat\nStack: Empty";
    assert_eq!(as_json(p.on_input(&input(1, opp), &g, Some(0))), json!({"t":"click","action":"ok"}));
    let stack = "Priority: Tester\nTurn: 3 (Tester)\nPhase: Main phase, precombat\nStack: 1 to Resolve.";
    assert_eq!(as_json(p.on_input(&input(2, stack), &g, Some(0))), json!({"t":"click","action":"ok"}));
}

#[test]
fn waits_when_engine_is_busy() {
    let g = game(vec![], vec![], vec![], 0);
    let mut p = AutoPolicy::new(1);
    let mut i = input(1, "");
    i.ok.enabled = false;
    i.cancel.enabled = false;
    assert!(p.on_input(&i, &g, Some(0)).is_none());
}

#[test]
fn mana_payment_presses_auto() {
    let g = game(vec![], vec![], vec![], 0);
    let mut p = AutoPolicy::new(1);
    let mut i = input(5, "Bear\n\nPay Mana Cost: {1}{G}");
    i.ok.label = "Auto".into();
    assert_eq!(as_json(p.on_input(&i, &g, Some(0))), json!({"t":"click","action":"ok"}));
}

#[test]
fn attacks_with_alpha_strike_when_opponent_has_no_untapped_creatures() {
    let mut g = game(vec![], vec![creature(30, false, false, 2, 2)], vec![], 1);
    g.phase = Some(PhaseType::CombatDeclareAttackers);
    let mut i = input(7, "Select creatures to attack Forge AI or select player/card you wish to attack.");
    i.cancel.label = "Alpha Strike".into();
    let mut p = AutoPolicy::new(1);
    assert_eq!(as_json(p.on_input(&i, &g, Some(0))), json!({"t":"click","action":"alphaStrike"}));
    // Once creatures are attacking, confirm with OK.
    let mut g2 = g.clone();
    g2.players[0].zones.battlefield[0].attacking = true;
    assert_eq!(as_json(p.on_input(&i, &g2, Some(0))), json!({"t":"click","action":"ok"}));
}

#[test]
fn does_not_attack_with_summoning_sick_creatures() {
    let mut g = game(vec![], vec![creature(30, true, false, 2, 2)], vec![], 1);
    g.phase = Some(PhaseType::CombatDeclareAttackers);
    let i = input(7, "Select creatures to attack Forge AI or select player/card you wish to attack.");
    let mut p = AutoPolicy::new(1);
    assert_eq!(as_json(p.on_input(&i, &g, Some(0))), json!({"t":"click","action":"ok"}));
}

#[test]
fn only_sends_safe_attackers_into_blockers() {
    let mut g = game(vec![], vec![creature(30, false, false, 5, 1), creature(31, false, false, 1, 6)], vec![creature(40, false, false, 3, 3)], 1);
    g.phase = Some(PhaseType::CombatDeclareAttackers);
    let mut i = input(7, "Select creatures to attack Forge AI or select player/card you wish to attack.");
    i.cancel.enabled = false; // no alpha strike offered
    let mut p = AutoPolicy::new(1);
    assert_eq!(as_json(p.on_input(&i, &g, Some(0))), json!({"t":"click","action":"card","id":31}));
}

#[test]
fn invalid_attack_declaration_forces_alpha_strike_not_ok_forever() {
    let mut g = game(vec![], vec![creature(30, false, false, 2, 2)], vec![creature(40, false, false, 3, 3)], 1);
    g.phase = Some(PhaseType::CombatDeclareAttackers);
    let mut i = input(7, "Select creatures to attack Forge AI or select player/card you wish to attack.");
    i.cancel.label = "Alpha Strike".into();
    let mut p = AutoPolicy::new(1);
    // Let the policy decide "no attack" first (opponent has a blocker; our creature is unsafe)...
    let first = as_json(p.on_input(&i, &g, Some(0)));
    assert!(first == json!({"t":"click","action":"ok"}) || first["action"] == "alphaStrike");
    // ...then the engine rejects the declaration and re-asks with a fresh seq.
    p.on_message("Attack declaration invalid");
    i.seq = 8;
    assert_eq!(as_json(p.on_input(&i, &g, Some(0))), json!({"t":"click","action":"alphaStrike"}));
}

#[test]
fn rejected_block_is_taken_back() {
    let mut g = game(vec![], vec![creature(30, false, false, 2, 5)], vec![], 1);
    g.phase = Some(PhaseType::CombatDeclareBlockers);
    g.player_turn = Some(1);
    g.players[0].zones.battlefield[0].blocking = true;
    let mut i = input(9, "Select creatures to block Construct Token (208) or select another attacker to declare blockers for.");
    i.highlighted = vec![208];
    let mut p = AutoPolicy::new(1);
    let mut earlier = i.clone();
    earlier.seq = 8;
    let _ = p.on_input(&earlier, &g, Some(0)); // establishes the turn (the rejection happens mid-turn)
    p.on_message("Construct Token (208) cannot be blocked with 1 creatures you've assigned");
    assert_eq!(as_json(p.on_input(&i, &g, Some(0))), json!({"t":"click","action":"card","id":30}));
}

#[test]
fn targeting_prefers_opponent_creature_then_player() {
    let g = game(vec![], vec![creature(30, false, false, 2, 2)], vec![creature(40, false, false, 3, 3)], 1);
    let mut p = AutoPolicy::new(1);
    let mut i = input(11, "Bolt\n\nSelect target creature");
    i.ok.enabled = false;
    i.selectable = vec![30, 40];
    assert_eq!(as_json(p.on_input(&i, &g, Some(0))), json!({"t":"click","action":"card","id":40}));
    let mut j = input(12, "Select target player");
    j.ok.enabled = false;
    j.selectable_players = vec![0, 1];
    assert_eq!(as_json(p.on_input(&j, &g, Some(0))), json!({"t":"click","action":"player","id":1}));
}

#[test]
fn never_stalls_on_a_repeating_seq() {
    let g = game(vec![], vec![], vec![], 0);
    let mut p = AutoPolicy::new(1);
    let mut i = input(20, "Weird input");
    let mut cancels = 0;
    for n in 0..12 {
        i.highlighted = vec![n]; // something changes each time so it isn't suppressed as a repaint
        if let Some(f) = p.on_input(&i, &g, Some(0))
            && matches!(f, ClientFrame::Click { action: ClickAction::Cancel, .. })
        {
            cancels += 1;
        }
    }
    assert!(cancels > 0, "fallback must eventually cancel");
}

// ---- prompts ---------------------------------------------------------------------------------

fn prompt(v: Value) -> PromptFrame {
    match serde_json::from_value::<ServerFrame>({
        let mut v = v;
        v["t"] = "prompt".into();
        v
    })
    .unwrap()
    {
        ServerFrame::Prompt(p) => p,
        o => panic!("{o:?}"),
    }
}

fn answer(p: &PromptFrame) -> Value {
    let g = Game::default();
    match AutoPolicy::new(1).on_prompt(p, &g, Some(0)) {
        ClientFrame::Answer { id, value } => {
            assert_eq!(id, p.id);
            value
        }
        o => panic!("{o:?}"),
    }
}

fn opts(n: i64) -> Value {
    (0..n).map(|i| json!({"i":i,"type":"text","label":format!("o{i}")})).collect::<Vec<_>>().into()
}

#[test]
fn prompt_confirm_option_text() {
    assert_eq!(answer(&prompt(json!({"id":1,"kind":"confirm","yes":"Y","no":"N","defaultYes":true}))), json!(true));
    assert_eq!(answer(&prompt(json!({"id":1,"kind":"confirm","yes":"Y","no":"N","defaultYes":false}))), json!(false));
    assert_eq!(answer(&prompt(json!({"id":2,"kind":"option","options":["a","b"],"default":1}))), json!(1));
    assert_eq!(answer(&prompt(json!({"id":3,"kind":"text","initial":"7","options":[],"numeric":true}))), json!("7"));
    assert_eq!(answer(&prompt(json!({"id":3,"kind":"text","options":[],"numeric":true}))), json!("1"));
}

#[test]
fn prompt_choices_take_first_min() {
    assert_eq!(answer(&prompt(json!({"id":4,"kind":"choices","min":2,"max":3,"options":opts(5)}))), json!([0, 1]));
    assert_eq!(answer(&prompt(json!({"id":4,"kind":"chooseEntity","min":1,"max":1,"options":opts(5)}))), json!([0]));
    // informational list: nothing to choose
    assert_eq!(answer(&prompt(json!({"id":4,"kind":"choices","min":-1,"max":-1,"options":opts(5)}))), json!([]));
    // optional pick still answers with an array
    assert!(answer(&prompt(json!({"id":4,"kind":"chooseEntity","min":0,"max":2,"options":opts(3)}))).is_array());
}

#[test]
fn prompt_choose_ability_first_playable() {
    let opts = json!([
        {"i":0,"type":"ability","label":"a","ability":{"id":1,"description":"x","canPlay":false}},
        {"i":1,"type":"ability","label":"b","ability":{"id":2,"description":"y","canPlay":true}}
    ]);
    assert_eq!(answer(&prompt(json!({"id":5,"kind":"chooseAbility","options":opts}))), json!(1));
    let none = json!([{"i":0,"type":"ability","label":"a","ability":{"id":1,"description":"x","canPlay":false}}]);
    assert_eq!(answer(&prompt(json!({"id":5,"kind":"chooseAbility","options":none}))), Value::Null);
}

#[test]
fn prompt_order_and_manipulate() {
    let v = answer(&prompt(json!({"id":6,"kind":"order","top":"Top","remainMin":0,"remainMax":0,"options":opts(3),"sourceCount":3})));
    assert_eq!(v, json!({"order":[0,1,2],"remember":false}));
    let v = answer(&prompt(json!({"id":6,"kind":"order","remainMin":1,"remainMax":1,"options":opts(3),"sourceCount":3})));
    assert_eq!(v["order"], json!([1, 2]));
    let v = answer(&prompt(json!({"id":7,"kind":"manipulateCards","cards":[{"id":9},{"id":4}],"manipulable":[9,4]})));
    assert_eq!(v, json!([9, 4]));
}

#[test]
fn prompt_assign_combat_damage_lethal_in_order_rest_to_defender() {
    let blockers = json!([
        {"id":50,"state":{"toughness":2},"damage":0},
        {"id":51,"state":{"toughness":3},"damage":1}
    ]);
    let v = answer(&prompt(json!({"id":8,"kind":"assignCombatDamage","attacker":{"id":1},"blockers":blockers,"damage":7,"defender":{"type":"player","id":1,"name":"x"}})));
    assert_eq!(v, json!({"50":2,"51":2,"defender":3}));
    // not enough damage: stops when it runs out
    let v = answer(&prompt(json!({"id":8,"kind":"assignCombatDamage","blockers":[{"id":50,"state":{"toughness":4}}],"damage":3,"defender":{"type":"player","id":1,"name":"x"}})));
    assert_eq!(v, json!({"50":3}));
    // no defender: surplus on the last blocker; no blockers: all to defender
    let v = answer(&prompt(json!({"id":8,"kind":"assignCombatDamage","blockers":[{"id":50,"state":{"toughness":1}}],"damage":4})));
    assert_eq!(v, json!({"50":4}));
    let v = answer(&prompt(json!({"id":8,"kind":"assignCombatDamage","blockers":[],"damage":4,"defender":{"type":"player","id":1,"name":"x"}})));
    assert_eq!(v, json!({"defender":4}));
}

#[test]
fn prompt_assign_amount_sums_to_amount() {
    let v = answer(&prompt(json!({"id":9,"kind":"assignAmount","amount":5,"atLeastOne":true,"label":"dmg","options":opts(3)})));
    let sum: i64 = v.as_object().unwrap().values().map(|x| x.as_i64().unwrap()).sum();
    assert_eq!(sum, 5);
    assert!(v.as_object().unwrap().values().all(|x| x.as_i64().unwrap() >= 1));
    let v = answer(&prompt(json!({"id":9,"kind":"assignAmount","amount":4,"options":opts(2)})));
    assert_eq!(v.as_object().unwrap().values().map(|x| x.as_i64().unwrap()).sum::<i64>(), 4);
}

#[test]
fn unknown_prompt_answers_null() {
    assert_eq!(answer(&prompt(json!({"id":10,"kind":"somethingNew"}))), Value::Null);
}

// ---- real fixtures --------------------------------------------------------------------------------

fn fixture(name: &str) -> Vec<ServerFrame> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../mtg_protocol/tests/fixtures").join(name);
    std::fs::read_to_string(p).unwrap().lines().filter(|l| !l.trim().is_empty()).map(|l| ServerFrame::parse(l).unwrap()).collect()
}

#[test]
fn every_recorded_prompt_gets_a_well_formed_answer() {
    let mut n = 0;
    for name in ["commander.jsonl", "constructed.jsonl", "synthetic_prompts.jsonl"] {
        for f in fixture(name) {
            if let ServerFrame::Prompt(p) = f {
                let v = answer(&p);
                match &p.body {
                    PromptBody::Choices { .. } | PromptBody::ChooseEntity { .. } | PromptBody::ManipulateCards { .. } => assert!(v.is_array(), "{v}"),
                    PromptBody::Order { .. } => assert!(v["order"].is_array()),
                    PromptBody::AssignCombatDamage { damage, .. } => {
                        let sum: i64 = v.as_object().unwrap().values().map(|x| x.as_i64().unwrap()).sum();
                        assert_eq!(sum, *damage);
                    }
                    PromptBody::Confirm { .. } => assert!(v.is_boolean()),
                    PromptBody::Text { .. } => assert!(v.is_string()),
                    _ => {}
                }
                n += 1;
            }
        }
    }
    assert!(n > 0, "fixtures contain no prompts");
}

#[test]
fn every_recorded_input_gets_an_answer_or_a_wait() {
    // Feed each (state, input) pair of the fixtures through a fresh policy: it must never panic and
    // must answer every input whose buttons or selection are live.
    let mut answered = 0;
    for name in ["commander.jsonl", "constructed.jsonl"] {
        let frames = fixture(name);
        let mut st: Option<StateFrame> = None;
        for f in frames {
            match f {
                ServerFrame::State(s) => st = Some(s),
                ServerFrame::Input(i) => {
                    let Some(s) = &st else { continue };
                    let mut p = AutoPolicy::new(3);
                    let live = i.ok.enabled || i.cancel.enabled || !i.selectable.is_empty() || !i.selectable_players.is_empty();
                    let r = p.on_input(&i, &s.game, s.you);
                    assert_eq!(r.is_some(), live, "input {:?}", i.message);
                    answered += usize::from(r.is_some());
                }
                _ => {}
            }
        }
    }
    assert!(answered > 0);
}
