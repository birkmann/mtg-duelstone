//! Main menu: logo + Play / Decks / Settings / Quit on the left, over the 3D portal stage
//! (`stage`). The UI intro is timed against the stage's `IntroClock` so both halves land
//! together: fade up from black, the logo settles in with a flare, its underline draws out, the
//! buttons rise into place one after another, then the chrome. Returning from another screen
//! replays only a fast version of the button reveal.
//!
//! Motion is deliberately quiet: no overshoot, no travel from off-screen. Buttons fade in while
//! rising a few pixels (a staggered reveal), so nothing is ever parked half-visible at the edge
//! before its turn, which is what made the old slide-in look cheap.

use bevy::prelude::*;
use bevy::ui::{BackgroundGradient, ColorStop, LinearGradient};

use super::MenuAction;
use super::stage::IntroClock;
use bevy::image::TRANSPARENT_IMAGE_HANDLE;

use crate::shared::{AppState, ArenaFonts, AvatarArt, ServerFacts, Settings};
use crate::style::*;

/// How far each button rises into place, and the gap between one button's start and the next.
const REVEAL_RISE: f32 = 14.0;
const REVEAL_STAGGER: f32 = 0.08;

pub struct MainMenuPlugin;

impl Plugin for MainMenuPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::MainMenu), spawn_main_menu).add_systems(Update, animate_intro.run_if(in_state(AppState::MainMenu)))
            .add_systems(Update, fit_main_menu);
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
    /// scale from this down to 1, settling without overshoot
    Pop(f32),
    /// a button fading in (see `ButtonReveal`) while rising this many px into place
    Reveal(f32),
    /// width grows from 0 to this many px
    Grow(f32),
    SlideY(f32),
    /// light bloom behind the logo: flashes on landing, then settles into a slow pulse
    Flare,
}

fn anim(at: f32, dur: f32, fx: Fx) -> Anim {
    Anim { at, dur, fx }
}

/// A menu button's reveal starting at `at`. It spawns already hidden and lowered, so the frame
/// before the intro system first runs can't flash it in its final place.
fn reveal(at: f32) -> impl Bundle {
    (anim(at, 0.6, Fx::Reveal(REVEAL_RISE)), ButtonReveal(0.0), UiTransform::from_translation(Val2::px(0.0, REVEAL_RISE)))
}

/// UI timeline: the full intro runs in real time; a return visit starts just before the buttons
/// and runs faster.
fn ui_t(clock: &IntroClock) -> f32 {
    if clock.full { clock.t } else { 2.2 + clock.t * 1.8 }
}

