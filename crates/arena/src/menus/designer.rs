//! Design studio: browse, equip and design sleeves, deckboxes and playmats, with a live 3D preview
//! on its own camera and render layer (`DESIGNER_LAYER`).
//!
//! Why its own click handling instead of `MenuAction`: the studio's buttons are dozens of small
//! edits to the design being worked on, and they never leave this screen. Only entering and
//! leaving the studio go through `MenuAction`, like every other screen transition.
//!
//! Editing a preset forks it into a custom copy on the first change, so presets stay pristine and
//! the player never has to think about "duplicate first". The library saves itself (cosmetics).
//!
//! The preview shows the whole loadout: each tab's selected design is what the other tabs show
//! too, so the sleeve chosen on the first tab is the one lying on the playmat on the third.

use std::collections::HashMap;
use std::f32::consts::PI;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{ClearColorConfig, Hdr};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::input::mouse::{AccumulatedMouseMotion, MouseScrollUnit, MouseWheel};
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::text::EditableText;
use bevy::ui::RelativeCursorPosition;
use mtg_client::curated::CURATED;
use mtg_client::{ImageRequest, ImageVersion};

use super::deck_select::{DeckList, DeckSelection};
use super::widgets::{Field, text_field};
use super::MenuAction;
use crate::cards::card_mesh;
use crate::cosmetics::mesh::deckbox_meshes;
use crate::cosmetics::{Design, DesignKind, Emblem, Finish, Frame, PALETTE, PaintTarget, Pattern, Rgb, apply_finish};
use crate::shared::*;
use crate::style::*;

pub struct DesignerPlugin;

impl Plugin for DesignerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Studio>()
            .add_systems(OnEnter(AppState::Designer), (enter_studio, spawn_screen, spawn_preview).chain())
            .add_systems(
                Update,
                (
                    handle_clicks,
                    name_edits,
                    scale_slider,
                    rebuild_list,
                    rebuild_editor,
                    refresh_visuals,
                    list_thumbs,
                    wheel_scroll,
                    (drag_preview, animate_preview, preview_textures),
                )
                    .chain()
                    .run_if(in_state(AppState::Designer)),
            );
    }
}

// ---- state ----------------------------------------------------------------------------------

#[derive(Resource)]
struct Studio {
    kind: DesignKind,
    /// design being worked on, per tab
    selected: HashMap<DesignKind, String>,
    rebuild_list: bool,
    rebuild_editor: bool,
    yaw: f32,
    pitch: f32,
    /// seconds since the last drag; auto-rotation resumes after a pause
    idle: f32,
    dragging: bool,
}

impl Default for Studio {
    fn default() -> Self {
        Self { kind: DesignKind::Sleeve, selected: HashMap::new(), rebuild_list: true, rebuild_editor: true, yaw: PI + 0.5, pitch: 0.0, idle: 10.0, dragging: false }
    }
}

impl Studio {
    fn current(&self) -> &str {
        self.selected.get(&self.kind).map(String::as_str).unwrap_or("")
    }
    fn design<'a>(&self, lib: &'a DesignLibrary, kind: DesignKind) -> &'a Design {
        lib.resolve(kind, self.selected.get(&kind).map(String::as_str).unwrap_or(""))
    }
    fn select(&mut self, kind: DesignKind, id: String) {
        self.selected.insert(kind, id);
        self.rebuild_list = true;
        self.rebuild_editor = true;
    }
}

#[derive(Component, Debug, Clone, PartialEq)]
enum StudioAction {
    Tab(DesignKind),
    Select(String),
    New,
    Delete,
    Randomize,
    Equip,
    EquipDeck,
    Pattern(Pattern),
    Emblem(Emblem),
    Frame(Frame),
    Finish(Finish),
    /// colour slot (0 base, 1 detail, 2 accent) and value
    Color(u8, Rgb),
}

fn enter_studio(mut studio: ResMut<Studio>, lib: Res<DesignLibrary>, settings: Res<Settings>, sel: Res<DeckSelection>) {
    // start every tab on what's equipped (for the deck picked in deck select, if any)
    let deck = sel.human.as_deref();
    let equipped = [
        (DesignKind::Sleeve, lib.sleeve_for(&settings, deck).id.clone()),
        (DesignKind::Deckbox, lib.deckbox_for(&settings, deck).id.clone()),
        (DesignKind::Playmat, lib.playmat(&settings).id.clone()),
    ];
    for (k, id) in equipped {
        if studio.selected.get(&k).is_none_or(|s| lib.get(s).is_none()) {
            studio.selected.insert(k, id);
        }
    }
    // dev aid for unattended screenshots: ARENA_MENU_SCREEN=designer-box / designer-mat
    match std::env::var("ARENA_MENU_SCREEN").as_deref() {
        Ok("designer-box") => studio.kind = DesignKind::Deckbox,
        Ok("designer-mat") => studio.kind = DesignKind::Playmat,
        _ => {}
    }
    studio.rebuild_list = true;
    studio.rebuild_editor = true;
    studio.yaw = rest_yaw(studio.kind);
    studio.pitch = 0.0;
}

fn rest_yaw(kind: DesignKind) -> f32 {
    match kind {
        // show the sleeved back first
        DesignKind::Sleeve => PI + 0.45,
        DesignKind::Deckbox => -0.55,
        DesignKind::Playmat => 0.0,
    }
}

// ---- screen ---------------------------------------------------------------------------------

#[derive(Component)]
struct DesignList;
#[derive(Component)]
struct EditorPanel;
#[derive(Component)]
struct StudioScroll;
#[derive(Component)]
struct PreviewArea;
#[derive(Component)]
struct ListRow(String);
#[derive(Component)]
struct ListName(String);
#[derive(Component)]
struct ListBadge(String);
#[derive(Component)]
struct ListThumb(String);
#[derive(Component)]
struct EquipButton;
#[derive(Component)]
struct EquipDeckButton;
#[derive(Component)]
struct EquipDeckLabel;
#[derive(Component)]
struct DeleteButton;
#[derive(Component)]
struct EquipStatus;
#[derive(Component)]
struct PresetNotice;
#[derive(Component)]
struct Swatch;
#[derive(Component)]
struct ScaleTrack;
#[derive(Component)]
struct ScaleFill;
#[derive(Component)]
struct ScaleText;

