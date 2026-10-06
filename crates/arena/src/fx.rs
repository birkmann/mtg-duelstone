//! Visual effects driven by `BoardEvent`s (plus `FxRequest`s from other plugins): floating damage
//! and life numbers, hit shake, spark/dust bursts, spell-cast flash, "YOUR TURN" banner and
//! victory/defeat confetti.
//!
//! Why `bevy_ui` nodes and not 3D particles: effects are brief, screen-space and need crisp text;
//! a projected UI node moved with `UiTransform` costs no layout work and needs no extra meshes,
//! materials or render layers. Counts are capped. All durations are divided by
//! `Settings.anim_speed`.

use std::f32::consts::TAU;

use bevy::prelude::*;
use bevy::ui::{BackgroundGradient, ColorStop, LinearGradient};
use bevy::window::PrimaryWindow;
use mtg_protocol::{CardId, ZoneKind};

use crate::shared::*;
use crate::style::*;

pub struct FxPlugin;

impl Plugin for FxPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<FxRequest>()
            .init_resource::<FxRng>()
            .add_systems(OnEnter(AppState::GameOver), game_over_fx)
            .add_systems(OnEnter(AppState::Loading), clear_fx)
            .add_systems(OnEnter(AppState::MainMenu), clear_fx)
            .add_systems(
                Update,
                (
                    on_board_events,
                    on_fx_requests,
                    tick_shake,
                    tick_particles,
                    tick_floats,
                    tick_rings,
                    tick_banner,
                    tick_flash,
                    fx_demo,
                )
                    .chain(),
            );
    }
}

// -------------------------------------------------------------------------------------------
// Components
// -------------------------------------------------------------------------------------------

#[derive(Component)]
struct FxNode;

/// Where an effect sits: fixed world point, or following an entity (cards fly into place first).
#[derive(Clone, Copy)]
enum Anchor {
    World(Vec3),
    Entity(Entity, Vec3),
}

#[derive(Component)]
struct Particle {
    pos: Vec2,
    vel: Vec2,
    acc: Vec2,
    age: f32,
    life: f32,
    size: f32,
    color: LinearRgba,
    spin: f32,
    rot: f32,
    /// stretch along velocity (sparks)
    streak: bool,
    fade_in: f32,
}

#[derive(Component)]
struct FloatNum {
    anchor: Anchor,
    last: Vec2,
    age: f32,
    life: f32,
    color: Color,
}

#[derive(Component)]
struct Ring {
    anchor: Anchor,
    last: Vec2,
    age: f32,
    life: f32,
    max_scale: f32,
    color: LinearRgba,
}

#[derive(Component)]
struct TurnBanner {
    age: f32,
    life: f32,
}

#[derive(Component)]
struct Flash {
    age: f32,
    life: f32,
    color: LinearRgba,
    peak: f32,
}

/// Hit shake: offsets the card's face quads in card-local space so it never fights with the
/// PoseOffset writers (hover, attack push).
#[derive(Component)]
struct Shake {
    age: f32,
    life: f32,
    amp: f32,
}

#[derive(Resource)]
struct FxRng(u32);
impl Default for FxRng {
    fn default() -> Self {
        Self(0xC0FFEE)
    }
}
impl FxRng {
    fn f(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        (self.0 >> 8) as f32 / (1u32 << 24) as f32
    }
    fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.f()
    }
}

const MAX_PARTICLES: usize = 360;

fn speed(settings: &Settings) -> f32 {
    settings.anim_speed.clamp(0.25, 4.0)
}

