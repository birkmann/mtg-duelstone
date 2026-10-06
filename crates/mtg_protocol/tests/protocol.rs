//! Fixture-driven protocol tests: every recorded engine frame must parse into a *known*
//! `ServerFrame`/`PromptBody`, and client frames must serialize exactly as protocol.md says.
//! Fixtures are curated from real autopilot recordings (see `curate_fixtures.py`).

use std::path::PathBuf;

use mtg_protocol::*;
use serde_json::{Value, json};

fn fixture_files() -> Vec<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "jsonl")).collect();
    v.sort();
    v
}

/// All frames of all fixtures as (file, raw json, parsed).
fn frames() -> Vec<(String, Value, ServerFrame)> {
    let mut out = Vec::new();
    for p in fixture_files() {
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        for (n, line) in std::fs::read_to_string(&p).unwrap().lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let raw: Value = serde_json::from_str(line).unwrap_or_else(|e| panic!("{name}:{}: bad json: {e}", n + 1));
            let f = ServerFrame::parse(line).unwrap_or_else(|e| panic!("{name}:{}: {e}", n + 1));
            out.push((format!("{name}:{}", n + 1), raw, f));
        }
    }
    out
}

#[test]
fn fixtures_exist() {
    assert!(!fixture_files().is_empty(), "no fixtures");
}

#[test]
fn every_fixture_frame_is_known() {
    for (at, raw, f) in frames() {
        if raw["t"] == "unknownForTest" {
            continue; // deliberately unknown frames (none shipped; hook for future fixtures)
        }
        assert!(!matches!(f, ServerFrame::Unknown), "{at}: unknown frame type {}", raw["t"]);
        if let ServerFrame::Prompt(p) = &f {
            assert!(!matches!(p.body, PromptBody::Unknown), "{at}: unknown prompt kind {}", raw["kind"]);
        }
    }
}

#[test]
fn fixtures_cover_core_frame_types() {
    let all = frames();
    let has = |pred: &dyn Fn(&ServerFrame) -> bool| all.iter().any(|(_, _, f)| pred(f));
    assert!(has(&|f| matches!(f, ServerFrame::Hello { .. })), "hello");
    assert!(has(&|f| matches!(f, ServerFrame::State(_))), "state");
    assert!(has(&|f| matches!(f, ServerFrame::Input(_))), "input");
    assert!(has(&|f| matches!(f, ServerFrame::Log { .. })), "log");
    assert!(has(&|f| matches!(f, ServerFrame::GameOver { .. })), "gameOver");
    assert!(has(&|f| matches!(f, ServerFrame::Prompt(_))), "prompt");
}

#[test]
fn every_prompt_kind_has_a_fixture() {
    // commander/constructed.jsonl are real recordings; synthetic_prompts.jsonl is hand-written from
    // WsGuiGame.java for the kinds soak games rarely or never trigger.
    let mut kinds = std::collections::BTreeSet::new();
    for (_, _, f) in frames() {
        if let ServerFrame::Prompt(p) = f {
            kinds.insert(p.body.kind_name());
        }
    }
    for k in ["chooseAbility", "confirm", "option", "text", "choices", "chooseEntity", "order", "assignCombatDamage", "assignAmount", "manipulateCards"] {
        assert!(kinds.contains(k), "no fixture for prompt kind {k}; have {kinds:?}");
    }
}

#[test]
fn state_frames_are_sane() {
    for (at, _, f) in frames() {
        if let ServerFrame::State(s) = f {
            let you = s.you.unwrap_or(-1);
            let me = s.game.player(you).unwrap_or_else(|| panic!("{at}: `you` {you} not among players"));
            assert!(me.is_viewer, "{at}: you must be the viewer");
            assert!(s.game.players.len() >= 2, "{at}");
            for c in s.game.all_cards() {
                assert!(c.hidden || c.state.is_some() || c.face_down || !c.name.is_empty(), "{at}: visible card {} without state", c.id);
            }
        }
    }
}

