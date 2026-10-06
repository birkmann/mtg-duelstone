//! Cube screen layout around the views: the top bar, the toolbar (view tabs, filter, pack layout),
//! the cube library drawer, the ADD CARDS panel and the card inspector that replaces it, the PASTE A
//! LIST dialog, the floating card preview and the toast.
//!
//! Why the library is a drawer over the views instead of a column: the table and stacks views want
//! every pixel of width for eight colour columns, and the library is only needed to switch cubes.

use bevy::prelude::*;
use bevy::ui::{FocusPolicy, RelativeCursorPosition};
use bevy::window::PrimaryWindow;
use mtg_client::deckstats::COLORS;
use mtg_client::{ImageRequest, ImageVersion};

use super::super::widgets::{Field, WheelScroll, cost_pips, text_field};
use super::model::{self, ColorGroup};
use super::views::{CardArt, CardRef, header_color};
use super::{CubeAction, CubeEditor, Region, RegionKind, TileSize, View};
use crate::images::CardImages;
use crate::shared::{AppState, ArenaFonts};
use crate::style::*;

#[derive(Component)]
pub(super) struct Drawer;
#[derive(Component)]
pub(super) struct AddPanel;
#[derive(Component)]
pub(super) struct InspectorPanel;
#[derive(Component)]
pub(super) struct ImportOverlay;
#[derive(Component)]
pub(super) struct TileRow;
#[derive(Component)]
pub(super) struct ClearFilterButton;
#[derive(Component)]
pub(super) struct Preview;
#[derive(Component)]
pub(super) struct Toast;
/// A filter chip; its highlight mirrors the state its action toggles.
#[derive(Component)]
pub(super) struct Chip;
#[derive(Component)]
pub(super) enum Readout {
    Info,
    Showing,
    Packs,
    PackSize,
    SearchStatus,
}

const SIDE_W: f32 = 340.0;
const DRAWER_W: f32 = 330.0;
const PREVIEW: Vec2 = Vec2::new(300.0, 419.0);

fn scroll_region(kind: RegionKind, inner: Node) -> impl Bundle {
    (
        Region::new(kind),
        ScrollPosition::default(),
        RelativeCursorPosition::default(),
        WheelScroll,
        Node { width: Val::Percent(100.0), flex_grow: 1.0, min_height: Val::Px(0.0), flex_direction: FlexDirection::Column, overflow: Overflow::scroll_y(), ..inner },
    )
}

fn chip_button(p: &mut ChildSpawnerCommands, fonts: &ArenaFonts, label: &str, act: CubeAction) -> Entity {
    p.spawn((
        Button,
        Chip,
        act,
        Node {
            padding: UiRect::axes(Val::Px(9.0), Val::Px(4.0)),
            border: UiRect::all(Val::Px(1.5)),
            border_radius: BorderRadius::all(Val::Px(99.0)),
            align_items: AlignItems::Center,
            column_gap: Val::Px(4.0),
            flex_shrink: 0.0,
            ..default()
        },
        BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.05)),
        BorderColor::all(GOLD_DIM),
    ))
    .with_children(|c| {
        if !label.is_empty() {
            c.spawn((ui_text(&fonts.body, label, 13.0, TEXT), Pickable::IGNORE));
        }
    })
    .id()
}

fn pip_chip(p: &mut ChildSpawnerCommands, fonts: &ArenaFonts, assets: &AssetServer, mana: char, act: CubeAction) {
    let e = chip_button(p, fonts, "", act);
    p.commands().entity(e).with_child((mana_pip(assets, mana, 17.0), Pickable::IGNORE));
}

fn stepper(p: &mut ChildSpawnerCommands, fonts: &ArenaFonts, label: &str, value: Readout, dec: CubeAction, inc: CubeAction) {
    p.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(4.0), ..default() }).with_children(|r| {
        r.spawn(ui_text(&fonts.body, label, 13.0, TEXT_DIM));
        let b = spawn_button(r, fonts, ButtonKind::Secondary, "−", Vec2::new(30.0, 28.0));
        r.commands().entity(b).insert(dec);
        r.spawn((value, Node { width: Val::Px(26.0), justify_content: JustifyContent::Center, ..default() }, ui_text(&fonts.numbers, "", 16.0, PARCHMENT)));
        let b = spawn_button(r, fonts, ButtonKind::Secondary, "+", Vec2::new(30.0, 28.0));
        r.commands().entity(b).insert(inc);
    });
}

