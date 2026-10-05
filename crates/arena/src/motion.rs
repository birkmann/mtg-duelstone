//! Eases every card's `Transform` toward `TargetPlacement ∘ PoseOffset` with critically damped
//! springs (draw, play, die, tap all fall out of "layout changed"), lifts cards in an arc when they
//! change zones, and moves cards between the two render layers without a visual jump.
//!
//! The hand layer is its own orthographic world, so when a card's target layer changes we project
//! its current screen position through the old camera and unproject it into the new one.

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;

use crate::cards::Despawning;
use crate::shared::*;

pub struct MotionPlugin;

impl Plugin for MotionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Settings>().add_systems(
            PostUpdate,
            move_cards.before(TransformSystems::Propagate),
        );
    }
}

#[derive(Component, Debug, Clone)]
pub struct MotionState {
    vel: Vec3,
    scale_vel: f32,
    layer: usize,
    last_target: Option<Vec3>,
    /// (initial distance, arc height)
    arc: Option<(f32, f32)>,
    /// current arc lift baked into `Transform.translation.y` (kept out of the spring state)
    lift: f32,
}

impl Default for MotionState {
    fn default() -> Self {
        Self { vel: Vec3::ZERO, scale_vel: 0.0, layer: TABLE_LAYER, last_target: None, arc: None, lift: 0.0 }
    }
}

fn smooth_damp_vec(cur: Vec3, target: Vec3, vel: &mut Vec3, smooth: f32, dt: f32) -> Vec3 {
    let omega = 2.0 / smooth.max(1e-4);
    let x = omega * dt;
    let e = 1.0 / (1.0 + x + 0.48 * x * x + 0.235 * x * x * x);
    let change = cur - target;
    let temp = (*vel + omega * change) * dt;
    *vel = (*vel - omega * temp) * e;
    target + (change + temp) * e
}

fn smooth_damp_f(cur: f32, target: f32, vel: &mut f32, smooth: f32, dt: f32) -> f32 {
    let mut v = Vec3::new(*vel, 0.0, 0.0);
    let r = smooth_damp_vec(Vec3::new(cur, 0.0, 0.0), Vec3::new(target, 0.0, 0.0), &mut v, smooth, dt);
    *vel = v.x;
    r.x
}

/// Target pose after applying the hover/attack offset and the flip for face-down cards.
pub fn final_pose(tp: &TargetPlacement, off: Option<&PoseOffset>) -> (Vec3, Quat, f32) {
    let (ot, or, os) = off.map(|o| (o.translation, o.rotation, o.scale)).unwrap_or((Vec3::ZERO, Quat::IDENTITY, 1.0));
    let mut rot = or * tp.rotation;
    if !tp.face_up {
        // flip about the card's own vertical axis so the back faces up
        rot *= Quat::from_rotation_y(std::f32::consts::PI);
    }
    (tp.translation + ot, rot, tp.scale * os)
}

