//! Settings overlay (usable from any state: main menu button, F10, or an `OpenSettings` message).

use bevy::prelude::*;

use super::widgets::{Field, SliderKey, spawn_slider, text_field};
use super::{MenuAction, MenuState};
use crate::shared::{ArenaFonts, OpenSettings, Settings};
use crate::style::*;

pub struct SettingsUiPlugin;

impl Plugin for SettingsUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (open_requests, sync_panel, update_toggles).chain());
    }
}

#[derive(Component)]
struct SettingsPanel;
#[derive(Component)]
struct ToggleText(MenuAction);
#[derive(Component)]
struct ThemeButton(&'static str);

pub const THEMES: [(&str, &str); 3] = [("sanctum", "SANCTUM"), ("jungle", "JUNGLE"), ("storm", "STORM")];

fn open_requests(mut msgs: MessageReader<OpenSettings>, keys: Res<ButtonInput<KeyCode>>, mut menu: ResMut<MenuState>) {
    if msgs.read().count() > 0 {
        menu.settings_open = true;
    }
    if keys.just_pressed(KeyCode::F10) {
        menu.settings_open = !menu.settings_open;
    }
    if menu.settings_open && keys.just_pressed(KeyCode::Escape) {
        menu.settings_open = false;
    }
}

fn sync_panel(mut commands: Commands, menu: Res<MenuState>, fonts: Res<ArenaFonts>, settings: Res<Settings>, existing: Query<Entity, With<SettingsPanel>>) {
    let has = !existing.is_empty();
    if menu.settings_open && !has {
        commands
            .spawn((
                SettingsPanel,
                GlobalZIndex(200),
                Node { justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..fullscreen_root() },
                BackgroundColor(SCRIM),
            ))
            .with_children(|scrim| {
                scrim
                    .spawn(ornate_panel(Node { width: Val::Px(640.0), padding: UiRect::all(Val::Px(30.0)), flex_direction: FlexDirection::Column, row_gap: Val::Px(4.0), ..default() }))
                    .with_children(|p| {
                        p.spawn(heading(&fonts, "SETTINGS", 36.0));
                        p.spawn(divider());
                        spawn_slider(p, &fonts, SliderKey::Anim, "Animation speed");
                        spawn_slider(p, &fonts, SliderKey::Master, "Master volume");
                        spawn_slider(p, &fonts, SliderKey::Sfx, "Effects volume");
                        spawn_slider(p, &fonts, SliderKey::Music, "Music volume");
                        p.spawn(divider());
                        row(p, &fonts, "Battlefield", |r| {
                            for (id, label) in THEMES {
                                let b = spawn_button(r, &fonts, ButtonKind::Secondary, label, Vec2::new(116.0, 38.0));
                                r.commands().entity(b).insert((MenuAction::SetTheme(id), ThemeButton(id)));
                            }
                        });
                        row(p, &fonts, "Fullscreen", |r| {
                            let b = spawn_button(r, &fonts, ButtonKind::Secondary, "", Vec2::new(116.0, 38.0));
                            r.commands().entity(b).insert(MenuAction::ToggleFullscreen).with_children(|c| {
                                c.spawn((ToggleText(MenuAction::ToggleFullscreen), ui_text(&fonts.title, "OFF", 16.0, GOLD_BRIGHT)));
                            });
                        });
                        row(p, &fonts, "Player name", |r| {
                            r.spawn(text_field(&fonts, Field::PlayerName, &settings.player_name, Val::Px(300.0), 1.0, false));
                        });
                        row(p, &fonts, "Engine URL", |r| {
                            r.spawn(text_field(&fonts, Field::EngineUrl, &settings.engine_url, Val::Px(300.0), 1.0, false));
                        });
                        row(p, &fonts, "Auto-launch engine", |r| {
                            let b = spawn_button(r, &fonts, ButtonKind::Secondary, "", Vec2::new(116.0, 38.0));
                            r.commands().entity(b).insert(MenuAction::ToggleAutolaunch).with_children(|c| {
                                c.spawn((ToggleText(MenuAction::ToggleAutolaunch), ui_text(&fonts.title, "ON", 16.0, GOLD_BRIGHT)));
                            });
                            r.spawn(ui_text(&fonts.body, "engine settings apply on restart", 13.0, TEXT_DIM));
                        });
                        p.spawn(Node { justify_content: JustifyContent::FlexEnd, margin: UiRect::top(Val::Px(14.0)), ..default() }).with_children(|r| {
                            let b = spawn_button(r, &fonts, ButtonKind::Primary, "DONE", Vec2::new(200.0, 52.0));
                            r.commands().entity(b).insert(MenuAction::CloseSettings);
                        });
                    });
            });
    } else if !menu.settings_open && has {
        for e in &existing {
            commands.entity(e).despawn();
        }
    }
}

fn row(p: &mut ChildSpawnerCommands, fonts: &ArenaFonts, label: &str, f: impl FnOnce(&mut ChildSpawnerCommands)) {
    p.spawn(Node { width: Val::Percent(100.0), align_items: AlignItems::Center, column_gap: Val::Px(12.0), margin: UiRect::vertical(Val::Px(5.0)), ..default() })
        .with_children(|r| {
            r.spawn((Node { width: Val::Px(186.0), ..default() }, ui_text(&fonts.body, label, 18.0, TEXT)));
            f(r);
        });
}

fn update_toggles(
    settings: Res<Settings>,
    mut texts: Query<(&ToggleText, &mut Text)>,
    mut themes: Query<(&ThemeButton, &mut ArenaButton)>,
) {
    for (t, mut text) in &mut texts {
        let on = match t.0 {
            MenuAction::ToggleFullscreen => settings.fullscreen,
            _ => settings.autolaunch,
        };
        let s = if on { "ON" } else { "OFF" };
        if text.0 != s {
            text.0 = s.into();
        }
    }
    for (t, mut b) in &mut themes {
        let want = if settings.theme == t.0 { ButtonKind::Primary } else { ButtonKind::Secondary };
        if b.kind != want {
            b.kind = want;
        }
    }
}
