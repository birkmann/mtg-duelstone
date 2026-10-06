//! App flow: Boot (engine up) → MainMenu/DeckSelect (or Multiplayer → DeckSelect) → Loading (match or
//! table created, socket open) → [Lobby until the host starts] → InMatch (first `state` seen) → GameOver.
//!
//! All blocking work (engine launch, REST) runs as `mtg_client::spawn` tasks reporting over channels
//! so the render loop never stalls — threads on desktop, the page's event loop in the browser, where
//! there is also nothing to launch: the engine served the page, so its origin is the engine URL.
//! `--quick` and `--replay` skip the menus; `--join` / `?join=` jumps to the multiplayer screen.

#[cfg(not(target_arch = "wasm32"))]
use std::path::{Path, PathBuf};

use bevy::prelude::*;
#[cfg(not(target_arch = "wasm32"))]
use mtg_client::launcher::find_bundled;
use crossbeam_channel::Receiver;
use mtg_client::{Api, EngineLink};
use mtg_protocol::{CreateLobby, CreateMatch, JoinLobby, MatchInfo, ServerFrame, ServerInfo};

use crate::net::NetLink;
use crate::shared::*;

pub struct AppStatePlugin;

impl Plugin for AppStatePlugin {
    fn build(&self, app: &mut App) {
        // The engine config must exist before the first frame: Bevy runs the initial state's OnEnter (Boot,
        // which contacts the engine) before even PreStartup, so a Startup system would be too late.
        let cfg = {
            let world = app.world();
            engine_config(
                &world.get_resource::<CliArgs>().cloned().unwrap_or_default(),
                &world.get_resource::<Settings>().cloned().unwrap_or_default(),
            )
        };
        app.init_resource::<BootStatus>()
            .init_resource::<MatchConfig>()
            .insert_resource(cfg)
            .init_resource::<Settings>()
            .init_resource::<MatchMode>()
            .init_resource::<SeatTicket>()
            .init_resource::<LobbyView>()
            .init_resource::<ServerFacts>()
            .add_message::<RetryBoot>()
            .add_systems(Update, retry_boot)
            .add_systems(OnEnter(AppState::Boot), start_boot)
            .add_systems(Update, poll_boot.run_if(in_state(AppState::Boot)))
            .add_systems(OnEnter(AppState::Loading), start_loading)
            .add_systems(Update, poll_loading.run_if(in_state(AppState::Loading)))
            .add_systems(Update, (track_lobby, enter_match_from_lobby.run_if(in_state(AppState::Lobby))).chain().after(crate::view::ViewSet))
            .add_systems(Update, watch_game_over.run_if(in_state(AppState::InMatch)).after(crate::view::ViewSet))
            .add_systems(OnExit(AppState::GameOver), end_match)
            .add_systems(OnEnter(AppState::MainMenu), end_match_if_link)
            .add_systems(Last, stop_engine_on_exit);
    }
}

// ---- engine process (desktop only) ------------------------------------------------------------

#[cfg(not(target_arch = "wasm32"))]
type Proc = Option<mtg_client::EngineProcess>;
#[cfg(target_arch = "wasm32")]
type Proc = ();

/// Engine child process we launched (killed when dropped / on app exit).
#[derive(Resource, Default)]
pub struct EngineHandle(#[allow(dead_code)] pub Proc);

/// Bevy's winit loop may exit the process without dropping resources, so kill the engine we
/// launched explicitly when the app is asked to exit.
fn stop_engine_on_exit(mut exits: MessageReader<AppExit>, handle: Option<ResMut<EngineHandle>>) {
    if exits.read().next().is_some()
        && let Some(mut h) = handle
    {
        h.0 = Proc::default();
    }
}

/// Id of the vs-AI match currently being played (deleted on the engine when we leave). Tables are
/// shared with other players, so leaving one never deletes it; the engine prunes it.
#[derive(Resource, Debug, Clone)]
pub struct ActiveMatch(pub String);

/// Walk up from `start` looking for `rel`.
#[cfg(not(target_arch = "wasm32"))]
fn find_up(start: &Path, rel: &str) -> Option<PathBuf> {
    let mut dir = Some(start);
    while let Some(d) = dir {
        let p = d.join(rel);
        if p.exists() {
            return Some(p);
        }
        dir = d.parent();
    }
    None
}

#[cfg(not(target_arch = "wasm32"))]
fn locate(rel: &str) -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    find_up(&cwd, rel).or_else(|| find_up(Path::new(env!("CARGO_MANIFEST_DIR")), rel))
}

