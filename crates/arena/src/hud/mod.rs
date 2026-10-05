//! HUD (bevy_ui): avatars + life orbs, phase track, action buttons, prompt bar, hover preview,
//! projected card overlays (P/T, counters, xN), game log + gear menu. Everything here only
//! *renders* engine state or sends `SendCmd` frames; the engine decides legality.
//!
//! Sizes are in `vh` so the HUD scales with the window; anything tied to a card is positioned each
//! frame from the card's `GlobalTransform` projected through the table camera.

pub mod avatar;
pub mod dev;
pub mod buttons;
pub mod kit;
pub mod menu;
pub mod model;
pub mod overlays;
pub mod phase;
pub mod portrait;
pub mod preview;

use bevy::prelude::*;
use bevy::transform::TransformSystems;
use bevy::ui::UiSystems;

use crate::shared::AppState;

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        let in_match = in_state(AppState::InMatch).or_else(in_state(AppState::GameOver));
        app.init_resource::<crate::shared::ArenaFonts>()
            .init_resource::<avatar::AvatarAnim>()
            .init_resource::<phase::PhaseUi>()
            .init_resource::<menu::MenuUi>()
            .init_resource::<overlays::OverlayMap>()
            .init_resource::<crate::shared::HoverPreview>()
            .init_resource::<crate::shared::CurrentInput>()
            .init_resource::<crate::shared::GameLog>()
            .init_resource::<crate::shared::CardCache>()
            .init_resource::<crate::shared::BoardAnchors>()
            .add_plugins((kit::KitPlugin, dev::DevPlugin))
            .add_systems(OnEnter(AppState::Boot), cleanup_hud)
            .add_systems(OnEnter(AppState::MainMenu), cleanup_hud)
            .add_systems(OnEnter(AppState::DeckSelect), cleanup_hud)
            .add_systems(OnEnter(AppState::Loading), cleanup_hud)
            .add_systems(
                OnEnter(AppState::InMatch),
                (
                    cleanup_hud,
                    avatar::spawn_avatars,
                    avatar::spawn_pile_labels,
                    buttons::spawn_buttons,
                    menu::spawn_menu,
                    preview::spawn_preview,
                    phase::spawn_ladder,
                ),
            )
            .add_systems(
                Update,
                (
                    avatar::update_avatars,
                    avatar::update_mana_pools,
                    phase::attach_phase_tracks,
                    phase::update_phase_icons,
                    phase::update_strip,
                    phase::update_ladder,
                    phase::handle_stop_acts,
                    buttons::update_buttons,
                    buttons::update_message_bar,
                    buttons::update_tags,
                    buttons::handle_button_acts,
                    menu::handle_menu_acts,
                    menu::update_menu,
                    preview::update_preview,
                    preview::update_fallback.after(kit::update_card_pics),
                    overlays::sync_overlays,
                )
                    .run_if(in_match.clone()),
            )
            .add_systems(
                PostUpdate,
                (avatar::position_avatars, avatar::update_pile_labels, overlays::position_overlays)
                    .after(TransformSystems::Propagate)
                    .before(UiSystems::Prepare)
                    .run_if(in_match),
            );
    }
}

/// Tear down every HUD entity (match ended / about to start) so a rematch starts clean.
fn cleanup_hud(
    mut commands: Commands,
    q: Query<Entity, With<kit::HudEntity>>,
    mut overlays: ResMut<overlays::OverlayMap>,
    mut stack: ResMut<crate::stack::StackUiMap>,
    mut drag: ResMut<crate::interact::DragState>,
    mut inspect: ResMut<crate::interact::InspectCard>,
    mut opt: ResMut<crate::combat::OptimisticAttack>,
) {
    for e in &q {
        commands.entity(e).despawn();
    }
    overlays.0.clear();
    stack.0.clear();
    *drag = Default::default();
    inspect.0 = None;
    *opt = Default::default();
}
