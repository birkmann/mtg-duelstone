//! The 3D table: playmat, ornate frame, lights and the fixed perspective camera.
//!
//! The playmat's texture comes from `Cosmetics` (the equipped playmat: shipped art or a painted
//! design); until it lands the mat shows a plain dark felt.
//!
//! The camera pitch is fixed (~57°) like Arena's; only the distance changes so the whole mat fits
//! any window aspect (`fit_distance`). A hair of cursor parallax makes the scene feel alive.

use bevy::camera::visibility::RenderLayers;
use bevy::camera::{ClearColorConfig, Hdr};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::{CascadeShadowConfigBuilder, GlobalAmbientLight, NotShadowCaster, NotShadowReceiver};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::layout::{TABLE_HALF_D, TABLE_HALF_W};
use crate::shared::*;

pub struct TablePlugin;

pub const CAM_PITCH_DEG: f32 = 57.0;
pub const CAM_FOV_DEG: f32 = 38.0;
/// z the camera looks at (shifted toward the player so the hand has room).
pub const CAM_TARGET_Z: f32 = 0.25;
/// Mat half extents (slightly larger than the layout area).
pub const MAT_HALF_W: f32 = TABLE_HALF_W + 0.3;
pub const MAT_HALF_D: f32 = TABLE_HALF_D + 0.3;

impl Plugin for TablePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Settings>()
            .insert_resource(GlobalAmbientLight { color: Color::srgb(0.85, 0.9, 1.0), brightness: 260.0, ..default() })
            .insert_resource(ClearColor(Color::srgb(0.02, 0.015, 0.012)))
            .add_systems(Startup, spawn_table)
            .add_systems(Update, unify_hdr)
            .add_systems(Update, (fit_camera, apply_playmat).chain());
    }
}

#[derive(Component)]
struct PlayMat;

