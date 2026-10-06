//! Builder screen layout and the systems that keep it in sync with the model.
//!
//! Three columns: card pool (search ⇄ recommendations) · the decklist · live stats. Each dynamic
//! region is a container tagged with what it shows and the revision it was built from; when the
//! model's revision moves on, the container's children are rebuilt wholesale. Rebuilding a few
//! hundred nodes on an edit is cheaper to reason about than diffing, and edits are human-paced.

use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::ui::{BackgroundGradient, ColorStop, LinearGradient, RelativeCursorPosition};
use bevy::window::PrimaryWindow;
use mtg_client::deckstats::{COLORS, CURVE_BUCKETS};
use mtg_client::{ImageRequest, ImageVersion};
use mtg_protocol::{CardKind, CatalogCard, RecCard};

use super::super::widgets::{Field, cost_pips, text_field};
use super::{BuilderAction, BuilderDeck, BuilderStats, BuilderUi, RecsState, SearchState, Tab};
use crate::images::CardImages;
use crate::shared::{AppState, ArenaFonts};
use crate::style::*;

const DECK_COL_W: f32 = 330.0;
const STATS_COL_W: f32 = 360.0;
const BLUE_BAR: Color = Color::srgb(0.36, 0.58, 0.90);
const PANEL_INSET: Color = Color::srgba(0.0, 0.0, 0.0, 0.28);

/// Base tile is 146×204 (63:88).
pub fn tile_size(scale: f32) -> Vec2 {
    Vec2::new(146.0, 204.0) * scale
}

// =============================================================================================
// Markers
// =============================================================================================

/// A rebuildable region and the revision of the data it currently shows.
#[derive(Component)]
pub(super) struct Region {
    kind: RegionKind,
    built: Option<u64>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RegionKind {
    Results,
    Recs,
    DeckList,
    Stats,
}

#[derive(Component)]
pub(super) struct Scroll;
#[derive(Component)]
pub(super) struct TabPanel(Tab);
#[derive(Component)]
pub(super) struct TabButton(Tab);
/// A filter chip; its highlight mirrors the search state for the action it carries.
#[derive(Component)]
pub(super) struct Chip;
/// The card a tile / row shows (hover preview).
#[derive(Component)]
pub(super) struct TileCard(pub CatalogCard);
#[derive(Component)]
pub(super) struct TileArt(Handle<Image>);
/// "IN DECK ×2" badge on a pool tile.
#[derive(Component)]
pub(super) struct InDeckBadge(String);
#[derive(Component)]
pub(super) struct Preview;
#[derive(Component)]
pub(super) struct ToastText;
#[derive(Component)]
pub(super) struct CountText;
#[derive(Component)]
pub(super) struct ProblemText;
#[derive(Component)]
pub(super) struct SearchStatus;
#[derive(Component)]
pub(super) struct RecStatus;
#[derive(Component)]
pub(super) struct PlayButton;
#[derive(Component)]
pub(super) struct SaveButton;
/// Constructed colour filter chips vs. the commander identity readout: only one is shown.
#[derive(Component)]
pub(super) struct ColorFilterRow;
#[derive(Component)]
pub(super) struct IdentityRow;
#[derive(Component)]
pub(super) struct CommanderChip;

// =============================================================================================
// Spawn
// =============================================================================================

pub(super) fn spawn_screen(mut commands: Commands, fonts: Res<ArenaFonts>, assets: Res<AssetServer>, deck: Res<BuilderDeck>) {
    let fmt_label = deck.format.label().to_uppercase();
    commands
        .spawn((
            Name::new("DeckBuilder"),
            DespawnOnExit(AppState::DeckBuilder),
            Node { flex_direction: FlexDirection::Column, padding: UiRect::all(Val::Px(18.0)), row_gap: Val::Px(12.0), ..fullscreen_root() },
            BackgroundColor(Color::srgba(0.02, 0.012, 0.008, 0.55)),
        ))
        .with_children(|root| {
            // ---- top bar
            root.spawn(Node { width: Val::Percent(100.0), align_items: AlignItems::Center, column_gap: Val::Px(12.0), ..default() }).with_children(|bar| {
                let b = spawn_button(bar, &fonts, ButtonKind::Secondary, "BACK", Vec2::new(110.0, 44.0));
                bar.commands().entity(b).insert(BuilderAction::Back);
                bar.spawn(text_field(&fonts, Field::BuilderName, &deck.name, Val::Px(300.0), 1.0, false));
                bar.spawn(chip(Node { padding: UiRect::axes(Val::Px(14.0), Val::Px(6.0)), ..default() })).with_child(ui_text(&fonts.title, fmt_label, 15.0, GOLD));
                bar.spawn((CountText, ui_text(&fonts.numbers, "", 26.0, GOLD_BRIGHT)));
                bar.spawn((ProblemText, Node { flex_grow: 1.0, flex_basis: Val::Px(0.0), overflow: Overflow::clip(), ..default() }, ui_text(&fonts.body, "", 14.0, TEXT_DIM)));
                let b = spawn_button(bar, &fonts, ButtonKind::Secondary, "PLAYTEST", Vec2::new(150.0, 44.0));
                bar.commands().entity(b).insert(BuilderAction::Playtest);
                let b = spawn_button(bar, &fonts, ButtonKind::Secondary, "SAVE", Vec2::new(120.0, 44.0));
                bar.commands().entity(b).insert((BuilderAction::Save, SaveButton));
                let b = spawn_button(bar, &fonts, ButtonKind::Primary, "PLAY", Vec2::new(170.0, 50.0));
                bar.commands().entity(b).insert((BuilderAction::Play, PlayButton));
            });
            // ---- three columns
            root.spawn(Node { width: Val::Percent(100.0), flex_grow: 1.0, min_height: Val::Px(0.0), column_gap: Val::Px(14.0), ..default() }).with_children(|cols| {
                // pool
                cols.spawn(ornate_panel(Node { flex_grow: 1.0, min_width: Val::Px(0.0), padding: UiRect::all(Val::Px(14.0)), flex_direction: FlexDirection::Column, row_gap: Val::Px(10.0), ..default() }))
                    .with_children(|p| {
                        p.spawn(Node { column_gap: Val::Px(10.0), ..default() }).with_children(|tabs| {
                            for (t, label) in [(Tab::Search, "CARD SEARCH"), (Tab::Recommend, "RECOMMENDATIONS")] {
                                let b = spawn_button(tabs, &fonts, ButtonKind::Secondary, label, Vec2::new(230.0, 42.0));
                                tabs.commands().entity(b).insert((BuilderAction::Tab(t), TabButton(t)));
                            }
                        });
                        spawn_search_panel(p, &fonts, &assets);
                        spawn_recs_panel(p, &fonts);
                    });
                // deck list
                cols.spawn(ornate_panel(Node { width: Val::Px(DECK_COL_W), flex_shrink: 0.0, padding: UiRect::all(Val::Px(12.0)), flex_direction: FlexDirection::Column, ..default() }))
                    .with_children(|p| {
                        p.spawn(heading(&fonts, "DECK", 22.0));
                        p.spawn(divider());
                        p.spawn(scroll_region(RegionKind::DeckList, Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(2.0), ..default() }));
                    });
                // stats
                cols.spawn(ornate_panel(Node { width: Val::Px(STATS_COL_W), flex_shrink: 0.0, padding: UiRect::all(Val::Px(12.0)), flex_direction: FlexDirection::Column, ..default() }))
                    .with_children(|p| {
                        p.spawn(heading(&fonts, "STATS", 22.0));
                        p.spawn(divider());
                        p.spawn(scroll_region(RegionKind::Stats, Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(14.0), ..default() }));
                    });
            });
        });
    // floating layers
    commands.spawn((
        Preview,
        DespawnOnExit(AppState::DeckBuilder),
        GlobalZIndex(90),
        Node { position_type: PositionType::Absolute, width: Val::Px(320.0), height: Val::Px(447.0), display: Display::None, border_radius: BorderRadius::all(Val::Px(14.0)), ..default() },
        ImageNode::default(),
        BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.8), Val::Px(0.0), Val::Px(10.0), Val::Px(0.0), Val::Px(28.0)),
        Pickable::IGNORE,
    ));
    commands
        .spawn((
            DespawnOnExit(AppState::DeckBuilder),
            GlobalZIndex(95),
            Node { position_type: PositionType::Absolute, bottom: Val::Px(26.0), width: Val::Percent(100.0), justify_content: JustifyContent::Center, ..default() },
            Pickable::IGNORE,
        ))
        .with_children(|r| {
            r.spawn((
                ToastText,
                chip(Node { padding: UiRect::axes(Val::Px(22.0), Val::Px(10.0)), display: Display::None, ..default() }),
                Pickable::IGNORE,
            ))
            .with_child((ui_text(&fonts.body, "", 17.0, TEXT), Pickable::IGNORE));
        });
}

