//! Deck builder & tester: search Forge's card pool, get EDHREC-style recommendations (high-synergy
//! cards for the commander / archetype, theme and role suggestions), watch the mana curve and
//! deck stats update live, draw sample hands, then save and play the deck against the AI.
//!
//! Why its own action enum (`BuilderAction`) instead of `MenuAction`: builder clicks are
//! high-volume edits of builder-local state (add a card, toggle a filter) that never change other
//! screens, so they stay next to the model they edit. Only the ways in (NEW / EDIT on deck select)
//! and out (BACK / PLAY) touch shared app flow, and those go through the same resources the other
//! menu screens use (`MatchConfig`, `DeckSelection`).
//!
//! Why threads + channels, as in deck select: `Rest` is blocking. Every request carries a
//! generation number so a slow answer for an old query can never overwrite a newer one.
//!
//! The engine stays the authority on legality: the deck is validated by `POST /api/deck/validate`
//! after every edit and PLAY is only enabled when the engine accepts it. Local copy limits
//! (1 in Commander, 4 in Constructed) are a typing aid, not a rule check.

mod playtest;
mod view;

use bevy::prelude::*;
use bevy::text::EditableText;
use crossbeam_channel::{Receiver, unbounded};
use mtg_client::Rest;
use mtg_client::deckstats::{self, DeckStats, Entry, Goldfish};
use mtg_protocol::{CardQuery, CatalogCard, DeckBody, DeckFull, DeckLine, Format, RecommendRequest, Recommendations};

use super::deck_select::{DeckList, DeckSelection, RefreshDecks, engine_base};
use super::widgets::Field;
use crate::shared::{AppState, EngineConfig, MatchConfig, MatchDeckNames, Settings};
use crate::style::ArenaButton;

pub struct BuilderPlugin;

impl Plugin for BuilderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BuilderOpen>()
            .init_resource::<BuilderDeck>()
            .init_resource::<SearchState>()
            .init_resource::<RecsState>()
            .init_resource::<BuilderUi>()
            .init_resource::<BuilderStats>()
            .init_resource::<BuilderNet>()
            .add_systems(OnEnter(AppState::DeckBuilder), (enter, view::spawn_screen).chain())
            .add_systems(Update, open_from_menu.run_if(in_state(AppState::DeckSelect)))
            .add_systems(Update, debug_open.run_if(in_state(AppState::MainMenu)))
            .add_systems(
                Update,
                (
                    handle_actions,
                    right_click_cards,
                    read_fields,
                    poll_net,
                    start_requests,
                    recompute_stats,
                    view::sync_views,
                    playtest::sync_playtest,
                    view::tiles_and_hover,
                    view::chips_and_badges,
                    view::panels_and_buttons,
                    view::identity_row,
                    view::status_texts,
                    view::preview_and_toast,
                    view::wheel_scroll,
                )
                    .chain()
                    .run_if(in_state(AppState::DeckBuilder)),
            );
    }
}

// =============================================================================================
// State
// =============================================================================================

/// Which deck to open when entering the builder. Written by deck select's NEW / EDIT buttons.
#[derive(Resource, Debug, Clone, Default)]
pub struct BuilderOpen {
    pub format: Format,
    /// None = start a new, empty deck.
    pub deck_ref: Option<String>,
}

/// The deck being edited.
#[derive(Resource, Debug, Default)]
pub struct BuilderDeck {
    pub format: Format,
    pub name: String,
    /// Name it has on disk as a `user:` deck (so a rename replaces it instead of duplicating).
    pub saved_name: Option<String>,
    pub commanders: Vec<CatalogCard>,
    /// In the order cards were added; the list view groups them.
    pub main: Vec<(CatalogCard, u32)>,
    pub dirty: bool,
    /// Bumped on every edit; stats, validation and recommendations follow it.
    pub rev: u64,
    pub loading: bool,
    /// The engine's verdict for `problem_rev` (None = legal).
    pub problem: Option<String>,
    pub problem_rev: u64,
}

