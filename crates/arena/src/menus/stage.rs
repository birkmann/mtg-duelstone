//! The main menu's 3D stage: an arcane portal (custom shader) with a carousel of real Magic
//! cards circling it, five mana orbs that light the cards in their colours, drifting embers and
//! nebula haze, all under HDR + bloom. The first time the menu opens it plays an intro: the
//! camera dollies in, the portal ignites with a flash, cards burst out of it into their orbit, and
//! the orbs pop in. `main_menu` times its UI against the same [`IntroClock`].
//!
//! Why its own camera and render layer: the table camera always renders the match world, and the
//! UI backdrop used to just cover it. The stage gets a camera on `MENU_LAYER` that's active only
//! in the main menu (the table camera is paused meanwhile, see `toggle_stage`).
//! The stage is spawned once and persists (hidden outside the menu), so coming back from deck
//! select doesn't replay the card fly-in or re-request images.

use std::f32::consts::TAU;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{ClearColorConfig, Hdr};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::pbr::{Material, MaterialPlugin};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, Extent3d, ShaderType, TextureDimension, TextureFormat};
use bevy::shader::ShaderRef;
use bevy::window::PrimaryWindow;
use mtg_client::scryfall::CURATED;
use mtg_client::{ImageRequest, ImageVersion};

use crate::cards::card_mesh;
use crate::shared::*;

/// When the full intro has settled; skipping jumps here.
pub(super) const INTRO_END: f32 = 4.0;

const CAM_FOV_DEG: f32 = 32.0;
const CAM_REST: Vec3 = Vec3::new(0.0, 1.0, 10.5);
const CAM_LOOK: Vec3 = Vec3::new(0.0, 0.15, 0.0);
const RING_RADIUS: f32 = 2.25;
const RING_TILT: f32 = 0.2;
const CARD_SCALE: f32 = 1.45;
const N_CARDS: usize = 12;
const PORTAL_SIZE: f32 = 3.5;
const MANA: [(f32, f32, f32); 5] = [(1.0, 0.92, 0.7), (0.25, 0.55, 1.0), (0.62, 0.3, 0.9), (1.0, 0.28, 0.1), (0.25, 0.95, 0.4)];

pub struct StagePlugin;

impl Plugin for StagePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<PortalMaterial>::default())
            .init_resource::<IntroClock>()
            .add_systems(Startup, spawn_stage)
            .add_systems(OnEnter(AppState::MainMenu), start_intro)
            .add_systems(Update, toggle_stage)
            .add_systems(
                Update,
                (tick_intro, (animate_camera, animate_portal, animate_cards, animate_orbs, animate_embers, animate_haze, swap_in_card_faces, sleeve_carousel))
                    .chain()
                    .run_if(in_state(AppState::MainMenu)),
            );
    }
}

/// Seconds since the main menu opened, and whether this visit plays the full intro (first visit
/// only). Owner: stage; read by `main_menu`.
#[derive(Resource, Default)]
pub(super) struct IntroClock {
    pub t: f32,
    pub full: bool,
    seen: bool,
    /// one bit per intro sound already played
    sfx_fired: u8,
}

impl IntroClock {
    /// Time on the stage timeline: return visits start with everything already settled.
    fn stage_t(&self) -> f32 {
        if self.full { self.t } else { INTRO_END + self.t }
    }
}

// ---- portal material ------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, ShaderType)]
struct PortalParams {
    /// rgb rim colour, a intensity
    color: Vec4,
    /// x ignition, y swirl speed
    params: Vec4,
}

#[derive(Asset, AsBindGroup, TypePath, Clone, Debug)]
struct PortalMaterial {
    #[uniform(0)]
    u: PortalParams,
}

impl Material for PortalMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/menu_portal.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Premultiplied
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
}

// ---- components -----------------------------------------------------------------------------

#[derive(Component)]
struct MenuCamera;
#[derive(Component)]
struct StageRoot;
#[derive(Component)]
struct PortalQuad;
#[derive(Component)]
struct PortalLight;