fn spawn_screen(mut commands: Commands, fonts: Res<ArenaFonts>) {
    commands
        .spawn((
            Name::new("Designer"),
            DespawnOnExit(AppState::Designer),
            Node { flex_direction: FlexDirection::Column, padding: UiRect::all(Val::Px(24.0)), row_gap: Val::Px(16.0), ..fullscreen_root() },
            Pickable::IGNORE,
        ))
        .with_children(|root| {
            // ---- top bar
            root.spawn(Node { width: Val::Percent(100.0), align_items: AlignItems::Center, column_gap: Val::Px(14.0), ..default() }).with_children(|bar| {
                let b = spawn_button(bar, &fonts, ButtonKind::Secondary, "BACK", Vec2::new(130.0, 46.0));
                bar.commands().entity(b).insert(MenuAction::CloseDesigner);
                bar.spawn((Node { flex_grow: 1.0, justify_content: JustifyContent::Center, ..default() }, Pickable::IGNORE)).with_child(heading(&fonts, "DESIGN STUDIO", 42.0));
                for k in DesignKind::ALL {
                    let b = spawn_button(bar, &fonts, ButtonKind::Secondary, k.label(), Vec2::new(170.0, 46.0));
                    bar.commands().entity(b).insert(StudioAction::Tab(k));
                }
            });
            // ---- middle
            root.spawn((Node { width: Val::Percent(100.0), flex_grow: 1.0, min_height: Val::Px(0.0), column_gap: Val::Px(18.0), ..default() }, Pickable::IGNORE)).with_children(|mid| {
                // collection
                mid.spawn(ornate_panel(Node { width: Val::Px(300.0), padding: UiRect::all(Val::Px(14.0)), flex_direction: FlexDirection::Column, row_gap: Val::Px(6.0), ..default() })).with_children(|p| {
                    p.spawn(heading(&fonts, "COLLECTION", 22.0));
                    p.spawn(divider());
                    p.spawn((
                        DesignList,
                        StudioScroll,
                        ScrollPosition::default(),
                        RelativeCursorPosition::default(),
                        Node { width: Val::Percent(100.0), flex_grow: 1.0, min_height: Val::Px(0.0), flex_direction: FlexDirection::Column, row_gap: Val::Px(6.0), overflow: Overflow::scroll_y(), ..default() },
                    ));
                    p.spawn(Node { column_gap: Val::Px(10.0), margin: UiRect::top(Val::Px(8.0)), ..default() }).with_children(|r| {
                        let b = spawn_button(r, &fonts, ButtonKind::Secondary, "NEW", Vec2::new(128.0, 42.0));
                        r.commands().entity(b).insert(StudioAction::New);
                        let b = spawn_button(r, &fonts, ButtonKind::Danger, "DELETE", Vec2::new(128.0, 42.0));
                        r.commands().entity(b).insert((StudioAction::Delete, DeleteButton));
                    });
                });
                // preview window (the 3D scene shows through)
                mid.spawn((
                    PreviewArea,
                    RelativeCursorPosition::default(),
                    Node { flex_grow: 1.0, min_width: Val::Px(0.0), flex_direction: FlexDirection::Column, justify_content: JustifyContent::FlexEnd, align_items: AlignItems::Center, row_gap: Val::Px(10.0), ..default() },
                ))
                .with_children(|c| {
                    c.spawn((EquipStatus, ui_text(&fonts.body, "", 16.0, TEXT), TextShadow { offset: Vec2::new(0.0, 1.0), color: Color::srgba(0.0, 0.0, 0.0, 0.9) }, Pickable::IGNORE));
                    c.spawn(Node { column_gap: Val::Px(12.0), ..default() }).with_children(|r| {
                        let b = spawn_button(r, &fonts, ButtonKind::Primary, "EQUIP", Vec2::new(200.0, 56.0));
                        r.commands().entity(b).insert((StudioAction::Equip, EquipButton));
                        r.spawn((button(ButtonKind::Secondary, Node { height: Val::Px(56.0), padding: UiRect::axes(Val::Px(18.0), Val::Px(0.0)), ..default() }), StudioAction::EquipDeck, EquipDeckButton)).with_child((
                            EquipDeckLabel,
                            Text::new("USE FOR DECK"),
                            text_font(&fonts.title, 18.0),
                            TextColor(GOLD_BRIGHT),
                            TextShadow { offset: Vec2::new(0.0, 1.5), color: Color::srgba(0.0, 0.0, 0.0, 0.7) },
                        ));
                    });
                    c.spawn((ui_text(&fonts.body, "drag to turn", 13.0, TEXT_DIM), Pickable::IGNORE));
                });
                // editor
                mid.spawn(ornate_panel(Node { width: Val::Px(450.0), padding: UiRect::all(Val::Px(16.0)), flex_direction: FlexDirection::Column, ..default() })).with_children(|p| {
                    p.spawn((
                        EditorPanel,
                        StudioScroll,
                        ScrollPosition::default(),
                        RelativeCursorPosition::default(),
                        Node { width: Val::Percent(100.0), flex_grow: 1.0, min_height: Val::Px(0.0), flex_direction: FlexDirection::Column, row_gap: Val::Px(6.0), overflow: Overflow::scroll_y(), ..default() },
                    ));
                });
            });
        });
}

fn thumb_size(kind: DesignKind) -> Vec2 {
    match kind {
        DesignKind::Sleeve => Vec2::new(36.0, 50.0),
        DesignKind::Deckbox => Vec2::new(46.0, 52.0),
        DesignKind::Playmat => Vec2::new(78.0, 50.0),
    }
}

