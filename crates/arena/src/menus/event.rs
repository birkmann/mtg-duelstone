//! The limited event screen: one pod, four phases — lobby (invite friends, fill seats with AI drafters),
//! drafting (pick a card from the pack in front of you), building (40-card deck from your pool plus basic
//! lands) and ready (the host deals games; finished games are listed with their winners).
//!
//! Why one screen that follows the engine's `phase`: the pod lives in the engine and pushes an `event`
//! frame on every change, so the UI is a pure function of the latest frame plus the deck you are
//! editing locally. The event socket (`EventConn`) outlives this screen: when the host deals a game,
//! we go through Loading into that match and come back here afterwards ("Return to pod").

use std::collections::{BTreeMap, BTreeSet};

use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use mtg_client::{Api, EngineLink, ImageRequest, ImageVersion, LinkEvent};
use mtg_protocol::{ClientFrame, DraftCard, EventFrame, EventKind, EventPhase, LimitedDeck, Pairing, SeatKind, ServerFrame};

use super::MenuAction;
use super::event_setup::EventTicket;
use crate::images::CardImages;
use crate::shared::*;
use crate::style::*;

pub struct EventPlugin;

impl Plugin for EventPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EventView>()
            .init_resource::<DeckDraft>()
            .add_systems(OnEnter(AppState::Event), (connect, spawn_screen))
            .add_systems(Update, drain.run_if(resource_exists::<EventConn>))
            .add_systems(Update, (handle, rebuild).chain().run_if(in_state(AppState::Event)))
            .add_systems(OnEnter(AppState::MainMenu), leave_event);
    }
}

/// The live event socket. Owner: event.
#[derive(Resource)]
pub struct EventConn(pub EngineLink);

/// Latest pod state as this seat sees it. Owner: event.
#[derive(Resource, Default)]
pub struct EventView {
    pub frame: Option<EventFrame>,
    pub error: Option<String>,
    pub connected: bool,
    /// bumps whenever something the screen shows changed
    pub version: u64,
}

/// The deck being edited (pool ids in the main deck + basics). Owner: event.
#[derive(Resource, Default)]
pub struct DeckDraft {
    pub main: BTreeSet<i64>,
    pub basics: BTreeMap<char, i32>,
    /// event id + pool size it was initialised for (re-init when the pool changes)
    pub for_pool: (String, usize),
    pub dirty: bool,
}

impl DeckDraft {
    fn size(&self) -> i32 {
        self.main.len() as i32 + self.basics.values().sum::<i32>()
    }
    fn to_deck(&self) -> LimitedDeck {
        LimitedDeck { main: self.main.iter().copied().collect(), basics: self.basics.iter().filter(|(_, n)| **n > 0).map(|(c, n)| (c.to_string(), *n)).collect() }
    }
    fn load(&mut self, d: &LimitedDeck) {
        self.main = d.main.iter().copied().collect();
        self.basics = d.basics.iter().filter_map(|(k, n)| k.chars().next().map(|c| (c, *n))).collect();
    }
}

/// Last step the unattended dev run took (so each is sent once).
#[derive(Default)]
struct DevStep(Option<i64>);

#[derive(Component)]
struct EventBody;
#[derive(Component)]
struct HeaderText;

fn connect(mut commands: Commands, ticket: Option<Res<EventTicket>>, conn: Option<Res<EventConn>>, cfg: Res<EngineConfig>, mut view: ResMut<EventView>) {
    if conn.is_some() {
        view.version += 1; // back from a game: redraw
        return;
    }
    let Some(t) = ticket else { return };
    let url = Api::new(cfg.base_url.clone()).event_ws_url(&t.0.id, &t.0.token);
    commands.insert_resource(EventConn(EngineLink::connect(&url, None)));
    *view = EventView::default();
}

fn leave_event(mut commands: Commands, mut view: ResMut<EventView>, mut draft: ResMut<DeckDraft>) {
    commands.remove_resource::<EventConn>();
    commands.remove_resource::<EventTicket>();
    *view = EventView::default();
    *draft = DeckDraft::default();
}

