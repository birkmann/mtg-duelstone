//! Pure view-models for the action buttons and the prompt bar: Forge's labels / messages in,
//! short player wording out. Kept ECS-free so the mapping is unit-tested.

use mtg_protocol::{InputFrame, PhaseType};

use crate::shared::{CurrentInput, GameView};

#[derive(Debug, Clone, PartialEq)]
pub struct PrimaryModel {
    pub label: String,
    pub subtitle: Option<String>,
    pub enabled: bool,
    /// Calm (grey) look — nothing for the player to decide, e.g. opponent's turn.
    pub passive: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecondaryKind {
    /// Fast-forward: Forge's cancel at priority ("End Turn") = pass to end of turn / next stop.
    FastForward,
    /// Forge's cancel while declaring attackers is "Alpha Strike".
    AttackAll,
    Generic,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SecondaryModel {
    pub label: String,
    pub kind: SecondaryKind,
    pub enabled: bool,
}

pub fn is_priority(msg: &str) -> bool {
    msg.trim_start().to_lowercase().starts_with("priority")
}

pub fn is_mana_payment(msg: &str) -> bool {
    msg.to_lowercase().contains("pay mana cost")
}

pub fn next_phase_subtitle(phase: PhaseType) -> &'static str {
    match phase {
        PhaseType::Untap => "To Upkeep",
        PhaseType::Upkeep => "To Draw",
        PhaseType::Draw => "To Main",
        PhaseType::Main1 => "To Combat",
        PhaseType::CombatBegin => "To Attackers",
        PhaseType::CombatDeclareAttackers => "To Blockers",
        PhaseType::CombatDeclareBlockers => "To Damage",
        PhaseType::CombatFirstStrikeDamage => "To Damage",
        PhaseType::CombatDamage => "To End Combat",
        PhaseType::CombatEnd => "To Main 2",
        PhaseType::Main2 => "To End Step",
        PhaseType::EndOfTurn => "To Cleanup",
        PhaseType::Cleanup => "To Next Turn",
    }
}

fn count_blockers(view: &GameView) -> usize {
    view.game
        .combat
        .as_ref()
        .map(|c| c.attackers.iter().map(|a| a.blockers.len().max(a.planned_blockers.len())).sum())
        .unwrap_or(0)
}

pub fn primary_model(view: &GameView, input: &CurrentInput) -> PrimaryModel {
    let Some(f) = input.frame.as_ref() else {
        return PrimaryModel { label: "Waiting".into(), subtitle: None, enabled: false, passive: true };
    };
    let enabled = f.ok.enabled && !input.awaiting;
    let phase = view.phase();
    let my_turn = view.my_turn();
    let raw = f.ok.label.trim();
    let msg = f.message.as_str();
    let (label, subtitle, passive): (String, Option<String>, bool) = if is_priority(msg) {
        if !view.game.stack.is_empty() {
            ("Resolve".into(), view.game.stack.first().map(|s| trim_len(&stack_name(s), 26)), false)
        } else if my_turn {
            match phase {
                Some(PhaseType::Main2) => ("End Turn".into(), None, false),
                Some(p) => ("Next".into(), Some(next_phase_subtitle(p).into()), false),
                None => ("Next".into(), None, false),
            }
        } else {
            // name the step: a bare "Opponent's Turn" left players wondering why they were asked
            let at = phase.map(|p| format!("Opponent's {}", p.label())).unwrap_or_else(|| "Opponent's Turn".into());
            ("Pass".into(), Some(at), true)
        }
    } else if is_mana_payment(msg) {
        (if raw.eq_ignore_ascii_case("auto") { "Auto Pay".into() } else { raw.to_string() }, Some("Pay mana".into()), false)
    } else if my_turn && phase == Some(PhaseType::CombatDeclareAttackers) && raw.eq_ignore_ascii_case("ok") {
        // A defending planeswalker is highlighted too; only my own picked creatures are attackers.
        let n = f.highlighted.iter().filter(|&&id| view.card(id).is_some_and(|c| c.controller.is_some_and(|p| view.is_me(p)))).count();
        if n == 0 { ("No Attack".into(), None, false) } else { ("Attack".into(), Some(format!("with {n}")), false) }
    } else if !my_turn && phase == Some(PhaseType::CombatDeclareBlockers) && raw.eq_ignore_ascii_case("ok") {
        let n = count_blockers(view);
        if n == 0 { ("No Blocks".into(), None, false) } else { ("Done".into(), Some(format!("{n} blocking")), false) }
    } else if raw.is_empty() {
        ("OK".into(), None, false)
    } else {
        (raw.to_string(), None, false)
    };
    PrimaryModel { label, subtitle, enabled, passive }
}

fn stack_name(s: &mtg_protocol::StackItem) -> String {
    s.source.as_ref().map(|c| c.display_name().to_string()).filter(|n| !n.is_empty()).unwrap_or_else(|| s.text.clone())
}

fn trim_len(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { format!("{}…", s.chars().take(n - 1).collect::<String>()) }
}

pub fn secondary_model(input: &CurrentInput) -> Option<SecondaryModel> {
    let f: &InputFrame = input.frame.as_ref()?;
    let label = f.cancel.label.trim();
    if label.is_empty() {
        return None;
    }
    let lower = label.to_lowercase();
    let kind = if lower.contains("alpha") {
        SecondaryKind::AttackAll
    } else if is_priority(&f.message) || lower.contains("end turn") || lower.contains("pass") {
        SecondaryKind::FastForward
    } else {
        SecondaryKind::Generic
    };
    let label = if kind == SecondaryKind::AttackAll { "Attack All".to_string() } else { label.to_string() };
    Some(SecondaryModel { label, kind, enabled: f.cancel.enabled && !input.awaiting })
}

/// Strip Forge's "Priority/Turn/Phase/Stack" boilerplate; keep what the player must act on.
/// `subject` = name of the card the input is about (targeting / paying): its description line is
/// replaced by the name so the bar stays one short sentence.
pub fn clean_message(msg: &str, subject: Option<&str>) -> Option<String> {
    let mut keep: Vec<String> = Vec::new();
    for line in msg.lines() {
        let l = line.trim();
        if l.is_empty() {
            continue;
        }
        let lower = l.to_lowercase();
        if ["priority:", "turn:", "phase:", "stack:", "active player"].iter().any(|p| lower.starts_with(p)) {
            continue;
        }
        keep.push(prettify_line(l));
    }
    if keep.is_empty() {
        return None;
    }
    if let (Some(name), true) = (subject, keep.len() > 1) {
        // first line(s) describe the card; the last one is the instruction
        let last = keep.last().cloned().unwrap_or_default();
        return Some(format!("{name}: {last}"));
    }
    Some(keep.join(" - "))
}

fn qualifier_words(q: &str) -> String {
    match q {
        "YouCtrl" => "you control".into(),
        "OppCtrl" => "an opponent controls".into(),
        "YouDontCtrl" => "you don't control".into(),
        "Other" => "other".into(),
        "nonLand" => "nonland".into(),
        "nonCreature" => "noncreature".into(),
        "nonArtifact" => "nonartifact".into(),
        other => {
            // camelCase -> words
            let mut out = String::new();
            for (i, ch) in other.chars().enumerate() {
                if ch.is_uppercase() && i > 0 {
                    out.push(' ');
                }
                out.extend(ch.to_lowercase());
            }
            out
        }
    }
}

/// "Select target Permanent.nonLand" -> "Select target nonland permanent".
fn prettify_target(l: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    for w in l.split_whitespace() {
        if let Some((noun, quals)) = w.trim_end_matches('.').split_once('.') {
            let mut pre: Vec<String> = Vec::new();
            let mut post: Vec<String> = Vec::new();
            for q in quals.split(['+', ',', '!']).filter(|q| !q.is_empty()) {
                let words = qualifier_words(q);
                if q.contains("Ctrl") || q.contains("Control") { post.push(words) } else { pre.push(words) }
            }
            let mut s = pre.join(" ");
            if !s.is_empty() {
                s.push(' ');
            }
            s.push_str(&noun.to_lowercase());
            if !post.is_empty() {
                s.push(' ');
                s.push_str(&post.join(" "));
            }
            out.push(s);
        } else {
            out.push(w.to_string());
        }
    }
    out.join(" ")
}

fn prettify_line(l: &str) -> String {
    let lower = l.to_lowercase();
    if lower.contains("declare attackers") || lower.contains("select creatures to attack") {
        return "Declare attackers".into();
    }
    if lower.contains("declare blockers") {
        return "Declare blockers".into();
    }
    prettify_target(l.trim_end_matches('.'))
}

/// Pay Mana Cost remaining, e.g. `Pay Mana Cost: {2}{R}` → "{2}{R}".
pub fn mana_to_pay(msg: &str) -> Option<String> {
    let lower = msg.to_lowercase();
    let i = lower.find("pay mana cost")?;
    let rest = &msg[i..];
    let start = rest.find('{')?;
    let mut end = start;
    let b = rest.as_bytes();
    while end < rest.len() && b[end] == b'{' {
        let close = rest[end..].find('}')?;
        end += close + 1;
    }
    Some(rest[start..end].to_string())
}

/// Phases grouped for the compact track.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Begin,
    Main1,
    Combat,
    Main2,
    End,
}

