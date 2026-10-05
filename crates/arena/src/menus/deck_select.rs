//! Deck select: format toggle, deck tile grid (fetched from the engine on a background thread),
//! AI opponent picker (incl. Random) and the deck-import modal.
//!
//! Why tasks + channels: the Bevy schedule must never wait on the network. Each request runs as an
//! `mtg_client::spawn` task (a thread on desktop, the page's event loop in the browser) that sends
//! one result back.
//!
//! The same screen picks your deck for a multiplayer table (`MatchMode::Host` / `Join`): PLAY then
//! reads HOST TABLE / TAKE SEAT, the right panel picks the deck for AI-filled seats, and a joiner's
//! format is fixed by the table.

use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::ui::{BackgroundGradient, ColorStop, LinearGradient, RelativeCursorPosition};
use crossbeam_channel::Receiver;
use mtg_client::{Api, ImageRequest, ImageVersion};
use mtg_protocol::{DeckInfo, Format};
use serde_json::Value;

use super::widgets::{Field, text_field};
use super::{MenuAction, MenuState};
use crate::images::CardImages;
use crate::shared::{AppState, ArenaFonts, EngineConfig, MatchMode, Settings};
use crate::style::*;

pub struct DeckSelectPlugin;

impl Plugin for DeckSelectPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DeckList>()
            .init_resource::<DeckFetch>()
            .init_resource::<DeckSelection>()
            .init_resource::<ImportState>()
            .add_message::<RefreshDecks>()
            .add_systems(OnEnter(AppState::DeckSelect), (spawn_screen, request_initial_fetch))
            .add_systems(
                Update,
                (
                    start_fetches,
                    poll_fetch,
                    rebuild_lists,
                    refresh_visuals,
                    art_fade,
                    wheel_scroll,
                    sync_import_panel,
                    poll_import,
                    update_import_text,
                    update_status_line,
                )
                    .chain()
                    .run_if(in_state(AppState::DeckSelect)),
            )
            .add_systems(Update, debug_press_play.before(super::actions::handle_actions).run_if(in_state(AppState::DeckSelect)));
    }
}

// -------------------------------------------------------------------------------------------
// Data
// -------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq)]
pub enum Fetch {
    #[default]
    Idle,
    Loading,
    Ready,
    Error(String),
}

#[derive(Resource, Debug, Default)]
pub struct DeckList {
    pub format: Format,
    pub decks: Vec<DeckInfo>,
    pub status: Fetch,
}

impl DeckList {
    pub fn name_of(&self, deck_ref: &str) -> String {
        self.decks
            .iter()
            .find(|d| d.deck_ref == deck_ref)
            .map(|d| d.name.clone())
            .unwrap_or_else(|| deck_ref.split_once(':').map(|(_, n)| n).unwrap_or(deck_ref).to_string())
    }
}

#[derive(Resource, Debug, Default)]
pub struct DeckSelection {
    pub human: Option<String>,
    /// `None` = Random
    pub ai: Option<String>,
}

#[derive(Resource, Default)]
struct DeckFetch {
    rx: Option<Receiver<(Format, Result<Vec<DeckInfo>, String>)>>,
}

#[derive(Message, Debug, Clone, Copy)]
pub struct RefreshDecks(pub Format);

#[derive(Resource, Debug, Default)]
pub struct ImportState {
    pub busy: bool,
    pub result: Option<Value>,
    pub message: String,
    pub error: bool,
    rx: Option<Receiver<ImportMsg>>,
}

enum ImportMsg {
    Analyzed(Result<Value, String>),
    Saved(Result<Value, String>),
}

fn rest_base(cfg: &Option<Res<EngineConfig>>, settings: &Settings) -> String {
    cfg.as_ref().map(|c| c.base_url.clone()).unwrap_or_else(|| settings.engine_url.clone())
}

fn request_initial_fetch(settings: Res<Settings>, mut w: MessageWriter<RefreshDecks>, mut list: ResMut<DeckList>) {
    list.format = settings.last_format;
    w.write(RefreshDecks(settings.last_format));
}

