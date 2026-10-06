//! Deck art picker: the ART chip on a deck select tile opens a modal of every card in that deck as
//! an art crop; the pick becomes the tile's face (`DeckLook::art`, saved with the settings).
//!
//! Why client-side settings and not the deck file: precons and themes are read-only on the engine,
//! and the art is a cosmetic choice like sleeves, which already live per deck in `deck_looks`.
//! Without a pick the tile shows the engine's `DeckInfo::cover` (commander or signature card), else
//! the first of the deck's `art_choices` whose art loads.

use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use crossbeam_channel::Receiver;
use mtg_client::{Api, ImageRequest, ImageVersion};
use mtg_protocol::{CatalogCard, DeckFull};

use super::deck_select::{ArtFade, DeckList, DeckSelection, Fetch, rest_base};
use crate::images::CardImages;
use crate::shared::{AppState, ArenaFonts, EngineConfig, Settings};
use crate::style::*;

pub struct DeckArtPlugin;

impl Plugin for DeckArtPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ArtPicker>()
            .add_systems(OnExit(AppState::DeckSelect), |mut p: ResMut<ArtPicker>| *p = ArtPicker::default())
            .add_systems(
                Update,
                (debug_open, chip_clicks, poll_cards, sync_panel, fill_grid, pick_clicks, close_on_escape, chip_visuals, thumb_visuals, wheel_scroll)
                    .chain()
                    .run_if(in_state(AppState::DeckSelect)),
            );
    }
}

/// `random` is generated at match start, so it has no cards to choose from.
pub(super) fn can_pick(deck_ref: &str) -> bool {
    deck_ref != "random"
}

#[derive(Default)]
enum Cards {
    #[default]
    Loading,
    Ready(Vec<String>),
    Error(String),
}

#[derive(Resource, Default)]
pub(super) struct ArtPicker {
    /// `(ref, name)` of the deck being dressed; `None` = closed.
    deck: Option<(String, String)>,
    cards: Cards,
    rx: Option<Receiver<Result<DeckFull, String>>>,
}

impl ArtPicker {
    pub(super) fn is_open(&self) -> bool {
        self.deck.is_some()
    }
}

#[derive(Component)]
struct ArtChip(String);
#[derive(Component)]
struct PickerPanel;
#[derive(Component)]
struct PickerGrid;
#[derive(Component)]
struct PickerStatus;
#[derive(Component, Clone)]
enum PickAction {
    Card(String),
    Default,
    Close,
}

/// The small ART pill in a tile's top-right corner.
pub(super) fn art_chip(deck_ref: &str, fonts: &ArenaFonts) -> impl Bundle {
    (
        ArtChip(deck_ref.to_string()),
        Button,
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(8.0),
            right: Val::Px(8.0),
            padding: UiRect::axes(Val::Px(10.0), Val::Px(4.0)),
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(99.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.05, 0.03, 0.02, 0.55)),
        BorderColor::all(GOLD_DIM),
        children![(ui_text(&fonts.title, "ART", 12.0, GOLD), Pickable::IGNORE)],
    )
}

fn open(picker: &mut ArtPicker, deck_ref: String, list: &DeckList, base: String) {
    let fmt = list.format;
    picker.deck = Some((deck_ref.clone(), list.name_of(&deck_ref)));
    picker.cards = Cards::Loading;
    picker.rx = Some(mtg_client::spawn(async move { Api::new(base).deck(&deck_ref, fmt).await.map_err(|e| format!("{e:#}")) }));
}

fn chip_clicks(
    q: Query<(&Interaction, &ArtChip), Changed<Interaction>>,
    mut picker: ResMut<ArtPicker>,
    list: Res<DeckList>,
    cfg: Option<Res<EngineConfig>>,
    settings: Res<Settings>,
) {
    for (i, chip) in &q {
        if *i == Interaction::Pressed && !picker.is_open() {
            open(&mut picker, chip.0.clone(), &list, rest_base(&cfg, &settings));
        }
    }
}

/// Dev aid for unattended screenshots: `ARENA_DECK_ART=1` (with `ARENA_MENU_SCREEN=deck`) opens the
/// picker on the selected deck once the list has loaded.
fn debug_open(
    mut done: Local<bool>,
    mut picker: ResMut<ArtPicker>,
    list: Res<DeckList>,
    sel: Res<DeckSelection>,
    cfg: Option<Res<EngineConfig>>,
    settings: Res<Settings>,
) {
    if *done || list.status != Fetch::Ready {
        return;
    }
    *done = true;
    if std::env::var("ARENA_DECK_ART").is_ok()
        && let Some(r) = sel.human.clone()
    {
        open(&mut picker, r, &list, rest_base(&cfg, &settings));
    }
}