pub(super) fn spawn_screen(mut commands: Commands, fonts: Res<ArenaFonts>, assets: Res<AssetServer>, ed: Res<CubeEditor>) {
    let here = DespawnOnExit(AppState::CubeDesigner);
    commands
        .spawn((Name::new("CubeDesigner"), here.clone(), Node { flex_direction: FlexDirection::Column, padding: UiRect::all(Val::Px(18.0)), row_gap: Val::Px(12.0), ..fullscreen_root() }))
        .with_children(|root| {
            // ---- top bar
            root.spawn(Node { width: Val::Percent(100.0), align_items: AlignItems::Center, column_gap: Val::Px(12.0), ..default() }).with_children(|bar| {
                let b = spawn_button(bar, &fonts, ButtonKind::Secondary, "BACK", Vec2::new(110.0, 44.0));
                bar.commands().entity(b).insert(CubeAction::Back);
                let b = spawn_button(bar, &fonts, ButtonKind::Secondary, "LIBRARY", Vec2::new(140.0, 44.0));
                bar.commands().entity(b).insert(CubeAction::Drawer);
                bar.spawn(text_field(&fonts, Field::CubeName, &ed.cube.name, Val::Px(380.0), 1.0, false));
                bar.spawn((Readout::Info, Node { flex_grow: 1.0, flex_basis: Val::Px(0.0), ..default() }, ui_text(&fonts.body, "", 15.0, TEXT_DIM)));
                let b = spawn_button(bar, &fonts, ButtonKind::Secondary, "SAVE", Vec2::new(110.0, 44.0));
                bar.commands().entity(b).insert(CubeAction::Save);
                let b = spawn_button(bar, &fonts, ButtonKind::Danger, "DELETE", Vec2::new(120.0, 44.0));
                bar.commands().entity(b).insert(CubeAction::Delete);
                let b = spawn_button(bar, &fonts, ButtonKind::Primary, "DRAFT THIS CUBE", Vec2::new(240.0, 48.0));
                bar.commands().entity(b).insert(CubeAction::Draft);
            });
            root.spawn(Node { width: Val::Percent(100.0), flex_grow: 1.0, min_height: Val::Px(0.0), column_gap: Val::Px(14.0), ..default() }).with_children(|mid| {
                // ---- views (+ the library drawer over them)
                mid.spawn(Node { flex_grow: 1.0, min_width: Val::Px(0.0), ..default() }).with_children(|area| {
                    area.spawn(ornate_panel(Node { width: Val::Percent(100.0), padding: UiRect::all(Val::Px(14.0)), flex_direction: FlexDirection::Column, row_gap: Val::Px(8.0), ..default() }))
                        .with_children(|p| {
                            toolbar(p, &fonts, &assets);
                            p.spawn(Node { width: Val::Percent(100.0), align_items: AlignItems::Center, column_gap: Val::Px(14.0), ..default() }).with_children(|r| {
                                r.spawn((Readout::Showing, Node { flex_grow: 1.0, flex_basis: Val::Px(0.0), ..default() }, ui_text(&fonts.body, "", 13.0, TEXT_DIM)));
                                let b = spawn_button(r, &fonts, ButtonKind::Ghost, "CLEAR FILTER", Vec2::new(130.0, 28.0));
                                r.commands().entity(b).insert((CubeAction::ClearFilter, ClearFilterButton));
                                let b = spawn_button(r, &fonts, ButtonKind::Secondary, "UNDO", Vec2::new(84.0, 28.0));
                                r.commands().entity(b).insert(CubeAction::Undo);
                                stepper(r, &fonts, "Packs", Readout::Packs, CubeAction::Packs(-1), CubeAction::Packs(1));
                                stepper(r, &fonts, "Pack size", Readout::PackSize, CubeAction::PackSize(-1), CubeAction::PackSize(1));
                            });
                            p.spawn(divider());
                            p.spawn(scroll_region(RegionKind::Body, Node { padding: UiRect::right(Val::Px(6.0)), ..default() }));
                        });
                    area.spawn((
                        Drawer,
                        FocusPolicy::Block,
                        ZIndex(10),
                        // the panel gradient is slightly translucent; the views must not show through
                        BackgroundColor(BROWN_DEEP),
                        ornate_panel(Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(0.0),
                            top: Val::Px(0.0),
                            bottom: Val::Px(0.0),
                            width: Val::Px(DRAWER_W),
                            padding: UiRect::all(Val::Px(12.0)),
                            flex_direction: FlexDirection::Column,
                            row_gap: Val::Px(8.0),
                            display: Display::None,
                            ..default()
                        }),
                    ))
                    .with_children(|d| {
                        d.spawn(Node { width: Val::Percent(100.0), align_items: AlignItems::Center, ..default() }).with_children(|r| {
                            r.spawn((Node { flex_grow: 1.0, ..default() }, heading(&fonts, "CUBE LIBRARY", 20.0)));
                            let b = spawn_button(r, &fonts, ButtonKind::Ghost, "CLOSE", Vec2::new(84.0, 32.0));
                            r.commands().entity(b).insert(CubeAction::Drawer);
                        });
                        d.spawn(text_field(&fonts, Field::CubeLibraryFilter, &ed.library_filter, Val::Percent(100.0), 1.0, false));
                        let b = spawn_button(d, &fonts, ButtonKind::Secondary, "NEW CUBE", Vec2::new(DRAWER_W - 28.0, 40.0));
                        d.commands().entity(b).insert(CubeAction::New);
                        d.spawn(scroll_region(RegionKind::Library, Node { row_gap: Val::Px(2.0), ..default() }));
                    });
                });
                // ---- add cards / inspector
                mid.spawn(ornate_panel(Node { width: Val::Px(SIDE_W), flex_shrink: 0.0, padding: UiRect::all(Val::Px(12.0)), flex_direction: FlexDirection::Column, ..default() })).with_children(|p| {
                    p.spawn((AddPanel, Node { width: Val::Percent(100.0), flex_grow: 1.0, min_height: Val::Px(0.0), flex_direction: FlexDirection::Column, row_gap: Val::Px(8.0), ..default() }))
                        .with_children(|a| add_panel(a, &fonts, &assets, &ed));
                    p.spawn((InspectorPanel, scroll_region(RegionKind::Inspector, Node { row_gap: Val::Px(8.0), display: Display::None, ..default() })));
                });
            });
        });
    import_overlay(&mut commands, &fonts);
    commands.spawn((
        Preview,
        here.clone(),
        GlobalZIndex(90),
        Node { position_type: PositionType::Absolute, width: Val::Px(PREVIEW.x), height: Val::Px(PREVIEW.y), display: Display::None, border_radius: BorderRadius::all(Val::Px(14.0)), ..default() },
        ImageNode::default(),
        BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.8), Val::Px(0.0), Val::Px(10.0), Val::Px(0.0), Val::Px(28.0)),
        Pickable::IGNORE,
    ));
    commands
        .spawn((here, GlobalZIndex(95), Node { position_type: PositionType::Absolute, bottom: Val::Px(26.0), width: Val::Percent(100.0), justify_content: JustifyContent::Center, ..default() }, Pickable::IGNORE))
        .with_children(|r| {
            r.spawn((Toast, chip(Node { padding: UiRect::axes(Val::Px(22.0), Val::Px(10.0)), display: Display::None, ..default() }), Pickable::IGNORE))
                .with_child((ui_text(&fonts.body, "", 17.0, TEXT), Pickable::IGNORE));
        });
}