#[test]
fn nothing_the_engine_sends_is_silently_dropped() {
    // Re-serialize each typed frame and make sure every key of the original survives (modulo the
    // few engine-internal debug keys we deliberately don't model).
    const IGNORED: &[&str] = &["javaType", "t"];
    fn missing(orig: &Value, ours: &Value, path: &str, out: &mut Vec<String>) {
        match (orig, ours) {
            (Value::Object(a), Value::Object(b)) => {
                for (k, v) in a {
                    if IGNORED.contains(&k.as_str()) {
                        continue;
                    }
                    match b.get(k) {
                        Some(bv) => missing(v, bv, &format!("{path}.{k}"), out),
                        None if v.is_null() => {} // null → Option::None → skipped or null, both fine
                        None => out.push(format!("{path}.{k}")),
                    }
                }
            }
            (Value::Array(a), Value::Array(b)) => {
                for (x, y) in a.iter().zip(b) {
                    missing(x, y, &format!("{path}[]"), out);
                }
            }
            _ => {}
        }
    }
    let mut dropped = std::collections::BTreeSet::new();
    for (_, raw, f) in frames() {
        let ours = serde_json::to_value(&f).unwrap();
        let mut out = Vec::new();
        missing(&raw, &ours, raw["t"].as_str().unwrap_or("?"), &mut out);
        for o in out {
            // normalise array indices already stripped by `[]`
            dropped.insert(o);
        }
    }
    assert!(dropped.is_empty(), "fields present on the wire but not modelled: {dropped:#?}");
}

