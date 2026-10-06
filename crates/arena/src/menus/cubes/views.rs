//! The cube's five views (table, spoiler, curve, stacks, stats), card pictures that load only when
//! scrolled near, and the selection / hover styling every card node shares.
//!
//! Why lazy thumbnails: a cube is 360–720 cards and decoded card images never leave memory, so the
//! picture views ask for small `Thumb` images and only for cards within a screen of the viewport.

use bevy::prelude::*;
use bevy::ui::{BackgroundGradient, ColorStop, LinearGradient};
use mtg_client::deckstats::ROLES;
use mtg_client::{ImageRequest, ImageVersion};
use mtg_protocol::CubeCard;

use super::model::{self, COMBOS, CURVE_LABELS, ColorGroup, MATRIX_KINDS};
use super::{CubeAction, CubeEditor, Region, RegionKind, View};
use crate::images::CardImages;
use crate::shared::ArenaFonts;
use crate::style::*;

/// A card node in any view or list; `in_cube` cards can be selected and right-clicked away.
#[derive(Component)]
pub(super) struct CardRef {
    pub name: String,
    pub set: Option<String>,
    pub in_cube: bool,
}

/// A card picture: requested once its node comes near the viewport, faded in when it arrives.
#[derive(Component)]
pub(super) struct CardArt {
    req: ImageRequest,
    handle: Option<Handle<Image>>,
}

impl CardArt {
    pub fn thumb(c: &CubeCard) -> Self {
        Self { req: ImageRequest { name: c.name.clone(), set: c.set.clone(), back: false, version: ImageVersion::Thumb }, handle: None }
    }
    /// Full-size, requested right away (the inspector).
    pub fn now(c: &CubeCard, images: &mut CardImages) -> Self {
        let req = ImageRequest { name: c.name.clone(), set: c.set.clone(), back: false, version: ImageVersion::Normal };
        let handle = images.request_raw(req.clone());
        Self { req, handle: Some(handle) }
    }
}

/// Card shape (63×88).
const CARD_RATIO: f32 = 88.0 / 63.0;
/// Narrow enough for two piles side by side in a colour column at 1600 px.
const STACK_W: f32 = 62.0;
/// How much of each card in a pile shows above the next one (its title bar).
const STACK_PEEK: f32 = 12.0;

/// Row tints (as in the deck builder's list).
fn tint(g: ColorGroup) -> Color {
    match g {
        ColorGroup::Colorless => Color::srgb(0.62, 0.62, 0.66),
        ColorGroup::Multi => GOLD,
        ColorGroup::Land => Color::srgb(0.55, 0.45, 0.32),
        g => mana_color(g.mana().unwrap_or('C')),
    }
}

/// Header colours, bright enough for text on the dark panel.
pub(super) fn header_color(g: ColorGroup) -> Color {
    match g {
        ColorGroup::White => Color::srgb(0.95, 0.92, 0.78),
        ColorGroup::Blue => Color::srgb(0.45, 0.68, 1.0),
        ColorGroup::Black => Color::srgb(0.70, 0.62, 0.75),
        ColorGroup::Red => Color::srgb(1.0, 0.48, 0.38),
        ColorGroup::Green => Color::srgb(0.45, 0.85, 0.5),
        ColorGroup::Multi => Color::srgb(1.0, 0.82, 0.38),
        ColorGroup::Colorless => Color::srgb(0.75, 0.75, 0.78),
        ColorGroup::Land => Color::srgb(0.80, 0.68, 0.50),
    }
}

fn cube_card(c: &CubeCard) -> impl Bundle {
    (Button, CubeAction::Select(c.name.clone()), CardRef { name: c.name.clone(), set: c.set.clone(), in_cube: true })
}

fn name_of(c: &CubeCard) -> String {
    if c.qty > 1 { format!("{}× {}", c.qty, c.name) } else { c.name.clone() }
}

