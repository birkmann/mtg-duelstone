//! Menus: Boot splash (over a full-screen card-art showcase), Main menu (over a 3D portal stage), Deck select (+ deck import),
//! Multiplayer (host / join by invite) and the table Lobby, Settings overlay, Loading/VS
//! screen and the GameOver banner — all `bevy_ui`, styled with `style`.
//!
//! Why one `MenuAction` enum: every clickable thing in the menus carries one, and a single system
//! turns clicks into state transitions / resource edits. Screens only *spawn* UI; they never
//! contain click logic, which keeps the flow readable in one place (`actions`).
//!
//! App state transitions are owned by `app_state`; we only `NextState::set` from button clicks
//! (MainMenu → DeckSelect → Loading, GameOver → Loading/MainMenu).

mod actions;
mod avatar_studio;
mod backdrop;
mod boot;
mod builder;
mod deck_art;
mod deck_select;
mod designer;
mod game_over;
mod cubes;
mod event;
mod event_setup;
mod loading;
mod lobby;
mod main_menu;
mod modes;
mod multiplayer;
mod settings_ui;
mod showcase;
mod stage;
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
    /// Open the deck builder on an empty deck of the current format.
    NewDeck,
    /// Open the deck builder on the selected deck (non-user decks are saved as a copy).
    EditDeck,
    // import panel
    ImportAnalyze,
    ImportSave,
    ImportClose,
    // settings
    CloseSettings,
    ToggleFullscreen,
    ToggleAutolaunch,
    SetTheme(&'static str),
    /// Open the avatar studio (main menu player chip).
    Avatar,
    // design studio
    Customize,
    CloseDesigner,
    // game over
    Rematch,
    ToMainMenu,
    // multiplayer (handled in `multiplayer` / `lobby`)
    Multiplayer,
    /// Table size for hosting (2..=4 players including you).
    SetTableSize(u8),
    HostTable,
    JoinTable,
    CopyInvite,
    /// Host: fill this seat with an AI.
    SeatAi(i64),
    /// Host: reopen this seat (removes a player or AI).
    SeatOpen(i64),
    StartTable,
    LeaveTable,
    // mode select (handled in `modes`)
    SetFormat(mtg_protocol::Format),
    SetLegality(Option<String>),
    ToggleVariant(mtg_protocol::Variant),
    OpenEvent(mtg_protocol::EventKind),
    ModeBack,
    ModeContinue,
    /// deck select: back to the mode screen
    ChangeMode,
    // event setup / event (handled in `event_setup` / `event`)
    PickSource(String),
    SetPodSize(u8),
    OpenPod,
    EventStart,
    EventSeat(i64, bool),
    Pick(i64),
    ToggleMain(i64),
    Basic(char, i32),
    SuggestDeck,
    SubmitDeck,
    DealGames(mtg_protocol::Pairing, u8),
    LeaveEvent,
    ReturnToPod,
    /// open the cube designer (its own clicks are `cubes::CubeAction`)
    Cubes,
}

/// UI-only state shared by the screens.
#[derive(Resource, Debug, Default)]
pub struct MenuState {
    pub settings_open: bool,
    pub import_open: bool,
    /// Where the design studio returns to (it opens from the main menu and deck select).
    pub designer_return: Option<AppState>,
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
                showcase::ShowcasePlugin,
                stage::StagePlugin,
            ))
            .add_plugins((builder::BuilderPlugin, deck_art::DeckArtPlugin))
            .add_plugins((designer::DesignerPlugin, avatar_studio::AvatarStudioPlugin))
            .add_plugins((multiplayer::MultiplayerPlugin, lobby::LobbyPlugin))
            .add_plugins((modes::ModesPlugin, event_setup::EventSetupPlugin, event::EventPlugin, cubes::CubesPlugin))
            .init_resource::<crate::shared::EventSetupChoice>()
            .add_systems(Update, (actions::handle_actions, actions::debug_jump));
    }
}

/// Menu-ish states where the animated 2D backdrop is visible. The main menu has its own 3D stage.
pub(crate) fn backdrop_state(s: AppState) -> bool {
    matches!(
        s,
        AppState::Boot
            | AppState::DeckSelect
            | AppState::Loading
            | AppState::Multiplayer
            | AppState::Lobby
            | AppState::ModeSelect
            | AppState::EventSetup
            | AppState::Event
            | AppState::CubeDesigner
            | AppState::DeckBuilder
            | AppState::AvatarStudio
    )
}