/// Event frames → `EventView`; a dealt game → Loading → that match.
fn drain(
    mut commands: Commands,
    conn: Res<EventConn>,
    mut view: ResMut<EventView>,
    mut draft: ResMut<DeckDraft>,
    mut mode: ResMut<MatchMode>,
    state: Res<State<AppState>>,
    mut next: ResMut<NextState<AppState>>,
) {
    while let Ok(ev) = conn.0.rx.try_recv() {
        match ev {
            LinkEvent::Connected => {
                view.connected = true;
                view.version += 1;
            }
            LinkEvent::Disconnected(_) => {
                view.connected = false;
                view.version += 1;
            }
            LinkEvent::Frame(ServerFrame::Event(f)) => {
                let pool_key = (f.id.clone(), f.pool.len());
                if draft.for_pool != pool_key && !draft.dirty {
                    draft.for_pool = pool_key;
                    if let Some(d) = &f.deck {
                        draft.load(d);
                    } else {
                        draft.main.clear();
                        draft.basics.clear();
                    }
                }
                view.frame = Some(f);
                view.error = None;
                view.version += 1;
            }
            LinkEvent::Frame(ServerFrame::DeckSuggestion { deck }) => {
                draft.load(&deck);
                draft.dirty = true;
                view.version += 1;
            }
            LinkEvent::Frame(ServerFrame::EventMatch(t)) => {
                *mode = MatchMode::Event { code: view.frame.as_ref().map(|f| f.id.clone()).unwrap_or_default() };
                commands.insert_resource(PendingEventMatch(t));
                if matches!(state.get(), AppState::Event) {
                    next.set(AppState::Loading);
                }
            }
            LinkEvent::Frame(ServerFrame::Error { text }) => {
                view.error = Some(text);
                view.version += 1;
            }
            LinkEvent::Frame(_) => {}
        }
    }
}

fn spawn_screen(mut commands: Commands, fonts: Res<ArenaFonts>, mut view: ResMut<EventView>) {
    view.version += 1;
    commands
        .spawn((
            Name::new("Event"),
            DespawnOnExit(AppState::Event),
            Node { flex_direction: FlexDirection::Column, padding: UiRect::all(Val::Px(22.0)), row_gap: Val::Px(12.0), ..fullscreen_root() },
        ))
        .with_children(|root| {
            root.spawn(Node { width: Val::Percent(100.0), align_items: AlignItems::Center, column_gap: Val::Px(16.0), ..default() }).with_children(|bar| {
                let b = spawn_button(bar, &fonts, ButtonKind::Secondary, "LEAVE", Vec2::new(130.0, 46.0));
                bar.commands().entity(b).insert(MenuAction::LeaveEvent);
                bar.spawn((HeaderText, Node { flex_grow: 1.0, justify_content: JustifyContent::Center, ..default() }, heading(&fonts, "CONNECTING…", 34.0)));
                bar.spawn(Node { width: Val::Px(130.0), ..default() });
            });
            root.spawn((EventBody, Node { width: Val::Percent(100.0), flex_grow: 1.0, min_height: Val::Px(0.0), column_gap: Val::Px(18.0), ..default() }));
        });
}

fn me(f: &EventFrame) -> Option<i64> {
    f.you
}
fn is_host(f: &EventFrame) -> bool {
    f.you == Some(f.host)
}

fn color_order(c: &DraftCard) -> (u8, i32, String) {
    let k = match c.card.colors.len() {
        0 if c.card.is_land => 7,
        0 => 6,
        1 => "WUBRG".find(c.card.colors.chars().next().unwrap_or('W')).unwrap_or(0) as u8,
        _ => 5,
    };
    (k, c.card.cmc, c.card.name.clone())
}

