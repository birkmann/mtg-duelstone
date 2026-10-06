//! Avatar studio: pick any card's art as your portrait and frame it.
//!
//! Why pick-then-frame on one screen: card art is composed for a landscape card window, not a
//! circle, so the face you want is rarely where a blind centre crop lands. Choosing a card shows
//! its whole art with the circle on top; drag the circle onto the face, scroll to zoom, and the
//! previews show the result at the sizes the table uses.
//!
//! Every edit goes straight into `Settings.avatar` (saved by settings), so there is nothing to
//! "apply" and leaving the screen can't lose work. Like the design studio, its many small buttons
//! use a local action enum; only entering goes through `MenuAction`.

use bevy::image::TRANSPARENT_IMAGE_HANDLE;
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::RelativeCursorPosition;
use crossbeam_channel::Receiver;
use mtg_client::Api;
use mtg_protocol::{Avatar, CardQuery};

use super::widgets::{Field, WheelScroll, text_field};
use crate::avatars::{FEATURED, art_request, art_size, crop_rect};
use crate::images::CardImages;
use crate::shared::*;
use crate::style::*;

pub struct AvatarStudioPlugin;

impl Plugin for AvatarStudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Picker>()
            .add_systems(OnEnter(AppState::AvatarStudio), (enter, spawn_screen).chain())
            .add_systems(
                Update,
                (handle_clicks, read_search, run_search, rebuild_grid, mark_selected, drag_crop, drag_zoom, wheel_zoom, show_editor, show_previews)
                    .chain()
                    .run_if(in_state(AppState::AvatarStudio)),
            );
    }
}

/// Art editor width in px; its height follows the art's aspect.
const EDIT_W: f32 = 470.0;
const RESULTS: u32 = 120;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Tab {
    #[default]
    Featured,
    Legends,
    Planeswalkers,
    Any,
}

impl Tab {
    const ALL: [Tab; 4] = [Tab::Featured, Tab::Legends, Tab::Planeswalkers, Tab::Any];
    fn label(self) -> &'static str {
        match self {
            Tab::Featured => "FEATURED",
            Tab::Legends => "LEGENDS",
            Tab::Planeswalkers => "PLANESWALKERS",
            Tab::Any => "ANY CARD",
        }
    }
    /// Type-line filter for the engine's catalog search.
    fn type_line(self) -> Option<&'static str> {
        match self {
            Tab::Legends => Some("Legendary Creature"),
            Tab::Planeswalkers => Some("Planeswalker"),
            _ => None,
        }
    }
}

type Found = Result<Vec<String>, String>;

#[derive(Resource, Default)]
struct Picker {
    tab: Tab,
    query: String,
    /// when the search text last changed (searches wait for typing to pause)
    dirty_at: Option<f32>,
    results: Vec<String>,
    pending: Option<Receiver<Found>>,
    status: String,
    rebuild: bool,
    /// circle centre minus the cursor when the drag began (grab the circle where you clicked it)
    grab: Option<Vec2>,
    zoom_drag: bool,
}

#[derive(Component, Debug, Clone, PartialEq)]
enum AvAction {
    Tab(Tab),
    Pick(String),
    ZoomStep(f32),
    Reset,
    Surprise,
    Back,
}

#[derive(Component)]
struct Grid;
#[derive(Component)]
struct GridStatus;
#[derive(Component)]
struct Tile(String);
#[derive(Component)]
struct TabButton(Tab);
#[derive(Component)]
struct CropArt;
#[derive(Component)]
struct CropCircle;
#[derive(Component)]
struct CropDim;
#[derive(Component)]
struct CardTitle;
#[derive(Component)]
struct ZoomTrack;
#[derive(Component)]
struct ZoomFill;
#[derive(Component)]
struct ZoomText;
#[derive(Component)]
struct Preview;

fn enter(mut picker: ResMut<Picker>) {
    // keep the tab and search from the last visit; results are re-fetched
    picker.rebuild = true;
    picker.dirty_at = Some(-1.0);
    picker.grab = None;
    picker.zoom_drag = false;
}

// ---- screen ---------------------------------------------------------------------------------

