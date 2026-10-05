//! One system that turns every menu button click into a state transition or resource edit.

use bevy::prelude::*;
use bevy::input_focus::InputFocus;
use bevy::text::EditableText;
use mtg_protocol::Format;

use super::deck_select::{DeckList, DeckSelection, ImportState, RefreshDecks, engine_base, start_import, start_save};
use super::widgets::Field;
use super::{MenuAction, MenuState};
use crate::shared::{AppState, EngineConfig, MatchConfig, MatchDeckNames, MatchMode, RetryBoot, Settings};
use crate::style::ArenaButton;

pub(super) fn handle_actions(
    q: Query<(&Interaction, &MenuAction, Option<&ArenaButton>), Changed<Interaction>>,
    (state, mut mode): (Res<State<AppState>>, ResMut<MatchMode>),
    mut next: ResMut<NextState<AppState>>,
    mut menu: ResMut<MenuState>,
    mut settings: ResMut<Settings>,
    mut exit: MessageWriter<AppExit>,
    mut retry: MessageWriter<RetryBoot>,
    mut refresh: MessageWriter<RefreshDecks>,
    mut sel: ResMut<DeckSelection>,
    list: Res<DeckList>,
    mut import: ResMut<ImportState>,
    mut cfg_match: ResMut<MatchConfig>,
    mut names: ResMut<MatchDeckNames>,
    engine: Option<Res<EngineConfig>>,
    fields: Query<(&Field, &EditableText)>,
    mut focus: ResMut<InputFocus>,
) {
    for (i, act, btn) in &q {
        if *i != Interaction::Pressed || btn.is_some_and(|b| b.disabled) {
            continue;
        }
        match act {
            MenuAction::Play => {
                *mode = MatchMode::VsAi;
                next.set(AppState::ModeSelect);
            }
            MenuAction::ChangeMode => next.set(AppState::ModeSelect),
            MenuAction::ReturnToPod => next.set(AppState::Event),
            MenuAction::Cubes => next.set(AppState::CubeDesigner),
            MenuAction::Decks => {
                menu.import_open = false;
                next.set(AppState::DeckSelect);
            }
            MenuAction::OpenSettings => menu.settings_open = true,
            MenuAction::Quit => {
                exit.write(AppExit::Success);
            }
            MenuAction::Retry => {
                retry.write(RetryBoot);
            }
            MenuAction::Back => {
                menu.import_open = false;
                // deck select is reached through the mode screen (joiners come from the multiplayer screen)
                let target = match (*state.get(), &*mode) {
                    (AppState::DeckSelect, MatchMode::Join { .. }) => AppState::Multiplayer,
                    (AppState::DeckSelect, _) => AppState::ModeSelect,
                    _ => AppState::MainMenu,
                };
                next.set(target);
            }
            MenuAction::FormatConstructed | MenuAction::FormatCommander => {
                let f = if *act == MenuAction::FormatCommander { Format::Commander } else { Format::Constructed };
                // a joiner plays the table's format
                if f != list.format && !matches!(*mode, MatchMode::Join { .. }) {
                    settings.last_format = f;
                    sel.human = None;
                    sel.ai = None;
                    refresh.write(RefreshDecks(f));
                }
            }
            MenuAction::SelectDeck(r) => sel.human = Some(r.clone()),
            MenuAction::SelectAi(r) => sel.ai = r.clone(),
            MenuAction::OpenImport => menu.import_open = true,
            MenuAction::ImportClose => menu.import_open = false,
            MenuAction::ImportAnalyze => {
                let text = field_text(&fields, Field::ImportText);
                start_import(&mut import, engine_base(&engine, &settings), text, list.format);
                focus.clear();
            }
            MenuAction::ImportSave => {
                let name = field_text(&fields, Field::ImportName);
                start_save(&mut import, engine_base(&engine, &settings), name, list.format);
                focus.clear();
            }
            MenuAction::StartMatch => {
                let Some(human) = sel.human.clone() else { continue };
                let ai = sel.ai.clone().or_else(|| random_ai(&list)).unwrap_or_else(|| human.clone());
                settings.last_deck.insert(list.format.as_str().to_string(), human.clone());
                settings.last_format = list.format;
                names.human = list.name_of(&human);
                names.ai = list.name_of(&ai);
                *cfg_match = MatchConfig { format: list.format, human_deck: human, ai_deck: ai, seed: None, setup: cfg_match.setup.clone() };
                next.set(AppState::Loading);
            }
            MenuAction::CloseSettings => menu.settings_open = false,
            MenuAction::ToggleFullscreen => settings.fullscreen = !settings.fullscreen,
            MenuAction::ToggleAutolaunch => settings.autolaunch = !settings.autolaunch,
            MenuAction::SetTheme(t) => settings.theme = (*t).to_string(),
            MenuAction::Rematch => next.set(AppState::Loading),
            MenuAction::ToMainMenu => next.set(AppState::MainMenu),
            MenuAction::Multiplayer
            | MenuAction::SetTableSize(_)
            | MenuAction::HostTable
            | MenuAction::JoinTable
            | MenuAction::CopyInvite
            | MenuAction::SeatAi(_)
            | MenuAction::SeatOpen(_)
            | MenuAction::StartTable
            | MenuAction::LeaveTable => {} // multiplayer::handle_mp_actions
            _ => {} // modes / event_setup / event / cubes handle their own
        }
    }
}

fn field_text(fields: &Query<(&Field, &EditableText)>, which: Field) -> String {
    fields.iter().find(|(f, _)| **f == which).map(|(_, t)| t.value().to_string()).unwrap_or_default()
}

fn random_ai(list: &DeckList) -> Option<String> {
    let valid: Vec<_> = list.decks.iter().filter(|d| d.problem.is_none()).collect();
    if valid.is_empty() {
        return None;
    }
    let n = mtg_client::now_millis() as usize;
    Some(valid[n % valid.len()].deck_ref.clone())
}

/// Dev aid for unattended screenshots: `ARENA_MENU_SCREEN=deck|import|settings|commander|multiplayer` jumps
/// straight to that screen once the main menu is reached.
pub(super) fn debug_jump(
    state: Res<State<AppState>>,
    mut next: ResMut<NextState<AppState>>,
    mut menu: ResMut<MenuState>,
    mut settings: ResMut<Settings>,
    mut done: Local<bool>,
) {
    if *done || *state.get() != AppState::MainMenu {
        return;
    }
    let Ok(which) = std::env::var("ARENA_MENU_SCREEN") else {
        *done = true;
        return;
    };
    *done = true;
    match which.as_str() {
        "deck" | "play" => next.set(AppState::DeckSelect),
        "commander" => {
            settings.last_format = Format::Commander;
            next.set(AppState::DeckSelect);
        }
        "import" => {
            menu.import_open = true;
            next.set(AppState::DeckSelect);
        }
        "settings" => menu.settings_open = true,
        "multiplayer" => next.set(AppState::Multiplayer),
        "modes" => next.set(AppState::ModeSelect),
        "cubes" => next.set(AppState::CubeDesigner),
        "draft" => next.set(AppState::EventSetup),
        _ => {}
    }
}
