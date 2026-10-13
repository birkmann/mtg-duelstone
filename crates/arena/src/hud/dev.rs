//! Dev-only: `ARENA_SHOT_WHEN=<cond>` takes a screenshot the first time a live game state matches
//! (stack, ability, attack, block, target, pay, attackdecl, blockdecl, sel, paused, won, lost), then exits. Lets us
//! eyeball arrows / stack / combat visuals on REAL engine data under `--autoplay`.
//! `ARENA_HOVER_HAND=<n>` pretends the pointer rests on my n-th hand card (hover lift check),
//! `ARENA_INSPECT_HAND` pins the hand-card inspector for layout checks.
//! `ARENA_HOVER_COMMAND` on my first command-zone card. `paused` pairs with `ARENA_AUTOPLAY_PAUSE`.

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};

use crate::shared::*;

pub struct DevPlugin;

impl Plugin for DevPlugin {
    fn build(&self, app: &mut App) {
        if std::env::var("ARENA_SHOT_WHEN").is_ok() {
            app.add_systems(Update, shot_when.run_if(in_state(AppState::InMatch).or_else(in_state(AppState::GameOver))));
        }
        if std::env::var("ARENA_SIM").is_ok() {
            app.add_systems(Update, sim_pointer.run_if(in_state(AppState::InMatch)));
        }
        if std::env::var("ARENA_DBG_CLICK").is_ok() {
            app.add_systems(Update, dbg_hover);
        }
        if std::env::var("ARENA_DEBUG_ARROW").is_ok() {
            app.add_systems(Update, debug_arrows.run_if(in_state(AppState::InMatch).or_else(in_state(AppState::GameOver))));
        }
        if std::env::var("ARENA_LOG_INPUT").is_ok() {
            app.add_systems(Update, log_input);
        }
        if std::env::var("ARENA_OPEN_MENU").is_ok() {
            app.add_systems(Update, open_menu.run_if(in_state(AppState::InMatch).or_else(in_state(AppState::GameOver))));
        }
        if std::env::var("ARENA_HOVER_HAND").is_ok() || std::env::var("ARENA_HOVER_COMMAND").is_ok() || std::env::var("ARENA_INSPECT_HAND").is_ok() {
            app.add_systems(Update, fake_hover.run_if(in_state(AppState::InMatch).or_else(in_state(AppState::GameOver))));
        }
    }
}

fn fake_hover(
    view: Res<GameView>,
    registry: Res<CardRegistry>,
    mut commands: Commands,
    mut preview: ResMut<HoverPreview>,
    mut inspect: ResMut<crate::interact::InspectCard>,
    time: Res<Time>,
) {
    if time.elapsed_secs() < 8.0 {
        return;
    }
    let n: usize = std::env::var("ARENA_HOVER_HAND").ok().and_then(|s| s.parse().ok()).unwrap_or(2);
    let Some(me) = view.me() else { return };
    // ARENA_HOVER_COMMAND: my first command-zone card that isn't the Planar Dice (a plane, a scheme, a commander)
    let c = if std::env::var("ARENA_HOVER_COMMAND").is_ok() {
        me.zones.command.iter().find(|c| c.name != "Planar Dice")
    } else {
        me.zones.hand.get(n)
    };
    let Some(c) = c else { return };
    if let Some(&e) = registry.0.get(&c.id) {
        commands.entity(e).insert(Hovered);
        preview.0 = Some(c.id);
        if std::env::var("ARENA_INSPECT_HAND").is_ok() { inspect.0 = Some(c.id); }
    }
}

