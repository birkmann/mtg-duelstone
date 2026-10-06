//! Card entities: one root per card id (hand, battlefield, piles AND spells on the stack, so a
//! spell flies hand → stack → graveyard as a single entity), with a front and a back quad.
//!
//! Meshes are shared (one rounded-rect mesh for every card); materials are cached per image handle.
//! Until a card's real image has arrived the front shows a neutral blank face, because Bevy
//! doesn't draw a mesh whose texture isn't ready.
//!
//! Backs wear their owner's sleeve (`Cosmetics`): one back material per side, retextured in place
//! when the equipped sleeve changes, so every card follows without touching the entities.

use std::collections::{HashMap, HashSet};
use std::f32::consts::PI;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use mtg_client::ImageVersion;
use mtg_protocol::{CardId, ZoneKind};

use crate::images::CardImages;
use crate::layout::{self, collect_cards};
use crate::shared::*;

pub struct CardsPlugin;

impl Plugin for CardsPlugin {
    fn build(&self, app: &mut App) {
        let in_match = in_state(AppState::InMatch).or_else(in_state(AppState::GameOver));
        app.init_resource::<CardRegistry>()
            .init_resource::<CardAssets>()
            .add_systems(Startup, setup_card_assets)
            .add_systems(
                Update,
                (sync_cards.run_if(in_match.clone()).after(crate::view::ViewSet), update_faces, fade_out, apply_sleeves).chain(),
            )
            .add_systems(PostUpdate, orient_shadows);
    }
}

/// Shared mesh + material cache.
#[derive(Resource, Default)]
pub struct CardAssets {
    pub quad: Handle<Mesh>,
    pub shadow_quad: Handle<Mesh>,
    pub shadow_mat: Handle<StandardMaterial>,
    /// Your sleeve.
    pub back_mat: Handle<StandardMaterial>,
    /// The opponent's sleeve.
    pub opp_back_mat: Handle<StandardMaterial>,
    pub blank_mat: Handle<StandardMaterial>,
    by_image: HashMap<AssetId<Image>, Handle<StandardMaterial>>,
}

/// What image the front currently wants (and whether it has arrived).
#[derive(Component, Debug)]
pub struct CardFace {
    pub image: Handle<Image>,
    pub loaded: bool,
    pub hidden: bool,
    pub name: String,
}

/// Soft contact-shadow blob under a card.
#[derive(Component)]
pub struct CardShadow;

/// Card whose id vanished: shrinks away, then despawns.
#[derive(Component)]
pub struct Despawning(pub f32);

/// Rounded-rectangle mesh in the XY plane facing +Z, UV (0,0) at the top-left of the image.
pub fn card_mesh(w: f32, h: f32, radius: f32) -> Mesh {
    const SEG: usize = 6;
    let mut pos: Vec<[f32; 3]> = vec![[0.0, 0.0, 0.0]];
    let corners = [
        (w / 2.0 - radius, h / 2.0 - radius, 0.0_f32),
        (-w / 2.0 + radius, h / 2.0 - radius, PI / 2.0),
        (-w / 2.0 + radius, -h / 2.0 + radius, PI),
        (w / 2.0 - radius, -h / 2.0 + radius, 3.0 * PI / 2.0),
    ];
    for (cx, cy, a0) in corners {
        for i in 0..=SEG {
            let a = a0 + (i as f32 / SEG as f32) * PI / 2.0;
            pos.push([cx + radius * a.cos(), cy + radius * a.sin(), 0.0]);
        }
    }
    let n = pos.len();
    let normals = vec![[0.0, 0.0, 1.0]; n];
    let uvs: Vec<[f32; 2]> = pos.iter().map(|p| [p[0] / w + 0.5, 0.5 - p[1] / h]).collect();
    let mut idx: Vec<u32> = Vec::new();
    for i in 1..n as u32 {
        let next = if i + 1 < n as u32 { i + 1 } else { 1 };
        idx.extend([0, i, next]);
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32(idx))
}

fn card_material(tex: Handle<Image>) -> StandardMaterial {
    StandardMaterial {
        base_color_texture: Some(tex),
        base_color: Color::WHITE,
        perceptual_roughness: 0.85,
        reflectance: 0.08,
        ..default()
    }
}

fn setup_card_assets(
    mut assets: ResMut<CardAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    imgs: Res<CardImages>,
    mut images: ResMut<Assets<Image>>,
) {
    assets.quad = meshes.add(card_mesh(CARD_W, CARD_H, 0.034));
    assets.shadow_quad = meshes.add(Rectangle::new(CARD_W * 1.32, CARD_H * 1.22));
    assets.shadow_mat = mats.add(StandardMaterial {
        base_color_texture: Some(images.add(shadow_image())),
        base_color: Color::srgba(0.0, 0.0, 0.0, 0.62),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        ..default()
    });
    assets.back_mat = mats.add(card_material(imgs.card_back.clone()));
    assets.opp_back_mat = mats.add(card_material(imgs.card_back.clone()));
    assets.blank_mat = mats.add(card_material(imgs.card_blank.clone()));
}

