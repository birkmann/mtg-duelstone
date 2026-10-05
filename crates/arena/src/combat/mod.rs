//! Combat presentation: attackers slide toward the centre line (tilted, slightly enlarged),
//! blockers step up, red ribbons run attacker → defender and orange/blue ribbons blocker →
//! attacker. While I declare attackers, clicked creatures move *immediately* (optimistic) and
//! settle to engine truth when the next input arrives.
//!
//! Positions are only ever `CombatPose` contributions; `interact::compose_pose` merges them with
//! hover/fx into the single `PoseOffset` motion reads.

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use mtg_protocol::{CardId, ClickAction, ClientFrame, PhaseType};

use crate::hud::kit::{Act, Acted};
use crate::hud::model::is_priority;
use crate::shared::*;
use crate::targeting::{Arrow, ArrowQueue, card_anchor, cursor_on_table, player_anchor};

#[derive(Component, Debug, Clone, Copy, PartialEq, Default)]
pub struct CombatPose(pub PoseOffset);

/// Eased 0..1 amounts per card (attacking / blocking).
#[derive(Component, Default)]
pub struct CombatAmt {
    attack: f32,
    block: f32,
}

/// Optimistic attacker toggles made by clicks since the last engine input.
#[derive(Resource, Default)]
pub struct OptimisticAttack {
    pub toggles: HashMap<CardId, bool>,
    pub input_seq: i64,
    pub age: f32,
}

pub struct CombatPlugin;

impl Plugin for CombatPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OptimisticAttack>()
            .init_resource::<CurrentInput>()
            .add_systems(
                Update,
                (ensure_components, track_optimistic, combat_pose, combat_arrows).chain().run_if(in_state(AppState::InMatch).or_else(in_state(AppState::GameOver))),
            );
    }
}

fn ensure_components(mut commands: Commands, added: Query<Entity, Added<CardEntity>>) {
    for e in &added {
        commands.entity(e).insert((CombatPose::default(), CombatAmt::default()));
    }
}

/// Are we in my declare-attackers decision (as opposed to priority inside that step)?
pub fn declaring_attackers(view: &GameView, input: &CurrentInput) -> bool {
    view.my_turn()
        && view.phase() == Some(PhaseType::CombatDeclareAttackers)
        && input.frame.as_ref().is_some_and(|f| !is_priority(&f.message) && (f.ok.enabled || !f.selectable.is_empty()))
}

pub fn declaring_blockers(view: &GameView, input: &CurrentInput) -> bool {
    !view.my_turn()
        && view.phase() == Some(PhaseType::CombatDeclareBlockers)
        && input.frame.as_ref().is_some_and(|f| !is_priority(&f.message))
}

/// Engine truth ∪ currently-picked, then optimistic toggles on top.
pub fn attacking_set(view: &GameView, input: &CurrentInput, opt: &OptimisticAttack) -> HashSet<CardId> {
    let mut s: HashSet<CardId> = view.game.combat.as_ref().map(|c| c.attackers.iter().map(|a| a.attacker).collect()).unwrap_or_default();
    if declaring_attackers(view, input)
        && let Some(f) = input.frame.as_ref()
    {
        s.extend(f.highlighted.iter().copied());
    }
    for (id, on) in &opt.toggles {
        if *on {
            s.insert(*id);
        } else {
            s.remove(id);
        }
    }
    s
}

fn track_optimistic(
    time: Res<Time>,
    view: Res<GameView>,
    input: Res<CurrentInput>,
    mut opt: ResMut<OptimisticAttack>,
    mut sent: MessageReader<SendCmd>,
    mut acts: MessageReader<Acted>,
) {
    let seq = input.frame.as_ref().map(|f| f.seq).unwrap_or(0);
    if seq != opt.input_seq {
        // a fresh engine input is the new truth
        opt.input_seq = seq;
        opt.toggles.clear();
        opt.age = 0.0;
    }
    opt.age += time.delta_secs();
    if opt.age > 1.5 {
        opt.toggles.clear();
    }
    let declaring = declaring_attackers(&view, &input);
    for SendCmd(f) in sent.read() {
        if let ClientFrame::Click { action: ClickAction::Card, id: Some(id) } = f
            && declaring
            && input.is_selectable(*id)
        {
            let now = attacking_set(&view, &input, &opt).contains(id);
            opt.toggles.insert(*id, !now);
            opt.age = 0.0;
        }
    }
    for Acted(a) in acts.read() {
        if matches!(a, Act::AttackAll) || (matches!(a, Act::Secondary) && declaring && crate::hud::model::secondary_model(&input).is_some_and(|s| s.kind == crate::hud::model::SecondaryKind::AttackAll)) {
            if let Some(f) = input.frame.as_ref() {
                for id in &f.selectable {
                    opt.toggles.insert(*id, true);
                }
                opt.age = 0.0;
            }
        }
    }
}