/// A tinted name row (table, curve).
fn name_row(p: &mut ChildSpawnerCommands, fonts: &ArenaFonts, c: &CubeCard) {
    let col = tint(ColorGroup::of(c));
    p.spawn((
        cube_card(c),
        NameRow,
        Node {
            width: Val::Percent(100.0),
            padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
            border: UiRect::all(Val::Px(1.5)),
            border_radius: BorderRadius::all(Val::Px(4.0)),
            flex_shrink: 0.0,
            ..default()
        },
        BackgroundGradient::from(LinearGradient::to_right(vec![
            ColorStop::new(col.with_alpha(0.34), Val::Percent(0.0)),
            ColorStop::new(col.with_alpha(0.06), Val::Percent(100.0)),
        ])),
        BackgroundColor(Color::NONE),
        BorderColor::all(Color::NONE),
    ))
    .with_child((ui_text(&fonts.body, name_of(c), 13.0, TEXT), Pickable::IGNORE));
}

/// A card picture tile; until its image arrives it shows the name on a dark face.
fn picture(p: &mut ChildSpawnerCommands, fonts: &ArenaFonts, c: &CubeCard, w: f32, extra: Node) -> Entity {
    let h = w * CARD_RATIO;
    p.spawn((
        cube_card(c),
        Node {
            width: Val::Px(w),
            height: Val::Px(h),
            border: UiRect::all(Val::Px(2.0)),
            border_radius: BorderRadius::all(Val::Px(w * 0.05)),
            padding: UiRect::all(Val::Px(4.0)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::FlexStart,
            overflow: Overflow::clip(),
            flex_shrink: 0.0,
            ..extra
        },
        BackgroundColor(Color::srgb(0.13, 0.09, 0.06)),
        BorderColor::all(Color::NONE),
    ))
    .with_children(|t| {
        t.spawn((ui_text(&fonts.title, c.name.clone(), (w * 0.085).clamp(8.0, 13.0), PARCHMENT), TextLayout::justify(bevy::text::Justify::Center), Pickable::IGNORE));
        t.spawn((
            CardArt::thumb(c),
            Node { position_type: PositionType::Absolute, left: Val::Px(0.0), top: Val::Px(0.0), width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
            ImageNode { color: Color::NONE, ..default() },
            Pickable::IGNORE,
        ));
        if c.qty > 1 {
            t.spawn((
                Node { position_type: PositionType::Absolute, right: Val::Px(4.0), bottom: Val::Px(4.0), padding: UiRect::axes(Val::Px(6.0), Val::Px(1.0)), border_radius: BorderRadius::MAX, ..default() },
                BackgroundColor(Color::srgba(0.95, 0.46, 0.13, 0.92)),
                Pickable::IGNORE,
            ))
            .with_child((ui_text(&fonts.numbers, format!("×{}", c.qty), 13.0, Color::WHITE), Pickable::IGNORE));
        }
    })
    .id()
}

fn group_header(p: &mut ChildSpawnerCommands, fonts: &ArenaFonts, g: ColorGroup, n: i32, size: f32) {
    // narrow columns wrap the count under a long name ("MULTICOLOUR") instead of overlapping the next
    p.spawn(Node { flex_wrap: FlexWrap::Wrap, column_gap: Val::Px(8.0), align_items: AlignItems::Baseline, flex_shrink: 0.0, ..default() }).with_children(|r| {
        r.spawn(ui_text(&fonts.title, g.label().to_uppercase(), size, header_color(g)));
        r.spawn(ui_text(&fonts.numbers, n.to_string(), size * 0.9, TEXT_DIM));
    });
}

fn block_header(p: &mut ChildSpawnerCommands, fonts: &ArenaFonts, label: &str, n: i32) {
    p.spawn((Node { margin: UiRect::top(Val::Px(8.0)), flex_shrink: 0.0, ..default() }, ui_text(&fonts.title, format!("{label} ({n})"), 13.0, GOLD)));
}

fn count(cards: &[&CubeCard]) -> i32 {
    cards.iter().map(|c| model::copies(c)).sum()
}

// ---- rebuild --------------------------------------------------------------------------------

pub(super) fn sync_body(
    mut commands: Commands,
    ed: Res<CubeEditor>,
    fonts: Res<ArenaFonts>,
    assets: Res<AssetServer>,
    mut regions: Query<(Entity, &mut Region, &mut ScrollPosition)>,
    mut last: Local<Option<(View, Option<String>)>>,
) {
    for (e, mut region, mut scroll) in &mut regions {
        if region.kind != RegionKind::Body || region.built == Some(ed.body_rev) {
            continue;
        }
        region.built = Some(ed.body_rev);
        // a different view or cube starts at the top; an edit keeps the place
        let shown = (ed.view, ed.current_ref());
        if last.as_ref() != Some(&shown) {
            scroll.y = 0.0;
            *last = Some(shown);
        }
        commands.entity(e).despawn_related::<Children>();
        let groups = model::by_group(&ed.cube.cards, &ed.filter);
        commands.entity(e).with_children(|p| {
            if ed.cube.cards.is_empty() {
                empty(p, &fonts, ed.loading);
                return;
            }
            if groups.iter().all(|g| g.is_empty()) {
                p.spawn(ui_text(&fonts.body, "No cards match the filter.", 16.0, TEXT_DIM));
                return;
            }
            match ed.view {
                View::Table => table(p, &fonts, &groups),
                View::Spoiler => spoiler(p, &fonts, &groups, ed.tile.width()),
                View::Curve => curve(p, &fonts, &groups),
                View::Stacks => stacks(p, &fonts, &groups),
                View::Stats => stats(p, &fonts, &assets, &groups, &ed),
            }
        });
    }
}

fn empty(p: &mut ChildSpawnerCommands, fonts: &ArenaFonts, loading: bool) {
    p.spawn(Node { width: Val::Percent(100.0), flex_direction: FlexDirection::Column, align_items: AlignItems::Center, row_gap: Val::Px(16.0), padding: UiRect::top(Val::Px(80.0)), ..default() })
        .with_children(|c| {
            if loading {
                c.spawn(ui_text(&fonts.title, "Loading the cube…", 24.0, TEXT_DIM));
                return;
            }
            c.spawn(ui_text(&fonts.title, "This cube is empty", 26.0, PARCHMENT));
            c.spawn(ui_text(&fonts.body, "Open one from the library, or add cards with the search and PASTE A LIST on the right.", 16.0, TEXT_DIM));
            let b = spawn_button(c, fonts, ButtonKind::Primary, "BROWSE CUBES", Vec2::new(240.0, 50.0));
            c.commands().entity(b).insert(CubeAction::Drawer);
        });
}

/// One column per colour group, blocks by type (multicolour by guild), tinted name rows.
fn table(p: &mut ChildSpawnerCommands, fonts: &ArenaFonts, groups: &[Vec<&CubeCard>; 8]) {
    p.spawn(Node { width: Val::Percent(100.0), column_gap: Val::Px(8.0), align_items: AlignItems::FlexStart, flex_shrink: 0.0, ..default() }).with_children(|row| {
        for g in ColorGroup::ALL {
            let cards = &groups[g.index()];
            if cards.is_empty() {
                continue;
            }
            row.spawn(Node { flex_grow: 1.0, flex_basis: Val::Px(0.0), min_width: Val::Px(120.0), flex_direction: FlexDirection::Column, row_gap: Val::Px(2.0), ..default() }).with_children(|col| {
                group_header(col, fonts, g, count(cards), 16.0);
                for (label, block) in model::blocks(g, cards) {
                    block_header(col, fonts, &label, count(&block));
                    for c in block {
                        name_row(col, fonts, c);
                    }
                }
            });
        }
    });
}

/// Card pictures per colour group.
fn spoiler(p: &mut ChildSpawnerCommands, fonts: &ArenaFonts, groups: &[Vec<&CubeCard>; 8], w: f32) {
    for g in ColorGroup::ALL {
        let cards = &groups[g.index()];
        if cards.is_empty() {
            continue;
        }
        group_header(p, fonts, g, count(cards), 18.0);
        p.spawn(Node { width: Val::Percent(100.0), flex_wrap: FlexWrap::Wrap, column_gap: Val::Px(8.0), row_gap: Val::Px(8.0), margin: UiRect::bottom(Val::Px(14.0)), flex_shrink: 0.0, ..default() })
            .with_children(|w_| {
                for c in cards {
                    picture(w_, fonts, c, w, Node::default());
                }
            });
    }
}

/// Per colour group: creatures and other spells by mana value; lands as a plain list.
fn curve(p: &mut ChildSpawnerCommands, fonts: &ArenaFonts, groups: &[Vec<&CubeCard>; 8]) {
    for g in ColorGroup::ALL {
        let cards = &groups[g.index()];
        if cards.is_empty() {
            continue;
        }
        p.spawn((
            Node { width: Val::Percent(100.0), flex_direction: FlexDirection::Column, row_gap: Val::Px(6.0), padding: UiRect::all(Val::Px(10.0)), margin: UiRect::bottom(Val::Px(10.0)), border_radius: BorderRadius::all(Val::Px(8.0)), flex_shrink: 0.0, ..default() },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.22)),
        ))
        .with_children(|sec| {
            group_header(sec, fonts, g, count(cards), 18.0);
            if g == ColorGroup::Land {
                sec.spawn(Node { flex_wrap: FlexWrap::Wrap, column_gap: Val::Px(6.0), row_gap: Val::Px(2.0), ..default() }).with_children(|w| {
                    for c in cards {
                        w.spawn(Node { width: Val::Px(200.0), ..default() }).with_children(|cell| name_row(cell, fonts, c));
                    }
                });
                return;
            }
            for (label, creature) in [("Creature", true), ("Non-creature", false)] {
                let part: Vec<&CubeCard> = cards.iter().copied().filter(|c| c.is_creature == creature).collect();
                if part.is_empty() {
                    continue;
                }
                block_header(sec, fonts, label, count(&part));
                sec.spawn(Node { width: Val::Percent(100.0), column_gap: Val::Px(6.0), align_items: AlignItems::FlexStart, ..default() }).with_children(|row| {
                    for (b, bucket_label) in CURVE_LABELS.iter().enumerate() {
                        let bucket: Vec<&CubeCard> = part.iter().copied().filter(|c| model::curve_bucket(c.cmc) == b).collect();
                        row.spawn(Node { flex_grow: 1.0, flex_basis: Val::Px(0.0), min_width: Val::Px(0.0), flex_direction: FlexDirection::Column, row_gap: Val::Px(2.0), ..default() }).with_children(|col| {
                            col.spawn((
                                Node { justify_content: JustifyContent::Center, padding: UiRect::vertical(Val::Px(2.0)), border: UiRect::all(Val::Px(1.0)), border_radius: BorderRadius::all(Val::Px(4.0)), margin: UiRect::bottom(Val::Px(2.0)), ..default() },
                                BorderColor::all(GOLD_DIM),
                            ))
                            .with_child(ui_text(&fonts.numbers, format!("{bucket_label} ({})", count(&bucket)), 13.0, if bucket.is_empty() { TEXT_DIM } else { TEXT }));
                            for c in bucket {
                                name_row(col, fonts, c);
                            }
                        });
                    }
                });
            }
        });
    }
}