impl CardAssets {
    fn back_for(&self, owner: Option<mtg_protocol::PlayerId>, you: Option<mtg_protocol::PlayerId>) -> Handle<StandardMaterial> {
        if owner.is_some() && owner != you { self.opp_back_mat.clone() } else { self.back_mat.clone() }
    }

    fn material_for(&mut self, mats: &mut Assets<StandardMaterial>, image: &Handle<Image>) -> Handle<StandardMaterial> {
        self.by_image.entry(image.id()).or_insert_with(|| mats.add(card_material(image.clone()))).clone()
    }
}

fn spawn_card(
    commands: &mut Commands,
    assets: &CardAssets,
    id: CardId,
    loc: CardLocation,
    face: CardFace,
    from: Vec3,
) -> Entity {
    let layers = RenderLayers::layer(TABLE_LAYER);
    let root = commands
        .spawn((
            CardEntity(id),
            loc,
            face,
            Transform::from_translation(from).with_rotation(layout::flat_rot(false)).with_scale(Vec3::splat(0.8)),
            Visibility::Inherited,
            layers.clone(),
            PoseOffset::default(),
            crate::motion::MotionState::default(),
            Pickable::default(),
            Name::new(format!("card {id}")),
        ))
        .id();
    commands.entity(root).with_children(|p| {
        p.spawn((
            CardShadow,
            Mesh3d(assets.shadow_quad.clone()),
            MeshMaterial3d(assets.shadow_mat.clone()),
            Transform::from_xyz(0.05, -0.07, -0.0012),
            layers.clone(),
            NotShadowCaster,
            NotShadowReceiver,
        ));
        p.spawn((
            CardFront,
            Mesh3d(assets.quad.clone()),
            MeshMaterial3d(assets.blank_mat.clone()),
            Transform::from_xyz(0.0, 0.0, 0.0006),
            layers.clone(),
            Pickable::default(),
            NotShadowCaster,
        ));
        p.spawn((
            CardBack,
            Mesh3d(assets.quad.clone()),
            MeshMaterial3d(assets.back_mat.clone()),
            Transform::from_xyz(0.0, 0.0, -0.0006).with_rotation(Quat::from_rotation_y(PI)),
            layers,
            Pickable::default(),
            NotShadowCaster,
        ));
    });
    root
}