fn combat_pose(
    time: Res<Time>,
    view: Res<GameView>,
    input: Res<CurrentInput>,
    opt: Res<OptimisticAttack>,
    mut q: Query<(&CardEntity, &CardLocation, &mut CombatAmt, &mut CombatPose)>,
) {
    let dt = time.delta_secs();
    let attacking = attacking_set(&view, &input, &opt);
    let blocking: HashSet<CardId> = view
        .game
        .combat
        .as_ref()
        .map(|c| c.attackers.iter().flat_map(|a| a.blockers.iter().chain(a.planned_blockers.iter()).copied()).collect())
        .unwrap_or_default();
    let k = 1.0 - (-dt * 10.0).exp();
    for (ce, loc, mut amt, mut pose) in &mut q {
        let on_bf = loc.zone == mtg_protocol::ZoneKind::Battlefield;
        let want_a = if on_bf && attacking.contains(&ce.0) { 1.0 } else { 0.0 };
        let want_b = if on_bf && blocking.contains(&ce.0) { 1.0 } else { 0.0 };
        amt.attack += (want_a - amt.attack) * k;
        amt.block += (want_b - amt.block) * k;
        if (amt.attack - want_a).abs() < 0.003 {
            amt.attack = want_a;
        }
        if (amt.block - want_b).abs() < 0.003 {
            amt.block = want_b;
        }
        // toward the centre line from either side
        let dir = if loc.mine { -1.0 } else { 1.0 };
        let push = amt.attack * 0.85 + amt.block * 0.42;
        let tilt = amt.attack * 0.10 * -dir;
        let p = PoseOffset {
            translation: Vec3::new(0.0, 0.06 * amt.attack, dir * push),
            rotation: Quat::from_rotation_x(tilt),
            scale: 1.0 + 0.05 * amt.attack,
        };
        let np = CombatPose(p);
        if *pose != np {
            *pose = np;
        }
    }
}

fn combat_arrows(
    time: Res<Time>,
    view: Res<GameView>,
    input: Res<CurrentInput>,
    anchors: Res<BoardAnchors>,
    registry: Res<CardRegistry>,
    cards: Query<(&GlobalTransform, Option<&TargetPlacement>)>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cam: Query<(&Camera, &GlobalTransform), With<TableCamera>>,
    hovered: Query<(&CardEntity, &GlobalTransform), (With<Hovered>, With<Selectable>)>,
    mut queue: ResMut<ArrowQueue>,
) {
    let t = time.elapsed_secs();
    let pulse = 0.5 + 0.5 * (t * 3.0).sin();
    if let Some(combat) = view.game.combat.as_ref() {
        for a in &combat.attackers {
            let Some(from) = card_anchor(a.attacker, &registry, &cards) else { continue };
            if let Some(d) = a.defender.as_ref() {
                let to = if d.kind == "player" {
                    Some(player_anchor(&view, &anchors, d.id))
                } else {
                    card_anchor(d.id, &registry, &cards)
                };
                if let Some(to) = to {
                    let mut ar = Arrow::new(from, to, Color::srgb(1.0, 0.28, 0.12));
                    ar.width = 0.055;
                    ar.intensity = 0.75 + 0.15 * pulse;
                    ar.arc = 0.12;
                    queue.0.push(ar);
                }
            }
            for (list, planned) in [(&a.blockers, false), (&a.planned_blockers, true)] {
                for b in list {
                    if !planned || !a.blockers.contains(b) {
                        let Some(bf) = card_anchor(*b, &registry, &cards) else { continue };
                        let mut ar = if planned {
                            Arrow::new(bf, from, Color::srgb(0.45, 0.75, 1.0))
                        } else {
                            Arrow::new(bf, from, Color::srgb(1.0, 0.72, 0.18))
                        };
                        ar.width = 0.05;
                        ar.dashed = planned;
                        ar.arc = 0.2;
                        ar.intensity = 0.9;
                        queue.0.push(ar);
                    }
                }
            }
        }
    }
    // Block assignment in progress: the picked blocker's arrow follows the cursor to an attacker.
    if declaring_blockers(&view, &input)
        && let Some(f) = input.frame.as_ref()
        && f.card.is_none()
        && f.highlighted.len() == 1
    {
        let picked = f.highlighted[0];
        let assigned = view.game.combat.as_ref().is_some_and(|c| c.attackers.iter().any(|a| a.planned_blockers.contains(&picked) || a.blockers.contains(&picked)));
        if !assigned
            && let Some(from) = card_anchor(picked, &registry, &cards)
        {
            let to = hovered.iter().find(|(c, _)| c.0 != picked).map(|(_, g)| g.translation() + Vec3::Y * 0.1).or_else(|| cursor_on_table(&windows, &cam, 0.3));
            if let Some(to) = to {
                let mut ar = Arrow::new(from, to, Color::srgb(0.45, 0.8, 1.0));
                ar.intensity = 1.0 + 0.2 * pulse;
                queue.0.push(ar);
            }
        }
    }
}