fn start_fetches(
    mut reqs: MessageReader<RefreshDecks>,
    mut list: ResMut<DeckList>,
    mut fetch: ResMut<DeckFetch>,
    cfg: Option<Res<EngineConfig>>,
    settings: Res<Settings>,
) {
    let Some(RefreshDecks(fmt)) = reqs.read().last().copied() else { return };
    let base = rest_base(&cfg, &settings);
    list.format = fmt;
    list.status = Fetch::Loading;
    fetch.rx = Some(mtg_client::spawn(async move { (fmt, Api::new(base).decks(fmt).await.map_err(|e| format!("{e:#}"))) }));
}

fn poll_fetch(
    mut fetch: ResMut<DeckFetch>,
    mut list: ResMut<DeckList>,
    mut sel: ResMut<DeckSelection>,
    mut settings: ResMut<Settings>,
) {
    let Some(rx) = &fetch.rx else { return };
    let Ok((fmt, res)) = rx.try_recv() else { return };
    fetch.rx = None;
    if fmt != list.format {
        return; // stale answer for a format we've already switched away from
    }
    match res {
        Ok(mut decks) => {
            decks.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
            list.decks = decks;
            list.status = Fetch::Ready;
            let valid = |d: &DeckInfo| d.problem.is_none();
            let key = fmt.as_str().to_string();
            let remembered = settings.last_deck.get(&key).cloned();
            let keep = sel.human.clone().filter(|r| list.decks.iter().any(|d| &d.deck_ref == r && valid(d)));
            sel.human = keep
                .or_else(|| remembered.filter(|r| list.decks.iter().any(|d| &d.deck_ref == r && valid(d))))
                .or_else(|| list.decks.iter().find(|d| valid(d)).map(|d| d.deck_ref.clone()));
            if !sel.ai.as_ref().is_some_and(|r| list.decks.iter().any(|d| &d.deck_ref == r)) {
                sel.ai = None;
            }
            let _ = &mut settings; // selection is persisted on Play
        }
        Err(e) => {
            list.decks.clear();
            list.status = Fetch::Error(e);
        }
    }
}

// -------------------------------------------------------------------------------------------
// Screen
// -------------------------------------------------------------------------------------------

#[derive(Component)]
struct DeckGrid;
#[derive(Component)]
struct AiList;
#[derive(Component)]
struct StatusLine;
#[derive(Component)]
struct Scrollable;
#[derive(Component)]
struct FormatButton(Format);
#[derive(Component)]
struct DeckTile(String);
#[derive(Component)]
struct AiRow(Option<String>);
#[derive(Component)]
struct PlayButton;
#[derive(Component)]
struct ArtFade(Handle<Image>);

