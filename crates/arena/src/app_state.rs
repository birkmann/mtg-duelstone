//! App flow: Boot (engine up) → MainMenu/DeckSelect → Loading (match created, socket open, first
//! `state` seen) → InMatch → GameOver.
//!
//! All blocking work (engine launch, REST) runs on plain threads reporting over channels so the
//! render loop never stalls. `--quick` and `--replay` skip the menus.

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use crossbeam_channel::{Receiver, unbounded};
use mtg_client::{EngineLink, EngineProcess, LaunchConfig, Rest, ensure_engine};
use mtg_protocol::{CreateMatch, MatchInfo, ServerFrame};

use crate::net::NetLink;
use crate::shared::*;

pub struct AppStatePlugin;

impl Plugin for AppStatePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BootStatus>()
            .init_resource::<MatchConfig>()
            .init_resource::<EngineConfig>()
            .init_resource::<Settings>()
            .add_message::<RetryBoot>()
            .add_systems(Startup, build_engine_config)
            .add_systems(Update, retry_boot)
            .add_systems(OnEnter(AppState::Boot), start_boot)
            .add_systems(Update, poll_boot.run_if(in_state(AppState::Boot)))
            .add_systems(OnEnter(AppState::Loading), start_loading)
            .add_systems(Update, poll_loading.run_if(in_state(AppState::Loading)))
            .add_systems(Update, watch_game_over.run_if(in_state(AppState::InMatch)))
            .add_systems(OnExit(AppState::GameOver), end_match)
            .add_systems(OnEnter(AppState::MainMenu), end_match_if_link)
            .add_systems(Last, stop_engine_on_exit);
    }
}

/// Bevy's winit loop may exit the process without dropping resources, so kill the engine we
/// launched explicitly when the app is asked to exit.
fn stop_engine_on_exit(mut exits: MessageReader<AppExit>, handle: Option<ResMut<EngineHandle>>) {
    if exits.read().next().is_some()
        && let Some(mut h) = handle
    {
        h.0.take();
    }
}

/// Engine child process we launched (killed when dropped / on app exit).
#[derive(Resource, Default)]
pub struct EngineHandle(#[allow(dead_code)] pub Option<EngineProcess>);

/// Id of the match currently being played (for cleanup).
#[derive(Resource, Debug, Clone)]
pub struct ActiveMatch(pub String);

/// Walk up from `start` looking for `rel`.
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

fn locate(rel: &str) -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    find_up(&cwd, rel).or_else(|| find_up(Path::new(env!("CARGO_MANIFEST_DIR")), rel))
}

fn build_engine_config(cli: Res<CliArgs>, settings: Res<Settings>, mut cfg: ResMut<EngineConfig>) {
    cfg.base_url = cli.engine_url.clone().unwrap_or_else(|| settings.engine_url.clone());
    cfg.autolaunch = !cli.no_autolaunch && settings.autolaunch;
    if let Some(jar) = locate("engine/target/forge-ws-server.jar") {
        cfg.jar = jar;
    }
}

fn port_of(url: &str) -> u16 {
    url.rsplit(':').next().and_then(|p| p.trim_end_matches('/').parse().ok()).unwrap_or(8080)
}

#[derive(Resource)]
struct BootTask {
    progress: Receiver<String>,
    done: Receiver<Result<Option<EngineProcess>, String>>,
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
    status.message = "Starting the game engine…".into();
    let (ptx, prx) = unbounded();
    let (dtx, drx) = unbounded();
    let launch = LaunchConfig {
        base_url: cfg.base_url.clone(),
        port: port_of(&cfg.base_url),
        jar: cfg.jar.clone(),
        assets: locate("forge/forge-gui"),
        java: None,
        health_timeout_secs: 90,
    };
    let autolaunch = cfg.autolaunch;
    let base = cfg.base_url.clone();
    std::thread::spawn(move || {
        let res = if autolaunch {
            let mut cb = |s: &str| {
                let _ = ptx.send(s.to_string());
            };
            ensure_engine(&launch, &mut cb).map_err(|e| format!("{e:#}"))
        } else {
            match Rest::new(base).health() {
                Ok(true) => Ok(None),
                Ok(false) => Err("engine reports unhealthy".to_string()),
                Err(e) => Err(format!("engine not reachable: {e:#}")),
            }
        };
        let _ = dtx.send(res);
    });
    commands.insert_resource(BootTask { progress: prx, done: drx });
}