/// One column per colour group; per type block, overlapping piles of pictures by mana value.
fn stacks(p: &mut ChildSpawnerCommands, fonts: &ArenaFonts, groups: &[Vec<&CubeCard>; 8]) {
    let h = STACK_W * CARD_RATIO;
    p.spawn(Node { width: Val::Percent(100.0), column_gap: Val::Px(6.0), align_items: AlignItems::FlexStart, flex_shrink: 0.0, ..default() }).with_children(|row| {
        for g in ColorGroup::ALL {
            let cards = &groups[g.index()];
            if cards.is_empty() {
                continue;
            }
            row.spawn(Node { flex_grow: 1.0, flex_basis: Val::Px(0.0), min_width: Val::Px(STACK_W + 4.0), flex_direction: FlexDirection::Column, align_items: AlignItems::Center, row_gap: Val::Px(4.0), ..default() })
                .with_children(|col| {
                    group_header(col, fonts, g, count(cards), 15.0);
                    for (label, block) in model::blocks(g, cards) {
                        block_header(col, fonts, &label, count(&block));
                        col.spawn(Node { width: Val::Percent(100.0), flex_wrap: FlexWrap::Wrap, justify_content: JustifyContent::Center, column_gap: Val::Px(4.0), row_gap: Val::Px(8.0), ..default() }).with_children(|piles| {
                            for b in 0..=8 {
                                let pile: Vec<&CubeCard> = block.iter().copied().filter(|c| model::stack_bucket(c.cmc) == b).collect();
                                if pile.is_empty() {
                                    continue;
                                }
                                piles.spawn(Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, row_gap: Val::Px(2.0), ..default() }).with_children(|pc| {
                                    pc.spawn(ui_text(&fonts.numbers, format!("{} ({})", model::stack_label(b), count(&pile)), 12.0, TEXT_DIM));
                                    let height = STACK_PEEK * (pile.len() as f32 - 1.0) + h;
                                    pc.spawn(Node { width: Val::Px(STACK_W), height: Val::Px(height), ..default() }).with_children(|s| {
                                        for (i, c) in pile.iter().enumerate() {
                                            let at = Node { position_type: PositionType::Absolute, left: Val::Px(0.0), top: Val::Px(i as f32 * STACK_PEEK), ..default() };
                                            let e = picture(s, fonts, c, STACK_W, at);
                                            s.commands().entity(e).insert(BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.6), Val::Px(0.0), Val::Px(-2.0), Val::Px(0.0), Val::Px(4.0)));
                                        }
                                    });
                                });
                            }
                        });
                    }
                });
        }
    });
}

