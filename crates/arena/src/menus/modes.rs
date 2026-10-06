//! Mode select: what kind of game to play — before choosing a deck.
//!
//! Why its own screen: with Constructed legalities (Standard … Pauper), three commander-style formats,
//! four casual variants and three limited events, a two-button format toggle on the deck screen no
//! longer fits. Every choice here only edits `MatchConfig.setup` / `Settings` (and is remembered); the
//! deck screen then lists decks for that format and legality. Limited tiles lead to the event setup
//! instead, since their decks come out of a draft or sealed pool.

use bevy::prelude::*;
use mtg_protocol::{EventKind, Format, LEGALITIES, Variant};

use super::MenuAction;
use crate::shared::*;
use crate::style::*;

pub struct ModesPlugin;

impl Plugin for ModesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::ModeSelect), (restore_choice, spawn_screen).chain())
            .add_systems(Update, (handle_mode_actions, refresh).chain().run_if(in_state(AppState::ModeSelect)));
    }
}

/// A choice tile: one of the formats (pick one), a variant (any combination) or a limited event
/// (opens its own setup screen).
#[derive(Component, Clone, Copy, PartialEq)]
enum Tile {
    Format(Format),
    Variant(Variant),
    Event,
}
/// The dot inside a tile's radio/checkbox, lit while the tile is chosen.
#[derive(Component)]
struct TileMark(Tile);
/// The radio/checkbox outline around a `TileMark`.
#[derive(Component)]
struct TileRing(Tile);
#[derive(Component)]
struct TileTitle(Tile);
#[derive(Component)]
struct LegalityChip(Option<String>);
#[derive(Component)]
struct SummaryText;

const TILE_BG: Color = Color::srgba(1.0, 1.0, 1.0, 0.035);
const TILE_BG_HOVER: Color = Color::srgba(1.0, 0.9, 0.7, 0.07);
const TILE_BG_ON: Color = Color::srgba(0.95, 0.46, 0.13, 0.16);

/// MatchConfig.setup starts from what the player chose last time.
fn restore_choice(settings: Res<Settings>, mut cfg: ResMut<MatchConfig>) {
    cfg.format = if settings.last_format == Format::Limited { Format::Constructed } else { settings.last_format };
    cfg.setup.legality = settings.last_legality.clone().filter(|_| cfg.format == Format::Constructed);
    cfg.setup.variants = settings.last_variants.clone();
}

/// Radio dot for formats, checkbox for variants, arrow for events: the shape tells pick-one from
/// combine-freely before anything is clicked.
fn tile(parent: &mut ChildSpawnerCommands, fonts: &ArenaFonts, kind: Tile, title: &str, desc: &str) -> Entity {
    parent
        .spawn((
            Button,
            kind,
            Node {
                padding: UiRect::axes(Val::Px(16.0), Val::Px(12.0)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(6.0),
                border: UiRect::all(Val::Px(2.0)),
                border_radius: BorderRadius::all(Val::Px(12.0)),
                ..default()
            },
            BackgroundColor(TILE_BG),
            BorderColor::all(GOLD_DIM),
        ))
        .with_children(|t| {
            t.spawn((Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, column_gap: Val::Px(10.0), ..default() }, Pickable::IGNORE))
                .with_children(|row| {
                    row.spawn((TileTitle(kind), ui_text(&fonts.title, title, 21.0, PARCHMENT), Pickable::IGNORE));
                    let radius = match kind {
                        Tile::Format(_) => 99.0,
                        _ => 5.0,
                    };
                    if kind == Tile::Event {
                        row.spawn((ui_text(&fonts.body, "→", 20.0, GOLD), Pickable::IGNORE));
                    } else {
                        row.spawn((
                            Node {
                                width: Val::Px(20.0),
                                height: Val::Px(20.0),
                                flex_shrink: 0.0,
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                border: UiRect::all(Val::Px(2.0)),
                                border_radius: BorderRadius::all(Val::Px(radius)),
                                ..default()
                            },
                            BorderColor::all(GOLD_DIM),
                            TileRing(kind),
                            Pickable::IGNORE,
                        ))
                        .with_children(|m| {
                            m.spawn((
                                TileMark(kind),
                                Node { width: Val::Px(10.0), height: Val::Px(10.0), border_radius: BorderRadius::all(Val::Px(radius)), ..default() },
                                BackgroundColor(Color::NONE),
                                Pickable::IGNORE,
                            ));
                        });
                    }
                });
            t.spawn((ui_text(&fonts.body, desc, 14.0, TEXT_DIM), Pickable::IGNORE));
        })
        .id()
}

