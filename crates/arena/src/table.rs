//! The 3D table: playmat, ornate frame, lights and the fixed perspective camera.
//!
//! The camera pitch is fixed (~57°) like Arena's; only the distance changes so the whole mat fits
//! any window aspect (`fit_distance`). A hair of cursor parallax makes the scene feel alive.

use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{ClearColorConfig, Hdr};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::{CascadeShadowConfigBuilder, GlobalAmbientLight, NotShadowCaster, NotShadowReceiver};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
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
            .add_systems(Update, (fit_camera, rebuild_mat_on_theme).chain());
    }
}

#[derive(Component)]
struct PlayMat;

#[derive(Resource, Default)]
struct MatTheme(String);

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
    mut images: ResMut<Assets<Image>>,
    asset_server: Res<AssetServer>,
    settings: Res<Settings>,
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
    let theme = settings.theme.clone();
    let tex = mat_texture(&theme, &mut images, &asset_server);
    commands.spawn((
        PlayMat,
        Mesh3d(meshes.add(Plane3d::default().mesh().size(MAT_HALF_W * 2.0, MAT_HALF_D * 2.0))),
        MeshMaterial3d(mats.add(StandardMaterial {
            base_color_texture: Some(tex),
            perceptual_roughness: 0.62,
            reflectance: 0.18,
            ..default()
        })),
        Transform::default(),
        RenderLayers::layer(TABLE_LAYER),
        NotShadowCaster,
        Name::new("playmat"),
    ));
    commands.insert_resource(MatTheme(theme));

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

fn mat_texture(theme: &str, images: &mut Assets<Image>, asset_server: &AssetServer) -> Handle<Image> {
    let rel = format!("textures/playmat_{theme}.png");
    if crate::images::assets_dir().join(&rel).exists() {
        return asset_server.load(rel);
    }
    images.add(procedural_mat(theme))
}

