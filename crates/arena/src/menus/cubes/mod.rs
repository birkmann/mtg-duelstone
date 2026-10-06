//! Cube designer: browse Forge's ~95 bundled cubes (MTGO Legacy/Vintage, Pauper, Peasant, Arena,
//! Old School…) and your own, look at one the way CubeCobra does (table, spoiler, curve, stacks, stats),
//! edit it — search the catalog, paste a list (CubeCobra / MTGO exports) — and send it straight to a draft.
//!
//! Why edits stay local until SAVE: a cube is hundreds of cards and the engine validates names on save,
//! so the screen edits a client-side copy (`CubeEditor.cube`) and only talks to the engine to list, load,
//! search, import and save. Bundled cubes are read-only on the engine; saving one stores your own copy.
//!
//! Why a click selects instead of removing: a cube is browsed with the mouse, card after card, so a
//! click opens the card in the inspector; right-click, Delete or REMOVE take it out, and every edit can
//! be undone (Ctrl+Z). Actions that would drop unsaved edits ask for a second click instead of a dialog.
//!
//! Why its own action enum (`CubeAction`) instead of `MenuAction`: the screen's clicks are edits of
//! cube-local state that never touch other screens; only CUBES on the menus leads in.

mod model;
mod side;
mod views;

use bevy::input_focus::InputFocus;
use bevy::picking::events::{Click, Pointer};
use bevy::prelude::*;
use bevy::text::EditableText;
use crossbeam_channel::Receiver;
use mtg_client::Api;
use mtg_protocol::{CardQuery, CatalogCard, Cube, CubeCard, CubeInfo, EventKind};

use super::widgets::Field;
use crate::shared::*;
use crate::style::ArenaButton;
use model::{Filter, Undo};

pub struct CubesPlugin;

impl Plugin for CubesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CubeEditor>()
            .add_systems(OnEnter(AppState::CubeDesigner), (start, side::spawn_screen).chain())
            .add_systems(
                Update,
                (
                    poll,
                    handle_clicks,
                    right_click,
                    keys,
                    read_fields,
                    start_search,
                    side::sync_regions,
                    views::sync_body,
                    views::lazy_art,
                    views::card_styles,
                    side::chips_and_buttons,
                    side::texts,
                    side::overlays,
                    side::preview_and_toast,
                )
                    .chain()
                    .run_if(in_state(AppState::CubeDesigner)),
            );
    }
}

// ---- state ----------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum View {
    #[default]
    Table,
    Spoiler,
    Curve,
    Stacks,
    Stats,
}

impl View {
    const ALL: [View; 5] = [View::Table, View::Spoiler, View::Curve, View::Stacks, View::Stats];
    fn label(self) -> &'static str {
        match self {
            View::Table => "TABLE",
            View::Spoiler => "SPOILER",
            View::Curve => "CURVE",
            View::Stacks => "STACKS",
            View::Stats => "STATS",
        }
    }
}

/// Card picture width in the spoiler view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum TileSize {
    S,
    #[default]
    M,
    L,
}

impl TileSize {
    const ALL: [TileSize; 3] = [TileSize::S, TileSize::M, TileSize::L];
    fn width(self) -> f32 {
        match self {
            TileSize::S => 100.0,
            TileSize::M => 140.0,
            TileSize::L => 190.0,
        }
    }
    fn label(self) -> &'static str {
        match self {
            TileSize::S => "S",
            TileSize::M => "M",
            TileSize::L => "L",
        }
    }
}

/// The ADD CARDS panel: catalog search with the builder's filters.
#[derive(Debug, Default)]
pub(super) struct Search {
    q: String,
    colors: [bool; 5],
    kind: Option<&'static str>,
    cmc: Option<i32>,
    results: Vec<CubeCard>,
    busy: bool,
    error: Option<String>,
    /// when the query last changed; the request goes out once typing pauses
    dirty_at: Option<f32>,
    /// generation of the newest request (older answers are dropped)
    generation: u64,
}

