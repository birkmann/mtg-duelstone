//! Glowing bezier ribbon arrows (Arena's targeting / combat / stack arrows).
//!
//! Any system pushes `Arrow`s into `ArrowQueue` during `Update`; one renderer in `PostUpdate`
//! turns them into camera-facing triangle-strip meshes (additive, vertex-coloured, unlit), reusing
//! a pool of mesh entities. A 5-vertex cross-section (transparent edge → soft body → white-hot
//! core) gives a bloom-like glow without a custom shader; time-based modulation of the vertex
//! alpha makes energy "flow" from source to target.

use bevy::asset::RenderAssetUsages;
use bevy::camera::primitives::Aabb;
use bevy::camera::visibility::{NoFrustumCulling, RenderLayers};
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::picking::Pickable;
use bevy::prelude::*;

use crate::shared::*;

#[derive(Debug, Clone)]
pub struct Arrow {
    pub from: Vec3,
    pub to: Vec3,
    pub color: Color,
    /// World-space ribbon half-width.
    pub width: f32,
    /// Arc height as a fraction of the distance (0 = straight line).
    pub arc: f32,
    /// 0..1.5: overall brightness (pulse for valid targets etc).
    pub intensity: f32,
    /// Dotted instead of solid (planned blocks, stack → target).
    pub dashed: bool,
    pub head: bool,
}

impl Arrow {
    pub fn new(from: Vec3, to: Vec3, color: Color) -> Self {
        Self { from, to, color, width: 0.085, arc: 0.3, intensity: 1.0, dashed: false, head: true }
    }
}

#[derive(Resource, Default)]
pub struct ArrowQueue(pub Vec<Arrow>);

#[derive(Resource, Default)]
struct RibbonPool {
    ents: Vec<(Entity, Handle<Mesh>)>,
    mat: Handle<StandardMaterial>,
}

pub struct RibbonPlugin;

impl Plugin for RibbonPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ArrowQueue>()
            .init_resource::<RibbonPool>()
            .add_systems(Startup, setup)
            .add_systems(PostUpdate, render_arrows.before(TransformSystems::Propagate));
    }
}

fn setup(mut pool: ResMut<RibbonPool>, mut mats: ResMut<Assets<StandardMaterial>>) {
    pool.mat = mats.add(StandardMaterial {
        base_color: Color::WHITE,
        unlit: true,
        alpha_mode: AlphaMode::Add,
        cull_mode: None,
        double_sided: true,
        ..default()
    });
}

fn bezier(a: Vec3, c: Vec3, b: Vec3, t: f32) -> Vec3 {
    let u = 1.0 - t;
    a * (u * u) + c * (2.0 * u * t) + b * (t * t)
}

const SEGS: usize = 40;
const CROSS: usize = 5;

/// Build the ribbon mesh data for one arrow as seen from `eye`.
pub fn build_ribbon(a: &Arrow, eye: Vec3, time: f32) -> (Vec<[f32; 3]>, Vec<[f32; 4]>, Vec<u32>) {
    let dist = a.from.distance(a.to).max(0.01);
    let ctrl = (a.from + a.to) * 0.5 + Vec3::Y * (a.arc * dist + 0.12);
    let col = a.color.to_linear();
    let mut pos: Vec<[f32; 3]> = Vec::with_capacity(SEGS * CROSS + 8);
    let mut cols: Vec<[f32; 4]> = Vec::with_capacity(SEGS * CROSS + 8);
    let mut idx: Vec<u32> = Vec::new();
    let head_len = if a.head { (0.42_f32).min(dist * 0.35) } else { 0.0 };
    let body_end = 1.0 - head_len / dist;
    for i in 0..SEGS {
        let s = i as f32 / (SEGS - 1) as f32;
        let t = s * body_end;
        let p = bezier(a.from, ctrl, a.to, t);
        let p2 = bezier(a.from, ctrl, a.to, (t + 0.01).min(1.0));
        let p1 = bezier(a.from, ctrl, a.to, (t - 0.01).max(0.0));
        let tan = (p2 - p1).normalize_or_zero();
        let view = (eye - p).normalize_or_zero();
        let mut side = tan.cross(view).normalize_or_zero();
        if side == Vec3::ZERO {
            side = Vec3::X;
        }
        // taper: thin at the source, full in the middle, slightly wider before the head
        let taper = 0.35 + 0.65 * (s * 3.0).min(1.0);
        let w = a.width * taper;
        // flow: bright pulses travelling source → target; dashed = hard gating of the pulses
        let flow = ((s * 14.0 - time * 3.2).sin() * 0.5 + 0.5).powf(if a.dashed { 3.0 } else { 1.0 });
        let gate = if a.dashed { 0.2 + 0.8 * flow } else { 0.8 + 0.2 * flow };
        let fade_in = (s * 6.0).clamp(0.0, 1.0) * 0.7 + 0.3;
        let k = a.intensity * gate * fade_in;
        let prof: [(f32, f32, f32); CROSS] = [(-1.0, 0.0, 0.0), (-0.62, 0.85, 0.0), (0.0, 1.0, 0.8), (0.62, 0.85, 0.0), (1.0, 0.0, 0.0)];
        for (off, alpha, white) in prof {
            let v = p + side * (off * w);
            pos.push(v.to_array());
            let c = Vec3::new(col.red, col.green, col.blue).lerp(Vec3::ONE, white) * 1.15;
            cols.push([c.x, c.y, c.z, (alpha * k).clamp(0.0, 1.4)]);
        }
        if i > 0 {
            let base = (i * CROSS) as u32;
            let prev = base - CROSS as u32;
            for j in 0..(CROSS as u32 - 1) {
                idx.extend([prev + j, base + j, base + j + 1, prev + j, base + j + 1, prev + j + 1]);
            }
        }
    }
    if a.head {
        let end = a.to;
        let tan = (end - bezier(a.from, ctrl, a.to, 1.0 - head_len / dist * 0.9)).normalize_or_zero();
        let view = (eye - end).normalize_or_zero();
        let side = tan.cross(view).normalize_or_zero();
        let base_c = end - tan * head_len;
        let hw = a.width * 2.9;
        let base_idx = pos.len() as u32;
        let c = Vec3::new(col.red, col.green, col.blue).lerp(Vec3::ONE, 0.35) * 1.2;
        let al = (a.intensity * 0.95).clamp(0.0, 1.4);
        pos.push(end.to_array());
        cols.push([1.0, 1.0, 1.0, al]);
        pos.push((base_c + side * hw).to_array());
        cols.push([c.x, c.y, c.z, al * 0.9]);
        pos.push((base_c - side * hw).to_array());
        cols.push([c.x, c.y, c.z, al * 0.9]);
        pos.push(base_c.to_array());
        cols.push([c.x, c.y, c.z, al * 0.95]);
        idx.extend([base_idx, base_idx + 1, base_idx + 3, base_idx, base_idx + 3, base_idx + 2]);
    }
    (pos, cols, idx)
}

