//! Animated menu backdrop: warm radial glow, slowly drifting tilted card backs and rising embers.
//! One persistent entity tree, shown (Display::Flex) only in menu-ish states so the match isn't
//! covered by it.

use bevy::prelude::*;
use bevy::ui::{BackgroundGradient, ColorStop, Gradient, RadialGradient, RadialGradientShape, UiPosition};
use bevy::window::PrimaryWindow;

use super::backdrop_state;
use crate::shared::AppState;

pub struct BackdropPlugin;

impl Plugin for BackdropPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_backdrop).add_systems(Update, (toggle_backdrop, animate_backdrop).chain());
    }
}

#[derive(Component)]
struct Backdrop;

/// Large tilted card back drifting diagonally, wrapping around the screen.
#[derive(Component)]
struct Drifter {
    /// start position in 0..1 of the window
    pos: Vec2,
    vel: Vec2,
    rot: f32,
    rot_speed: f32,
    size: f32,
}

#[derive(Component)]
struct Ember {
    x: f32,
    speed: f32,
    phase: f32,
    sway: f32,
}

/// Tiny deterministic LCG so the layout is stable between runs without a rand dependency.
struct Lcg(u32);
impl Lcg {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        (self.0 >> 8) as f32 / (1u32 << 24) as f32
    }
}

fn spawn_backdrop(mut commands: Commands, assets: Res<AssetServer>) {
    let back = assets.load::<Image>("textures/card_back.png");
    let mut rng = Lcg(0x5EED);
    commands
        .spawn((
            Name::new("MenuBackdrop"),
            Backdrop,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                overflow: Overflow::clip(),
                ..default()
            },
            GlobalZIndex(-50),
            BackgroundGradient::from(Gradient::Radial(RadialGradient::new(
                UiPosition::center(Val::Percent(0.0), Val::Percent(-8.0)),
                RadialGradientShape::FarthestCorner,
                vec![
                    ColorStop::new(Color::srgb(0.30, 0.17, 0.07), Val::Percent(0.0)),
                    ColorStop::new(Color::srgb(0.13, 0.075, 0.04), Val::Percent(45.0)),
                    ColorStop::new(Color::srgb(0.035, 0.022, 0.016), Val::Percent(100.0)),
                ],
            ))),
        ))
        .with_children(|root| {
            for _ in 0..14 {
                let size = 90.0 + rng.next() * 150.0;
                let depth = (size - 90.0) / 150.0; // bigger = closer = brighter, faster
                root.spawn((
                    Drifter {
                        pos: Vec2::new(rng.next(), rng.next()),
                        vel: Vec2::new(0.004 + 0.01 * depth, -0.002 - 0.006 * depth) * (0.6 + rng.next() * 0.8),
                        rot: (rng.next() - 0.5) * 70.0,
                        rot_speed: (rng.next() - 0.5) * 3.0,
                        size,
                    },
                    Node { position_type: PositionType::Absolute, left: Val::Px(0.0), top: Val::Px(0.0), width: Val::Px(size), height: Val::Px(size * 1.39), ..default() },
                    ImageNode { image: back.clone(), color: Color::srgba(1.0, 0.9, 0.8, 0.07 + 0.16 * depth), ..default() },
                    UiTransform::IDENTITY,
                ));
            }
            for _ in 0..46 {
                let s = 2.0 + rng.next() * 4.0;
                root.spawn((
                    Ember { x: rng.next(), speed: 0.02 + rng.next() * 0.05, phase: rng.next(), sway: 10.0 + rng.next() * 40.0 },
                    Node { position_type: PositionType::Absolute, left: Val::Px(0.0), top: Val::Px(0.0), width: Val::Px(s), height: Val::Px(s), border_radius: BorderRadius::all(Val::Px(99.0)), ..default() },
                    BackgroundColor(Color::srgba(1.0, 0.6, 0.2, 0.5)),
                    BoxShadow::new(Color::srgba(1.0, 0.5, 0.1, 0.8), Val::Px(0.0), Val::Px(0.0), Val::Px(1.0), Val::Px(6.0)),
                    UiTransform::IDENTITY,
                ));
            }
            // vignette
            root.spawn((
                Node { position_type: PositionType::Absolute, left: Val::Px(0.0), top: Val::Px(0.0), width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
                BackgroundGradient::from(Gradient::Radial(RadialGradient::new(
                    UiPosition::CENTER,
                    RadialGradientShape::FarthestCorner,
                    vec![
                        ColorStop::new(Color::srgba(0.0, 0.0, 0.0, 0.0), Val::Percent(50.0)),
                        ColorStop::new(Color::srgba(0.0, 0.0, 0.0, 0.75), Val::Percent(100.0)),
                    ],
                ))),
                Pickable::IGNORE,
            ));
        });
}

fn toggle_backdrop(state: Res<State<AppState>>, mut q: Query<&mut Node, With<Backdrop>>) {
    let want = if backdrop_state(*state.get()) { Display::Flex } else { Display::None };
    for mut n in &mut q {
        if n.display != want {
            n.display = want;
        }
    }
}

fn animate_backdrop(
    time: Res<Time>,
    window: Query<&Window, With<PrimaryWindow>>,
    state: Res<State<AppState>>,
    mut drifters: Query<(&Drifter, &mut UiTransform), Without<Ember>>,
    mut embers: Query<(&Ember, &mut UiTransform, &mut BackgroundColor), Without<Drifter>>,
) {
    if !backdrop_state(*state.get()) {
        return;
    }
    let Ok(w) = window.single() else { return };
    let (ww, wh) = (w.width(), w.height());
    let t = time.elapsed_secs();
    for (d, mut tf) in &mut drifters {
        // wrap with a margin so cards slide fully off-screen before re-entering
        let m = 0.35;
        let span = 1.0 + 2.0 * m;
        let p = Vec2::new(((d.pos.x + d.vel.x * t + m).rem_euclid(span)) - m, ((d.pos.y + d.vel.y * t + m).rem_euclid(span)) - m);
        tf.translation = Val2::px(p.x * ww - d.size * 0.5, p.y * wh - d.size * 0.7);
        tf.rotation = Rot2::degrees(d.rot + d.rot_speed * t);
    }
    for (e, mut tf, mut bg) in &mut embers {
        let p = (e.phase + e.speed * t).fract();
        let y = (1.0 - p) * (wh + 40.0) - 20.0;
        let x = e.x * ww + (t * 0.7 + e.phase * 9.0).sin() * e.sway;
        tf.translation = Val2::px(x, y);
        let fade = (p * (1.0 - p) * 4.0).clamp(0.0, 1.0);
        bg.0 = Color::srgba(1.0, 0.55 + 0.3 * p, 0.2, 0.65 * fade);
    }
}