fn clear_fx(mut commands: Commands, q: Query<Entity, With<FxNode>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

// -------------------------------------------------------------------------------------------
// Spawn helpers
// -------------------------------------------------------------------------------------------

fn base_node(w: f32, h: f32) -> Node {
    Node { position_type: PositionType::Absolute, left: Val::Px(0.0), top: Val::Px(0.0), width: Val::Px(w), height: Val::Px(h), ..default() }
}

fn spawn_particle(commands: &mut Commands, p: Particle, z: i32) {
    let h = if p.streak { p.size * 0.35 } else { p.size };
    commands.spawn((
        FxNode,
        GlobalZIndex(z),
        Pickable::IGNORE,
        Node { border_radius: BorderRadius::all(Val::Px(if p.streak { 1.0 } else { 99.0 })), ..base_node(p.size, h) },
        BackgroundColor(Color::NONE),
        UiTransform::IDENTITY,
        Visibility::Hidden,
        p,
    ));
}

fn burst_sparks(commands: &mut Commands, rng: &mut FxRng, at: Vec2, count: usize, power: f32, color: LinearRgba, live: usize) {
    let n = count.min(MAX_PARTICLES.saturating_sub(live));
    for _ in 0..n {
        let a = rng.range(0.0, TAU);
        let s = rng.range(0.35, 1.0) * 380.0 * power;
        spawn_particle(
            commands,
            Particle {
                pos: at,
                vel: Vec2::new(a.cos(), a.sin()) * s,
                acc: Vec2::new(0.0, 700.0),
                age: 0.0,
                life: rng.range(0.28, 0.6),
                size: rng.range(9.0, 18.0) * (0.7 + 0.3 * power),
                color,
                spin: 0.0,
                rot: a,
                streak: true,
                fade_in: 0.0,
            },
            40,
        );
    }
}

fn spawn_float(commands: &mut Commands, fonts: &ArenaFonts, anchor: Anchor, text: String, color: Color, size: f32, life: f32) {
    commands
        .spawn((
            FxNode,
            GlobalZIndex(45),
            Pickable::IGNORE,
            Node { justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..base_node(220.0, 90.0) },
            UiTransform::IDENTITY,
            Visibility::Hidden,
            FloatNum { anchor, last: Vec2::ZERO, age: 0.0, life, color },
        ))
        .with_child((
            Text::new(text),
            text_font(&fonts.numbers, size),
            TextColor(color),
            TextShadow { offset: Vec2::new(0.0, 3.0), color: Color::srgba(0.0, 0.0, 0.0, 0.9) },
            Pickable::IGNORE,
        ));
}

fn spawn_ring(commands: &mut Commands, anchor: Anchor, color: LinearRgba, max_scale: f32, life: f32, width: f32) {
    commands.spawn((
        FxNode,
        GlobalZIndex(35),
        Pickable::IGNORE,
        Node { border: UiRect::all(Val::Px(width)), border_radius: BorderRadius::all(Val::Percent(50.0)), ..base_node(100.0, 100.0) },
        BorderColor::all(Color::NONE),
        UiTransform::IDENTITY,
        Visibility::Hidden,
        Ring { anchor, last: Vec2::ZERO, age: 0.0, life, max_scale, color },
    ));
}

fn spawn_flash(commands: &mut Commands, color: LinearRgba, peak: f32, life: f32) {
    commands.spawn((
        FxNode,
        GlobalZIndex(30),
        Pickable::IGNORE,
        Node { width: Val::Percent(100.0), height: Val::Percent(100.0), ..base_node(0.0, 0.0) },
        BackgroundColor(Color::NONE),
        Flash { age: 0.0, life, color, peak },
    ));
}

// -------------------------------------------------------------------------------------------
// Event handling
// -------------------------------------------------------------------------------------------

fn card_anchor(registry: &CardRegistry, id: CardId) -> Option<Anchor> {
    registry.0.get(&id).map(|e| Anchor::Entity(*e, Vec3::ZERO))
}

fn on_board_events(
    mut commands: Commands,
    mut events: MessageReader<BoardEvent>,
    view: Res<GameView>,
    cache: Res<CardCache>,
    registry: Res<CardRegistry>,
    anchors: Res<BoardAnchors>,
    fonts: Res<ArenaFonts>,
    settings: Res<Settings>,
    mut rng: ResMut<FxRng>,
    particles: Query<(), With<Particle>>,
    banners: Query<Entity, With<TurnBanner>>,
) {
    let live = particles.iter().count();
    let sp = speed(&settings);
    for ev in events.read() {
        match ev {
            BoardEvent::CardDamaged { id, amount } => {
                let Some(a) = card_anchor(&registry, *id) else { continue };
                spawn_float(&mut commands, &fonts, a, format!("-{amount}"), RED_DAMAGE, 40.0 + (*amount as f32).min(10.0) * 3.0, 1.1 / sp);
                spawn_sparks_at(&mut commands, a, 12, 0.9, LinearRgba::new(1.0, 0.55, 0.2, 1.0), live);
                if let Some(e) = registry.0.get(id) {
                    commands.entity(*e).insert(Shake { age: 0.0, life: 0.38 / sp, amp: 0.012 + 0.004 * (*amount as f32).min(8.0) });
                }
            }
            BoardEvent::LifeChanged { player, old, new } => {
                let d = new - old;
                if d == 0 {
                    continue;
                }
                let mine = view.is_me(*player);
                let a = Anchor::World(anchors.avatar_of(*player, mine) + Vec3::Y * 0.25);
                let (txt, col) = if d < 0 { (format!("{d}"), RED_DAMAGE) } else { (format!("+{d}"), GREEN_HEAL) };
                spawn_float(&mut commands, &fonts, a, txt, col, 52.0 + (d.abs() as f32).min(12.0) * 3.0, 1.3 / sp);
                if d < 0 {
                    spawn_sparks_at(&mut commands, a, 16, 1.1, LinearRgba::new(1.0, 0.25, 0.15, 1.0), live);
                    if mine {
                        spawn_flash(&mut commands, LinearRgba::new(0.9, 0.05, 0.03, 1.0), 0.33, 0.55 / sp);
                    }
                } else {
                    spawn_ring(&mut commands, a, LinearRgba::new(0.4, 1.0, 0.5, 1.0), 3.2, 0.7 / sp, 4.0);
                }
            }
            BoardEvent::CardMoved { id, from, to, .. } => {
                let Some(a) = card_anchor(&registry, *id) else { continue };
                let is_land = cache.0.get(id).or_else(|| view.card(*id)).and_then(|c| c.state.as_ref()).is_some_and(|s| s.is_land);
                match (*from, *to) {
                    (ZoneKind::Hand, ZoneKind::Battlefield) if is_land => land_dust(&mut commands, &mut rng, a, live),
                    (_, ZoneKind::Battlefield) => {
                        spawn_ring(&mut commands, a, LinearRgba::new(1.0, 0.8, 0.4, 1.0), 2.6, 0.55 / sp, 3.0);
                    }
                    (ZoneKind::Battlefield, ZoneKind::Graveyard) | (ZoneKind::Battlefield, ZoneKind::Exile) => {
                        // death: ash puffs rising from where it stood
                        spawn_sparks_at(&mut commands, a, 18, 0.55, LinearRgba::new(0.55, 0.5, 0.5, 1.0), live);
                        spawn_ring(&mut commands, a, LinearRgba::new(0.9, 0.2, 0.15, 1.0), 2.0, 0.45 / sp, 3.0);
                    }
                    _ => {}
                }
            }
            BoardEvent::StackPushed { source: Some(id), .. } => {
                if let Some(a) = card_anchor(&registry, *id) {
                    spawn_ring(&mut commands, a, LinearRgba::new(0.55, 0.75, 1.0, 1.0), 3.4, 0.6 / sp, 4.0);
                    spawn_sparks_at(&mut commands, a, 10, 0.6, LinearRgba::new(0.7, 0.85, 1.0, 1.0), live);
                }
            }
            BoardEvent::StackResolved { source: Some(id), .. } => {
                if let Some(a) = card_anchor(&registry, *id) {
                    spawn_ring(&mut commands, a, LinearRgba::new(1.0, 0.9, 0.5, 1.0), 2.4, 0.45 / sp, 3.0);
                }
            }
            BoardEvent::TurnStarted { player, .. } => {
                for b in &banners {
                    commands.entity(b).despawn();
                }
                let mine = view.is_me(*player);
                spawn_banner(&mut commands, &fonts, if mine { "YOUR TURN" } else { "OPPONENT'S TURN" }, mine, 1.7 / sp);
            }
            BoardEvent::Cue(cue) => {
                let kind = cue.kind.to_ascii_lowercase();
                if kind.contains("damage") {
                    if let Some(id) = cue.target_card {
                        if let Some(a) = card_anchor(&registry, id) {
                            spawn_sparks_at(&mut commands, a, 8, 0.8, LinearRgba::new(1.0, 0.7, 0.3, 1.0), live);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn on_fx_requests(
    mut commands: Commands,
    mut reqs: MessageReader<FxRequest>,
    fonts: Res<ArenaFonts>,
    settings: Res<Settings>,
    mut rng: ResMut<FxRng>,
    particles: Query<(), With<Particle>>,
    banners: Query<Entity, With<TurnBanner>>,
) {
    let live = particles.iter().count();
    let sp = speed(&settings);
    for r in reqs.read() {
        match r {
            FxRequest::FloatingNumber { world, amount, positive } => {
                let (t, c) = if *positive { (format!("+{amount}"), GREEN_HEAL) } else { (format!("-{amount}"), RED_DAMAGE) };
                spawn_float(&mut commands, &fonts, Anchor::World(*world), t, c, 48.0, 1.1 / sp);
            }
            FxRequest::Impact { world, strength } => {
                spawn_sparks_at(&mut commands, Anchor::World(*world), (10.0 * strength) as usize + 4, *strength, LinearRgba::new(1.0, 0.6, 0.25, 1.0), live);
            }
            FxRequest::Banner { text } => {
                for b in &banners {
                    commands.entity(b).despawn();
                }
                spawn_banner(&mut commands, &fonts, text, true, 1.7 / sp);
            }
        }
        let _ = &mut rng;
    }
}

/// Sparks need a screen position now, but anchors resolve per frame, so spawn a tiny delayed
/// emitter: a zero-life float that bursts on its first tick. Simpler: resolve immediately where
/// possible, else fall back to a ring-less burst at the screen centre.
fn spawn_sparks_at(commands: &mut Commands, anchor: Anchor, count: usize, power: f32, color: LinearRgba, live: usize) {
    commands.spawn((FxNode, Emitter { anchor, count, power, color, live },));
}

fn land_dust(commands: &mut Commands, _rng: &mut FxRng, anchor: Anchor, live: usize) {
    spawn_ring(commands, anchor, LinearRgba::new(0.85, 0.7, 0.45, 1.0), 3.0, 0.6, 5.0);
    commands.spawn((FxNode, Emitter { anchor, count: 10, power: 0.35, color: LinearRgba::new(0.8, 0.65, 0.4, 1.0), live },));
}

/// One-shot: converts an anchor into screen space on its first update and emits particles.
#[derive(Component)]
struct Emitter {
    anchor: Anchor,
    count: usize,
    power: f32,
    color: LinearRgba,
    live: usize,
}

// -------------------------------------------------------------------------------------------
// Anchors → screen
// -------------------------------------------------------------------------------------------

fn resolve(anchor: Anchor, cam: &Camera, cam_gt: &GlobalTransform, gts: &Query<&GlobalTransform>) -> Option<Vec2> {
    let world = match anchor {
        Anchor::World(w) => w,
        Anchor::Entity(e, off) => gts.get(e).ok()?.translation() + off,
    };
    world_to_ui(cam, cam_gt, world)
}

// -------------------------------------------------------------------------------------------
// Ticks
// -------------------------------------------------------------------------------------------

fn tick_particles(
    mut commands: Commands,
    time: Res<Time>,
    settings: Res<Settings>,
    mut rng: ResMut<FxRng>,
    emitters: Query<(Entity, &Emitter)>,
    camera: Query<(&Camera, &GlobalTransform), With<TableCamera>>,
    gts: Query<&GlobalTransform>,
    mut q: Query<(Entity, &mut Particle, &mut UiTransform, &mut BackgroundColor, &mut Visibility)>,
) {
    if let Ok((cam, cam_gt)) = camera.single() {
        for (e, em) in &emitters {
            if let Some(p) = resolve(em.anchor, cam, cam_gt, &gts) {
                burst_sparks(&mut commands, &mut rng, p, em.count, em.power, em.color, em.live);
            }
            commands.entity(e).despawn();
        }
    }
    let dt = time.delta_secs() * speed(&settings);
    for (e, mut p, mut tf, mut bg, mut vis) in &mut q {
        p.age += dt;
        if p.age >= p.life {
            commands.entity(e).despawn();
            continue;
        }
        let acc = p.acc;
        p.vel += acc * dt;
        let v = p.vel;
        p.pos += v * dt;
        let t = p.age / p.life;
        let rot = if p.streak { p.vel.y.atan2(p.vel.x) } else { p.rot + p.spin * p.age };
        let (w, h) = if p.streak { (p.size, p.size * 0.35) } else { (p.size, p.size) };
        let shrink = 1.0 - t * 0.6;
        tf.translation = Val2::px(p.pos.x - w * 0.5, p.pos.y - h * 0.5);
        tf.rotation = Rot2::radians(rot);
        tf.scale = Vec2::splat(shrink);
        let fade = (1.0 - t).powf(1.4) * if p.fade_in > 0.0 { (p.age / p.fade_in).min(1.0) } else { 1.0 };
        // hot white-yellow at birth, settling to the base colour
        let hot = (1.0 - t * 2.5).max(0.0);
        let c = LinearRgba::new(p.color.red + hot * 0.5, p.color.green + hot * 0.5, p.color.blue + hot * 0.4, fade);
        bg.0 = Color::from(c);
        if *vis != Visibility::Inherited {
            *vis = Visibility::Inherited;
        }
    }
}

fn tick_floats(
    mut commands: Commands,
    time: Res<Time>,
    camera: Query<(&Camera, &GlobalTransform), With<TableCamera>>,
    gts: Query<&GlobalTransform>,
    mut q: Query<(Entity, &mut FloatNum, &mut UiTransform, &mut Visibility, Option<&Children>)>,
    mut texts: Query<&mut TextColor>,
) {
    let Ok((cam, cam_gt)) = camera.single() else { return };
    for (e, mut f, mut tf, mut vis, children) in &mut q {
        // `life` already carries the animation speed; age advances in real time
        f.age += time.delta_secs();
        if f.age >= f.life {
            commands.entity(e).despawn();
            continue;
        }
        if let Some(p) = resolve(f.anchor, cam, cam_gt, &gts) {
            f.last = p;
        }
        let t = f.age / f.life;
        // pop (overshoot) then settle; rise and fade
        let pop = if t < 0.18 { 0.4 + 1.1 * (t / 0.18) } else if t < 0.3 { 1.5 - 0.5 * ((t - 0.18) / 0.12) } else { 1.0 };
        let rise = 70.0 * (1.0 - (1.0 - t).powi(2));
        tf.translation = Val2::px(f.last.x - 110.0, f.last.y - 45.0 - rise);
        tf.scale = Vec2::splat(pop);
        let a = if t > 0.6 { 1.0 - (t - 0.6) / 0.4 } else { 1.0 };
        if let Some(ch) = children {
            for c in ch.iter() {
                if let Ok(mut tc) = texts.get_mut(c) {
                    tc.0 = f.color.with_alpha(a);
                }
            }
        }
        if *vis != Visibility::Inherited {
            *vis = Visibility::Inherited;
        }
    }
}

fn tick_rings(
    mut commands: Commands,
    time: Res<Time>,
    camera: Query<(&Camera, &GlobalTransform), With<TableCamera>>,
    gts: Query<&GlobalTransform>,
    mut q: Query<(Entity, &mut Ring, &mut UiTransform, &mut BorderColor, &mut Visibility)>,
) {
    let Ok((cam, cam_gt)) = camera.single() else { return };
    for (e, mut r, mut tf, mut bc, mut vis) in &mut q {
        r.age += time.delta_secs();
        if r.age >= r.life {
            commands.entity(e).despawn();
            continue;
        }
        if let Some(p) = resolve(r.anchor, cam, cam_gt, &gts) {
            r.last = p;
        }
        let t = r.age / r.life;
        let ease = 1.0 - (1.0 - t).powi(3);
        let s = 0.2 + (r.max_scale - 0.2) * ease;
        tf.translation = Val2::px(r.last.x - 50.0, r.last.y - 50.0);
        tf.scale = Vec2::new(s, s * 0.62); // squashed: it lies on the table
        let c = r.color;
        *bc = BorderColor::all(Color::from(LinearRgba::new(c.red, c.green, c.blue, (1.0 - t).powf(1.5))));
        if *vis != Visibility::Inherited {
            *vis = Visibility::Inherited;
        }
    }
}

fn tick_shake(
    mut commands: Commands,
    time: Res<Time>,
    mut cards: Query<(Entity, &mut Shake, &Children)>,
    mut faces: Query<&mut Transform, Or<(With<CardFront>, With<CardBack>)>>,
) {
    for (e, mut s, children) in &mut cards {
        s.age += time.delta_secs();
        let done = s.age >= s.life;
        let k = if done { 0.0 } else { (1.0 - s.age / s.life).powi(2) };
        let x = (s.age * 95.0).sin() * s.amp * k;
        let y = (s.age * 77.0 + 1.3).sin() * s.amp * 0.6 * k;
        for c in children.iter() {
            if let Ok(mut t) = faces.get_mut(c) {
                t.translation.x = x;
                t.translation.y = y;
            }
        }
        if done {
            commands.entity(e).remove::<Shake>();
        }
    }
}

fn tick_flash(mut commands: Commands, time: Res<Time>, mut q: Query<(Entity, &mut Flash, &mut BackgroundColor)>) {
    for (e, mut f, mut bg) in &mut q {
        f.age += time.delta_secs();
        if f.age >= f.life {
            commands.entity(e).despawn();
            continue;
        }
        let a = f.peak * (1.0 - f.age / f.life).powi(2);
        let c = f.color;
        bg.0 = Color::from(LinearRgba::new(c.red, c.green, c.blue, a));
    }
}

// -------------------------------------------------------------------------------------------
// Banner
// -------------------------------------------------------------------------------------------

fn spawn_banner(commands: &mut Commands, fonts: &ArenaFonts, text: &str, mine: bool, life: f32) {
    let (tint, txt) = if mine { (Color::srgb(0.95, 0.5, 0.12), GOLD_BRIGHT) } else { (Color::srgb(0.55, 0.12, 0.1), Color::srgb(1.0, 0.8, 0.75)) };
    let s = tint.to_srgba();
    commands
        .spawn((
            FxNode,
            TurnBanner { age: 0.0, life },
            GlobalZIndex(42),
            Pickable::IGNORE,
            Node { position_type: PositionType::Absolute, left: Val::Px(0.0), top: Val::Percent(40.0), width: Val::Percent(100.0), height: Val::Px(120.0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
            BackgroundGradient::from(LinearGradient::to_right(vec![
                ColorStop::new(Color::srgba(s.red * 0.25, s.green * 0.25, s.blue * 0.25, 0.0), Val::Percent(0.0)),
                ColorStop::new(Color::srgba(s.red * 0.25, s.green * 0.25, s.blue * 0.25, 0.82), Val::Percent(25.0)),
                ColorStop::new(Color::srgba(s.red * 0.25, s.green * 0.25, s.blue * 0.25, 0.82), Val::Percent(75.0)),
                ColorStop::new(Color::srgba(s.red * 0.25, s.green * 0.25, s.blue * 0.25, 0.0), Val::Percent(100.0)),
            ])),
            UiTransform::IDENTITY,
            Visibility::Hidden,
        ))
        .with_children(|b| {
            b.spawn((
                Text::new(text),
                text_font(&fonts.title, 72.0),
                TextColor(txt),
                TextShadow { offset: Vec2::new(0.0, 4.0), color: Color::srgba(0.0, 0.0, 0.0, 0.85) },
                Pickable::IGNORE,
            ));
            b.spawn((
                Node { position_type: PositionType::Absolute, left: Val::Percent(15.0), right: Val::Percent(15.0), top: Val::Px(0.0), height: Val::Px(2.0), ..default() },
                BackgroundColor(tint),
            ));
            b.spawn((
                Node { position_type: PositionType::Absolute, left: Val::Percent(15.0), right: Val::Percent(15.0), bottom: Val::Px(0.0), height: Val::Px(2.0), ..default() },
                BackgroundColor(tint),
            ));
        });
}

fn tick_banner(
    mut commands: Commands,
    time: Res<Time>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut q: Query<(Entity, &mut TurnBanner, &mut UiTransform, &mut Visibility)>,
) {
    let w = window.single().map(|w| w.width()).unwrap_or(1600.0);
    for (e, mut b, mut tf, mut vis) in &mut q {
        b.age += time.delta_secs();
        let t = b.age / b.life;
        if t >= 1.0 {
            commands.entity(e).despawn();
            continue;
        }
        // sweep in (0..0.25), hold, sweep out (0.75..1)
        let x = if t < 0.25 {
            let p = t / 0.25;
            -w * (1.0 - (1.0 - (1.0 - p).powi(3)))
        } else if t > 0.75 {
            let p = (t - 0.75) / 0.25;
            w * p.powi(3)
        } else {
            // slow drift while held
            (t - 0.5) * 40.0
        };
        tf.translation = Val2::px(x, 0.0);
        let sy = if t < 0.1 { t / 0.1 } else if t > 0.9 { (1.0 - t) / 0.1 } else { 1.0 };
        tf.scale = Vec2::new(1.0, sy.clamp(0.05, 1.0));
        if *vis != Visibility::Inherited {
            *vis = Visibility::Inherited;
        }
    }
}

// -------------------------------------------------------------------------------------------
// Game over: confetti (victory) or falling embers (defeat)
// -------------------------------------------------------------------------------------------

fn game_over_fx(
    mut commands: Commands,
    result: Res<GameResult>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut rng: ResMut<FxRng>,
) {
    let won = result.0.as_ref().is_some_and(|r| r.you_won);
    let (w, h) = window.single().map(|w| (w.width(), w.height())).unwrap_or((1600.0, 900.0));
    let palette = [
        LinearRgba::new(1.0, 0.75, 0.2, 1.0),
        LinearRgba::new(1.0, 0.35, 0.2, 1.0),
        LinearRgba::new(0.3, 0.7, 1.0, 1.0),
        LinearRgba::new(0.5, 1.0, 0.5, 1.0),
        LinearRgba::new(1.0, 0.95, 0.8, 1.0),
    ];
    let n = if won { 140 } else { 60 };
    for i in 0..n {
        let x = rng.range(0.0, w);
        let (color, size, vy, ax) = if won {
            (palette[i % palette.len()], rng.range(8.0, 16.0), rng.range(120.0, 320.0), 0.0)
        } else {
            (LinearRgba::new(0.9, 0.25, 0.1, 1.0), rng.range(3.0, 6.0), rng.range(-90.0, -40.0), 0.0)
        };
        let start_y = if won { rng.range(-h * 0.6, -20.0) } else { h + 10.0 };
        spawn_particle(
            &mut commands,
            Particle {
                pos: Vec2::new(x, start_y),
                vel: Vec2::new(rng.range(-60.0, 60.0), vy),
                acc: Vec2::new(ax, if won { 40.0 } else { -8.0 }),
                age: 0.0,
                life: rng.range(3.5, 6.0),
                size,
                color,
                spin: rng.range(-6.0, 6.0),
                rot: rng.range(0.0, TAU),
                streak: false,
                fade_in: 0.2,
            },
            60,
        );
    }
    if won {
        spawn_flash(&mut commands, LinearRgba::new(1.0, 0.85, 0.4, 1.0), 0.28, 0.9);
    } else {
        spawn_flash(&mut commands, LinearRgba::new(0.2, 0.0, 0.0, 1.0), 0.4, 1.4);
    }
}


/// Dev aid: `ARENA_FX_DEMO=<secs>` fires one of each effect that long after the match starts
/// (for unattended screenshots).
fn fx_demo(
    time: Res<Time>,
    state: Res<State<AppState>>,
    anchors: Res<BoardAnchors>,
    mut w: MessageWriter<FxRequest>,
    mut started: Local<Option<f32>>,
    mut fired: Local<bool>,
) {
    let Ok(v) = std::env::var("ARENA_FX_DEMO") else { return };
    if *state.get() != AppState::InMatch || *fired {
        return;
    }
    let t0 = *started.get_or_insert(time.elapsed_secs());
    let after: f32 = v.parse().unwrap_or(8.0);
    if time.elapsed_secs() - t0 < after {
        return;
    }
    *fired = true;
    let c = Vec3::new(0.0, 0.0, 0.5);
    w.write(FxRequest::Banner { text: "YOUR TURN".into() });
    w.write(FxRequest::FloatingNumber { world: c + Vec3::new(-1.5, 0.0, 0.0), amount: 3, positive: false });
    w.write(FxRequest::FloatingNumber { world: anchors.my_avatar + Vec3::Y * 0.3, amount: 4, positive: true });
    w.write(FxRequest::Impact { world: c + Vec3::new(1.5, 0.0, 0.0), strength: 1.6 });
    w.write(FxRequest::Impact { world: c + Vec3::new(-1.5, 0.0, 0.0), strength: 1.2 });
}