/// One card picture; `on` = gold border (picked / in deck), `dim` = sideboard.
fn card_tile(p: &mut ChildSpawnerCommands, images: &mut CardImages, c: &DraftCard, w: f32, on: bool, dim: bool, act: MenuAction) {
    let h = w * 680.0 / 488.0;
    let img = images.request_raw(ImageRequest { name: c.card.name.clone(), set: c.card.set.clone(), back: false, version: ImageVersion::Normal });
    p.spawn((
        Button,
        act,
        Node { width: Val::Px(w), height: Val::Px(h), border: UiRect::all(Val::Px(3.0)), border_radius: BorderRadius::all(Val::Px(w * 0.05)), overflow: Overflow::clip(), flex_shrink: 0.0, ..default() },
        BorderColor::all(if on { GOLD_BRIGHT } else { Color::NONE }),
        BackgroundColor(Color::srgb(0.08, 0.06, 0.05)),
    ))
    .with_children(|t| {
        t.spawn((
            ImageNode { image: img, color: if dim { Color::srgba(1.0, 1.0, 1.0, 0.45) } else { Color::WHITE }, ..default() },
            Node { width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
            Pickable::IGNORE,
        ));
    });
}

fn rebuild(
    mut commands: Commands,
    view: Res<EventView>,
    draft: Res<DeckDraft>,
    fonts: Res<ArenaFonts>,
    mut images: ResMut<CardImages>,
    body: Query<(Entity, Option<&Children>), With<EventBody>>,
    mut header: Query<&mut Text, With<HeaderText>>,
    mut shown: Local<u64>,
) {
    if *shown == view.version && !draft.is_changed() {
        return;
    }
    *shown = view.version;
    let Ok((root, children)) = body.single() else { return };
    for c in children.into_iter().flatten() {
        commands.entity(*c).despawn();
    }
    let Some(f) = view.frame.clone() else {
        for mut t in &mut header {
            t.0 = if view.connected { "JOINING THE POD…".into() } else { "CONNECTING…".into() };
        }
        return;
    };
    let phase_label = match f.phase {
        EventPhase::Lobby => "WAITING FOR DRAFTERS",
        EventPhase::Drafting => "DRAFTING",
        EventPhase::Building => "BUILD YOUR DECK",
        EventPhase::Ready => "GAMES",
    };
    for mut t in &mut header {
        t.0 = format!("{}  ·  {}  ·  {}", f.kind.label().to_uppercase(), f.source_name, phase_label);
    }
    let host = is_host(&f);
    let err = view.error.clone();
    commands.entity(root).with_children(|b| {
        // ---- left: phase content
        b.spawn(ornate_panel(Node { flex_grow: 1.0, min_width: Val::Px(0.0), padding: UiRect::all(Val::Px(16.0)), flex_direction: FlexDirection::Column, row_gap: Val::Px(12.0), ..default() }))
            .with_children(|p| match f.phase {
                EventPhase::Lobby => lobby_body(p, &fonts, &f, host),
                EventPhase::Drafting => draft_body(p, &fonts, &mut images, &f),
                EventPhase::Building | EventPhase::Ready => build_body(p, &fonts, &mut images, &f, &draft),
            });
        // ---- right: seats + actions
        b.spawn(ornate_panel(Node { width: Val::Px(330.0), padding: UiRect::all(Val::Px(16.0)), flex_direction: FlexDirection::Column, row_gap: Val::Px(8.0), ..default() }))
            .with_children(|p| {
                p.spawn(ui_text(&fonts.title, "POD", 20.0, GOLD));
                for s in &f.seats {
                    let name = match s.kind {
                        SeatKind::Open => "Open seat".to_string(),
                        _ => s.name.clone().unwrap_or_default(),
                    };
                    let status = match f.phase {
                        EventPhase::Lobby => match s.kind {
                            SeatKind::Ai => "AI".to_string(),
                            SeatKind::Human if !s.connected => "connecting…".into(),
                            SeatKind::Human => if s.seat == f.host { "host".into() } else { "ready".into() },
                            SeatKind::Open => "AI at start".into(),
                        },
                        EventPhase::Drafting => format!("{} picks{}", s.picked, if s.thinking { " · picking" } else { "" }),
                        _ => if s.deck_ready { "deck ✓".into() } else { "building…".into() },
                    };
                    p.spawn(Node { width: Val::Percent(100.0), column_gap: Val::Px(8.0), align_items: AlignItems::Center, ..default() }).with_children(|r| {
                        r.spawn((
                            Node { width: Val::Px(10.0), height: Val::Px(10.0), border_radius: BorderRadius::MAX, ..default() },
                            BackgroundColor(if s.kind == SeatKind::Open { Color::srgba(1.0, 1.0, 1.0, 0.2) } else if s.connected { GREEN_HEAL } else { RED_DAMAGE }),
                        ));
                        r.spawn((Node { flex_grow: 1.0, ..default() }, ui_text(&fonts.body, name, 15.0, if s.you { GOLD_BRIGHT } else { TEXT })));
                        r.spawn(ui_text(&fonts.body, status, 13.0, TEXT_DIM));
                        if host && f.phase == EventPhase::Lobby && s.seat != f.host {
                            let (label, ai) = if s.kind == SeatKind::Ai { ("OPEN", false) } else { ("AI", true) };
                            let bt = spawn_button(r, &fonts, ButtonKind::Ghost, label, Vec2::new(64.0, 30.0));
                            r.commands().entity(bt).insert(MenuAction::EventSeat(s.seat, ai));
                        }
                    });
                }
                p.spawn(Node { flex_grow: 1.0, ..default() });
                if let Some(e) = &err {
                    p.spawn(ui_text(&fonts.body, e.clone(), 14.0, RED_DAMAGE));
                }
                side_actions(p, &fonts, &f, host, &draft);
            });
    });
}

fn lobby_body(p: &mut ChildSpawnerCommands, fonts: &ArenaFonts, f: &EventFrame, host: bool) {
    p.spawn(ui_text(&fonts.title, "INVITE", 20.0, GOLD));
    p.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(14.0), ..default() }).with_children(|r| {
        r.spawn((
            Node { flex_grow: 1.0, padding: UiRect::axes(Val::Px(14.0), Val::Px(10.0)), border: UiRect::all(Val::Px(2.0)), border_radius: BorderRadius::all(Val::Px(8.0)), ..default() },
            BackgroundColor(Color::srgba(0.03, 0.02, 0.015, 0.9)),
            BorderColor::all(GOLD_DIM),
        ))
        .with_child(ui_text(&fonts.body, f.invite.clone().unwrap_or_else(|| f.id.clone()), 20.0, PARCHMENT));
        let b = spawn_button(r, fonts, ButtonKind::Secondary, "COPY LINK", Vec2::new(170.0, 50.0));
        r.commands().entity(b).insert(MenuAction::CopyInvite);
    });
    p.spawn(ui_text(&fonts.body, "Friends open the link in a browser (or paste it in MULTIPLAYER → JOIN). Up to 4 people can draft together; every empty seat becomes a Forge AI drafter when the host starts.", 15.0, TEXT_DIM));
    p.spawn(Node { flex_grow: 1.0, ..default() });
    let msg = if host { "Start when everyone is here." } else { "Waiting for the host to start the draft…" };
    p.spawn(ui_text(&fonts.title, msg, 22.0, TEXT));
}