fn rebuild_list(mut commands: Commands, mut studio: ResMut<Studio>, lib: Res<DesignLibrary>, fonts: Res<ArenaFonts>, list: Query<(Entity, Option<&Children>), With<DesignList>>) {
    if !studio.rebuild_list {
        return;
    }
    let Ok((e, ch)) = list.single() else { return };
    studio.rebuild_list = false;
    for c in ch.into_iter().flatten() {
        commands.entity(*c).despawn();
    }
    let size = thumb_size(studio.kind);
    commands.entity(e).with_children(|l| {
        for d in lib.of_kind(studio.kind) {
            l.spawn((
                Button,
                StudioAction::Select(d.id.clone()),
                ListRow(d.id.clone()),
                Node {
                    width: Val::Percent(100.0),
                    padding: UiRect::axes(Val::Px(8.0), Val::Px(6.0)),
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(10.0),
                    border: UiRect::all(Val::Px(1.5)),
                    border_radius: BorderRadius::all(Val::Px(8.0)),
                    flex_shrink: 0.0,
                    ..default()
                },
                BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.04)),
                BorderColor::all(Color::NONE),
            ))
            .with_children(|r| {
                r.spawn((
                    ListThumb(d.id.clone()),
                    Node { width: Val::Px(size.x), height: Val::Px(size.y), flex_shrink: 0.0, border_radius: BorderRadius::all(Val::Px(4.0)), ..default() },
                    ImageNode { color: Color::NONE, ..default() },
                    Pickable::IGNORE,
                ));
                r.spawn((Node { flex_direction: FlexDirection::Column, flex_grow: 1.0, min_width: Val::Px(0.0), row_gap: Val::Px(2.0), ..default() }, Pickable::IGNORE)).with_children(|c| {
                    c.spawn((ListName(d.id.clone()), ui_text(&fonts.title, d.name.clone(), 15.0, PARCHMENT), Pickable::IGNORE));
                    c.spawn((ListBadge(d.id.clone()), ui_text(&fonts.body, "", 12.0, TEXT_DIM), Pickable::IGNORE));
                });
            });
        }
    });
}

fn section(p: &mut ChildSpawnerCommands, fonts: &ArenaFonts, label: &str) {
    p.spawn((Node { margin: UiRect::top(Val::Px(10.0)), ..default() }, ui_text(&fonts.title, label, 15.0, GOLD), Pickable::IGNORE));
}

fn chip(p: &mut ChildSpawnerCommands, fonts: &ArenaFonts, label: &str, act: StudioAction) {
    p.spawn((button(ButtonKind::Secondary, Node { height: Val::Px(30.0), padding: UiRect::axes(Val::Px(12.0), Val::Px(0.0)), ..default() }), act))
        .with_child((Text::new(label), text_font(&fonts.body, 14.0), TextColor(TEXT), Pickable::IGNORE));
}

fn chip_row(p: &mut ChildSpawnerCommands, f: impl FnOnce(&mut ChildSpawnerCommands)) {
    p.spawn(Node { flex_wrap: FlexWrap::Wrap, column_gap: Val::Px(6.0), row_gap: Val::Px(6.0), flex_shrink: 0.0, ..default() }).with_children(f);
}

fn rgb(c: Rgb) -> Color {
    Color::srgb_u8(c[0], c[1], c[2])
}

fn rebuild_editor(mut commands: Commands, mut studio: ResMut<Studio>, lib: Res<DesignLibrary>, fonts: Res<ArenaFonts>, panel: Query<(Entity, Option<&Children>), With<EditorPanel>>) {
    if !studio.rebuild_editor {
        return;
    }
    let Ok((e, ch)) = panel.single() else { return };
    studio.rebuild_editor = false;
    for c in ch.into_iter().flatten() {
        commands.entity(*c).despawn();
    }
    let kind = studio.kind;
    let d = studio.design(&lib, kind).clone();
    commands.entity(e).with_children(|p| {
        p.spawn((ui_text(&fonts.title, format!("YOUR {}", kind.singular().to_uppercase()), 22.0, GOLD_BRIGHT), Pickable::IGNORE));
        p.spawn(divider());
        p.spawn(text_field(&fonts, Field::DesignName, &d.name, Val::Percent(100.0), 1.0, false));
        p.spawn((
            PresetNotice,
            Node { margin: UiRect::top(Val::Px(4.0)), ..default() },
            ui_text(&fonts.body, "Preset: your first change saves it as a new custom design.", 13.0, TEXT_DIM),
            Pickable::IGNORE,
        ));
        section(p, &fonts, "PATTERN");
        chip_row(p, |r| {
            for &v in Pattern::ALL {
                chip(r, &fonts, v.label(), StudioAction::Pattern(v));
            }
        });
        section(p, &fonts, "COLOURS");
        for (slot, label) in [(0u8, "Base"), (1, "Detail"), (2, "Accent")] {
            p.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(8.0), flex_shrink: 0.0, ..default() }).with_children(|r| {
                r.spawn((Node { width: Val::Px(56.0), ..default() }, ui_text(&fonts.body, label, 14.0, TEXT), Pickable::IGNORE));
                r.spawn(Node { flex_wrap: FlexWrap::Wrap, column_gap: Val::Px(3.0), row_gap: Val::Px(3.0), ..default() }).with_children(|w| {
                    for c in PALETTE {
                        w.spawn((
                            Button,
                            Swatch,
                            StudioAction::Color(slot, c),
                            Node { width: Val::Px(19.0), height: Val::Px(19.0), border: UiRect::all(Val::Px(2.0)), border_radius: BorderRadius::all(Val::Px(4.0)), ..default() },
                            BackgroundColor(rgb(c)),
                            BorderColor::all(Color::srgba(0.0, 0.0, 0.0, 0.6)),
                        ));
                    }
                });
            });
        }
        section(p, &fonts, "EMBLEM");
        chip_row(p, |r| {
            for &v in Emblem::ALL {
                chip(r, &fonts, v.label(), StudioAction::Emblem(v));
            }
        });
        p.spawn(Node { column_gap: Val::Px(26.0), flex_shrink: 0.0, ..default() }).with_children(|r| {
            r.spawn(Node { flex_direction: FlexDirection::Column, ..default() }).with_children(|c| {
                section(c, &fonts, "FRAME");
                chip_row(c, |r| {
                    for &v in Frame::ALL {
                        chip(r, &fonts, v.label(), StudioAction::Frame(v));
                    }
                });
            });
            r.spawn(Node { flex_direction: FlexDirection::Column, ..default() }).with_children(|c| {
                section(c, &fonts, "FINISH");
                chip_row(c, |r| {
                    for &v in Finish::ALL {
                        chip(r, &fonts, v.label(), StudioAction::Finish(v));
                    }
                });
            });
        });
        section(p, &fonts, "PATTERN SCALE");
        p.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(12.0), flex_shrink: 0.0, ..default() }).with_children(|r| {
            r.spawn((
                Button,
                ScaleTrack,
                RelativeCursorPosition::default(),
                Node { flex_grow: 1.0, height: Val::Px(14.0), border: UiRect::all(Val::Px(1.5)), border_radius: BorderRadius::all(Val::Px(99.0)), ..default() },
                BackgroundColor(Color::srgba(0.02, 0.012, 0.01, 0.9)),
                BorderColor::all(GOLD_DIM),
            ))
            .with_child((ScaleFill, Node { width: Val::Percent(50.0), height: Val::Percent(100.0), border_radius: BorderRadius::all(Val::Px(99.0)), ..default() }, BackgroundColor(ORANGE), Pickable::IGNORE));
            r.spawn((ScaleText, Node { width: Val::Px(52.0), ..default() }, ui_text(&fonts.numbers, "", 18.0, GOLD_BRIGHT), Pickable::IGNORE));
        });
        p.spawn(Node { margin: UiRect::top(Val::Px(14.0)), flex_shrink: 0.0, ..default() }).with_children(|r| {
            let b = spawn_button(r, &fonts, ButtonKind::Secondary, "RANDOMIZE", Vec2::new(200.0, 44.0));
            r.commands().entity(b).insert(StudioAction::Randomize);
        });
    });
}

