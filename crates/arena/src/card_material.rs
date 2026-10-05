//! The card look: art-crop battlefield frames, glow rims, and name plates.
//!
//! Why a material swap instead of one shader for everything: hand/stack/graveyard keep the full
//! card image (you need the text), while battlefield permanents use a compact frame
//! (coloured border, pale name bar, art window). cards.rs owns `MeshMaterial3d<StandardMaterial>`
//! on the `CardFront`; we *remove* that component while a card wears the frame material, so the
//! two systems never fight, and put a StandardMaterial back when it leaves the battlefield
//! (cards.rs then corrects it to the proper full-card texture on its next pass).
//!
//! Glow: every card root gets one extra additive quad slightly larger than the card, whose
//! material depends on Selectable / Highlighted / WeakSelectable (blue = playable/selectable,
//! gold = highlighted, soft blue = weak). That gives hand cards the blue edge glow too.

use std::collections::HashMap;

use bevy::camera::visibility::RenderLayers;
use bevy::light::NotShadowCaster;
use bevy::pbr::{ExtendedMaterial, MaterialExtension, MaterialPlugin};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use mtg_client::ImageVersion;
use mtg_protocol::{Card, CardId, ZoneKind};

use crate::cards::{CardAssets, CardFace, Despawning, card_mesh};
use crate::images::CardImages;
use crate::shared::*;
use crate::style::text_font;

pub struct CardMaterialPlugin;

impl Plugin for CardMaterialPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            MaterialPlugin::<ArenaCardMaterial>::default(),
            MaterialPlugin::<CardGlowMaterial>::default(),
        ))
        .init_resource::<NamePlates>()
        .add_systems(Startup, setup_glow_assets)
        .add_systems(
            Update,
            (
                ensure_glow_child,
                update_glow,
                swap_materials,
                update_card_uniforms,
                toggle_name_plates,
                update_name_plates,
            )
                .chain()
                .after(crate::view::ViewSet),
        );
    }
}

// -------------------------------------------------------------------------------------------
// Battlefield frame material
// -------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, ShaderType)]
pub struct CardFrameParams {
    /// rgb frame colour A
    pub frame_a: Vec4,
    /// rgb frame colour B, a = mix amount
    pub frame_b: Vec4,
    /// x selectable, y highlighted, z weak, w hover
    pub state: Vec4,
    /// x tapped, y damage fraction, z kind, w unused
    pub misc: Vec4,
}

#[derive(Asset, AsBindGroup, TypePath, Clone, Debug)]
pub struct CardFrameExt {
    #[uniform(100)]
    pub params: CardFrameParams,
    #[texture(101)]
    #[sampler(102)]
    pub art: Handle<Image>,
}

impl MaterialExtension for CardFrameExt {
    fn fragment_shader() -> ShaderRef {
        "shaders/card_frame.wgsl".into()
    }
    // Forward-only: the prepass would need its own variant of the shader for no visual gain.
    fn enable_prepass() -> bool {
        false
    }
}

/// Custom WGSL material for battlefield card fronts.
pub type ArenaCardMaterial = ExtendedMaterial<StandardMaterial, CardFrameExt>;

/// The frame colour pair for a card (WUBRG, gold for multicolour, grey artifacts, brown lands).
pub fn frame_colors(card: &Card) -> (Vec3, Vec3, f32) {
    let st = card.state.as_ref();
    let colors: Vec<char> = st.map(|s| s.colors.chars().filter(|c| "WUBRG".contains(*c)).collect()).unwrap_or_default();
    // authored as sRGB, uploaded linear (the shader works in linear light)
    let lin = |v: Vec3| -> Vec3 {
        let l = Color::srgb(v.x, v.y, v.z).to_linear();
        Vec3::new(l.red, l.green, l.blue)
    };
    let rgb = |c: char| -> Vec3 {
        lin(match c {
            'W' => Vec3::new(0.80, 0.76, 0.62),
            'U' => Vec3::new(0.16, 0.38, 0.70),
            'B' => Vec3::new(0.17, 0.14, 0.19),
            'R' => Vec3::new(0.72, 0.17, 0.12),
            'G' => Vec3::new(0.14, 0.45, 0.22),
            _ => Vec3::new(0.5, 0.5, 0.5),
        })
    };
    let land = lin(Vec3::new(0.40, 0.29, 0.17));
    let artifact = lin(Vec3::new(0.52, 0.56, 0.60));
    let gold = lin(Vec3::new(0.78, 0.60, 0.20));
    let is_land = st.is_some_and(|s| s.is_land);
    match colors.len() {
        0 => {
            // basic lands take their colour from the type (their `colors` is empty)
            let n = card.display_name();
            let basic = match n {
                "Plains" => Some('W'),
                "Island" => Some('U'),
                "Swamp" => Some('B'),
                "Mountain" => Some('R'),
                "Forest" => Some('G'),
                _ => None,
            };
            if let Some(c) = basic {
                (rgb(c), rgb(c), 0.0)
            } else if is_land {
                (land, land, 0.0)
            } else {
                (artifact, artifact, 0.0)
            }
        }
        1 => (rgb(colors[0]), rgb(colors[0]), 0.0),
        2 => (rgb(colors[0]), rgb(colors[1]), 1.0),
        _ => (gold, gold, 0.0),
    }
}