// ---- stats ----------------------------------------------------------------------------------

const BLUE_BAR: Color = Color::srgb(0.36, 0.58, 0.90);

fn stat_block(p: &mut ChildSpawnerCommands, fonts: &ArenaFonts, title: &str, sub: &str, f: impl FnOnce(&mut ChildSpawnerCommands)) {
    p.spawn((
        Node { width: Val::Percent(100.0), flex_direction: FlexDirection::Column, row_gap: Val::Px(8.0), padding: UiRect::all(Val::Px(12.0)), margin: UiRect::bottom(Val::Px(12.0)), border_radius: BorderRadius::all(Val::Px(8.0)), flex_shrink: 0.0, ..default() },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.25)),
    ))
    .with_children(|b| {
        b.spawn(Node { column_gap: Val::Px(10.0), align_items: AlignItems::Baseline, ..default() }).with_children(|r| {
            r.spawn(ui_text(&fonts.title, title, 15.0, GOLD));
            if !sub.is_empty() {
                r.spawn(ui_text(&fonts.body, sub, 12.0, TEXT_DIM));
            }
        });
        f(b);
    });
}

/// A grid row: a label cell and fixed-width number cells.
fn grid_row(p: &mut ChildSpawnerCommands, fonts: &ArenaFonts, label: (&str, Color), cells: &[String], strong: bool) {
    p.spawn(Node { column_gap: Val::Px(2.0), align_items: AlignItems::Center, ..default() }).with_children(|r| {
        r.spawn((Node { width: Val::Px(150.0), flex_shrink: 0.0, ..default() }, ui_text(if strong { &fonts.title } else { &fonts.body }, label.0, 13.0, label.1)));
        for v in cells {
            let col = if v == "·" { TEXT_DIM } else if strong { GOLD_BRIGHT } else { TEXT };
            r.spawn((Node { width: Val::Px(92.0), justify_content: JustifyContent::Center, flex_shrink: 0.0, ..default() }, BackgroundColor(Color::srgba(1.0, 1.0, 1.0, if strong { 0.0 } else { 0.03 }))))
                .with_child(ui_text(&fonts.numbers, v.clone(), 14.0, col));
        }
    });
}