fn toolbar(p: &mut ChildSpawnerCommands, fonts: &ArenaFonts, assets: &AssetServer) {
    p.spawn(Node { width: Val::Percent(100.0), flex_wrap: FlexWrap::Wrap, align_items: AlignItems::Center, column_gap: Val::Px(6.0), row_gap: Val::Px(6.0), ..default() }).with_children(|r| {
        for v in View::ALL {
            let b = spawn_button(r, fonts, ButtonKind::Secondary, v.label(), Vec2::new(96.0, 36.0));
            r.commands().entity(b).insert(CubeAction::View(v));
        }
        r.spawn((Node { margin: UiRect::left(Val::Px(10.0)), ..default() }, ui_text(&fonts.body, "Filter", 13.0, TEXT_DIM)));
        r.spawn(text_field(fonts, Field::CubeFilter, "", Val::Px(170.0), 1.0, false));
        for g in ColorGroup::ALL {
            match g.mana() {
                Some(m) => pip_chip(r, fonts, assets, m, CubeAction::ToggleGroup(g.index())),
                None => {
                    chip_button(r, fonts, if g == ColorGroup::Multi { "Multi" } else { "Lands" }, CubeAction::ToggleGroup(g.index()));
                }
            }
        }
        r.spawn((TileRow, Node { align_items: AlignItems::Center, column_gap: Val::Px(4.0), margin: UiRect::left(Val::Px(8.0)), ..default() })).with_children(|t| {
            t.spawn(ui_text(&fonts.body, "Size", 13.0, TEXT_DIM));
            for s in TileSize::ALL {
                let b = spawn_button(t, fonts, ButtonKind::Secondary, s.label(), Vec2::new(34.0, 30.0));
                t.commands().entity(b).insert(CubeAction::Tile(s));
            }
        });
    });
}