fn move_cards(
    mut commands: Commands,
    time: Res<Time>,
    settings: Res<Settings>,
    table_cam: Query<(&Camera, &GlobalTransform), With<TableCamera>>,
    hand_cam: Query<(&Camera, &GlobalTransform), With<HandCamera>>,
    mut cards: Query<
        (Entity, &TargetPlacement, Option<&PoseOffset>, &mut Transform, &mut MotionState, Option<&Children>),
        (Without<Dragging>, Without<Despawning>),
    >,
) {
    let dt = time.delta_secs().min(1.0 / 20.0);
    if dt <= 0.0 {
        return;
    }
    let speed = settings.anim_speed.clamp(0.25, 8.0);
    let (table, hand) = (table_cam.single().ok(), hand_cam.single().ok());
    for (e, tp, off, mut tf, mut ms, children) in &mut cards {
        let (target, rot, scale) = final_pose(tp, off);

        if ms.layer != tp.layer {
            if let (Some(t), Some(h)) = (table, hand) {
                let (from, to) = if ms.layer == HAND_LAYER { (h, t) } else { (t, h) };
                convert_layer(&mut tf, from, to, tp.layer == HAND_LAYER);
            }
            ms.layer = tp.layer;
            ms.vel = Vec3::ZERO;
            ms.scale_vel = 0.0;
            ms.arc = None;
            ms.lift = 0.0;
            let layers = RenderLayers::layer(tp.layer);
            commands.entity(e).insert(layers.clone());
            if let Some(ch) = children {
                for c in ch.iter() {
                    commands.entity(c).insert(layers.clone());
                }
            }
        }

        // A big jump in the rest position (zone change) starts an arc.
        let moved = ms.last_target.map(|l| l.distance(tp.translation)).unwrap_or(f32::MAX);
        if moved > 1.4 && tp.layer == TABLE_LAYER {
            let d0 = (tf.translation - Vec3::Y * ms.lift).distance(target).max(0.1);
            ms.arc = Some((d0, (0.12 * d0 + 0.18).min(1.1)));
        }
        ms.last_target = Some(tp.translation);

        let mut vel = ms.vel;
        let base = tf.translation - Vec3::Y * ms.lift;
        let pos = smooth_damp_vec(base, target, &mut vel, 0.16 / speed, dt);
        ms.vel = vel;
        let mut lift = 0.0;
        if let Some((d0, h)) = ms.arc {
            let p = (1.0 - pos.distance(target) / d0).clamp(0.0, 1.0);
            if p > 0.995 {
                ms.arc = None;
            } else {
                lift = h * 4.0 * p * (1.0 - p);
            }
        }
        ms.lift = lift;
        let pos = pos + Vec3::Y * lift;
        tf.translation = pos;
        let mut sv = ms.scale_vel;
        let s = smooth_damp_f(tf.scale.x, scale, &mut sv, 0.14 / speed, dt);
        ms.scale_vel = sv;
        tf.scale = Vec3::splat(s);
        let k = 1.0 - (-dt * 14.0 * speed).exp();
        tf.rotation = tf.rotation.slerp(rot, k);
    }
}

/// Keep a card visually in place while it changes between the table and hand worlds.
fn convert_layer(
    tf: &mut Transform,
    from: (&Camera, &GlobalTransform),
    to: (&Camera, &GlobalTransform),
    to_hand: bool,
) {
    let (fc, fg) = from;
    let (tc, tg) = to;
    let Ok(center) = fc.world_to_viewport(fg, tf.translation) else { return };
    let edge_world = tf.translation + tf.rotation * Vec3::X * (CARD_W * 0.5 * tf.scale.x);
    let half_px = fc.world_to_viewport(fg, edge_world).map(|p| (p - center).length()).unwrap_or(30.0).max(4.0);
    let Ok(ray) = tc.viewport_to_world(tg, center) else { return };
    let dir = *ray.direction;
    let new_pos = if to_hand {
        // plane z = 0.3 in the hand world
        let t = (0.3 - ray.origin.z) / dir.z;
        ray.origin + dir * t
    } else {
        // lifted plane above the mat
        let t = (0.7 - ray.origin.y) / dir.y;
        ray.origin + dir * t
    };
    // choose scale so that half-width in pixels matches
    let probe = new_pos + Vec3::X * (CARD_W * 0.5);
    let unit_px = tc
        .world_to_viewport(tg, new_pos)
        .ok()
        .zip(tc.world_to_viewport(tg, probe).ok())
        .map(|(a, b)| (b - a).length())
        .unwrap_or(half_px);
    tf.translation = new_pos;
    tf.scale = Vec3::splat((half_px / unit_px.max(1e-3)).clamp(0.2, 4.0));
    tf.rotation = if to_hand { Quat::IDENTITY } else { Quat::from_rotation_x(-0.9) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spring_converges_without_overshoot() {
        let mut v = Vec3::ZERO;
        let mut p = Vec3::ZERO;
        let target = Vec3::new(3.0, 1.0, -2.0);
        for _ in 0..240 {
            p = smooth_damp_vec(p, target, &mut v, 0.16, 1.0 / 60.0);
            assert!(p.x <= target.x + 1e-3);
        }
        assert!(p.distance(target) < 1e-2);
    }

    #[test]
    fn face_down_flips() {
        let tp = TargetPlacement { face_up: false, ..default() };
        let (_, rot, _) = final_pose(&tp, None);
        assert!(rot.angle_between(Quat::IDENTITY) > 3.0);
    }

    #[test]
    fn offsets_compose() {
        let tp = TargetPlacement { translation: Vec3::X, scale: 2.0, ..default() };
        let off = PoseOffset { translation: Vec3::Y, scale: 1.5, ..default() };
        let (p, _, s) = final_pose(&tp, Some(&off));
        assert_eq!(p, Vec3::new(1.0, 1.0, 0.0));
        assert_eq!(s, 3.0);
    }
}