impl BuilderDeck {
    pub fn count(&self) -> u32 {
        self.commanders.len() as u32 + self.main.iter().map(|(_, q)| q).sum::<u32>()
    }
    pub fn qty(&self, name: &str) -> u32 {
        self.main.iter().find(|(c, _)| c.name == name).map(|(_, q)| *q).unwrap_or(0)
    }
    pub fn is_commander(&self, name: &str) -> bool {
        self.commanders.iter().any(|c| c.name == name)
    }
    /// Combined colour identity of the commanders (`WUBG`); None outside Commander or without one.
    pub fn identity(&self) -> Option<String> {
        if self.format != Format::Commander || self.commanders.is_empty() {
            return None;
        }
        Some(deckstats::COLORS.iter().filter(|c| self.commanders.iter().any(|k| k.color_identity.contains(**c))).collect())
    }
    /// True when `card` falls outside the commander's colour identity (shown in red).
    pub fn off_identity(&self, card: &CatalogCard) -> bool {
        self.identity().is_some_and(|id| !card.color_identity.chars().all(|c| id.contains(c)))
    }
    fn touch(&mut self) {
        self.dirty = true;
        self.rev += 1;
    }
    /// Add one copy. Errors are user-facing hints.
    pub fn add(&mut self, card: &CatalogCard) -> Result<(), String> {
        if self.is_commander(&card.name) {
            return Err(format!("{} is your commander", card.name));
        }
        let have = self.qty(&card.name);
        let max = max_copies(self.format, card);
        if have >= max {
            return Err(if max == 1 { format!("Commander decks are singleton — {} is already in", card.name) } else { format!("Already {max} copies of {}", card.name) });
        }
        match self.main.iter_mut().find(|(c, _)| c.name == card.name) {
            Some((_, q)) => *q += 1,
            None => self.main.push((card.clone(), 1)),
        }
        self.touch();
        Ok(())
    }
    pub fn remove(&mut self, name: &str) {
        if let Some(i) = self.main.iter().position(|(c, _)| c.name == name) {
            if self.main[i].1 > 1 {
                self.main[i].1 -= 1;
            } else {
                self.main.remove(i);
            }
            self.touch();
        }
    }
    /// Make `card` a commander. A second commander is kept only when both mention partner /
    /// background / "friends forever" (the engine validates the pairing); otherwise it replaces.
    pub fn set_commander(&mut self, card: &CatalogCard) {
        if self.is_commander(&card.name) {
            return;
        }
        let pairs = |c: &CatalogCard| {
            let t = c.oracle_text.as_deref().unwrap_or("").to_ascii_lowercase();
            t.contains("partner") || t.contains("background") || t.contains("friends forever") || c.type_line.contains("Background")
        };
        if !(self.commanders.len() == 1 && pairs(&self.commanders[0]) && pairs(card)) {
            self.commanders.clear();
        }
        // it can't be in the 99 and the command zone at once
        self.main.retain(|(c, _)| c.name != card.name);
        self.commanders.push(card.clone());
        self.touch();
    }
    pub fn remove_commander(&mut self, name: &str) {
        let before = self.commanders.len();
        self.commanders.retain(|c| c.name != name);
        if self.commanders.len() != before {
            self.touch();
        }
    }
    pub fn lines(&self) -> Vec<DeckLine> {
        self.main.iter().map(|(c, q)| DeckLine { name: c.name.clone(), qty: *q }).collect()
    }
    pub fn body(&self) -> DeckBody {
        DeckBody {
            name: self.name.trim().to_string(),
            format: self.format,
            commanders: self.commanders.iter().map(|c| c.name.clone()).collect(),
            main: self.lines(),
            sideboard: Vec::new(),
            previous_name: self.saved_name.clone().filter(|p| *p != self.name.trim()),
        }
    }
    pub fn entries(&self) -> Vec<Entry<'_>> {
        self.main.iter().map(|(c, q)| Entry { card: c, qty: *q }).collect()
    }
}