fn spawn_screen(mut commands: Commands, fonts: Res<ArenaFonts>, mode: Res<MatchMode>) {
    let (play_label, side_heading) = match *mode {
        MatchMode::VsAi => ("PLAY", "OPPONENT"),
        MatchMode::Host { .. } => ("HOST TABLE", "AI SEATS"),
        MatchMode::Join { .. } => ("TAKE SEAT", "AI SEATS"),
    };
    commands
        .spawn((
            Name::new("DeckSelect"),
            DespawnOnExit(AppState::DeckSelect),
            Node { flex_direction: FlexDirection::Column, padding: UiRect::all(Val::Px(28.0)), row_gap: Val::Px(18.0), ..fullscreen_root() },
        ))
        .with_children(|root| {
            // ---- top bar
            root.spawn(Node { width: Val::Percent(100.0), align_items: AlignItems::Center, column_gap: Val::Px(16.0), ..default() })
                .with_children(|bar| {
                    let b = spawn_button(bar, &fonts, ButtonKind::Secondary, "BACK", Vec2::new(130.0, 46.0));
                    bar.commands().entity(b).insert(MenuAction::Back);
                    bar.spawn(Node { flex_grow: 1.0, justify_content: JustifyContent::Center, ..default() })
                        .with_child(heading(&fonts, "CHOOSE YOUR DECK", 44.0));
                    for (fmt, label, act) in [
                        (Format::Constructed, "CONSTRUCTED", MenuAction::FormatConstructed),
                        (Format::Commander, "COMMANDER", MenuAction::FormatCommander),
                    ] {
                        let b = spawn_button(bar, &fonts, ButtonKind::Secondary, label, Vec2::new(190.0, 46.0));
                        bar.commands().entity(b).insert((act, FormatButton(fmt)));
                    }
                });
            // ---- middle
            root.spawn(Node { width: Val::Percent(100.0), flex_grow: 1.0, min_height: Val::Px(0.0), column_gap: Val::Px(22.0), ..default() })
                .with_children(|mid| {
                    mid.spawn(ornate_panel(Node { flex_grow: 1.0, min_width: Val::Px(0.0), padding: UiRect::all(Val::Px(18.0)), flex_direction: FlexDirection::Column, ..default() }))
                        .with_children(|p| {
                            p.spawn((
                                DeckGrid,
                                Scrollable,
                                ScrollPosition::default(),
                                RelativeCursorPosition::default(),
                                Node {
                                    width: Val::Percent(100.0),
                                    flex_grow: 1.0,
                                    min_height: Val::Px(0.0),
                                    flex_wrap: FlexWrap::Wrap,
                                    justify_content: JustifyContent::Center,
                                    padding: UiRect::all(Val::Px(8.0)),
                                    align_content: AlignContent::FlexStart,
                                    row_gap: Val::Px(18.0),
                                    column_gap: Val::Px(18.0),
                                    overflow: Overflow::scroll_y(),
                                    ..default()
                                },
                            ));
                        });
                    mid.spawn(ornate_panel(Node { width: Val::Px(310.0), padding: UiRect::all(Val::Px(16.0)), flex_direction: FlexDirection::Column, row_gap: Val::Px(6.0), ..default() }))
                        .with_children(|p| {
                            p.spawn(heading(&fonts, side_heading, 24.0));
                            p.spawn(divider());
                            p.spawn((
                                AiList,
                                Scrollable,
                                ScrollPosition::default(),
                                RelativeCursorPosition::default(),
                                Node { width: Val::Percent(100.0), flex_grow: 1.0, min_height: Val::Px(0.0), flex_direction: FlexDirection::Column, row_gap: Val::Px(6.0), overflow: Overflow::scroll_y(), ..default() },
                            ));
                        });
                });
            // ---- bottom bar
            root.spawn(Node { width: Val::Percent(100.0), align_items: AlignItems::Center, column_gap: Val::Px(16.0), ..default() })
                .with_children(|bar| {
                    bar.spawn((StatusLine, Node { flex_grow: 1.0, ..default() }, ui_text(&fonts.body, "", 17.0, TEXT_DIM)));
                    let b = spawn_button(bar, &fonts, ButtonKind::Secondary, "IMPORT DECK", Vec2::new(210.0, 56.0));
                    bar.commands().entity(b).insert(MenuAction::OpenImport);
                    let b = spawn_button(bar, &fonts, ButtonKind::Primary, play_label, Vec2::new(280.0, 66.0));
                    bar.commands().entity(b).insert((MenuAction::StartMatch, PlayButton));
                });
        });
}

fn identity_letters(ci: &str) -> Vec<char> {
    let mut v: Vec<char> = "WUBRG".chars().filter(|c| ci.to_ascii_uppercase().contains(*c)).collect();
    if v.is_empty() {
        v.push('C');
    }
    v
}

fn identity_gradient(ci: &str) -> BackgroundGradient {
    let cols = identity_letters(ci);
    let dim = |c: Color, k: f32| {
        let s = c.to_srgba();
        Color::srgb(s.red * k, s.green * k, s.blue * k)
    };
    let stops: Vec<ColorStop> = if cols.len() == 1 {
        let c = mana_color(cols[0]);
        vec![ColorStop::new(dim(c, 0.9), Val::Percent(0.0)), ColorStop::new(dim(c, 0.28), Val::Percent(100.0))]
    } else {
        let n = cols.len();
        cols.iter().enumerate().map(|(i, &c)| ColorStop::new(dim(mana_color(c), 0.7), Val::Percent(100.0 * i as f32 / (n - 1) as f32))).collect()
    };
    BackgroundGradient::from(LinearGradient::new(std::f32::consts::FRAC_PI_2 * 1.4, stops))
}