/// The PASTE A LIST dialog and what the last import could not resolve.
#[derive(Debug, Default)]
pub(super) struct Import {
    open: bool,
    busy: bool,
    added: Option<usize>,
    /// (pasted line, the engine's suggestions)
    unresolved: Vec<(String, Vec<String>)>,
}

#[derive(Resource, Default)]
pub(super) struct CubeEditor {
    list: Vec<CubeInfo>,
    cube: Cube,
    /// saved under this ref (user cubes); None = new or a bundled cube being copied
    saved_ref: Option<String>,
    bundled: bool,
    dirty: bool,
    loading: bool,
    view: View,
    tile: TileSize,
    filter: Filter,
    /// card shown in the inspector
    selected: Option<String>,
    /// card under the pointer (name, printing), for the floating preview
    hover: Option<(String, Option<String>)>,
    undo: Undo,
    drawer: bool,
    library_filter: String,
    /// an action that would lose edits (or delete), waiting for its second click
    confirm: Option<CubeAction>,
    search: Search,
    import: Import,
    /// (message, shown at, is an error)
    toast: Option<(String, f32, bool)>,
    pending: Vec<Receiver<Reply>>,
    /// `Time::elapsed_secs` of this frame (stamps toasts and debounces search)
    now: f32,
    /// what each rebuildable region shows; bumped when it must be rebuilt
    lib_rev: u64,
    body_rev: u64,
    results_rev: u64,
    inspector_rev: u64,
    import_rev: u64,
}

impl CubeEditor {
    fn say(&mut self, s: impl Into<String>, error: bool) {
        self.toast = Some((s.into(), self.now, error));
    }
    /// Apply an edit to the card list: snapshot for undo, mark unsaved, rebuild what shows cards.
    fn edit(&mut self, f: impl FnOnce(&mut Vec<CubeCard>)) {
        self.undo.push(&self.cube.cards);
        f(&mut self.cube.cards);
        self.cards_changed();
    }
    fn cards_changed(&mut self) {
        self.dirty = true;
        self.body_rev += 1;
        self.results_rev += 1;
        self.inspector_rev += 1;
        if self.selected.as_ref().is_some_and(|s| !self.cube.cards.iter().any(|c| &c.name == s)) {
            self.selected = None;
        }
    }
    fn current_ref(&self) -> Option<String> {
        self.saved_ref.clone().or_else(|| self.cube.cube_ref.clone())
    }
    fn select(&mut self, name: Option<String>) {
        if self.selected != name {
            self.selected = name;
            self.inspector_rev += 1;
        }
    }
    fn set_view(&mut self, v: View) {
        if self.view != v {
            self.view = v;
            self.body_rev += 1;
        }
    }
    /// Start a fresh cube (the drawer opens so an empty screen still offers the library).
    fn reset(&mut self) {
        self.cube = Cube { name: "My Cube".into(), packs: 3, pack_size: 15, ..default() };
        self.saved_ref = None;
        self.bundled = false;
        self.dirty = false;
        self.undo.clear();
        self.selected = None;
        self.filter = Filter::default();
        self.body_rev += 1;
        self.results_rev += 1;
        self.inspector_rev += 1;
        self.lib_rev += 1;
    }
    /// True when `act` may go ahead; with unsaved edits the first click only warns.
    fn may_discard(&mut self, act: &CubeAction) -> bool {
        if !self.dirty || self.confirm.as_ref() == Some(act) {
            self.confirm = None;
            return true;
        }
        self.confirm = Some(act.clone());
        self.say("Unsaved changes — click again to discard them.", true);
        false
    }
    fn add(&mut self, card: CubeCard) {
        let name = card.name.clone();
        if self.cube.cards.iter().any(|c| c.name == name) {
            self.select(Some(name));
            return;
        }
        self.edit(|cards| cards.push(CubeCard { qty: 1, ..card }));
        self.say(format!("Added {name}."), false);
    }
    fn remove(&mut self, name: &str) {
        if !self.cube.cards.iter().any(|c| c.name == name) {
            return;
        }
        self.edit(|cards| cards.retain(|c| c.name != name));
        self.say(format!("Removed {name} — Ctrl+Z to undo."), false);
    }
    fn undo(&mut self) {
        match self.undo.pop() {
            Some(cards) => {
                self.cube.cards = cards;
                self.cards_changed();
                self.say("Undone.", false);
            }
            None => self.say("Nothing to undo.", false),
        }
    }
}