/// A panel with a title, a hint on the right and an even grid of `columns`: every row fills the
/// panel's width, so no tile ends up alone on a line.
fn section(parent: &mut ChildSpawnerCommands, fonts: &ArenaFonts, title: &str, hint: &str, columns: u16, f: impl FnOnce(&mut ChildSpawnerCommands)) {
    parent
        .spawn(ornate_panel(Node {
            width: Val::Percent(100.0),
            flex_shrink: 0.0,
            padding: UiRect::new(Val::Px(18.0), Val::Px(18.0), Val::Px(12.0), Val::Px(16.0)),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(12.0),
            ..default()
        }))
        .with_children(|p| {
            p.spawn(Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Baseline, column_gap: Val::Px(16.0), ..default() }).with_children(|h| {
                h.spawn(ui_text(&fonts.title, title, 19.0, GOLD));
                h.spawn(ui_text(&fonts.body, hint, 14.0, TEXT_DIM));
            });
            p.spawn(Node {
                display: Display::Grid,
                grid_template_columns: RepeatedGridTrack::flex(columns, 1.0),
                column_gap: Val::Px(12.0),
                row_gap: Val::Px(12.0),
                ..default()
            })
            .with_children(f);
        });
}

/// A legality chip: a secondary button that fills its grid cell.
fn legality_chip(parent: &mut ChildSpawnerCommands, fonts: &ArenaFonts, label: &str, legality: Option<String>) {
    parent
        .spawn((button(ButtonKind::Secondary, Node { height: Val::Px(46.0), ..default() }), LegalityChip(legality.clone()), MenuAction::SetLegality(legality)))
        .with_children(|b| {
            b.spawn((
                Text::new(label),
                text_font(&fonts.title, 18.0),
                TextColor(GOLD_BRIGHT),
                TextShadow { offset: Vec2::new(0.0, 1.5), color: Color::srgba(0.0, 0.0, 0.0, 0.7) },
            ));
        });
}

