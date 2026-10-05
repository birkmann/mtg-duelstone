//! Main menu: logo + Play / Decks / Settings / Quit over the animated backdrop, with a fanned
//! trio of card backs on the right for a bit of showroom feel.

use bevy::prelude::*;

use super::MenuAction;
use super::boot::logo;
use crate::shared::{AppState, ArenaFonts, Settings};
use crate::style::*;

pub struct MainMenuPlugin;

impl Plugin for MainMenuPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::MainMenu), spawn_main_menu)
            .add_systems(Update, bob_hero.run_if(in_state(AppState::MainMenu)));
    }
}

#[derive(Component)]
struct HeroCard {
    base_rot: f32,
    phase: f32,
}

fn spawn_main_menu(mut commands: Commands, fonts: Res<ArenaFonts>, assets: Res<AssetServer>, settings: Res<Settings>) {
    let back = assets.load::<Image>("textures/card_back.png");
    commands
        .spawn((
            Name::new("MainMenu"),
            DespawnOnExit(AppState::MainMenu),
            Node { align_items: AlignItems::Center, ..fullscreen_root() },
        ))
        .with_children(|root| {
            // left column
            root.spawn(Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, margin: UiRect::left(Val::Percent(9.0)), row_gap: Val::Px(14.0), ..default() })
                .with_children(|col| {
                    logo(col, &fonts, 74.0);
                    col.spawn((Node { margin: UiRect::new(Val::Px(0.0), Val::Px(0.0), Val::Px(4.0), Val::Px(30.0)), ..default() }, ui_text(&fonts.body, "Constructed  ·  Commander  ·  vs the Forge AI", 16.0, TEXT_DIM)));
                    let p = spawn_button(col, &fonts, ButtonKind::Primary, "PLAY", Vec2::new(340.0, 72.0));
                    col.commands().entity(p).insert(MenuAction::Play);
                    for (label, act) in [("DECKS", MenuAction::Decks), ("SETTINGS", MenuAction::OpenSettings), ("QUIT", MenuAction::Quit)] {
                        let kind = if act == MenuAction::Quit { ButtonKind::Ghost } else { ButtonKind::Secondary };
                        let b = spawn_button(col, &fonts, kind, label, Vec2::new(340.0, 54.0));
                        col.commands().entity(b).insert(act);
                    }
                });
            // hero fan
            for (i, (rot, dx, dy)) in [(-14.0f32, -150.0f32, 20.0f32), (0.0, 0.0, -10.0), (14.0, 150.0, 20.0)].into_iter().enumerate() {
                root.spawn((
                    HeroCard { base_rot: rot, phase: i as f32 * 1.7 },
                    Node { position_type: PositionType::Absolute, right: Val::Percent(17.0), top: Val::Percent(24.0), width: Val::Px(280.0), height: Val::Px(390.0), ..default() },
                    ImageNode::new(back.clone()),
                    BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.7), Val::Px(0.0), Val::Px(16.0), Val::Px(0.0), Val::Px(30.0)),
                    UiTransform { translation: Val2::px(dx, dy), rotation: Rot2::degrees(rot), scale: Vec2::ONE },
                    GlobalZIndex(i as i32),
                ));
            }
            // footer
            root.spawn(Node { position_type: PositionType::Absolute, bottom: Val::Px(14.0), left: Val::Px(0.0), width: Val::Percent(100.0), justify_content: JustifyContent::Center, ..default() })
                .with_child(ui_text(
                    &fonts.body,
                    "Unofficial fan project · Rules engine: Forge (GPL-3) · Card images © Wizards of the Coast via Scryfall",
                    13.0,
                    Color::srgba(0.7, 0.62, 0.5, 0.6),
                ));
            // player chip
            root.spawn((
                chip(Node { position_type: PositionType::Absolute, top: Val::Px(18.0), right: Val::Px(22.0), padding: UiRect::axes(Val::Px(18.0), Val::Px(8.0)), ..default() }),
            ))
            .with_child(ui_text(&fonts.title, settings.player_name.clone(), 18.0, GOLD));
        });
}

fn bob_hero(time: Res<Time>, mut q: Query<(&HeroCard, &mut UiTransform)>) {
    let t = time.elapsed_secs();
    for (h, mut tf) in &mut q {
        tf.rotation = Rot2::degrees(h.base_rot + (t * 0.6 + h.phase).sin() * 1.6);
    }
}
