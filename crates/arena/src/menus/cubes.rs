//! Cube designer: browse Forge's ~95 bundled cubes (MTGO Legacy/Vintage, Pauper, Peasant, Arena,
//! Old School…), design your own — search the card catalog or paste a list (CubeCobra / MTGO exports) —
//! and send any cube straight to a draft.
//!
//! Why edits stay local until SAVE: a cube is hundreds of cards and the engine validates names on save,
//! so the screen edits a client-side copy (`CubeEditor.cube`) and only talks to the engine to list, load,
//! search, import and save. Bundled cubes are read-only on the engine; saving one stores your own copy.

use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::RelativeCursorPosition;
use crossbeam_channel::Receiver;
use mtg_client::{Api, ImageRequest, ImageVersion};
use mtg_protocol::{Cube, CubeCard, CubeInfo, EventKind};

use super::MenuAction;
use super::widgets::{Field, text_field};
use crate::images::CardImages;
use crate::shared::*;
use crate::style::*;

pub struct CubesPlugin;

impl Plugin for CubesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CubeEditor>()
            .add_systems(OnEnter(AppState::CubeDesigner), (start, spawn_screen).chain())
            .add_systems(Update, (poll, handle, hover, rebuild).chain().run_if(in_state(AppState::CubeDesigner)));
    }
}

enum Reply {
    List(Result<Vec<CubeInfo>, String>),
    Loaded(Result<Cube, String>),
    Saved(Result<Cube, String>),
    Deleted(Result<(), String>),
    Search(Result<Vec<CubeCard>, String>),
    Imported(Result<serde_json::Value, String>),
}

#[derive(Resource, Default)]
struct CubeEditor {
    list: Vec<CubeInfo>,
    cube: Cube,
    /// saved under this ref (user cubes); None = new or a bundled cube being copied
    saved_ref: Option<String>,
    bundled: bool,
    dirty: bool,
    results: Vec<CubeCard>,
    status: String,
    error: bool,
    pending: Vec<Receiver<Reply>>,
    hover: Option<(String, Option<String>)>,
    version: u64,
}

impl CubeEditor {
    fn say(&mut self, s: impl Into<String>, error: bool) {
        self.status = s.into();
        self.error = error;
        self.version += 1;
    }
}

#[derive(Component)]
struct ListPane;
#[derive(Component)]
struct CardsPane;
#[derive(Component)]
struct ResultsPane;
#[derive(Component)]
struct InfoText;
#[derive(Component)]
struct StatusText;
#[derive(Component)]
struct PreviewImage;
#[derive(Component)]
struct HoverCard(String, Option<String>);

fn api(cfg: &EngineConfig) -> Api {
    Api::new(cfg.base_url.clone())
}

fn start(mut ed: ResMut<CubeEditor>, cfg: Res<EngineConfig>) {
    let a = api(&cfg);
    ed.pending.push(mtg_client::spawn(async move { Reply::List(a.cubes().await.map_err(|e| format!("{e:#}"))) }));
    if ed.cube.name.is_empty() {
        ed.cube = Cube { name: "My Cube".into(), packs: 3, pack_size: 15, ..default() };
    }
    ed.version += 1;
}

