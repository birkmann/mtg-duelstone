//! Pointer + keyboard interaction.
//!
//! * Pointer observers are attached to every `CardEntity` the board spawns (hover lift + preview,
//!   click → `click_card`, drag a hand card up onto the table to play it, right-click inspect).
//! * `CurrentInput` is mirrored onto cards as `Selectable` / `Highlighted` / `WeakSelectable` and
//!   drawn as an additive glow halo (blue = playable/targetable, gold = picked, soft = weak).
//! * One composer owns `PoseOffset` = hover ∘ combat ∘ fx, so the three writers can't clobber each
//!   other.
//!
//! The client never decides legality: a click is *sent* if the engine marked the card selectable
//! (or, during priority, if it is one of my own cards — the engine then accepts or `flash`es).

use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowCaster;
use bevy::picking::Pickable;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::window::PrimaryWindow;
use mtg_protocol::{CardId, ClientFrame, ZoneKind};

use crate::combat::CombatPose;
use crate::hud::kit::{Act, Acted};
use crate::hud::menu::MenuUi;
use crate::hud::model::{SecondaryKind, secondary_model};
use crate::layout::HAND_VIEW_H;
use crate::shared::*;

pub mod halo;

#[derive(Resource, Default, Debug, Clone)]
pub struct InspectCard(pub Option<CardId>);

#[derive(Resource, Default)]
pub struct DragState {
    pub active: Option<Entity>,
    /// Pixels moved since the press (Click fires before DragEnd, so it checks this).
    pub moved: f32,
    /// Cards released over the table, waiting for the engine's answer: (entity, seconds left).
    pub holding: Vec<(Entity, f32)>,
}

/// Eased hover amount + the pose derived from it. Private: only `compose_pose` publishes it.
#[derive(Component, Default)]
pub struct HoverPose {
    pub amt: f32,
    pub pose: PoseOffset,
}

pub struct InteractPlugin;

impl Plugin for InteractPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<InspectCard>()
            .init_resource::<DragState>()
            .init_resource::<HoverPreview>()
            .init_resource::<CurrentInput>()
            .init_resource::<PendingPrompt>()
            .add_message::<Acted>()
            .add_systems(Startup, halo::setup_halo_assets)
            .add_systems(
                Update,
                (
                    attach_card_observers,
                    mirror_input,
                    halo::update_halos,
                    ease_hover,
                    drag_follow,
                    drag_hold,
                    compose_pose,
                    hotkeys,
                    inspect_acts,
                    awaiting_timeout,
                    shake_rejected,
                    clear_stale_hover,
                )
                    .chain()
                    .run_if(in_state(AppState::InMatch).or_else(in_state(AppState::GameOver))),
            );
    }
}

fn attach_card_observers(
    mut commands: Commands,
    added: Query<Entity, Added<CardEntity>>,
    assets: Option<Res<halo::HaloAssets>>,
) {
    for e in &added {
        commands
            .entity(e)
            .insert(HoverPose::default())
            .observe(on_enter)
            .observe(on_leave)
            .observe(on_click)
            .observe(on_drag_start)
            .observe(on_drag)
            .observe(on_drag_end);
        if let Some(a) = assets.as_ref() {
            halo::spawn_halo(&mut commands, e, a);
        }
    }
}

fn on_enter(
    on: On<Pointer<Enter>>,
    mut commands: Commands,
    cards: Query<&CardEntity>,
    view: Res<GameView>,
    cache: Res<CardCache>,
    drag: Res<DragState>,
    mut preview: ResMut<HoverPreview>,
) {
    let e = on.event_target();
    let Ok(ce) = cards.get(e) else { return };
    if drag.active.is_some() {
        return;
    }
    commands.entity(e).insert(Hovered);
    let visible = view.card(ce.0).or_else(|| cache.0.get(&ce.0)).is_some_and(|c| !c.hidden && c.state.is_some());
    if visible {
        preview.0 = Some(ce.0);
    }
}

fn on_leave(on: On<Pointer<Leave>>, mut commands: Commands, cards: Query<&CardEntity>, mut preview: ResMut<HoverPreview>) {
    let e = on.event_target();
    let Ok(ce) = cards.get(e) else { return };
    commands.entity(e).remove::<Hovered>();
    if preview.0 == Some(ce.0) {
        preview.0 = None;
    }
}