fn shot_when(
    time: Res<Time>,
    view: Res<GameView>,
    input: Res<CurrentInput>,
    result: Res<GameResult>,
    mut commands: Commands,
    mut held: Local<f32>,
    mut fired: Local<Option<f32>>,
    mut exit: MessageWriter<AppExit>,
) {
    if let Some(t) = *fired {
        if time.elapsed_secs() - t > 1.5 {
            exit.write(AppExit::Success);
        }
        return;
    }
    let cond = std::env::var("ARENA_SHOT_WHEN").unwrap_or_default();
    let g = &view.game;
    let f = input.frame.as_ref();
    let ok = match cond.as_str() {
        "stack" => !g.stack.is_empty(),
        "ability" => g.stack.iter().any(|s| s.is_ability || s.is_trigger),
        "targets" => g.stack.iter().any(|s| !s.target_cards.is_empty() || !s.target_players.is_empty()),
        "attack" => g.combat.as_ref().is_some_and(|c| !c.attackers.is_empty()),
        "block" => g.combat.as_ref().is_some_and(|c| c.attackers.iter().any(|a| !a.blockers.is_empty())),
        "target" => f.is_some_and(|f| f.card.is_some() && !f.selectable.is_empty() && !f.message.to_lowercase().contains("pay mana")),
        "pay" => f.is_some_and(|f| f.message.to_lowercase().contains("pay mana")),
        "attackdecl" => crate::combat::declaring_attackers(&view, &input) && f.is_some_and(|f| !f.selectable.is_empty()),
        "blockdecl" => crate::combat::declaring_blockers(&view, &input) && f.is_some_and(|f| !f.selectable.is_empty()),
        "sel" => f.is_some_and(|f| !f.selectable.is_empty()),
        "paused" => crate::debug::paused_for_shot(&view),
        // the game-over banner (pair with ARENA_SHOT_DELAY so it has finished animating in)
        "won" => result.0.as_ref().is_some_and(|r| r.you_won),
        "lost" => result.0.as_ref().is_some_and(|r| !r.you_won),
        _ => false,
    };
    // ARENA_SHOT_MIN_TURN: skip the first turns, whose boards are too bare to show off
    let min_turn: i32 = std::env::var("ARENA_SHOT_MIN_TURN").ok().and_then(|s| s.parse().ok()).unwrap_or(0);
    let ok = ok && (g.turn >= min_turn || matches!(cond.as_str(), "won" | "lost"));
    // ARENA_SHOT_AFTER: earliest shot (s); portraits and card art stream in during the first seconds
    let after: f32 = std::env::var("ARENA_SHOT_AFTER").ok().and_then(|s| s.parse().ok()).unwrap_or(8.0);
    if ok && time.elapsed_secs() > after {
        *held += time.delta_secs();
    } else {
        *held = 0.0;
    }
    let delay: f32 = std::env::var("ARENA_SHOT_DELAY").ok().and_then(|s| s.parse().ok()).unwrap_or(0.3);
    if *held >= delay {
        let path = std::env::var("ARENA_SHOT_PATH").unwrap_or_else(|_| "/tmp/hud/when.png".into());
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
        *fired = Some(time.elapsed_secs());
    }
}

fn open_menu(time: Res<Time>, mut ui: ResMut<crate::hud::menu::MenuUi>) {
    if time.elapsed_secs() > 8.0 {
        ui.gear_open = true;
        ui.log_open = true;
    }
}

fn log_input(input: Res<CurrentInput>, view: Res<GameView>, mut last: Local<i64>) {
    let Some(f) = input.frame.as_ref() else { return };
    if f.seq == *last {
        return;
    }
    *last = f.seq;
    info!(
        "INPUT seq={} phase={:?} card={:?} sel={} hi={} weak={} players={} tray={} ok={:?}/{} cancel={:?}/{} msg={:?}",
        f.seq, view.phase(), f.card, f.selectable.len(), f.highlighted.len(), f.weak.len(), f.selectable_players.len(), f.tray_cards.len(),
        f.ok.label, f.ok.enabled, f.cancel.label, f.cancel.enabled, f.message.replace('\n', " | ")
    );
}

fn debug_arrows(anchors: Res<BoardAnchors>, mut q: ResMut<crate::targeting::ArrowQueue>) {
    use crate::targeting::Arrow;
    let a = anchors.my_avatar + Vec3::Y * 0.3;
    let b = anchors.opp_avatar + Vec3::Y * 0.3;
    q.0.push(Arrow::new(a, b, Color::srgb(0.25, 0.95, 0.85)));
    let mut r = Arrow::new(a + Vec3::new(-3.0, 0.0, -0.5), b + Vec3::new(3.0, 0.0, 1.5), Color::srgb(1.0, 0.3, 0.12));
    r.dashed = true;
    q.0.push(r);
    q.0.push(Arrow::new(a + Vec3::new(3.0, 0.0, -1.0), a + Vec3::new(5.0, 0.0, -3.0), Color::srgb(1.0, 0.75, 0.2)));
}

