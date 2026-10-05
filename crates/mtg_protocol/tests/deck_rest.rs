//! Deck-builder REST payloads recorded from a real engine (`/api/deck`, `/api/deck/recommend`):
//! they must decode into the typed structs with nothing important silently defaulted.

use std::path::PathBuf;

use mtg_protocol::*;

fn load<T: serde::de::DeserializeOwned>(name: &str) -> T {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
    serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap_or_else(|e| panic!("{name}: {e}"))
}

#[test]
fn commander_recommendations_decode() {
    let r: Recommendations = load("rest_recommend_commander.json");
    assert_eq!(r.format, Some(Format::Commander));
    assert_eq!(r.color_identity, "WUBG");
    assert!(r.commander_decks > 1000);
    assert!(r.themes.iter().any(|t| t.key == "proliferate" && t.active));
    let syn = r.sections.iter().find(|s| s.key == "synergy").expect("synergy section");
    let first = &syn.cards[0];
    assert!(!first.card.name.is_empty() && first.inclusion > 0.0 && first.synergy > 0.0);
    assert!(!first.card.tags.is_empty());
    // the deck's own cards are never suggested back
    assert!(r.sections.iter().flat_map(|s| &s.cards).all(|c| c.card.name != "Sol Ring"));
}

#[test]
fn constructed_recommendations_decode() {
    let r: Recommendations = load("rest_recommend_constructed.json");
    assert_eq!(r.format, Some(Format::Constructed));
    assert_eq!(r.commander_decks, 0);
    assert!(!r.archetypes.is_empty() && r.archetypes[0].decks > 0);
    assert!(r.sections.iter().any(|s| s.key == "synergy" && !s.cards.is_empty()));
}

#[test]
fn full_deck_decodes() {
    let d: DeckFull = load("rest_deck_constructed.json");
    assert_eq!(d.deck_ref, "starter:mono-red-aggro");
    assert!(d.problem.is_none());
    assert_eq!(d.main.iter().map(|e| e.qty).sum::<u32>(), 60);
    let mtn = d.main.iter().find(|e| e.card.name == "Mountain").unwrap();
    assert!(mtn.card.is_basic_land && mtn.card.primary_type() == CardKind::Land);
    assert!(d.main.iter().any(|e| e.card.primary_type() == CardKind::Creature && e.card.mana_cost.is_some()));
}

#[test]
fn deck_body_serializes_like_protocol_md() {
    let b = DeckBody {
        name: "Burn".into(),
        format: Format::Constructed,
        main: vec![DeckLine { name: "Lightning Bolt".into(), qty: 4 }],
        ..Default::default()
    };
    let v = serde_json::to_value(&b).unwrap();
    assert_eq!(v["format"], "constructed");
    assert_eq!(v["main"][0]["qty"], 4);
    assert!(v.get("previousName").is_none());
}
