//! Menus: Boot splash, Main menu, Deck select (+ deck import), Settings overlay, Loading/VS
//! screen and the GameOver banner — all `bevy_ui`, styled with `style`.
//!
//! Why one `MenuAction` enum: every clickable thing in the menus carries one, and a single system
//! turns clicks into state transitions / resource edits. Screens only *spawn* UI; they never
//! contain click logic, which keeps the flow readable in one place (`actions`).
//!
//! App state transitions are owned by `app_state`; we only `NextState::set` from button clicks
//! (MainMenu → DeckSelect → Loading, GameOver → Loading/MainMenu).

mod actions;
mod backdrop;
mod boot;
mod deck_select;
mod game_over;
mod loading;
mod main_menu;
mod settings_ui;
pub mod widgets;

use bevy::prelude::*;

use crate::shared::{AppState, MatchDeckNames, OpenSettings, RetryBoot};

pub use deck_select::DeckList;

/// What a menu button does. Attached to `Button` entities next to `ArenaButton`.
#[derive(Component, Debug, Clone, PartialEq)]
pub enum MenuAction {
    // main menu
    Play,
    Decks,
    OpenSettings,
    Quit,
    // boot
    Retry,
    // deck select
    Back,
    StartMatch,
    FormatConstructed,
    FormatCommander,
    SelectDeck(String),
    /// `None` = Random
    SelectAi(Option<String>),
    OpenImport,
    // import panel
    ImportAnalyze,
    ImportSave,
    ImportClose,
    // settings
    CloseSettings,
    ToggleFullscreen,
    ToggleAutolaunch,
    SetTheme(&'static str),
    // game over
    Rematch,
    ToMainMenu,
}

/// UI-only state shared by the screens.
#[derive(Resource, Debug, Default)]
pub struct MenuState {
    pub settings_open: bool,
    pub import_open: bool,
}

pub struct MenusPlugin;

impl Plugin for MenusPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MenuState>()
            .init_resource::<MatchDeckNames>()
            .add_message::<RetryBoot>()
            .add_message::<OpenSettings>()
            .add_plugins((
                backdrop::BackdropPlugin,
                boot::BootPlugin,
                main_menu::MainMenuPlugin,
                deck_select::DeckSelectPlugin,
                settings_ui::SettingsUiPlugin,
                loading::LoadingPlugin,
                game_over::GameOverPlugin,
                widgets::WidgetsPlugin,
            ))
            .add_systems(Update, (actions::handle_actions, actions::debug_jump));
    }
}

/// Menu-ish states where the animated backdrop is visible.
pub(crate) fn backdrop_state(s: AppState) -> bool {
    matches!(s, AppState::Boot | AppState::MainMenu | AppState::DeckSelect | AppState::Loading)
}