fn spawn_main_menu(mut commands: Commands, fonts: Res<ArenaFonts>, settings: Res<Settings>, facts: Res<ServerFacts>) {
    let public = facts.is_public();
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
                    anim(1.7, 1.1, Fx::Pop(1.08)),
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
                    c.spawn((anim(1.7, 0.5, Fx::Fade(GOLD_BRIGHT)), Text::new("MTG DUELSTONE"), text_font(&fonts.title, LOGO_SIZE), TextColor(Color::NONE), shadow));
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
                    ui_text(&fonts.body, "Constructed  ·  Commander  ·  Draft  ·  vs the Forge AI or up to 3 friends", 16.0, Color::NONE),
                    TextShadow { offset: Vec2::new(0.0, 1.0), color: Color::srgba(0.0, 0.0, 0.0, 0.9) },
                ));
                let p = spawn_button(col, &fonts, ButtonKind::Primary, "PLAY", Vec2::new(340.0, 72.0));
                col.commands().entity(p).insert((MenuAction::Play, reveal(2.45)));
                for (i, (label, act)) in [("MULTIPLAYER", MenuAction::Multiplayer), ("CUBES", MenuAction::Cubes), ("DECKS", MenuAction::Decks), ("CUSTOMIZE", MenuAction::Customize), ("SETTINGS", MenuAction::OpenSettings), ("QUIT", MenuAction::Quit)]
                    .into_iter()
                    // a browser tab can't quit itself
                    .filter(|(_, a)| !(crate::platform::WEB && *a == MenuAction::Quit))
                    // a public engine only plays the AI and keeps no decks or cubes for visitors
                    .filter(|(_, a)| !(public && matches!(a, MenuAction::Multiplayer | MenuAction::Cubes | MenuAction::Decks)))
                    .enumerate()
                {
                    let kind = if act == MenuAction::Quit { ButtonKind::Ghost } else { ButtonKind::Secondary };
                    let b = spawn_button(col, &fonts, kind, label, Vec2::new(340.0, 54.0));
                    col.commands().entity(b).insert((act, reveal(2.45 + (i + 1) as f32 * REVEAL_STAGGER)));
                }
            });
            // footer
            root.spawn((Node { position_type: PositionType::Absolute, bottom: Val::Px(14.0), left: Val::Px(0.0), width: Val::Percent(100.0), justify_content: JustifyContent::Center, ..default() }, Pickable::IGNORE))
                .with_child((
                    anim(3.2, 0.8, Fx::Fade(Color::srgba(0.7, 0.62, 0.5, 0.6))),
                    ui_text(&fonts.body, "Unofficial fan project · Rules engine: Forge (GPL-3) · Card images © Wizards of the Coast via Scryfall · hover or click the cards", 13.0, Color::NONE),
                ));
            // player chip: your portrait + name; opens the avatar studio
            root.spawn((
                anim(3.0, 0.6, Fx::SlideY(-90.0)),
                UiTransform::from_translation(Val2::px(0.0, -90.0)),
                Button,
                MenuAction::Avatar,
                chip(Node {
                    position_type: PositionType::Absolute,
                    top: Val::Px(18.0),
                    right: Val::Px(22.0),
                    padding: UiRect::new(Val::Px(6.0), Val::Px(18.0), Val::Px(6.0), Val::Px(6.0)),
                    column_gap: Val::Px(12.0),
                    align_items: AlignItems::Center,
                    ..default()
                }),
            ))
            .with_children(|c| {
                c.spawn((
                    Node { width: Val::Px(54.0), height: Val::Px(54.0), border: UiRect::all(Val::Px(2.5)), border_radius: BorderRadius::MAX, overflow: Overflow::clip(), ..default() },
                    BorderColor::all(GOLD),
                    BackgroundColor(Color::srgb(0.05, 0.035, 0.03)),
                    ImageNode::new(TRANSPARENT_IMAGE_HANDLE),
                    AvatarArt { avatar: Some(settings.shown_avatar()), fallback: TRANSPARENT_IMAGE_HANDLE },
                    Pickable::IGNORE,
                ));
                c.spawn(Node { flex_direction: FlexDirection::Column, ..default() }).with_children(|t| {
                    t.spawn((ui_text(&fonts.title, settings.player_name.clone(), 18.0, GOLD), Pickable::IGNORE));
                    t.spawn((ui_text(&fonts.body, "Change avatar", 12.0, TEXT_DIM), Pickable::IGNORE));
                });
            });
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

fn ease_out_quart(x: f32) -> f32 {
    1.0 - (1.0 - x.clamp(0.0, 1.0)).powi(4)
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
        Option<&mut ButtonReveal>,
    )>,
) {
    let t = ui_t(&clock);
    for (a, tf, tc, ts, mut node, bg, shadow, rv) in &mut q {
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
                    tf.scale = Vec2::splat(from + (1.0 - from) * ease_out_quart(p));
                }
            }
            Fx::Reveal(rise) => {
                // opacity on a gentle S-curve, position on a long ease-out: the button is mostly
                // visible before it finishes settling, which reads as one soft motion
                if let Some(mut rv) = rv {
                    rv.0 = p * p * (3.0 - 2.0 * p);
                }
                if let Some(mut tf) = tf {
                    tf.translation.y = Val::Px(rise * (1.0 - ease_out_quart(p)));
                }
            }
            Fx::Grow(w) => node.width = Val::Px(w * ease_out_cubic(p)),
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

/// Preserve the menu's proportions on compact windows, including during resizing.
/// Gameplay uses its own viewport-based sizing and restores the default scale.
fn fit_main_menu(state: Res<State<AppState>>, windows: Query<&Window>, mut scale: ResMut<UiScale>) {
    let Ok(window) = windows.single() else { return };
    let desired = if *state.get() == AppState::MainMenu {
        (window.width() / 1200.0).min(window.height() / 850.0).min(1.0)
    } else {
        1.0
    };
    if scale.0 != desired {
        scale.0 = desired;
    }
}