#[derive(Component)]
struct MenuCard {
    slot: usize,
    image: Handle<Image>,
    material: Handle<StandardMaterial>,
    loaded: bool,
    hovered: bool,
    hover: f32,
    /// seconds left of a click-triggered spin
    spin: f32,
}

#[derive(Component)]
struct ManaOrb(usize);

#[derive(Component)]
struct Ember {
    base: Vec3,
    speed: f32,
    phase: f32,
    size: f32,
}

#[derive(Component)]
struct Haze(f32);

/// Tiny deterministic LCG; the scenery layout doesn't need real randomness.
struct Lcg(u32);
impl Lcg {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        (self.0 >> 8) as f32 / (1u32 << 24) as f32
    }
    fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.next()
    }
}

// ---- spawn ----------------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn spawn_stage(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut portal_mats: ResMut<Assets<PortalMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut card_images: ResMut<CardImages>,
) {
    let layer = RenderLayers::layer(MENU_LAYER);
    commands.spawn((
        MenuCamera,
        Camera3d::default(),
        Camera { order: 1, is_active: false, clear_color: ClearColorConfig::Custom(Color::srgb(0.014, 0.009, 0.008)), ..default() },
        Projection::Perspective(PerspectiveProjection { fov: CAM_FOV_DEG.to_radians(), ..default() }),
        Transform::from_translation(CAM_REST).looking_at(CAM_LOOK, Vec3::Y),
        Hdr,
        Tonemapping::TonyMcMapface,
        Bloom { intensity: 0.1, ..Bloom::NATURAL },
        Msaa::Sample4,
        layer.clone(),
        Name::new("menu camera"),
    ));
    commands.spawn((
        DirectionalLight { color: Color::srgb(1.0, 0.9, 0.78), illuminance: 2200.0, shadow_maps_enabled: false, ..default() },
        Transform::from_xyz(-3.0, 4.0, 8.0).looking_at(Vec3::ZERO, Vec3::Y),
        layer.clone(),
        Name::new("menu key light"),
    ));

    let glow_tex = images.add(radial_glow_image(128));
    let haze_tex: Vec<Handle<Image>> = (0..3).map(|i| images.add(cloud_image(256, 11 + i * 7))).collect();
    let quad = meshes.add(Rectangle::new(1.0, 1.0));

    let root = commands.spawn((StageRoot, Transform::default(), Visibility::Hidden, layer.clone(), Name::new("menu stage"))).id();
    commands.entity(root).with_children(|s| {
        // ---- far haze: nebula clouds + a warm glow behind the portal ----
        let haze = [((0.30, 0.10, 0.03), 19.0, Vec3::new(-0.5, 0.5, -9.0), 0.018), ((0.20, 0.05, 0.28), 16.0, Vec3::new(-4.5, -1.5, -8.0), -0.024), ((0.04, 0.11, 0.17), 14.0, Vec3::new(3.5, 2.5, -10.0), 0.012)];
        for (i, ((r, g, b), size, pos, spin)) in haze.into_iter().enumerate() {
            s.spawn((
                Haze(spin),
                Mesh3d(quad.clone()),
                MeshMaterial3d(mats.add(StandardMaterial {
                    base_color: Color::srgb(r, g, b),
                    base_color_texture: Some(haze_tex[i].clone()),
                    unlit: true,
                    alpha_mode: AlphaMode::Add,
                    ..default()
                })),
                Transform::from_translation(pos).with_scale(Vec3::splat(size)),
                layer.clone(),
                NotShadowCaster,
                NotShadowReceiver,
            ));
        }
        s.spawn((
            Mesh3d(quad.clone()),
            MeshMaterial3d(mats.add(StandardMaterial {
                base_color: Color::srgb(0.55, 0.2, 0.06),
                base_color_texture: Some(glow_tex.clone()),
                unlit: true,
                alpha_mode: AlphaMode::Add,
                ..default()
            })),
            Transform::from_xyz(0.0, 0.0, -2.5).with_scale(Vec3::splat(9.0)),
            layer.clone(),
        ));
        // distant motes
        let mut rng = Lcg(0xA11CE);
        let star_mesh = meshes.add(Sphere::new(0.02).mesh().ico(1).unwrap());
        let star_mat = mats.add(StandardMaterial { base_color: Color::BLACK, emissive: LinearRgba::rgb(2.2, 1.6, 1.1), ..default() });
        for _ in 0..110 {
            s.spawn((
                Mesh3d(star_mesh.clone()),
                MeshMaterial3d(star_mat.clone()),
                Transform::from_xyz(rng.range(-16.0, 12.0), rng.range(-6.0, 7.0), rng.range(-14.0, -7.0)).with_scale(Vec3::splat(rng.range(0.4, 1.4))),
                layer.clone(),
                NotShadowCaster,
            ));
        }

        // ---- portal ----
        s.spawn((
            PortalQuad,
            Mesh3d(quad.clone()),
            MeshMaterial3d(portal_mats.add(PortalMaterial { u: PortalParams { color: Vec4::new(1.0, 0.62, 0.22, 1.0), params: Vec4::new(0.0, 0.55, 0.0, 0.0) } })),
            Transform::from_scale(Vec3::splat(PORTAL_SIZE)),
            layer.clone(),
            NotShadowCaster,
            NotShadowReceiver,
            Pickable::IGNORE,
        ));
        s.spawn((
            PortalLight,
            PointLight { color: Color::srgb(1.0, 0.55, 0.2), intensity: 0.0, range: 9.0, radius: 0.6, shadow_maps_enabled: false, ..default() },
            Transform::from_xyz(0.0, 0.0, 0.4),
            layer.clone(),
        ));

        // ---- mana orbs ----
        let orb_mesh = meshes.add(Sphere::new(0.075).mesh().uv(24, 16));
        for (i, (r, g, b)) in MANA.into_iter().enumerate() {
            s.spawn((
                ManaOrb(i),
                Mesh3d(orb_mesh.clone()),
                MeshMaterial3d(mats.add(StandardMaterial { base_color: Color::srgb(r, g, b), emissive: LinearRgba::rgb(r * 4.0, g * 4.0, b * 4.0), ..default() })),
                Transform::from_scale(Vec3::ZERO),
                layer.clone(),
                NotShadowCaster,
            ))
            .with_child((
                PointLight { color: Color::srgb(r, g, b), intensity: 22_000.0, range: 5.0, radius: 0.07, shadow_maps_enabled: false, ..default() },
                layer.clone(),
            ));
        }

        // ---- embers ----
        let ember_mesh = meshes.add(Sphere::new(0.014).mesh().ico(1).unwrap());
        let ember_mat = mats.add(StandardMaterial { base_color: Color::BLACK, emissive: LinearRgba::rgb(9.0, 3.2, 0.7), ..default() });
        for _ in 0..90 {
            s.spawn((
                Ember { base: Vec3::new(rng.range(-6.5, 5.0), 0.0, rng.range(-5.0, 3.5)), speed: rng.range(0.18, 0.55), phase: rng.next(), size: rng.range(0.5, 1.6) },
                Mesh3d(ember_mesh.clone()),
                MeshMaterial3d(ember_mat.clone()),
                Transform::default(),
                layer.clone(),
                NotShadowCaster,
            ));
        }

        // ---- card carousel: iconic printings, fetched through the engine's image proxy ----
        let front_mesh = meshes.add(card_mesh(CARD_W, CARD_H, 0.034));
        let back_mat = mats.add(StandardMaterial { base_color_texture: Some(card_images.card_back.clone()), perceptual_roughness: 0.6, reflectance: 0.2, ..default() });
        s.commands().insert_resource(CarouselBack(back_mat.clone()));
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(1);
        let mut pick = Lcg(seed);
        let mut pool: Vec<_> = CURATED.iter().filter(|c| c.name != "Island").collect();
        for i in (1..pool.len()).rev() {
            pool.swap(i, (pick.next() * (i + 1) as f32) as usize % (i + 1));
        }
        for (slot, c) in pool.into_iter().take(N_CARDS).enumerate() {
            let image = card_images.request_raw(ImageRequest { name: c.name.to_string(), set: Some(c.set.to_string()), back: false, version: ImageVersion::Normal });
            let material = mats.add(StandardMaterial {
                base_color_texture: Some(card_images.card_blank.clone()),
                perceptual_roughness: 0.55,
                reflectance: 0.25,
                ..default()
            });
            s.spawn((
                MenuCard { slot, image, material: material.clone(), loaded: false, hovered: false, hover: 0.0, spin: 0.0 },
                Transform::from_scale(Vec3::ZERO),
                Visibility::Inherited,
                layer.clone(),
                Name::new(format!("menu card {}", c.name)),
            ))
            .with_children(|card| {
                card.spawn((Mesh3d(front_mesh.clone()), MeshMaterial3d(material), Transform::from_xyz(0.0, 0.0, 0.002), layer.clone()));
                card.spawn((Mesh3d(front_mesh.clone()), MeshMaterial3d(back_mat.clone()), Transform::from_rotation(Quat::from_rotation_y(TAU / 2.0)), layer.clone()));
            })
            .observe(on_card_enter)
            .observe(on_card_leave)
            .observe(on_card_click);
        }
    });
}