// -------------------------------------------------------------------------------------------
// Glow material
// -------------------------------------------------------------------------------------------

#[derive(Asset, AsBindGroup, TypePath, Clone, Debug)]
pub struct CardGlowMaterial {
    #[uniform(0)]
    pub glow: GlowParams,
}

#[derive(Clone, Copy, Debug, Default, ShaderType)]
pub struct GlowParams {
    /// rgb colour, a = intensity
    pub color: Vec4,
    /// x pulse amount, y pulse speed, z quad margin
    pub extra: Vec4,
}

impl Material for CardGlowMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/card_glow.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Add
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
}

const GLOW_MARGIN: f32 = 0.07;

#[derive(Resource)]
struct GlowAssets {
    mesh: Handle<Mesh>,
    selectable: Handle<CardGlowMaterial>,
    highlighted: Handle<CardGlowMaterial>,
    weak: Handle<CardGlowMaterial>,
    hovered: Handle<CardGlowMaterial>,
}

fn setup_glow_assets(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut mats: ResMut<Assets<CardGlowMaterial>>) {
    let mut mk = |c: Vec3, intensity: f32, pulse: f32| {
        mats.add(CardGlowMaterial { glow: GlowParams { color: c.extend(intensity), extra: Vec4::new(pulse, 4.0, GLOW_MARGIN, 0.0) } })
    };
    commands.insert_resource(GlowAssets {
        mesh: meshes.add(card_mesh(CARD_W + 2.0 * GLOW_MARGIN, CARD_H + 2.0 * GLOW_MARGIN, 0.034 + GLOW_MARGIN)),
        selectable: mk(Vec3::new(0.22, 0.62, 1.0), 2.4, 0.45),
        highlighted: mk(Vec3::new(1.0, 0.78, 0.28), 2.4, 0.3),
        weak: mk(Vec3::new(0.35, 0.6, 0.95), 1.0, 0.2),
        hovered: mk(Vec3::new(1.0, 0.95, 0.85), 0.5, 0.0),
    });
}

/// The glow quad child of a card root.
#[derive(Component)]
struct HasGlow(Entity);
#[derive(Component)]
struct GlowQuad;

fn ensure_glow_child(
    mut commands: Commands,
    assets: Option<Res<GlowAssets>>,
    roots: Query<(Entity, Option<&RenderLayers>), (With<CardEntity>, Without<HasGlow>, Without<Despawning>)>,
) {
    let Some(g) = assets else { return };
    for (e, layers) in &roots {
        let child = commands
            .spawn((
                GlowQuad,
                Mesh3d(g.mesh.clone()),
                MeshMaterial3d(g.selectable.clone()),
                Transform::from_xyz(0.0, 0.0, 0.0001),
                Visibility::Hidden,
                layers.cloned().unwrap_or(RenderLayers::layer(TABLE_LAYER)),
                NotShadowCaster,
                Pickable::IGNORE,
                ChildOf(e),
            ))
            .id();
        commands.entity(e).insert(HasGlow(child));
    }
}

fn update_glow(
    g: Option<Res<GlowAssets>>,
    roots: Query<(&HasGlow, Has<Selectable>, Has<Highlighted>, Has<WeakSelectable>, Has<Hovered>, Option<&RenderLayers>), With<CardEntity>>,
    mut quads: Query<(&mut MeshMaterial3d<CardGlowMaterial>, &mut Visibility, Option<&RenderLayers>), With<GlowQuad>>,
    mut commands: Commands,
) {
    let Some(g) = g else { return };
    for (hg, sel, hi, weak, hov, layers) in &roots {
        let Ok((mut m, mut vis, ql)) = quads.get_mut(hg.0) else { continue };
        let (want, show) = if hi {
            (&g.highlighted, true)
        } else if sel {
            (&g.selectable, true)
        } else if weak {
            (&g.weak, true)
        } else if hov {
            (&g.hovered, true)
        } else {
            (&g.selectable, false)
        };
        if m.0 != *want {
            m.0 = want.clone();
        }
        let v = if show { Visibility::Inherited } else { Visibility::Hidden };
        if *vis != v {
            *vis = v;
        }
        // motion switches the card between the table and hand worlds; follow it
        if let Some(l) = layers {
            if ql != Some(l) {
                commands.entity(hg.0).insert(l.clone());
            }
        }
    }
}