fn spawn_screen(mut commands: Commands, fonts: Res<ArenaFonts>, picker: Res<Picker>, settings: Res<Settings>) {
    commands
        .spawn((
            Name::new("AvatarStudio"),
            DespawnOnExit(AppState::AvatarStudio),
            Node { flex_direction: FlexDirection::Column, padding: UiRect::all(Val::Px(24.0)), row_gap: Val::Px(16.0), ..fullscreen_root() },
            Pickable::IGNORE,
        ))
        .with_children(|root| {
            root.spawn(Node { width: Val::Percent(100.0), align_items: AlignItems::Center, column_gap: Val::Px(18.0), ..default() }).with_children(|bar| {
                let b = spawn_button(bar, &fonts, ButtonKind::Secondary, "BACK", Vec2::new(120.0, 44.0));
                bar.commands().entity(b).insert(AvAction::Back);
                bar.spawn(heading(&fonts, "AVATAR", 40.0));
                bar.spawn(ui_text(&fonts.body, "Any card's art can be your face at the table. Everyone you play sees it.", 16.0, TEXT_DIM));
            });
            root.spawn(Node { width: Val::Percent(100.0), flex_grow: 1.0, min_height: Val::Px(0.0), column_gap: Val::Px(16.0), ..default() }).with_children(|mid| {
                spawn_picker(mid, &fonts, &picker);
                spawn_editor(mid, &fonts, &settings);
            });
        });
}

fn spawn_picker(mid: &mut ChildSpawnerCommands, fonts: &ArenaFonts, picker: &Picker) {
    mid.spawn(ornate_panel(Node {
        flex_grow: 1.0,
        min_width: Val::Px(0.0),
        padding: UiRect::all(Val::Px(14.0)),
        flex_direction: FlexDirection::Column,
        row_gap: Val::Px(10.0),
        ..default()
    }))
    .with_children(|p| {
        p.spawn(Node { column_gap: Val::Px(8.0), align_items: AlignItems::Center, flex_wrap: FlexWrap::Wrap, row_gap: Val::Px(8.0), ..default() }).with_children(|r| {
            for t in Tab::ALL {
                let kind = if t == picker.tab { ButtonKind::Primary } else { ButtonKind::Secondary };
                let b = spawn_button(r, fonts, kind, t.label(), Vec2::new(if t == Tab::Planeswalkers { 176.0 } else { 132.0 }, 40.0));
                r.commands().entity(b).insert((AvAction::Tab(t), TabButton(t)));
            }
            r.spawn(Node { flex_grow: 1.0, ..default() });
            r.spawn(text_field(fonts, Field::AvatarSearch, &picker.query, Val::Px(260.0), 1.0, false));
        });
        p.spawn((GridStatus, ui_text(&fonts.body, "", 14.0, TEXT_DIM)));
        p.spawn((
            Grid,
            ScrollPosition::default(),
            RelativeCursorPosition::default(),
            WheelScroll,
            Node {
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                flex_wrap: FlexWrap::Wrap,
                align_content: AlignContent::FlexStart,
                column_gap: Val::Px(10.0),
                row_gap: Val::Px(10.0),
                overflow: Overflow::scroll_y(),
                ..default()
            },
        ));
    });
}

/// A portrait circle like the HUD's: gold ring, clipped art.
fn portrait(size: f32, ring: f32, avatar: &Avatar) -> impl Bundle {
    (
        Preview,
        Node {
            width: Val::Px(size),
            height: Val::Px(size),
            border: UiRect::all(Val::Px(ring)),
            border_radius: BorderRadius::MAX,
            overflow: Overflow::clip(),
            ..default()
        },
        BackgroundColor(Color::srgb(0.05, 0.035, 0.03)),
        BorderColor::all(GOLD),
        BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.7), Val::Px(0.0), Val::Px(4.0), Val::Px(0.0), Val::Px(14.0)),
        ImageNode::new(TRANSPARENT_IMAGE_HANDLE),
        AvatarArt { avatar: Some(avatar.clone()), fallback: TRANSPARENT_IMAGE_HANDLE },
    )
}