#[derive(Component, Debug, Clone, PartialEq)]
pub(super) enum CubeAction {
    Back,
    Drawer,
    Open(String),
    New,
    Save,
    Delete,
    Draft,
    View(View),
    Tile(TileSize),
    /// show / hide a colour column of the views
    ToggleGroup(usize),
    ClearFilter,
    /// a card in the cube: open it in the inspector
    Select(String),
    Deselect,
    Remove(String),
    Copies(String, i32),
    /// a search result: add it (or select it when it's already in)
    Add(Box<CubeCard>),
    SearchColor(usize),
    SearchKind(Option<&'static str>),
    SearchCmc(Option<i32>),
    OpenImport,
    CloseImport,
    Import,
    /// (unresolved pasted line, suggested card name)
    Suggest(String, String),
    Packs(i32),
    PackSize(i32),
    Undo,
}

/// A container whose children are rebuilt wholesale when the revision it shows moves on (as in the
/// deck builder): edits are human-paced, and a few hundred nodes are cheaper to rebuild than to diff.
#[derive(Component)]
pub(super) struct Region {
    kind: RegionKind,
    built: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RegionKind {
    Library,
    Body,
    Results,
    Inspector,
    ImportResult,
}

impl Region {
    fn new(kind: RegionKind) -> Self {
        Self { kind, built: None }
    }
}

impl CubeEditor {
    fn rev_of(&self, kind: RegionKind) -> u64 {
        match kind {
            RegionKind::Library => self.lib_rev,
            RegionKind::Body => self.body_rev,
            RegionKind::Results => self.results_rev,
            RegionKind::Inspector => self.inspector_rev,
            RegionKind::ImportResult => self.import_rev,
        }
    }
}

enum Reply {
    List(Result<Vec<CubeInfo>, String>),
    Loaded(Result<Cube, String>),
    Saved(Result<Cube, String>),
    Deleted(Result<(), String>),
    Search(u64, Result<Vec<CatalogCard>, String>),
    Imported(Result<serde_json::Value, String>),
    Suggested(String, Result<serde_json::Value, String>),
}

fn api(cfg: &EngineConfig) -> Api {
    Api::new(cfg.base_url.clone())
}

fn err(e: anyhow::Error) -> String {
    format!("{e:#}")
}

fn request_list(ed: &mut CubeEditor, cfg: &EngineConfig) {
    let a = api(cfg);
    ed.pending.push(mtg_client::spawn(async move { Reply::List(a.cubes().await.map_err(err)) }));
}

fn request_cube(ed: &mut CubeEditor, cfg: &EngineConfig, r: String) {
    let a = api(cfg);
    ed.loading = true;
    ed.pending.push(mtg_client::spawn(async move { Reply::Loaded(a.cube(&r).await.map_err(err)) }));
}

fn start(mut ed: ResMut<CubeEditor>, cfg: Res<EngineConfig>, time: Res<Time>) {
    ed.now = time.elapsed_secs();
    request_list(&mut ed, &cfg);
    if ed.cube.name.is_empty() {
        ed.reset();
    }
    ed.drawer = ed.cube.cards.is_empty();
    ed.import = Import::default();
    ed.confirm = None;
    ed.hover = None;
    ed.toast = None;
    ed.search.dirty_at = Some(f32::NEG_INFINITY);
    // dev aids for unattended screenshots: ARENA_CUBE=<ref> opens that cube, ARENA_CUBE_VIEW=<view> picks the view
    if let Ok(r) = std::env::var("ARENA_CUBE") {
        request_cube(&mut ed, &cfg, r);
        ed.drawer = false;
    }
    if let Ok(v) = std::env::var("ARENA_CUBE_VIEW")
        && let Some(v) = View::ALL.into_iter().find(|x| x.label().eq_ignore_ascii_case(&v))
    {
        ed.view = v;
    }
}

fn set_field(fields: &mut Query<(&Field, &mut EditableText)>, which: Field, text: &str) {
    for (f, mut t) in fields.iter_mut() {
        if *f == which && t.value() != text {
            t.editor_mut().set_text(text);
        }
    }
}

fn poll(mut ed: ResMut<CubeEditor>, cfg: Res<EngineConfig>, time: Res<Time>, mut fields: Query<(&Field, &mut EditableText)>) {
    ed.now = time.elapsed_secs();
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
                ed.lib_rev += 1;
            }
            Reply::Loaded(Ok(c)) => {
                ed.loading = false;
                ed.reset();
                ed.bundled = c.cube_ref.as_deref().is_some_and(|r| r.starts_with("bundled:"));
                ed.saved_ref = c.cube_ref.clone().filter(|_| !ed.bundled);
                let name = if ed.bundled { format!("{} (my copy)", c.name) } else { c.name.clone() };
                ed.cube = Cube { name: name.clone(), ..c };
                ed.drawer = false;
                set_field(&mut fields, Field::CubeName, &name);
                set_field(&mut fields, Field::CubeFilter, "");
                if ed.bundled {
                    ed.say("Bundled cube — saving stores your own copy.", false);
                }
            }
            Reply::Saved(Ok(c)) => {
                ed.saved_ref = c.cube_ref.clone();
                ed.bundled = false;
                ed.cube = Cube { cards: std::mem::take(&mut ed.cube.cards), ..c };
                ed.dirty = false;
                let total = model::total(&ed.cube.cards);
                let per = (ed.cube.packs * ed.cube.pack_size).max(1);
                let msg = if total < per * 8 { format!("Saved. {total} cards: enough for {} drafters (8 need {}).", total / per, per * 8) } else { format!("Saved ({total} cards).") };
                ed.say(msg, false);
                ed.lib_rev += 1;
                request_list(&mut ed, &cfg);
            }
            Reply::Deleted(Ok(())) => {
                ed.say("Deleted.", false);
                ed.reset();
                set_field(&mut fields, Field::CubeName, "My Cube");
                ed.drawer = true;
                request_list(&mut ed, &cfg);
            }
            Reply::Search(g, res) => {
                if g != ed.search.generation {
                    continue;
                }
                ed.search.busy = false;
                match res {
                    Ok(cards) => {
                        ed.search.results = cards.iter().map(CubeCard::from).collect();
                        ed.search.error = None;
                    }
                    Err(e) => ed.search.error = Some(e),
                }
                ed.results_rev += 1;
            }
            Reply::Imported(Ok(v)) => {
                ed.import.busy = false;
                let cards: Vec<CubeCard> = serde_json::from_value(v.get("cards").cloned().unwrap_or_default()).unwrap_or_default();
                ed.import.unresolved = v
                    .get("unresolved")
                    .and_then(|u| u.as_array())
                    .map(|a| {
                        a.iter()
                            .map(|u| {
                                let line = u.get("line").and_then(|l| l.as_str()).unwrap_or_default().to_string();
                                let sug = u.get("suggestions").and_then(|s| s.as_array()).map(|s| s.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect()).unwrap_or_default();
                                (line, sug)
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let new: Vec<CubeCard> = cards.into_iter().filter(|c| !ed.cube.cards.iter().any(|x| x.name == c.name)).fold(Vec::new(), |mut v, c| {
                    if !v.iter().any(|x: &CubeCard| x.name == c.name) {
                        v.push(c);
                    }
                    v
                });
                let added = new.len();
                if added > 0 {
                    ed.edit(|cards| cards.extend(new));
                }
                ed.import.added = Some(added);
                ed.import_rev += 1;
                let unresolved = ed.import.unresolved.len();
                if unresolved == 0 {
                    ed.import.open = false;
                    set_field(&mut fields, Field::CubeImportText, "");
                }
                let msg = if unresolved > 0 { format!("Imported {added} cards; {unresolved} lines not recognised.") } else { format!("Imported {added} cards.") };
                ed.say(msg, unresolved > 0);
            }
            Reply::Suggested(line, Ok(v)) => {
                let cards: Vec<CubeCard> = serde_json::from_value(v.get("cards").cloned().unwrap_or_default()).unwrap_or_default();
                if let Some(c) = cards.into_iter().next() {
                    ed.import.unresolved.retain(|(l, _)| *l != line);
                    ed.import_rev += 1;
                    ed.add(c);
                }
            }
            Reply::Loaded(Err(e)) => {
                ed.loading = false;
                ed.say(e, true);
            }
            Reply::Imported(Err(e)) => {
                ed.import.busy = false;
                ed.say(e, true);
            }
            Reply::List(Err(e)) | Reply::Saved(Err(e)) | Reply::Deleted(Err(e)) | Reply::Suggested(_, Err(e)) => ed.say(e, true),
        }
    }
}

fn field(fields: &Query<(&Field, &mut EditableText)>, which: Field) -> String {
    fields.iter().find(|(f, _)| **f == which).map(|(_, t)| t.value().to_string()).unwrap_or_default()
}

fn save(ed: &mut CubeEditor, cfg: &EngineConfig) {
    let mut c = ed.cube.clone();
    c.name = c.name.trim().to_string();
    c.previous_name = ed.saved_ref.as_deref().and_then(|r| r.strip_prefix("user:")).map(str::to_owned).filter(|p| *p != c.name);
    c.cube_ref = None;
    if c.cards.is_empty() {
        ed.say("Add some cards first.", true);
        return;
    }
    if c.name.is_empty() {
        ed.say("Give the cube a name first.", true);
        return;
    }
    ed.say("Saving…", false);
    let a = api(cfg);
    ed.pending.push(mtg_client::spawn(async move { Reply::Saved(a.save_cube(&c).await.map_err(err)) }));
}

fn handle_clicks(
    q: Query<(&Interaction, &CubeAction, Option<&ArenaButton>), Changed<Interaction>>,
    mut ed: ResMut<CubeEditor>,
    cfg: Res<EngineConfig>,
    mut fields: Query<(&Field, &mut EditableText)>,
    mut choice: ResMut<EventSetupChoice>,
    mut next: ResMut<NextState<AppState>>,
) {
    for (i, act, btn) in &q {
        if *i != Interaction::Pressed || btn.is_some_and(|b| b.disabled) {
            continue;
        }
        // a second click confirms only when nothing else was clicked in between
        if ed.confirm.as_ref().is_some_and(|c| c != act) {
            ed.confirm = None;
        }
        match act {
            CubeAction::Back => {
                if ed.may_discard(act) {
                    next.set(AppState::MainMenu);
                }
            }
            CubeAction::Drawer => ed.drawer = !ed.drawer,
            CubeAction::Open(r) => {
                if ed.current_ref().as_deref() == Some(r.as_str()) && !ed.dirty {
                    ed.drawer = false;
                } else if ed.may_discard(act) {
                    ed.say("Loading…", false);
                    request_cube(&mut ed, &cfg, r.clone());
                }
            }
            CubeAction::New => {
                if ed.may_discard(act) {
                    ed.reset();
                    ed.drawer = false;
                    set_field(&mut fields, Field::CubeName, "My Cube");
                    set_field(&mut fields, Field::CubeFilter, "");
                    ed.say("New cube: search or paste cards on the right.", false);
                }
            }
            CubeAction::Save => save(&mut ed, &cfg),
            CubeAction::Delete => {
                let Some(name) = ed.saved_ref.as_deref().and_then(|r| r.strip_prefix("user:")).map(str::to_owned) else {
                    ed.say("Only your own saved cubes can be deleted.", true);
                    continue;
                };
                if ed.confirm.as_ref() != Some(act) {
                    ed.confirm = Some(act.clone());
                    ed.say(format!("Click DELETE again to delete “{name}”."), true);
                    continue;
                }
                ed.confirm = None;
                let a = api(&cfg);
                ed.pending.push(mtg_client::spawn(async move { Reply::Deleted(a.delete_cube(&name).await.map_err(err)) }));
            }
            CubeAction::Draft => match ed.current_ref() {
                Some(r) if !ed.dirty => {
                    *choice = EventSetupChoice(EventKind::CubeDraft, Some(r));
                    next.set(AppState::EventSetup);
                }
                _ => ed.say("Save the cube before drafting it.", true),
            },
            CubeAction::View(v) => ed.set_view(*v),
            CubeAction::Tile(t) => {
                if ed.tile != *t {
                    ed.tile = *t;
                    ed.body_rev += 1;
                }
            }
            CubeAction::ToggleGroup(g) => {
                ed.filter.hidden[*g] = !ed.filter.hidden[*g];
                ed.body_rev += 1;
            }
            CubeAction::ClearFilter => {
                ed.filter = Filter::default();
                set_field(&mut fields, Field::CubeFilter, "");
                ed.body_rev += 1;
            }
            CubeAction::Select(n) => {
                let n = (ed.selected.as_ref() != Some(n)).then(|| n.clone());
                ed.select(n);
            }
            CubeAction::Deselect => ed.select(None),
            CubeAction::Remove(n) => ed.remove(n),
            CubeAction::Copies(n, d) => {
                let Some(q) = ed.cube.cards.iter().find(|c| &c.name == n).map(model::copies) else { continue };
                let want = (q + d).clamp(1, 99);
                if want != q {
                    ed.edit(|cards| {
                        if let Some(c) = cards.iter_mut().find(|c| &c.name == n) {
                            c.qty = want;
                        }
                    });
                }
            }
            CubeAction::Add(c) => ed.add((**c).clone()),
            CubeAction::SearchColor(i) => {
                ed.search.colors[*i] = !ed.search.colors[*i];
                ed.search.dirty_at = Some(f32::NEG_INFINITY);
            }
            CubeAction::SearchKind(k) => {
                ed.search.kind = if ed.search.kind == *k { None } else { *k };
                ed.search.dirty_at = Some(f32::NEG_INFINITY);
            }
            CubeAction::SearchCmc(v) => {
                ed.search.cmc = if ed.search.cmc == *v { None } else { *v };
                ed.search.dirty_at = Some(f32::NEG_INFINITY);
            }
            CubeAction::OpenImport => {
                ed.import.open = true;
                ed.import_rev += 1;
            }
            CubeAction::CloseImport => ed.import.open = false,
            CubeAction::Import => {
                let text = field(&fields, Field::CubeImportText);
                if text.trim().is_empty() {
                    ed.say("Paste a card list first.", true);
                    continue;
                }
                ed.import.busy = true;
                ed.import_rev += 1;
                let a = api(&cfg);
                ed.pending.push(mtg_client::spawn(async move { Reply::Imported(a.import_cube(&text).await.map_err(err)) }));
            }
            CubeAction::Suggest(line, name) => {
                // the import endpoint resolves one exact name into a full card
                let (a, line, name) = (api(&cfg), line.clone(), name.clone());
                ed.pending.push(mtg_client::spawn(async move { Reply::Suggested(line, a.import_cube(&name).await.map_err(err)) }));
            }
            CubeAction::Packs(d) => {
                let v = (ed.cube.packs + d).clamp(1, 6);
                if v != ed.cube.packs {
                    ed.cube.packs = v;
                    ed.dirty = true;
                }
            }
            CubeAction::PackSize(d) => {
                let v = (ed.cube.pack_size + d).clamp(5, 20);
                if v != ed.cube.pack_size {
                    ed.cube.pack_size = v;
                    ed.dirty = true;
                }
            }
            CubeAction::Undo => ed.undo(),
        }
    }
}

/// Right-click a card anywhere on the screen to take it out of the cube.
fn right_click(mut clicks: MessageReader<Pointer<Click>>, cards: Query<&views::CardRef>, parents: Query<&ChildOf>, mut ed: ResMut<CubeEditor>) {
    for c in clicks.read() {
        if c.button != PointerButton::Secondary {
            continue;
        }
        // the pick may land on a child (image, name): walk up to the card
        let mut e = c.entity;
        for _ in 0..4 {
            if let Ok(r) = cards.get(e) {
                if r.in_cube {
                    let name = r.name.clone();
                    ed.remove(&name);
                }
                break;
            }
            match parents.get(e) {
                Ok(p) => e = p.parent(),
                Err(_) => break,
            }
        }
    }
}

fn keys(keys: Res<ButtonInput<KeyCode>>, focus: Res<InputFocus>, mut ed: ResMut<CubeEditor>, cfg: Res<EngineConfig>) {
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight, KeyCode::SuperLeft, KeyCode::SuperRight]);
    if ctrl && keys.just_pressed(KeyCode::KeyS) {
        save(&mut ed, &cfg);
        return;
    }
    if keys.just_pressed(KeyCode::Escape) {
        if ed.import.open {
            ed.import.open = false;
        } else if ed.drawer {
            ed.drawer = false;
        } else {
            ed.select(None);
        }
        return;
    }
    // everything else is plain typing while a text field has focus
    if focus.get().is_some() {
        return;
    }
    if ctrl && keys.just_pressed(KeyCode::KeyZ) {
        ed.undo();
    } else if keys.just_pressed(KeyCode::Delete)
        && let Some(n) = ed.selected.clone()
    {
        ed.remove(&n);
    }
}

fn read_fields(q: Query<(&Field, &EditableText), Changed<EditableText>>, mut ed: ResMut<CubeEditor>) {
    for (f, t) in &q {
        let v = t.value().to_string();
        match f {
            Field::CubeName if ed.cube.name != v => {
                ed.cube.name = v;
                ed.dirty = true;
            }
            Field::CubeSearch if ed.search.q != v => {
                ed.search.q = v;
                ed.search.dirty_at = Some(ed.now);
            }
            Field::CubeFilter if ed.filter.text != v => {
                ed.filter.text = v;
                ed.body_rev += 1;
            }
            Field::CubeLibraryFilter if ed.library_filter != v => {
                ed.library_filter = v;
                ed.lib_rev += 1;
            }
            _ => {}
        }
    }
}

/// Typing pause before the catalog search goes out.
const SEARCH_DEBOUNCE: f32 = 0.25;

fn start_search(mut ed: ResMut<CubeEditor>, cfg: Res<EngineConfig>) {
    let Some(at) = ed.search.dirty_at else { return };
    if ed.now - at < SEARCH_DEBOUNCE {
        return;
    }
    ed.search.dirty_at = None;
    ed.search.generation += 1;
    ed.search.busy = true;
    let s = &ed.search;
    let ci: String = mtg_client::deckstats::COLORS.iter().zip(s.colors).filter(|(_, on)| *on).map(|(c, _)| *c).collect();
    let q = CardQuery { q: s.q.trim().to_string(), ci: (!ci.is_empty()).then_some(ci), type_line: s.kind.map(str::to_owned), cmc: s.cmc, limit: 60, ..default() };
    let (a, g) = (api(&cfg), s.generation);
    ed.pending.push(mtg_client::spawn(async move { Reply::Search(g, a.catalog_search(&q).await.map_err(err)) }));
}