/// Every distinct card worth looking at: commanders, then creatures and planeswalkers, other
/// spells, and non-basic lands (basics all look alike).
pub(super) fn art_choices(deck: &DeckFull) -> Vec<String> {
    let rank = |c: &CatalogCard| {
        let t = c.type_line.to_lowercase();
        if c.is_land { 3 } else if t.contains("creature") || t.contains("planeswalker") { 1 } else { 2 }
    };
    let mut rest: Vec<&CatalogCard> = deck.main.iter().chain(&deck.sideboard).map(|e| &e.card).filter(|c| !c.type_line.contains("Basic")).collect();
    rest.sort_by(|a, b| rank(a).cmp(&rank(b)).then_with(|| a.name.cmp(&b.name)));
    let mut out: Vec<String> = Vec::new();
    for c in deck.commanders.iter().chain(rest) {
        if !out.contains(&c.name) {
            out.push(c.name.clone());
        }
    }
    out
}

fn poll_cards(mut picker: ResMut<ArtPicker>) {
    let Some(rx) = &picker.rx else { return };
    let Ok(res) = rx.try_recv() else { return };
    picker.rx = None;
    picker.cards = match res {
        Ok(deck) => Cards::Ready(art_choices(&deck)),
        Err(e) => Cards::Error(e),
    };
}

fn sync_panel(mut commands: Commands, picker: Res<ArtPicker>, fonts: Res<ArenaFonts>, existing: Query<Entity, With<PickerPanel>>) {
    if !picker.is_changed() {
        return;
    }
    match (&picker.deck, existing.is_empty()) {
        (Some((_, name)), true) => {
            commands
                .spawn((
                    PickerPanel,
                    DespawnOnExit(AppState::DeckSelect),
                    GlobalZIndex(100),
                    Button, // swallows clicks so tiles underneath don't react
                    Node { justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..fullscreen_root() },
                    BackgroundColor(SCRIM),
                ))
                .with_children(|scrim| {
                    scrim
                        .spawn(ornate_panel(Node {
                            width: Val::Px(1040.0),
                            max_width: Val::Percent(94.0),
                            height: Val::Percent(86.0),
                            padding: UiRect::all(Val::Px(26.0)),
                            flex_direction: FlexDirection::Column,
                            row_gap: Val::Px(10.0),
                            ..default()
                        }))
                        .with_children(|p| {
                            p.spawn(heading(&fonts, format!("DECK ART  ·  {name}"), 30.0));
                            p.spawn(divider());
                            p.spawn((PickerStatus, ui_text(&fonts.body, "Loading cards…", 15.0, TEXT_DIM)));
                            p.spawn((
                                PickerGrid,
                                ScrollPosition::default(),
                                RelativeCursorPosition::default(),
                                Node {
                                    width: Val::Percent(100.0),
                                    flex_grow: 1.0,
                                    min_height: Val::Px(0.0),
                                    flex_wrap: FlexWrap::Wrap,
                                    justify_content: JustifyContent::Center,
                                    align_content: AlignContent::FlexStart,
                                    padding: UiRect::all(Val::Px(8.0)),
                                    row_gap: Val::Px(14.0),
                                    column_gap: Val::Px(14.0),
                                    overflow: Overflow::scroll_y(),
                                    ..default()
                                },
                            ));
                            p.spawn(Node { justify_content: JustifyContent::FlexEnd, column_gap: Val::Px(12.0), margin: UiRect::top(Val::Px(6.0)), ..default() }).with_children(|r| {
                                let b = spawn_button(r, &fonts, ButtonKind::Ghost, "USE DEFAULT", Vec2::new(200.0, 48.0));
                                r.commands().entity(b).insert(PickAction::Default);
                                let b = spawn_button(r, &fonts, ButtonKind::Secondary, "CLOSE", Vec2::new(150.0, 48.0));
                                r.commands().entity(b).insert(PickAction::Close);
                            });
                        });
                });
        }
        (None, false) => {
            for e in &existing {
                commands.entity(e).despawn();
            }
        }
        _ => {}
    }
}