fn draft_body(p: &mut ChildSpawnerCommands, fonts: &ArenaFonts, images: &mut CardImages, f: &EventFrame) {
    let arrow = if f.pass_left { "passing left" } else { "passing right" };
    p.spawn(ui_text(&fonts.title, format!("PACK {} / {}   ·   PICK {}   ·   {}", f.round, f.rounds, f.pick, arrow), 20.0, GOLD));
    if f.pack.is_empty() {
        p.spawn(Node { flex_grow: 1.0, justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() })
            .with_child(ui_text(&fonts.title, "Waiting for the next pack…", 26.0, TEXT_DIM));
    } else {
        p.spawn(ui_text(&fonts.body, "Click a card to draft it.", 14.0, TEXT_DIM));
        p.spawn((
            ScrollPosition::default(),
            RelativeCursorPosition::default(),
            super::widgets::WheelScroll,
            Node { flex_wrap: FlexWrap::Wrap, align_content: AlignContent::FlexStart, column_gap: Val::Px(10.0), row_gap: Val::Px(10.0), flex_grow: 1.0, min_height: Val::Px(0.0), overflow: Overflow::scroll_y(), ..default() },
        ))
        .with_children(|g| {
            for c in &f.pack {
                card_tile(g, images, c, 148.0, false, false, MenuAction::Pick(c.id));
            }
        });
    }
    // picks so far, colour-sorted
    let mut pool = f.pool.clone();
    pool.sort_by_key(color_order);
    p.spawn(ui_text(&fonts.title, format!("YOUR PICKS ({})", pool.len()), 16.0, GOLD));
    p.spawn((Node { column_gap: Val::Px(4.0), overflow: Overflow::scroll_x(), min_height: Val::Px(92.0), ..default() }, ScrollPosition::default())).with_children(|r| {
        for c in &pool {
            card_tile(r, images, c, 64.0, false, false, MenuAction::Pick(-1));
        }
    });
}

