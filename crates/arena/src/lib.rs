//! MTG Arena–style client for the Forge engine. One plugin per module; modules talk only via
//! `shared` (the cross-plugin contract). See PLAN.md for the architecture.
#![allow(clippy::too_many_arguments, clippy::type_complexity)]

pub mod shared;

// board core
pub mod app_state;
pub mod cards;
pub mod debug;
pub mod hand_camera;
pub mod images;
pub mod layout;
pub mod motion;
pub mod net;
pub mod table;
pub mod view;
// interaction & HUD
pub mod combat;
pub mod hud;
pub mod interact;
pub mod prompts;
pub mod stack;
pub mod targeting;
// meta & polish
pub mod audio;
pub mod card_material;
pub mod fx;
pub mod menus;
pub mod settings;
pub mod style;

use bevy::prelude::*;

pub struct ArenaPlugin;

impl Plugin for ArenaPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            settings::SettingsPlugin,
            style::StylePlugin,
            app_state::AppStatePlugin,
            net::NetPlugin,
            view::ViewPlugin,
            images::ImagesPlugin,
            cards::CardsPlugin,
            layout::LayoutPlugin,
            motion::MotionPlugin,
            table::TablePlugin,
            hand_camera::HandCameraPlugin,
            debug::DebugPlugin,
        ));
        // Dev flag: board core only (no HUD/menus/fx) for isolating rendering problems.
        if std::env::var("ARENA_CORE_ONLY").is_ok() {
            return;
        }
        app.add_plugins((
            interact::InteractPlugin,
            hud::HudPlugin,
            prompts::PromptsPlugin,
            targeting::TargetingPlugin,
            combat::CombatPlugin,
            stack::StackPlugin,
        ))
        .add_plugins((
            menus::MenusPlugin,
            fx::FxPlugin,
            audio::AudioPlugin,
            card_material::CardMaterialPlugin,
        ));
    }
}
