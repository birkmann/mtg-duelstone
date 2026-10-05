//! Live targeting arrow: while the engine asks for targets for a source card, a glowing bezier
//! ribbon follows the cursor from that card and snaps to a hovered valid target. The ribbon
//! renderer (`ribbon`) is shared with combat and stack arrows.

pub mod ribbon;

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::hud::model::is_mana_payment;
use crate::shared::*;
pub use ribbon::{Arrow, ArrowQueue, card_anchor, cursor_on_table};

pub struct TargetingPlugin;

impl Plugin for TargetingPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(ribbon::RibbonPlugin)
            .init_resource::<CurrentInput>()
            .init_resource::<BoardAnchors>()
            .add_systems(Update, targeting_arrow.run_if(in_state(AppState::InMatch).or_else(in_state(AppState::GameOver))));
    }
}

/// Ground point of a player's avatar, slightly lifted so ribbons start above the mat.
pub fn player_anchor(view: &GameView, anchors: &BoardAnchors, pid: mtg_protocol::PlayerId) -> Vec3 {
    anchors.avatar(view.is_me(pid)) + Vec3::Y * 0.25
}

fn targeting_arrow(
    time: Res<Time>,
    input: Res<CurrentInput>,
    view: Res<GameView>,
    anchors: Res<BoardAnchors>,
    registry: Res<CardRegistry>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cam: Query<(&Camera, &GlobalTransform), With<TableCamera>>,
    cards: Query<(&GlobalTransform, Option<&TargetPlacement>)>,
    hovered: Query<(&CardEntity, &GlobalTransform), (With<Hovered>, With<Selectable>)>,
    mut queue: ResMut<ArrowQueue>,
) {
    let Some(f) = input.frame.as_ref() else { return };
    let Some(src_id) = f.card else { return };
    if (f.selectable.is_empty() && f.selectable_players.is_empty()) || is_mana_payment(&f.message) {
        return;
    }
    let from = card_anchor(src_id, &registry, &cards).unwrap_or(anchors.my_avatar + Vec3::Y * 0.3);
    let mut snapped = false;
    let mut to = cursor_on_table(&windows, &cam, 0.3);
    if let Some((ce, gt)) = hovered.iter().find(|(ce, _)| ce.0 != src_id) {
        let _ = ce;
        to = Some(gt.translation() + Vec3::Y * 0.1);
        snapped = true;
    } else if let (Some(p), Ok((c, g))) = (to, cam.single()) {
        // snap to a selectable player's avatar when the cursor is near it
        for pid in &f.selectable_players {
            let a = player_anchor(&view, &anchors, *pid);
            if let (Some(sa), Some(sp)) = (crate::shared::world_to_ui(c, g, a), crate::shared::world_to_ui(c, g, p)) {
                if sa.distance(sp) < 70.0 {
                    to = Some(a);
                    snapped = true;
                }
            }
        }
    }
    let Some(to) = to else { return };
    if to.distance(from) < 0.2 {
        return;
    }
    let pulse = 0.5 + 0.5 * (time.elapsed_secs() * 6.0).sin();
    let mut a = Arrow::new(from, to, Color::srgb(0.25, 0.95, 0.85));
    a.width = if snapped { 0.095 } else { 0.08 };
    a.intensity = if snapped { 1.25 + 0.25 * pulse } else { 1.0 };
    queue.0.push(a);
}