fn spawn_editor(mid: &mut ChildSpawnerCommands, fonts: &ArenaFonts, settings: &Settings) {
    mid.spawn(ornate_panel(Node {
        width: Val::Px(EDIT_W + 32.0),
        padding: UiRect::all(Val::Px(16.0)),
        flex_direction: FlexDirection::Column,
        align_items: AlignItems::Center,
        row_gap: Val::Px(12.0),
        overflow: Overflow::clip(),
        ..default()
    }))
    .with_children(|p| {
        p.spawn((CardTitle, ui_text(&fonts.title, settings.avatar.card.clone(), 22.0, GOLD_BRIGHT)));
        // the whole art, the circle on top. The dimming is a round node with a thick border: its
        // inner edge is the circle and the art clips the rest, a cut-out without a mask shader
        p.spawn((
            CropArt,
            Node {
                width: Val::Px(EDIT_W),
                height: Val::Px(EDIT_W * 0.7),
                // a short window must not squash the art: the circle maps onto it 1:1
                flex_shrink: 0.0,
                border: UiRect::all(Val::Px(2.0)),
                border_radius: BorderRadius::all(Val::Px(8.0)),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(Color::srgb(0.04, 0.03, 0.025)),
            BorderColor::all(GOLD_DIM),
            ImageNode { image: TRANSPARENT_IMAGE_HANDLE, image_mode: NodeImageMode::Stretch, ..default() },
            RelativeCursorPosition::default(),
            Interaction::None,
        ))
        .with_children(|a| {
            a.spawn((
                CropDim,
                Node { position_type: PositionType::Absolute, border_radius: BorderRadius::MAX, ..default() },
                BorderColor::all(Color::srgba(0.0, 0.0, 0.0, 0.66)),
                Pickable::IGNORE,
            ));
            a.spawn((
                CropCircle,
                Node { position_type: PositionType::Absolute, border: UiRect::all(Val::Px(3.0)), border_radius: BorderRadius::MAX, ..default() },
                BorderColor::all(GOLD_BRIGHT),
                Pickable::IGNORE,
            ));
        });
        p.spawn(ui_text(&fonts.body, "Drag the circle onto a face · scroll over the art to zoom", 14.0, TEXT_DIM));
        // zoom
        p.spawn(Node { width: Val::Px(EDIT_W), align_items: AlignItems::Center, column_gap: Val::Px(10.0), ..default() }).with_children(|r| {
            r.spawn(ui_text(&fonts.title, "ZOOM", 15.0, GOLD));
            let b = spawn_button(r, fonts, ButtonKind::Secondary, "–", Vec2::new(46.0, 40.0));
            r.commands().entity(b).insert(AvAction::ZoomStep(1.0 / 1.15));
            r.spawn((
                ZoomTrack,
                Node { flex_grow: 1.0, height: Val::Px(12.0), border: UiRect::all(Val::Px(1.0)), border_radius: BorderRadius::MAX, overflow: Overflow::clip(), ..default() },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.5)),
                BorderColor::all(GOLD_DIM),
                RelativeCursorPosition::default(),
                Interaction::None,
            ))
            .with_child((ZoomFill, Node { width: Val::Percent(0.0), height: Val::Percent(100.0), ..default() }, BackgroundColor(ORANGE), Pickable::IGNORE));
            let b = spawn_button(r, fonts, ButtonKind::Secondary, "+", Vec2::new(46.0, 40.0));
            r.commands().entity(b).insert(AvAction::ZoomStep(1.15));
            r.spawn((ZoomText, ui_text(&fonts.body, "", 15.0, PARCHMENT), Node { width: Val::Px(48.0), ..default() }));
        });
        p.spawn(divider());
        // the result at table size (a duel), at a crowded table, and in the lobby list
        p.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(30.0), ..default() }).with_children(|r| {
            r.spawn(Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, ..default() }).with_children(|c| {
                c.spawn(portrait(130.0, 5.0, &settings.avatar));
                c.spawn((
                    Node {
                        width: Val::Px(62.0),
                        height: Val::Px(62.0),
                        margin: UiRect::top(Val::Px(-34.0)),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        border: UiRect::all(Val::Px(4.0)),
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.06, 0.06, 0.09)),
                    BorderColor::all(GOLD),
                ))
                .with_child(ui_text(&fonts.title, "20", 30.0, TEXT));
                c.spawn(ui_text(&fonts.title, settings.player_name.clone(), 15.0, TEXT_DIM));
            });
            r.spawn(Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, row_gap: Val::Px(12.0), ..default() }).with_children(|c| {
                c.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(16.0), ..default() }).with_children(|s| {
                    s.spawn(portrait(80.0, 4.0, &settings.avatar));
                    s.spawn(portrait(44.0, 2.0, &settings.avatar));
                });
                let b = spawn_button(c, fonts, ButtonKind::Secondary, "RESET FRAMING", Vec2::new(200.0, 40.0));
                c.commands().entity(b).insert(AvAction::Reset);
                let b = spawn_button(c, fonts, ButtonKind::Secondary, "SURPRISE ME", Vec2::new(200.0, 40.0));
                c.commands().entity(b).insert(AvAction::Surprise);
            });
        });
    });
}