fn empty_mesh() -> Mesh {
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, Vec::<[f32; 3]>::new())
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, Vec::<[f32; 4]>::new())
        .with_inserted_indices(Indices::U32(vec![]))
}

fn render_arrows(
    mut commands: Commands,
    mut queue: ResMut<ArrowQueue>,
    mut pool: ResMut<RibbonPool>,
    mut meshes: ResMut<Assets<Mesh>>,
    time: Res<Time>,
    cam: Query<&GlobalTransform, With<TableCamera>>,
    mut vis: Query<&mut Visibility>,
) {
    let Ok(cam) = cam.single() else {
        queue.0.clear();
        return;
    };
    let eye = cam.translation();
    let t = time.elapsed_secs();
    let arrows = std::mem::take(&mut queue.0);
    // grow the pool
    while pool.ents.len() < arrows.len() {
        let h = meshes.add(empty_mesh());
        let e = commands
            .spawn((
                Mesh3d(h.clone()),
                MeshMaterial3d(pool.mat.clone()),
                Transform::IDENTITY,
                Visibility::Hidden,
                RenderLayers::layer(TABLE_LAYER),
                NoFrustumCulling,
                NotShadowCaster,
                Pickable::IGNORE,
                Name::new("arrow ribbon"),
            ))
            .id();
        pool.ents.push((e, h));
    }
    for (i, (e, h)) in pool.ents.iter().enumerate() {
        match arrows.get(i) {
            Some(a) => {
                let (pos, cols, idx) = build_ribbon(a, eye, t);
                if let Some(mut m) = meshes.get_mut(h) {
                    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
                    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, cols);
                    m.insert_indices(Indices::U32(idx));
                }
                if let Ok(mut v) = vis.get_mut(*e)
                    && *v != Visibility::Inherited
                {
                    *v = Visibility::Inherited;
                }
                let _ = Aabb::default();
            }
            None => {
                if let Ok(mut v) = vis.get_mut(*e)
                    && *v != Visibility::Hidden
                {
                    *v = Visibility::Hidden;
                }
            }
        }
    }
}

/// Cursor ray ∩ horizontal plane at height `y` on the table camera.
pub fn cursor_on_table(
    windows: &Query<&Window, With<bevy::window::PrimaryWindow>>,
    cam: &Query<(&Camera, &GlobalTransform), With<TableCamera>>,
    y: f32,
) -> Option<Vec3> {
    let win = windows.single().ok()?;
    // dev: ARENA_FAKE_CURSOR="x,y" pretends the pointer rests there (headless screenshots)
    let fake = std::env::var("ARENA_FAKE_CURSOR").ok().and_then(|s| {
        let (x, y) = s.split_once(',')?;
        Some(Vec2::new(x.trim().parse().ok()?, y.trim().parse().ok()?))
    });
    let cursor = fake.or_else(|| win.cursor_position())?;
    let (c, g) = cam.single().ok()?;
    let ray = c.viewport_to_world(g, cursor).ok()?;
    let d = *ray.direction;
    if d.y.abs() < 1e-4 {
        return None;
    }
    let t = (y - ray.origin.y) / d.y;
    if t < 0.0 {
        return None;
    }
    Some(ray.origin + d * t)
}

/// World position of a card entity for arrow endpoints (None if not on the table world).
pub fn card_anchor(
    id: mtg_protocol::CardId,
    registry: &CardRegistry,
    q: &Query<(&GlobalTransform, Option<&TargetPlacement>)>,
) -> Option<Vec3> {
    let e = registry.0.get(&id)?;
    let (gt, tp) = q.get(*e).ok()?;
    if tp.is_some_and(|t| t.layer != TABLE_LAYER) {
        return None;
    }
    Some(gt.translation() + Vec3::Y * 0.06)
}