// ---- editing --------------------------------------------------------------------------------

/// Apply `f` to the design being worked on. A preset is forked into a custom copy first; the
/// editor is rebuilt for that (the copy's name changes) unless `keep_editor` (typing in the name
/// field, whose focus a rebuild would steal).
fn edit(lib: &mut DesignLibrary, studio: &mut Studio, keep_editor: bool, f: impl FnOnce(&mut Design)) {
    let kind = studio.kind;
    let id = studio.design(lib, kind).id.clone();
    let Some(d) = lib.get(&id) else { return };
    if d.builtin {
        let mut copy = d.clone();
        copy.id = lib.new_id(kind);
        copy.name = format!("{} (Custom)", d.name);
        copy.builtin = false;
        copy.image = None;
        f(&mut copy);
        let new_id = copy.id.clone();
        lib.designs.push(copy);
        studio.selected.insert(kind, new_id);
        studio.rebuild_list = true;
        studio.rebuild_editor |= !keep_editor;
    } else if let Some(d) = lib.get_mut(&id) {
        f(d);
    }
}

fn handle_clicks(
    q: Query<(&Interaction, &StudioAction, Option<&ArenaButton>), Changed<Interaction>>,
    mut studio: ResMut<Studio>,
    mut lib: ResMut<DesignLibrary>,
    mut settings: ResMut<Settings>,
    sel: Res<DeckSelection>,
) {
    for (i, act, btn) in &q {
        if *i != Interaction::Pressed || btn.is_some_and(|b| b.disabled) {
            continue;
        }
        let kind = studio.kind;
        match act.clone() {
            StudioAction::Tab(k) => {
                if k != kind {
                    studio.kind = k;
                    studio.rebuild_list = true;
                    studio.rebuild_editor = true;
                    studio.yaw = rest_yaw(k);
                    studio.pitch = 0.0;
                }
            }
            StudioAction::Select(id) => {
                if id != studio.current() {
                    studio.select(kind, id);
                }
            }
            StudioAction::New => {
                let mut d = studio.design(&lib, kind).clone();
                d.id = lib.new_id(kind);
                d.name = format!("My {}", capitalize(kind.singular()));
                d.builtin = false;
                d.image = None;
                let id = d.id.clone();
                lib.designs.push(d);
                studio.select(kind, id);
            }
            StudioAction::Delete => {
                let id = studio.current().to_string();
                if lib.get(&id).is_none_or(|d| d.builtin) {
                    continue;
                }
                let pos = lib.designs.iter().position(|d| d.id == id).unwrap_or(0);
                lib.designs.retain(|d| d.id != id);
                forget_design(&mut settings, &id);
                // select the neighbour above, which always exists (presets come first)
                let next = lib.designs[..pos.min(lib.designs.len())].iter().rev().find(|d| d.kind == kind).map(|d| d.id.clone()).unwrap_or_default();
                studio.select(kind, next);
            }
            StudioAction::Randomize => {
                let seed = (mtg_client::now_millis() as u32).wrapping_mul(2_654_435_761) | 7;
                edit(&mut lib, &mut studio, false, |d| d.randomize(seed));
            }
            StudioAction::Equip => {
                let id = studio.current().to_string();
                match kind {
                    DesignKind::Sleeve => settings.sleeve = id,
                    DesignKind::Deckbox => settings.deckbox = id,
                    DesignKind::Playmat => settings.theme = id,
                }
            }
            StudioAction::EquipDeck => {
                let (Some(deck), id) = (sel.human.clone(), studio.current().to_string()) else { continue };
                let look = settings.deck_looks.entry(deck).or_default();
                match kind {
                    DesignKind::Sleeve => look.sleeve = Some(id),
                    DesignKind::Deckbox => look.deckbox = Some(id),
                    DesignKind::Playmat => {}
                }
            }
            StudioAction::Pattern(v) => edit(&mut lib, &mut studio, false, |d| d.pattern = v),
            StudioAction::Emblem(v) => edit(&mut lib, &mut studio, false, |d| d.emblem = v),
            StudioAction::Frame(v) => edit(&mut lib, &mut studio, false, |d| d.frame = v),
            StudioAction::Finish(v) => edit(&mut lib, &mut studio, false, |d| d.finish = v),
            StudioAction::Color(slot, c) => edit(&mut lib, &mut studio, false, |d| match slot {
                0 => d.base = c,
                1 => d.detail = c,
                _ => d.accent = c,
            }),
        }
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

/// Drop every reference to a deleted design so settings fall back to the defaults.
fn forget_design(settings: &mut Settings, id: &str) {
    let defaults = Settings::default();
    if settings.sleeve == id {
        settings.sleeve = defaults.sleeve.clone();
    }
    if settings.deckbox == id {
        settings.deckbox = defaults.deckbox.clone();
    }
    if settings.theme == id {
        settings.theme = defaults.theme.clone();
    }
    for look in settings.deck_looks.values_mut() {
        if look.sleeve.as_deref() == Some(id) {
            look.sleeve = None;
        }
        if look.deckbox.as_deref() == Some(id) {
            look.deckbox = None;
        }
    }
}

fn name_edits(q: Query<(&Field, &EditableText), Changed<EditableText>>, mut studio: ResMut<Studio>, mut lib: ResMut<DesignLibrary>) {
    for (f, t) in &q {
        if *f != Field::DesignName {
            continue;
        }
        let name = t.value().to_string();
        if name.trim().is_empty() || studio.design(&lib, studio.kind).name == name {
            continue;
        }
        edit(&mut lib, &mut studio, true, |d| d.name = name.clone());
    }
}

fn scale_slider(q: Query<(&Interaction, &RelativeCursorPosition), With<ScaleTrack>>, mut studio: ResMut<Studio>, mut lib: ResMut<DesignLibrary>) {
    for (i, rel) in &q {
        if *i != Interaction::Pressed {
            continue;
        }
        let Some(n) = rel.normalized else { continue };
        let v = ((0.5 + (n.x + 0.5).clamp(0.0, 1.0) * 1.5) * 20.0).round() / 20.0;
        if (studio.design(&lib, studio.kind).scale - v).abs() > 1e-4 {
            edit(&mut lib, &mut studio, false, |d| d.scale = v);
        }
    }
}

// ---- visuals --------------------------------------------------------------------------------

fn refresh_visuals(
    studio: Res<Studio>,
    lib: Res<DesignLibrary>,
    settings: Res<Settings>,
    sel: Res<DeckSelection>,
    decks: Res<DeckList>,
    mut buttons: Query<(&StudioAction, &mut ArenaButton), Without<Swatch>>,
    mut swatches: Query<(&StudioAction, &mut BorderColor), (With<Swatch>, Without<ListRow>)>,
    mut rows: Query<(&ListRow, &Interaction, &mut BackgroundColor, &mut BorderColor), Without<Swatch>>,
    mut names: Query<(&ListName, &mut Text), (Without<ListBadge>, Without<EquipStatus>, Without<ScaleText>, Without<EquipDeckLabel>)>,
    mut badges: Query<(&ListBadge, &mut Text), (Without<ListName>, Without<EquipStatus>, Without<ScaleText>, Without<EquipDeckLabel>)>,
    mut status: Query<&mut Text, (With<EquipStatus>, Without<ListName>, Without<ListBadge>, Without<ScaleText>, Without<EquipDeckLabel>)>,
    mut scale_text: Query<&mut Text, (With<ScaleText>, Without<ListName>, Without<ListBadge>, Without<EquipStatus>, Without<EquipDeckLabel>)>,
    mut deck_label: Query<&mut Text, (With<EquipDeckLabel>, Without<ListName>, Without<ListBadge>, Without<EquipStatus>, Without<ScaleText>)>,
    mut scale_fill: Query<&mut Node, With<ScaleFill>>,
    mut notice: Query<&mut Node, (With<PresetNotice>, Without<ScaleFill>)>,
    mut deck_btn: Query<&mut Node, (With<EquipDeckButton>, Without<ScaleFill>, Without<PresetNotice>)>,
) {
    let kind = studio.kind;
    let d = studio.design(&lib, kind);
    let deck = sel.human.as_deref();
    let default_id = match kind {
        DesignKind::Sleeve => &settings.sleeve,
        DesignKind::Deckbox => &settings.deckbox,
        DesignKind::Playmat => &settings.theme,
    };
    let deck_id = deck.and_then(|r| settings.deck_looks.get(r)).and_then(|l| match kind {
        DesignKind::Sleeve => l.sleeve.as_ref(),
        DesignKind::Deckbox => l.deckbox.as_ref(),
        DesignKind::Playmat => None,
    });
    let is_default = lib.resolve(kind, default_id).id == d.id;
    let is_deck = deck_id == Some(&d.id);

    for (act, mut b) in &mut buttons {
        let on = match act {
            StudioAction::Tab(k) => *k == kind,
            StudioAction::Pattern(v) => *v == d.pattern,
            StudioAction::Emblem(v) => *v == d.emblem,
            StudioAction::Frame(v) => *v == d.frame,
            StudioAction::Finish(v) => *v == d.finish,
            _ => false,
        };
        let disabled = match act {
            StudioAction::Delete => d.builtin,
            StudioAction::Equip => is_default,
            StudioAction::EquipDeck => is_deck,
            _ => false,
        };
        let want = match act {
            StudioAction::Delete => ButtonKind::Danger,
            StudioAction::Equip => ButtonKind::Primary,
            _ if on => ButtonKind::Primary,
            _ => ButtonKind::Secondary,
        };
        if b.kind != want {
            b.kind = want;
        }
        if b.disabled != disabled {
            b.disabled = disabled;
        }
    }
    for (act, mut bc) in &mut swatches {
        let StudioAction::Color(slot, c) = act else { continue };
        let cur = [d.base, d.detail, d.accent][*slot as usize];
        let want = if cur == *c { GOLD_BRIGHT } else { Color::srgba(0.0, 0.0, 0.0, 0.6) };
        if bc.top != want {
            *bc = BorderColor::all(want);
        }
    }
    for (row, i, mut bg, mut bc) in &mut rows {
        let selected = row.0 == d.id;
        let hov = matches!(i, Interaction::Hovered | Interaction::Pressed);
        bg.0 = if selected { Color::srgba(0.95, 0.46, 0.13, 0.2) } else if hov { Color::srgba(1.0, 1.0, 1.0, 0.09) } else { Color::srgba(1.0, 1.0, 1.0, 0.04) };
        *bc = BorderColor::all(if selected { ORANGE } else { Color::NONE });
    }
    for (n, mut t) in &mut names {
        if let Some(d) = lib.get(&n.0)
            && t.0 != d.name
        {
            t.0 = d.name.clone();
        }
    }
    for (b, mut t) in &mut badges {
        let Some(row) = lib.get(&b.0) else { continue };
        let mut s = String::from(if row.builtin { "Preset" } else { "Custom" });
        if lib.resolve(kind, default_id).id == row.id {
            s += "  ·  EQUIPPED";
        } else if deck_id == Some(&row.id) {
            s += "  ·  THIS DECK";
        }
        if t.0 != s {
            t.0 = s;
        }
    }
    let deck_name = deck.map(|r| decks.name_of(r));
    let msg = match (is_default, is_deck, &deck_name) {
        (_, true, Some(n)) => format!("Used by {n}"),
        (true, _, _) => format!("Your default {}", kind.singular()),
        _ => String::new(),
    };
    for mut t in &mut status {
        if t.0 != msg {
            t.0 = msg.clone();
        }
    }
    let label = deck_name.map(|n| format!("USE FOR {}", n.to_uppercase())).unwrap_or_default();
    for mut t in &mut deck_label {
        if t.0 != label {
            t.0 = label.clone();
        }
    }
    // per-deck choices exist for sleeves and boxes, and only once a deck is picked
    let show_deck = deck.is_some() && kind != DesignKind::Playmat;
    for mut n in &mut deck_btn {
        let want = if show_deck { Display::Flex } else { Display::None };
        if n.display != want {
            n.display = want;
        }
    }
    let p = ((d.scale - 0.5) / 1.5).clamp(0.0, 1.0) * 100.0;
    for mut n in &mut scale_fill {
        if n.width != Val::Percent(p) {
            n.width = Val::Percent(p);
        }
    }
    let s = format!("{:.2}x", d.scale);
    for mut t in &mut scale_text {
        if t.0 != s {
            t.0 = s.clone();
        }
    }
    let want = if d.builtin { Display::Flex } else { Display::None };
    for mut n in &mut notice {
        if n.display != want {
            n.display = want;
        }
    }
}

fn list_thumbs(lib: Res<DesignLibrary>, mut paints: ResMut<Paints>, mut q: Query<(&ListThumb, &mut ImageNode)>) {
    for (t, mut img) in &mut q {
        let Some(d) = lib.get(&t.0) else { continue };
        if let Some(h) = paints.get(d, PaintTarget::Thumb)
            && img.image != h
        {
            img.image = h;
            img.color = Color::WHITE;
        }
    }
}

fn wheel_scroll(mut wheel: MessageReader<MouseWheel>, mut q: Query<(&RelativeCursorPosition, &ComputedNode, &mut ScrollPosition), With<StudioScroll>>) {
    let dy: f32 = wheel
        .read()
        .map(|e| match e.unit {
            MouseScrollUnit::Line => e.y * 42.0,
            MouseScrollUnit::Pixel => e.y,
        })
        .sum();
    if dy == 0.0 {
        return;
    }
    for (rel, node, mut sp) in &mut q {
        if rel.cursor_over {
            let max = ((node.content_size().y - node.size().y) * node.inverse_scale_factor()).max(0.0);
            sp.y = (sp.y - dy).clamp(0.0, max);
        }
    }
}

// ---- 3D preview -----------------------------------------------------------------------------

#[derive(Component)]
struct PreviewCamera;
#[derive(Component)]
struct PreviewRoot;
#[derive(Component)]
struct Group(DesignKind);
#[derive(Component)]
struct Pedestal;

#[derive(Resource)]
struct PreviewMats {
    sleeve: Handle<StandardMaterial>,
    face: Handle<StandardMaterial>,
    face_image: Handle<Image>,
    deckbox: Handle<StandardMaterial>,
    playmat: Handle<StandardMaterial>,
}

/// Playmat proportions in the preview (the painted texture is 1536×990).
const MAT_W: f32 = 3.9;
const MAT_D: f32 = MAT_W * 990.0 / 1536.0;

fn spawn_preview(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut card_images: ResMut<CardImages>,
) {
    let layer = RenderLayers::layer(DESIGNER_LAYER);
    let here = DespawnOnExit(AppState::Designer);
    commands.spawn((
        PreviewCamera,
        Camera3d::default(),
        Camera { order: 1, clear_color: ClearColorConfig::Custom(Color::srgb(0.028, 0.02, 0.016)), ..default() },
        Projection::Perspective(PerspectiveProjection { fov: 30f32.to_radians(), ..default() }),
        Transform::from_translation(camera_pose(DesignKind::Sleeve).0).looking_at(Vec3::ZERO, Vec3::Y),
        Hdr,
        Tonemapping::TonyMcMapface,
        Bloom { intensity: 0.08, ..Bloom::NATURAL },
        Msaa::Sample4,
        layer.clone(),
        here.clone(),
        Name::new("designer camera"),
    ));
    commands.spawn((
        DirectionalLight { color: Color::srgb(1.0, 0.92, 0.8), illuminance: 3800.0, shadow_maps_enabled: false, ..default() },
        Transform::from_xyz(-3.0, 5.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
        layer.clone(),
        here.clone(),
    ));
    // rim (cool, behind), warm kicker, and a soft fill from the viewer's side so faces turned to
    // the camera never go dark
    for (pos, color, intensity) in [
        (Vec3::new(4.0, 2.5, -3.5), Color::srgb(0.55, 0.7, 1.0), 90_000.0),
        (Vec3::new(-4.0, -0.5, 3.0), Color::srgb(1.0, 0.6, 0.28), 40_000.0),
        (Vec3::new(0.5, 1.5, 6.0), Color::srgb(1.0, 0.95, 0.88), 120_000.0),
    ] {
        commands.spawn((PointLight { color, intensity, range: 20.0, shadow_maps_enabled: false, ..default() }, Transform::from_translation(pos), layer.clone(), here.clone()));
    }

    // backdrop glow and pedestal
    let glow = images.add(radial_glow(96));
    commands.spawn((
        Mesh3d(meshes.add(Rectangle::new(1.0, 1.0))),
        MeshMaterial3d(mats.add(StandardMaterial { base_color: Color::srgb(0.45, 0.2, 0.07), base_color_texture: Some(glow), unlit: true, alpha_mode: AlphaMode::Add, ..default() })),
        Transform::from_xyz(0.0, 0.2, -5.0).with_scale(Vec3::splat(11.0)),
        layer.clone(),
        here.clone(),
        NotShadowCaster,
        NotShadowReceiver,
        Pickable::IGNORE,
    ));
    let gold = mats.add(StandardMaterial { base_color: Color::srgb(0.75, 0.52, 0.2), emissive: LinearRgba::rgb(0.5, 0.28, 0.06), metallic: 1.0, perceptual_roughness: 0.3, ..default() });
    commands
        .spawn((Pedestal, Transform::from_xyz(0.0, -1.14, 0.0), Visibility::Inherited, layer.clone(), here.clone()))
        .with_children(|p| {
            p.spawn((
                Mesh3d(meshes.add(Cylinder::new(1.55, 0.12))),
                MeshMaterial3d(mats.add(StandardMaterial { base_color: Color::srgb(0.09, 0.055, 0.035), metallic: 0.6, perceptual_roughness: 0.35, ..default() })),
                layer.clone(),
                Pickable::IGNORE,
            ));
            p.spawn((
                Mesh3d(meshes.add(Torus { minor_radius: 0.025, major_radius: 1.55 })),
                MeshMaterial3d(gold.clone()),
                Transform::from_xyz(0.0, 0.06, 0.0),
                layer.clone(),
                Pickable::IGNORE,
            ));
        });

    // materials, retextured by `preview_textures`
    let card_mat = |tex: Handle<Image>| StandardMaterial { base_color_texture: Some(tex), perceptual_roughness: 0.6, reflectance: 0.2, ..default() };
    let pick = CURATED.iter().find(|c| c.name == "Lightning Bolt").or(CURATED.first());
    let face_image = pick
        .map(|c| card_images.request_raw(ImageRequest { name: c.name.to_string(), set: Some(c.set.to_string()), back: false, version: ImageVersion::Normal }))
        .unwrap_or_else(|| card_images.card_blank.clone());
    let pm = PreviewMats {
        sleeve: mats.add(card_mat(card_images.card_back.clone())),
        face: mats.add(card_mat(card_images.card_blank.clone())),
        face_image,
        deckbox: mats.add(StandardMaterial { base_color: Color::srgb(0.2, 0.15, 0.1), perceptual_roughness: 0.6, ..default() }),
        playmat: mats.add(StandardMaterial { base_color: Color::srgb(0.1, 0.07, 0.05), perceptual_roughness: 0.62, reflectance: 0.18, ..default() }),
    };
    let card = meshes.add(card_mesh(CARD_W, CARD_H, 0.034));
    let (body, lid) = deckbox_meshes();
    let (body, lid) = (meshes.add(body), meshes.add(lid));

    let root = commands.spawn((PreviewRoot, Transform::default(), Visibility::Inherited, layer.clone(), here.clone(), Name::new("designer preview"))).id();
    // A sleeved card: the face on the front quad, the sleeve on the back.
    let sleeved = |p: &mut ChildSpawnerCommands, tf: Transform| {
        p.spawn((tf, Visibility::Inherited, layer.clone())).with_children(|c| {
            c.spawn((Mesh3d(card.clone()), MeshMaterial3d(pm.face.clone()), Transform::from_xyz(0.0, 0.0, 0.002), layer.clone(), Pickable::IGNORE));
            c.spawn((Mesh3d(card.clone()), MeshMaterial3d(pm.sleeve.clone()), Transform::from_xyz(0.0, 0.0, -0.002).with_rotation(Quat::from_rotation_y(PI)), layer.clone(), Pickable::IGNORE));
        });
    };
    let face_up = Quat::from_rotation_x(-PI / 2.0);
    let back_up = Quat::from_rotation_x(PI / 2.0);
    commands.entity(root).with_children(|r| {
        // sleeves: one card up front and two fanned behind it
        r.spawn((Group(DesignKind::Sleeve), Transform::default(), Visibility::Inherited, layer.clone())).with_children(|g| {
            for (x, z, roll) in [(-0.42, -0.14, 0.2), (0.42, -0.14, -0.2)] {
                sleeved(g, Transform::from_xyz(x, -0.08, z).with_rotation(Quat::from_rotation_z(roll)).with_scale(Vec3::splat(1.95)));
            }
            sleeved(g, Transform::from_xyz(0.0, -0.05, 0.0).with_scale(Vec3::splat(2.1)));
        });
        // the deckbox on its own
        r.spawn((Group(DesignKind::Deckbox), Transform::default(), Visibility::Hidden, layer.clone())).with_children(|g| {
            for m in [body.clone(), lid.clone()] {
                g.spawn((Mesh3d(m), MeshMaterial3d(pm.deckbox.clone()), Transform::from_xyz(0.0, -1.06, 0.0).with_scale(Vec3::splat(2.0)), layer.clone(), Pickable::IGNORE));
            }
        });
        // a playmat dressed like a game in progress
        r.spawn((Group(DesignKind::Playmat), Transform::default(), Visibility::Hidden, layer.clone())).with_children(|g| {
            g.spawn((Mesh3d(meshes.add(Plane3d::default().mesh().size(MAT_W, MAT_D))), MeshMaterial3d(pm.playmat.clone()), layer.clone(), Pickable::IGNORE));
            let s = Vec3::splat(0.62);
            for i in 0..4 {
                sleeved(g, Transform::from_xyz(-1.2 + i as f32 * 0.006, 0.01 + i as f32 * 0.006, 0.55 - i as f32 * 0.004).with_rotation(back_up).with_scale(s));
            }
            for (i, x) in [-0.45, 0.0, 0.45].into_iter().enumerate() {
                let tapped = if i == 2 { Quat::from_rotation_y(-PI / 2.0) } else { Quat::IDENTITY };
                sleeved(g, Transform::from_xyz(x, 0.012, 0.3).with_rotation(tapped * face_up).with_scale(s));
            }
            for x in [-0.3, 0.3] {
                sleeved(g, Transform::from_xyz(x, 0.012, -0.55).with_rotation(back_up).with_scale(s));
            }
            for m in [body.clone(), lid.clone()] {
                g.spawn((
                    Mesh3d(m),
                    MeshMaterial3d(pm.deckbox.clone()),
                    Transform::from_xyz(1.15, 0.0, 0.75).with_rotation(Quat::from_rotation_y(-0.5)).with_scale(Vec3::splat(0.6)),
                    layer.clone(),
                    Pickable::IGNORE,
                ));
            }
        });
    });
    commands.insert_resource(pm);
}

/// (camera position, look-at) per tab.
fn camera_pose(kind: DesignKind) -> (Vec3, Vec3) {
    match kind {
        DesignKind::Sleeve => (Vec3::new(0.0, 0.35, 7.6), Vec3::new(0.0, -0.05, 0.0)),
        DesignKind::Deckbox => (Vec3::new(0.0, 1.2, 7.4), Vec3::new(0.0, -0.05, 0.0)),
        DesignKind::Playmat => (Vec3::new(0.0, 6.0, 5.4), Vec3::new(0.0, -0.35, 0.2)),
    }
}

fn drag_preview(
    mut studio: ResMut<Studio>,
    time: Res<Time>,
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    area: Query<&RelativeCursorPosition, With<PreviewArea>>,
) {
    let over = area.iter().any(|r| r.cursor_over);
    if mouse.just_pressed(MouseButton::Left) && over {
        studio.dragging = true;
    }
    if !mouse.pressed(MouseButton::Left) {
        studio.dragging = false;
    }
    if studio.dragging {
        studio.yaw += motion.delta.x * 0.009;
        studio.pitch = (studio.pitch + motion.delta.y * 0.006).clamp(-0.45, 0.55);
        studio.idle = 0.0;
    } else {
        studio.idle += time.delta_secs();
    }
}

fn animate_preview(
    time: Res<Time>,
    mut studio: ResMut<Studio>,
    mut root: Query<&mut Transform, (With<PreviewRoot>, Without<PreviewCamera>)>,
    mut cam: Query<&mut Transform, (With<PreviewCamera>, Without<PreviewRoot>)>,
    mut groups: Query<(&Group, &mut Visibility), Without<Pedestal>>,
    mut pedestal: Query<&mut Visibility, (With<Pedestal>, Without<Group>)>,
) {
    let dt = time.delta_secs();
    let kind = studio.kind;
    if studio.idle > 2.5 {
        // auto-turn: objects spin slowly, the playmat only sways
        if kind == DesignKind::Playmat {
            let t = time.elapsed_secs();
            studio.yaw += ((t * 0.25).sin() * 0.18 - studio.yaw) * (dt * 1.5).min(1.0);
        } else {
            studio.yaw += dt * 0.32;
        }
        studio.pitch += (0.0 - studio.pitch) * (dt * 1.5).min(1.0);
    }
    if kind == DesignKind::Playmat {
        studio.yaw = studio.yaw.clamp(-0.7, 0.7);
    }
    for mut tf in &mut root {
        let want = Quat::from_rotation_x(studio.pitch) * Quat::from_rotation_y(studio.yaw);
        tf.rotation = tf.rotation.slerp(want, (dt * 10.0).min(1.0));
    }
    let (eye, look) = camera_pose(kind);
    for mut tf in &mut cam {
        let k = (dt * 5.0).min(1.0);
        tf.translation = tf.translation.lerp(eye, k);
        let target = tf.looking_at(look, Vec3::Y).rotation;
        tf.rotation = tf.rotation.slerp(target, k);
    }
    for (g, mut v) in &mut groups {
        let want = if g.0 == kind { Visibility::Inherited } else { Visibility::Hidden };
        if *v != want {
            *v = want;
        }
    }
    for mut v in &mut pedestal {
        let want = if kind == DesignKind::Playmat { Visibility::Hidden } else { Visibility::Inherited };
        if *v != want {
            *v = want;
        }
    }
}

/// Keep the preview materials on the newest paint of each tab's selected design.
fn preview_textures(
    studio: Res<Studio>,
    lib: Res<DesignLibrary>,
    mut paints: ResMut<Paints>,
    pm: Option<Res<PreviewMats>>,
    images: Res<Assets<Image>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut applied: Local<HashMap<DesignKind, Handle<Image>>>,
    mut face_done: Local<Option<AssetId<Image>>>,
) {
    let Some(pm) = pm else { return };
    if pm.is_added() {
        applied.clear();
        *face_done = None;
    }
    for (kind, mat) in [(DesignKind::Sleeve, &pm.sleeve), (DesignKind::Deckbox, &pm.deckbox), (DesignKind::Playmat, &pm.playmat)] {
        let d = studio.design(&lib, kind);
        let Some(tex) = paints.get(d, PaintTarget::Full) else { continue };
        if applied.get(&kind) == Some(&tex) {
            continue;
        }
        if let Some(mut m) = mats.get_mut(mat) {
            m.base_color = Color::WHITE;
            m.base_color_texture = Some(tex.clone());
            apply_finish(&mut m, d.finish);
        }
        applied.insert(kind, tex);
    }
    if *face_done != Some(pm.face_image.id()) && images.contains(pm.face_image.id()) {
        if let Some(mut m) = mats.get_mut(&pm.face) {
            m.base_color_texture = Some(pm.face_image.clone());
        }
        *face_done = Some(pm.face_image.id());
    }
}

/// Soft white radial falloff (RGB white, alpha in the colour channel for additive blending).
fn radial_glow(n: usize) -> Image {
    let mut data = vec![0u8; n * n * 4];
    for y in 0..n {
        for x in 0..n {
            let (u, v) = ((x as f32 + 0.5) / n as f32 * 2.0 - 1.0, (y as f32 + 0.5) / n as f32 * 2.0 - 1.0);
            let a = (1.0 - (u * u + v * v).sqrt()).clamp(0.0, 1.0).powf(2.2);
            let b = (a * 255.0) as u8;
            data[(y * n + x) * 4..(y * n + x) * 4 + 4].copy_from_slice(&[b, b, b, 255]);
        }
    }
    Image::new(Extent3d { width: n as u32, height: n as u32, depth_or_array_layers: 1 }, TextureDimension::D2, data, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cosmetics::design::presets;

    fn studio_on(kind: DesignKind, id: &str) -> Studio {
        let mut s = Studio { kind, rebuild_list: false, rebuild_editor: false, ..Studio::default() };
        s.selected.insert(kind, id.into());
        s
    }

    #[test]
    fn editing_a_preset_forks_it_and_leaves_the_preset_alone() {
        let mut lib = DesignLibrary { designs: presets() };
        let mut studio = studio_on(DesignKind::Sleeve, "celestial");
        edit(&mut lib, &mut studio, false, |d| d.pattern = Pattern::Hexes);
        assert_eq!(lib.get("celestial").unwrap().pattern, Pattern::Nebula);
        let fork = lib.get(studio.current()).unwrap();
        assert!(!fork.builtin && fork.pattern == Pattern::Hexes && fork.kind == DesignKind::Sleeve);
        assert!(studio.rebuild_list && studio.rebuild_editor);
        // further edits change the fork in place
        let (id, n) = (fork.id.clone(), lib.designs.len());
        edit(&mut lib, &mut studio, true, |d| d.name = "Mine".into());
        assert_eq!((studio.current(), lib.designs.len()), (id.as_str(), n));
        assert_eq!(lib.get(&id).unwrap().name, "Mine");
    }

    #[test]
    fn forking_shipped_art_paints_it_instead() {
        let mut lib = DesignLibrary { designs: presets() };
        let mut studio = studio_on(DesignKind::Playmat, "sanctum");
        edit(&mut lib, &mut studio, true, |d| d.name = "My Sanctum".into());
        let fork = lib.get(studio.current()).unwrap();
        assert_eq!(fork.image, None);
        assert!(!studio.rebuild_editor, "typing in the name field must keep its focus");
    }

    #[test]
    fn deleting_a_design_clears_every_reference() {
        let mut s = Settings { sleeve: "x".into(), theme: "x".into(), ..Settings::default() };
        s.deck_looks.insert("deck".into(), DeckLook { sleeve: Some("x".into()), deckbox: Some("keep".into()), ..Default::default() });
        forget_design(&mut s, "x");
        assert_eq!(s.sleeve, Settings::default().sleeve);
        assert_eq!(s.theme, Settings::default().theme);
        assert_eq!(s.deck_looks["deck"], DeckLook { sleeve: None, deckbox: Some("keep".into()), ..Default::default() });
    }
}