fn scroll_region(kind: RegionKind, inner: Node) -> impl Bundle {
    (
        Region { kind, built: None },
        Scroll,
        ScrollPosition::default(),
        RelativeCursorPosition::default(),
        Node { width: Val::Percent(100.0), flex_grow: 1.0, min_height: Val::Px(0.0), overflow: Overflow::scroll_y(), ..inner },
    )
}

fn chip_button(p: &mut ChildSpawnerCommands, fonts: &ArenaFonts, label: &str, act: BuilderAction) -> Entity {
    p.spawn((
        Button,
        Chip,
        act,
        Node {
            padding: UiRect::axes(Val::Px(10.0), Val::Px(5.0)),
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
            c.spawn((ui_text(&fonts.body, label, 14.0, TEXT), Pickable::IGNORE));
        }
    })
    .id()
}

fn spawn_search_panel(p: &mut ChildSpawnerCommands, fonts: &ArenaFonts, assets: &AssetServer) {
    p.spawn((TabPanel(Tab::Search), Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(8.0), flex_grow: 1.0, min_height: Val::Px(0.0), ..default() }))
        .with_children(|s| {
            s.spawn(Node { column_gap: Val::Px(10.0), align_items: AlignItems::Center, ..default() }).with_children(|r| {
                r.spawn(ui_text(&fonts.body, "Name", 14.0, TEXT_DIM));
                r.spawn(text_field(fonts, Field::BuilderSearch, "", Val::Px(240.0), 1.0, false));
                r.spawn(ui_text(&fonts.body, "Rules text", 14.0, TEXT_DIM));
                r.spawn(text_field(fonts, Field::BuilderText, "", Val::Px(200.0), 1.0, false));
                let c = chip_button(r, fonts, "Commanders only", BuilderAction::CommanderOnly);
                r.commands().entity(c).insert(CommanderChip);
            });
            s.spawn(Node { column_gap: Val::Px(6.0), row_gap: Val::Px(6.0), align_items: AlignItems::Center, flex_wrap: FlexWrap::Wrap, ..default() }).with_children(|r| {
                r.spawn((ColorFilterRow, Node { column_gap: Val::Px(4.0), align_items: AlignItems::Center, ..default() })).with_children(|cr| {
                    for (i, c) in COLORS.iter().enumerate() {
                        let e = chip_button(cr, fonts, "", BuilderAction::ToggleColor(i));
                        cr.commands().entity(e).with_child((mana_pip(assets, *c, 18.0), Pickable::IGNORE));
                    }
                });
                r.spawn((IdentityRow, Node { column_gap: Val::Px(4.0), align_items: AlignItems::Center, display: Display::None, ..default() }));
                r.spawn(Node { width: Val::Px(8.0), ..default() });
                for (label, k) in [
                    ("Creature", "Creature"),
                    ("Instant", "Instant"),
                    ("Sorcery", "Sorcery"),
                    ("Artifact", "Artifact"),
                    ("Enchantment", "Enchantment"),
                    ("Planeswalker", "Planeswalker"),
                    ("Land", "Land"),
                ] {
                    chip_button(r, fonts, label, BuilderAction::Kind(Some(k)));
                }
                r.spawn(Node { width: Val::Px(8.0), ..default() });
                r.spawn(ui_text(&fonts.body, "MV", 14.0, TEXT_DIM));
                for v in 0..=7 {
                    let label = if v == 7 { "7+".to_string() } else { v.to_string() };
                    chip_button(r, fonts, &label, BuilderAction::Cmc(Some(v)));
                }
            });
            s.spawn((SearchStatus, ui_text(&fonts.body, "", 14.0, TEXT_DIM)));
            s.spawn(scroll_region(
                RegionKind::Results,
                Node { flex_wrap: FlexWrap::Wrap, align_content: AlignContent::FlexStart, column_gap: Val::Px(12.0), row_gap: Val::Px(12.0), padding: UiRect::all(Val::Px(4.0)), ..default() },
            ));
        });
}