fn build_body(p: &mut ChildSpawnerCommands, fonts: &ArenaFonts, images: &mut CardImages, f: &EventFrame, draft: &DeckDraft) {
    p.spawn(ui_text(&fonts.body, "Click cards to move them between your deck (gold) and sideboard (dimmed). Add basic lands on the right.", 14.0, TEXT_DIM));
    let mut pool = f.pool.clone();
    pool.sort_by_key(color_order);
    p.spawn((
        ScrollPosition::default(),
        RelativeCursorPosition::default(),
        super::widgets::WheelScroll,
        Node { flex_wrap: FlexWrap::Wrap, align_content: AlignContent::FlexStart, column_gap: Val::Px(6.0), row_gap: Val::Px(6.0), flex_grow: 1.0, min_height: Val::Px(0.0), overflow: Overflow::scroll_y(), ..default() },
    ))
    .with_children(|g| {
        for c in &pool {
            let on = draft.main.contains(&c.id);
            card_tile(g, images, c, 104.0, on, !on, MenuAction::ToggleMain(c.id));
        }
    });
}

fn side_actions(p: &mut ChildSpawnerCommands, fonts: &ArenaFonts, f: &EventFrame, host: bool, draft: &DeckDraft) {
    match f.phase {
        EventPhase::Lobby => {
            if host {
                let b = spawn_button(p, fonts, ButtonKind::Primary, if f.kind == EventKind::Sealed { "OPEN PACKS" } else { "START DRAFT" }, Vec2::new(296.0, 62.0));
                p.commands().entity(b).insert(MenuAction::EventStart);
            }
        }
        EventPhase::Drafting => {
            p.spawn(ui_text(&fonts.body, "AI drafters pick instantly; friends pick at their own pace.", 13.0, TEXT_DIM));
        }
        EventPhase::Building | EventPhase::Ready => {
            let size = draft.size();
            let col = if size >= f.min_deck as i32 { GREEN_HEAL } else { TEXT };
            p.spawn(ui_text(&fonts.title, format!("DECK {} / {}   ({} spells + {} lands)", size, f.min_deck, draft.main.len(), draft.basics.values().sum::<i32>()), 16.0, col));
            for (c, land) in [('W', "Plains"), ('U', "Island"), ('B', "Swamp"), ('R', "Mountain"), ('G', "Forest")] {
                let n = draft.basics.get(&c).copied().unwrap_or(0);
                p.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(8.0), ..default() }).with_children(|r| {
                    r.spawn((Node { width: Val::Px(110.0), ..default() }, ui_text(&fonts.body, land, 15.0, TEXT)));
                    let b = spawn_button(r, fonts, ButtonKind::Secondary, "−", Vec2::new(40.0, 34.0));
                    r.commands().entity(b).insert(MenuAction::Basic(c, -1));
                    r.spawn((Node { width: Val::Px(36.0), justify_content: JustifyContent::Center, ..default() }, ui_text(&fonts.numbers, n.to_string(), 18.0, PARCHMENT)));
                    let b = spawn_button(r, fonts, ButtonKind::Secondary, "+", Vec2::new(40.0, 34.0));
                    r.commands().entity(b).insert(MenuAction::Basic(c, 1));
                });
            }
            let b = spawn_button(p, fonts, ButtonKind::Secondary, "SUGGEST A DECK", Vec2::new(296.0, 46.0));
            p.commands().entity(b).insert(MenuAction::SuggestDeck);
            let submitted = f.seats.iter().any(|s| s.you && s.deck_ready);
            let b = spawn_button(p, fonts, ButtonKind::Primary, if submitted && !draft.dirty { "DECK SUBMITTED ✓" } else { "SUBMIT DECK" }, Vec2::new(296.0, 56.0));
            p.commands().entity(b).insert(MenuAction::SubmitDeck);
            if f.phase == EventPhase::Ready {
                p.spawn(divider());
                for g in &f.games {
                    let s = match (&g.winner, g.over) {
                        (Some(w), true) => format!("{} — {} won", g.players.join(" vs "), w),
                        (None, true) => format!("{} — finished", g.players.join(" vs ")),
                        _ => format!("{} — playing", g.players.join(" vs ")),
                    };
                    p.spawn(ui_text(&fonts.body, s, 13.0, TEXT_DIM));
                }
                if host {
                    let humans = f.seats.iter().filter(|s| s.kind == SeatKind::Human).count();
                    let b = spawn_button(p, fonts, ButtonKind::Primary, if humans > 1 { "DEAL 1v1 DUELS" } else { "PLAY vs AI DRAFTER" }, Vec2::new(296.0, 52.0));
                    p.commands().entity(b).insert(MenuAction::DealGames(Pairing::Duels, 2));
                    let n = (humans.max(2) as u8).clamp(3, 4);
                    let b = spawn_button(p, fonts, ButtonKind::Secondary, &format!("FREE-FOR-ALL OF {n}"), Vec2::new(296.0, 46.0));
                    p.commands().entity(b).insert(MenuAction::DealGames(Pairing::Table, n));
                } else {
                    p.spawn(ui_text(&fonts.body, "The host deals the games.", 14.0, TEXT_DIM));
                }
            }
        }
    }
}