fn spawn_screen(mut commands: Commands, fonts: Res<ArenaFonts>, mode: Res<MatchMode>, facts: Res<ServerFacts>) {
    let public = facts.is_public();
    let subtitle = match *mode {
        MatchMode::Host { players } => format!("Hosting a table for {players}"),
        _ => "Against the Forge AI".to_string(),
    };
    const WIDTH: f32 = 1180.0;
    commands
        .spawn((
            Name::new("ModeSelect"),
            DespawnOnExit(AppState::ModeSelect),
            Node { flex_direction: FlexDirection::Column, padding: UiRect::axes(Val::Px(26.0), Val::Px(20.0)), row_gap: Val::Px(12.0), align_items: AlignItems::Center, ..fullscreen_root() },
        ))
        .with_children(|root| {
            root.spawn(Node { width: Val::Percent(100.0), align_items: AlignItems::Center, column_gap: Val::Px(16.0), ..default() }).with_children(|bar| {
                let b = spawn_button(bar, &fonts, ButtonKind::Secondary, "BACK", Vec2::new(130.0, 46.0));
                bar.commands().entity(b).insert(MenuAction::ModeBack);
                bar.spawn(Node { flex_grow: 1.0, flex_direction: FlexDirection::Column, align_items: AlignItems::Center, ..default() }).with_children(|c| {
                    c.spawn(heading(&fonts, "CHOOSE A MODE", 42.0));
                    c.spawn(ui_text(&fonts.body, subtitle, 16.0, TEXT_DIM));
                });
                bar.spawn(Node { width: Val::Px(130.0), ..default() });
            });
            // scrolls only when the window is too short for all four panels
            root.spawn((
                Node { width: Val::Px(WIDTH), max_width: Val::Percent(100.0), flex_direction: FlexDirection::Column, row_gap: Val::Px(12.0), flex_grow: 1.0, min_height: Val::Px(0.0), overflow: Overflow::scroll_y(), ..default() },
                ScrollPosition::default(),
                bevy::ui::RelativeCursorPosition::default(),
                super::widgets::WheelScroll,
            ))
            .with_children(|col| {
                section(col, &fonts, "FORMAT", "Pick one", 4, |r| {
                    for (f, desc) in [
                        (Format::Constructed, "60-card decks, 20 life. Choose a legality below or play anything."),
                        (Format::Commander, "100-card singleton led by a legendary commander. 40 life."),
                        (Format::Brawl, "60-card singleton commander deck of Standard cards. 25 life."),
                        (Format::Oathbreaker, "A planeswalker and its signature spell lead a singleton deck."),
                    ] {
                        let t = tile(r, &fonts, Tile::Format(f), f.label(), desc);
                        r.commands().entity(t).insert(MenuAction::SetFormat(f));
                    }
                });
                section(col, &fonts, "LEGALITY", "Constructed only: which cards decks may use", 1 + LEGALITIES.len() as u16, |r| {
                    legality_chip(r, &fonts, "CASUAL", None);
                    for (id, name) in LEGALITIES {
                        legality_chip(r, &fonts, &name.to_uppercase(), Some(id.to_string()));
                    }
                });
                section(col, &fonts, "VARIANTS", "Optional: combine any, except Archenemy with 2v2", 4, |r| {
                    for (v, desc) in [
                        (Variant::Planechase, "Planar decks and the planar die: the battlefield itself changes."),
                        (Variant::Archenemy, "One player runs a scheme deck against everyone else."),
                        (Variant::Momir, "No decks: discard a card, get a random creature of that cost."),
                        (Variant::Teams, "Two against two: a team wins together, each with its own life."),
                    ] {
                        let t = tile(r, &fonts, Tile::Variant(v), v.label(), desc);
                        r.commands().entity(t).insert(MenuAction::ToggleVariant(v));
                    }
                });
                // events keep a pod and its decks on the engine, which a public host doesn't offer
                if !public {
                    section(col, &fonts, "LIMITED", "Draft or open packs, then build a 40-card deck", 3, |r| {
                        for (k, desc) in [
                            (EventKind::CubeDraft, "Draft a cube (Legacy, Vintage, Pauper or your own) against AI drafters and friends."),
                            (EventKind::SetDraft, "Booster draft a real Magic set."),
                            (EventKind::Sealed, "Open six packs of a cube or set and build from what you get."),
                        ] {
                            let t = tile(r, &fonts, Tile::Event, k.label(), desc);
                            r.commands().entity(t).insert(MenuAction::OpenEvent(k));
                        }
                    });
                }
            });
            root.spawn(Node { width: Val::Px(WIDTH), max_width: Val::Percent(100.0), align_items: AlignItems::Center, column_gap: Val::Px(16.0), padding: UiRect::horizontal(Val::Px(4.0)), ..default() }).with_children(|bar| {
                bar.spawn(Node { flex_grow: 1.0, flex_direction: FlexDirection::Column, row_gap: Val::Px(4.0), ..default() }).with_children(|c| {
                    c.spawn(ui_text(&fonts.body, "YOUR GAME", 13.0, TEXT_DIM));
                    c.spawn((SummaryText, ui_text(&fonts.title, "", 22.0, GOLD_BRIGHT)));
                });
                let b = spawn_button(bar, &fonts, ButtonKind::Primary, "CHOOSE DECK", Vec2::new(280.0, 64.0));
                bar.commands().entity(b).insert(MenuAction::ModeContinue);
            });
        });
}

fn handle_mode_actions(
    q: Query<(&Interaction, &MenuAction), Changed<Interaction>>,
    mut cfg: ResMut<MatchConfig>,
    mut settings: ResMut<Settings>,
    mode: Res<MatchMode>,
    mut next: ResMut<NextState<AppState>>,
    mut event_kind: ResMut<EventSetupChoice>,
) {
    for (i, act) in &q {
        if *i != Interaction::Pressed {
            continue;
        }
        match act {
            MenuAction::SetFormat(f) => {
                cfg.format = *f;
                if *f != Format::Constructed {
                    cfg.setup.legality = None;
                }
            }
            MenuAction::SetLegality(l) => {
                cfg.format = Format::Constructed;
                cfg.setup.legality = l.clone();
            }
            MenuAction::ToggleVariant(v) => {
                if let Some(p) = cfg.setup.variants.iter().position(|x| x == v) {
                    cfg.setup.variants.remove(p);
                } else {
                    cfg.setup.variants.push(*v);
                    // Teams and Archenemy both decide who plays with whom: one at a time
                    let other = match v {
                        Variant::Teams => Some(Variant::Archenemy),
                        Variant::Archenemy => Some(Variant::Teams),
                        _ => None,
                    };
                    cfg.setup.variants.retain(|x| Some(*x) != other);
                    cfg.setup.variants.sort();
                }
            }
            MenuAction::OpenEvent(k) => {
                event_kind.0 = *k;
                next.set(AppState::EventSetup);
            }
            MenuAction::ModeBack => {
                next.set(if mode.is_multiplayer() { AppState::Multiplayer } else { AppState::MainMenu });
            }
            MenuAction::ModeContinue => {
                settings.last_format = cfg.format;
                settings.last_legality = cfg.setup.legality.clone();
                settings.last_variants = cfg.setup.variants.clone();
                if cfg.setup.variants.iter().any(|v| v.ignores_decks()) {
                    // Momir decks are generated by the engine: no deck to choose
                    cfg.human_deck = "random".into();
                    cfg.ai_deck = "random".into();
                    next.set(AppState::Loading);
                } else {
                    next.set(AppState::DeckSelect);
                }
            }
            _ => {}
        }
    }
}