fn n_or_dot(n: i32) -> String {
    if n == 0 { "·".into() } else { n.to_string() }
}

fn overview_tile(p: &mut ChildSpawnerCommands, fonts: &ArenaFonts, value: String, label: &str) {
    p.spawn((
        Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, padding: UiRect::axes(Val::Px(18.0), Val::Px(10.0)), border: UiRect::all(Val::Px(1.0)), border_radius: BorderRadius::all(Val::Px(8.0)), min_width: Val::Px(120.0), ..default() },
        BorderColor::all(GOLD_DIM),
        BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.03)),
    ))
    .with_children(|t| {
        t.spawn(ui_text(&fonts.numbers, value, 26.0, GOLD_BRIGHT));
        t.spawn(ui_text(&fonts.body, label, 12.0, TEXT_DIM));
    });
}

fn stats(p: &mut ChildSpawnerCommands, fonts: &ArenaFonts, assets: &AssetServer, groups: &[Vec<&CubeCard>; 8], ed: &CubeEditor) {
    let s = model::stats(groups.iter().flatten().copied());
    let shown: Vec<ColorGroup> = ColorGroup::ALL.into_iter().filter(|g| s.groups[g.index()] > 0).collect();
    if ed.filter.active() {
        p.spawn((Node { margin: UiRect::bottom(Val::Px(8.0)), ..default() }, ui_text(&fonts.body, "Counting only the cards that match the filter.", 13.0, ORANGE_HI)));
    }
    // ---- overview
    p.spawn(Node { flex_wrap: FlexWrap::Wrap, column_gap: Val::Px(10.0), row_gap: Val::Px(10.0), margin: UiRect::bottom(Val::Px(12.0)), flex_shrink: 0.0, ..default() }).with_children(|r| {
        let pct = |n: i32| if s.total > 0 { format!("{:.0}%", n as f32 * 100.0 / s.total as f32) } else { "–".into() };
        overview_tile(r, fonts, s.total.to_string(), "cards");
        overview_tile(r, fonts, s.creatures.to_string(), &format!("creatures · {}", pct(s.creatures)));
        overview_tile(r, fonts, (s.total - s.creatures - s.lands).to_string(), "other spells");
        overview_tile(r, fonts, s.lands.to_string(), &format!("lands · {}", pct(s.lands)));
        overview_tile(r, fonts, format!("{:.2}", s.avg_mv), "avg mana value (nonland)");
        let per = (ed.cube.packs * ed.cube.pack_size).max(1);
        overview_tile(r, fonts, (s.total / per).min(8).to_string(), &format!("drafters ({}×{})", ed.cube.packs, ed.cube.pack_size));
    });
    // ---- colour × type
    stat_block(p, fonts, "COLOUR × TYPE", "", |b| {
        let mut head: Vec<String> = MATRIX_KINDS.iter().map(|(l, _)| l.to_string()).collect();
        head.push("Total".into());
        grid_row(b, fonts, ("", TEXT), &head, true);
        let mut totals = [0i32; 7];
        for g in &shown {
            let row = s.matrix[g.index()];
            for (t, v) in totals.iter_mut().zip(row) {
                *t += v;
            }
            let mut cells: Vec<String> = row.iter().map(|v| n_or_dot(*v)).collect();
            cells.push(s.groups[g.index()].to_string());
            grid_row(b, fonts, (g.label(), header_color(*g)), &cells, false);
        }
        let mut cells: Vec<String> = totals.iter().map(|v| n_or_dot(*v)).collect();
        cells.push(s.total.to_string());
        grid_row(b, fonts, ("Total", GOLD), &cells, true);
    });
    // ---- curve per colour
    stat_block(p, fonts, "MANA CURVE BY COLOUR", "creatures / other spells", |b| {
        let max = shown.iter().flat_map(|g| s.curve[g.index()].iter().map(|(c, o)| c + o)).max().unwrap_or(0).max(1) as f32;
        const H: f32 = 80.0;
        b.spawn(Node { flex_wrap: FlexWrap::Wrap, column_gap: Val::Px(16.0), row_gap: Val::Px(12.0), ..default() }).with_children(|w| {
            for g in shown.iter().filter(|g| **g != ColorGroup::Land) {
                w.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(4.0), width: Val::Px(190.0), ..default() }).with_children(|c| {
                    c.spawn(ui_text(&fonts.title, g.label(), 13.0, header_color(*g)));
                    c.spawn(Node { height: Val::Px(H + 34.0), align_items: AlignItems::FlexEnd, column_gap: Val::Px(3.0), ..default() }).with_children(|chart| {
                        for (i, (cr, ot)) in s.curve[g.index()].iter().enumerate() {
                            chart.spawn(Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, flex_grow: 1.0, flex_basis: Val::Px(0.0), ..default() }).with_children(|col| {
                                col.spawn(ui_text(&fonts.numbers, if cr + ot > 0 { (cr + ot).to_string() } else { String::new() }, 11.0, TEXT));
                                col.spawn((Node { width: Val::Percent(80.0), height: Val::Px(H * *ot as f32 / max), border_radius: BorderRadius::top(Val::Px(2.0)), ..default() }, BackgroundColor(BLUE_BAR)));
                                col.spawn((Node { width: Val::Percent(80.0), height: Val::Px(H * *cr as f32 / max), ..default() }, BackgroundColor(ORANGE)));
                                col.spawn((Node { width: Val::Percent(100.0), height: Val::Px(1.0), ..default() }, BackgroundColor(GOLD_DIM)));
                                col.spawn(ui_text(&fonts.numbers, CURVE_LABELS[i], 11.0, TEXT_DIM));
                            });
                        }
                    });
                });
            }
        });
        b.spawn(Node { column_gap: Val::Px(12.0), align_items: AlignItems::Center, ..default() }).with_children(|r| {
            for (col, label) in [(ORANGE, "Creatures"), (BLUE_BAR, "Other spells")] {
                r.spawn((Node { width: Val::Px(10.0), height: Val::Px(10.0), border_radius: BorderRadius::all(Val::Px(2.0)), ..default() }, BackgroundColor(col)));
                r.spawn(ui_text(&fonts.body, label, 12.0, TEXT_DIM));
            }
        });
    });
    // ---- roles
    stat_block(p, fonts, "ROLES BY COLOUR", "from the engine's card tags", |b| {
        let mut head: Vec<String> = shown.iter().map(|g| g.label().to_string()).collect();
        head.push("Total".into());
        grid_row(b, fonts, ("", TEXT), &head, true);
        for (i, (_, label)) in ROLES.iter().enumerate() {
            let mut cells: Vec<String> = shown.iter().map(|g| n_or_dot(s.roles[g.index()][i])).collect();
            cells.push(ColorGroup::ALL.iter().map(|g| s.roles[g.index()][i]).sum::<i32>().to_string());
            grid_row(b, fonts, (label, TEXT), &cells, false);
        }
    });
    // ---- guilds and shards
    stat_block(p, fonts, "GUILDS · SHARDS · WEDGES", "multicolour spells by colour pair / triple", |b| {
        for range in [0..10, 10..20] {
            b.spawn(Node { flex_wrap: FlexWrap::Wrap, column_gap: Val::Px(8.0), row_gap: Val::Px(8.0), ..default() }).with_children(|r| {
                for i in range {
                    let (key, name) = COMBOS[i];
                    let n = s.combos[i];
                    r.spawn((
                        Node { align_items: AlignItems::Center, column_gap: Val::Px(6.0), padding: UiRect::axes(Val::Px(10.0), Val::Px(5.0)), border: UiRect::all(Val::Px(1.0)), border_radius: BorderRadius::MAX, ..default() },
                        BorderColor::all(if n > 0 { GOLD_DIM } else { Color::srgba(1.0, 1.0, 1.0, 0.08) }),
                        BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.03)),
                    ))
                    .with_children(|c| {
                        for l in key.chars() {
                            c.spawn(mana_pip(assets, l, 15.0));
                        }
                        c.spawn(ui_text(&fonts.body, name, 13.0, if n > 0 { TEXT } else { TEXT_DIM }));
                        c.spawn(ui_text(&fonts.numbers, n.to_string(), 14.0, if n > 0 { GOLD_BRIGHT } else { TEXT_DIM }));
                    });
                }
            });
        }
    });
}