fn handle(
    q: Query<(&Interaction, &MenuAction, Option<&ArenaButton>), Changed<Interaction>>,
    conn: Option<Res<EventConn>>,
    view: Res<EventView>,
    mut draft: ResMut<DeckDraft>,
    mut next: ResMut<NextState<AppState>>,
    mut clipboard: ResMut<bevy::clipboard::Clipboard>,
    mut dev_started: Local<DevStep>,
) {
    let send = |f: ClientFrame| {
        if let Some(c) = &conn {
            c.0.send(f);
        }
    };
    for (i, act, b) in &q {
        if *i != Interaction::Pressed || b.is_some_and(|b| b.disabled) {
            continue;
        }
        match act {
            MenuAction::LeaveEvent => next.set(AppState::MainMenu),
            MenuAction::EventStart => send(ClientFrame::StartEvent),
            MenuAction::EventSeat(seat, ai) => send(ClientFrame::EventSeat { seat: *seat, kind: if *ai { SeatKind::Ai } else { SeatKind::Open } }),
            MenuAction::Pick(id) if *id >= 0 => send(ClientFrame::Pick { id: *id }),
            MenuAction::ToggleMain(id) => {
                if !draft.main.remove(id) {
                    draft.main.insert(*id);
                }
                draft.dirty = true;
            }
            MenuAction::Basic(c, d) => {
                let n = draft.basics.entry(*c).or_insert(0);
                *n = (*n + d).clamp(0, 40);
                draft.dirty = true;
            }
            MenuAction::SuggestDeck => send(ClientFrame::SuggestDeck),
            MenuAction::SubmitDeck => {
                send(ClientFrame::SubmitDeck { deck: draft.to_deck() });
                draft.dirty = false;
            }
            MenuAction::DealGames(p, n) => send(ClientFrame::DealGames { pairing: *p, table_size: *n as i64 }),
            MenuAction::CopyInvite => {
                if let Some(link) = view.frame.as_ref().and_then(|f| f.invite.clone())
                    && crate::platform::clipboard_usable()
                {
                    let _ = clipboard.set_text(link);
                }
            }
            _ => {}
        }
    }
    let _ = me;
    // Dev aid (ARENA_MENU_SCREEN=draft): run the whole event unattended — start, take the first card of
    // every pack, take the engine's suggested deck, deal a game — for screenshots and UI smoke tests.
    // ARENA_DRAFT_STOP=pick:N holds on the pack after N picks, =build on the suggested deck.
    if std::env::var("ARENA_MENU_SCREEN").as_deref() == Ok("draft")
        && let Some(f) = &view.frame
    {
        let stop = std::env::var("ARENA_DRAFT_STOP").unwrap_or_default();
        let hold_pick = stop.strip_prefix("pick:").and_then(|n| n.parse::<usize>().ok()).is_some_and(|n| f.pool.len() >= n);
        let step = match f.phase {
            EventPhase::Lobby if is_host(f) && f.can_start => Some((-10, ClientFrame::StartEvent)),
            EventPhase::Drafting if hold_pick => None,
            EventPhase::Drafting => f.pack.first().map(|c| (c.id, ClientFrame::Pick { id: c.id })),
            EventPhase::Building | EventPhase::Ready if draft.main.is_empty() => Some((-1, ClientFrame::SuggestDeck)),
            EventPhase::Building if stop == "build" => None,
            EventPhase::Building if draft.dirty => Some((-2, ClientFrame::SubmitDeck { deck: draft.to_deck() })),
            EventPhase::Ready if is_host(f) && f.games.is_empty() => Some((-3, ClientFrame::DealGames { pairing: Pairing::Duels, table_size: 2 })),
            _ => None,
        };
        if let Some((key, frame)) = step
            && dev_started.0 != Some(key)
        {
            dev_started.0 = Some(key);
            if key == -2 {
                draft.dirty = false;
            }
            send(frame);
        }
    }
}