/// Is a click on this card worth sending? The engine's `selectable` is often EMPTY (priority,
/// declare attackers/blockers), so my own hand/battlefield/command cards are always sendable (the
/// engine accepts or `flash`es); the opponent's battlefield only while blocking/targeting.
fn clickable(input: &CurrentInput, view: &GameView, loc: &CardLocation, id: CardId) -> bool {
    let Some(f) = input.frame.as_ref() else { return false };
    if input.is_selectable(id) {
        return true;
    }
    if loc.mine {
        return matches!(loc.zone, ZoneKind::Hand | ZoneKind::Battlefield | ZoneKind::Command | ZoneKind::Graveyard | ZoneKind::Exile);
    }
    loc.zone == ZoneKind::Battlefield && (f.card.is_some() || crate::combat::declaring_blockers(view, input))
}

/// Cosmetic "playable" guess for the blue glow (never gates a click).
pub fn playable_hint(view: &GameView, input: &CurrentInput, card: &mtg_protocol::Card) -> bool {
    use crate::hud::model::is_priority;
    let Some(f) = input.frame.as_ref() else { return false };
    let Some(me) = view.me() else { return false };
    let Some(st) = card.state.as_ref() else { return false };
    if !is_priority(&f.message) || !f.ok.enabled {
        return false;
    }
    let my_main = view.my_turn()
        && view.game.stack.is_empty()
        && matches!(view.phase(), Some(mtg_protocol::PhaseType::Main1 | mtg_protocol::PhaseType::Main2));
    if st.is_land {
        return my_main && me.lands_played_this_turn < me.max_land_plays.max(1);
    }
    let flash = st.is_instant || st.keywords.iter().any(|k| k.eq_ignore_ascii_case("flash"));
    if !(my_main || flash) {
        return false;
    }
    let lands = me.zones.battlefield.iter().filter(|c| !c.tapped && c.state.as_ref().is_some_and(|s| s.is_land)).count() as i32;
    let avail = lands + me.mana_pool.values().sum::<i32>();
    let mv: i32 = crate::hud::kit::mana_symbols(st.mana_cost.as_deref().unwrap_or(""))
        .iter()
        .map(|s| s.parse::<i32>().unwrap_or(if s == "X" { 0 } else { 1 }))
        .sum();
    mv <= avail
}

fn on_click(
    on: On<Pointer<Click>>,
    cards: Query<(&CardEntity, &CardLocation)>,
    mut input: ResMut<CurrentInput>,
    pending: Res<PendingPrompt>,
    drag: Res<DragState>,
    mut inspect: ResMut<InspectCard>,
    view: Res<GameView>,
    mut out: MessageWriter<SendCmd>,
) {
    let e = on.event_target();
    let Ok((ce, loc)) = cards.get(e) else { return };
    match on.event().button {
        PointerButton::Secondary => {
            let visible = view.card(ce.0).is_some_and(|c| !c.hidden && c.state.is_some());
            if visible {
                inspect.0 = if inspect.0 == Some(ce.0) { None } else { Some(ce.0) };
            }
        }
        PointerButton::Primary => {
            if pending.0.is_some() || drag.moved > 10.0 {
                return;
            }
            inspect.0 = None;
            if clickable(&input, &view, loc, ce.0) {
                info!("click_card {}", ce.0);
                out.write(SendCmd(ClientFrame::click_card(ce.0)));
                input.awaiting = true;
            }
        }
        _ => {}
    }
}

fn on_drag_start(
    on: On<Pointer<DragStart>>,
    mut commands: Commands,
    cards: Query<(&CardLocation, &Transform)>,
    mut drag: ResMut<DragState>,
    input: Res<CurrentInput>,
    pending: Res<PendingPrompt>,
) {
    let e = on.event_target();
    if on.event().button != PointerButton::Primary || pending.0.is_some() {
        return;
    }
    let Ok((loc, _)) = cards.get(e) else { return };
    drag.moved = 0.0;
    if loc.zone == ZoneKind::Hand && loc.mine && !loc.hidden && input.frame.is_some() {
        drag.active = Some(e);
        drag.holding.retain(|(h, _)| *h != e);
        commands.entity(e).insert(Dragging).remove::<Hovered>();
    }
}