#[test]
fn prompt_shapes() {
    let p: ServerFrame = serde_json::from_value(json!({
        "t":"prompt","id":7,"kind":"confirm","title":"T","message":"M","yes":"Yes","no":"No","defaultYes":true
    }))
    .unwrap();
    match p {
        ServerFrame::Prompt(p) => {
            assert_eq!(p.id, 7);
            assert_eq!(p.title.as_deref(), Some("T"));
            assert!(matches!(p.body, PromptBody::Confirm { default_yes: true, .. }));
        }
        o => panic!("{o:?}"),
    }
    let f = ServerFrame::parse(r#"{"t":"prompt","id":1,"kind":"someFutureKind","x":1}"#).unwrap();
    assert!(matches!(f, ServerFrame::Prompt(PromptFrame { body: PromptBody::Unknown, .. })));
}

#[test]
fn unknown_and_minimal_frames_never_fail() {
    assert!(matches!(ServerFrame::parse(r#"{"t":"brandNew","a":[1,2]}"#).unwrap(), ServerFrame::Unknown));
    assert!(matches!(ServerFrame::parse(r#"{"t":"flash"}"#).unwrap(), ServerFrame::Flash));
    assert!(matches!(ServerFrame::parse(r#"{"t":"pong"}"#).unwrap(), ServerFrame::Pong));
    assert!(matches!(ServerFrame::parse(r#"{"t":"state"}"#).unwrap(), ServerFrame::State(_)));
    assert!(matches!(ServerFrame::parse(r#"{"t":"input"}"#).unwrap(), ServerFrame::Input(_)));
    assert!(matches!(ServerFrame::parse(r#"{"t":"gameOver","winner":null,"youWon":false}"#).unwrap(), ServerFrame::GameOver { .. }));
    assert!(ServerFrame::parse("not json").is_err());
}

#[test]
fn events_frame_parses() {
    let f = ServerFrame::parse(r#"{"t":"events","seq":3,"items":[{"kind":"damage","source":4,"targetCard":5,"amount":3},{"kind":"lifeChanged","targetPlayer":1,"amount":-3},{"kind":"zoneChange","card":9,"zoneFrom":"Hand","zoneTo":"Battlefield"}]}"#).unwrap();
    match f {
        ServerFrame::Events { seq, items } => {
            assert_eq!(seq, 3);
            assert_eq!(items[0].target_card, Some(5));
            assert_eq!(items[1].amount, Some(-3));
            assert_eq!(items[2].zone_to.as_deref(), Some("Battlefield"));
        }
        o => panic!("{o:?}"),
    }
}

fn j(f: ClientFrame) -> Value {
    serde_json::from_str(&f.to_json()).unwrap()
}

#[test]
fn client_frames_match_protocol() {
    assert_eq!(j(ClientFrame::click_card(5)), json!({"t":"click","action":"card","id":5}));
    assert_eq!(j(ClientFrame::click_player(1)), json!({"t":"click","action":"player","id":1}));
    assert_eq!(j(ClientFrame::ok()), json!({"t":"click","action":"ok"}));
    assert_eq!(j(ClientFrame::cancel()), json!({"t":"click","action":"cancel"}));
    assert_eq!(j(ClientFrame::Click { action: ClickAction::AlphaStrike, id: None }), json!({"t":"click","action":"alphaStrike"}));
    assert_eq!(j(ClientFrame::Click { action: ClickAction::Pass, id: None }), json!({"t":"click","action":"pass"}));
    assert_eq!(j(ClientFrame::Click { action: ClickAction::Undo, id: None }), json!({"t":"click","action":"undo"}));
    assert_eq!(j(ClientFrame::ClearYields), json!({"t":"clearYields"}));
    assert_eq!(j(ClientFrame::Concede), json!({"t":"concede"}));
    assert_eq!(j(ClientFrame::Resync), json!({"t":"resync"}));
    assert_eq!(j(ClientFrame::Ping), json!({"t":"ping"}));
    assert_eq!(j(ClientFrame::answer(3, json!([0, 2]))), json!({"t":"answer","id":3,"value":[0,2]}));
    assert_eq!(j(ClientFrame::answer(3, Value::Null)), json!({"t":"answer","id":3,"value":null}));
    assert_eq!(j(ClientFrame::Yield { key: "k".into(), auto: true }), json!({"t":"yield","key":"k","auto":true}));
    assert_eq!(
        j(ClientFrame::Settings { stops: StopsPatch { my_turn: Some(vec![PhaseType::Main1, PhaseType::CombatDeclareBlockers]), opp_turn: None } }),
        json!({"t":"settings","stops":{"myTurn":["MAIN1","COMBAT_DECLARE_BLOCKERS"]}})
    );
}

#[test]
fn client_frames_roundtrip() {
    for f in [ClientFrame::ok(), ClientFrame::click_card(5), ClientFrame::ClearYields, ClientFrame::Resync, ClientFrame::answer(1, json!({"order":[1,2],"remember":false}))] {
        let back: ClientFrame = serde_json::from_str(&f.to_json()).unwrap();
        assert_eq!(back, f);
    }
}

#[test]
fn rest_payloads() {
    let c = CreateMatch { human_deck: "a".into(), ai_deck: "b".into(), format: Format::Commander, seed: Some(5), human_name: None, human_avatar: None, setup: Default::default() };
    assert_eq!(serde_json::to_value(&c).unwrap(), json!({"humanDeck":"a","aiDeck":"b","format":"commander","seed":5}));
    let d: DeckInfo = serde_json::from_str(r#"{"ref":"precon:X","name":"X","commanders":["C"],"mainCount":99,"colorIdentity":"WB"}"#).unwrap();
    assert_eq!(d.deck_ref, "precon:X");
    assert_eq!(d.main_count, 99);
    let m: MatchInfo = serde_json::from_str(r#"{"id":"m1","humanDeck":"a","aiDeck":"b","started":true,"over":false,"sockets":1,"turn":3,"gameOver":false}"#).unwrap();
    assert_eq!(m.id, "m1");
    assert_eq!(m.turn, Some(3));
}