fn spawn_recs_panel(p: &mut ChildSpawnerCommands, fonts: &ArenaFonts) {
    p.spawn((
        TabPanel(Tab::Recommend),
        Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(8.0), flex_grow: 1.0, min_height: Val::Px(0.0), display: Display::None, ..default() },
    ))
    .with_children(|s| {
        s.spawn((RecStatus, ui_text(&fonts.body, "", 14.0, TEXT_DIM)));
        s.spawn(scroll_region(RegionKind::Recs, Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(14.0), padding: UiRect::all(Val::Px(4.0)), ..default() }));
    });
}

// =============================================================================================
// Card tile
// =============================================================================================

/// A card picture tile with an optional caption under it. Returns the tile entity (a `Button`
/// carrying `TileCard`) so callers can attach a `BuilderAction`.
/// `rim` outlines the card in gold; `badge` adds the "IN DECK ×n" marker (pool tiles only).
pub fn card_tile(p: &mut ChildSpawnerCommands, fonts: &ArenaFonts, card: &CatalogCard, img: Handle<Image>, size: Vec2, caption: Option<&RecCard>, rim: bool, badge: bool) -> Entity {
    let mut tile = p.spawn((
        Button,
        TileCard(card.clone()),
        Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, row_gap: Val::Px(3.0), flex_shrink: 0.0, ..default() },
        UiTransform::IDENTITY,
    ));
    tile.with_children(|t| {
        t.spawn((
            Node {
                width: Val::Px(size.x),
                height: Val::Px(size.y),
                border: UiRect::all(Val::Px(if rim { 2.5 } else { 0.0 })),
                border_radius: BorderRadius::all(Val::Px(size.x * 0.05)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                padding: UiRect::all(Val::Px(8.0)),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(Color::srgb(0.13, 0.09, 0.06)),
            BorderColor::all(GOLD_BRIGHT),
            Pickable::IGNORE,
        ))
        .with_children(|f| {
            f.spawn((ui_text(&fonts.title, card.name.clone(), 13.0, PARCHMENT), TextLayout::justify(bevy::text::Justify::Center), Pickable::IGNORE));
            f.spawn((
                TileArt(img.clone()),
                Node { position_type: PositionType::Absolute, left: Val::Px(0.0), top: Val::Px(0.0), width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
                ImageNode { image: img, color: Color::srgba(1.0, 1.0, 1.0, 0.0), ..default() },
                Pickable::IGNORE,
            ));
            if !badge {
                return;
            }
            f.spawn((
                InDeckBadge(card.name.clone()),
                Node {
                    position_type: PositionType::Absolute,
                    top: Val::Px(size.y * 0.42),
                    padding: UiRect::axes(Val::Px(8.0), Val::Px(2.0)),
                    border_radius: BorderRadius::all(Val::Px(99.0)),
                    display: Display::None,
                    ..default()
                },
                BackgroundColor(Color::srgba(0.95, 0.46, 0.13, 0.92)),
                Pickable::IGNORE,
            ))
            .with_child((ui_text(&fonts.numbers, "", 15.0, Color::WHITE), Pickable::IGNORE));
        });
        if let Some(rc) = caption {
            let line = if rc.inclusion > 0.0 { format!("{:.0}% of decks", rc.inclusion * 100.0) } else { rc.reason.clone() };
            t.spawn((ui_text(&fonts.numbers, line, 14.0, TEXT), Pickable::IGNORE));
            if rc.synergy.abs() >= 0.005 {
                let col = if rc.synergy > 0.0 { GREEN_HEAL } else { TEXT_DIM };
                t.spawn((ui_text(&fonts.numbers, format!("{:+.0}% synergy", rc.synergy * 100.0), 13.0, col), Pickable::IGNORE));
            }
        }
    });
    tile.id()
}

fn spawn_pool_tile(p: &mut ChildSpawnerCommands, fonts: &ArenaFonts, images: &mut CardImages, card: &CatalogCard, caption: Option<&RecCard>, scale: f32) {
    let h = images.request_raw(ImageRequest { name: card.name.clone(), set: card.set.clone(), back: false, version: ImageVersion::Normal });
    let e = card_tile(p, fonts, card, h, tile_size(scale), caption, false, true);
    p.commands().entity(e).insert(BuilderAction::AddCard(Box::new(card.clone())));
}

// =============================================================================================
// Sync
// =============================================================================================

pub(super) fn sync_views(
    mut commands: Commands,
    fonts: Res<ArenaFonts>,
    assets: Res<AssetServer>,
    mut images: ResMut<CardImages>,
    deck: Res<BuilderDeck>,
    search: Res<SearchState>,
    recs: Res<RecsState>,
    stats: Res<BuilderStats>,
    mut regions: Query<(Entity, &mut Region, &mut ScrollPosition)>,
) {
    for (e, mut region, mut scroll) in &mut regions {
        let want = match region.kind {
            RegionKind::Results => search.shown,
            RegionKind::Recs => recs.shown,
            RegionKind::DeckList => deck.rev,
            RegionKind::Stats => stats.rev.unwrap_or(0),
        };
        if region.built == Some(want) {
            continue;
        }
        let first = region.built.is_none();
        region.built = Some(want);
        commands.entity(e).despawn_related::<Children>();
        match region.kind {
            RegionKind::Results => {
                if !search.append_shown() {
                    scroll.y = 0.0;
                }
                commands.entity(e).with_children(|g| {
                    for c in &search.results {
                        spawn_pool_tile(g, &fonts, &mut images, c, None, 1.0);
                    }
                    if search.more {
                        g.spawn(Node { width: Val::Percent(100.0), justify_content: JustifyContent::Center, padding: UiRect::vertical(Val::Px(8.0)), ..default() }).with_children(|r| {
                            let b = spawn_button(r, &fonts, ButtonKind::Secondary, "LOAD MORE", Vec2::new(200.0, 42.0));
                            r.commands().entity(b).insert(BuilderAction::MoreResults);
                        });
                    }
                });
            }
            RegionKind::Recs => {
                if first || recs.data.is_none() {
                    scroll.y = 0.0;
                }
                commands.entity(e).with_children(|g| build_recs(g, &fonts, &mut images, &deck, &recs));
            }
            RegionKind::DeckList => {
                commands.entity(e).with_children(|g| build_deck_list(g, &fonts, &assets, &deck));
            }
            RegionKind::Stats => {
                commands.entity(e).with_children(|g| build_stats(g, &fonts, &assets, &deck, &stats));
            }
        }
    }
}

impl SearchState {
    /// True while the last answer was a "load more" page (keep the scroll position).
    fn append_shown(&self) -> bool {
        self.results.len() as u32 > super::PAGE
    }
}

fn section_title(g: &mut ChildSpawnerCommands, fonts: &ArenaFonts, title: &str, sub: &str) {
    g.spawn(Node { align_items: AlignItems::Baseline, column_gap: Val::Px(10.0), width: Val::Percent(100.0), ..default() }).with_children(|r| {
        r.spawn(ui_text(&fonts.title, title.to_uppercase(), 18.0, GOLD_BRIGHT));
        if !sub.is_empty() {
            r.spawn(ui_text(&fonts.body, sub, 13.0, TEXT_DIM));
        }
    });
}

fn build_recs(g: &mut ChildSpawnerCommands, fonts: &ArenaFonts, images: &mut CardImages, deck: &BuilderDeck, recs: &RecsState) {
    let Some(d) = &recs.data else { return };
    // what the suggestions are based on
    let basis = if deck.format.has_commander() {
        match deck.commanders.first() {
            Some(c) if d.commander_decks > 0 => format!("Based on {} decks led by {}", d.commander_decks, c.name),
            Some(c) => format!("{} isn't in the deck data — suggestions come from themes and popularity", c.name),
            None => "Pick a commander for synergy picks — showing popular cards meanwhile".into(),
        }
    } else if d.archetypes.is_empty() {
        "Add a few cards to match the deck against known archetypes".into()
    } else {
        let a: Vec<String> = d.archetypes.iter().take(3).map(|a| format!("{} ({}, {:.0}%)", a.name, a.format, a.similarity * 100.0)).collect();
        format!("Closest archetypes: {}", a.join("  ·  "))
    };
    g.spawn(ui_text(&fonts.body, basis, 15.0, TEXT));
    if !d.themes.is_empty() {
        section_title(g, fonts, "Themes", "click to focus the suggestions");
        g.spawn(Node { flex_wrap: FlexWrap::Wrap, column_gap: Val::Px(8.0), row_gap: Val::Px(8.0), ..default() }).with_children(|r| {
            for t in &d.themes {
                let label = if t.count > 0 { format!("{}  {}", t.label, t.count) } else { t.label.clone() };
                let e = chip_button(r, fonts, &label, BuilderAction::ToggleTheme(t.key.clone()));
                if t.active {
                    r.commands().entity(e).insert(ActiveChip);
                }
            }
        });
    }
    for s in &d.sections {
        if s.cards.is_empty() {
            continue;
        }
        let sub = match s.key.as_str() {
            "synergy" => "played far more with this deck than elsewhere",
            "top" => "most played",
            k if k.starts_with("role:") => "fills a role",
            _ => "",
        };
        section_title(g, fonts, &s.title, sub);
        g.spawn(Node { width: Val::Percent(100.0), flex_wrap: FlexWrap::Wrap, column_gap: Val::Px(12.0), row_gap: Val::Px(12.0), ..default() }).with_children(|r| {
            for rc in &s.cards {
                spawn_pool_tile(r, fonts, images, &rc.card, Some(rc), 0.84);
            }
        });
    }
}

/// Theme chip the engine marked active (highlight).
#[derive(Component)]
pub(super) struct ActiveChip;

fn build_deck_list(g: &mut ChildSpawnerCommands, fonts: &ArenaFonts, assets: &AssetServer, deck: &BuilderDeck) {
    if deck.loading {
        g.spawn(ui_text(&fonts.body, "Loading deck…", 15.0, TEXT_DIM));
        return;
    }
    if deck.format.has_commander() {
        group_header(g, fonts, "Commander", deck.commanders.len() as u32);
        if deck.commanders.is_empty() {
            g.spawn((Node { padding: UiRect::all(Val::Px(6.0)), ..default() }, ui_text(&fonts.body, "Click a legendary creature in the search to lead this deck (right-click any eligible card later to swap).", 13.0, TEXT_DIM)));
        }
        for c in &deck.commanders {
            deck_row(g, fonts, assets, c, 1, false, BuilderAction::RemoveCommander(c.name.clone()), false);
        }
    }
    if deck.main.is_empty() && !(deck.format.has_commander() && deck.commanders.is_empty()) {
        g.spawn((Node { padding: UiRect::all(Val::Px(6.0)), ..default() }, ui_text(&fonts.body, "Click cards in the search or recommendations to add them. Click a row here to remove one.", 13.0, TEXT_DIM)));
    }
    for kind in CardKind::ALL {
        let mut cards: Vec<&(CatalogCard, u32)> = deck.main.iter().filter(|(c, _)| c.primary_type() == kind).collect();
        if cards.is_empty() {
            continue;
        }
        cards.sort_by(|a, b| a.0.cmc.cmp(&b.0.cmc).then_with(|| a.0.name.cmp(&b.0.name)));
        group_header(g, fonts, kind.label(), cards.iter().map(|(_, q)| q).sum());
        for (c, q) in cards {
            deck_row(g, fonts, assets, c, *q, deck.off_identity(c), BuilderAction::RemoveOne(c.name.clone()), true);
        }
    }
}

fn group_header(g: &mut ChildSpawnerCommands, fonts: &ArenaFonts, label: &str, n: u32) {
    g.spawn(Node { justify_content: JustifyContent::SpaceBetween, margin: UiRect::new(Val::Px(0.0), Val::Px(0.0), Val::Px(8.0), Val::Px(2.0)), flex_shrink: 0.0, ..default() }).with_children(|r| {
        r.spawn(ui_text(&fonts.title, label.to_uppercase(), 14.0, GOLD));
        r.spawn(ui_text(&fonts.numbers, n.to_string(), 15.0, GOLD));
    });
}

fn deck_row(g: &mut ChildSpawnerCommands, fonts: &ArenaFonts, assets: &AssetServer, c: &CatalogCard, qty: u32, off: bool, act: BuilderAction, plus: bool) {
    g.spawn((
        Button,
        act,
        TileCard(c.clone()),
        DeckRow,
        Node {
            width: Val::Percent(100.0),
            padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
            align_items: AlignItems::Center,
            column_gap: Val::Px(6.0),
            border_radius: BorderRadius::all(Val::Px(6.0)),
            flex_shrink: 0.0,
            ..default()
        },
        BackgroundGradient::from(LinearGradient::to_right(vec![
            ColorStop::new(row_tint(c).with_alpha(0.30), Val::Percent(0.0)),
            ColorStop::new(Color::srgba(0.0, 0.0, 0.0, 0.0), Val::Percent(85.0)),
        ])),
        BackgroundColor(Color::NONE),
    ))
    .with_children(|r| {
        r.spawn((Node { width: Val::Px(22.0), ..default() }, ui_text(&fonts.numbers, qty.to_string(), 16.0, GOLD_BRIGHT), Pickable::IGNORE));
        r.spawn((
            Node { flex_grow: 1.0, flex_basis: Val::Px(0.0), overflow: Overflow::clip(), ..default() },
            ui_text(&fonts.body, c.name.clone(), 14.0, if off { RED_DAMAGE } else { TEXT }),
            Pickable::IGNORE,
        ));
        if let Some(cost) = &c.mana_cost {
            cost_pips(r, fonts, assets, cost, 14.0);
        }
        if plus {
            r.spawn((
                Button,
                BuilderAction::AddOne(c.name.clone()),
                Node { width: Val::Px(20.0), height: Val::Px(20.0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, border_radius: BorderRadius::MAX, flex_shrink: 0.0, ..default() },
                BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.08)),
            ))
            .with_child((ui_text(&fonts.numbers, "+", 15.0, GOLD_BRIGHT), Pickable::IGNORE));
        }
    });
}

#[derive(Component)]
pub(super) struct DeckRow;

fn row_tint(c: &CatalogCard) -> Color {
    let cols: Vec<char> = c.colors.chars().collect();
    match cols.len() {
        0 if c.primary_type() == CardKind::Land => Color::srgb(0.55, 0.45, 0.32),
        0 => Color::srgb(0.62, 0.62, 0.66),
        1 => mana_color(cols[0]),
        _ => GOLD,
    }
}

// ---- stats --------------------------------------------------------------------------------

fn stat_block(g: &mut ChildSpawnerCommands, fonts: &ArenaFonts, title: &str, f: impl FnOnce(&mut ChildSpawnerCommands)) {
    g.spawn((
        Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(6.0), padding: UiRect::all(Val::Px(10.0)), border_radius: BorderRadius::all(Val::Px(8.0)), flex_shrink: 0.0, ..default() },
        BackgroundColor(PANEL_INSET),
    ))
    .with_children(|b| {
        b.spawn(ui_text(&fonts.title, title, 14.0, GOLD));
        f(b);
    });
}