// -------------------------------------------------------------------------------------------
// Material swap
// -------------------------------------------------------------------------------------------

/// Present while the card wears the battlefield frame material.
#[derive(Component)]
struct ArenaMat {
    handle: Handle<ArenaCardMaterial>,
    last: CardFrameParams,
}

/// Art-crop request in flight for a battlefield card.
#[derive(Component)]
struct ArtRequest {
    name: String,
    handle: Handle<Image>,
}

fn find_card<'a>(view: &'a GameView, cache: &'a CardCache, id: CardId) -> Option<&'a Card> {
    view.card(id).or_else(|| cache.0.get(&id))
}

fn swap_materials(
    mut commands: Commands,
    view: Res<GameView>,
    cache: Res<CardCache>,
    mut images: ResMut<CardImages>,
    image_assets: Res<Assets<Image>>,
    mut mats: ResMut<Assets<ArenaCardMaterial>>,
    card_assets: Res<CardAssets>,
    roots: Query<
        (Entity, &CardEntity, &CardLocation, &CardFace, &Children, Option<&ArenaMat>, Option<&ArtRequest>),
        Without<Despawning>,
    >,
    fronts: Query<(), With<CardFront>>,
) {
    for (e, ce, loc, face, children, arena, req) in &roots {
        let Some(front) = children.iter().find(|c| fronts.contains(*c)) else { continue };
        let card = find_card(&view, &cache, ce.0);
        let wants_frame = loc.zone == ZoneKind::Battlefield
            && !face.hidden
            && !loc.hidden
            && card.is_some_and(|c| !c.face_down && c.state.is_some());
        let revert = |commands: &mut Commands| {
            commands.entity(front).remove::<MeshMaterial3d<ArenaCardMaterial>>().insert(MeshMaterial3d(card_assets.blank_mat.clone()));
            commands.entity(e).remove::<ArenaMat>().remove::<ArtRequest>();
        };
        if !wants_frame {
            if arena.is_some() {
                revert(&mut commands);
            } else if req.is_some() {
                commands.entity(e).remove::<ArtRequest>();
            }
            continue;
        }
        let card = card.unwrap();
        // (re)request the art crop; a renamed card (transform/copy) starts over
        let handle = match req {
            Some(r) if r.name == face.name => r.handle.clone(),
            _ => {
                if arena.is_some() {
                    revert(&mut commands);
                }
                let h = images.request(card, ImageVersion::ArtCrop);
                commands.entity(e).insert(ArtRequest { name: face.name.clone(), handle: h.clone() });
                continue;
            }
        };
        if arena.is_some() {
            continue;
        }
        // Servers that don't know art_crop return the full portrait card: keep the full card then.
        let landscape = image_assets.get(&handle).is_some_and(|i| i.width() > 16 && i.width() > i.height());
        if !landscape {
            continue;
        }
        let (fa, fb, mix) = frame_colors(card);
        let params = CardFrameParams {
            frame_a: fa.extend(1.0),
            frame_b: fb.extend(mix),
            state: Vec4::ZERO,
            misc: Vec4::new(card.tapped as u8 as f32, 0.0, 0.0, 0.0),
        };
        let h = mats.add(ArenaCardMaterial {
            base: StandardMaterial { perceptual_roughness: 0.8, reflectance: 0.1, ..default() },
            extension: CardFrameExt { params, art: handle },
        });
        commands.entity(front).remove::<MeshMaterial3d<StandardMaterial>>().insert(MeshMaterial3d(h.clone()));
        commands.entity(e).insert(ArenaMat { handle: h, last: params });
    }
}

fn update_card_uniforms(
    view: Res<GameView>,
    cache: Res<CardCache>,
    mut mats: ResMut<Assets<ArenaCardMaterial>>,
    mut q: Query<(&CardEntity, &mut ArenaMat, Has<Selectable>, Has<Highlighted>, Has<WeakSelectable>, Has<Hovered>)>,
) {
    for (ce, mut am, sel, hi, weak, hov) in &mut q {
        let Some(card) = find_card(&view, &cache, ce.0) else { continue };
        let toughness = card.state.as_ref().and_then(|s| s.toughness).unwrap_or(0).max(1) as f32;
        let dmg = if card.damage > 0 { (card.damage as f32 / toughness).min(1.0) } else { 0.0 };
        let mut p = am.last;
        p.state = Vec4::new(sel as u8 as f32, hi as u8 as f32, weak as u8 as f32, hov as u8 as f32);
        p.misc = Vec4::new(card.tapped as u8 as f32, dmg, 0.0, 0.0);
        if p != am.last {
            am.last = p;
            if let Some(mut m) = mats.get_mut(&am.handle) {
                m.extension.params = p;
            }
        }
    }
}