// ---- per-frame ------------------------------------------------------------------------------

/// Ask for pictures within a screen of the cube view's viewport, and fade them in when they land.
pub(super) fn lazy_art(
    time: Res<Time>,
    mut images: ResMut<CardImages>,
    loaded: Res<Assets<Image>>,
    body: Query<(&Region, &UiGlobalTransform, &ComputedNode)>,
    mut arts: Query<(&mut CardArt, &mut ImageNode, &UiGlobalTransform, &ComputedNode)>,
) {
    let view = body.iter().find(|(r, _, _)| r.kind == RegionKind::Body).map(|(_, t, n)| {
        let size = n.size();
        let margin = Vec2::new(size.x, size.y);
        (t.translation - size * 0.5 - margin, t.translation + size * 0.5 + margin)
    });
    let k = (time.delta_secs() * 8.0).min(1.0);
    for (mut art, mut img, tf, node) in &mut arts {
        if art.handle.is_none() {
            let Some((lo, hi)) = view else { continue };
            let half = node.size() * 0.5;
            let (a, b) = (tf.translation - half, tf.translation + half);
            if b.x < lo.x || a.x > hi.x || b.y < lo.y || a.y > hi.y {
                continue;
            }
            let h = images.request_raw(art.req.clone());
            img.image = h.clone();
            art.handle = Some(h);
        }
        if let Some(h) = &art.handle
            && img.image != *h
        {
            img.image = h.clone();
        }
        let ready = art.handle.as_ref().is_some_and(|h| loaded.contains(h));
        let cur = img.color.alpha();
        let target = if ready { 1.0 } else { 0.0 };
        if (cur - target).abs() > 0.004 {
            img.color = Color::srgba(1.0, 1.0, 1.0, cur + (target - cur) * k);
        }
    }
}