/// Copy limit used while clicking cards in. Basics and "any number of cards named ~" are free.
fn max_copies(format: Format, card: &CatalogCard) -> u32 {
    let text = card.oracle_text.as_deref().unwrap_or("").to_ascii_lowercase();
    if card.is_basic_land || text.contains("any number of cards named") {
        u32::MAX
    } else if format == Format::Commander {
        1
    } else {
        4
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Search,
    Recommend,
}

/// Card-search filters and results.
#[derive(Resource, Debug, Default)]
pub struct SearchState {
    pub q: String,
    pub text: String,
    /// Constructed colour filter (cards must fit in the ticked colours); empty = any.
    pub colors: [bool; 5],
    pub cmc: Option<i32>,
    /// Type-line filter (`Creature`, `Instant`, …).
    pub kind: Option<&'static str>,
    pub commander_only: bool,
    pub results: Vec<CatalogCard>,
    pub more: bool,
    pub busy: bool,
    pub error: Option<String>,
    /// Filters changed at this time; the search fires after a short debounce.
    pub dirty_at: Option<f32>,
    /// Append the next page instead of replacing.
    pub append: bool,
    generation: u64,
    /// Bumped when `results` change (view rebuild trigger).
    pub shown: u64,
}

pub const PAGE: u32 = 60;

/// Recommendations for the current deck.
#[derive(Resource, Debug, Default)]
pub struct RecsState {
    pub data: Option<Recommendations>,
    /// Themes the user pinned; empty = engine's pick.
    pub pinned: Vec<String>,
    pub busy: bool,
    pub error: Option<String>,
    /// Deck rev + theme change the current data answers; refetch when it differs.
    pub for_rev: Option<(u64, Vec<String>)>,
    pub dirty_at: Option<f32>,
    generation: u64,
    pub shown: u64,
}

#[derive(Resource, Debug, Default)]
pub struct BuilderUi {
    pub tab: Tab,
    pub playtest: Option<playtest::Playtest>,
    /// Card under the pointer (big preview).
    pub hover: Option<CatalogCard>,
    pub toast: Option<(String, f32)>,
    /// BACK pressed once with unsaved changes.
    pub confirm_back: bool,
    pub saving: bool,
}

impl BuilderUi {
    pub fn say(&mut self, msg: impl Into<String>, now: f32) {
        self.toast = Some((msg.into(), now));
    }
}

#[derive(Resource, Debug, Default)]
pub struct BuilderStats {
    pub rev: Option<u64>,
    pub stats: DeckStats,
    pub goldfish: Goldfish,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AfterSave {
    Stay,
    Play,
}

#[derive(Resource, Default)]
struct BuilderNet {
    search: Option<Receiver<(u64, bool, Result<Vec<CatalogCard>, String>)>>,
    recs: Option<Receiver<(u64, Result<Recommendations, String>)>>,
    load: Option<Receiver<Result<DeckFull, String>>>,
    save: Option<Receiver<(AfterSave, Result<DeckFull, String>)>>,
    validate: Option<Receiver<(u64, Result<Option<String>, String>)>>,
    validate_at: Option<f32>,
    validate_inflight: bool,
}

// =============================================================================================
// Actions
// =============================================================================================

#[derive(Component, Debug, Clone, PartialEq)]
pub enum BuilderAction {
    Back,
    Save,
    Play,
    Playtest,
    Tab(Tab),
    ToggleColor(usize),
    Cmc(Option<i32>),
    Kind(Option<&'static str>),
    CommanderOnly,
    MoreResults,
    ToggleTheme(String),
    /// Click on a search / recommendation tile: add a copy (or pick it as commander).
    AddCard(Box<CatalogCard>),
    /// `+` on a deck row.
    AddOne(String),
    /// Click on a deck row: remove one copy.
    RemoveOne(String),
    RemoveCommander(String),
    PtNewHand,
    PtMulligan,
    PtDraw,
    PtClose,
}

/// NEW DECK / EDIT DECK on the deck-select screen.
fn open_from_menu(
    q: Query<(&Interaction, &super::MenuAction), Changed<Interaction>>,
    list: Res<DeckList>,
    sel: Res<DeckSelection>,
    mut open: ResMut<BuilderOpen>,
    mut next: ResMut<NextState<AppState>>,
) {
    for (i, act) in &q {
        if *i != Interaction::Pressed {
            continue;
        }
        let deck_ref = match act {
            super::MenuAction::NewDeck => None,
            super::MenuAction::EditDeck => match sel.human.clone() {
                Some(r) => Some(r),
                None => continue,
            },
            _ => continue,
        };
        *open = BuilderOpen { format: list.format, deck_ref };
        next.set(AppState::DeckBuilder);
    }
}

/// Dev aid for unattended screenshots: `ARENA_MENU_SCREEN=builder|builder-commander|builder-constructed`
/// opens an empty deck; add `ARENA_BUILDER_DECK=<ref>` to open an existing one and
/// `ARENA_BUILDER_TAB=recs|playtest` to start on the recommendations / a sample hand.
fn debug_open(mut open: ResMut<BuilderOpen>, settings: Res<Settings>, mut next: ResMut<NextState<AppState>>, mut done: Local<bool>) {
    if *done {
        return;
    }
    *done = true;
    let format = match std::env::var("ARENA_MENU_SCREEN").as_deref() {
        Ok("builder") => settings.last_format,
        Ok("builder-commander") => Format::Commander,
        Ok("builder-constructed") => Format::Constructed,
        _ => return,
    };
    *open = BuilderOpen { format, deck_ref: std::env::var("ARENA_BUILDER_DECK").ok() };
    next.set(AppState::DeckBuilder);
}

fn enter(
    open: Res<BuilderOpen>,
    cfg: Option<Res<EngineConfig>>,
    settings: Res<Settings>,
    mut deck: ResMut<BuilderDeck>,
    mut search: ResMut<SearchState>,
    mut recs: ResMut<RecsState>,
    mut ui: ResMut<BuilderUi>,
    mut stats: ResMut<BuilderStats>,
    mut net: ResMut<BuilderNet>,
    time: Res<Time>,
) {
    // problem_rev ≠ rev: nothing has been validated yet, so PLAY waits for the engine's verdict
    *deck = BuilderDeck { format: open.format, name: "New Deck".into(), problem_rev: u64::MAX, ..default() };
    *search = SearchState { commander_only: open.format == Format::Commander && open.deck_ref.is_none(), dirty_at: Some(time.elapsed_secs()), ..default() };
    *recs = RecsState::default();
    *ui = BuilderUi::default();
    // dev aid for screenshots, see `debug_open`
    if std::env::var("ARENA_BUILDER_TAB").as_deref() == Ok("recs") {
        ui.tab = Tab::Recommend;
    }
    *stats = BuilderStats::default();
    *net = BuilderNet::default();
    if let Some(r) = open.deck_ref.clone() {
        deck.loading = true;
        let base = engine_base(&cfg, &settings);
        let fmt = open.format;
        let (tx, rx) = unbounded();
        net.load = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(Rest::new(base).deck(&r, fmt).map_err(|e| format!("{e:#}")));
        });
    }
}

fn handle_actions(
    q: Query<(&Interaction, &BuilderAction, Option<&ArenaButton>), Changed<Interaction>>,
    mut deck: ResMut<BuilderDeck>,
    mut search: ResMut<SearchState>,
    mut recs: ResMut<RecsState>,
    mut ui: ResMut<BuilderUi>,
    mut net: ResMut<BuilderNet>,
    mut next: ResMut<NextState<AppState>>,
    mut refresh: MessageWriter<RefreshDecks>,
    cfg: Option<Res<EngineConfig>>,
    settings: Res<Settings>,
    time: Res<Time>,
) {
    let now = time.elapsed_secs();
    for (i, act, btn) in &q {
        if *i != Interaction::Pressed || btn.is_some_and(|b| b.disabled) {
            continue;
        }
        if !matches!(act, BuilderAction::Back) {
            ui.confirm_back = false;
        }
        match act {
            BuilderAction::Back => {
                if deck.dirty && !ui.confirm_back {
                    ui.confirm_back = true;
                    ui.say("Unsaved changes — press BACK again to discard them", now);
                } else {
                    refresh.write(RefreshDecks(deck.format));
                    next.set(AppState::DeckSelect);
                }
            }
            BuilderAction::Save | BuilderAction::Play => {
                let then = if *act == BuilderAction::Play { AfterSave::Play } else { AfterSave::Stay };
                start_save(&mut deck, &mut ui, &mut net, engine_base(&cfg, &settings), then, now);
            }
            BuilderAction::Playtest => {
                if deck.main.is_empty() {
                    ui.say("Add some cards first", now);
                } else {
                    ui.playtest = Some(playtest::Playtest::new(&deck));
                }
            }
            BuilderAction::Tab(t) => ui.tab = *t,
            BuilderAction::ToggleColor(i) => {
                search.colors[*i] = !search.colors[*i];
                search.dirty_at = Some(now);
            }
            BuilderAction::Cmc(c) => {
                search.cmc = if search.cmc == *c { None } else { *c };
                search.dirty_at = Some(now);
            }
            BuilderAction::Kind(k) => {
                search.kind = if search.kind == *k { None } else { *k };
                search.dirty_at = Some(now);
            }
            BuilderAction::CommanderOnly => {
                search.commander_only = !search.commander_only;
                search.dirty_at = Some(now);
            }
            BuilderAction::MoreResults => {
                search.append = true;
                search.dirty_at = Some(now - 1.0);
            }
            BuilderAction::ToggleTheme(k) => {
                // First click pins the engine's current picks plus/minus this one, so toggling one
                // theme doesn't silently drop the others it chose.
                if recs.pinned.is_empty()
                    && let Some(d) = &recs.data
                {
                    recs.pinned = d.themes.iter().filter(|t| t.active).map(|t| t.key.clone()).collect();
                }
                if let Some(i) = recs.pinned.iter().position(|p| p == k) {
                    recs.pinned.remove(i);
                } else {
                    recs.pinned.push(k.clone());
                }
                recs.dirty_at = Some(now - 1.0);
            }
            BuilderAction::AddCard(card) => add_card(&mut deck, &mut search, &mut ui, card, now),
            BuilderAction::AddOne(name) => {
                if let Some(card) = deck.main.iter().find(|(c, _)| &c.name == name).map(|(c, _)| c.clone())
                    && let Err(e) = deck.add(&card)
                {
                    ui.say(e, now);
                }
            }
            BuilderAction::RemoveOne(name) => deck.remove(name),
            BuilderAction::RemoveCommander(name) => {
                deck.remove_commander(name);
                if deck.commanders.is_empty() {
                    search.commander_only = true;
                    search.dirty_at = Some(now);
                }
            }
            BuilderAction::PtNewHand | BuilderAction::PtMulligan | BuilderAction::PtDraw | BuilderAction::PtClose => {
                playtest::act(&mut ui, &deck, act);
            }
        }
    }
}

/// Left-click semantics: in a Commander deck without a commander, a commander-eligible card
/// becomes the commander (the screen opens on a commander search); otherwise add one copy.
fn add_card(deck: &mut BuilderDeck, search: &mut SearchState, ui: &mut BuilderUi, card: &CatalogCard, now: f32) {
    if deck.format == Format::Commander && deck.commanders.is_empty() && card.can_be_commander {
        deck.set_commander(card);
        ui.say(format!("{} is your commander — now add the 99", card.name), now);
        if search.commander_only {
            search.commander_only = false;
            search.dirty_at = Some(now);
        }
        return;
    }
    match deck.add(card) {
        Ok(()) => {}
        Err(e) => ui.say(e, now),
    }
}

/// Right-click a commander-eligible tile to make it the commander (or partner).
fn right_click_cards(
    mut clicks: MessageReader<bevy::picking::events::Pointer<bevy::picking::events::Click>>,
    tiles: Query<&BuilderAction>,
    parents: Query<&ChildOf>,
    mut deck: ResMut<BuilderDeck>,
    mut search: ResMut<SearchState>,
    mut ui: ResMut<BuilderUi>,
    time: Res<Time>,
) {
    for c in clicks.read() {
        if c.button != PointerButton::Secondary || deck.format != Format::Commander {
            continue;
        }
        // the pick may land on the tile's image child: walk up to the tile
        let mut e = c.entity;
        for _ in 0..4 {
            if let Ok(BuilderAction::AddCard(card)) = tiles.get(e) {
                if card.can_be_commander {
                    deck.set_commander(card);
                    ui.say(format!("{} is now your commander", card.name), time.elapsed_secs());
                    if search.commander_only {
                        search.commander_only = false;
                        search.dirty_at = Some(time.elapsed_secs());
                    }
                } else {
                    ui.say(format!("{} can't be a commander", card.name), time.elapsed_secs());
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

fn read_fields(fields: Query<(&Field, &EditableText), Changed<EditableText>>, mut deck: ResMut<BuilderDeck>, mut search: ResMut<SearchState>, time: Res<Time>) {
    for (f, t) in &fields {
        let v = t.value().to_string();
        match f {
            Field::BuilderName if deck.name != v => {
                deck.name = v;
                deck.dirty = true;
            }
            Field::BuilderSearch if search.q != v => {
                search.q = v;
                search.dirty_at = Some(time.elapsed_secs());
            }
            Field::BuilderText if search.text != v => {
                search.text = v;
                search.dirty_at = Some(time.elapsed_secs());
            }
            _ => {}
        }
    }
}

// =============================================================================================
// Network
// =============================================================================================

fn start_save(deck: &mut BuilderDeck, ui: &mut BuilderUi, net: &mut BuilderNet, base: String, then: AfterSave, now: f32) {
    if ui.saving || deck.loading {
        return;
    }
    if deck.name.trim().is_empty() {
        ui.say("Give the deck a name first", now);
        return;
    }
    if then == AfterSave::Play && deck.problem_rev == deck.rev && deck.problem.is_some() {
        ui.say(deck.problem.clone().unwrap_or_default(), now);
        return;
    }
    ui.saving = true;
    let body = deck.body();
    let (tx, rx) = unbounded();
    net.save = Some(rx);
    std::thread::spawn(move || {
        let _ = tx.send((then, Rest::new(base).save_deck_body(&body).map_err(|e| format!("{e:#}"))));
    });
}

fn search_query(search: &SearchState, deck: &BuilderDeck) -> CardQuery {
    let ci = match deck.identity() {
        Some(id) => Some(if id.is_empty() { "C".to_string() } else { id }),
        None if search.colors.iter().any(|c| *c) => Some(deckstats::COLORS.iter().zip(search.colors).filter(|(_, on)| *on).map(|(c, _)| *c).collect()),
        None => None,
    };
    let opt = |s: &str| Some(s.trim().to_string()).filter(|s| !s.is_empty());
    CardQuery {
        q: search.q.trim().to_string(),
        ci,
        type_line: search.kind.map(str::to_string),
        text: opt(&search.text),
        cmc: search.cmc,
        tag: None,
        commander_only: search.commander_only && deck.format == Format::Commander,
        sort: None,
        offset: if search.append { search.results.len() as u32 } else { 0 },
        limit: PAGE,
    }
}

fn start_requests(
    mut deck: ResMut<BuilderDeck>,
    mut search: ResMut<SearchState>,
    mut recs: ResMut<RecsState>,
    ui: Res<BuilderUi>,
    mut net: ResMut<BuilderNet>,
    cfg: Option<Res<EngineConfig>>,
    settings: Res<Settings>,
    time: Res<Time>,
) {
    let now = time.elapsed_secs();
    let base = || engine_base(&cfg, &settings);
    // card search, debounced while typing
    if search.dirty_at.is_some_and(|t| now - t > 0.28) && !deck.loading {
        search.dirty_at = None;
        search.generation += 1;
        search.busy = true;
        let q = search_query(&search, &deck);
        let (generation, append) = (search.generation, search.append);
        let (tx, rx) = unbounded();
        net.search = Some(rx);
        let b = base();
        std::thread::spawn(move || {
            let _ = tx.send((generation, append, Rest::new(b).search_cards(&q).map_err(|e| format!("{e:#}"))));
        });
    }
    // recommendations: only while the tab is open, and after edits settle
    if ui.tab == Tab::Recommend && !deck.loading {
        let want = (deck.rev, recs.pinned.clone());
        if recs.for_rev.as_ref() != Some(&want) && recs.dirty_at.is_none() {
            recs.dirty_at = Some(now);
        }
        if recs.dirty_at.is_some_and(|t| now - t > 0.7) {
            recs.dirty_at = None;
            recs.for_rev = Some(want);
            recs.generation += 1;
            recs.busy = true;
            let req = RecommendRequest {
                format: deck.format,
                commanders: deck.commanders.iter().map(|c| c.name.clone()).collect(),
                main: deck.lines(),
                themes: recs.pinned.clone(),
                colors: None,
                limit: 18,
            };
            let generation = recs.generation;
            let (tx, rx) = unbounded();
            net.recs = Some(rx);
            let b = base();
            std::thread::spawn(move || {
                let _ = tx.send((generation, Rest::new(b).recommend(&req).map_err(|e| format!("{e:#}"))));
            });
        }
    }
    // engine validation after edits
    if deck.problem_rev != deck.rev && !deck.loading && !net.validate_inflight {
        let due = *net.validate_at.get_or_insert(now);
        if now - due > 0.5 {
            net.validate_at = None;
            net.validate_inflight = true;
            let body = deck.body();
            let rev = deck.rev;
            let (tx, rx) = unbounded();
            net.validate = Some(rx);
            let b = base();
            std::thread::spawn(move || {
                let _ = tx.send((rev, Rest::new(b).validate_deck(&body).map_err(|e| format!("{e:#}"))));
            });
        }
    }
    let _ = &mut deck;
}

fn poll_net(
    mut net: ResMut<BuilderNet>,
    mut deck: ResMut<BuilderDeck>,
    mut search: ResMut<SearchState>,
    mut recs: ResMut<RecsState>,
    mut ui: ResMut<BuilderUi>,
    mut fields: Query<(&Field, &mut EditableText)>,
    mut next: ResMut<NextState<AppState>>,
    mut sel: ResMut<DeckSelection>,
    list: Res<DeckList>,
    mut settings: ResMut<Settings>,
    mut cfg_match: ResMut<MatchConfig>,
    mut names: ResMut<MatchDeckNames>,
    time: Res<Time>,
) {
    let now = time.elapsed_secs();
    if let Some(rx) = &net.search
        && let Ok((generation, append, res)) = rx.try_recv()
    {
        net.search = None;
        if generation == search.generation {
            search.busy = false;
            search.append = false;
            match res {
                Ok(cards) => {
                    search.more = cards.len() as u32 >= PAGE;
                    if append {
                        let have: std::collections::HashSet<String> = search.results.iter().map(|c| c.name.clone()).collect();
                        search.results.extend(cards.into_iter().filter(|c| !have.contains(&c.name)));
                    } else {
                        search.results = cards;
                    }
                    search.error = None;
                }
                Err(e) => search.error = Some(e),
            }
            search.shown += 1;
        }
    }
    if let Some(rx) = &net.recs
        && let Ok((generation, res)) = rx.try_recv()
    {
        net.recs = None;
        if generation == recs.generation {
            recs.busy = false;
            match res {
                Ok(d) => {
                    recs.data = Some(d);
                    recs.error = None;
                }
                Err(e) => recs.error = Some(e),
            }
            recs.shown += 1;
        }
    }
    if let Some(rx) = &net.load
        && let Ok(res) = rx.try_recv()
    {
        net.load = None;
        deck.loading = false;
        match res {
            Ok(d) => {
                deck.name = d.name.clone();
                deck.saved_name = d.deck_ref.strip_prefix("user:").map(str::to_string);
                deck.commanders = d.commanders;
                deck.main = d.main.into_iter().map(|e| (e.card, e.qty.max(1))).collect();
                deck.rev += 1;
                deck.dirty = false;
                for (f, mut t) in &mut fields {
                    if *f == Field::BuilderName {
                        t.editor_mut().set_text(&deck.name);
                    }
                }
                if deck.format == Format::Commander && deck.commanders.is_empty() {
                    search.commander_only = true;
                }
                if std::env::var("ARENA_BUILDER_TAB").as_deref() == Ok("playtest") {
                    ui.playtest = Some(playtest::Playtest::new(&deck));
                }
                search.dirty_at = Some(now);
            }
            Err(e) => ui.say(format!("Could not load the deck: {e}"), now),
        }
    }
    if let Some(rx) = &net.validate
        && let Ok((rev, res)) = rx.try_recv()
    {
        net.validate = None;
        net.validate_inflight = false;
        if let Ok(p) = res {
            deck.problem = p;
            deck.problem_rev = rev;
        }
    }
    if let Some(rx) = &net.save
        && let Ok((then, res)) = rx.try_recv()
    {
        net.save = None;
        ui.saving = false;
        match res {
            Ok(saved) => {
                deck.saved_name = Some(saved.name.clone());
                deck.dirty = false;
                deck.problem = saved.problem.clone();
                deck.problem_rev = deck.rev;
                let r = format!("user:{}", saved.name);
                sel.human = Some(r.clone());
                settings.last_deck.insert(deck.format.as_str().to_string(), r.clone());
                settings.last_format = deck.format;
                match (then, &saved.problem) {
                    (AfterSave::Play, None) => {
                        let ai = sel.ai.clone().unwrap_or_else(|| "random".into());
                        names.human = saved.name.clone();
                        names.ai = if ai == "random" { "Random".into() } else { list.name_of(&ai) };
                        *cfg_match = MatchConfig { format: deck.format, human_deck: r, ai_deck: ai, seed: None };
                        next.set(AppState::Loading);
                    }
                    (AfterSave::Play, Some(p)) => ui.say(format!("Saved, but not playable yet: {p}"), now),
                    (AfterSave::Stay, _) => ui.say(format!("Saved “{}”", saved.name), now),
                }
            }
            Err(e) => ui.say(format!("Save failed: {e}"), now),
        }
    }
    let _ = &mut recs;
}

fn recompute_stats(deck: Res<BuilderDeck>, mut stats: ResMut<BuilderStats>) {
    if stats.rev == Some(deck.rev) {
        return;
    }
    stats.rev = Some(deck.rev);
    let cmd: Vec<&CatalogCard> = deck.commanders.iter().collect();
    let entries = deck.entries();
    stats.stats = deckstats::analyze(deck.format, &cmd, &entries);
    // fixed seed: the same deck always shows the same numbers
    stats.goldfish = deckstats::goldfish(&entries, &cmd, 1200, 0x5eed);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(name: &str, text: &str, ci: &str) -> CatalogCard {
        CatalogCard { name: name.into(), oracle_text: Some(text.into()), color_identity: ci.into(), can_be_commander: true, ..Default::default() }
    }

    #[test]
    fn copy_limits_follow_format() {
        let mut d = BuilderDeck { format: Format::Commander, ..default() };
        let bolt = card("Lightning Bolt", "", "R");
        assert!(d.add(&bolt).is_ok());
        assert!(d.add(&bolt).is_err(), "singleton");
        let forest = CatalogCard { is_basic_land: true, ..card("Forest", "", "") };
        for _ in 0..30 {
            d.add(&forest).unwrap();
        }
        let rats = card("Relentless Rats", "A deck can have any number of cards named Relentless Rats.", "B");
        d.add(&rats).unwrap();
        d.add(&rats).unwrap();
        assert_eq!(d.count(), 33);

        let mut c = BuilderDeck { format: Format::Constructed, ..default() };
        for _ in 0..4 {
            c.add(&bolt).unwrap();
        }
        assert!(c.add(&bolt).is_err());
        c.remove("Lightning Bolt");
        assert_eq!(c.qty("Lightning Bolt"), 3);
        assert!(c.dirty && c.rev == 5);
    }

    #[test]
    fn commanders_replace_unless_partners() {
        let mut d = BuilderDeck { format: Format::Commander, ..default() };
        let atraxa = card("Atraxa", "Flying", "WUBG");
        let krenko = card("Krenko", "Haste", "R");
        d.add(&atraxa).unwrap();
        d.set_commander(&atraxa);
        assert!(d.main.is_empty(), "a commander leaves the 99");
        assert_eq!(d.identity().as_deref(), Some("WUBG"));
        d.set_commander(&krenko);
        assert_eq!(d.commanders.len(), 1);
        assert!(d.off_identity(&card("Swords", "", "W")));

        let a = card("Tymna", "Partner", "WB");
        let b = card("Thrasios", "Partner", "GU");
        d.set_commander(&a);
        d.set_commander(&b);
        assert_eq!(d.commanders.len(), 2);
        assert_eq!(d.identity().as_deref(), Some("WUBG"));
        assert!(d.add(&a).is_err(), "can't add the commander to the 99");
        d.remove_commander("Tymna");
        assert_eq!(d.commanders.len(), 1);
    }

    #[test]
    fn body_carries_rename() {
        let mut d = BuilderDeck { format: Format::Constructed, name: "New name".into(), saved_name: Some("Old".into()), ..default() };
        d.add(&card("Shock", "", "R")).unwrap();
        let b = d.body();
        assert_eq!(b.previous_name.as_deref(), Some("Old"));
        assert_eq!(b.main, vec![DeckLine { name: "Shock".into(), qty: 1 }]);
        d.saved_name = Some("New name".into());
        assert_eq!(d.body().previous_name, None);
    }
}