// -------------------------------------------------------------------------------------------
// Name plates
// -------------------------------------------------------------------------------------------

/// Projected UI labels with the card name over each battlefield card's name bar. UI text can't
/// live in the 3D material, and a per-card text->texture render would be far more expensive; a
/// fixed font size scaled/rotated with `UiTransform` also avoids regenerating font atlases.
#[derive(Resource)]
pub struct NamePlates {
    pub enabled: bool,
    map: HashMap<Entity, (Entity, String)>,
}

impl Default for NamePlates {
    fn default() -> Self {
        Self { enabled: true, map: HashMap::new() }
    }
}

#[derive(Component)]
struct NamePlate;

const PLATE_W: f32 = 118.0;
const PLATE_H: f32 = 24.0;

fn toggle_name_plates(keys: Res<ButtonInput<KeyCode>>, mut plates: ResMut<NamePlates>) {
    if keys.just_pressed(KeyCode::F8) {
        plates.enabled = !plates.enabled;
    }
}

fn update_name_plates(
    mut commands: Commands,
    mut plates: ResMut<NamePlates>,
    fonts: Res<ArenaFonts>,
    roots: Query<(Entity, &CardFace, &GlobalTransform), With<ArenaMat>>,
    camera: Query<(&Camera, &GlobalTransform), With<TableCamera>>,
    mut nodes: Query<(&mut Node, &mut UiTransform, &mut Visibility), With<NamePlate>>,
) {
    let Ok((cam, cam_gt)) = camera.single() else { return };
    // drop plates whose card left the battlefield (or when disabled)
    let stale: Vec<Entity> = plates
        .map
        .iter()
        .filter(|(root, (_, name))| !plates.enabled || !roots.get(**root).is_ok_and(|(_, f, _)| &f.name == name))
        .map(|(r, _)| *r)
        .collect();
    for r in stale {
        if let Some((pe, _)) = plates.map.remove(&r) {
            commands.entity(pe).despawn();
        }
    }
    if !plates.enabled {
        return;
    }
    let bar_y = CARD_H * 0.408;
    let half = CARD_W * 0.38;
    for (root, face, gt) in &roots {
        let pl = gt.transform_point(Vec3::new(-half, bar_y, 0.002));
        let pr = gt.transform_point(Vec3::new(half, bar_y, 0.002));
        let visible = match (cam.world_to_viewport(cam_gt, pl), cam.world_to_viewport(cam_gt, pr)) {
            (Ok(a), Ok(b)) => Some((a, b)),
            _ => None,
        };
        let entry = plates.map.get(&root).map(|(e, _)| *e);
        let pe = match entry {
            Some(e) => e,
            None => {
                let name = face.name.clone();
                let shown = if name.chars().count() > 15 { format!("{}…", name.chars().take(13).collect::<String>().trim_end()) } else { name.clone() };
                let e = commands
                    .spawn((
                        NamePlate,
                        GlobalZIndex(-5),
                        Pickable::IGNORE,
                        Node {
                            position_type: PositionType::Absolute,
                            width: Val::Px(PLATE_W),
                            height: Val::Px(PLATE_H),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            overflow: Overflow::clip(),
                            ..default()
                        },
                        UiTransform::IDENTITY,
                        Visibility::Hidden,
                    ))
                    .with_child((
                        Text::new(shown),
                        text_font(&fonts.title, 17.0),
                        TextColor(Color::srgb(0.09, 0.07, 0.06)),
                        TextLayout { linebreak: LineBreak::NoWrap, ..default() },
                        Pickable::IGNORE,
                    ))
                    .id();
                plates.map.insert(root, (e, name));
                e
            }
        };
        if let Ok((mut n, mut tf, mut vis)) = nodes.get_mut(pe) {
            match visible {
                Some((a, b)) => {
                    let c = (a + b) * 0.5;
                    let d = b - a;
                    let scale = (d.length() / PLATE_W).clamp(0.2, 1.4);
                    n.left = Val::Px(c.x - PLATE_W * 0.5);
                    n.top = Val::Px(c.y - PLATE_H * 0.5);
                    tf.scale = Vec2::splat(scale);
                    tf.rotation = Rot2::radians(d.y.atan2(d.x));
                    if *vis != Visibility::Inherited {
                        *vis = Visibility::Inherited;
                    }
                }
                None => {
                    if *vis != Visibility::Hidden {
                        *vis = Visibility::Hidden;
                    }
                }
            }
        }
    }
}