fn rebuild_mat_on_theme(
    settings: Res<Settings>,
    mut theme: ResMut<MatTheme>,
    mut images: ResMut<Assets<Image>>,
    asset_server: Res<AssetServer>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mat: Query<&MeshMaterial3d<StandardMaterial>, With<PlayMat>>,
) {
    if settings.theme == theme.0 {
        return;
    }
    theme.0 = settings.theme.clone();
    let tex = mat_texture(&theme.0, &mut images, &asset_server);
    if let Ok(m) = mat.single() {
        if let Some(mut m) = mats.get_mut(&m.0) {
            m.base_color_texture = Some(tex);
        }
    }
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

// ---- procedural playmat ---------------------------------------------------------------------

fn hash(x: i32, y: i32) -> f32 {
    let mut h = (x as u32).wrapping_mul(374761393).wrapping_add((y as u32).wrapping_mul(668265263));
    h = (h ^ (h >> 13)).wrapping_mul(1274126177);
    ((h ^ (h >> 16)) & 0xffff) as f32 / 65535.0
}

fn vnoise(x: f32, y: f32) -> f32 {
    let (xi, yi) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (x - x.floor(), y - y.floor());
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let a = hash(xi, yi);
    let b = hash(xi + 1, yi);
    let c = hash(xi, yi + 1);
    let d = hash(xi + 1, yi + 1);
    (a + (b - a) * sx) + ((c + (d - c) * sx) - (a + (b - a) * sx)) * sy
}

fn fbm(x: f32, y: f32) -> f32 {
    vnoise(x, y) * 0.5 + vnoise(x * 2.0, y * 2.0) * 0.25 + vnoise(x * 4.0, y * 4.0) * 0.125 + vnoise(x * 8.0, y * 8.0) * 0.0625
}

struct Palette {
    dark: [f32; 3],
    mid: [f32; 3],
    light: [f32; 3],
    line: [f32; 3],
}

fn palette(theme: &str) -> Palette {
    match theme {
        "storm" => Palette {
            dark: [0.04, 0.06, 0.07],
            mid: [0.12, 0.17, 0.17],
            light: [0.22, 0.29, 0.28],
            line: [0.45, 0.55, 0.6],
        },
        "grove" => Palette {
            dark: [0.03, 0.07, 0.04],
            mid: [0.10, 0.20, 0.08],
            light: [0.22, 0.36, 0.14],
            line: [0.65, 0.75, 0.35],
        },
        _ => Palette {
            dark: [0.20, 0.10, 0.03],
            mid: [0.55, 0.32, 0.08],
            light: [0.80, 0.55, 0.16],
            line: [0.95, 0.76, 0.35],
        },
    }
}

/// Arena-ish engraved playmat: warm vignette, flowing lobes, a central medallion and a centre line.
pub fn procedural_mat(theme: &str) -> Image {
    let (w, h) = (1536usize, 990usize);
    let pal = palette(theme);
    let mut data = vec![255u8; w * h * 4];
    let lerp = |a: [f32; 3], b: [f32; 3], t: f32| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];
    for y in 0..h {
        for x in 0..w {
            // u,v in table-ish units: u∈[-1,1] across, v∈[-1,1] depth
            let u = x as f32 / w as f32 * 2.0 - 1.0;
            let v = y as f32 / h as f32 * 2.0 - 1.0;
            let (ax, ay) = (u * MAT_HALF_W, v * MAT_HALF_D);
            let r = (u * u * 0.8 + v * v * 1.1).sqrt();
            let grain = fbm(ax * 3.0, ay * 3.0);
            let fine = (hash(x as i32, y as i32) - 0.5) * 0.035;
            // base: lighter toward the middle, darker toward the rim
            let t = (1.0 - r * 0.85).clamp(0.0, 1.0);
            let mut col = lerp(pal.dark, pal.mid, (t * 1.2 + grain * 0.35).clamp(0.0, 1.0));
            col = lerp(col, pal.light, ((t - 0.35) * 1.4 * (0.6 + grain)).clamp(0.0, 0.8));
            // flowing lobes (the big swooping ribbons in the Arena mat)
            let lobe = ((ay * 1.1 + (ax * 0.55).sin() * 1.2).abs() - 1.55).abs();
            let lobe2 = ((ay * 1.1 - (ax * 0.55).sin() * 1.2).abs() - 1.55).abs();
            let ribbon = (1.0 - (lobe.min(lobe2) * 9.0)).clamp(0.0, 1.0);
            col = lerp(col, pal.line, ribbon * 0.26);
            let ribbon_soft = (1.0 - (lobe.min(lobe2) * 2.2)).clamp(0.0, 1.0);
            col = lerp(col, pal.light, ribbon_soft * 0.10);
            // central medallion rings
            let mr = ((ax * ax) / 1.0 + (ay * ay) / 1.0).sqrt();
            for rad in [1.15_f32, 1.45, 2.4] {
                let d = (mr - rad).abs();
                col = lerp(col, pal.line, (1.0 - d * 24.0).clamp(0.0, 1.0) * 0.30);
            }
            // centre line + outer inset line
            col = lerp(col, pal.line, (1.0 - ay.abs() * 22.0).clamp(0.0, 1.0) * 0.22);
            let inset = (MAT_HALF_W - 0.35 - ax.abs()).min(MAT_HALF_D - 0.35 - ay.abs());
            col = lerp(col, pal.line, (1.0 - inset.abs() * 20.0).clamp(0.0, 1.0) * 0.35);
            // vignette
            let vig = (1.0 - (r - 0.55).max(0.0) * 0.55).clamp(0.35, 1.0);
            let i = (y * w + x) * 4;
            for c in 0..3 {
                let lin = ((col[c] * vig) + fine).clamp(0.0, 1.0);
                data[i + c] = (lin.powf(1.0 / 2.2) * 255.0) as u8;
            }
        }
    }
    let mut img = Image::new(
        Extent3d { width: w as u32, height: h as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    crate::images::with_mips(&mut img);
    img.sampler = bevy::image::ImageSampler::Descriptor(bevy::image::ImageSamplerDescriptor {
        mag_filter: bevy::image::ImageFilterMode::Linear,
        min_filter: bevy::image::ImageFilterMode::Linear,
        mipmap_filter: bevy::image::ImageFilterMode::Linear,
        anisotropy_clamp: 8,
        ..default()
    });
    img.asset_usage = RenderAssetUsages::RENDER_WORLD;
    let _ = (FRAC_PI_2, TAU);
    img
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