fn pct(p: f32) -> String {
    format!("{:.0}%", (p * 100.0).clamp(0.0, 100.0))
}

/// `label ……… value` with an optional thin bar under it (`fill` 0..1, `col`).
fn meter(g: &mut ChildSpawnerCommands, fonts: &ArenaFonts, label: &str, value: &str, value_col: Color, bar: Option<(f32, Color)>) {
    g.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(2.0), ..default() }).with_children(|m| {
        m.spawn(Node { justify_content: JustifyContent::SpaceBetween, ..default() }).with_children(|r| {
            r.spawn(ui_text(&fonts.body, label, 13.0, TEXT));
            r.spawn(ui_text(&fonts.numbers, value, 14.0, value_col));
        });
        if let Some((fill, col)) = bar {
            m.spawn((Node { height: Val::Px(5.0), border_radius: BorderRadius::MAX, ..default() }, BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.07)))).with_child((
                Node { width: Val::Percent(fill.clamp(0.0, 1.0) * 100.0), height: Val::Percent(100.0), border_radius: BorderRadius::MAX, ..default() },
                BackgroundColor(col),
            ));
        }
    });
}

fn build_stats(g: &mut ChildSpawnerCommands, fonts: &ArenaFonts, assets: &AssetServer, deck: &BuilderDeck, st: &BuilderStats) {
    let s = &st.stats;
    // ---- mana curve
    stat_block(g, fonts, "MANA CURVE", |b| {
        let max = s.curve_max().max(1) as f32;
        const H: f32 = 110.0;
        b.spawn(Node { height: Val::Px(H + 34.0), align_items: AlignItems::FlexEnd, justify_content: JustifyContent::SpaceBetween, column_gap: Val::Px(4.0), ..default() }).with_children(|chart| {
            for i in 0..CURVE_BUCKETS {
                let (cr, ot) = (s.curve_creature[i], s.curve_other[i]);
                chart.spawn(Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, flex_grow: 1.0, flex_basis: Val::Px(0.0), ..default() }).with_children(|col| {
                    col.spawn(ui_text(&fonts.numbers, if cr + ot > 0 { (cr + ot).to_string() } else { String::new() }, 13.0, TEXT));
                    // stacked: noncreature on top of creature
                    col.spawn(Node { width: Val::Percent(78.0), height: Val::Px(H * ot as f32 / max), border_radius: BorderRadius::top(Val::Px(3.0)), ..default() }).insert(BackgroundColor(BLUE_BAR));
                    col.spawn(Node { width: Val::Percent(78.0), height: Val::Px(H * cr as f32 / max), ..default() }).insert(BackgroundColor(ORANGE));
                    col.spawn((Node { width: Val::Percent(100.0), height: Val::Px(1.0), ..default() }, BackgroundColor(GOLD_DIM)));
                    col.spawn(ui_text(&fonts.numbers, if i == CURVE_BUCKETS - 1 { "7+".into() } else { i.to_string() }, 13.0, TEXT_DIM));
                });
            }
        });
        b.spawn(Node { column_gap: Val::Px(12.0), align_items: AlignItems::Center, ..default() }).with_children(|r| {
            for (col, label) in [(ORANGE, "Creatures"), (BLUE_BAR, "Other spells")] {
                r.spawn((Node { width: Val::Px(10.0), height: Val::Px(10.0), border_radius: BorderRadius::all(Val::Px(2.0)), ..default() }, BackgroundColor(col)));
                r.spawn(ui_text(&fonts.body, label, 12.0, TEXT_DIM));
            }
        });
        meter(b, fonts, "Average mana value", &format!("{:.2}", s.avg_cmc), GOLD_BRIGHT, None);
        let lands_ok = (s.lands as f32 - s.suggested_lands).abs() <= 1.5 || s.nonlands == 0;
        meter(b, fonts, &format!("Lands (≈{:.0} suggested for this curve)", s.suggested_lands), &s.lands.to_string(), if lands_ok { GOLD_BRIGHT } else { ORANGE_HI }, None);
    });
    // ---- colours
    let used = s.colors_used();
    if !used.is_empty() {
        stat_block(g, fonts, "COLOURS  ·  pips vs. land sources", |b| {
            let pip_total: f32 = s.pips.iter().sum::<f32>().max(1.0);
            let src_total = s.lands.max(1) as f32;
            for i in used {
                let c = COLORS[i];
                let pip_share = s.pips[i] / pip_total;
                let src_share = s.sources[i] as f32 / src_total;
                // a colour asking for much more than its share of sources is the classic colour-screw
                let short = pip_share > 0.0 && src_share + 0.12 < pip_share;
                b.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(8.0), ..default() }).with_children(|r| {
                    r.spawn(mana_pip(assets, c, 18.0));
                    r.spawn(Node { flex_direction: FlexDirection::Column, flex_grow: 1.0, row_gap: Val::Px(2.0), ..default() }).with_children(|bars| {
                        for (share, col) in [(pip_share, mana_color(c)), (src_share, Color::srgba(0.85, 0.80, 0.70, 0.7))] {
                            bars.spawn((Node { height: Val::Px(5.0), border_radius: BorderRadius::MAX, ..default() }, BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.06)))).with_child((
                                Node { width: Val::Percent(share.clamp(0.0, 1.0) * 100.0), height: Val::Percent(100.0), border_radius: BorderRadius::MAX, ..default() },
                                BackgroundColor(col),
                            ));
                        }
                    });
                    r.spawn((
                        Node { width: Val::Px(132.0), justify_content: JustifyContent::FlexEnd, flex_shrink: 0.0, ..default() },
                        ui_text(&fonts.numbers, format!("{:.0} pips · {} sources", s.pips[i], s.sources[i]), 13.0, if short { ORANGE_HI } else { TEXT }),
                        TextLayout::no_wrap(),
                    ));
                });
            }
        });
    }
    // ---- roles
    stat_block(g, fonts, "ROLES", |b| {
        for r in &s.roles {
            if r.target == 0 && r.count == 0 {
                continue;
            }
            let (val, col, bar) = if r.target > 0 {
                let ok = r.count >= r.target;
                (format!("{} / {}", r.count, r.target), if ok { GREEN_HEAL } else { ORANGE_HI }, Some((r.count as f32 / r.target as f32, if ok { GREEN_HEAL } else { ORANGE })))
            } else {
                (r.count.to_string(), TEXT, None)
            };
            meter(b, fonts, r.label, &val, col, bar);
        }
    });
    // ---- types
    stat_block(g, fonts, "CARD TYPES", |b| {
        b.spawn(Node { flex_wrap: FlexWrap::Wrap, column_gap: Val::Px(14.0), row_gap: Val::Px(4.0), ..default() }).with_children(|w| {
            for (k, n) in &s.types {
                w.spawn(ui_text(&fonts.body, format!("{} {}", k.label(), n), 13.0, TEXT));
            }
        });
    });
    // ---- odds
    if s.total >= 20 {
        stat_block(g, fonts, "OPENING HAND (7 cards)", |b| {
            meter(b, fonts, "Keepable land count (2–5)", &pct(s.p_open_2_to_5_lands), GOLD_BRIGHT, Some((s.p_open_2_to_5_lands, GOLD)));
            meter(b, fonts, "Mana screw (0–1 lands)", &pct(s.p_open_screw), if s.p_open_screw > 0.15 { ORANGE_HI } else { TEXT }, None);
            meter(b, fonts, "Flood (6–7 lands)", &pct(s.p_open_flood), if s.p_open_flood > 0.08 { ORANGE_HI } else { TEXT }, None);
            meter(b, fonts, "Expected lands", &format!("{:.1}", s.expected_open_lands), TEXT, None);
            b.spawn(ui_text(&fonts.body, "Land drop hit by turn (on the draw)", 12.0, TEXT_DIM));
            b.spawn(Node { justify_content: JustifyContent::SpaceBetween, column_gap: Val::Px(4.0), ..default() }).with_children(|r| {
                for (t, p) in s.p_land_drop.iter().enumerate() {
                    r.spawn(Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, flex_grow: 1.0, ..default() }).with_children(|c| {
                        c.spawn(ui_text(&fonts.numbers, pct(*p), 13.0, if *p < 0.6 { ORANGE_HI } else { TEXT }));
                        c.spawn(ui_text(&fonts.body, format!("T{}", t + 1), 11.0, TEXT_DIM));
                    });
                }
            });
        });
        let gf = &st.goldfish;
        if !gf.cards.is_empty() {
            stat_block(g, fonts, "GOLDFISH  ·  cast on curve", |b| {
                b.spawn(ui_text(&fonts.body, format!("{} simulated games, one land per turn, no ramp", gf.iterations), 11.0, TEXT_DIM));
                meter(b, fonts, "Spells castable on curve", &pct(gf.curve_consistency), GOLD_BRIGHT, Some((gf.curve_consistency, GOLD)));
                let hard: Vec<_> = gf.cards.iter().filter(|c| c.on_curve < 0.75).take(5).collect();
                if !hard.is_empty() {
                    b.spawn(ui_text(&fonts.body, "Hardest to cast on time", 12.0, TEXT_DIM));
                    for c in hard {
                        meter(b, fonts, &format!("{} (MV {})", c.name, c.cmc), &pct(c.on_curve), ORANGE_HI, None);
                    }
                }
            });
        }
    }
    let _ = deck;
}

