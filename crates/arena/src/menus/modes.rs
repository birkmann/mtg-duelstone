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

#[derive(Component)]
struct FormatTile(Format);
#[derive(Component)]
struct LegalityChip(Option<String>);
#[derive(Component)]
struct VariantToggle(Variant);
#[derive(Component)]
struct SummaryText;
#[derive(Component)]
struct LegalityRow;

/// MatchConfig.setup starts from what the player chose last time.
fn restore_choice(settings: Res<Settings>, mut cfg: ResMut<MatchConfig>) {
    cfg.format = if settings.last_format == Format::Limited { Format::Constructed } else { settings.last_format };
    cfg.setup.legality = settings.last_legality.clone().filter(|_| cfg.format == Format::Constructed);
    cfg.setup.variants = settings.last_variants.clone();
}

fn tile(parent: &mut ChildSpawnerCommands, fonts: &ArenaFonts, title: &str, desc: &str, w: f32) -> Entity {
    parent
        .spawn((
            Button,
            Node {
                width: Val::Px(w),
                min_height: Val::Px(96.0),
                padding: UiRect::all(Val::Px(14.0)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(6.0),
                border: UiRect::all(Val::Px(2.0)),
                border_radius: BorderRadius::all(Val::Px(12.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.04)),
            BorderColor::all(GOLD_DIM),
        ))
        .with_children(|t| {
            t.spawn((ui_text(&fonts.title, title, 22.0, PARCHMENT), Pickable::IGNORE));
            t.spawn((ui_text(&fonts.body, desc, 14.0, TEXT_DIM), Pickable::IGNORE));
        })
        .id()
}

fn section(parent: &mut ChildSpawnerCommands, fonts: &ArenaFonts, title: &str, f: impl FnOnce(&mut ChildSpawnerCommands)) {
    parent
        .spawn(ornate_panel(Node { width: Val::Percent(100.0), padding: UiRect::all(Val::Px(16.0)), flex_direction: FlexDirection::Column, row_gap: Val::Px(10.0), ..default() }))
        .with_children(|p| {
            p.spawn(ui_text(&fonts.title, title, 20.0, GOLD));
            p.spawn(Node { flex_wrap: FlexWrap::Wrap, column_gap: Val::Px(12.0), row_gap: Val::Px(12.0), align_items: AlignItems::Stretch, ..default() }).with_children(f);
        });
}

fn spawn_screen(mut commands: Commands, fonts: Res<ArenaFonts>, mode: Res<MatchMode>) {
    let subtitle = match *mode {
        MatchMode::Host { players } => format!("Hosting a table for {players}"),
        _ => "Against the Forge AI".to_string(),
    };
    commands
        .spawn((
            Name::new("ModeSelect"),
            DespawnOnExit(AppState::ModeSelect),
            Node { flex_direction: FlexDirection::Column, padding: UiRect::all(Val::Px(26.0)), row_gap: Val::Px(14.0), align_items: AlignItems::Center, ..fullscreen_root() },
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
            root.spawn((
                Node { width: Val::Px(1180.0), max_width: Val::Percent(100.0), flex_direction: FlexDirection::Column, row_gap: Val::Px(12.0), flex_grow: 1.0, min_height: Val::Px(0.0), overflow: Overflow::scroll_y(), ..default() },
                ScrollPosition::default(),
                bevy::ui::RelativeCursorPosition::default(),
                super::widgets::WheelScroll,
            ))
            .with_children(|col| {
                section(col, &fonts, "FORMAT", |r| {
                    for (f, desc) in [
                        (Format::Constructed, "60-card decks, 20 life. Pick a legality below or play anything."),
                        (Format::Commander, "100-card singleton led by a legendary commander. 40 life."),
                        (Format::Brawl, "60-card singleton commander deck from Standard cards. 25 life."),
                        (Format::Oathbreaker, "60-card singleton led by a planeswalker and its signature spell."),
                    ] {
                        let t = tile(r, &fonts, f.label(), desc, 278.0);
                        r.commands().entity(t).insert((FormatTile(f), MenuAction::SetFormat(f)));
                    }
                });
                section(col, &fonts, "LEGALITY (CONSTRUCTED)", |r| {
                    r.spawn((LegalityRow, Node { column_gap: Val::Px(10.0), flex_wrap: FlexWrap::Wrap, row_gap: Val::Px(10.0), ..default() })).with_children(|r| {
                        let b = spawn_button(r, &fonts, ButtonKind::Secondary, "CASUAL", Vec2::new(150.0, 46.0));
                        r.commands().entity(b).insert((LegalityChip(None), MenuAction::SetLegality(None)));
                        for (id, name) in LEGALITIES {
                            let b = spawn_button(r, &fonts, ButtonKind::Secondary, &name.to_uppercase(), Vec2::new(150.0, 46.0));
                            r.commands().entity(b).insert((LegalityChip(Some(id.to_string())), MenuAction::SetLegality(Some(id.to_string()))));
                        }
                    });
                });
                section(col, &fonts, "CASUAL VARIANTS (OPTIONAL)", |r| {
                    for (v, desc) in [
                        (Variant::Planechase, "Planar decks and the planar die: the battlefield itself changes."),
                        (Variant::Archenemy, "One player runs a scheme deck against everyone else."),
                        (Variant::Momir, "No decklist: discard a card to make a random creature of that mana value."),
                        (Variant::Teams, "Four players in two teams; a team wins together (each player keeps their own life)."),
                    ] {
                        let t = tile(r, &fonts, v.label(), desc, 278.0);
                        r.commands().entity(t).insert((VariantToggle(v), MenuAction::ToggleVariant(v)));
                    }
                });
                section(col, &fonts, "LIMITED", |r| {
                    for (k, desc) in [
                        (EventKind::CubeDraft, "Draft a cube — Legacy, Vintage, Pauper or your own — against AI drafters and friends."),
                        (EventKind::SetDraft, "Booster draft a real Magic set."),
                        (EventKind::Sealed, "Open six packs of a cube or set and build from them."),
                    ] {
                        let t = tile(r, &fonts, k.label(), desc, 375.0);
                        r.commands().entity(t).insert(MenuAction::OpenEvent(k));
                    }
                });
            });
            root.spawn(Node { width: Val::Px(1180.0), max_width: Val::Percent(100.0), align_items: AlignItems::Center, column_gap: Val::Px(16.0), ..default() }).with_children(|bar| {
                bar.spawn((SummaryText, Node { flex_grow: 1.0, ..default() }, ui_text(&fonts.body, "", 18.0, TEXT)));
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

fn refresh(
    cfg: Res<MatchConfig>,
    mode: Res<MatchMode>,
    mut tiles: Query<(&FormatTile, &mut BorderColor, &mut BackgroundColor), (Without<VariantToggle>, Without<LegalityChip>)>,
    mut toggles: Query<(&VariantToggle, &mut BorderColor, &mut BackgroundColor, &mut Node), Without<FormatTile>>,
    mut chips: Query<(&LegalityChip, &mut ArenaButton)>,
    mut summary: Query<&mut Text, With<SummaryText>>,
) {
    if !cfg.is_changed() && !mode.is_changed() {
        return;
    }
    let pick = |on: bool| (if on { ORANGE } else { GOLD_DIM }, if on { Color::srgba(0.95, 0.46, 0.13, 0.18) } else { Color::srgba(1.0, 1.0, 1.0, 0.04) });
    for (t, mut bc, mut bg) in &mut tiles {
        let (b, g) = pick(t.0 == cfg.format);
        *bc = BorderColor::all(b);
        bg.0 = g;
    }
    let table = match *mode {
        MatchMode::Host { players } => Some(players),
        _ => None,
    };
    for (v, mut bc, mut bg, mut node) in &mut toggles {
        let (b, g) = pick(cfg.setup.variants.contains(&v.0));
        *bc = BorderColor::all(b);
        bg.0 = g;
        // Teams need exactly four seats: a hosted table of four (vs AI it fills three AI seats)
        let show = v.0 != Variant::Teams || table.is_none_or(|n| n == 4);
        let want = if show { Display::Flex } else { Display::None };
        if node.display != want {
            node.display = want;
        }
    }
    for (c, mut b) in &mut chips {
        let on = cfg.format == Format::Constructed && c.0 == cfg.setup.legality;
        b.kind = if on { ButtonKind::Primary } else { ButtonKind::Secondary };
    }
    for mut t in &mut summary {
        t.0 = setup_label(cfg.format, &cfg.setup);
    }
}