fn rebuild_lists(
    mut commands: Commands,
    list: Res<DeckList>,
    fonts: Res<ArenaFonts>,
    assets: Res<AssetServer>,
    mut images: ResMut<CardImages>,
    grid: Query<(Entity, Option<&Children>), With<DeckGrid>>,
    ai: Query<(Entity, Option<&Children>), With<AiList>>,
    added: Query<(), Added<DeckGrid>>,
) {
    if !list.is_changed() && added.is_empty() {
        return;
    }
    if let Ok((e, ch)) = grid.single() {
        for c in ch.into_iter().flatten() {
            commands.entity(*c).despawn();
        }
        commands.entity(e).with_children(|g| {
            for d in &list.decks {
                spawn_tile(g, d, &fonts, &assets, &mut images);
            }
        });
    }
    if let Ok((e, ch)) = ai.single() {
        for c in ch.into_iter().flatten() {
            commands.entity(*c).despawn();
        }
        commands.entity(e).with_children(|g| {
            spawn_ai_row(g, None, "Random", "", &fonts, &assets);
            for d in list.decks.iter().filter(|d| d.problem.is_none()) {
                spawn_ai_row(g, Some(d.deck_ref.clone()), &d.name, &d.color_identity, &fonts, &assets);
            }
        });
    }
}

fn spawn_ai_row(g: &mut ChildSpawnerCommands, deck_ref: Option<String>, name: &str, ci: &str, fonts: &ArenaFonts, assets: &AssetServer) {
    g.spawn((
        Button,
        MenuAction::SelectAi(deck_ref.clone()),
        AiRow(deck_ref.clone()),
        Node {
            width: Val::Percent(100.0),
            padding: UiRect::axes(Val::Px(12.0), Val::Px(9.0)),
            align_items: AlignItems::Center,
            column_gap: Val::Px(8.0),
            border: UiRect::all(Val::Px(1.5)),
            border_radius: BorderRadius::all(Val::Px(8.0)),
            flex_shrink: 0.0,
            ..default()
        },
        BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.04)),
        BorderColor::all(Color::NONE),
    ))
    .with_children(|r| {
        if deck_ref.is_none() {
            r.spawn(ui_text(&fonts.title, "?", 20.0, GOLD_BRIGHT));
        }
        r.spawn((Node { flex_grow: 1.0, flex_basis: Val::Px(0.0), ..default() }, ui_text(&fonts.body, name, 16.0, TEXT)));
        if deck_ref.is_some() {
            for c in identity_letters(ci) {
                r.spawn(mana_pip(assets, c, 16.0));
            }
        }
    });
}