// ---- picking --------------------------------------------------------------------------------

fn handle_clicks(
    q: Query<(&Interaction, &AvAction), Changed<Interaction>>,
    mut picker: ResMut<Picker>,
    mut settings: ResMut<Settings>,
    mut next: ResMut<NextState<AppState>>,
    mut tabs: Query<(&TabButton, &mut ArenaButton)>,
) {
    for (i, act) in &q {
        if *i != Interaction::Pressed {
            continue;
        }
        match act {
            AvAction::Back => next.set(AppState::MainMenu),
            AvAction::Tab(t) if *t != picker.tab => {
                picker.tab = *t;
                picker.dirty_at = Some(-1.0);
                for (tb, mut b) in &mut tabs {
                    b.kind = if tb.0 == *t { ButtonKind::Primary } else { ButtonKind::Secondary };
                }
            }
            AvAction::Tab(_) => {}
            AvAction::Pick(card) if *card != settings.avatar.card => settings.avatar = Avatar::framed(card.clone()),
            AvAction::Pick(_) => {}
            AvAction::ZoomStep(k) => settings.avatar.zoom = (settings.avatar.zoom * k).clamp(1.0, Avatar::MAX_ZOOM),
            AvAction::Reset => settings.avatar = Avatar::framed(settings.avatar.card.clone()),
            AvAction::Surprise => {
                let n = mtg_client::now_millis() as usize;
                let pool: Vec<&&str> = FEATURED.iter().filter(|c| **c != settings.avatar.card).collect();
                settings.avatar = Avatar::framed(pool[n % pool.len()].to_string());
            }
        }
    }
}

fn read_search(fields: Query<(&Field, &EditableText), Changed<EditableText>>, mut picker: ResMut<Picker>, time: Res<Time>) {
    for (f, t) in &fields {
        let v = t.value().to_string();
        if *f == Field::AvatarSearch && picker.query != v {
            picker.query = v;
            picker.dirty_at = Some(time.elapsed_secs());
        }
    }
}

fn run_search(mut picker: ResMut<Picker>, time: Res<Time>, cfg: Option<Res<EngineConfig>>) {
    if let Some(rx) = &picker.pending
        && let Ok(found) = rx.try_recv()
    {
        picker.pending = None;
        match found {
            Ok(names) => {
                picker.status = if names.is_empty() { "No cards match.".into() } else { String::new() };
                picker.results = names;
            }
            Err(e) => {
                picker.status = format!("Search failed: {e}");
                picker.results.clear();
            }
        }
        picker.rebuild = true;
    }
    // typing waits for a pause; a tab switch (dirty_at < 0) searches at once
    let Some(at) = picker.dirty_at else { return };
    if at >= 0.0 && time.elapsed_secs() - at < 0.35 {
        return;
    }
    picker.dirty_at = None;
    let q = picker.query.trim().to_lowercase();
    if picker.tab == Tab::Featured {
        picker.results = FEATURED.iter().filter(|c| c.to_lowercase().contains(&q)).map(|c| c.to_string()).collect();
        picker.status = if picker.results.is_empty() { "No featured character matches — try LEGENDS or ANY CARD.".into() } else { String::new() };
        picker.pending = None;
        picker.rebuild = true;
        return;
    }
    let Some(cfg) = cfg else { return };
    let query = CardQuery {
        q: q.clone(),
        type_line: picker.tab.type_line().map(str::to_string),
        sort: Some(if q.is_empty() { "popular" } else { "name" }.into()),
        limit: RESULTS,
        ..Default::default()
    };
    let api = Api::new(cfg.base_url.clone());
    picker.status = "Searching…".into();
    // a newer search simply replaces the receiver; the older answer is dropped
    picker.pending = Some(mtg_client::spawn(async move {
        let cards = api.catalog_search(&query).await.map_err(|e| format!("{e:#}"))?;
        let mut names: Vec<String> = Vec::new();
        for c in cards {
            // basic lands make poor faces, and a name can come back once per printing
            if !c.is_basic_land && !names.contains(&c.name) {
                names.push(c.name);
            }
        }
        Ok(names)
    }));
}