fn on_card_enter(on: On<Pointer<Enter>>, mut cards: Query<&mut MenuCard>, mut sfx: MessageWriter<PlaySfx>) {
    if let Ok(mut c) = cards.get_mut(on.event_target()) {
        c.hovered = true;
        sfx.write(PlaySfx(Sfx::HoverTick));
    }
}

fn on_card_leave(on: On<Pointer<Leave>>, mut cards: Query<&mut MenuCard>) {
    if let Ok(mut c) = cards.get_mut(on.event_target()) {
        c.hovered = false;
    }
}

fn on_card_click(on: On<Pointer<Click>>, mut cards: Query<&mut MenuCard>, mut sfx: MessageWriter<PlaySfx>) {
    if let Ok(mut c) = cards.get_mut(on.event_target())
        && c.spin <= 0.0
    {
        c.spin = SPIN_SECS;
        sfx.write(PlaySfx(Sfx::CardPlay));
    }
}

const SPIN_SECS: f32 = 0.9;

// ---- procedural textures --------------------------------------------------------------------

fn rgba_image(size: u32, mut px: impl FnMut(f32, f32) -> f32) -> Image {
    let mut data = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let a = px(x as f32 / (size - 1) as f32 * 2.0 - 1.0, y as f32 / (size - 1) as f32 * 2.0 - 1.0);
            data.extend_from_slice(&[255, 255, 255, (a.clamp(0.0, 1.0) * 255.0) as u8]);
        }
    }
    Image::new(Extent3d { width: size, height: size, depth_or_array_layers: 1 }, TextureDimension::D2, data, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::RENDER_WORLD)
}