fn on_drag(on: On<Pointer<Drag>>, mut drag: ResMut<DragState>) {
    if on.event().button == PointerButton::Primary {
        drag.moved = drag.moved.max(on.event().distance.length());
    }
}

fn on_drag_end(
    on: On<Pointer<DragEnd>>,
    mut commands: Commands,
    cards: Query<&CardEntity>,
    mut drag: ResMut<DragState>,
    mut input: ResMut<CurrentInput>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut out: MessageWriter<SendCmd>,
) {
    let e = on.event_target();
    if on.event().button != PointerButton::Primary {
        return;
    }
    let moved = drag.moved;
    drag.moved = 0.0;
    if drag.active != Some(e) {
        return;
    }
    drag.active = None;
    let Ok(ce) = cards.get(e) else { return };
    let h = windows.single().map(|w| w.height()).unwrap_or(900.0);
    let y = on.event().pointer_location.position.y;
    info!("drag end moved={moved:.0} y={y:.0} threshold={:.0}", h * 0.64);
    if moved > 10.0 && y < h * 0.64 {
        info!("drag-play click_card {}", ce.0);
        out.write(SendCmd(ClientFrame::click_card(ce.0)));
        input.awaiting = true;
        // keep the card where it was dropped until the engine moves it (or gives up)
        drag.holding.push((e, 0.8));
    } else {
        commands.entity(e).remove::<Dragging>();
    }
}

fn drag_follow(
    mut drag: ResMut<DragState>,
    pointers: Query<(&bevy::picking::pointer::PointerId, &bevy::picking::pointer::PointerLocation)>,
    hand_cam: Query<(&Camera, &GlobalTransform), With<HandCamera>>,
    mut cards: Query<(&mut Transform, Option<&TargetPlacement>), With<Dragging>>,
    time: Res<Time>,
    mut commands: Commands,
) {
    let Some(e) = drag.active else { return };
    // the picking pointer (not the OS cursor) so scripted / touch input drags too
    let Some(cursor) = pointers
        .iter()
        .find(|(id, _)| matches!(id, bevy::picking::pointer::PointerId::Mouse))
        .and_then(|(_, l)| l.location().map(|l| l.position))
    else {
        return;
    };
    let Ok((cam, cgt)) = hand_cam.single() else { return };
    let Ok(ray) = cam.viewport_to_world(cgt, cursor) else { return };
    let Ok((mut tf, tp)) = cards.get_mut(e) else {
        drag.active = None;
        return;
    };
    let dir = *ray.direction;
    if dir.z.abs() < 1e-4 {
        return;
    }
    let t = (3.0 - ray.origin.z) / dir.z;
    let target = ray.origin + dir * t;
    let k = 1.0 - (-time.delta_secs() * 28.0).exp();
    tf.translation = tf.translation.lerp(target, k);
    let want_scale = tp.map(|t| t.scale).unwrap_or(2.7) * 1.1;
    tf.scale = Vec3::splat(tf.scale.x + (want_scale - tf.scale.x) * k);
    tf.rotation = tf.rotation.slerp(Quat::IDENTITY, k);
    let _ = &mut commands;
}

fn drag_hold(
    mut drag: ResMut<DragState>,
    time: Res<Time>,
    mut commands: Commands,
    locs: Query<&CardLocation>,
    mut tfs: Query<(&mut Transform, Option<&TargetPlacement>), With<Dragging>>,
) {
    let dt = time.delta_secs();
    let mut done = Vec::new();
    for (e, left) in drag.holding.iter_mut() {
        *left -= dt;
        let moved_zone = locs.get(*e).map(|l| l.zone != ZoneKind::Hand).unwrap_or(true);
        if *left <= 0.0 || moved_zone {
            done.push(*e);
        } else if let Ok((mut tf, _)) = tfs.get_mut(*e) {
            // shrink slightly while waiting so it reads as "on its way"
            tf.scale *= 1.0 - dt * 0.4;
        }
    }
    drag.holding.retain(|(e, _)| !done.contains(e));
    for e in done {
        if drag.active != Some(e)
            && let Ok(mut ec) = commands.get_entity(e)
        {
            ec.remove::<Dragging>();
        }
    }
}

