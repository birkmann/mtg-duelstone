//! Boot splash: logo, indeterminate progress shimmer, `BootStatus` message/error and a Retry button,
//! with a bounded central status panel that remains useful without downloaded showcase art.

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
#[derive(Component)]
struct LoadingDetail;
#[derive(Resource)]
struct BootStarted(f32);

fn spawn_boot(mut commands: Commands, fonts: Res<ArenaFonts>, time: Res<Time>) {
    commands.insert_resource(BootStarted(time.elapsed_secs()));
    commands.spawn((
        Name::new("UI/BootScreen"), DespawnOnExit(AppState::Boot),
        Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween, padding: UiRect::all(Val::VMin(5.0)),
            row_gap: Val::Vh(3.0), ..fullscreen_root() },
    )).with_children(|root| {
        root.spawn(Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, row_gap: Val::Vh(1.0), ..default() }).with_children(|top| {
            top.spawn((Text::new("MTG DUELSTONE"), TextFont { font_size: bevy::text::FontSize::VMin(6.4), ..text_font(&fonts.title, 64.0) }, TextColor(GOLD_BRIGHT)));
            top.spawn((Node { width: Val::Vw(34.0), height: Val::Px(2.0), ..default() }, BackgroundColor(GOLD_DIM)));
            top.spawn((ui_text(&fonts.body, "A FAN-MADE CLIENT · POWERED BY FORGE", 13.0, TEXT_DIM), TextLayout { justify: Justify::Center, ..default() }));
        });
        root.spawn((
            ornate_panel(Node { width: Val::Vw(76.0), max_width: Val::Px(720.0), max_height: Val::Vh(64.0),
                flex_direction: FlexDirection::Column, align_items: AlignItems::Center,
                padding: UiRect::all(Val::VMin(3.5)), row_gap: Val::Vh(1.8), ..default() }),
        )).with_children(|panel| {
            panel.spawn((ui_text(&fonts.title, "PREPARING YOUR TABLE", 20.0, GOLD_BRIGHT), TextLayout { justify: Justify::Center, ..default() }));
            panel.spawn((StatusText, ui_text(&fonts.body, "Starting…", 18.0, TEXT), TextLayout { justify: Justify::Center, ..default() }, Node { max_width: Val::Percent(100.0), ..default() }));
            panel.spawn((Node { width: Val::Percent(100.0), height: Val::Px(6.0), flex_shrink: 0.0,
                overflow: Overflow::clip(), border_radius: BorderRadius::all(Val::Px(99.0)), ..default() },
                BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.12)),
            )).with_children(|track| {
                track.spawn((Shimmer, Node { position_type: PositionType::Absolute, width: Val::Percent(35.0), height: Val::Percent(100.0), ..default() },
                    BackgroundGradient::from(LinearGradient::to_right(vec![
                        ColorStop::new(Color::srgba(0.95, 0.46, 0.13, 0.0), Val::Percent(0.0)),
                        ColorStop::new(ORANGE_HI, Val::Percent(60.0)),
                        ColorStop::new(Color::srgba(0.95, 0.46, 0.13, 0.0), Val::Percent(100.0)),
                    ]))));
            });
            panel.spawn((LoadingDetail, ui_text(&fonts.body, "The first launch can take a little longer.", 14.0, TEXT_DIM), TextLayout { justify: Justify::Center, ..default() }, Node { max_width: Val::Percent(100.0), ..default() }));
            panel.spawn(crate::hud::kit::scroll_node(Node { width: Val::Percent(100.0), max_height: Val::Vh(18.0), min_height: Val::Px(0.0), ..default() }))
                .insert(crate::hud::kit::ScrollArea).with_children(|area| {
                    area.spawn((ErrorText, ui_text(&fonts.body, "", 14.0, RED_DAMAGE), TextLayout { justify: Justify::Center, ..default() }, Node { max_width: Val::Percent(100.0), ..default() }));
                });
            panel.spawn((RetryRow, Node { display: Display::None, flex_shrink: 0.0, ..default() })).with_children(|row| {
                let b = spawn_button(row, &fonts, ButtonKind::Primary, "RETRY", Vec2::new(180.0, 44.0));
                row.commands().entity(b).insert(MenuAction::Retry);
            });
        });
        root.spawn((ui_text(&fonts.body, "Card art © Wizards of the Coast, via Scryfall", 12.0, TEXT_DIM), TextLayout { justify: Justify::Center, ..default() }));
    });
}

fn update_boot(
    time: Res<Time>,
    status: Res<BootStatus>,
    started: Res<BootStarted>,
    mut detail: Query<&mut Text, (With<LoadingDetail>, Without<StatusText>, Without<ErrorText>)>,
    mut shimmer: Query<&mut Node, (With<Shimmer>, Without<RetryRow>)>,
    mut status_text: Query<&mut Text, (With<StatusText>, Without<ErrorText>, Without<LoadingDetail>)>,
    mut err_text: Query<&mut Text, (With<ErrorText>, Without<StatusText>, Without<LoadingDetail>)>,
    mut retry: Query<&mut Node, (With<RetryRow>, Without<Shimmer>)>,
) {
    let t = time.elapsed_secs();
    let elapsed = (t - started.0).max(0.0) as u32;
    for mut text in &mut detail {
        text.0 = if status.error.is_some() { "Unable to finish loading. Check the message below, then retry.".into() }
            else if elapsed >= 15 { format!("Still working · {elapsed}s elapsed. The first launch can take up to 90 seconds.") }
            else { format!("{elapsed}s elapsed · Loading rules and card data for your games.") };
    }
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