// =============================================================================================
// Per-frame visuals
// =============================================================================================

pub(super) fn tiles_and_hover(
    time: Res<Time>,
    mut ui: ResMut<BuilderUi>,
    images: Res<Assets<Image>>,
    mut arts: Query<(&TileArt, &mut ImageNode), Without<Preview>>,
    tiles: Query<(&Interaction, &TileCard)>,
    mut tile_tf: Query<(&Interaction, &mut UiTransform), (With<TileCard>, Without<DeckRow>)>,
    mut rows: Query<(&Interaction, &mut BackgroundColor), With<DeckRow>>,
) {
    let dt = time.delta_secs();
    // image fade-in once the texture arrives
    for (a, mut n) in &mut arts {
        let loaded = images.get(&a.0).is_some_and(|i| i.width() > 8);
        let cur = n.color.alpha();
        let target = if loaded { 1.0 } else { 0.0 };
        if (cur - target).abs() > 0.004 {
            n.color = Color::srgba(1.0, 1.0, 1.0, cur + (target - cur) * (dt * 8.0).min(1.0));
        }
    }
    // hover: tiles grow a little, rows light up, the preview follows
    let k = (dt * 14.0).min(1.0);
    for (i, mut tf) in &mut tile_tf {
        let target = if matches!(i, Interaction::Hovered | Interaction::Pressed) { 1.05 } else { 1.0 };
        let s = tf.scale.x + (target - tf.scale.x) * k;
        if (s - tf.scale.x).abs() > 1e-4 {
            tf.scale = Vec2::splat(s);
        }
    }
    for (i, mut bg) in &mut rows {
        let want = match i {
            Interaction::Pressed => Color::srgba(0.95, 0.30, 0.2, 0.25),
            Interaction::Hovered => Color::srgba(1.0, 1.0, 1.0, 0.08),
            Interaction::None => Color::NONE,
        };
        if bg.0 != want {
            bg.0 = want;
        }
    }
    let hovered = tiles.iter().find(|(i, _)| matches!(i, Interaction::Hovered | Interaction::Pressed)).map(|(_, t)| &t.0);
    if hovered.map(|c| &c.name) != ui.hover.as_ref().map(|c| &c.name) {
        ui.hover = hovered.cloned();
    }
}

