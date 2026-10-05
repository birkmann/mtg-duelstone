//! Main menu: logo + Play / Decks / Settings / Quit on the left, over the 3D portal stage
//! (`stage`). The UI intro is timed against the stage's `IntroClock` so both halves land
//! together: fade up from black, the logo drops in with a flare, its underline draws out, the
//! buttons slide in one after another, then the chrome. Returning from another screen replays
//! only a fast version of the button slide.

use bevy::prelude::*;
use bevy::ui::{BackgroundGradient, ColorStop, LinearGradient};

use super::MenuAction;
use super::stage::IntroClock;
use crate::shared::{AppState, ArenaFonts, Settings};
use crate::style::*;

pub struct MainMenuPlugin;

impl Plugin for MainMenuPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::MainMenu), spawn_main_menu).add_systems(Update, animate_intro.run_if(in_state(AppState::MainMenu)));
    }
}

const LOGO_SIZE: f32 = 78.0;

/// One intro animation on a UI node. `at`/`dur` are on the UI timeline (see `ui_t`).
#[derive(Component)]
struct Anim {
    at: f32,
    dur: f32,
    fx: Fx,
}

#[derive(Clone, Copy)]
enum Fx {
    /// full-screen black that lifts at the start
    Curtain,
    /// text fading in to this colour (its shadow fades along)
    Fade(Color),
    /// scale from this down to 1
    Pop(f32),
    /// width grows from 0 to this many px
    Grow(f32),
    SlideX(f32),
    SlideY(f32),
    /// light bloom behind the logo: flashes on landing, then settles into a slow pulse
    Flare,
}

fn anim(at: f32, dur: f32, fx: Fx) -> Anim {
    Anim { at, dur, fx }
}

/// UI timeline: the full intro runs in real time; a return visit starts just before the buttons
/// and runs faster.
fn ui_t(clock: &IntroClock) -> f32 {
    if clock.full { clock.t } else { 2.2 + clock.t * 1.8 }
}

fn spawn_main_menu(mut commands: Commands, fonts: Res<ArenaFonts>, settings: Res<Settings>) {
    let shadow = TextShadow { offset: Vec2::new(0.0, 4.0), color: Color::srgba(0.0, 0.0, 0.0, 0.85) };
    commands
        .spawn((
            Name::new("MainMenu"),
            DespawnOnExit(AppState::MainMenu),
            Node { align_items: AlignItems::Center, ..fullscreen_root() },
            // let the pointer through to the 3D cards
            Pickable::IGNORE,
        ))
        .with_children(|root| {
            // left scrim so the buttons read over the stage
            root.spawn((
                Node { width: Val::Percent(60.0), ..fullscreen_root() },
                BackgroundGradient::from(LinearGradient::to_right(vec![
                    ColorStop::new(Color::srgba(0.01, 0.006, 0.004, 0.82), Val::Percent(0.0)),
                    ColorStop::new(Color::srgba(0.01, 0.006, 0.004, 0.45), Val::Percent(55.0)),
                    ColorStop::new(Color::srgba(0.01, 0.006, 0.004, 0.0), Val::Percent(100.0)),
                ])),
                Pickable::IGNORE,
            ));
            root.spawn((
                Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, margin: UiRect::left(Val::Percent(8.0)), row_gap: Val::Px(14.0), ..default() },
                Pickable::IGNORE,
            ))
            .with_children(|col| {
                // logo
                col.spawn((
                    anim(1.7, 0.75, Fx::Pop(1.6)),
                    Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, ..default() },
                    UiTransform::IDENTITY,
                    Pickable::IGNORE,
                ))
                .with_children(|c| {
                    let logo_w = LOGO_SIZE * 6.0;
                    // An invisible box the size of the wordmark whose blurred shadow is the glow:
                    // a box shadow is soft by construction, unlike a UI radial gradient.
                    c.spawn((
                        anim(1.95, 1.6, Fx::Flare),
                        Node {
                            position_type: PositionType::Absolute,
                            width: Val::Px(logo_w * 0.9),
                            height: Val::Px(LOGO_SIZE * 0.7),
                            left: Val::Px(logo_w * 0.05),
                            top: Val::Px(LOGO_SIZE * 0.3),
                            ..default()
                        },
                        BoxShadow::new(Color::NONE, Val::Px(0.0), Val::Px(0.0), Val::Px(LOGO_SIZE * 0.4), Val::Px(LOGO_SIZE * 1.3)),
                        Pickable::IGNORE,
                    ));
                    c.spawn((anim(1.7, 0.5, Fx::Fade(GOLD_BRIGHT)), Text::new("MTG ARENA"), text_font(&fonts.title, LOGO_SIZE), TextColor(Color::NONE), shadow));
                    c.spawn((
                        anim(2.15, 0.65, Fx::Grow(logo_w)),
                        Node { width: Val::Px(0.0), height: Val::Px(3.0), margin: UiRect::top(Val::Px(4.0)), ..default() },
                        BackgroundGradient::from(LinearGradient::to_right(vec![
                            ColorStop::new(Color::srgba(0.86, 0.68, 0.30, 0.0), Val::Percent(0.0)),
                            ColorStop::new(Color::srgb(1.0, 0.78, 0.35), Val::Percent(50.0)),
                            ColorStop::new(Color::srgba(0.86, 0.68, 0.30, 0.0), Val::Percent(100.0)),
                        ])),
                    ));
                });
                col.spawn((
                    anim(2.4, 0.7, Fx::Fade(TEXT_DIM)),
                    Node { margin: UiRect::new(Val::Px(0.0), Val::Px(0.0), Val::Px(4.0), Val::Px(30.0)), ..default() },
                    ui_text(&fonts.body, "Constructed  ·  Commander  ·  vs the Forge AI", 16.0, Color::NONE),
                    TextShadow { offset: Vec2::new(0.0, 1.0), color: Color::srgba(0.0, 0.0, 0.0, 0.9) },
                ));
                let p = spawn_button(col, &fonts, ButtonKind::Primary, "PLAY", Vec2::new(340.0, 72.0));
                col.commands().entity(p).insert((MenuAction::Play, anim(2.55, 0.75, Fx::SlideX(-640.0))));
                for (i, (label, act)) in [("DECKS", MenuAction::Decks), ("CUSTOMIZE", MenuAction::Customize), ("SETTINGS", MenuAction::OpenSettings), ("QUIT", MenuAction::Quit)].into_iter().enumerate() {
                    let kind = if act == MenuAction::Quit { ButtonKind::Ghost } else { ButtonKind::Secondary };
                    let b = spawn_button(col, &fonts, kind, label, Vec2::new(340.0, 54.0));
                    col.commands().entity(b).insert((act, anim(2.65 + i as f32 * 0.09, 0.75, Fx::SlideX(-640.0))));
                }
            });
            // footer
            root.spawn((Node { position_type: PositionType::Absolute, bottom: Val::Px(14.0), left: Val::Px(0.0), width: Val::Percent(100.0), justify_content: JustifyContent::Center, ..default() }, Pickable::IGNORE))
                .with_child((
                    anim(3.2, 0.8, Fx::Fade(Color::srgba(0.7, 0.62, 0.5, 0.6))),
                    ui_text(&fonts.body, "Unofficial fan project · Rules engine: Forge (GPL-3) · Card images © Wizards of the Coast via Scryfall · hover or click the cards", 13.0, Color::NONE),
                ));
            // player chip
            root.spawn((
                anim(3.0, 0.6, Fx::SlideY(-90.0)),
                UiTransform::from_translation(Val2::px(0.0, -90.0)),
                chip(Node { position_type: PositionType::Absolute, top: Val::Px(18.0), right: Val::Px(22.0), padding: UiRect::axes(Val::Px(18.0), Val::Px(8.0)), ..default() }),
            ))
            .with_child(ui_text(&fonts.title, settings.player_name.clone(), 18.0, GOLD));
        });
    commands.spawn((
        Name::new("MainMenuCurtain"),
        DespawnOnExit(AppState::MainMenu),
        anim(0.0, 1.0, Fx::Curtain),
        fullscreen_root(),
        BackgroundColor(Color::BLACK),
        GlobalZIndex(60),
        Pickable::IGNORE,
    ));
}