fn spawn_screen(mut commands: Commands, fonts: Res<ArenaFonts>, ed: Res<CubeEditor>) {
    commands
        .spawn((
            Name::new("CubeDesigner"),
            DespawnOnExit(AppState::CubeDesigner),
            Node { flex_direction: FlexDirection::Column, padding: UiRect::all(Val::Px(20.0)), row_gap: Val::Px(12.0), ..fullscreen_root() },
        ))
        .with_children(|root| {
            root.spawn(Node { width: Val::Percent(100.0), align_items: AlignItems::Center, column_gap: Val::Px(14.0), ..default() }).with_children(|bar| {
                let b = spawn_button(bar, &fonts, ButtonKind::Secondary, "BACK", Vec2::new(120.0, 44.0));
                bar.commands().entity(b).insert(MenuAction::ToMainMenu);
                bar.spawn(Node { flex_grow: 1.0, justify_content: JustifyContent::Center, ..default() }).with_child(heading(&fonts, "CUBES", 40.0));
                let b = spawn_button(bar, &fonts, ButtonKind::Primary, "DRAFT THIS CUBE", Vec2::new(240.0, 48.0));
                bar.commands().entity(b).insert(MenuAction::CubeDraft);
            });
            root.spawn(Node { width: Val::Percent(100.0), flex_grow: 1.0, min_height: Val::Px(0.0), column_gap: Val::Px(14.0), ..default() }).with_children(|mid| {
                // ---- cube list
                mid.spawn(ornate_panel(Node { width: Val::Px(290.0), padding: UiRect::all(Val::Px(12.0)), flex_direction: FlexDirection::Column, row_gap: Val::Px(8.0), ..default() }))
                    .with_children(|p| {
                        let b = spawn_button(p, &fonts, ButtonKind::Secondary, "NEW CUBE", Vec2::new(264.0, 42.0));
                        p.commands().entity(b).insert(MenuAction::CubeNew);
                        p.spawn((
                            ListPane,
                            ScrollPosition::default(),
                            RelativeCursorPosition::default(),
                            super::widgets::WheelScroll,
                            Node { flex_grow: 1.0, min_height: Val::Px(0.0), flex_direction: FlexDirection::Column, row_gap: Val::Px(3.0), overflow: Overflow::scroll_y(), ..default() },
                        ));
                    });
                // ---- editor
                mid.spawn(ornate_panel(Node { flex_grow: 1.0, min_width: Val::Px(0.0), padding: UiRect::all(Val::Px(14.0)), flex_direction: FlexDirection::Column, row_gap: Val::Px(8.0), ..default() }))
                    .with_children(|p| {
                        p.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(10.0), ..default() }).with_children(|r| {
                            r.spawn(text_field(&fonts, Field::CubeName, &ed.cube.name, Val::Px(360.0), 1.0, false));
                            r.spawn((InfoText, Node { flex_grow: 1.0, ..default() }, ui_text(&fonts.body, "", 15.0, TEXT_DIM)));
                            let b = spawn_button(r, &fonts, ButtonKind::Primary, "SAVE", Vec2::new(120.0, 44.0));
                            r.commands().entity(b).insert(MenuAction::CubeSave);
                            let b = spawn_button(r, &fonts, ButtonKind::Danger, "DELETE", Vec2::new(120.0, 44.0));
                            r.commands().entity(b).insert(MenuAction::CubeDelete);
                        });
                        p.spawn((StatusText, ui_text(&fonts.body, "", 14.0, TEXT_DIM)));
                        p.spawn((
                            CardsPane,
                            ScrollPosition::default(),
                            RelativeCursorPosition::default(),
                            super::widgets::WheelScroll,
                            Node { flex_grow: 1.0, min_height: Val::Px(0.0), flex_direction: FlexDirection::Column, row_gap: Val::Px(8.0), overflow: Overflow::scroll_y(), ..default() },
                        ));
                    });
                // ---- search / import / preview
                mid.spawn(ornate_panel(Node { width: Val::Px(360.0), padding: UiRect::all(Val::Px(12.0)), flex_direction: FlexDirection::Column, row_gap: Val::Px(8.0), ..default() }))
                    .with_children(|p| {
                        p.spawn(ui_text(&fonts.title, "ADD CARDS", 18.0, GOLD));
                        p.spawn(Node { column_gap: Val::Px(8.0), ..default() }).with_children(|r| {
                            r.spawn(text_field(&fonts, Field::CubeSearch, "", Val::Px(220.0), 1.0, false));
                            let b = spawn_button(r, &fonts, ButtonKind::Secondary, "SEARCH", Vec2::new(100.0, 40.0));
                            r.commands().entity(b).insert(MenuAction::CubeSearch);
                        });
                        p.spawn((
                            ResultsPane,
                            ScrollPosition::default(),
                            RelativeCursorPosition::default(),
                            super::widgets::WheelScroll,
                            Node { height: Val::Px(220.0), flex_direction: FlexDirection::Column, row_gap: Val::Px(2.0), overflow: Overflow::scroll_y(), ..default() },
                        ));
                        p.spawn(ui_text(&fonts.body, "Paste a list (\"1 Lightning Bolt\", CubeCobra/MTGO export):", 13.0, TEXT_DIM));
                        p.spawn(text_field(&fonts, Field::CubeImportText, "", Val::Percent(100.0), 4.0, true));
                        let b = spawn_button(p, &fonts, ButtonKind::Secondary, "IMPORT LIST", Vec2::new(336.0, 40.0));
                        p.commands().entity(b).insert(MenuAction::CubeImport);
                        p.spawn((
                            PreviewImage,
                            ImageNode::default(),
                            Node { width: Val::Px(220.0), height: Val::Px(306.0), align_self: AlignSelf::Center, border_radius: BorderRadius::all(Val::Px(10.0)), ..default() },
                            Visibility::Hidden,
                        ));
                    });
            });
        });
}