pub(super) fn chips_and_badges(
    deck: Res<BuilderDeck>,
    search: Res<SearchState>,
    recs: Res<RecsState>,
    mut chips: Query<(&BuilderAction, &Interaction, &mut BackgroundColor, &mut BorderColor, Has<ActiveChip>), (With<Chip>, Without<DeckRow>)>,
    mut badges: Query<(&InDeckBadge, &mut Node, &Children)>,
    mut texts: Query<&mut Text>,
) {
    // chips mirror the filter state
    for (act, i, mut bg, mut bc, active_theme) in &mut chips {
        let on = match act {
            BuilderAction::ToggleColor(c) => search.colors[*c],
            BuilderAction::Cmc(v) => search.cmc == *v,
            BuilderAction::Kind(v) => search.kind == *v,
            BuilderAction::CommanderOnly => search.commander_only,
            BuilderAction::ToggleTheme(t) => {
                if recs.pinned.is_empty() {
                    active_theme
                } else {
                    recs.pinned.contains(t)
                }
            }
            _ => false,
        };
        let hov = matches!(i, Interaction::Hovered | Interaction::Pressed);
        let want_bg = if on { Color::srgba(0.95, 0.46, 0.13, 0.32) } else if hov { Color::srgba(1.0, 1.0, 1.0, 0.12) } else { Color::srgba(1.0, 1.0, 1.0, 0.05) };
        if bg.0 != want_bg {
            bg.0 = want_bg;
        }
        let want_bc = if on { ORANGE_HI } else { GOLD_DIM };
        if bc.top != want_bc {
            *bc = BorderColor::all(want_bc);
        }
    }
    // "in deck" badges on pool tiles
    for (b, mut node, children) in &mut badges {
        let cmd = deck.is_commander(&b.0);
        let q = deck.qty(&b.0);
        let want = if q > 0 || cmd { Display::Flex } else { Display::None };
        if node.display != want {
            node.display = want;
        }
        if (q > 0 || cmd)
            && let Some(c) = children.first()
            && let Ok(mut t) = texts.get_mut(*c)
        {
            let s = if cmd { "COMMANDER".to_string() } else { format!("IN DECK ×{q}") };
            if t.0 != s {
                t.0 = s;
            }
        }
    }
}