fn add_panel(a: &mut ChildSpawnerCommands, fonts: &ArenaFonts, assets: &AssetServer, ed: &CubeEditor) {
    a.spawn(heading(fonts, "ADD CARDS", 20.0));
    a.spawn(ui_text(&fonts.body, "Card name (empty = most played)", 12.0, TEXT_DIM));
    a.spawn(text_field(fonts, Field::CubeSearch, &ed.search.q, Val::Percent(100.0), 1.0, false));
    a.spawn(Node { flex_wrap: FlexWrap::Wrap, column_gap: Val::Px(4.0), row_gap: Val::Px(4.0), align_items: AlignItems::Center, ..default() }).with_children(|r| {
        for (i, c) in COLORS.iter().enumerate() {
            pip_chip(r, fonts, assets, *c, CubeAction::SearchColor(i));
        }
        r.spawn((Node { margin: UiRect::left(Val::Px(4.0)), ..default() }, ui_text(&fonts.body, "fits these colours", 11.0, TEXT_DIM)));
    });
    a.spawn(Node { flex_wrap: FlexWrap::Wrap, column_gap: Val::Px(4.0), row_gap: Val::Px(4.0), ..default() }).with_children(|r| {
        for k in ["Creature", "Instant", "Sorcery", "Artifact", "Enchantment", "Planeswalker", "Land"] {
            chip_button(r, fonts, k, CubeAction::SearchKind(Some(k)));
        }
    });
    a.spawn(Node { flex_wrap: FlexWrap::Wrap, column_gap: Val::Px(4.0), row_gap: Val::Px(4.0), align_items: AlignItems::Center, ..default() }).with_children(|r| {
        r.spawn(ui_text(&fonts.body, "MV", 13.0, TEXT_DIM));
        for v in 0..=7 {
            chip_button(r, fonts, &if v == 7 { "7+".to_string() } else { v.to_string() }, CubeAction::SearchCmc(Some(v)));
        }
    });
    a.spawn((Readout::SearchStatus, ui_text(&fonts.body, "", 12.0, TEXT_DIM)));
    a.spawn(scroll_region(RegionKind::Results, Node { row_gap: Val::Px(2.0), ..default() }));
    let b = spawn_button(a, fonts, ButtonKind::Secondary, "PASTE A LIST…", Vec2::new(SIDE_W - 24.0, 42.0));
    a.commands().entity(b).insert(CubeAction::OpenImport);
}

fn import_overlay(commands: &mut Commands, fonts: &ArenaFonts) {
    commands
        .spawn((
            ImportOverlay,
            DespawnOnExit(AppState::CubeDesigner),
            GlobalZIndex(80),
            FocusPolicy::Block,
            BackgroundColor(SCRIM),
            Node { justify_content: JustifyContent::Center, align_items: AlignItems::Center, display: Display::None, ..fullscreen_root() },
        ))
        .with_children(|o| {
            o.spawn((
                BackgroundColor(BROWN_DEEP),
                ornate_panel(Node { width: Val::Px(700.0), max_height: Val::Percent(88.0), padding: UiRect::all(Val::Px(20.0)), flex_direction: FlexDirection::Column, row_gap: Val::Px(10.0), ..default() }),
            ))
                .with_children(|p| {
                    p.spawn(heading(fonts, "PASTE A LIST", 26.0));
                    p.spawn(ui_text(&fonts.body, "One card per line: \"1 Lightning Bolt\", \"Lightning Bolt (LEA)\", or a CubeCobra / MTGO export. Cards already in the cube are skipped.", 14.0, TEXT_DIM));
                    p.spawn(text_field(fonts, Field::CubeImportText, "", Val::Percent(100.0), 10.0, true));
                    p.spawn(Node { column_gap: Val::Px(10.0), ..default() }).with_children(|r| {
                        let b = spawn_button(r, fonts, ButtonKind::Primary, "IMPORT", Vec2::new(180.0, 46.0));
                        r.commands().entity(b).insert(CubeAction::Import);
                        let b = spawn_button(r, fonts, ButtonKind::Secondary, "CLOSE", Vec2::new(140.0, 46.0));
                        r.commands().entity(b).insert(CubeAction::CloseImport);
                    });
                    p.spawn(scroll_region(RegionKind::ImportResult, Node { row_gap: Val::Px(6.0), max_height: Val::Px(260.0), ..default() }));
                });
        });
}

// ---- rebuilt regions --------------------------------------------------------------------------