fn engine_config(cli: &CliArgs, settings: &Settings) -> EngineConfig {
    let mut cfg = EngineConfig::default();
    if let Some(origin) = crate::platform::page_origin() {
        cfg.base_url = origin;
        cfg.autolaunch = false;
        return cfg;
    }
    cfg.base_url = cli.engine_url.clone().unwrap_or_else(|| settings.engine_url.clone());
    cfg.autolaunch = !cli.no_autolaunch && settings.autolaunch;
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(jar) = find_bundled("engine/forge-ws-server.jar").or_else(|| locate("engine/target/forge-ws-server.jar")) {
        cfg.jar = jar;
    }
    cfg
}

#[derive(Resource)]
struct BootTask {
    progress: Receiver<String>,
    done: Receiver<Result<(Proc, Option<ServerInfo>), String>>,
}

fn start_boot(
    mut commands: Commands,
    cli: Res<CliArgs>,
    cfg: Res<EngineConfig>,
    mut status: ResMut<BootStatus>,
    mut next: ResMut<NextState<AppState>>,
) {
    status.error = None;
    if cli.replay.is_some() {
        status.message = "Replay".into();
        next.set(AppState::Loading);
        return;
    }
    status.message = if cfg.autolaunch { "Starting the game engine…".into() } else { "Connecting to the game engine…".into() };
    let (ptx, prx) = crossbeam_channel::unbounded::<String>();
    let api = Api::new(cfg.base_url.clone());
    #[cfg(not(target_arch = "wasm32"))]
    let done = {
        let launch = mtg_client::LaunchConfig {
            base_url: cfg.base_url.clone(),
            port: port_of(&cfg.base_url),
            jar: cfg.jar.clone(),
            assets: find_bundled("forge").or_else(|| locate("forge/forge-gui")),
            java: None,
            health_timeout_secs: 90,
        };
        let autolaunch = cfg.autolaunch;
        mtg_client::spawn(async move {
            let proc = if autolaunch {
                let mut cb = |s: &str| {
                    let _ = ptx.send(s.to_string());
                };
                mtg_client::ensure_engine(&launch, &mut cb).map_err(|e| format!("{e:#}"))?
            } else {
                None
            };
            match api.server_info().await {
                Ok(i) if i.ok => Ok((proc, Some(i))),
                Ok(_) => Err("engine reports unhealthy".to_string()),
                Err(e) => Err(format!("engine not reachable: {e:#}")),
            }
        })
    };
    #[cfg(target_arch = "wasm32")]
    let done = {
        drop(ptx);
        mtg_client::spawn(async move {
            match api.server_info().await {
                Ok(i) if i.ok => Ok(((), Some(i))),
                Ok(_) => Err("engine reports unhealthy".to_string()),
                Err(e) => Err(format!("engine not reachable: {e:#}")),
            }
        })
    };
    commands.insert_resource(BootTask { progress: prx, done });
}

#[cfg(not(target_arch = "wasm32"))]
fn port_of(url: &str) -> u16 {
    url.rsplit(':').next().and_then(|p| p.trim_end_matches('/').parse().ok()).unwrap_or(8080)
}

fn poll_boot(
    mut commands: Commands,
    task: Option<Res<BootTask>>,
    cli: Res<CliArgs>,
    settings: Res<Settings>,
    mut status: ResMut<BootStatus>,
    mut next: ResMut<NextState<AppState>>,
    mut match_cfg: ResMut<MatchConfig>,
    mut mode: ResMut<MatchMode>,
    mut facts: ResMut<ServerFacts>,
) {
    let Some(task) = task else { return };
    while let Ok(line) = task.progress.try_recv() {
        status.message = line;
    }
    let Ok(res) = task.done.try_recv() else { return };
    commands.remove_resource::<BootTask>();
    match res {
        Ok((proc, info)) => {
            commands.insert_resource(EngineHandle(proc));
            facts.0 = info;
            status.message = "Engine ready".into();
            if cli.join.is_some() {
                // the multiplayer screen picks the invite up from CliArgs
                next.set(AppState::Multiplayer);
            } else if cli.quick {
                *mode = match cli.players {
                    Some(n) if n > 2 => MatchMode::Host { players: n.min(4) },
                    _ => MatchMode::VsAi,
                };
                *match_cfg = MatchConfig {
                    format: cli.format.unwrap_or(settings.last_format),
                    human_deck: cli.deck.clone().unwrap_or_default(),
                    ai_deck: cli.ai_deck.clone().unwrap_or_default(),
                    seed: cli.seed,
                    setup: cli.setup.clone(),
                };
                next.set(AppState::Loading);
            } else {
                next.set(AppState::MainMenu);
            }
        }
        Err(e) => {
            error!("engine boot failed: {e}");
            status.error = Some(e);
            // Still let the menus show the error / retry.
            next.set(AppState::MainMenu);
        }
    }
}