fn field(fields: &Query<(&Field, &EditableText)>, which: Field) -> String {
    fields.iter().find(|(f, _)| **f == which).map(|(_, t)| t.value().to_string()).unwrap_or_default()
}

fn poll(mut ed: ResMut<CubeEditor>, cfg: Res<EngineConfig>, mut names: Query<(&Field, &mut EditableText)>) {
    let mut replies = Vec::new();
    ed.pending.retain(|rx| match rx.try_recv() {
        Ok(r) => {
            replies.push(r);
            false
        }
        Err(crossbeam_channel::TryRecvError::Empty) => true,
        Err(_) => false,
    });
    for r in replies {
        match r {
            Reply::List(Ok(l)) => {
                ed.list = l;
                ed.version += 1;
            }
            Reply::Loaded(Ok(c)) => {
                ed.bundled = c.cube_ref.as_deref().is_some_and(|r| r.starts_with("bundled:"));
                ed.saved_ref = c.cube_ref.clone().filter(|_| !ed.bundled);
                let n = if ed.bundled { format!("{} (my copy)", c.name) } else { c.name.clone() };
                for (f, mut t) in &mut names {
                    if *f == Field::CubeName {
                        t.editor_mut().set_text(&n);
                    }
                }
                let msg = if ed.bundled { "Bundled cube — saving stores your own copy." } else { "" };
                ed.cube = c;
                ed.dirty = false;
                ed.say(msg, false);
            }
            Reply::Saved(Ok(c)) => {
                ed.saved_ref = c.cube_ref.clone();
                ed.bundled = false;
                ed.cube = c;
                ed.dirty = false;
                let total: i32 = ed.cube.cards.iter().map(|c| c.qty.max(1)).sum();
                let need = ed.cube.packs * ed.cube.pack_size * 8;
                let msg = if total < need { format!("Saved. {total} cards: enough for {} drafters (8 need {need}).", total / (ed.cube.packs * ed.cube.pack_size).max(1)) } else { format!("Saved ({total} cards).") };
                ed.say(msg, false);
                let a = api(&cfg);
                ed.pending.push(mtg_client::spawn(async move { Reply::List(a.cubes().await.map_err(|e| format!("{e:#}"))) }));
            }
            Reply::Deleted(Ok(())) => {
                ed.saved_ref = None;
                ed.say("Deleted.", false);
                let a = api(&cfg);
                ed.pending.push(mtg_client::spawn(async move { Reply::List(a.cubes().await.map_err(|e| format!("{e:#}"))) }));
            }
            Reply::Search(Ok(r)) => {
                let n = r.len();
                ed.results = r;
                ed.say(if n == 0 { "No cards found." } else { "" }, false);
            }
            Reply::Imported(Ok(v)) => {
                let cards: Vec<CubeCard> = serde_json::from_value(v.get("cards").cloned().unwrap_or_default()).unwrap_or_default();
                let unresolved = v.get("unresolved").and_then(|u| u.as_array()).map(|a| a.len()).unwrap_or(0);
                let mut added = 0;
                for c in cards {
                    if !ed.cube.cards.iter().any(|x| x.name == c.name) {
                        ed.cube.cards.push(c);
                        added += 1;
                    }
                }
                ed.dirty = true;
                let msg = if unresolved > 0 { format!("Imported {added} cards; {unresolved} lines not recognised.") } else { format!("Imported {added} cards.") };
                ed.say(msg, unresolved > 0);
            }
            Reply::List(Err(e)) | Reply::Loaded(Err(e)) | Reply::Saved(Err(e)) | Reply::Deleted(Err(e)) | Reply::Search(Err(e)) | Reply::Imported(Err(e)) => ed.say(e, true),
        }
    }
}