fn poll_boot(
    mut commands: Commands,
    task: Option<Res<BootTask>>,
    cli: Res<CliArgs>,
    settings: Res<Settings>,
    mut status: ResMut<BootStatus>,
    mut next: ResMut<NextState<AppState>>,
    mut match_cfg: ResMut<MatchConfig>,
) {
    let Some(task) = task else { return };
    while let Ok(line) = task.progress.try_recv() {
        status.message = line;
    }
    let Ok(res) = task.done.try_recv() else { return };
    commands.remove_resource::<BootTask>();
    match res {
        Ok(proc) => {
            if proc.is_some() {
                commands.insert_resource(EngineHandle(proc));
            }
            status.message = "Engine ready".into();
            if cli.quick {
                *match_cfg = MatchConfig {
                    format: cli.format.unwrap_or(settings.last_format),
                    human_deck: cli.deck.clone().unwrap_or_default(),
                    ai_deck: cli.ai_deck.clone().unwrap_or_default(),
                    seed: cli.seed,
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

#[derive(Resource)]
struct LoadTask(Receiver<Result<MatchInfo, String>>);

fn start_loading(
    mut commands: Commands,
    cli: Res<CliArgs>,
    cfg: Res<EngineConfig>,
    match_cfg: Res<MatchConfig>,
    settings: Res<Settings>,
    mut status: ResMut<BootStatus>,
    mut next: ResMut<NextState<AppState>>,
) {
    commands.remove_resource::<NetLink>();
    status.error = None;
    if let Some(path) = &cli.replay {
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
        return;
    }
    status.message = "Creating match…".into();
    let (tx, rx) = unbounded();
    let rest = Rest::new(cfg.base_url.clone());
    let mc = match_cfg.clone();
    let name = settings.player_name.clone();
    std::thread::spawn(move || {
        let res = (|| -> anyhow::Result<MatchInfo> {
            let mut human = mc.human_deck.clone();
            let mut ai = mc.ai_deck.clone();
            if human.is_empty() || ai.is_empty() {
                let decks = rest.decks(mc.format)?;
                let ok: Vec<_> = decks.iter().filter(|d| d.problem.is_none()).collect();
                let pool = if ok.is_empty() { decks.iter().collect() } else { ok };
                if human.is_empty() {
                    human = pool.first().map(|d| d.deck_ref.clone()).ok_or_else(|| anyhow::anyhow!("no decks available"))?;
                }
                if ai.is_empty() {
                    ai = pool.get(1).map(|d| d.deck_ref.clone()).unwrap_or_else(|| "random".into());
                }
            }
            rest.create_match(&CreateMatch {
                human_deck: human,
                ai_deck: ai,
                format: mc.format,
                seed: mc.seed,
                human_name: Some(name).filter(|n| !n.is_empty()),
            })
        })();
        let _ = tx.send(res.map_err(|e| format!("{e:#}")));
    });
    commands.insert_resource(LoadTask(rx));
}

fn poll_loading(
    mut commands: Commands,
    task: Option<Res<LoadTask>>,
    cli: Res<CliArgs>,
    cfg: Res<EngineConfig>,
    view: Res<GameView>,
    link: Option<Res<NetLink>>,
    mut status: ResMut<BootStatus>,
    mut next: ResMut<NextState<AppState>>,
) {
    if let Some(task) = task {
        if let Ok(res) = task.0.try_recv() {
            commands.remove_resource::<LoadTask>();
            match res {
                Ok(info) => {
                    let url = Rest::new(cfg.base_url.clone()).ws_url(&info.id);
                    info!("match {} created; connecting {url}", info.id);
                    let link = EngineLink::connect(&url, cli.record.as_deref());
                    commands.insert_resource(NetLink(link));
                    commands.insert_resource(ActiveMatch(info.id));
                    status.message = "Waiting for the first state…".into();
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

fn watch_game_over(mut msgs: MessageReader<ServerMsg>, mut next: ResMut<NextState<AppState>>) {
    for ServerMsg(f) in msgs.read() {
        if matches!(f, ServerFrame::GameOver { .. }) {
            next.set(AppState::GameOver);
        }
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
) {
    for e in &cards {
        commands.entity(e).despawn();
    }
    registry.0.clear();
    commands.remove_resource::<NetLink>();
    if let Some(m) = active {
        let rest = Rest::new(cfg.base_url.clone());
        let id = m.0.clone();
        std::thread::spawn(move || {
            let _ = rest.delete_match(&id);
        });
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
) {
    if link.is_some() || active.is_some() || !cards.is_empty() {
        end_match(commands, cards, registry, cfg, active);
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