/// Smallest camera distance such that every point projects inside the viewport (`margin` of NDC).
pub fn fit_distance(aspect: f32, fov_deg: f32, pitch_deg: f32, target: Vec3, points: &[Vec3], margin: f32) -> f32 {
    let (fov, pitch) = (fov_deg.to_radians(), pitch_deg.to_radians());
    let proj = Mat4::perspective_rh(fov, aspect, 0.1, 200.0);
    let dir = Vec3::new(0.0, pitch.sin(), pitch.cos());
    let ok = |d: f32| {
        let eye = target + dir * d;
        let view = Mat4::look_at_rh(eye, target, Vec3::Y);
        points.iter().all(|p| {
            let c = proj * view * p.extend(1.0);
            c.w > 0.0 && (c.x / c.w).abs() <= margin && (c.y / c.w).abs() <= margin
        })
    };
    let (mut lo, mut hi) = (4.0_f32, 80.0_f32);
    for _ in 0..40 {
        let mid = (lo + hi) / 2.0;
        if ok(mid) {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    hi
}

/// Points that must stay on screen: the play area corners (the bottom ones sit above the hand).
fn must_see() -> Vec<Vec3> {
    let (w, near, far) = (TABLE_HALF_W - 0.05, 3.0, -3.1);
    vec![
        Vec3::new(-w, 0.0, far),
        Vec3::new(w, 0.0, far),
        Vec3::new(-w, 0.0, near),
        Vec3::new(w, 0.0, near),
        Vec3::new(0.0, 0.0, far),
    ]
}

fn camera_transform(aspect: f32, parallax: Vec2) -> Transform {
    let target = Vec3::new(0.0, 0.0, CAM_TARGET_Z);
    let d = fit_distance(aspect, CAM_FOV_DEG, CAM_PITCH_DEG, target, &must_see(), 0.985);
    let pitch = CAM_PITCH_DEG.to_radians();
    let eye = target + Vec3::new(0.0, pitch.sin(), pitch.cos()) * d + Vec3::new(parallax.x * 0.25, parallax.y * 0.12, 0.0);
    Transform::from_translation(eye).looking_at(target + Vec3::new(parallax.x * 0.1, 0.0, 0.0), Vec3::Y)
}

fn spawn_table(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    // ---- camera ----
    let aspect = 16.0 / 9.0;
    commands.spawn((
        TableCamera,
        Camera3d::default(),
        Camera { order: 0, clear_color: ClearColorConfig::Default, ..default() },
        Projection::Perspective(PerspectiveProjection { fov: CAM_FOV_DEG.to_radians(), ..default() }),
        camera_transform(aspect, Vec2::ZERO),
        Hdr,
        Tonemapping::TonyMcMapface,
        Bloom { intensity: 0.12, ..Bloom::NATURAL },
        Msaa::Sample4,
        RenderLayers::layer(TABLE_LAYER),
        Name::new("table camera"),
    ));

    // ---- lights ----
    commands.spawn((
        DirectionalLight {
            color: Color::srgb(1.0, 0.93, 0.8),
            illuminance: 3600.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(-4.0, 9.0, 3.0).looking_at(Vec3::new(0.0, 0.0, 0.0), Vec3::Y),
        CascadeShadowConfigBuilder { num_cascades: 1, maximum_distance: 40.0, first_cascade_far_bound: 40.0, ..default() }.build(),
        RenderLayers::layer(TABLE_LAYER),
        Name::new("key light"),
    ));
    commands.spawn((
        DirectionalLight { color: Color::srgb(0.6, 0.72, 1.0), illuminance: 700.0, shadow_maps_enabled: false, ..default() },
        Transform::from_xyz(5.0, 6.0, -4.0).looking_at(Vec3::ZERO, Vec3::Y),
        RenderLayers::layer(TABLE_LAYER),
        Name::new("fill light"),
    ));

    // ---- surround (so there's never void around the mat) ----
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(90.0, 60.0))),
        MeshMaterial3d(mats.add(StandardMaterial {
            base_color: Color::srgb(0.05, 0.035, 0.03),
            perceptual_roughness: 0.9,
            ..default()
        })),
        Transform::from_xyz(0.0, -0.16, 0.0),
        RenderLayers::layer(TABLE_LAYER),
        NotShadowCaster,
    ));

    // ---- mat ----
    commands.spawn((
        PlayMat,
        Mesh3d(meshes.add(Plane3d::default().mesh().size(MAT_HALF_W * 2.0, MAT_HALF_D * 2.0))),
        MeshMaterial3d(mats.add(StandardMaterial {
            base_color: Color::srgb(0.12, 0.08, 0.05),
            perceptual_roughness: 0.62,
            reflectance: 0.18,
            ..default()
        })),
        Transform::default(),
        RenderLayers::layer(TABLE_LAYER),
        NotShadowCaster,
        Name::new("playmat"),
    ));

    // ---- frame: dark gilded rails + glowing corner orbs ----
    let rail_mat = mats.add(StandardMaterial {
        base_color: Color::srgb(0.16, 0.10, 0.05),
        metallic: 0.85,
        perceptual_roughness: 0.32,
        ..default()
    });
    let trim_mat = mats.add(StandardMaterial {
        base_color: Color::srgb(0.62, 0.42, 0.16),
        metallic: 1.0,
        perceptual_roughness: 0.28,
        ..default()
    });
    let (rw, rd, th, hh) = (MAT_HALF_W, MAT_HALF_D, 0.5, 0.16);
    let rails = [
        (Vec3::new(0.0, hh / 2.0 - 0.02, -(rd + th / 2.0)), Vec3::new(rw * 2.0 + th * 2.0, hh, th)),
        (Vec3::new(0.0, hh / 2.0 - 0.02, rd + th / 2.0), Vec3::new(rw * 2.0 + th * 2.0, hh, th)),
        (Vec3::new(-(rw + th / 2.0), hh / 2.0 - 0.02, 0.0), Vec3::new(th, hh, rd * 2.0)),
        (Vec3::new(rw + th / 2.0, hh / 2.0 - 0.02, 0.0), Vec3::new(th, hh, rd * 2.0)),
    ];
    for (pos, size) in rails {
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(size.x, size.y, size.z))),
            MeshMaterial3d(rail_mat.clone()),
            Transform::from_translation(pos),
            RenderLayers::layer(TABLE_LAYER),
        ));
    }
    // thin bright inner trim
    let trims = [
        (Vec3::new(0.0, hh - 0.02, -rd - 0.03), Vec3::new(rw * 2.0 + 0.1, 0.03, 0.06)),
        (Vec3::new(0.0, hh - 0.02, rd + 0.03), Vec3::new(rw * 2.0 + 0.1, 0.03, 0.06)),
        (Vec3::new(-rw - 0.03, hh - 0.02, 0.0), Vec3::new(0.06, 0.03, rd * 2.0)),
        (Vec3::new(rw + 0.03, hh - 0.02, 0.0), Vec3::new(0.06, 0.03, rd * 2.0)),
    ];
    for (pos, size) in trims {
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(size.x, size.y, size.z))),
            MeshMaterial3d(trim_mat.clone()),
            Transform::from_translation(pos),
            RenderLayers::layer(TABLE_LAYER),
            NotShadowCaster,
        ));
    }
    let orb_mesh = meshes.add(Sphere::new(0.36).mesh().uv(32, 18));
    let orb_mat = mats.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.7, 0.25),
        emissive: LinearRgba::rgb(5.0, 2.6, 0.5),
        perceptual_roughness: 0.2,
        ..default()
    });
    let base_mesh = meshes.add(Cylinder::new(0.5, 0.2));
    for (sx, sz) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
        let p = Vec3::new(sx * (rw + 0.15), 0.0, sz * (rd + 0.15));
        commands.spawn((
            Mesh3d(base_mesh.clone()),
            MeshMaterial3d(trim_mat.clone()),
            Transform::from_translation(p + Vec3::Y * 0.1),
            RenderLayers::layer(TABLE_LAYER),
        ));
        commands.spawn((
            Mesh3d(orb_mesh.clone()),
            MeshMaterial3d(orb_mat.clone()),
            Transform::from_translation(p + Vec3::Y * 0.42),
            RenderLayers::layer(TABLE_LAYER),
            NotShadowCaster,
            NotShadowReceiver,
        ));
    }
}