fn spawn_tile(g: &mut ChildSpawnerCommands, d: &DeckInfo, fonts: &ArenaFonts, assets: &AssetServer, images: &mut CardImages) {
    let invalid = d.problem.is_some();
    let letters = identity_letters(&d.color_identity);
    let mut tile = g.spawn((
        DeckTile(d.deck_ref.clone()),
        Button,
        Node {
            width: Val::Px(198.0),
            height: Val::Px(272.0),
            flex_direction: FlexDirection::Column,
            border: UiRect::all(Val::Px(2.5)),
            border_radius: BorderRadius::all(Val::Px(12.0)),
            overflow: Overflow::clip(),
            flex_shrink: 0.0,
            ..default()
        },
        BackgroundColor(Color::srgb(0.10, 0.065, 0.045)),
        BorderColor::all(GOLD_DIM),
        BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.55), Val::Px(0.0), Val::Px(6.0), Val::Px(0.0), Val::Px(12.0)),
        UiTransform::IDENTITY,
    ));
    if !invalid {
        tile.insert(MenuAction::SelectDeck(d.deck_ref.clone()));
    }
    tile.with_children(|t| {
        // art area
        t.spawn((Node { width: Val::Percent(100.0), height: Val::Px(146.0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, flex_shrink: 0.0, ..default() }, identity_gradient(&d.color_identity)))
            .with_children(|a| {
                // big faint pips behind / fallback art
                let n = letters.len().min(3);
                a.spawn(Node { column_gap: Val::Px(6.0), ..default() }).with_children(|r| {
                    for &c in letters.iter().take(n) {
                        r.spawn((mana_pip(assets, c, if n == 1 { 74.0 } else { 54.0 }), Pickable::IGNORE));
                    }
                });
                if let Some(cmd) = d.commanders.first() {
                    let h = images.request_raw(ImageRequest { name: cmd.clone(), set: None, back: false, version: ImageVersion::ArtCrop });
                    a.spawn((
                        ArtFade(h.clone()),
                        Node { position_type: PositionType::Absolute, left: Val::Px(0.0), top: Val::Px(0.0), width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
                        ImageNode { image: h, color: Color::srgba(1.0, 1.0, 1.0, 0.0), image_mode: NodeImageMode::Auto, ..default() },
                        Pickable::IGNORE,
                    ));
                }
            });
        // name + meta
        t.spawn(Node { flex_grow: 1.0, flex_direction: FlexDirection::Column, justify_content: JustifyContent::SpaceBetween, padding: UiRect::all(Val::Px(10.0)), ..default() })
            .with_children(|b| {
                b.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(3.0), overflow: Overflow::clip(), max_height: Val::Px(80.0), ..default() }).with_children(|c| {
                    c.spawn((ui_text(&fonts.title, d.name.clone(), 16.0, if invalid { TEXT_DIM } else { PARCHMENT }), Pickable::IGNORE));
                    let sub = if let Some(p) = &d.problem {
                        p.chars().take(40).collect::<String>()
                    } else if let Some(cmd) = d.commanders.first() {
                        cmd.clone()
                    } else {
                        String::new()
                    };
                    c.spawn((ui_text(&fonts.body, sub, 12.0, if invalid { RED_DAMAGE } else { TEXT_DIM }), Pickable::IGNORE));
                });
                b.spawn(Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, ..default() }).with_children(|r| {
                    r.spawn(Node { column_gap: Val::Px(3.0), ..default() }).with_children(|p| {
                        for &c in &letters {
                            p.spawn((mana_pip(assets, c, 20.0), Pickable::IGNORE));
                        }
                    });
                    r.spawn((ui_text(&fonts.numbers, format!("{} cards", d.main_count), 15.0, GOLD), Pickable::IGNORE));
                });
            });
    });
}

fn refresh_visuals(
    list: Res<DeckList>,
    sel: Res<DeckSelection>,
    time: Res<Time>,
    mut tiles: Query<(&DeckTile, &Interaction, &mut BorderColor, &mut UiTransform, &mut BoxShadow), (Without<AiRow>, Without<FormatButton>)>,
    mut rows: Query<(&AiRow, &Interaction, &mut BackgroundColor, &mut BorderColor), (Without<DeckTile>, Without<FormatButton>)>,
    mut fmt_btns: Query<(&FormatButton, &mut ArenaButton)>,
    mut play: Query<&mut ArenaButton, (With<PlayButton>, Without<FormatButton>)>,
) {
    let k = (time.delta_secs() * 14.0).min(1.0);
    for (t, i, mut bc, mut tf, mut sh) in &mut tiles {
        let selected = sel.human.as_deref() == Some(&t.0);
        let hov = matches!(i, Interaction::Hovered | Interaction::Pressed);
        let target = if selected { 1.04 } else if hov { 1.025 } else { 1.0 };
        let s = tf.scale.x + (target - tf.scale.x) * k;
        tf.scale = Vec2::splat(s);
        let col = if selected { GOLD_BRIGHT } else if hov { GOLD } else { GOLD_DIM };
        if bc.top != col {
            *bc = BorderColor::all(col);
            sh.0 = vec![bevy::ui::ShadowStyle {
                color: if selected { Color::srgba(1.0, 0.7, 0.25, 0.55) } else { Color::srgba(0.0, 0.0, 0.0, 0.55) },
                x_offset: Val::Px(0.0),
                y_offset: Val::Px(if selected { 0.0 } else { 6.0 }),
                spread_radius: Val::Px(if selected { 2.0 } else { 0.0 }),
                blur_radius: Val::Px(if selected { 22.0 } else { 12.0 }),
            }];
        }
    }
    for (r, i, mut bg, mut bc) in &mut rows {
        let selected = sel.ai == r.0;
        let hov = matches!(i, Interaction::Hovered | Interaction::Pressed);
        bg.0 = if selected { Color::srgba(0.95, 0.46, 0.13, 0.22) } else if hov { Color::srgba(1.0, 1.0, 1.0, 0.09) } else { Color::srgba(1.0, 1.0, 1.0, 0.04) };
        *bc = BorderColor::all(if selected { ORANGE } else { Color::NONE });
    }
    for (f, mut b) in &mut fmt_btns {
        let want = if f.0 == list.format { ButtonKind::Primary } else { ButtonKind::Secondary };
        if b.kind != want {
            b.kind = want;
        }
    }
    for mut b in &mut play {
        b.disabled = sel.human.is_none();
    }
}

