//! Boot splash: logo, indeterminate progress shimmer, `BootStatus` message/error and a Retry button.

use bevy::prelude::*;
use bevy::ui::{BackgroundGradient, ColorStop, LinearGradient};

use super::MenuAction;
use crate::shared::{AppState, ArenaFonts, BootStatus};
use crate::style::*;

pub struct BootPlugin;

impl Plugin for BootPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BootStatus>()
            .add_systems(OnEnter(AppState::Boot), spawn_boot)
            .add_systems(Update, update_boot.run_if(in_state(AppState::Boot)));
    }
}

#[derive(Component)]
struct StatusText;
#[derive(Component)]
struct ErrorText;
#[derive(Component)]
struct RetryRow;
#[derive(Component)]
struct Shimmer;

/// The big "MTG ARENA" wordmark, shared with the main menu.
pub fn logo(parent: &mut ChildSpawnerCommands, fonts: &ArenaFonts, size: f32) {
    parent
        .spawn(Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, ..default() })
        .with_children(|c| {
            c.spawn((
                Text::new("MTG ARENA"),
                text_font(&fonts.title, size),
                TextColor(GOLD_BRIGHT),
                TextShadow { offset: Vec2::new(0.0, 4.0), color: Color::srgba(0.0, 0.0, 0.0, 0.85) },
            ));
            c.spawn((
                Node { width: Val::Px(size * 6.0), height: Val::Px(3.0), margin: UiRect::top(Val::Px(4.0)), ..default() },
                BackgroundGradient::from(LinearGradient::to_right(vec![
                    ColorStop::new(Color::srgba(0.86, 0.68, 0.30, 0.0), Val::Percent(0.0)),
                    ColorStop::new(Color::srgb(1.0, 0.78, 0.35), Val::Percent(50.0)),
                    ColorStop::new(Color::srgba(0.86, 0.68, 0.30, 0.0), Val::Percent(100.0)),
                ])),
            ));
        });
}

fn spawn_boot(mut commands: Commands, fonts: Res<ArenaFonts>) {
    commands
        .spawn((
            Name::new("BootScreen"),
            DespawnOnExit(AppState::Boot),
            Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, justify_content: JustifyContent::Center, row_gap: Val::Px(14.0), ..fullscreen_root() },
        ))
        .with_children(|root| {
            logo(root, &fonts, 96.0);
            root.spawn((ui_text(&fonts.body, "A  FAN-MADE  CLIENT  ·  POWERED  BY  FORGE", 15.0, TEXT_DIM), Node { margin: UiRect::bottom(Val::Px(36.0)), ..default() }));
            // progress track
            root.spawn((
                Node { width: Val::Px(440.0), height: Val::Px(6.0), overflow: Overflow::clip(), border_radius: BorderRadius::all(Val::Px(99.0)), ..default() },
                BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.08)),
            ))
            .with_children(|t| {
                t.spawn((
                    Shimmer,
                    Node { position_type: PositionType::Absolute, left: Val::Percent(0.0), width: Val::Percent(35.0), height: Val::Percent(100.0), border_radius: BorderRadius::all(Val::Px(99.0)), ..default() },
                    BackgroundGradient::from(LinearGradient::to_right(vec![
                        ColorStop::new(Color::srgba(0.95, 0.46, 0.13, 0.0), Val::Percent(0.0)),
                        ColorStop::new(ORANGE_HI, Val::Percent(60.0)),
                        ColorStop::new(Color::srgba(0.95, 0.46, 0.13, 0.0), Val::Percent(100.0)),
                    ])),
                ));
            });
            root.spawn((StatusText, ui_text(&fonts.body, "Starting…", 18.0, TEXT), Node { margin: UiRect::top(Val::Px(10.0)), ..default() }));
            root.spawn((ErrorText, ui_text(&fonts.body, "", 16.0, RED_DAMAGE), TextLayout { justify: Justify::Center, ..default() }, Node { max_width: Val::Px(720.0), ..default() }));
            root.spawn((RetryRow, Node { display: Display::None, margin: UiRect::top(Val::Px(12.0)), ..default() })).with_children(|r| {
                let b = spawn_button(r, &fonts, ButtonKind::Primary, "RETRY", Vec2::new(200.0, 52.0));
                r.commands().entity(b).insert(MenuAction::Retry);
            });
        });
}

fn update_boot(
    time: Res<Time>,
    status: Res<BootStatus>,
    mut shimmer: Query<&mut Node, (With<Shimmer>, Without<RetryRow>)>,
    mut status_text: Query<&mut Text, (With<StatusText>, Without<ErrorText>)>,
    mut err_text: Query<&mut Text, (With<ErrorText>, Without<StatusText>)>,
    mut retry: Query<&mut Node, (With<RetryRow>, Without<Shimmer>)>,
) {
    let t = time.elapsed_secs();
    for mut n in &mut shimmer {
        n.left = Val::Percent(((t * 0.55).fract()) * 135.0 - 35.0);
    }
    let msg = if status.message.is_empty() { "Starting…" } else { status.message.as_str() };
    for mut s in &mut status_text {
        if s.0 != msg {
            s.0 = msg.to_string();
        }
    }
    let err = status.error.clone().unwrap_or_default();
    for mut s in &mut err_text {
        if s.0 != err {
            s.0 = err.clone();
        }
    }
    for mut n in &mut retry {
        let want = if status.error.is_some() { Display::Flex } else { Display::None };
        if n.display != want {
            n.display = want;
        }
    }
}
