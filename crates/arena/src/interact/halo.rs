//! Additive glow halo around cards the engine wants clicked: blue = selectable/playable,
//! gold = already picked, soft teal = weak (e.g. lands auto-pay would tap). One shared material per
//! kind whose alpha is pulsed globally, so 60 glowing cards cost three material writes per frame.

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::light::NotShadowCaster;
use bevy::picking::Pickable;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::shared::*;

const MARGIN: f32 = 0.13;
const TW: u32 = 136;
const TH: u32 = 176;

#[derive(Resource, Clone)]
pub struct HaloAssets {
    pub mesh: Handle<Mesh>,
    pub selectable: Handle<StandardMaterial>,
    pub highlighted: Handle<StandardMaterial>,
    pub weak: Handle<StandardMaterial>,
}

#[derive(Component)]
pub struct Halo(pub Entity);

fn sd_round_box(p: Vec2, half: Vec2, r: f32) -> f32 {
    let q = p.abs() - half + Vec2::splat(r);
    q.max(Vec2::ZERO).length() + q.x.max(q.y).min(0.0) - r
}

/// Glow texture: transparent inside the card outline, a crisp thin rim, then a soft falloff.
pub fn glow_image() -> Image {
    let (gw, gh) = (CARD_W + 2.0 * MARGIN, CARD_H + 2.0 * MARGIN);
    let mut data = vec![0u8; (TW * TH * 4) as usize];
    for y in 0..TH {
        for x in 0..TW {
            let u = (x as f32 + 0.5) / TW as f32 - 0.5;
            let v = 0.5 - (y as f32 + 0.5) / TH as f32;
            let d = sd_round_box(Vec2::new(u * gw, v * gh), Vec2::new(CARD_W / 2.0, CARD_H / 2.0), 0.034);
            let a = if d <= 0.0 {
                0.0
            } else {
                let rim = (1.0 - (d / 0.018)).clamp(0.0, 1.0);
                let glow = (1.0 - d / MARGIN).clamp(0.0, 1.0).powf(2.0);
                (rim * 0.95).max(glow * 0.75)
            };
            let i = ((y * TW + x) * 4) as usize;
            data[i] = 255;
            data[i + 1] = 255;
            data[i + 2] = 255;
            data[i + 3] = (a * 255.0) as u8;
        }
    }
    Image::new(
        Extent3d { width: TW, height: TH, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

pub fn setup_halo_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let tex = images.add(glow_image());
    let mk = |c: Color| StandardMaterial {
        base_color: c,
        base_color_texture: Some(tex.clone()),
        unlit: true,
        alpha_mode: AlphaMode::Add,
        cull_mode: None,
        ..default()
    };
    commands.insert_resource(HaloAssets {
        mesh: meshes.add(Rectangle::new(CARD_W + 2.0 * MARGIN, CARD_H + 2.0 * MARGIN)),
        selectable: mats.add(mk(Color::srgb(0.2, 0.6, 1.0))),
        highlighted: mats.add(mk(Color::srgb(1.0, 0.78, 0.25))),
        weak: mats.add(mk(Color::srgb(0.25, 0.75, 0.7))),
    });
}

pub fn spawn_halo(commands: &mut Commands, root: Entity, a: &HaloAssets) {
    commands.entity(root).with_children(|p| {
        p.spawn((
            Halo(root),
            Mesh3d(a.mesh.clone()),
            MeshMaterial3d(a.selectable.clone()),
            Transform::from_xyz(0.0, 0.0, 0.0003),
            Visibility::Hidden,
            NotShadowCaster,
            Pickable::IGNORE,
        ));
    });
}

pub fn update_halos(
    time: Res<Time>,
    assets: Option<Res<HaloAssets>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
    roots: Query<(Has<Selectable>, Has<Highlighted>, Has<WeakSelectable>, Has<super::PlayableHint>, &RenderLayers)>,
    mut halos: Query<(Entity, &Halo, &mut Visibility, &mut MeshMaterial3d<StandardMaterial>, Option<&RenderLayers>)>,
) {
    let Some(a) = assets else { return };
    let t = time.elapsed_secs();
    let pulse = 0.5 + 0.5 * (t * 3.6).sin();
    if let Some(mut m) = mats.get_mut(&a.selectable) {
        m.base_color = Color::srgb(0.2, 0.6, 1.0).with_alpha(0.62 + 0.38 * pulse);
    }
    if let Some(mut m) = mats.get_mut(&a.highlighted) {
        m.base_color = Color::srgb(1.0, 0.78, 0.25).with_alpha(0.9);
    }
    if let Some(mut m) = mats.get_mut(&a.weak) {
        m.base_color = Color::srgb(0.25, 0.75, 0.7).with_alpha(0.35 + 0.1 * pulse);
    }
    for (e, halo, mut vis, mut mat, layers) in &mut halos {
        let Ok((sel, hi, weak, hint, root_layers)) = roots.get(halo.0) else { continue };
        let (want_vis, want_mat) = if hi {
            (Visibility::Inherited, &a.highlighted)
        } else if sel || hint {
            (Visibility::Inherited, &a.selectable)
        } else if weak {
            (Visibility::Inherited, &a.weak)
        } else {
            (Visibility::Hidden, &a.selectable)
        };
        if *vis != want_vis {
            *vis = want_vis;
        }
        if mat.0 != *want_mat {
            mat.0 = want_mat.clone();
        }
        if layers != Some(root_layers) {
            commands.entity(e).insert(root_layers.clone());
        }
    }
}