fn art_fade(images: Res<Assets<Image>>, time: Res<Time>, mut q: Query<(&ArtFade, &mut ImageNode)>) {
    for (a, mut n) in &mut q {
        let loaded = images.get(&a.0).is_some_and(|i| i.width() > 8);
        let target = if loaded { 1.0 } else { 0.0 };
        let cur = n.color.alpha();
        if (cur - target).abs() > 0.004 {
            n.color = Color::srgba(1.0, 1.0, 1.0, cur + (target - cur) * (time.delta_secs() * 8.0).min(1.0));
        }
    }
}

fn wheel_scroll(
    mut wheel: MessageReader<MouseWheel>,
    mut q: Query<(&RelativeCursorPosition, &ComputedNode, &mut ScrollPosition), With<Scrollable>>,
) {
    let mut dy = 0.0;
    for e in wheel.read() {
        dy += match e.unit {
            MouseScrollUnit::Line => e.y * 42.0,
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

fn update_status_line(list: Res<DeckList>, sel: Res<DeckSelection>, mut q: Query<(&mut Text, &mut TextColor), With<StatusLine>>) {
    let (msg, col) = match &list.status {
        Fetch::Loading | Fetch::Idle => ("Loading decks…".to_string(), TEXT_DIM),
        Fetch::Error(e) => (format!("Could not load decks: {e}"), RED_DAMAGE),
        Fetch::Ready if list.decks.is_empty() => ("No decks found for this format — use IMPORT DECK to add one.".into(), TEXT_DIM),
        Fetch::Ready => {
            let mine = sel.human.as_deref().map(|r| list.name_of(r)).unwrap_or_else(|| "—".into());
            let opp = sel.ai.as_deref().map(|r| list.name_of(r)).unwrap_or_else(|| "Random".into());
            (format!("{mine}   vs   {opp}"), TEXT)
        }
    };
    for (mut t, mut c) in &mut q {
        if t.0 != msg {
            t.0 = msg.clone();
        }
        if c.0 != col {
            c.0 = col;
        }
    }
}

// -------------------------------------------------------------------------------------------
// Import modal
// -------------------------------------------------------------------------------------------

#[derive(Component)]
struct ImportPanel;
#[derive(Component)]
struct ImportResultText;
#[derive(Component)]
struct ImportSaveButton;
#[derive(Component)]
struct ImportAnalyzeButton;

fn sync_import_panel(
    mut commands: Commands,
    menu: Res<MenuState>,
    fonts: Res<ArenaFonts>,
    list: Res<DeckList>,
    existing: Query<Entity, With<ImportPanel>>,
    mut import: ResMut<ImportState>,
) {
    let open = menu.import_open;
    let has = !existing.is_empty();
    if open && !has {
        *import = ImportState::default();
        import.message = "Paste a decklist (e.g. \"4 Lightning Bolt\"), then press ANALYZE. For Commander put the commander first or under \"Commander\".".into();
        commands
            .spawn((
                ImportPanel,
                DespawnOnExit(AppState::DeckSelect),
                GlobalZIndex(100),
                Node { justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..fullscreen_root() },
                BackgroundColor(SCRIM),
            ))
            .with_children(|scrim| {
                scrim
                    .spawn(ornate_panel(Node { width: Val::Px(880.0), max_height: Val::Percent(92.0), padding: UiRect::all(Val::Px(26.0)), flex_direction: FlexDirection::Column, row_gap: Val::Px(10.0), ..default() }))
                    .with_children(|p| {
                        p.spawn(heading(&fonts, format!("IMPORT DECK  ·  {}", list.format.as_str().to_uppercase()), 30.0));
                        p.spawn(divider());
                        p.spawn(text_field(&fonts, Field::ImportText, "", Val::Percent(100.0), 11.0, true));
                        p.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(12.0), ..default() }).with_children(|r| {
                            r.spawn(ui_text(&fonts.body, "Deck name", 16.0, TEXT_DIM));
                            r.spawn(text_field(&fonts, Field::ImportName, "Imported Deck", Val::Px(360.0), 1.0, false));
                        });
                        p.spawn((
                            ImportResultText,
                            Node { min_height: Val::Px(90.0), max_height: Val::Px(150.0), overflow: Overflow::clip(), ..default() },
                            ui_text(&fonts.body, "", 15.0, TEXT),
                        ));
                        p.spawn(Node { justify_content: JustifyContent::FlexEnd, column_gap: Val::Px(12.0), margin: UiRect::top(Val::Px(6.0)), ..default() }).with_children(|r| {
                            let b = spawn_button(r, &fonts, ButtonKind::Ghost, "CLOSE", Vec2::new(140.0, 48.0));
                            r.commands().entity(b).insert(MenuAction::ImportClose);
                            let b = spawn_button(r, &fonts, ButtonKind::Secondary, "ANALYZE", Vec2::new(180.0, 48.0));
                            r.commands().entity(b).insert((MenuAction::ImportAnalyze, ImportAnalyzeButton));
                            let b = spawn_button(r, &fonts, ButtonKind::Primary, "SAVE DECK", Vec2::new(200.0, 48.0));
                            r.commands().entity(b).insert((MenuAction::ImportSave, ImportSaveButton));
                        });
                    });
            });
    } else if !open && has {
        for e in &existing {
            commands.entity(e).despawn();
        }
    }
}

fn import_summary(v: &Value) -> (String, bool) {
    let count = |k: &str| -> i64 { v.get(k).and_then(|a| a.as_array()).map(|a| a.iter().map(|e| e.get("qty").and_then(|q| q.as_i64()).unwrap_or(1)).sum()).unwrap_or(0) };
    let cmd: Vec<String> = v
        .get("commanders")
        .and_then(|a| a.as_array())
        .map(|a| a.iter().filter_map(|c| c.get("name").and_then(|n| n.as_str()).map(String::from)).collect())
        .unwrap_or_default();
    let mut s = format!("{} cards in the main deck, {} in the sideboard", count("main") + cmd.len() as i64, count("sideboard"));
    if !cmd.is_empty() {
        s += &format!(" · Commander: {}", cmd.join(" & "));
    }
    let unresolved = v.get("unresolved").and_then(|a| a.as_array()).cloned().unwrap_or_default();
    for u in unresolved.iter().take(8) {
        let line = u.get("line").and_then(|l| l.as_str()).unwrap_or("?");
        let sug: Vec<&str> = u.get("suggestions").and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|x| x.as_str()).take(3).collect()).unwrap_or_default();
        s += &format!("\n✗ {line}");
        if !sug.is_empty() {
            s += &format!("   → did you mean: {}", sug.join(" / "));
        }
    }
    if unresolved.len() > 8 {
        s += &format!("\n… and {} more unresolved lines", unresolved.len() - 8);
    }
    if let Some(w) = v.get("warnings").and_then(|a| a.as_array()) {
        for w in w.iter().take(3).filter_map(|w| w.as_str()) {
            s += &format!("\n! {w}");
        }
    }
    (s, unresolved.is_empty())
}