// ---- input mirror ---------------------------------------------------------------------------

#[derive(Component)]
pub struct PlayableHint;

fn mirror_input(
    mut commands: Commands,
    input: Res<CurrentInput>,
    view: Res<GameView>,
    cards: Query<(Entity, &CardEntity, &CardLocation, Has<Selectable>, Has<Highlighted>, Has<WeakSelectable>, Has<PlayableHint>)>,
) {
    for (e, ce, loc, sel, hi, weak, ph) in &cards {
        let want_ph = loc.mine
            && loc.zone == ZoneKind::Hand
            && view.card(ce.0).is_some_and(|c| playable_hint(&view, &input, c))
            && !input.is_selectable(ce.0);
        if want_ph != ph {
            if want_ph { commands.entity(e).insert(PlayableHint); } else { commands.entity(e).remove::<PlayableHint>(); }
        }
        let (ws, wh, ww) = (input.is_selectable(ce.0), input.is_highlighted(ce.0), input.is_weak(ce.0));
        if ws != sel || wh != hi || ww != weak {
            let mut ec = commands.entity(e);
            match (ws, sel) {
                (true, false) => {
                    ec.insert(Selectable);
                }
                (false, true) => {
                    ec.remove::<Selectable>();
                }
                _ => {}
            }
            match (wh, hi) {
                (true, false) => {
                    ec.insert(Highlighted);
                }
                (false, true) => {
                    ec.remove::<Highlighted>();
                }
                _ => {}
            }
            match (ww, weak) {
                (true, false) => {
                    ec.insert(WeakSelectable);
                }
                (false, true) => {
                    ec.remove::<WeakSelectable>();
                }
                _ => {}
            }
        }
    }
}

// ---- hover pose -----------------------------------------------------------------------------

fn ease_hover(
    time: Res<Time>,
    mut q: Query<(&mut HoverPose, Has<Hovered>, &CardLocation, Option<&TargetPlacement>), Without<Dragging>>,
) {
    let dt = time.delta_secs();
    for (mut hp, hovered, loc, tp) in &mut q {
        let Some(tp) = tp else { continue };
        let want = if hovered && !loc.hidden && (loc.zone != ZoneKind::Hand || loc.mine) { 1.0 } else { 0.0 };
        let k = 1.0 - (-dt * 15.0).exp();
        hp.amt += (want - hp.amt) * k;
        if (hp.amt - want).abs() < 0.002 {
            hp.amt = want;
        }
        let a = hp.amt;
        hp.pose = if tp.layer == HAND_LAYER && loc.mine {
            // The card rises fully into view, un-rolls and grows.
            let h = CARD_H * tp.scale * 1.4;
            let bottom_to_center = h / 2.0;
            let lift = (-HAND_VIEW_H / 2.0 + bottom_to_center + 0.18) - tp.translation.y;
            PoseOffset {
                translation: Vec3::new(0.0, lift.max(0.0) * a, 2.0 * a),
                rotation: Quat::IDENTITY.slerp(tp.rotation.inverse(), a),
                scale: 1.0 + 0.4 * a,
            }
        } else if tp.layer == HAND_LAYER {
            PoseOffset::default()
        } else {
            PoseOffset { translation: Vec3::new(0.0, 0.32 * a, -0.08 * a), rotation: Quat::IDENTITY, scale: 1.0 + 0.12 * a }
        };
    }
}

fn compose_pose(
    mut q: Query<(&HoverPose, Option<&CombatPose>, Option<&FxPose>, &mut PoseOffset), Without<Dragging>>,
) {
    for (h, c, f, mut out) in &mut q {
        let mut p = h.pose;
        for extra in [c.map(|c| c.0), f.map(|f| f.0)].into_iter().flatten() {
            p.translation += extra.translation;
            p.rotation = extra.rotation * p.rotation;
            p.scale *= extra.scale;
        }
        if *out != p {
            *out = p;
        }
    }
}

// ---- hotkeys --------------------------------------------------------------------------------