fn handle(
    q: Query<(&Interaction, &MenuAction), Changed<Interaction>>,
    mut ed: ResMut<CubeEditor>,
    cfg: Res<EngineConfig>,
    fields: Query<(&Field, &EditableText)>,
    mut choice: ResMut<EventSetupChoice>,
    mut next: ResMut<NextState<AppState>>,
) {
    for (i, act) in &q {
        if *i != Interaction::Pressed {
            continue;
        }
        let a = api(&cfg);
        match act {
            MenuAction::CubeOpen(r) => {
                let r = r.clone();
                ed.say("Loading…", false);
                ed.pending.push(mtg_client::spawn(async move { Reply::Loaded(a.cube(&r).await.map_err(|e| format!("{e:#}"))) }));
            }
            MenuAction::CubeNew => {
                ed.cube = Cube { name: "My Cube".into(), packs: 3, pack_size: 15, ..default() };
                ed.saved_ref = None;
                ed.bundled = false;
                ed.dirty = false;
                ed.say("New cube: search or paste cards on the right.", false);
            }
            MenuAction::CubeAdd(name) => {
                if ed.cube.cards.iter().any(|c| &c.name == name) {
                    ed.say(format!("{name} is already in the cube."), false);
                } else if let Some(c) = ed.results.iter().find(|c| &c.name == name).cloned() {
                    ed.cube.cards.push(CubeCard { qty: 1, ..c });
                    ed.dirty = true;
                    ed.version += 1;
                }
            }
            MenuAction::CubeRemove(name) => {
                ed.cube.cards.retain(|c| &c.name != name);
                ed.dirty = true;
                ed.version += 1;
            }
            MenuAction::CubeSearch => {
                let q = field(&fields, Field::CubeSearch);
                ed.say("Searching…", false);
                ed.pending.push(mtg_client::spawn(async move { Reply::Search(a.search_cards(&q, 80).await.map_err(|e| format!("{e:#}"))) }));
            }
            MenuAction::CubeImport => {
                let text = field(&fields, Field::CubeImportText);
                if text.trim().is_empty() {
                    ed.say("Paste a card list first.", true);
                    continue;
                }
                ed.say("Importing…", false);
                ed.pending.push(mtg_client::spawn(async move { Reply::Imported(a.import_cube(&text).await.map_err(|e| format!("{e:#}"))) }));
            }
            MenuAction::CubeSave => {
                let mut c = ed.cube.clone();
                c.name = field(&fields, Field::CubeName).trim().to_string();
                c.previous_name = ed.saved_ref.as_deref().and_then(|r| r.strip_prefix("user:")).map(str::to_owned).filter(|p| *p != c.name);
                c.cube_ref = None;
                if c.cards.is_empty() {
                    ed.say("Add some cards first.", true);
                    continue;
                }
                ed.say("Saving…", false);
                ed.pending.push(mtg_client::spawn(async move { Reply::Saved(a.save_cube(&c).await.map_err(|e| format!("{e:#}"))) }));
            }
            MenuAction::CubeDelete => {
                let Some(name) = ed.saved_ref.as_deref().and_then(|r| r.strip_prefix("user:")).map(str::to_owned) else {
                    ed.say("Only your own saved cubes can be deleted.", true);
                    continue;
                };
                ed.pending.push(mtg_client::spawn(async move { Reply::Deleted(a.delete_cube(&name).await.map_err(|e| format!("{e:#}"))) }));
            }
            MenuAction::CubeDraft => {
                let r = ed.saved_ref.clone().or_else(|| ed.cube.cube_ref.clone());
                match r {
                    Some(r) if !ed.dirty => {
                        *choice = EventSetupChoice(EventKind::CubeDraft, Some(r));
                        next.set(AppState::EventSetup);
                    }
                    _ => ed.say("Save the cube before drafting it.", true),
                }
            }
            _ => {}
        }
    }
}