fn radial_glow_image(size: u32) -> Image {
    rgba_image(size, |x, y| {
        let r2 = x * x + y * y;
        (-r2 * 4.0).exp() * (1.0 - r2).max(0.0)
    })
}

/// Soft fbm cloud with a round falloff, so the quad's edges never show.
fn cloud_image(size: u32, seed: u32) -> Image {
    let hash = |x: i32, y: i32| {
        let mut h = (x as u32).wrapping_mul(374761393) ^ (y as u32).wrapping_mul(668265263) ^ seed.wrapping_mul(2246822519);
        h = (h ^ (h >> 13)).wrapping_mul(1274126177);
        (h ^ (h >> 16)) as f32 / u32::MAX as f32
    };
    let noise = |x: f32, y: f32| {
        let (ix, iy) = (x.floor() as i32, y.floor() as i32);
        let (fx, fy) = (x - x.floor(), y - y.floor());
        let (ux, uy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
        let a = hash(ix, iy) + (hash(ix + 1, iy) - hash(ix, iy)) * ux;
        let b = hash(ix, iy + 1) + (hash(ix + 1, iy + 1) - hash(ix, iy + 1)) * ux;
        a + (b - a) * uy
    };
    rgba_image(size, |x, y| {
        let (mut f, mut amp, mut sx, mut sy) = (0.0, 0.5, x * 2.2 + 7.0, y * 2.2 + 3.0);
        for _ in 0..5 {
            f += amp * noise(sx, sy);
            sx = sx * 2.07 + 1.3;
            sy = sy * 2.07 + 7.1;
            amp *= 0.5;
        }
        let r2 = x * x + y * y;
        ((f - 0.38) * 2.2).clamp(0.0, 1.0).powf(1.6) * (1.0 - r2).max(0.0).powf(1.5)
    })
}

// ---- animation ------------------------------------------------------------------------------

fn smooth(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

fn ease_out_cubic(x: f32) -> f32 {
    1.0 - (1.0 - x.clamp(0.0, 1.0)).powi(3)
}

fn ease_out_back(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    let (c1, c3) = (1.4, 2.4);
    1.0 + c3 * (x - 1.0).powi(3) + c1 * (x - 1.0).powi(2)
}

fn start_intro(mut clock: ResMut<IntroClock>) {
    clock.t = 0.0;
    clock.full = !clock.seen;
    clock.seen = true;
    clock.sfx_fired = 0;
}

/// The menu camera replaces the table camera while the menu is up. Both render into the same
/// shared HDR target, and with both active the table's frame wins over our clear. The table is
/// hidden in menus anyway, so pausing it also saves a full 3D pass.
fn toggle_stage(
    state: Res<State<AppState>>,
    mut menu_cam: Query<&mut Camera, (With<MenuCamera>, Without<TableCamera>)>,
    mut table_cam: Query<&mut Camera, (With<TableCamera>, Without<MenuCamera>)>,
    mut root: Query<&mut Visibility, With<StageRoot>>,
) {
    let on = *state.get() == AppState::MainMenu;
    // the design studio has its own preview camera, so the table stays off there too
    let table_on = !on && *state.get() != AppState::Designer;
    for mut c in &mut menu_cam {
        if c.is_active != on {
            c.is_active = on;
        }
    }
    for mut c in &mut table_cam {
        if c.is_active != table_on {
            c.is_active = table_on;
        }
    }
    let want = if on { Visibility::Inherited } else { Visibility::Hidden };
    for mut v in &mut root {
        if *v != want {
            *v = want;
        }
    }
}

/// Advance the clock; any key or click during the full intro skips to its end.
fn tick_intro(
    time: Res<Time>,
    mut clock: ResMut<IntroClock>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut sfx: MessageWriter<PlaySfx>,
) {
    clock.t += time.delta_secs();
    if clock.full && clock.t < INTRO_END && (keys.get_just_pressed().next().is_some() || mouse.get_just_pressed().next().is_some()) {
        clock.t = INTRO_END;
        clock.sfx_fired = u8::MAX;
    }
    if !clock.full {
        return;
    }
    for (bit, at, s) in [(0, 0.45, Sfx::SpellCast), (1, 1.25, Sfx::AttackWhoosh), (2, 2.3, Sfx::Resolve)] {
        if clock.t >= at && clock.sfx_fired & (1 << bit) == 0 {
            clock.sfx_fired |= 1 << bit;
            sfx.write(PlaySfx(s));
        }
    }
}

fn animate_camera(
    time: Res<Time>,
    clock: Res<IntroClock>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut cam: Query<&mut Transform, (With<MenuCamera>, Without<StageRoot>)>,
    mut root: Query<&mut Transform, (With<StageRoot>, Without<MenuCamera>)>,
    mut parallax: Local<Vec2>,
) {
    let Ok(w) = window.single() else { return };
    let aspect = w.width() / w.height().max(1.0);
    // Keep the portal right of centre at any aspect, leaving the left for the menu buttons.
    let half_w = CAM_REST.z * (CAM_FOV_DEG.to_radians() * 0.5).tan() * aspect;
    let stage_x = (half_w * 0.43).clamp(1.0, 3.6);
    for mut tf in &mut root {
        tf.translation = Vec3::new(stage_x, 0.0, 0.0);
    }
    let cursor = w.cursor_position().map(|p| Vec2::new(p.x / w.width() * 2.0 - 1.0, p.y / w.height() * 2.0 - 1.0)).unwrap_or_default();
    *parallax = parallax.lerp(cursor, (time.delta_secs() * 2.5).min(1.0));
    let t = clock.stage_t();
    let d = ease_out_cubic(t / 2.9);
    let start = Vec3::new(-2.0, 3.2, 19.0);
    let idle = Vec3::new((t * 0.21).sin() * 0.18, (t * 0.17).cos() * 0.1, 0.0);
    let eye = start.lerp(CAM_REST, d) + idle + Vec3::new(parallax.x * 0.45, -parallax.y * 0.25, 0.0);
    // a slight roll that unwinds as the camera arrives
    let roll = (1.0 - d) * -0.12;
    for mut tf in &mut cam {
        *tf = Transform::from_translation(eye).looking_at(CAM_LOOK + Vec3::new(parallax.x * 0.15, 0.0, 0.0), Vec3::new(roll.sin(), roll.cos(), 0.0));
    }
}

fn animate_portal(
    clock: Res<IntroClock>,
    q: Query<&MeshMaterial3d<PortalMaterial>, With<PortalQuad>>,
    mut mats: ResMut<Assets<PortalMaterial>>,
    mut light: Query<&mut PointLight, With<PortalLight>>,
) {
    let t = clock.stage_t();
    // grow, overshoot into a flash, settle; then breathe
    let ign = if t < 0.45 {
        0.0
    } else if t < 1.25 {
        1.45 * ease_out_cubic((t - 0.45) / 0.8)
    } else {
        1.0 + 0.45 * (1.0 - smooth((t - 1.25) / 0.9)) + 0.03 * (t * 1.3).sin()
    };
    for h in &q {
        if let Some(mut m) = mats.get_mut(&h.0) {
            m.u.params.x = ign;
        }
    }
    for mut l in &mut light {
        l.intensity = 90_000.0 * ign.min(1.6);
    }
}

fn animate_cards(
    time: Res<Time>,
    clock: Res<IntroClock>,
    mut cards: Query<(&mut MenuCard, &mut Transform)>,
) {
    let dt = time.delta_secs();
    let t = clock.stage_t();
    let now = time.elapsed_secs();
    let ring_rot = now * 0.11;
    let tilt = Quat::from_rotation_x(RING_TILT);
    for (mut c, mut tf) in &mut cards {
        c.hover += ((if c.hovered { 1.0 } else { 0.0 }) - c.hover) * (dt * 10.0).min(1.0);
        c.spin = (c.spin - dt).max(0.0);
        let i = c.slot as f32;
        let a = i / N_CARDS as f32 * TAU + ring_rot;
        // fly-out from the portal centre, staggered, with a spiral and a spin
        let launch = 1.25 + i * 0.065;
        let e = ((t - launch) / 1.05).clamp(0.0, 1.0);
        if e <= 0.0 {
            tf.scale = Vec3::ZERO;
            continue;
        }
        let k = ease_out_back(e);
        let swirl = (1.0 - ease_out_cubic(e)) * 2.6;
        let radius = RING_RADIUS * k + c.hover * 0.45;
        let aa = a - swirl;
        let bob = (now * 0.8 + i * 1.3).sin() * 0.07;
        let local = Vec3::new(aa.sin() * radius, bob + c.hover * 0.12, aa.cos() * radius);
        tf.translation = tilt * local;
        let spin = (1.0 - ease_out_cubic(e)) * TAU * 1.5 + TAU * smooth(1.0 - c.spin / SPIN_SECS) * if c.spin > 0.0 { 1.0 } else { 0.0 };
        let wobble = Quat::from_rotation_z((now * 0.6 + i).sin() * 0.05);
        tf.rotation = tilt * Quat::from_rotation_y(aa + spin) * wobble;
        tf.scale = Vec3::splat(CARD_SCALE * (0.15 + 0.85 * ease_out_cubic(e)) * (1.0 + 0.1 * c.hover));
    }
}

fn animate_orbs(time: Res<Time>, clock: Res<IntroClock>, mut orbs: Query<(&ManaOrb, &mut Transform)>) {
    let t = clock.stage_t();
    let now = time.elapsed_secs();
    for (o, mut tf) in &mut orbs {
        let i = o.0 as f32;
        let b = i / 5.0 * TAU + now * 0.42;
        let r = 1.55 + (now * 0.9 + i).sin() * 0.06;
        // an inclined orbit, so the orbs pass in front of and behind the portal
        tf.translation = Vec3::new(b.cos() * r, b.sin() * r * 0.42 + (now * 1.7 + i * 2.0).sin() * 0.05, b.sin() * r * 0.85);
        let pop = ease_out_back((t - 1.9 - i * 0.12) / 0.45).max(0.0);
        tf.scale = Vec3::splat(pop * (1.0 + 0.12 * (now * 3.0 + i).sin()));
    }
}

fn animate_embers(time: Res<Time>, mut q: Query<(&Ember, &mut Transform)>) {
    let now = time.elapsed_secs();
    for (e, mut tf) in &mut q {
        let p = (e.phase + now * e.speed / 7.0).fract();
        let y = -3.2 + p * 7.0;
        let sway = (now * 0.5 + e.phase * 17.0).sin() * 0.35;
        tf.translation = Vec3::new(e.base.x + sway, y, e.base.z + (now * 0.4 + e.phase * 9.0).cos() * 0.2);
        // fade in at the bottom, out at the top, and flicker
        let life = (p * (1.0 - p) * 4.0).clamp(0.0, 1.0);
        tf.scale = Vec3::splat(e.size * life * (0.7 + 0.3 * (now * 7.0 + e.phase * 40.0).sin()));
    }
}

fn animate_haze(time: Res<Time>, mut q: Query<(&Haze, &mut Transform)>) {
    let now = time.elapsed_secs();
    for (h, mut tf) in &mut q {
        tf.rotation = Quat::from_rotation_z(now * h.0);
    }
}

/// Card faces arrive asynchronously from the engine's image proxy; until then the blank face shows.
fn swap_in_card_faces(images: Res<Assets<Image>>, mut mats: ResMut<Assets<StandardMaterial>>, mut cards: Query<&mut MenuCard>) {
    for mut c in &mut cards {
        if !c.loaded && images.contains(&c.image) {
            if let Some(mut m) = mats.get_mut(&c.material) {
                m.base_color_texture = Some(c.image.clone());
            }
            c.loaded = true;
        }
    }
}

/// Back material shared by the carousel cards: they wear your equipped sleeve.
#[derive(Resource)]
struct CarouselBack(Handle<StandardMaterial>);

fn sleeve_carousel(cosmetics: Res<Cosmetics>, back: Option<Res<CarouselBack>>, mut mats: ResMut<Assets<StandardMaterial>>, mut applied: Local<Option<Handle<Image>>>) {
    let (Some(back), Some(tex)) = (back, cosmetics.my_sleeve.clone()) else { return };
    if applied.as_ref() == Some(&tex) {
        return;
    }
    if let Some(mut m) = mats.get_mut(&back.0) {
        m.base_color_texture = Some(tex.clone());
        crate::cosmetics::apply_finish(&mut m, cosmetics.my_finish);
    }
    *applied = Some(tex);
}