pub(super) fn sync_regions(
    mut commands: Commands,
    ed: Res<CubeEditor>,
    fonts: Res<ArenaFonts>,
    assets: Res<AssetServer>,
    mut images: ResMut<CardImages>,
    mut regions: Query<(Entity, &mut Region, &mut ScrollPosition)>,
) {
    for (e, mut region, mut scroll) in &mut regions {
        let want = ed.rev_of(region.kind);
        if region.kind == RegionKind::Body || region.built == Some(want) {
            continue;
        }
        region.built = Some(want);
        commands.entity(e).despawn_related::<Children>();
        if matches!(region.kind, RegionKind::Results | RegionKind::Inspector) {
            scroll.y = 0.0;
        }
        let mut ec = commands.entity(e);
        match region.kind {
            RegionKind::Library => {
                ec.with_children(|p| library(p, &fonts, &ed));
            }
            RegionKind::Results => {
                ec.with_children(|p| results(p, &fonts, &assets, &ed));
            }
            RegionKind::Inspector => {
                ec.with_children(|p| inspector(p, &fonts, &assets, &mut images, &ed));
            }
            RegionKind::ImportResult => {
                ec.with_children(|p| import_result(p, &fonts, &ed));
            }
            RegionKind::Body => {}
        }
    }
}

fn library(p: &mut ChildSpawnerCommands, fonts: &ArenaFonts, ed: &CubeEditor) {
    if ed.list.is_empty() {
        p.spawn(ui_text(&fonts.body, "Loading cubes…", 14.0, TEXT_DIM));
        return;
    }
    let f = ed.library_filter.trim().to_lowercase();
    let current = ed.current_ref();
    for (title, user) in [("MY CUBES", true), ("BUNDLED", false)] {
        let rows: Vec<_> = ed.list.iter().filter(|c| c.user == user && (f.is_empty() || c.name.to_lowercase().contains(&f))).collect();
        p.spawn((Node { margin: UiRect::new(Val::Px(0.0), Val::Px(0.0), Val::Px(8.0), Val::Px(2.0)), flex_shrink: 0.0, ..default() }, ui_text(&fonts.title, format!("{title} ({})", rows.len()), 14.0, GOLD)));
        if rows.is_empty() && user && f.is_empty() {
            p.spawn(ui_text(&fonts.body, "Cubes you save show up here.", 13.0, TEXT_DIM));
        }
        for c in rows {
            let on = current.as_deref() == Some(c.cube_ref.as_str());
            p.spawn((
                Button,
                CubeAction::Open(c.cube_ref.clone()),
                Node { padding: UiRect::axes(Val::Px(8.0), Val::Px(5.0)), column_gap: Val::Px(6.0), border_radius: BorderRadius::all(Val::Px(6.0)), flex_shrink: 0.0, ..default() },
                BackgroundColor(if on { Color::srgba(0.95, 0.46, 0.13, 0.25) } else { Color::NONE }),
            ))
            .with_children(|r| {
                r.spawn((Node { flex_grow: 1.0, flex_basis: Val::Px(0.0), ..default() }, ui_text(&fonts.body, c.name.clone(), 14.0, if user { GOLD_BRIGHT } else { TEXT }), Pickable::IGNORE));
                r.spawn((ui_text(&fonts.numbers, c.size.to_string(), 13.0, TEXT_DIM), Pickable::IGNORE));
            });
        }
    }
}

fn results(p: &mut ChildSpawnerCommands, fonts: &ArenaFonts, assets: &AssetServer, ed: &CubeEditor) {
    for c in &ed.search.results {
        let have = ed.cube.cards.iter().any(|x| x.name == c.name);
        p.spawn((
            Button,
            CubeAction::Add(Box::new(c.clone())),
            CardRef { name: c.name.clone(), set: c.set.clone(), in_cube: false },
            Node {
                width: Val::Percent(100.0),
                padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                column_gap: Val::Px(6.0),
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(5.0)),
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(if have { Color::srgba(0.35, 0.92, 0.45, 0.12) } else { Color::srgba(1.0, 1.0, 1.0, 0.03) }),
            BorderColor::all(Color::NONE),
        ))
        .with_children(|r| {
            r.spawn((Node { flex_direction: FlexDirection::Column, flex_grow: 1.0, flex_basis: Val::Px(0.0), min_width: Val::Px(0.0), ..default() }, Pickable::IGNORE)).with_children(|t| {
                t.spawn((ui_text(&fonts.body, c.name.clone(), 13.0, TEXT), Pickable::IGNORE));
                t.spawn((ui_text(&fonts.body, if have { format!("IN CUBE · {}", c.type_line) } else { c.type_line.clone() }, 11.0, if have { GREEN_HEAL } else { TEXT_DIM }), Pickable::IGNORE));
            });
            if let Some(cost) = &c.mana_cost {
                cost_pips(r, fonts, assets, cost, 13.0);
            }
        });
    }
}