/// Selected card outlined in orange, hovered card in gold (name rows also light up); the hovered
/// card, in the cube or in the search results, feeds the floating preview.
pub(super) fn card_styles(mut ed: ResMut<CubeEditor>, mut q: Query<(&CardRef, &Interaction, &mut BorderColor, &mut BackgroundColor, Has<NameRow>)>) {
    let mut hover = None;
    for (r, i, mut bc, mut bg, row) in &mut q {
        let hov = matches!(i, Interaction::Hovered | Interaction::Pressed);
        if hov {
            hover = Some((r.name.clone(), r.set.clone()));
        }
        if !r.in_cube {
            let want = if hov { GOLD_DIM } else { Color::NONE };
            if bc.top != want {
                *bc = BorderColor::all(want);
            }
            continue;
        }
        let sel = ed.selected.as_deref() == Some(r.name.as_str());
        let want = if sel { ORANGE_HI } else if hov { GOLD_BRIGHT } else { Color::NONE };
        if bc.top != want {
            *bc = BorderColor::all(want);
        }
        let want_bg = if row && (hov || sel) { Color::srgba(1.0, 1.0, 1.0, 0.08) } else if row { Color::NONE } else { bg.0 };
        if bg.0 != want_bg {
            bg.0 = want_bg;
        }
    }
    if ed.hover != hover {
        ed.hover = hover;
    }
}

/// A tinted name row (its background lights up on hover).
#[derive(Component)]
pub(super) struct NameRow;