/// Cameras that draw into the same window must agree on HDR, or the later ones wipe the frame.
fn unify_hdr(mut commands: Commands, cams: Query<Entity, (With<Camera>, Without<Hdr>)>) {
    for e in &cams {
        commands.entity(e).insert(Hdr);
    }
}

fn apply_playmat(
    cosmetics: Res<Cosmetics>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mat: Query<&MeshMaterial3d<StandardMaterial>, With<PlayMat>>,
    mut applied: Local<Option<Handle<Image>>>,
) {
    let Some(tex) = cosmetics.playmat.clone() else { return };
    if applied.as_ref() == Some(&tex) {
        return;
    }
    let Ok(m) = mat.single() else { return };
    let Some(mut m) = mats.get_mut(&m.0) else { return };
    m.base_color = Color::WHITE;
    m.base_color_texture = Some(tex.clone());
    // a playmat is cloth: the finish only nudges its sheen
    m.reflectance = match cosmetics.playmat_finish {
        crate::cosmetics::Finish::Matte => 0.18,
        _ => 0.3,
    };
    *applied = Some(tex);
}

fn fit_camera(
    windows: Query<&Window, With<PrimaryWindow>>,
    mut cam: Query<&mut Transform, With<TableCamera>>,
    mut smoothed: Local<Vec2>,
) {
    let Ok(w) = windows.single() else { return };
    let Ok(mut tf) = cam.single_mut() else { return };
    let aspect = w.width() / w.height().max(1.0);
    // cursor parallax, eased
    let want = w
        .cursor_position()
        .map(|c| Vec2::new(c.x / w.width() - 0.5, c.y / w.height() - 0.5) * 2.0)
        .unwrap_or(Vec2::ZERO)
        .clamp(Vec2::splat(-1.0), Vec2::splat(1.0));
    *smoothed = smoothed.lerp(want, 0.06);
    *tf = camera_transform(aspect, *smoothed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_fits_common_aspects() {
        let t = Vec3::new(0.0, 0.0, CAM_TARGET_Z);
        let wide = fit_distance(16.0 / 9.0, CAM_FOV_DEG, CAM_PITCH_DEG, t, &must_see(), 0.985);
        let tall = fit_distance(16.0 / 10.0, CAM_FOV_DEG, CAM_PITCH_DEG, t, &must_see(), 0.985);
        let boxy = fit_distance(4.0 / 3.0, CAM_FOV_DEG, CAM_PITCH_DEG, t, &must_see(), 0.985);
        assert!(wide <= tall && tall <= boxy);
        assert!(wide > 5.0 && boxy < 79.0);
    }
}