fn update_import_text(
    import: Res<ImportState>,
    mut text: Query<(&mut Text, &mut TextColor), With<ImportResultText>>,
    mut save: Query<&mut ArenaButton, (With<ImportSaveButton>, Without<ImportAnalyzeButton>)>,
    mut analyze: Query<&mut ArenaButton, (With<ImportAnalyzeButton>, Without<ImportSaveButton>)>,
) {
    if !import.is_changed() {
        return;
    }
    let (msg, ok) = match (&import.result, import.busy) {
        (_, true) => ("Working…".to_string(), true),
        (Some(v), false) if import.message.is_empty() => import_summary(v),
        _ => (import.message.clone(), !import.error),
    };
    for (mut t, mut c) in &mut text {
        t.0 = msg.clone();
        c.0 = if import.error { RED_DAMAGE } else if ok { TEXT } else { Color::srgb(1.0, 0.75, 0.4) };
    }
    let can_save = import.result.as_ref().is_some_and(|v| import_summary(v).1) && !import.busy;
    for mut b in &mut save {
        b.disabled = !can_save;
    }
    for mut b in &mut analyze {
        b.disabled = import.busy;
    }
}

fn poll_import(
    mut import: ResMut<ImportState>,
    mut menu: ResMut<MenuState>,
    mut refresh: MessageWriter<RefreshDecks>,
    list: Res<DeckList>,
) {
    let Some(rx) = &import.rx else { return };
    let Ok(msg) = rx.try_recv() else { return };
    import.rx = None;
    import.busy = false;
    match msg {
        ImportMsg::Analyzed(Ok(v)) => {
            import.message.clear();
            import.error = false;
            import.result = Some(v);
        }
        ImportMsg::Analyzed(Err(e)) => {
            import.error = true;
            import.result = None;
            import.message = e;
        }
        ImportMsg::Saved(Ok(_)) => {
            menu.import_open = false;
            refresh.write(RefreshDecks(list.format));
        }
        ImportMsg::Saved(Err(e)) => {
            import.error = true;
            import.message = format!("Save failed: {e}");
        }
    }
}