fn hotkeys(
    keys: Res<ButtonInput<KeyCode>>,
    input: Res<CurrentInput>,
    pending: Res<PendingPrompt>,
    mut inspect: ResMut<InspectCard>,
    mut menu: ResMut<MenuUi>,
    mut acts: MessageWriter<Acted>,
) {
    if pending.0.is_some() {
        return;
    }
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    if keys.just_pressed(KeyCode::Escape) {
        if inspect.0.is_some() {
            inspect.0 = None;
        } else if menu.confirm_concede {
            acts.write(Acted(Act::CloseInspect));
        } else if menu.gear_open || menu.log_open {
            menu.gear_open = false;
            menu.log_open = false;
        } else if let Some(s) = secondary_model(&input)
            && s.kind == SecondaryKind::Generic
            && s.enabled
        {
            acts.write(Acted(Act::Secondary));
        }
        return;
    }
    if keys.just_pressed(KeyCode::Space) && ctrl {
        acts.write(Acted(Act::PassTurn));
    } else if keys.just_pressed(KeyCode::Space) || keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter) {
        acts.write(Acted(Act::Primary));
    }
    if keys.just_pressed(KeyCode::KeyF) {
        acts.write(Acted(Act::PassTurn));
    }
    if keys.just_pressed(KeyCode::KeyZ) {
        acts.write(Acted(Act::Undo));
    }
    if keys.just_pressed(KeyCode::KeyA)
        && let Some(s) = secondary_model(&input)
        && s.kind == SecondaryKind::AttackAll
    {
        acts.write(Acted(Act::AttackAll));
    }
    if keys.just_pressed(KeyCode::KeyL) {
        acts.write(Acted(Act::ToggleLog));
    }
}

fn inspect_acts(mut acts: MessageReader<Acted>, mut inspect: ResMut<InspectCard>) {
    for Acted(a) in acts.read() {
        if matches!(a, Act::CloseInspect | Act::Primary | Act::Secondary | Act::PassTurn | Act::AttackAll | Act::Undo) {
            inspect.0 = None;
        }
    }
}

/// Safety net: if the engine never answers a click (rejected → only a `flash`), re-enable the
/// buttons instead of leaving the player stuck.
fn awaiting_timeout(
    time: Res<Time>,
    mut input: ResMut<CurrentInput>,
    mut events: MessageReader<BoardEvent>,
    mut since: Local<f32>,
) {
    for e in events.read() {
        if matches!(e, BoardEvent::Rejected { .. } | BoardEvent::Alert) {
            input.awaiting = false;
        }
    }
    if input.awaiting {
        *since += time.delta_secs();
        if *since > 1.6 {
            input.awaiting = false;
        }
    } else {
        *since = 0.0;
    }
}

fn clear_stale_hover(registry: Res<CardRegistry>, mut preview: ResMut<HoverPreview>, mut inspect: ResMut<InspectCard>) {
    if let Some(id) = preview.0
        && !registry.0.contains_key(&id)
    {
        preview.0 = None;
    }
    if let Some(id) = inspect.0
        && !registry.0.contains_key(&id)
    {
        inspect.0 = None;
    }
}

#[allow(dead_code)]
fn _unused(_: Extent3d, _: TextureDimension, _: TextureFormat, _: RenderAssetUsages, _: NotShadowCaster, _: Pickable) {}

#[derive(Component)]
pub struct Shake(f32);

/// `flash` (engine rejected) → shake the card the user last clicked.
fn shake_rejected(
    time: Res<Time>,
    mut events: MessageReader<BoardEvent>,
    registry: Res<CardRegistry>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut Shake, &mut FxPose)>,
) {
    for ev in events.read() {
        if let BoardEvent::Rejected { card: Some(id) } = ev
            && let Some(&e) = registry.0.get(id)
        {
            commands.entity(e).insert((Shake(0.0), FxPose::default()));
        }
    }
    for (e, mut sh, mut fx) in &mut q {
        sh.0 += time.delta_secs();
        let k = (1.0 - sh.0 / 0.4).max(0.0);
        fx.0.translation = Vec3::new((sh.0 * 60.0).sin() * 0.12 * k, 0.0, 0.0);
        if sh.0 >= 0.4 {
            fx.0 = PoseOffset::default();
            commands.entity(e).remove::<Shake>();
        }
    }
}