fn inspector(p: &mut ChildSpawnerCommands, fonts: &ArenaFonts, assets: &AssetServer, images: &mut CardImages, ed: &CubeEditor) {
    let Some(c) = ed.selected.as_ref().and_then(|n| ed.cube.cards.iter().find(|c| &c.name == n)) else { return };
    let b = spawn_button(p, fonts, ButtonKind::Ghost, "BACK TO SEARCH", Vec2::new(170.0, 32.0));
    p.commands().entity(b).insert(CubeAction::Deselect);
    p.spawn((
        Node { width: Val::Px(260.0), height: Val::Px(260.0 * 88.0 / 63.0), align_self: AlignSelf::Center, border_radius: BorderRadius::all(Val::Px(12.0)), overflow: Overflow::clip(), flex_shrink: 0.0, ..default() },
        BackgroundColor(Color::srgb(0.13, 0.09, 0.06)),
    ))
    .with_child((CardArt::now(c, images), Node { width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() }, ImageNode { color: Color::NONE, ..default() }));
    p.spawn(Node { width: Val::Percent(100.0), align_items: AlignItems::Center, column_gap: Val::Px(8.0), flex_shrink: 0.0, ..default() }).with_children(|r| {
        r.spawn((Node { flex_grow: 1.0, flex_basis: Val::Px(0.0), ..default() }, ui_text(&fonts.title, c.name.clone(), 19.0, PARCHMENT)));
        if let Some(cost) = &c.mana_cost {
            cost_pips(r, fonts, assets, cost, 16.0);
        }
    });
    let g = ColorGroup::of(c);
    p.spawn(ui_text(&fonts.body, c.type_line.clone(), 14.0, header_color(g)));
    let stats = match (&c.power, &c.toughness, &c.loyalty) {
        (Some(pw), Some(t), _) => Some(format!("{pw}/{t}")),
        (_, _, Some(l)) => Some(format!("Loyalty {l}")),
        _ => None,
    };
    if let Some(s) = stats {
        p.spawn(ui_text(&fonts.numbers, s, 16.0, GOLD_BRIGHT));
    }
    if let Some(t) = c.oracle_text.as_ref().filter(|t| !t.is_empty()) {
        p.spawn((Node { flex_shrink: 0.0, ..default() }, ui_text(&fonts.body, t.replace("\\n", "\n"), 14.0, TEXT)));
    }
    let printing = [c.set.clone(), c.rarity.as_ref().map(|r| r.to_lowercase())].into_iter().flatten().collect::<Vec<_>>().join(" · ");
    if !printing.is_empty() {
        p.spawn(ui_text(&fonts.body, printing, 12.0, TEXT_DIM));
    }
    p.spawn(divider());
    p.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(8.0), flex_shrink: 0.0, ..default() }).with_children(|r| {
        r.spawn(ui_text(&fonts.body, "Copies", 14.0, TEXT));
        let b = spawn_button(r, fonts, ButtonKind::Secondary, "−", Vec2::new(36.0, 32.0));
        r.commands().entity(b).insert(CubeAction::Copies(c.name.clone(), -1));
        r.spawn((Node { width: Val::Px(30.0), justify_content: JustifyContent::Center, ..default() }, ui_text(&fonts.numbers, model::copies(c).to_string(), 18.0, PARCHMENT)));
        let b = spawn_button(r, fonts, ButtonKind::Secondary, "+", Vec2::new(36.0, 32.0));
        r.commands().entity(b).insert(CubeAction::Copies(c.name.clone(), 1));
    });
    let b = spawn_button(p, fonts, ButtonKind::Danger, "REMOVE FROM CUBE", Vec2::new(SIDE_W - 30.0, 44.0));
    p.commands().entity(b).insert(CubeAction::Remove(c.name.clone()));
    p.spawn(ui_text(&fonts.body, "Right-click any card or press Delete to remove it; Ctrl+Z undoes. Esc goes back.", 12.0, TEXT_DIM));
}