fn ease_out_cubic(x: f32) -> f32 {
    1.0 - (1.0 - x.clamp(0.0, 1.0)).powi(3)
}

fn ease_out_back(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    1.0 + 2.2 * (x - 1.0).powi(3) + 1.2 * (x - 1.0).powi(2)
}

#[allow(clippy::type_complexity)]
fn animate_intro(
    time: Res<Time>,
    clock: Res<IntroClock>,
    mut q: Query<(
        &Anim,
        Option<&mut UiTransform>,
        Option<&mut TextColor>,
        Option<&mut TextShadow>,
        &mut Node,
        Option<&mut BackgroundColor>,
        Option<&mut BoxShadow>,
    )>,
) {
    let t = ui_t(&clock);
    for (a, tf, tc, ts, mut node, bg, shadow) in &mut q {
        let p = ((t - a.at) / a.dur).clamp(0.0, 1.0);
        match a.fx {
            Fx::Curtain => {
                // a return visit only needs a quick dip, not the full fade from black
                let p = if clock.full { ((clock.t - 0.1) / a.dur).clamp(0.0, 1.0) } else { (clock.t / 0.3).clamp(0.0, 1.0) };
                if let Some(mut bg) = bg {
                    bg.0 = Color::srgba(0.0, 0.0, 0.0, 1.0 - p * p * (3.0 - 2.0 * p));
                }
                if p >= 1.0 && node.display != Display::None {
                    node.display = Display::None;
                }
            }
            Fx::Fade(c) => {
                let k = p * p * (3.0 - 2.0 * p);
                if let Some(mut tc) = tc {
                    tc.0 = c.with_alpha(c.alpha() * k);
                }
                if let Some(mut ts) = ts {
                    ts.color = ts.color.with_alpha(0.85 * k);
                }
            }
            Fx::Pop(from) => {
                if let Some(mut tf) = tf {
                    tf.scale = Vec2::splat(from + (1.0 - from) * ease_out_back(p));
                }
            }
            Fx::Grow(w) => node.width = Val::Px(w * ease_out_cubic(p)),
            Fx::SlideX(from) => {
                if let Some(mut tf) = tf {
                    tf.translation.x = Val::Px(from * (1.0 - ease_out_back(p)));
                }
            }
            Fx::SlideY(from) => {
                if let Some(mut tf) = tf {
                    tf.translation.y = Val::Px(from * (1.0 - ease_out_cubic(p)));
                }
            }
            Fx::Flare => {
                // flash on landing, then a slow breathing glow behind the wordmark
                let flash = if t < a.at { 0.0 } else { (-(t - a.at) * 2.4).exp() };
                let idle = ease_out_cubic(p) * (0.16 + 0.05 * (time.elapsed_secs() * 1.4).sin());
                let alpha = (flash * 0.7 + idle).min(1.0);
                if let Some(mut sh) = shadow
                    && let Some(first) = sh.0.first_mut()
                {
                    first.color = Color::srgba(1.0, 0.55, 0.16, alpha);
                }
            }
        }
    }
}