pub(super) fn panels_and_buttons(
    deck: Res<BuilderDeck>,
    ui: Res<BuilderUi>,
    mut panels: Query<(&TabPanel, &mut Node)>,
    mut tab_btns: Query<(&TabButton, &mut ArenaButton), (Without<PlayButton>, Without<SaveButton>)>,
    mut play: Query<&mut ArenaButton, (With<PlayButton>, Without<TabButton>, Without<SaveButton>)>,
    mut save: Query<&mut ArenaButton, (With<SaveButton>, Without<TabButton>, Without<PlayButton>)>,
) {
    for (p, mut n) in &mut panels {
        let want = if p.0 == ui.tab { Display::Flex } else { Display::None };
        if n.display != want {
            n.display = want;
        }
    }
    for (t, mut b) in &mut tab_btns {
        let want = if t.0 == ui.tab { ButtonKind::Primary } else { ButtonKind::Secondary };
        if b.kind != want {
            b.kind = want;
        }
    }
    let playable = !deck.loading && deck.problem_rev == deck.rev && deck.problem.is_none() && !ui.saving;
    for mut b in &mut play {
        if b.disabled == playable {
            b.disabled = !playable;
        }
    }
    for mut b in &mut save {
        let dis = ui.saving || deck.loading;
        if b.disabled != dis {
            b.disabled = dis;
        }
    }
}

/// Commander decks show the fixed colour identity instead of colour filter chips.
pub(super) fn identity_row(
    mut commands: Commands,
    deck: Res<BuilderDeck>,
    assets: Res<AssetServer>,
    mut ident: Query<(Entity, &mut Node), (With<IdentityRow>, Without<ColorFilterRow>, Without<CommanderChip>)>,
    mut color_row: Query<&mut Node, (With<ColorFilterRow>, Without<IdentityRow>, Without<CommanderChip>)>,
    mut cmd_chip: Query<&mut Node, (With<CommanderChip>, Without<ColorFilterRow>, Without<IdentityRow>)>,
    mut built: Local<Option<(Entity, Option<String>)>>,
) {
    let commander = deck.format.has_commander();
    let show = |on: bool| if on { Display::Flex } else { Display::None };
    for mut n in &mut color_row {
        if n.display != show(!commander) {
            n.display = show(!commander);
        }
    }
    for mut n in &mut cmd_chip {
        if n.display != show(commander) {
            n.display = show(commander);
        }
    }
    let identity = deck.identity();
    for (e, mut n) in &mut ident {
        if n.display != show(identity.is_some()) {
            n.display = show(identity.is_some());
        }
        if built.as_ref() != Some(&(e, identity.clone())) {
            *built = Some((e, identity.clone()));
            commands.entity(e).despawn_related::<Children>();
            if let Some(id) = &identity {
                commands.entity(e).with_children(|r| {
                    let letters: Vec<char> = if id.is_empty() { vec!['C'] } else { id.chars().collect() };
                    for c in letters {
                        r.spawn(mana_pip(&assets, c, 20.0));
                    }
                });
            }
        }
    }
}