fn rebuild_grid(
    mut commands: Commands,
    mut picker: ResMut<Picker>,
    fonts: Res<ArenaFonts>,
    mut card_images: ResMut<CardImages>,
    grid: Query<(Entity, Option<&Children>), With<Grid>>,
    mut grid_scroll: Query<&mut ScrollPosition, With<Grid>>,
    mut status: Query<&mut Text, With<GridStatus>>,
) {
    if !picker.rebuild {
        return;
    }
    let Ok((e, children)) = grid.single() else { return };
    picker.rebuild = false;
    for mut t in &mut status {
        t.0 = picker.status.clone();
    }
    for c in children.into_iter().flatten() {
        commands.entity(*c).despawn();
    }
    for mut sp in &mut grid_scroll {
        sp.y = 0.0;
    }
    commands.entity(e).with_children(|g| {
        for name in &picker.results {
            let art = card_images.request_raw(art_request(name));
            g.spawn((
                Button,
                AvAction::Pick(name.clone()),
                Tile(name.clone()),
                Node {
                    width: Val::Px(176.0),
                    flex_direction: FlexDirection::Column,
                    border: UiRect::all(Val::Px(2.0)),
                    border_radius: BorderRadius::all(Val::Px(8.0)),
                    overflow: Overflow::clip(),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
                BorderColor::all(Color::srgba(0.86, 0.68, 0.30, 0.25)),
            ))
            .with_children(|t| {
                t.spawn((
                    Node { width: Val::Percent(100.0), height: Val::Px(124.0), ..default() },
                    BackgroundColor(Color::srgb(0.06, 0.045, 0.035)),
                    ImageNode { image: art, image_mode: NodeImageMode::Stretch, ..default() },
                    Pickable::IGNORE,
                ));
                t.spawn((
                    Node { padding: UiRect::axes(Val::Px(8.0), Val::Px(5.0)), height: Val::Px(28.0), overflow: Overflow::clip(), ..default() },
                    Pickable::IGNORE,
                ))
                .with_child((ui_text(&fonts.body, name.clone(), 13.0, PARCHMENT), TextLayout::no_wrap(), Pickable::IGNORE));
            });
        }
    });
}

fn mark_selected(settings: Res<Settings>, mut tiles: Query<(&Tile, &Interaction, &mut BorderColor)>) {
    for (t, i, mut bc) in &mut tiles {
        let c = if t.0 == settings.avatar.card {
            ORANGE_HI
        } else if *i == Interaction::Hovered {
            GOLD_BRIGHT
        } else {
            Color::srgba(0.86, 0.68, 0.30, 0.25)
        };
        if bc.top != c {
            *bc = BorderColor::all(c);
        }
    }
}

// ---- framing --------------------------------------------------------------------------------

fn drag_crop(
    buttons: Res<ButtonInput<MouseButton>>,
    mut picker: ResMut<Picker>,
    mut settings: ResMut<Settings>,
    mut card_images: ResMut<CardImages>,
    images: Res<Assets<Image>>,
    art: Query<&RelativeCursorPosition, With<CropArt>>,
) {
    if !buttons.pressed(MouseButton::Left) {
        picker.grab = None;
        return;
    }
    let Ok(rel) = art.single() else { return };
    let Some(cursor) = rel.normalized.map(|n| n + Vec2::splat(0.5)) else { return };
    let Some(size) = art_size(&mut card_images, &images, &settings.avatar.card) else { return };
    let a = settings.avatar.clamped(size.x, size.y);
    if buttons.just_pressed(MouseButton::Left) && rel.cursor_over {
        // grabbing the circle keeps the spot under the cursor; clicking elsewhere jumps there
        let r = size.x.min(size.y) / a.zoom * 0.5;
        let centre = Vec2::new(a.x, a.y);
        let inside = ((cursor - centre) * size).length() <= r;
        picker.grab = Some(if inside { centre - cursor } else { Vec2::ZERO });
    }
    let Some(grab) = picker.grab else { return };
    let want = cursor + grab;
    let moved = Avatar { x: want.x, y: want.y, ..a }.clamped(size.x, size.y);
    if moved != settings.avatar {
        settings.avatar = moved;
    }
}

fn drag_zoom(buttons: Res<ButtonInput<MouseButton>>, mut picker: ResMut<Picker>, mut settings: ResMut<Settings>, track: Query<&RelativeCursorPosition, With<ZoomTrack>>) {
    let Ok(rel) = track.single() else { return };
    if buttons.just_pressed(MouseButton::Left) && rel.cursor_over {
        picker.zoom_drag = true;
    }
    if !buttons.pressed(MouseButton::Left) {
        picker.zoom_drag = false;
    }
    if !picker.zoom_drag {
        return;
    }
    let Some(n) = rel.normalized else { return };
    let z = 1.0 + (n.x + 0.5).clamp(0.0, 1.0) * (Avatar::MAX_ZOOM - 1.0);
    if (z - settings.avatar.zoom).abs() > 1e-3 {
        settings.avatar.zoom = z;
    }
}

fn wheel_zoom(mut wheel: MessageReader<MouseWheel>, mut settings: ResMut<Settings>, art: Query<&RelativeCursorPosition, With<CropArt>>) {
    let steps: f32 = wheel
        .read()
        .map(|e| match e.unit {
            MouseScrollUnit::Line => e.y,
            MouseScrollUnit::Pixel => e.y / 40.0,
        })
        .sum();
    if steps == 0.0 || !art.single().is_ok_and(|r| r.cursor_over) {
        return;
    }
    let z = (settings.avatar.zoom * 1.1f32.powf(steps)).clamp(1.0, Avatar::MAX_ZOOM);
    if z != settings.avatar.zoom {
        settings.avatar.zoom = z;
    }
}

fn show_editor(
    settings: Res<Settings>,
    mut card_images: ResMut<CardImages>,
    images: Res<Assets<Image>>,
    mut art: Query<(&mut Node, &mut ImageNode), (With<CropArt>, Without<CropCircle>, Without<ZoomFill>)>,
    mut circle: Query<(&mut Node, &mut Visibility, Has<CropDim>), (Or<(With<CropCircle>, With<CropDim>)>, Without<CropArt>, Without<ZoomFill>)>,
    mut fill: Query<&mut Node, (With<ZoomFill>, Without<CropArt>, Without<CropCircle>)>,
    mut texts: ParamSet<(Query<&mut Text, With<CardTitle>>, Query<&mut Text, With<ZoomText>>)>,
) {
    let a = &settings.avatar;
    let handle = card_images.request_raw(art_request(&a.card));
    let size = images.get(&handle).map(|i| i.size().as_vec2());
    if let Ok((mut node, mut img)) = art.single_mut() {
        if img.image != handle {
            img.image = handle;
        }
        // content box (inside the 2px border) keeps the art's aspect, so the circle maps 1:1
        let h = Val::Px(size.map(|s| (EDIT_W - 4.0) * s.y / s.x + 4.0).unwrap_or(EDIT_W * 0.7));
        if node.height != h {
            node.height = h;
        }
    }
    for (mut node, mut vis, dim) in &mut circle {
        let want = if size.is_some() { Visibility::Inherited } else { Visibility::Hidden };
        if *vis != want {
            *vis = want;
        }
        if let Some(size) = size {
            let r = crop_rect(a, size);
            let k = (EDIT_W - 4.0) / size.x;
            // the ring reaches just past the art's farthest corner: a much larger border stops
            // rendering at all
            let reach = (r.center().max(size - r.center()) * k).length() - r.width() * k * 0.5;
            let grow = if dim { reach.max(0.0) + 4.0 } else { 0.0 };
            let (l, t, w) = (Val::Px(r.min.x * k - grow), Val::Px(r.min.y * k - grow), Val::Px(r.width() * k + 2.0 * grow));
            if node.left != l || node.top != t || node.width != w {
                node.left = l;
                node.top = t;
                node.width = w;
                node.height = w;
                if dim {
                    node.border = UiRect::all(Val::Px(grow));
                }
            }
        }
    }
    if let Ok(mut n) = fill.single_mut() {
        let w = Val::Percent((a.zoom - 1.0) / (Avatar::MAX_ZOOM - 1.0) * 100.0);
        if n.width != w {
            n.width = w;
        }
    }
    if settings.is_changed() {
        for mut t in &mut texts.p0() {
            t.0 = a.card.clone();
        }
        for mut t in &mut texts.p1() {
            t.0 = format!("{:.1}×", a.zoom);
        }
    }
}

fn show_previews(settings: Res<Settings>, mut q: Query<&mut AvatarArt, With<Preview>>) {
    if !settings.is_changed() {
        return;
    }
    for mut art in &mut q {
        if art.avatar.as_ref() != Some(&settings.avatar) {
            art.avatar = Some(settings.avatar.clone());
        }
    }
}