fn import_result(p: &mut ChildSpawnerCommands, fonts: &ArenaFonts, ed: &CubeEditor) {
    let im = &ed.import;
    if im.busy {
        p.spawn(ui_text(&fonts.body, "Importing…", 14.0, TEXT_DIM));
        return;
    }
    if let Some(n) = im.added {
        p.spawn(ui_text(&fonts.body, format!("Added {n} cards."), 14.0, GREEN_HEAL));
    }
    if im.unresolved.is_empty() {
        return;
    }
    p.spawn(ui_text(&fonts.title, format!("NOT RECOGNISED ({}) — click a suggestion to add it", im.unresolved.len()), 14.0, ORANGE_HI));
    for (line, sug) in &im.unresolved {
        p.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(6.0), flex_wrap: FlexWrap::Wrap, row_gap: Val::Px(4.0), flex_shrink: 0.0, ..default() }).with_children(|r| {
            r.spawn((Node { width: Val::Px(220.0), ..default() }, ui_text(&fonts.body, line.clone(), 13.0, TEXT)));
            if sug.is_empty() {
                r.spawn(ui_text(&fonts.body, "no close match", 12.0, TEXT_DIM));
            }
            for s in sug {
                chip_button(r, fonts, s, CubeAction::Suggest(line.clone(), s.clone()));
            }
        });
    }
}

// ---- per-frame --------------------------------------------------------------------------------

pub(super) fn chips_and_buttons(
    ed: Res<CubeEditor>,
    mut buttons: Query<(&CubeAction, &mut ArenaButton)>,
    mut chips: Query<(&CubeAction, &Interaction, &mut BackgroundColor, &mut BorderColor), With<Chip>>,
) {
    let user_cube = ed.saved_ref.as_deref().is_some_and(|r| r.starts_with("user:"));
    for (act, mut b) in &mut buttons {
        let (kind, disabled) = match act {
            CubeAction::View(v) => (if *v == ed.view { ButtonKind::Primary } else { ButtonKind::Secondary }, false),
            CubeAction::Tile(t) => (if *t == ed.tile { ButtonKind::Primary } else { ButtonKind::Secondary }, false),
            CubeAction::Save => (if ed.dirty { ButtonKind::Primary } else { ButtonKind::Secondary }, false),
            CubeAction::Undo => (ButtonKind::Secondary, ed.undo.is_empty()),
            CubeAction::Delete => (ButtonKind::Danger, !user_cube),
            CubeAction::Import => (ButtonKind::Primary, ed.import.busy),
            _ => continue,
        };
        if b.kind != kind {
            b.kind = kind;
        }
        if b.disabled != disabled {
            b.disabled = disabled;
        }
    }
    for (act, i, mut bg, mut bc) in &mut chips {
        let hov = matches!(i, Interaction::Hovered | Interaction::Pressed);
        let (want_bg, want_bc) = match act {
            // colour columns are on by default: only a hidden one looks different
            CubeAction::ToggleGroup(g) if ed.filter.hidden[*g] => (Color::srgba(0.0, 0.0, 0.0, 0.5), Color::srgba(1.0, 1.0, 1.0, 0.1)),
            CubeAction::ToggleGroup(_) => (Color::srgba(1.0, 1.0, 1.0, if hov { 0.14 } else { 0.06 }), GOLD_DIM),
            _ => {
                let on = match act {
                    CubeAction::SearchColor(c) => ed.search.colors[*c],
                    CubeAction::SearchKind(k) => ed.search.kind == *k,
                    CubeAction::SearchCmc(v) => ed.search.cmc == *v,
                    _ => false,
                };
                if on {
                    (Color::srgba(0.95, 0.46, 0.13, 0.32), ORANGE_HI)
                } else {
                    (Color::srgba(1.0, 1.0, 1.0, if hov { 0.12 } else { 0.05 }), GOLD_DIM)
                }
            }
        };
        if bg.0 != want_bg {
            bg.0 = want_bg;
        }
        if bc.top != want_bc {
            *bc = BorderColor::all(want_bc);
        }
    }
}