/// Kicked off from `actions` (needs the field text).
pub(super) fn start_import(import: &mut ImportState, base: String, text: String, format: Format) {
    if import.busy || text.trim().is_empty() {
        if text.trim().is_empty() {
            import.error = true;
            import.message = "Paste a decklist first.".into();
        }
        return;
    }
    import.busy = true;
    import.error = false;
    import.result = None;
    import.rx = Some(mtg_client::spawn(async move {
        ImportMsg::Analyzed(Api::new(base).import_deck(&text, format).await.map_err(|e| format!("{e:#}")))
    }));
}

pub(super) fn start_save(import: &mut ImportState, base: String, name: String, format: Format) {
    let Some(res) = import.result.clone() else { return };
    if import.busy {
        return;
    }
    let names = |k: &str| -> Vec<Value> {
        res.get(k)
            .and_then(|a| a.as_array())
            .map(|a| {
                a.iter()
                    .map(|e| {
                        let c = e.get("card").unwrap_or(e);
                        let mut o = serde_json::json!({
                            "name": c.get("name").cloned().unwrap_or(Value::Null),
                            "qty": e.get("qty").cloned().unwrap_or(Value::from(1)),
                        });
                        if let Some(set) = c.get("set").filter(|s| s.as_str().is_some_and(|s| !s.is_empty())) {
                            o["set"] = set.clone();
                        }
                        o
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let commanders: Vec<Value> = res
        .get("commanders")
        .and_then(|a| a.as_array())
        .map(|a| a.iter().filter_map(|c| c.get("name").cloned()).collect())
        .unwrap_or_default();
    let name = if name.trim().is_empty() { res.get("name").and_then(|n| n.as_str()).unwrap_or("Imported Deck").to_string() } else { name.trim().to_string() };
    let body = serde_json::json!({ "name": name, "commanders": commanders, "main": names("main"), "sideboard": names("sideboard") });
    import.busy = true;
    import.rx = Some(mtg_client::spawn(async move {
        ImportMsg::Saved(Api::new(base).save_deck(&body, format).await.map_err(|e| format!("{e:#}")))
    }));
}

pub(super) fn engine_base(cfg: &Option<Res<EngineConfig>>, settings: &Settings) -> String {
    rest_base(cfg, settings)
}

/// Dev aid (`ARENA_MENU_SCREEN=play`): once decks are listed, "click" PLAY the way a user would,
/// so the whole menu -> loading -> match path can be exercised unattended.
fn debug_press_play(
    list: Res<DeckList>,
    sel: Res<DeckSelection>,
    mut play: Query<&mut Interaction, With<PlayButton>>,
    mut done: Local<bool>,
) {
    if *done || list.status != Fetch::Ready || sel.human.is_none() {
        return;
    }
    if std::env::var("ARENA_MENU_SCREEN").as_deref() != Ok("play") {
        return;
    }
    for mut i in &mut play {
        *i = Interaction::Pressed;
        *done = true;
    }
}