/// Human-readable summary of a setup, e.g. "Constructed · Pauper · Planechase".
pub fn setup_label(format: Format, setup: &mtg_protocol::GameSetup) -> String {
    let mut parts = vec![format.label().to_string()];
    if let Some(l) = &setup.legality {
        parts.push(LEGALITIES.iter().find(|(id, _)| id == l).map(|(_, n)| n.to_string()).unwrap_or_else(|| l.clone()));
    }
    for v in &setup.variants {
        parts.push(v.label().to_string());
    }
    parts.join("  ·  ")
}

/// Runs every frame for the hover look; `set_if_neq` keeps it from touching what didn't change.
#[allow(clippy::type_complexity)]
fn refresh(
    cfg: Res<MatchConfig>,
    mode: Res<MatchMode>,
    mut tiles: Query<(&Tile, &Interaction, &mut BorderColor, &mut BackgroundColor, &mut Node)>,
    mut marks: Query<(&TileMark, &mut BackgroundColor), Without<Tile>>,
    mut rings: Query<(&TileRing, &mut BorderColor), Without<Tile>>,
    mut titles: Query<(&TileTitle, &mut TextColor)>,
    mut chips: Query<(&LegalityChip, &mut ArenaButton)>,
    mut summary: Query<&mut Text, With<SummaryText>>,
) {
    let chosen = |t: Tile| match t {
        Tile::Format(f) => f == cfg.format,
        Tile::Variant(v) => cfg.setup.variants.contains(&v),
        Tile::Event => false,
    };
    // Teams need exactly four seats: a hosted table of four (vs AI it fills three AI seats)
    let table = match *mode {
        MatchMode::Host { players } => Some(players),
        _ => None,
    };
    for (&t, i, mut bc, mut bg, mut node) in &mut tiles {
        let (on, hover) = (chosen(t), *i != Interaction::None);
        bc.set_if_neq(BorderColor::all(match (on, hover) {
            (true, _) => ORANGE,
            (false, true) => GOLD,
            _ => GOLD_DIM,
        }));
        bg.set_if_neq(BackgroundColor(if on { TILE_BG_ON } else if hover { TILE_BG_HOVER } else { TILE_BG }));
        if t == Tile::Variant(Variant::Teams) {
            let want = if table.is_none_or(|n| n == 4) { Display::Flex } else { Display::None };
            if node.display != want {
                node.display = want;
            }
        }
    }
    for (m, mut bg) in &mut marks {
        bg.set_if_neq(BackgroundColor(if chosen(m.0) { ORANGE_HI } else { Color::NONE }));
    }
    for (r, mut bc) in &mut rings {
        bc.set_if_neq(BorderColor::all(if chosen(r.0) { ORANGE_HI } else { GOLD_DIM }));
    }
    for (t, mut c) in &mut titles {
        c.set_if_neq(TextColor(if chosen(t.0) { GOLD_BRIGHT } else { PARCHMENT }));
    }
    if !cfg.is_changed() && !mode.is_changed() {
        return;
    }
    for (c, mut b) in &mut chips {
        let on = cfg.format == Format::Constructed && c.0 == cfg.setup.legality;
        b.kind = if on { ButtonKind::Primary } else { ButtonKind::Secondary };
    }
    for mut t in &mut summary {
        t.0 = setup_label(cfg.format, &cfg.setup);
    }
}
