//! Turns the raw `ServerMsg` stream into the read-only game view the rest of the app consumes,
//! and diffs consecutive `state` frames into `BoardEvent`s (fx/audio/motion cues).
//!
//! The diff is a pure function over two `GameView`s so it can be unit-tested; the engine's own
//! `events` frames are forwarded verbatim as `BoardEvent::Cue` for precise damage attribution.

use std::collections::HashMap;

use bevy::prelude::*;
use mtg_protocol::{CardId, ClientFrame, ClickAction, PlayerId, ServerFrame, ZoneKind};

use crate::layout::collect_cards;
use crate::shared::*;

pub struct ViewPlugin;

impl Plugin for ViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<ServerMsg>()
            .add_message::<SendCmd>()
            .add_message::<BoardEvent>()
            .init_resource::<GameView>()
            .init_resource::<CurrentInput>()
            .init_resource::<PendingPrompt>()
            .init_resource::<GameLog>()
            .init_resource::<GameResult>()
            .init_resource::<CardCache>()
            .init_resource::<ViewTracking>()
            .init_resource::<SeatAvatars>()
            .add_systems(Update, apply_frames.in_set(ViewSet))
            .add_systems(OnEnter(AppState::Loading), reset_view);
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct ViewSet;

#[derive(Resource, Default)]
pub struct ViewTracking {
    last_clicked: Option<CardId>,
    prompt_input_seq: i64,
}

pub fn reset_view(
    mut view: ResMut<GameView>,
    mut input: ResMut<CurrentInput>,
    mut prompt: ResMut<PendingPrompt>,
    mut log: ResMut<GameLog>,
    mut result: ResMut<GameResult>,
    mut cache: ResMut<CardCache>,
    mut tracking: ResMut<ViewTracking>,
    mut avatars: ResMut<SeatAvatars>,
) {
    *view = GameView::default();
    avatars.0.clear();
    *input = CurrentInput::default();
    prompt.0 = None;
    log.0.clear();
    result.0 = None;
    cache.0.clear();
    *tracking = ViewTracking::default();
}