enum Created {
    Match(MatchInfo),
    Seat(mtg_protocol::SeatTicket),
}

#[derive(Resource)]
struct LoadTask(Receiver<Result<Created, String>>);

fn start_loading(
    mut commands: Commands,
    cli: Res<CliArgs>,
    cfg: Res<EngineConfig>,
    match_cfg: Res<MatchConfig>,
    mode: Res<MatchMode>,
    settings: Res<Settings>,
    mut status: ResMut<BootStatus>,
    mut lobby: ResMut<LobbyView>,
    mut ticket: ResMut<SeatTicket>,
    mut next: ResMut<NextState<AppState>>,
    pending: Option<Res<PendingEventMatch>>,
) {
    commands.remove_resource::<NetLink>();
    status.error = None;
    *lobby = LobbyView::default();
    ticket.0 = None;
    if let Some(path) = &cli.replay {
        #[cfg(not(target_arch = "wasm32"))]
        match EngineLink::replay(path, 1.0) {
            Ok(link) => {
                commands.insert_resource(NetLink(link));
                status.message = "Loading replay…".into();
            }
            Err(e) => {
                status.error = Some(format!("cannot open replay: {e:#}"));
                next.set(AppState::MainMenu);
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = path;
            next.set(AppState::MainMenu);
        }
        return;
    }
    let api = Api::new(cfg.base_url.clone());
    let mc = match_cfg.clone();
    let name = Some(settings.player_name.trim().to_string()).filter(|n| !n.is_empty());
    let rx = match mode.clone() {
        MatchMode::VsAi => {
            status.message = "Creating match…".into();
            mtg_client::spawn(async move {
                let mut human = mc.human_deck.clone();
                let mut ai = mc.ai_deck.clone();
                if human.is_empty() || ai.is_empty() {
                    let decks = api.decks(mc.format).await.map_err(|e| format!("{e:#}"))?;
                    let ok: Vec<_> = decks.iter().filter(|d| d.problem.is_none()).collect();
                    let pool = if ok.is_empty() { decks.iter().collect() } else { ok };
                    if human.is_empty() {
                        human = pool.first().map(|d| d.deck_ref.clone()).ok_or("no decks available")?;
                    }
                    if ai.is_empty() {
                        ai = pool.get(1).map(|d| d.deck_ref.clone()).unwrap_or_else(|| "random".into());
                    }
                }
                api.create_match(&CreateMatch { human_deck: human, ai_deck: ai, format: mc.format, setup: mc.setup.clone(), seed: mc.seed, human_name: name })
                    .await
                    .map(Created::Match)
                    .map_err(|e| format!("{e:#}"))
            })
        }
        MatchMode::Host { players } => {
            status.message = "Opening your table…".into();
            let req = CreateLobby {
                format: mc.format,
                setup: mc.setup.clone(),
                players: players.clamp(2, 4) as i64,
                name: name.unwrap_or_else(|| "Host".into()),
                deck: if mc.human_deck.is_empty() { "random".into() } else { mc.human_deck.clone() },
            };
            mtg_client::spawn(async move { api.create_lobby(&req).await.map(Created::Seat).map_err(|e| format!("{e:#}")) })
        }
        MatchMode::Event { .. } => match pending {
            // a game the pod dealt us: connect to it like a lobby seat (it starts once everyone is in)
            Some(p) => {
                status.message = "Joining your game…".into();
                let t = p.0.clone();
                commands.remove_resource::<PendingEventMatch>();
                mtg_client::spawn(async move { Ok(Created::Seat(t)) })
            }
            None => {
                next.set(AppState::Event);
                return;
            }
        },
        MatchMode::Join { code } => {
            status.message = format!("Taking a seat at table {code}…");
            let req = JoinLobby {
                name: name.unwrap_or_else(|| "Player".into()),
                deck: if mc.human_deck.is_empty() { "random".into() } else { mc.human_deck.clone() },
            };
            mtg_client::spawn(async move { api.join_lobby(&code, &req).await.map(Created::Seat).map_err(|e| format!("{e:#}")) })
        }
    };
    commands.insert_resource(LoadTask(rx));
}

fn poll_loading(
    mut commands: Commands,
    task: Option<Res<LoadTask>>,
    cli: Res<CliArgs>,
    cfg: Res<EngineConfig>,
    view: Res<GameView>,
    link: Option<Res<NetLink>>,
    mut ticket: ResMut<SeatTicket>,
    mut status: ResMut<BootStatus>,
    mut next: ResMut<NextState<AppState>>,
) {
    if let Some(task) = task {
        if let Ok(res) = task.0.try_recv() {
            commands.remove_resource::<LoadTask>();
            let api = Api::new(cfg.base_url.clone());
            match res {
                Ok(Created::Match(info)) => {
                    let url = api.ws_url(&info.id, None);
                    info!("match {} created; connecting {url}", info.id);
                    commands.insert_resource(NetLink(EngineLink::connect(&url, cli.record.as_deref())));
                    commands.insert_resource(ActiveMatch(info.id));
                    status.message = "Waiting for the first state…".into();
                }
                Ok(Created::Seat(t)) => {
                    let url = api.ws_url(&t.id, Some(&t.token));
                    info!("seat {} at table {}; connecting", t.seat, t.id);
                    commands.insert_resource(NetLink(EngineLink::connect(&url, cli.record.as_deref())));
                    ticket.0 = Some(t);
                    next.set(AppState::Lobby);
                }
                Err(e) => {
                    error!("create match failed: {e}");
                    status.error = Some(e);
                    next.set(AppState::MainMenu);
                }
            }
        }
        return;
    }
    if link.is_some() && view.has_state() {
        next.set(AppState::InMatch);
    }
}

/// Keep `LobbyView` current from the socket (the lobby screen only reads it).
fn track_lobby(mut msgs: MessageReader<ServerMsg>, state: Res<State<AppState>>, mut lobby: ResMut<LobbyView>) {
    for ServerMsg(f) in msgs.read() {
        match f {
            ServerFrame::Lobby(l) => {
                lobby.frame = Some(l.clone());
                lobby.error = None;
            }
            ServerFrame::Error { text } if *state.get() == AppState::Lobby => lobby.error = Some(text.clone()),
            _ => {}
        }
    }
}

/// The host pressed start: `hello` + the first `state` arrive and the table becomes a match.
fn enter_match_from_lobby(view: Res<GameView>, mut next: ResMut<NextState<AppState>>) {
    if view.has_state() {
        next.set(AppState::InMatch);
    }
}

/// `view` sets `GameResult` on `gameOver` — or, at a multiplayer table, as soon as *we* are out
/// while the others play on (the GameOver overlay sits over the still-live board).
fn watch_game_over(result: Res<GameResult>, mut next: ResMut<NextState<AppState>>) {
    if result.0.is_some() {
        next.set(AppState::GameOver);
    }
}

/// Tear the match down: close the link, despawn every card, reset view resources (view resets on
/// entering Loading; here we make sure nothing outlives the match).
fn end_match(
    mut commands: Commands,
    cards: Query<Entity, With<CardEntity>>,
    mut registry: ResMut<CardRegistry>,
    cfg: Res<EngineConfig>,
    active: Option<Res<ActiveMatch>>,
    mut ticket: ResMut<SeatTicket>,
    mut lobby: ResMut<LobbyView>,
) {
    for e in &cards {
        commands.entity(e).despawn();
    }
    registry.0.clear();
    commands.remove_resource::<NetLink>();
    ticket.0 = None;
    *lobby = LobbyView::default();
    if let Some(m) = active {
        let api = Api::new(cfg.base_url.clone());
        let id = m.0.clone();
        drop(mtg_client::spawn(async move {
            let _ = api.delete_match(&id).await;
        }));
        commands.remove_resource::<ActiveMatch>();
    }
}

fn end_match_if_link(
    commands: Commands,
    cards: Query<Entity, With<CardEntity>>,
    registry: ResMut<CardRegistry>,
    cfg: Res<EngineConfig>,
    active: Option<Res<ActiveMatch>>,
    link: Option<Res<NetLink>>,
    ticket: ResMut<SeatTicket>,
    lobby: ResMut<LobbyView>,
) {
    if link.is_some() || active.is_some() || !cards.is_empty() || ticket.0.is_some() {
        end_match(commands, cards, registry, cfg, active, ticket, lobby);
    }
}

fn retry_boot(mut msgs: MessageReader<RetryBoot>, state: Res<State<AppState>>, mut next: ResMut<NextState<AppState>>) {
    if msgs.read().count() > 0 {
        if *state.get() == AppState::Boot {
            return;
        }
        next.set(AppState::Boot);
    }
}