/// Scripted pointer through the REAL picking pipeline (PointerInput messages):
/// `ARENA_SIM=hover|click|drag|dragback|avatar` starting at `ARENA_SIM_T0` (default 9s).
fn sim_pointer(
    time: Res<Time>,
    view: Res<GameView>,
    registry: Res<CardRegistry>,
    hand_cam: Query<(&Camera, &GlobalTransform), With<HandCamera>>,
    windows: Query<Entity, With<bevy::window::PrimaryWindow>>,
    cards: Query<&GlobalTransform>,
    mut out: MessageWriter<bevy::picking::pointer::PointerInput>,
    mut last: Local<Vec2>,
    mut started: Local<Option<f32>>,
    mut stage: Local<u8>,
) {
    use bevy::camera::{NormalizedRenderTarget, RenderTarget};
    use bevy::picking::pointer::{Location, PointerAction, PointerButton, PointerId, PointerInput};
    let t0: f32 = std::env::var("ARENA_SIM_T0").ok().and_then(|s| s.parse().ok()).unwrap_or(9.0);
    let now = time.elapsed_secs();
    if now < t0 {
        return;
    }
    let Ok(win) = windows.single() else { return };
    if std::env::var("ARENA_SIM").as_deref() == Ok("click_at") {
        // "x,y;x,y;..." — one click every 1.5 s
        let spec = std::env::var("ARENA_SIM_POS").unwrap_or_else(|_| "800,450".into());
        let pts: Vec<Vec2> = spec
            .split(';')
            .filter_map(|p| p.split_once(',').map(|(a, b)| Vec2::new(a.trim().parse().unwrap_or(0.0), b.trim().parse().unwrap_or(0.0))))
            .collect();
        let began = *started.get_or_insert(now);
        let dt = now - began;
        let i = ((dt / 1.5) as usize).min(pts.len().saturating_sub(1));
        let Some(&p) = pts.get(i) else { return };
        let local = dt - i as f32 * 1.5;
        let Some(target) = RenderTarget::Window(bevy::window::WindowRef::Entity(win)).normalize(Some(win)) else { return };
        let loc = Location { target, position: p };
        out.write(PointerInput::new(PointerId::Mouse, loc.clone(), PointerAction::Move { delta: p - *last }));
        *last = p;
        // stage encodes (click index, phase): 2*i = ready, 2*i+1 = pressed
        if local >= 0.4 && *stage as usize == 2 * i {
            *stage += 1;
            out.write(PointerInput::new(PointerId::Mouse, loc, PointerAction::Press(PointerButton::Primary)));
        } else if local >= 0.55 && *stage as usize == 2 * i + 1 {
            *stage += 1;
            out.write(PointerInput::new(PointerId::Mouse, loc, PointerAction::Release(PointerButton::Primary)));
        }
        return;
    }
    let Some(me) = view.me() else { return };
    let idx: usize = std::env::var("ARENA_SIM_CARD").ok().and_then(|s| s.parse().ok()).unwrap_or(2);
    let Some(card) = me.zones.hand.get(idx) else { return };
    let Some(&ce) = registry.0.get(&card.id) else { return };
    let (Ok(gt), Ok((cam, cgt))) = (cards.get(ce), hand_cam.single()) else { return };
    let Ok(start) = cam.world_to_viewport(cgt, gt.translation() + Vec3::Y * 0.4) else { return };
    let began = *started.get_or_insert(now);
    let dt = now - began;
    let Some(target) = RenderTarget::Window(bevy::window::WindowRef::Entity(win)).normalize(Some(win)) else { return };
    let mode = std::env::var("ARENA_SIM").unwrap_or_default();
    let send = |pos: Vec2, action: PointerAction, out: &mut MessageWriter<PointerInput>| {
        let loc = Location { target: target.clone(), position: pos };
        out.write(PointerInput::new(PointerId::Mouse, loc, action));
    };
    let _ = NormalizedRenderTarget::None { width: 0, height: 0 };
    let goal = Vec2::new(800.0, 330.0);
    let mv = |pos: Vec2, out: &mut MessageWriter<PointerInput>, last: &mut Vec2| {
        send(pos, PointerAction::Move { delta: pos - *last }, out);
        *last = pos;
    };
    match mode.as_str() {
        "hover" => mv(start, &mut out, &mut last),
        "click" => {
            mv(start, &mut out, &mut last);
            if (0.4..0.45).contains(&dt) {
                send(start, PointerAction::Press(PointerButton::Primary), &mut out);
            }
            if (0.5..0.55).contains(&dt) {
                send(start, PointerAction::Release(PointerButton::Primary), &mut out);
            }
        }
        "drag" | "dragback" => {
            if dt < 0.4 {
                mv(start, &mut out, &mut last);
            } else if dt < 0.45 {
                send(start, PointerAction::Press(PointerButton::Primary), &mut out);
            } else if dt < 1.6 {
                let k = ((dt - 0.45) / 1.0).clamp(0.0, 1.0);
                let end = if mode == "drag" { goal } else { start + Vec2::new(40.0, -30.0) };
                mv(start.lerp(end, k), &mut out, &mut last);
            } else if dt > 2.6 && dt < 2.65 {
                let end = if mode == "drag" { goal } else { start + Vec2::new(40.0, -30.0) };
                send(end, PointerAction::Release(PointerButton::Primary), &mut out);
            }
        }
        "avatar" => {
            let p = Vec2::new(800.0, 760.0);
            mv(p, &mut out, &mut last);
            if (0.4..0.45).contains(&dt) {
                send(p, PointerAction::Press(PointerButton::Primary), &mut out);
            }
            if (0.5..0.55).contains(&dt) {
                send(p, PointerAction::Release(PointerButton::Primary), &mut out);
            }
        }
        _ => {}
    }
}

/// Dev: prints what the pointer is hovering (entity + component names) a few times a second.
pub fn dbg_hover(world: &World, mut last: Local<f32>) {
    let t = world.resource::<Time>().elapsed_secs();
    if t - *last < 0.25 {
        return;
    }
    *last = t;
    let Some(map) = world.resource::<bevy::picking::hover::HoverMap>().0.get(&bevy::picking::pointer::PointerId::Mouse) else { return };
    for (e, _) in map.iter() {
        let names: Vec<String> = world
            .inspect_entity(*e)
            .map(|it| it.map(|c| c.name().to_string().rsplit("::").next().unwrap_or("").to_string()).collect())
            .unwrap_or_default();
        info!("HOVER {e:?}: {}", names.join(","));
    }
}