fn apply_frames(
    mut msgs: MessageReader<ServerMsg>,
    mut cmds: MessageReader<SendCmd>,
    mut view: ResMut<GameView>,
    mut input: ResMut<CurrentInput>,
    mut prompt: ResMut<PendingPrompt>,
    mut log: ResMut<GameLog>,
    mut result: ResMut<GameResult>,
    mut cache: ResMut<CardCache>,
    mut tracking: ResMut<ViewTracking>,
    mut events: MessageWriter<BoardEvent>,
    mut avatars: ResMut<SeatAvatars>,
) {
    for SendCmd(c) in cmds.read() {
        match c {
            ClientFrame::Click { action: ClickAction::Card, id } => tracking.last_clicked = *id,
            ClientFrame::Answer { id, .. } if prompt.0.as_ref().is_some_and(|p| p.id == *id) => prompt.0 = None,
            _ => {}
        }
    }
    for ServerMsg(frame) in msgs.read() {
        match frame {
            ServerFrame::State(s) => {
                let mut new = GameView {
                    seq: s.seq.max(1),
                    you: s.you.or(view.you),
                    format: if s.game.is_commander { mtg_protocol::Format::Commander } else { view.format },
                    stops: s.stops.clone(),
                    auto_yields: s.auto_yields.clone(),
                    game: s.game.clone(),
                };
                if !s.game.is_commander && view.format == mtg_protocol::Format::Commander {
                    new.format = mtg_protocol::Format::Commander;
                }
                if view.has_state() {
                    for e in diff(&view, &new) {
                        events.write(e);
                    }
                }
                for s in collect_cards(&new) {
                    let c = s.card;
                    if c.is_visible() {
                        cache.0.insert(c.id, c.clone());
                    }
                }
                *view = new;
                // At a multiplayer table the game goes on after we are out; that is our game over.
                if view.is_multiplayer() && result.0.is_none() && view.me().is_some_and(|p| p.has_lost) {
                    let winner = view.game.winner.clone();
                    result.0 = Some(MatchOutcome { winner, you_won: false });
                    events.write(BoardEvent::GameOver { you_won: false });
                }
            }
            ServerFrame::Input(i) => {
                if prompt.0.is_some() && i.seq != tracking.prompt_input_seq {
                    prompt.0 = None;
                }
                input.frame = Some(i.clone());
                input.awaiting = false;
                if let Some(c) = &i.card_view {
                    if c.is_visible() {
                        cache.0.entry(c.id).or_insert_with(|| c.clone());
                    }
                }
                for c in &i.tray_cards {
                    if c.is_visible() {
                        cache.0.insert(c.id, c.clone());
                    }
                }
            }
            ServerFrame::Prompt(p) => {
                tracking.prompt_input_seq = input.frame.as_ref().map(|f| f.seq).unwrap_or(0);
                prompt.0 = Some(p.clone());
            }
            ServerFrame::Log { entries } => {
                log.0.extend(entries.iter().cloned());
                let n = log.0.len();
                if n > 600 {
                    log.0.drain(0..n - 600);
                }
            }
            ServerFrame::Message { title, text, error } => {
                let i = log.0.len() as i64;
                log.0.push(mtg_protocol::LogEntry {
                    i,
                    kind: if *error { "error".into() } else { "message".into() },
                    text: if title.is_empty() { text.clone() } else { format!("{title}: {text}") },
                });
            }
            ServerFrame::GameOver { winner, you_won } => {
                result.0 = Some(MatchOutcome { winner: winner.clone(), you_won: *you_won });
                input.awaiting = false;
                events.write(BoardEvent::GameOver { you_won: *you_won });
            }
            ServerFrame::Events { items, .. } => {
                for it in items {
                    events.write(BoardEvent::Cue(it.clone()));
                }
            }
            ServerFrame::Flash => {
                input.awaiting = false;
                events.write(BoardEvent::Rejected { card: tracking.last_clicked });
            }
            ServerFrame::Alert => {
                events.write(BoardEvent::Alert);
            }
            ServerFrame::Hello { format, seats, .. } => {
                avatars.0 = seats.iter().filter_map(|s| Some((s.name.clone()?, s.avatar.clone()?))).collect();
                if format.as_deref() == Some("commander") {
                    view.format = mtg_protocol::Format::Commander;
                } else if format.as_deref() == Some("constructed") {
                    view.format = mtg_protocol::Format::Constructed;
                }
            }
            ServerFrame::Error { text } => warn!("engine error: {text}"),
            // lobby frames are app_state's (LobbyView); the game view starts with `hello`
            // lobby/event frames belong to app_state and the event screens; the game view starts with `hello`
            ServerFrame::Pong
            | ServerFrame::Lobby(_)
            | ServerFrame::Event(_)
            | ServerFrame::EventMatch(_)
            | ServerFrame::DeckSuggestion { .. }
            | ServerFrame::Unknown => {}
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Snap {
    zone: ZoneKind,
    owner: Option<PlayerId>,
    tapped: bool,
    damage: i32,
}

fn snapshot(v: &GameView) -> HashMap<CardId, Snap> {
    collect_cards(v)
        .into_iter()
        .map(|s| {
            let c = s.card;
            (c.id, Snap { zone: s.zone, owner: c.owner.or(s.player), tapped: c.tapped, damage: c.damage })
        })
        .collect()
}

/// Board events between two consecutive views. Order: structure first (moves), then numbers.
pub fn diff(old: &GameView, new: &GameView) -> Vec<BoardEvent> {
    let mut out = Vec::new();
    let (a, b) = (snapshot(old), snapshot(new));
    // deterministic order
    let mut ids: Vec<&CardId> = b.keys().collect();
    ids.sort();
    for id in ids {
        let n = &b[id];
        match a.get(id) {
            None => out.push(BoardEvent::CardAppeared { id: *id, zone: n.zone, owner: n.owner }),
            Some(o) => {
                if o.zone != n.zone {
                    out.push(BoardEvent::CardMoved { id: *id, from: o.zone, to: n.zone, owner: n.owner });
                }
                if o.tapped != n.tapped {
                    out.push(BoardEvent::CardTapped { id: *id, tapped: n.tapped });
                }
                if n.damage > o.damage {
                    out.push(BoardEvent::CardDamaged { id: *id, amount: n.damage - o.damage });
                }
            }
        }
    }
    let mut gone: Vec<&CardId> = a.keys().filter(|id| !b.contains_key(id)).collect();
    gone.sort();
    for id in gone {
        out.push(BoardEvent::CardRemoved { id: *id });
    }
    for p in &new.game.players {
        if let Some(o) = old.game.player(p.id) {
            if o.life != p.life {
                out.push(BoardEvent::LifeChanged { player: p.id, old: o.life, new: p.life });
            }
        }
    }
    if new.game.turn != old.game.turn {
        if let Some(player) = new.game.player_turn {
            out.push(BoardEvent::TurnStarted { player, turn: new.game.turn });
        }
    }
    if new.game.phase != old.game.phase || new.game.player_turn != old.game.player_turn {
        if let Some(phase) = new.game.phase {
            out.push(BoardEvent::PhaseChanged { phase, player: new.game.player_turn });
        }
    }
    let src = |s: &mtg_protocol::StackItem| s.source.as_ref().map(|c| c.id);
    for s in &new.game.stack {
        if !old.game.stack.iter().any(|o| o.id == s.id) {
            out.push(BoardEvent::StackPushed { item: s.id, source: src(s) });
        }
    }
    for s in &old.game.stack {
        if !new.game.stack.iter().any(|o| o.id == s.id) {
            out.push(BoardEvent::StackResolved { item: s.id, source: src(s) });
        }
    }
    let atk = |v: &GameView| -> Vec<CardId> {
        v.game.combat.as_ref().map(|c| c.attackers.iter().map(|a| a.attacker).collect()).unwrap_or_default()
    };
    let (oa, na) = (atk(old), atk(new));
    if !na.is_empty() && na.iter().any(|id| !oa.contains(id)) {
        out.push(BoardEvent::AttackersDeclared { attackers: na });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use mtg_protocol::{Card, CardState, Game, Player, StackItem, Zones};

    fn card(id: i64, zone: &str) -> Card {
        Card {
            id,
            zone: Some(zone.into()),
            owner: Some(1),
            state: Some(CardState { name: format!("c{id}"), ..default() }),
            ..default()
        }
    }
    fn view(hand: Vec<Card>, bf: Vec<Card>, life: i32) -> GameView {
        GameView {
            seq: 1,
            you: Some(1),
            game: Game {
                turn: 1,
                players: vec![Player { id: 1, life, zones: Zones { hand, battlefield: bf, ..default() }, ..default() }],
                ..default()
            },
            ..default()
        }
    }

    #[test]
    fn move_tap_life_remove() {
        let old = view(vec![card(1, "Hand"), card(2, "Hand")], vec![], 20);
        let mut tapped = card(1, "Battlefield");
        tapped.tapped = true;
        let new = view(vec![], vec![tapped], 17);
        let ev = diff(&old, &new);
        assert!(ev.iter().any(|e| matches!(e, BoardEvent::CardMoved { id: 1, from: ZoneKind::Hand, to: ZoneKind::Battlefield, .. })));
        assert!(ev.iter().any(|e| matches!(e, BoardEvent::CardTapped { id: 1, tapped: true })));
        assert!(ev.iter().any(|e| matches!(e, BoardEvent::LifeChanged { player: 1, old: 20, new: 17 })));
        assert!(ev.iter().any(|e| matches!(e, BoardEvent::CardRemoved { id: 2 })));
    }

    #[test]
    fn appear_damage_stack() {
        let old = view(vec![], vec![card(5, "Battlefield")], 20);
        let mut hurt = card(5, "Battlefield");
        hurt.damage = 2;
        let mut new = view(vec![card(7, "Hand")], vec![hurt], 20);
        new.game.stack.push(StackItem { id: 11, source: Some(card(9, "Stack")), ..default() });
        let ev = diff(&old, &new);
        assert!(ev.iter().any(|e| matches!(e, BoardEvent::CardAppeared { id: 7, .. })));
        assert!(ev.iter().any(|e| matches!(e, BoardEvent::CardDamaged { id: 5, amount: 2 })));
        assert!(ev.iter().any(|e| matches!(e, BoardEvent::StackPushed { item: 11, source: Some(9) })));
        // spell card 9 appears in the stack zone
        assert!(ev.iter().any(|e| matches!(e, BoardEvent::CardAppeared { id: 9, zone: ZoneKind::Stack, .. })));
        // reverse: it resolves
        let back = diff(&new, &old);
        assert!(back.iter().any(|e| matches!(e, BoardEvent::StackResolved { item: 11, .. })));
    }

    #[test]
    fn identical_views_emit_nothing() {
        let v = view(vec![card(1, "Hand")], vec![], 20);
        assert!(diff(&v, &v.clone()).is_empty());
    }
}