/// Soft rounded-rect alpha falloff for the contact shadow (RGB black, alpha gradient).
fn shadow_image() -> Image {
    const N: usize = 96;
    let mut data = vec![0u8; N * N * 4];
    for y in 0..N {
        for x in 0..N {
            let (u, v) = ((x as f32 + 0.5) / N as f32 * 2.0 - 1.0, (y as f32 + 0.5) / N as f32 * 2.0 - 1.0);
            // signed distance to a rect covering ~70% of the quad, softened
            let (dx, dy) = (u.abs() - 0.62, v.abs() - 0.60);
            let dist = (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt() + dx.max(dy).min(0.0);
            let a = (1.0 - (dist / 0.38).clamp(0.0, 1.0)).powf(1.6);
            data[(y * N + x) * 4 + 3] = (a * 255.0) as u8;
        }
    }
    let mut img = Image::new(
        Extent3d { width: N as u32, height: N as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    img.sampler = bevy::image::ImageSampler::linear();
    img
}

/// The root flips (180° about its vertical axis) for face-down cards, so the blob has to sit on
/// the table side of the card either way: mirror its local offset with the flip.
fn orient_shadows(
    mut shadows: Query<(&mut Transform, &ChildOf), With<CardShadow>>,
    targets: Query<&TargetPlacement>,
) {
    for (mut tf, parent) in &mut shadows {
        let Ok(tp) = targets.get(parent.parent()) else { continue };
        let s = if tp.face_up { 1.0 } else { -1.0 };
        let want = Vec3::new(0.05 * s, -0.07, -0.0012 * s);
        if tf.translation != want {
            tf.translation = want;
        }
    }
}

fn sync_cards(
    mut commands: Commands,
    view: Res<GameView>,
    mut registry: ResMut<CardRegistry>,
    assets: Res<CardAssets>,
    mut images: ResMut<CardImages>,
    mut q: Query<(&mut CardLocation, &mut CardFace)>,
    anchors: Res<BoardAnchors>,
) {
    if !view.is_changed() || !view.has_state() {
        return;
    }
    let cards = collect_cards(&view);
    let mut alive: HashSet<CardId> = HashSet::with_capacity(cards.len());
    for s in &cards {
        let c = s.card;
        alive.insert(c.id);
        let controller = c.controller.or(s.player);
        let owner = c.owner.or(s.player);
        let loc = CardLocation {
            zone: s.zone,
            owner,
            controller,
            mine: match s.zone {
                ZoneKind::Battlefield | ZoneKind::Stack => controller == view.you,
                _ => owner == view.you,
            },
            index: s.index,
            hidden: !c.is_visible(),
        };
        let hidden = !c.is_visible();
        match registry.0.get(&c.id).copied() {
            Some(e) => {
                if let Ok((mut cur, mut face)) = q.get_mut(e) {
                    if *cur != loc {
                        *cur = loc;
                    }
                    let name = c.display_name().to_string();
                    if hidden != face.hidden || (!hidden && name != face.name) {
                        let h = images.request(c, ImageVersion::Normal);
                        face.image = h;
                        face.loaded = false;
                        face.hidden = hidden;
                        face.name = name;
                    }
                }
            }
            None => {
                let image = images.request(c, ImageVersion::Normal);
                let face = CardFace { image, loaded: false, hidden, name: c.display_name().to_string() };
                // Cards appear from their owner's library (a draw reads as a draw).
                let from = loc
                    .owner
                    .and_then(|o| anchors.seat_of(o))
                    .map(|s| s.library)
                    .unwrap_or(if loc.mine { anchors.my_library } else { anchors.opp_library });
                let from = if s.zone == ZoneKind::Library { from } else { from + Vec3::Y * 0.3 };
                let e = spawn_card(&mut commands, &assets, c.id, loc, face, from);
                registry.0.insert(c.id, e);
            }
        }
    }
    let gone: Vec<CardId> = registry.0.keys().filter(|id| !alive.contains(id)).copied().collect();
    for id in gone {
        if let Some(e) = registry.0.remove(&id) {
            commands.entity(e).insert(Despawning(0.0)).remove::<TargetPlacement>();
        }
    }
}

/// Keep each front face on the right material: back for hidden cards, blank until the real
/// image has downloaded, then the cached per-image material.
fn update_faces(
    mut assets: ResMut<CardAssets>,
    images: Res<Assets<Image>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    view: Res<GameView>,
    mut cards: Query<(&mut CardFace, &CardLocation, &Children), Without<Despawning>>,
    mut fronts: Query<&mut MeshMaterial3d<StandardMaterial>, (With<CardFront>, Without<CardBack>)>,
    mut backs: Query<&mut MeshMaterial3d<StandardMaterial>, (With<CardBack>, Without<CardFront>)>,
) {
    for (mut face, loc, children) in &mut cards {
        let ready = images.contains(face.image.id());
        let back = assets.back_for(loc.owner, view.you);
        let m = if face.hidden {
            back.clone()
        } else if ready {
            assets.material_for(&mut mats, &face.image)
        } else {
            assets.blank_mat.clone()
        };
        if face.loaded != ready {
            face.loaded = ready;
        }
        for ch in children.iter() {
            if let Ok(mut fm) = fronts.get_mut(ch) {
                if fm.0 != m {
                    fm.0 = m.clone();
                }
            } else if let Ok(mut bm) = backs.get_mut(ch)
                && bm.0 != back
            {
                bm.0 = back.clone();
            }
        }
    }
}

fn fade_out(mut commands: Commands, time: Res<Time>, mut q: Query<(Entity, &mut Despawning, &mut Transform)>) {
    for (e, mut d, mut tf) in &mut q {
        d.0 += time.delta_secs();
        let k = (1.0 - d.0 / 0.35).clamp(0.0, 1.0);
        tf.scale = Vec3::splat(tf.scale.x.min(k.max(0.001) * 1.0));
        if d.0 >= 0.35 {
            commands.entity(e).despawn();
        }
    }
}

/// Retexture the two back materials when the equipped sleeves change (or finish painting).
fn apply_sleeves(cosmetics: Res<Cosmetics>, assets: Res<CardAssets>, mut mats: ResMut<Assets<StandardMaterial>>) {
    if !cosmetics.is_changed() {
        return;
    }
    for (mat, tex, finish) in [(&assets.back_mat, &cosmetics.my_sleeve, cosmetics.my_finish), (&assets.opp_back_mat, &cosmetics.opp_sleeve, cosmetics.opp_finish)] {
        let (Some(tex), Some(mut m)) = (tex, mats.get_mut(mat)) else { continue };
        m.base_color_texture = Some(tex.clone());
        crate::cosmetics::apply_finish(&mut m, finish);
    }
}