pub fn group_of(p: PhaseType) -> Group {
    match p {
        PhaseType::Untap | PhaseType::Upkeep | PhaseType::Draw => Group::Begin,
        PhaseType::Main1 => Group::Main1,
        p if p.is_combat() => Group::Combat,
        PhaseType::Main2 => Group::Main2,
        _ => Group::End,
    }
}

pub fn short_label(p: PhaseType) -> &'static str {
    match p {
        PhaseType::Untap => "UNT",
        PhaseType::Upkeep => "UPK",
        PhaseType::Draw => "DRW",
        PhaseType::Main1 => "M1",
        PhaseType::CombatBegin => "BEG",
        PhaseType::CombatDeclareAttackers => "ATK",
        PhaseType::CombatDeclareBlockers => "BLK",
        PhaseType::CombatFirstStrikeDamage => "FS",
        PhaseType::CombatDamage => "DMG",
        PhaseType::CombatEnd => "END",
        PhaseType::Main2 => "M2",
        PhaseType::EndOfTurn => "EOT",
        PhaseType::Cleanup => "CLN",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mtg_protocol::{ButtonState, Card, Game, Player, Zones};

    fn view(phase: PhaseType, my_turn: bool) -> GameView {
        GameView {
            seq: 1,
            you: Some(1),
            game: Game { phase: Some(phase), player_turn: Some(if my_turn { 1 } else { 2 }), ..Default::default() },
            ..Default::default()
        }
    }
    fn input(msg: &str, ok: &str, cancel: &str) -> CurrentInput {
        CurrentInput {
            frame: Some(InputFrame {
                message: msg.into(),
                ok: ButtonState { label: ok.into(), enabled: true },
                cancel: ButtonState { label: cancel.into(), enabled: true },
                ..Default::default()
            }),
            awaiting: false,
        }
    }

    #[test]
    fn priority_main1_is_next_to_combat() {
        let m = primary_model(&view(PhaseType::Main1, true), &input("Priority: You\nTurn: 3 (You)\nPhase: Main phase, precombat\nStack: Empty", "OK", "End Turn"));
        assert_eq!((m.label.as_str(), m.subtitle.as_deref()), ("Next", Some("To Combat")));
        assert!(m.enabled);
    }

    #[test]
    fn main2_is_end_turn_and_opp_turn_is_pass() {
        let i = input("Priority: You", "OK", "End Turn");
        assert_eq!(primary_model(&view(PhaseType::Main2, true), &i).label, "End Turn");
        let m = primary_model(&view(PhaseType::Upkeep, false), &i);
        assert_eq!((m.label.as_str(), m.subtitle.as_deref()), ("Pass", Some("Opponent's Upkeep")));
        assert!(m.passive);
    }

    #[test]
    fn attackers_and_blockers_wording() {
        let mut i = input("Declare attackers", "OK", "Alpha Strike");
        let mut v = view(PhaseType::CombatDeclareAttackers, true);
        let card = |id, controller| Card { id, controller: Some(controller), ..Default::default() };
        v.game.players = vec![
            Player { id: 1, zones: Zones { battlefield: vec![card(5, 1), card(6, 1)], ..Default::default() }, ..Default::default() },
            Player { id: 2, zones: Zones { battlefield: vec![card(9, 2)], ..Default::default() }, ..Default::default() },
        ];
        // Forge highlights the current defender (here a planeswalker): that alone is still "No Attack".
        i.frame.as_mut().unwrap().highlighted = vec![9];
        assert_eq!(primary_model(&v, &i).label, "No Attack");
        i.frame.as_mut().unwrap().highlighted = vec![9, 5, 6];
        let m = primary_model(&v, &i);
        assert_eq!((m.label.as_str(), m.subtitle.as_deref()), ("Attack", Some("with 2")));
        assert_eq!(secondary_model(&i).unwrap().kind, SecondaryKind::AttackAll);
        let b = primary_model(&view(PhaseType::CombatDeclareBlockers, false), &input("Declare blockers", "OK", "Cancel"));
        assert_eq!(b.label, "No Blocks");
    }

    #[test]
    fn awaiting_disables() {
        let mut i = input("Priority: You", "OK", "End Turn");
        i.awaiting = true;
        assert!(!primary_model(&view(PhaseType::Main1, true), &i).enabled);
        assert!(!secondary_model(&i).unwrap().enabled);
    }

    #[test]
    fn message_cleanup() {
        assert_eq!(clean_message("Priority: You\nTurn: 3 (You)\nPhase: Main\nStack: Empty", None), None);
        assert_eq!(clean_message("Choose targets for Shock.\nPriority: x", None).as_deref(), Some("Choose targets for Shock"));
        assert_eq!(
            clean_message("Anguished Unmaking (42) - Exile target nonland permanent.\n\nSelect target Permanent.nonLand", Some("Anguished Unmaking")).as_deref(),
            Some("Anguished Unmaking: Select target nonland permanent")
        );
        assert_eq!(prettify_target("Select target Creature.YouDontCtrl"), "Select target creature you don't control");
        assert_eq!(mana_to_pay("Pay Mana Cost: {2}{R}{R} more").as_deref(), Some("{2}{R}{R}"));
    }
}