pub(super) fn texts(ed: Res<CubeEditor>, mut q: Query<(&Readout, &mut Text, &mut TextColor)>) {
    let total = model::total(&ed.cube.cards);
    let per = (ed.cube.packs * ed.cube.pack_size).max(1);
    for (l, mut t, mut col) in &mut q {
        let (s, c) = match l {
            Readout::Info => {
                let src = if ed.bundled { " · bundled" } else { "" };
                let unsaved = if ed.dirty { " · unsaved changes" } else { "" };
                (format!("{total} cards · drafts up to {} players{src}{unsaved}", (total / per).min(8)), if ed.dirty { ORANGE_HI } else { TEXT_DIM })
            }
            Readout::Showing => {
                let shown: i32 = ed.cube.cards.iter().filter(|c| ed.filter.matches(c)).map(model::copies).sum();
                let hint = "Click a card to inspect it · right-click removes · Ctrl+Z undoes";
                if ed.filter.active() { (format!("Showing {shown} of {total}  ·  {hint}"), TEXT_DIM) } else { (hint.to_string(), TEXT_DIM) }
            }
            Readout::Packs => (ed.cube.packs.to_string(), PARCHMENT),
            Readout::PackSize => (ed.cube.pack_size.to_string(), PARCHMENT),
            Readout::SearchStatus => match (&ed.search.error, ed.search.busy) {
                (Some(e), _) => (format!("Search failed: {e}"), RED_DAMAGE),
                (None, true) => ("Searching…".into(), TEXT_DIM),
                _ if ed.search.results.is_empty() => ("No cards match.".into(), TEXT_DIM),
                _ => (format!("{} cards · click to add · hover to preview", ed.search.results.len()), TEXT_DIM),
            },
        };
        if t.0 != s {
            t.0 = s;
        }
        if col.0 != c {
            col.0 = c;
        }
    }
}

/// Which panels and overlays show, and which scroll area takes the wheel while the drawer is open.
pub(super) fn overlays(
    mut commands: Commands,
    ed: Res<CubeEditor>,
    mut nodes: Query<(&mut Node, Has<Drawer>, Has<ImportOverlay>, Has<AddPanel>, Has<InspectorPanel>, Has<TileRow>, Has<ClearFilterButton>)>,
    body: Query<(Entity, &Region, Has<WheelScroll>)>,
) {
    let show = |on: bool| if on { Display::Flex } else { Display::None };
    for (mut n, drawer, import, add, inspect, tile, clear) in &mut nodes {
        let want = if drawer {
            show(ed.drawer)
        } else if import {
            show(ed.import.open)
        } else if add {
            show(ed.selected.is_none())
        } else if inspect {
            show(ed.selected.is_some())
        } else if tile {
            show(ed.view == View::Spoiler)
        } else if clear {
            show(ed.filter.active())
        } else {
            continue;
        };
        if n.display != want {
            n.display = want;
        }
    }
    // the drawer covers the views: the wheel should scroll only the library under it
    for (e, r, wheel) in &body {
        if r.kind == RegionKind::Body && wheel == ed.drawer {
            if ed.drawer {
                commands.entity(e).remove::<WheelScroll>();
            } else {
                commands.entity(e).insert(WheelScroll);
            }
        }
    }
}

pub(super) fn preview_and_toast(
    ed: Res<CubeEditor>,
    mut cards: ResMut<CardImages>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut preview: Query<(&mut Node, &mut ImageNode), (With<Preview>, Without<Toast>)>,
    mut toast: Query<(&mut Node, &mut BorderColor, &Children), (With<Toast>, Without<Preview>)>,
    mut texts: Query<&mut Text>,
) {
    let cursor = windows.single().ok().and_then(|w| w.cursor_position().map(|c| (c, Vec2::new(w.width(), w.height()))));
    for (mut n, mut img) in &mut preview {
        match (&ed.hover, cursor) {
            (Some((name, set)), Some((c, win))) if !ed.import.open => {
                let h = cards.request_raw(ImageRequest { name: name.clone(), set: set.clone(), back: false, version: ImageVersion::Normal });
                if img.image != h {
                    img.image = h;
                }
                // beside the pointer, on whichever side has room
                let x = if c.x + 30.0 + PREVIEW.x < win.x { c.x + 30.0 } else { (c.x - 30.0 - PREVIEW.x).max(0.0) };
                let y = (c.y - PREVIEW.y * 0.5).clamp(8.0, (win.y - PREVIEW.y - 8.0).max(8.0));
                n.left = Val::Px(x);
                n.top = Val::Px(y);
                if n.display != Display::Flex {
                    n.display = Display::Flex;
                }
            }
            _ => {
                if n.display != Display::None {
                    n.display = Display::None;
                }
            }
        }
    }
    let live = ed.toast.as_ref().filter(|(_, at, _)| ed.now - at < 3.5);
    for (mut n, mut bc, ch) in &mut toast {
        let want = if live.is_some() { Display::Flex } else { Display::None };
        if n.display != want {
            n.display = want;
        }
        if let Some((msg, _, error)) = live {
            let col = if *error { RED_DAMAGE } else { GOLD_DIM };
            if bc.top != col {
                *bc = BorderColor::all(col);
            }
            if let Some(c) = ch.first()
                && let Ok(mut t) = texts.get_mut(*c)
                && t.0 != *msg
            {
                t.0 = msg.clone();
            }
        }
    }
}