pub(super) fn status_texts(
    deck: Res<BuilderDeck>,
    search: Res<SearchState>,
    recs: Res<RecsState>,
    ui: Res<BuilderUi>,
    stats: Res<BuilderStats>,
    mut q: ParamSet<(
        Query<(&mut Text, &mut TextColor), With<CountText>>,
        Query<(&mut Text, &mut TextColor), With<ProblemText>>,
        Query<(&mut Text, &mut TextColor), With<SearchStatus>>,
        Query<(&mut Text, &mut TextColor), With<RecStatus>>,
    )>,
) {
    let set = |t: &mut Text, c: &mut TextColor, s: String, col: Color| {
        if t.0 != s {
            t.0 = s;
        }
        if c.0 != col {
            c.0 = col;
        }
    };
    let n = deck.count();
    let req = stats.stats.required;
    let count_col = if (deck.format.has_commander() && n == req) || (!deck.format.has_commander() && n >= req) { GREEN_HEAL } else { GOLD_BRIGHT };
    for (mut t, mut c) in &mut q.p0() {
        set(&mut t, &mut c, format!("{n} / {req}"), count_col);
    }
    let (msg, col) = if deck.loading {
        ("Loading deck…".to_string(), TEXT_DIM)
    } else if ui.saving {
        ("Saving…".to_string(), TEXT_DIM)
    } else if deck.problem_rev != deck.rev {
        ("Checking with the engine…".to_string(), TEXT_DIM)
    } else if let Some(p) = &deck.problem {
        let mut c = p.chars();
        (c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default(), ORANGE_HI)
    } else {
        (format!("Legal {} deck{}", deck.format.as_str(), if deck.dirty { " · unsaved changes" } else { "" }), GREEN_HEAL)
    };
    for (mut t, mut c) in &mut q.p1() {
        set(&mut t, &mut c, msg.clone(), col);
    }
    let (msg, col) = if let Some(e) = &search.error {
        (format!("Search failed: {e}"), RED_DAMAGE)
    } else if search.busy {
        ("Searching…".to_string(), TEXT_DIM)
    } else if deck.format.has_commander() && deck.commanders.is_empty() {
        ("Choose your commander: click one below (search by name to find a specific one).".to_string(), GOLD_BRIGHT)
    } else if search.results.is_empty() {
        ("No cards match these filters.".to_string(), TEXT_DIM)
    } else {
        let id = deck.identity().map(|i| format!(" in {} identity", if i.is_empty() { "colourless".to_string() } else { i })).unwrap_or_default();
        (format!("{} cards{id} · click to add · right-click a legend to make it commander", search.results.len()), TEXT_DIM)
    };
    for (mut t, mut c) in &mut q.p2() {
        set(&mut t, &mut c, msg.clone(), col);
    }
    let (msg, col) = if let Some(e) = &recs.error {
        (format!("Recommendations unavailable: {e}"), RED_DAMAGE)
    } else if recs.busy || recs.dirty_at.is_some() {
        ("Finding cards for this deck…".to_string(), TEXT_DIM)
    } else {
        (String::new(), TEXT_DIM)
    };
    for (mut t, mut c) in &mut q.p3() {
        set(&mut t, &mut c, msg.clone(), col);
    }
}

pub(super) fn preview_and_toast(
    time: Res<Time>,
    mut ui: ResMut<BuilderUi>,
    mut cards: ResMut<CardImages>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut preview: Query<(&mut Node, &mut ImageNode), (With<Preview>, Without<ToastText>)>,
    mut toast: Query<(&mut Node, &Children), (With<ToastText>, Without<Preview>)>,
    mut texts: Query<&mut Text>,
) {
    let cursor = windows.single().ok().and_then(|w| w.cursor_position().map(|c| (c, Vec2::new(w.width(), w.height()))));
    for (mut n, mut img) in &mut preview {
        match (&ui.hover, cursor) {
            (Some(card), Some((c, win))) => {
                let h = cards.request_raw(ImageRequest { name: card.name.clone(), set: card.set.clone(), back: false, version: ImageVersion::Normal });
                if img.image != h {
                    img.image = h;
                }
                let (w, hgt) = (320.0, 447.0);
                // keep the preview beside the pointer, on whichever side has room
                let x = if c.x + 30.0 + w < win.x { c.x + 30.0 } else { (c.x - 30.0 - w).max(0.0) };
                let y = (c.y - hgt * 0.5).clamp(8.0, (win.y - hgt - 8.0).max(8.0));
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
    let now = time.elapsed_secs();
    if ui.toast.as_ref().is_some_and(|(_, t)| now - t > 3.5) {
        ui.toast = None;
    }
    for (mut n, ch) in &mut toast {
        let want = if ui.toast.is_some() { Display::Flex } else { Display::None };
        if n.display != want {
            n.display = want;
        }
        if let Some((msg, _)) = &ui.toast
            && let Some(c) = ch.first()
            && let Ok(mut t) = texts.get_mut(*c)
            && t.0 != *msg
        {
            t.0 = msg.clone();
        }
    }
}

pub(super) fn wheel_scroll(mut wheel: MessageReader<MouseWheel>, mut q: Query<(&RelativeCursorPosition, &ComputedNode, &mut ScrollPosition), With<Scroll>>) {
    let mut dy = 0.0;
    for e in wheel.read() {
        dy += match e.unit {
            MouseScrollUnit::Line => e.y * 48.0,
            MouseScrollUnit::Pixel => e.y,
        };
    }
    if dy == 0.0 {
        return;
    }
    for (rel, node, mut sp) in &mut q {
        if !rel.cursor_over {
            continue;
        }
        let max = ((node.content_size().y - node.size().y) * node.inverse_scale_factor()).max(0.0);
        sp.y = (sp.y - dy).clamp(0.0, max);
    }
}