/// Fills the grid once the deck's cards arrive (the panel may be spawned the same frame).
fn fill_grid(
    mut commands: Commands,
    picker: Res<ArtPicker>,
    fonts: Res<ArenaFonts>,
    mut images: ResMut<CardImages>,
    grid: Query<(Entity, Option<&Children>), With<PickerGrid>>,
    added: Query<(), Added<PickerGrid>>,
    mut status: Query<(&mut Text, &mut TextColor), With<PickerStatus>>,
) {
    if !picker.is_changed() && added.is_empty() {
        return;
    }
    let (msg, col) = match &picker.cards {
        Cards::Loading => ("Loading cards…".to_string(), TEXT_DIM),
        Cards::Ready(c) if c.is_empty() => ("This deck has no cards to take art from.".into(), TEXT_DIM),
        Cards::Ready(_) => ("Pick the card whose art stands for this deck.".into(), TEXT_DIM),
        Cards::Error(e) => (format!("Could not load the deck: {e}"), RED_DAMAGE),
    };
    for (mut t, mut c) in &mut status {
        t.0 = msg.clone();
        c.0 = col;
    }
    let Cards::Ready(cards) = &picker.cards else { return };
    let Ok((e, ch)) = grid.single() else { return };
    if ch.is_some_and(|c| !c.is_empty()) {
        return;
    }
    commands.entity(e).with_children(|g| {
        for name in cards {
            let h = images.request_raw(ImageRequest { name: name.clone(), set: None, back: false, version: ImageVersion::ArtCrop });
            g.spawn((
                PickAction::Card(name.clone()),
                Button,
                Node {
                    width: Val::Px(176.0),
                    flex_direction: FlexDirection::Column,
                    border: UiRect::all(Val::Px(2.0)),
                    border_radius: BorderRadius::all(Val::Px(10.0)),
                    overflow: Overflow::clip(),
                    flex_shrink: 0.0,
                    ..default()
                },
                BackgroundColor(Color::srgb(0.10, 0.065, 0.045)),
                BorderColor::all(GOLD_DIM),
                UiTransform::IDENTITY,
            ))
            .with_children(|t| {
                t.spawn((
                    ArtFade(h.clone()),
                    Node { width: Val::Percent(100.0), height: Val::Px(128.0), ..default() },
                    ImageNode { image: h, color: Color::srgba(1.0, 1.0, 1.0, 0.0), image_mode: NodeImageMode::Auto, ..default() },
                    Pickable::IGNORE,
                ));
                t.spawn((Node { padding: UiRect::axes(Val::Px(8.0), Val::Px(6.0)), ..default() }, Pickable::IGNORE))
                    .with_child((ui_text(&fonts.body, name.clone(), 13.0, PARCHMENT), Pickable::IGNORE));
            });
        }
    });
}

fn pick_clicks(q: Query<(&Interaction, &PickAction), Changed<Interaction>>, mut picker: ResMut<ArtPicker>, mut settings: ResMut<Settings>) {
    for (i, act) in &q {
        if *i != Interaction::Pressed {
            continue;
        }
        let Some((deck, _)) = picker.deck.clone() else { continue };
        match act {
            PickAction::Card(name) => settings.deck_looks.entry(deck).or_default().art = Some(name.clone()),
            PickAction::Default => {
                if let Some(l) = settings.deck_looks.get_mut(&deck) {
                    l.art = None;
                }
            }
            PickAction::Close => {}
        }
        *picker = ArtPicker::default();
    }
}

fn close_on_escape(keys: Res<ButtonInput<KeyCode>>, mut picker: ResMut<ArtPicker>) {
    if picker.is_open() && keys.just_pressed(KeyCode::Escape) {
        *picker = ArtPicker::default();
    }
}

fn chip_visuals(mut q: Query<(&Interaction, &mut BackgroundColor, &mut BorderColor), With<ArtChip>>) {
    for (i, mut bg, mut bc) in &mut q {
        let hov = matches!(i, Interaction::Hovered | Interaction::Pressed);
        bg.set_if_neq(BackgroundColor(if hov { Color::srgba(0.95, 0.46, 0.13, 0.85) } else { Color::srgba(0.05, 0.03, 0.02, 0.55) }));
        bc.set_if_neq(BorderColor::all(if hov { GOLD_BRIGHT } else { GOLD_DIM }));
    }
}

/// Gold frame on the current face, hover lift on the rest.
fn thumb_visuals(
    picker: Res<ArtPicker>,
    settings: Res<Settings>,
    list: Res<DeckList>,
    time: Res<Time>,
    mut q: Query<(&PickAction, &Interaction, &mut BorderColor, &mut UiTransform)>,
) {
    let Some((deck, _)) = &picker.deck else { return };
    let current = settings
        .deck_looks
        .get(deck)
        .and_then(|l| l.art.clone())
        .or_else(|| list.decks.iter().find(|d| &d.deck_ref == deck).and_then(|d| d.cover.clone().or_else(|| d.commanders.first().cloned())));
    let k = (time.delta_secs() * 14.0).min(1.0);
    for (act, i, mut bc, mut tf) in &mut q {
        let PickAction::Card(name) = act else { continue };
        let selected = current.as_deref() == Some(name.as_str());
        let hov = matches!(i, Interaction::Hovered | Interaction::Pressed);
        let target = if hov { 1.04 } else { 1.0 };
        tf.scale = Vec2::splat(tf.scale.x + (target - tf.scale.x) * k);
        bc.set_if_neq(BorderColor::all(if selected { GOLD_BRIGHT } else if hov { GOLD } else { GOLD_DIM }));
    }
}

fn wheel_scroll(
    picker: Res<ArtPicker>,
    mut wheel: MessageReader<MouseWheel>,
    mut q: Query<(&RelativeCursorPosition, &ComputedNode, &mut ScrollPosition), With<PickerGrid>>,
) {
    if !picker.is_open() {
        return;
    }
    let dy: f32 = wheel
        .read()
        .map(|e| match e.unit {
            MouseScrollUnit::Line => e.y * 42.0,
            MouseScrollUnit::Pixel => e.y,
        })
        .sum();
    for (rel, node, mut sp) in &mut q {
        if dy == 0.0 || !rel.cursor_over {
            continue;
        }
        let max = ((node.content_size().y - node.size().y) * node.inverse_scale_factor()).max(0.0);
        sp.y = (sp.y - dy).clamp(0.0, max);
    }
}