/// Hovering a card name shows its image in the preview box.
fn hover(q: Query<(&Interaction, &HoverCard), Changed<Interaction>>, mut ed: ResMut<CubeEditor>, mut images: ResMut<CardImages>, mut preview: Query<(&mut ImageNode, &mut Visibility), With<PreviewImage>>) {
    for (i, h) in &q {
        if *i != Interaction::Hovered {
            continue;
        }
        let key = (h.0.clone(), h.1.clone());
        if ed.hover.as_ref() == Some(&key) {
            continue;
        }
        let img = images.request_raw(ImageRequest { name: h.0.clone(), set: h.1.clone(), back: false, version: ImageVersion::Normal });
        for (mut n, mut v) in &mut preview {
            n.image = img.clone();
            *v = Visibility::Inherited;
        }
        ed.hover = Some(key);
    }
}

fn group_of(c: &CubeCard) -> usize {
    if c.is_land {
        return 7;
    }
    match c.colors.len() {
        0 => 6,
        1 => "WUBRG".find(c.colors.chars().next().unwrap_or('W')).unwrap_or(0),
        _ => 5,
    }
}

const GROUPS: [(&str, Color); 8] = [
    ("WHITE", Color::srgb(0.95, 0.92, 0.78)),
    ("BLUE", Color::srgb(0.45, 0.68, 1.0)),
    ("BLACK", Color::srgb(0.70, 0.62, 0.75)),
    ("RED", Color::srgb(1.0, 0.48, 0.38)),
    ("GREEN", Color::srgb(0.45, 0.85, 0.5)),
    ("MULTICOLOR", Color::srgb(1.0, 0.82, 0.38)),
    ("COLORLESS", Color::srgb(0.75, 0.75, 0.78)),
    ("LANDS", Color::srgb(0.80, 0.68, 0.50)),
];

