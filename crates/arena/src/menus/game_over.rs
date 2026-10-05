//! GameOver overlay: VICTORY / DEFEAT banner sweeping in over the final board, with Rematch and
//! Main Menu. (Confetti / rays live in `fx`, keyed off `BoardEvent::GameOver`.)

use bevy::prelude::*;
use bevy::ui::{BackgroundGradient, ColorStop, LinearGradient};

use super::MenuAction;
use crate::shared::{AppState, ArenaFonts, GameResult, GameView, MatchMode};
use crate::style::*;

pub struct GameOverPlugin;

impl Plugin for GameOverPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::GameOver), spawn_overlay).add_systems(Update, animate.run_if(in_state(AppState::GameOver)));
    }
}

#[derive(Component)]
struct Banner;
#[derive(Component)]
struct BannerText;
#[derive(Component)]
struct FadeIn(f32);

fn spawn_overlay(mut commands: Commands, fonts: Res<ArenaFonts>, result: Res<GameResult>, view: Res<GameView>, mode: Res<MatchMode>) {
    let won = result.0.as_ref().is_some_and(|r| r.you_won);
    let table = view.is_multiplayer();
    let winner = result.0.as_ref().and_then(|r| r.winner.clone()).filter(|w| !w.is_empty());
    let sub = match (won, table, winner) {
        (true, true, _) => "Last one standing".to_string(),
        (true, false, _) => "You defeated your opponent".to_string(),
        (false, _, Some(w)) => format!("{w} wins"),
        (false, true, None) => "You're out — the table plays on behind this screen".to_string(),
        (false, false, None) => "Better luck next time".to_string(),
    };
    let (title, col) = if won { ("VICTORY", Color::srgb(1.0, 0.82, 0.38)) } else { ("DEFEAT", Color::srgb(0.9, 0.3, 0.25)) };
    let band = |c: Color| {
        let s = c.to_srgba();
        BackgroundGradient::from(LinearGradient::to_right(vec![
            ColorStop::new(Color::srgba(s.red * 0.3, s.green * 0.3, s.blue * 0.3, 0.0), Val::Percent(0.0)),
            ColorStop::new(Color::srgba(s.red * 0.3, s.green * 0.3, s.blue * 0.3, 0.88), Val::Percent(22.0)),
            ColorStop::new(Color::srgba(s.red * 0.3, s.green * 0.3, s.blue * 0.3, 0.88), Val::Percent(78.0)),
            ColorStop::new(Color::srgba(s.red * 0.3, s.green * 0.3, s.blue * 0.3, 0.0), Val::Percent(100.0)),
        ]))
    };
    commands
        .spawn((
            Name::new("GameOverOverlay"),
            DespawnOnExit(AppState::GameOver),
            GlobalZIndex(50),
            Node { flex_direction: FlexDirection::Column, justify_content: JustifyContent::Center, align_items: AlignItems::Center, row_gap: Val::Px(34.0), ..fullscreen_root() },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.0)),
            FadeIn(0.0),
        ))
        .with_children(|root| {
            root.spawn((Banner, Node { width: Val::Percent(100.0), height: Val::Px(210.0), flex_direction: FlexDirection::Column, justify_content: JustifyContent::Center, align_items: AlignItems::Center, row_gap: Val::Px(6.0), ..default() }, band(col), UiTransform::from_scale(Vec2::new(1.0, 0.0))))
                .with_children(|b| {
                    b.spawn((
                        BannerText,
                        Text::new(title),
                        text_font(&fonts.title, 110.0),
                        TextColor(col),
                        TextShadow { offset: Vec2::new(0.0, 5.0), color: Color::srgba(0.0, 0.0, 0.0, 0.9) },
                        UiTransform::from_scale(Vec2::splat(2.2)),
                    ));
                    b.spawn(ui_text(&fonts.body, sub, 20.0, TEXT));
                });
            root.spawn(Node { column_gap: Val::Px(22.0), ..default() }).with_children(|r| {
                // a table can't be re-dealt from one seat; its host opens a new one
                if !mode.is_multiplayer() {
                    let b = spawn_button(r, &fonts, ButtonKind::Primary, "REMATCH", Vec2::new(280.0, 64.0));
                    r.commands().entity(b).insert(MenuAction::Rematch);
                }
                let b = spawn_button(r, &fonts, ButtonKind::Secondary, "MAIN MENU", Vec2::new(240.0, 64.0));
                r.commands().entity(b).insert(MenuAction::ToMainMenu);
            });
        });
}

fn animate(
    time: Res<Time>,
    mut root: Query<(&mut FadeIn, &mut BackgroundColor)>,
    mut banner: Query<&mut UiTransform, (With<Banner>, Without<BannerText>)>,
    mut text: Query<&mut UiTransform, (With<BannerText>, Without<Banner>)>,
) {
    for (mut f, mut bg) in &mut root {
        f.0 += time.delta_secs();
        bg.0 = Color::srgba(0.0, 0.0, 0.0, 0.55 * (f.0 / 0.5).min(1.0));
        let e = |p: f32| 1.0 - (1.0 - p.clamp(0.0, 1.0)).powi(3);
        for mut t in &mut banner {
            t.scale = Vec2::new(1.0, e(f.0 / 0.35));
        }
        for mut t in &mut text {
            let s = 1.0 + 1.2 * (1.0 - e((f.0 - 0.15) / 0.5));
            t.scale = Vec2::splat(s);
        }
    }
}