fn rebuild(
    mut commands: Commands,
    ed: Res<CubeEditor>,
    fonts: Res<ArenaFonts>,
    list: Query<(Entity, Option<&Children>), With<ListPane>>,
    cards: Query<(Entity, Option<&Children>), With<CardsPane>>,
    results: Query<(Entity, Option<&Children>), With<ResultsPane>>,
    mut texts: ParamSet<(Query<&mut Text, With<InfoText>>, Query<(&mut Text, &mut TextColor), With<StatusText>>)>,
    mut shown: Local<u64>,
) {
    if *shown == ed.version && !ed.is_changed() {
        return;
    }
    *shown = ed.version;
    let clear = |commands: &mut Commands, ch: Option<&Children>| {
        for c in ch.into_iter().flatten() {
            commands.entity(*c).despawn();
        }
    };
    // cube list
    if let Ok((e, ch)) = list.single() {
        clear(&mut commands, ch);
        let current = ed.saved_ref.clone().or_else(|| ed.cube.cube_ref.clone());
        commands.entity(e).with_children(|l| {
            for c in &ed.list {
                let on = current.as_deref() == Some(c.cube_ref.as_str());
                l.spawn((
                    Button,
                    MenuAction::CubeOpen(c.cube_ref.clone()),
                    Node { padding: UiRect::axes(Val::Px(8.0), Val::Px(5.0)), column_gap: Val::Px(6.0), border_radius: BorderRadius::all(Val::Px(6.0)), flex_shrink: 0.0, ..default() },
                    BackgroundColor(if on { Color::srgba(0.95, 0.46, 0.13, 0.25) } else if c.user { Color::srgba(1.0, 0.8, 0.4, 0.07) } else { Color::NONE }),
                ))
                .with_children(|r| {
                    r.spawn((Node { flex_grow: 1.0, ..default() }, ui_text(&fonts.body, c.name.clone(), 14.0, if c.user { GOLD_BRIGHT } else { TEXT }), Pickable::IGNORE));
                    r.spawn((ui_text(&fonts.body, c.size.to_string(), 12.0, TEXT_DIM), Pickable::IGNORE));
                });
            }
        });
    }
    // cube contents by colour
    if let Ok((e, ch)) = cards.single() {
        clear(&mut commands, ch);
        let mut groups: Vec<Vec<&CubeCard>> = vec![Vec::new(); GROUPS.len()];
        for c in &ed.cube.cards {
            groups[group_of(c)].push(c);
        }
        commands.entity(e).with_children(|p| {
            for (gi, g) in groups.iter_mut().enumerate() {
                if g.is_empty() {
                    continue;
                }
                g.sort_by(|a, b| (a.cmc, &a.name).cmp(&(b.cmc, &b.name)));
                let (label, col) = GROUPS[gi];
                p.spawn(ui_text(&fonts.title, format!("{label}  ({})", g.len()), 15.0, col));
                p.spawn(Node { flex_wrap: FlexWrap::Wrap, column_gap: Val::Px(4.0), row_gap: Val::Px(4.0), ..default() }).with_children(|w| {
                    for c in g.iter() {
                        w.spawn((
                            Button,
                            MenuAction::CubeRemove(c.name.clone()),
                            HoverCard(c.name.clone(), c.set.clone()),
                            Node { padding: UiRect::axes(Val::Px(8.0), Val::Px(3.0)), border: UiRect::all(Val::Px(1.0)), border_radius: BorderRadius::all(Val::Px(5.0)), ..default() },
                            BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.05)),
                            BorderColor::all(col.with_alpha(0.4)),
                        ))
                        .with_child((ui_text(&fonts.body, c.name.clone(), 13.0, TEXT), Pickable::IGNORE));
                    }
                });
            }
            if ed.cube.cards.is_empty() {
                p.spawn(ui_text(&fonts.body, "This cube is empty. Search for cards or paste a list on the right.", 16.0, TEXT_DIM));
            }
        });
    }
    // search results
    if let Ok((e, ch)) = results.single() {
        clear(&mut commands, ch);
        commands.entity(e).with_children(|p| {
            for c in &ed.results {
                let have = ed.cube.cards.iter().any(|x| x.name == c.name);
                p.spawn((
                    Button,
                    MenuAction::CubeAdd(c.name.clone()),
                    HoverCard(c.name.clone(), c.set.clone()),
                    Node { padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)), column_gap: Val::Px(6.0), flex_shrink: 0.0, border_radius: BorderRadius::all(Val::Px(4.0)), ..default() },
                    BackgroundColor(if have { Color::srgba(0.35, 0.92, 0.45, 0.12) } else { Color::NONE }),
                ))
                .with_children(|r| {
                    r.spawn((Node { flex_grow: 1.0, ..default() }, ui_text(&fonts.body, c.name.clone(), 13.0, TEXT), Pickable::IGNORE));
                    r.spawn((ui_text(&fonts.body, c.mana_cost.clone().unwrap_or_default(), 12.0, TEXT_DIM), Pickable::IGNORE));
                });
            }
        });
    }
    let total: i32 = ed.cube.cards.iter().map(|c| c.qty.max(1)).sum();
    let per_pod = ed.cube.packs.max(1) * ed.cube.pack_size.max(1);
    for mut t in &mut texts.p0() {
        t.0 = format!("{total} cards · {} packs × {} · drafts up to {} players{}", ed.cube.packs, ed.cube.pack_size, (total / per_pod).min(8), if ed.dirty { " · unsaved" } else { "" });
    }
    for (mut t, mut c) in &mut texts.p1() {
        t.0 = ed.status.clone();
        c.0 = if ed.error { RED_DAMAGE } else { TEXT_DIM };
    }
}
