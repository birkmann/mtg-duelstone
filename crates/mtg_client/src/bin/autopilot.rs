//! Headless soak bot: plays full games against the Forge AI over the real WebSocket protocol with
//! `mtg_client::autoplay::AutoPolicy`. Port of `mtg-cmd/tools/ws-autopilot.mjs`, extended with stall
//! detection and a prompt histogram. Exit code is non-zero on any stall or error.
//!
//! usage: autopilot [--engine URL] [--format constructed|commander] [--games N] [--seed S]
//!                  [--deck REF] [--ai-deck REF] [--launch] [--jar PATH] [--record DIR] [--verbose]
//!                  [--players N [--humans H]] [--join INVITE [--bots N]] [--seat CODE:TOKEN]
//!
//! `--players N` (3..=4, or 2) plays multiplayer tables instead: a lobby is opened over REST, `H` bots
//! (default: all N) take seats with their own tokens, the host fills the rest with Forge AI and
//! starts, and every bot drives its own seat on its own thread — the soak test for per-seat routing.
//!
//! `--join INVITE [--bots N]` sits N bots (default 1) at somebody else's table (link or code) and plays
//! them once its host starts — handy for trying a four-player game alone. `--seat CODE:TOKEN` plays an
//! existing seat (e.g. the host ticket from `POST /api/lobby`) and, being the host, starts the game
//! as soon as every seat is filled and connected.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use mtg_client::autoplay::AutoPolicy;
use mtg_client::launcher::{LaunchConfig, ensure_engine};
use mtg_client::{EngineLink, LinkEvent, Rest};
use mtg_protocol::{ClientFrame, ClickAction, CreateMatch, DeckInfo, Format, InputFrame, PlayerId, ServerFrame, StateFrame};

const STALL_SECS: u64 = 30;
const NUDGE_SECS: u64 = 6;

struct Args {
    engine: String,
    format: Format,
    games: u32,
    seed: u64,
    deck: Option<String>,
    ai_deck: Option<String>,
    launch: bool,
    jar: PathBuf,
    record: Option<PathBuf>,
    verbose: bool,
    game_timeout: u64,
    max_turns: i32,
    /// multiplayer table size (0 = classic 1 human vs 1 AI)
    players: u32,
    /// bot-driven seats at the table (rest are Forge AI)
    humans: Option<u32>,
    /// `--join`: invite link or code of an existing table
    join: Option<String>,
    bots: u32,
    /// `--seat CODE:TOKEN`
    seat: Option<String>,
    /// `--legality pauper`, `--variant planechase` (repeatable)
    setup: mtg_protocol::GameSetup,
}

fn parse_args() -> Result<Args> {
    let mut a = Args {
        engine: "http://127.0.0.1:18082".into(),
        format: Format::Constructed,
        games: 1,
        seed: 1,
        deck: None,
        ai_deck: None,
        launch: false,
        jar: PathBuf::from("engine/target/forge-ws-server.jar"),
        record: None,
        verbose: false,
        game_timeout: 600,
        max_turns: 60,
        players: 0,
        humans: None,
        join: None,
        bots: 1,
        seat: None,
        setup: Default::default(),
    };
    let mut it = std::env::args().skip(1);
    while let Some(k) = it.next() {
        let mut val = |name: &str| it.next().ok_or_else(|| anyhow::anyhow!("{name} needs a value"));
        match k.as_str() {
            "--engine" => a.engine = val("--engine")?,
            "--format" => {
                let f = val("--format")?;
                a.format = match Format::parse(&f) {
                    Some(f) => f,
                    None => bail!("unknown format {f}"),
                }
            }
            "--games" => a.games = val("--games")?.parse()?,
            "--seed" => a.seed = val("--seed")?.parse()?,
            "--deck" => a.deck = Some(val("--deck")?),
            "--ai-deck" => a.ai_deck = Some(val("--ai-deck")?),
            "--launch" => a.launch = true,
            "--jar" => a.jar = PathBuf::from(val("--jar")?),
            "--record" => a.record = Some(PathBuf::from(val("--record")?)),
            "--max-turns" => a.max_turns = val("--max-turns")?.parse()?,
            "--game-timeout" => a.game_timeout = val("--game-timeout")?.parse()?,
            "--verbose" | "-v" => a.verbose = true,
            "--players" => a.players = val("--players")?.parse()?,
            "--humans" => a.humans = Some(val("--humans")?.parse()?),
            "--join" => a.join = Some(val("--join")?),
            "--bots" => a.bots = val("--bots")?.parse()?,
            "--seat" => a.seat = Some(val("--seat")?),
            "--legality" => a.setup.legality = Some(val("--legality")?),
            "--variant" => {
                let v = val("--variant")?;
                let var = mtg_protocol::Variant::ALL.into_iter().find(|x| x.as_str().eq_ignore_ascii_case(&v)).ok_or_else(|| anyhow::anyhow!("unknown variant {v}"))?;
                a.setup.variants.push(var);
            }
            "-h" | "--help" => {
                println!("autopilot [--engine URL] [--format constructed|commander] [--games N] [--seed S] [--deck REF] [--ai-deck REF] [--launch] [--jar PATH] [--record DIR] [--game-timeout SECS] [--max-turns N] [--verbose] [--players N [--humans H]] [--join INVITE [--bots N]] [--seat CODE:TOKEN]");
                std::process::exit(0);
            }
            o => bail!("unknown argument {o}"),
        }
    }
    Ok(a)
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

fn pick_deck(decks: &[DeckInfo], spec: Option<&str>, rng: &mut Rng, avoid: Option<&str>) -> Result<String> {
    if let Some(s) = spec.filter(|s| *s != "random") {
        if let Some(d) = decks.iter().find(|d| d.deck_ref == s).or_else(|| decks.iter().find(|d| d.name.to_lowercase().contains(&s.to_lowercase()))) {
            return Ok(d.deck_ref.clone());
        }
        // Unknown to the listing (e.g. a file: ref): pass through verbatim.
        return Ok(s.to_string());
    }
    let mut ok: Vec<&DeckInfo> = decks.iter().filter(|d| d.problem.is_none() && Some(d.deck_ref.as_str()) != avoid).collect();
    if ok.is_empty() {
        // only one usable deck (often just the engine's generated "random"): both sides may use it
        ok = decks.iter().filter(|d| d.problem.is_none()).collect();
    }
    if ok.is_empty() {
        bail!("no usable decks reported by the engine");
    }
    Ok(ok[(rng.next() as usize) % ok.len()].deck_ref.clone())
}

#[derive(Default)]
struct GameResult {
    won: Option<bool>,
    winner: Option<String>,
    turns: i32,
    secs: f32,
    failure: Option<String>,
}

fn play_game(args: &Args, rest: &Rest, n: u32, human: &str, ai: &str, hist: &mut BTreeMap<String, u32>) -> GameResult {
    let t0 = Instant::now();
    let mut res = GameResult::default();
    let fin = |mut r: GameResult, t0: Instant| {
        r.secs = t0.elapsed().as_secs_f32();
        r
    };
    let m = match rest.create_match(&CreateMatch {
        human_deck: human.into(),
        ai_deck: ai.into(),
        format: args.format,
        seed: Some((args.seed as i64).wrapping_add(n as i64)),
        human_name: Some("Tester".into()),
        human_avatar: None,
        setup: args.setup.clone(),
    }) {
        Ok(m) => m,
        Err(e) => {
            res.failure = Some(format!("create match: {e:#}"));
            return fin(res, t0);
        }
    };
    let rec_path = args.record.as_ref().map(|d| d.join(format!("game-{n}-{}.jsonl", args.format.as_str())));
    let link = EngineLink::connect(&rest.ws_url(&m.id), rec_path.as_deref());
    let res = drive_seat(args, &format!("game {n}"), args.seed.wrapping_add(n as u64), &link, hist, STALL_SECS);
    let _ = rest.delete_match(&m.id);
    fin(res, t0)
}

/// Play one seat over `link` until `gameOver` (or a stall / livelock / timeout). `tag` prefixes log lines.
fn drive_seat(args: &Args, tag: &str, seed: u64, link: &EngineLink, hist: &mut BTreeMap<String, u32>, stall_secs: u64) -> GameResult {
    let t0 = Instant::now();
    let mut res = GameResult::default();
    let n = tag;
    let mut policy = AutoPolicy::new(seed);

    let mut state: Option<StateFrame> = None;
    let mut you: Option<PlayerId> = None;
    let mut pending_input: Option<InputFrame> = None;
    let mut last_input: Option<InputFrame> = None;
    let mut last_progress = Instant::now();
    let mut nudged = false;
    let mut seen_seq = -1i64;
    let mut seen_prompt = -1i64;
    let mut conceded = false;
    let (mut turn_seen, mut inputs_this_turn) = (-1, 0u32);
    // Recent protocol activity, dumped when a game stalls (state frames are too big to keep).
    let mut trail: std::collections::VecDeque<String> = Default::default();
    let note = |trail: &mut std::collections::VecDeque<String>, s: String| {
        if trail.len() >= 30 {
            trail.pop_front();
        }
        trail.push_back(s);
    };
    let count = |hist: &mut BTreeMap<String, u32>, k: String| *hist.entry(k).or_default() += 1;

    loop {
        let mut events = Vec::new();
        match link.rx.recv_timeout(Duration::from_millis(500)) {
            Ok(e) => {
                events.push(e);
                while let Ok(e) = link.rx.try_recv() {
                    events.push(e);
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            Err(_) => {
                res.failure = Some("link thread died".into());
                break;
            }
        }
        let mut prompts = Vec::new();
        let mut flashed = false;
        let mut over = None;
        for e in events {
            match e {
                LinkEvent::Connected => {}
                LinkEvent::Disconnected(r) => eprintln!("  [game {n}] link disconnected: {r} (reconnecting)"),
                LinkEvent::Frame(f) => match f {
                    ServerFrame::State(s) => {
                        you = s.you.or(you);
                        state = Some(s);
                        // The board may have changed under an input we've already seen (declare attackers).
                        if pending_input.is_none() {
                            pending_input = last_input.clone();
                        }
                    }
                    ServerFrame::Input(i) => {
                        if i.seq != seen_seq {
                            seen_seq = i.seq;
                            last_progress = Instant::now();
                            nudged = false;
                        }
                        inputs_this_turn += 1;
                        note(&mut trail, format!("<- input seq={} ok={:?} cancel={:?} sel={:?} hl={:?} players={:?} | {}", i.seq, i.ok, i.cancel, i.selectable, i.highlighted, i.selectable_players, i.message.replace('\n', " | ")));
                        pending_input = Some(i.clone());
                        last_input = Some(i);
                    }
                    ServerFrame::Prompt(p) => {
                        if p.id != seen_prompt {
                            seen_prompt = p.id;
                            last_progress = Instant::now();
                            nudged = false;
                        }
                        prompts.push(p);
                    }
                    ServerFrame::Flash => {
                        count(hist, "flash".into());
                        note(&mut trail, "<- flash".into());
                        flashed = true;
                    }
                    ServerFrame::Message { title, text, error } => {
                        count(hist, "message".into());
                        policy.on_message(&text);
                        note(&mut trail, format!("<- message error={error}: {title} {text}"));
                        if args.verbose || error {
                            println!("  [game {n}] message: {title} {text}");
                        }
                    }
                    ServerFrame::Error { text } => {
                        println!("  [game {n}] ERROR frame: {text}");
                        count(hist, "error".into());
                    }
                    ServerFrame::GameOver { winner, you_won } => {
                        over = Some((winner, you_won));
                    }
                    ServerFrame::Events { .. } => count(hist, "events".into()),
                    ServerFrame::Unknown => count(hist, "unknown-frame".into()),
                    _ => {}
                },
            }
        }
        if let Some((winner, you_won)) = over {
            res.won = Some(you_won);
            res.winner = winner;
            break;
        }
        let game = state.as_ref().map(|s| &s.game);
        if let Some(g) = game {
            res.turns = g.turn;
            if g.turn != turn_seen {
                turn_seen = g.turn;
                inputs_this_turn = 0;
            }
        }
        if inputs_this_turn > 3000 {
            res.failure = Some(format!("LIVELOCK: {inputs_this_turn} inputs within turn {turn_seen}; last: {}", last_input.as_ref().map(|i| i.message.replace('\n', " | ")).unwrap_or_default()));
            break;
        }
        let empty = mtg_protocol::Game::default();
        let g = game.unwrap_or(&empty);
        for p in prompts {
            count(hist, format!("prompt:{}", p.body.kind_name()));
            if args.verbose {
                println!("  [game {n}] PROMPT {}", p.body.kind_name());
            }
            link.send(policy.on_prompt(&p, g, you));
        }
        if flashed {
            policy.on_flash();
            pending_input = last_input.clone();
        }
        // No progress for a while after we acted: poke the policy once to retry with the OK/cancel fallback.
        if !nudged && last_progress.elapsed() > Duration::from_secs(NUDGE_SECS) && pending_input.is_none() {
            nudged = true;
            policy.on_stall();
            pending_input = last_input.clone();
        }
        if let Some(i) = pending_input.take()
            && let Some(f) = policy.on_input(&i, g, you)
        {
            let k = match &f {
                ClientFrame::Click { action, .. } => format!("click:{}", action_name(*action)),
                _ => "other".into(),
            };
            count(hist, k);
            if args.verbose {
                println!("  [game {n}] INPUT {} -> {}", i.message.replace('\n', " | "), f.to_json());
            }
            note(&mut trail, format!("-> {}", f.to_json()));
            link.send(f);
        }
        // A seat that is out of a multiplayer game legitimately hears nothing until the table ends.
        let out = you.and_then(|y| state.as_ref().and_then(|s| s.game.player(y))).is_some_and(|p| p.has_lost);
        if out {
            last_progress = Instant::now();
            // we gave up at the turn cap and are out; the AI seats may play on for a while
            if conceded {
                res.won = Some(false);
                break;
            }
        }
        if last_progress.elapsed() > Duration::from_secs(stall_secs) {
            res.failure = Some(format!(
                "STALL: no new input/prompt for {stall_secs}s (turn {}, last input: {})",
                res.turns,
                last_input.as_ref().map(|i| i.message.replace('\n', " | ")).unwrap_or_default()
            ));
            for l in &trail {
                println!("    trail {l}");
            }
            break;
        }
        if (t0.elapsed().as_secs() > args.game_timeout || res.turns > args.max_turns) && !conceded {
            println!("  [game {n}] game timeout ({}s / {} turns) at turn {}: conceding", args.game_timeout, args.max_turns, res.turns);
            link.send(ClientFrame::Concede);
            conceded = true;
            last_progress = Instant::now();
        }
    }
    res.secs = t0.elapsed().as_secs_f32();
    res
}

/// One multiplayer table: lobby over REST, `humans` bot seats (each on its own thread), AI for the rest.
fn play_table(args: &Args, rest: &Rest, n: u32, decks: &[String], hist: &mut BTreeMap<String, u32>) -> GameResult {
    use mtg_protocol::{CreateLobby, JoinLobby, SeatKind};
    let t0 = Instant::now();
    let players = args.players.clamp(2, 4);
    let humans = args.humans.unwrap_or(players).clamp(1, players);
    let fail = |msg: String| GameResult { failure: Some(msg), secs: t0.elapsed().as_secs_f32(), ..Default::default() };
    let host = match rest.create_lobby(&CreateLobby { format: args.format, setup: args.setup.clone(), players: players as i64, name: "Bot 1".into(), deck: decks[0].clone(), avatar: None }) {
        Ok(t) => t,
        Err(e) => return fail(format!("create lobby: {e:#}")),
    };
    let mut tickets = vec![host.clone()];
    for i in 1..humans {
        match rest.join_lobby(&host.id, &JoinLobby { name: format!("Bot {}", i + 1), deck: decks[i as usize % decks.len()].clone(), avatar: None }) {
            Ok(t) => tickets.push(t),
            Err(e) => return fail(format!("join lobby: {e:#}")),
        }
    }
    println!("  [table {n}] lobby {} (invite {}), {humans} bots + {} AI", host.id, host.invite.clone().unwrap_or_default(), players - humans);
    let links: Vec<EngineLink> = tickets.iter().map(|t| EngineLink::connect(&rest.ws_url(&format!("{}?token={}", t.id, t.token)), None)).collect();
    // host: once its socket is up (commands sent while offline are dropped), fill the empty seats
    // with AI, then start once everyone is connected
    let mut early: Vec<Vec<LinkEvent>> = vec![Vec::new(); links.len()];
    let deadline = Instant::now() + Duration::from_secs(20);
    let (mut started, mut filled) = (false, false);
    while !started && Instant::now() < deadline {
        for (i, l) in links.iter().enumerate() {
            while let Ok(e) = l.rx.try_recv() {
                if i == 0
                    && let LinkEvent::Frame(ServerFrame::Lobby(lf)) = &e
                {
                    if !filled {
                        filled = true;
                        for s in humans..players {
                            links[0].send(ClientFrame::LobbySeat { seat: s as i64, kind: SeatKind::Ai, deck: Some(decks[s as usize % decks.len()].clone()) });
                        }
                    }
                    if !started && lf.can_start && lf.seats.iter().all(|s| s.kind != SeatKind::Open) {
                        links[0].send(ClientFrame::Start { seed: Some((args.seed as i64).wrapping_add(n as i64)) });
                        started = true;
                    }
                }
                if matches!(e, LinkEvent::Frame(ServerFrame::Lobby(_))) {
                    continue;
                }
                early[i].push(e);
            }
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    if !started {
        return fail("lobby never became startable".into());
    }
    // Each seat plays on its own thread; frames that raced ahead of the split are replayed first.
    let results: Vec<(GameResult, BTreeMap<String, u32>)> = std::thread::scope(|sc| {
        let handles: Vec<_> = links
            .into_iter()
            .zip(early)
            .enumerate()
            .map(|(i, (link, early))| {
                sc.spawn(move || {
                    let (tx, rx) = crossbeam_channel::unbounded();
                    for e in early {
                        let _ = tx.send(e);
                    }
                    // forward the live link behind the replayed prefix
                    let fwd = EngineLink { tx: link.tx.clone(), rx };
                    let src = link.rx.clone();
                    std::thread::spawn(move || {
                        while let Ok(e) = src.recv() {
                            if tx.send(e).is_err() {
                                break;
                            }
                        }
                    });
                    let mut h = BTreeMap::new();
                    let r = drive_seat(args, &format!("table {n} seat {}", i + 1), args.seed.wrapping_add((n * 10 + i as u32) as u64), &fwd, &mut h, STALL_SECS * 3);
                    drop(link);
                    (r, h)
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap_or_else(|_| (GameResult { failure: Some("seat thread panicked".into()), ..Default::default() }, BTreeMap::new()))).collect()
    });
    let mut out = GameResult { secs: t0.elapsed().as_secs_f32(), ..Default::default() };
    for (i, (r, h)) in results.into_iter().enumerate() {
        for (k, v) in h {
            *hist.entry(k).or_default() += v;
        }
        out.turns = out.turns.max(r.turns);
        if out.winner.is_none() {
            out.winner = r.winner.clone();
        }
        if r.won == Some(true) {
            out.won = Some(true);
        }
        if let Some(f) = r.failure {
            out.failure.get_or_insert(format!("seat {}: {f}", i + 1));
        }
    }
    out.won.get_or_insert(false);
    out
}

/// `--join` / `--seat`: play seats at an existing table until it ends.
fn sit_in(args: &Args, rest: &Rest) -> Result<bool> {
    use mtg_protocol::{JoinLobby, SeatKind};
    let mut seats: Vec<(String, String, bool)> = Vec::new(); // (code, token, is_host)
    if let Some(s) = &args.seat {
        let (code, token) = s.split_once(':').ok_or_else(|| anyhow::anyhow!("--seat wants CODE:TOKEN"))?;
        seats.push((code.to_string(), token.to_string(), true));
    }
    if let Some(inv) = &args.join {
        let (_, code) = mtg_client::parse_invite(inv).ok_or_else(|| anyhow::anyhow!("not an invite link or code: {inv}"))?;
        for i in 0..args.bots {
            let t = rest.join_lobby(&code, &JoinLobby { name: format!("Bot {}", i + 1), deck: args.deck.clone().unwrap_or_else(|| "random".into()), avatar: None })?;
            println!("seated {} at table {} (seat {})", t.name.clone().unwrap_or_default(), t.id, t.seat + 1);
            seats.push((t.id, t.token, false));
        }
    }
    let results: Vec<GameResult> = std::thread::scope(|sc| {
        let hs: Vec<_> = seats
            .iter()
            .enumerate()
            .map(|(i, (code, token, host))| {
                sc.spawn(move || {
                    let link = EngineLink::connect(&rest.ws_url(&format!("{code}?token={token}")), None);
                    if *host {
                        // wait in the lobby until every seat is filled and connected, then start
                        let (tx, rx) = crossbeam_channel::unbounded();
                        let mut started = false;
                        while !started {
                            match link.rx.recv_timeout(Duration::from_secs(600)) {
                                Ok(LinkEvent::Frame(ServerFrame::Lobby(lf))) => {
                                    if lf.can_start && lf.seats.iter().all(|s| s.kind != SeatKind::Open) {
                                        link.send(ClientFrame::Start { seed: Some(args.seed as i64) });
                                    }
                                }
                                Ok(e @ LinkEvent::Frame(ServerFrame::Hello { .. })) => {
                                    started = true;
                                    let _ = tx.send(e);
                                }
                                Ok(_) => {}
                                Err(_) => return GameResult { failure: Some("nobody joined within 10 minutes".into()), ..Default::default() },
                            }
                        }
                        let src = link.rx.clone();
                        std::thread::spawn(move || {
                            while let Ok(e) = src.recv() {
                                if tx.send(e).is_err() {
                                    break;
                                }
                            }
                        });
                        let fwd = EngineLink { tx: link.tx.clone(), rx };
                        let mut h = BTreeMap::new();
                        drive_seat(args, &format!("seat {}", i + 1), args.seed.wrapping_add(i as u64), &fwd, &mut h, 3600)
                    } else {
                        let mut h = BTreeMap::new();
                        drive_seat(args, &format!("seat {}", i + 1), args.seed.wrapping_add(i as u64), &link, &mut h, 3600)
                    }
                })
            })
            .collect();
        hs.into_iter().map(|h| h.join().unwrap_or_default()).collect()
    });
    let mut ok = true;
    for (i, r) in results.iter().enumerate() {
        match &r.failure {
            Some(f) => {
                ok = false;
                println!("seat {}: FAILED: {f}", i + 1);
            }
            None => println!("seat {}: {} winner={:?} turns={}", i + 1, if r.won == Some(true) { "WON" } else { "lost" }, r.winner, r.turns),
        }
    }
    Ok(ok)
}

fn action_name(a: ClickAction) -> &'static str {
    match a {
        ClickAction::Ok => "ok",
        ClickAction::Cancel => "cancel",
        ClickAction::Pass => "pass",
        ClickAction::AlphaStrike => "alphaStrike",
        ClickAction::Undo => "undo",
        ClickAction::Card => "card",
        ClickAction::Player => "player",
    }
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: {e:#}");
            std::process::exit(2);
        }
    };
    std::process::exit(match run(&args) {
        Ok(true) => 0,
        Ok(false) => 1,
        Err(e) => {
            eprintln!("error: {e:#}");
            2
        }
    });
}

fn run(args: &Args) -> Result<bool> {
    let port: u16 = args.engine.rsplit(':').next().and_then(|p| p.trim_end_matches('/').parse().ok()).unwrap_or(18082);
    let _engine = if args.launch {
        let cfg = LaunchConfig {
            base_url: args.engine.clone(),
            port,
            jar: args.jar.clone(),
            assets: Some(PathBuf::from("forge/forge-gui")),
            java: None,
            health_timeout_secs: 120,
        };
        ensure_engine(&cfg, &mut |m| println!("[engine] {m}"))?
    } else {
        None
    };
    let rest = Rest::new(args.engine.clone());
    if !rest.health().unwrap_or(false) {
        bail!("engine at {} is not healthy (use --launch to start one)", args.engine);
    }
    if let Some(dir) = &args.record {
        std::fs::create_dir_all(dir)?;
    }
    if args.join.is_some() || args.seat.is_some() {
        return sit_in(args, &rest);
    }
    // A legality filters the engine's list; illegal decks come back with `problem` set and are skipped.
    let decks = if let Some(l) = &args.setup.legality { rest.decks_for(args.format, Some(l))? } else { rest.decks(args.format)? };
    println!("engine ok, {} {} decks", decks.len(), args.format.as_str());
    let mut rng = Rng(args.seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut hist = BTreeMap::new();
    let mut all_ok = true;
    let (mut wins, mut played) = (0, 0);
    for n in 1..=args.games {
        let human = pick_deck(&decks, args.deck.as_deref(), &mut rng, None)?;
        let ai = pick_deck(&decks, args.ai_deck.as_deref(), &mut rng, Some(&human))?;
        let r = if args.players >= 2 {
            let mut seat_decks = vec![human.clone(), ai.clone()];
            while seat_decks.len() < 4 {
                seat_decks.push(pick_deck(&decks, None, &mut rng, None)?);
            }
            println!("table {n}/{}: {} players: {}", args.games, args.players, seat_decks[..args.players.clamp(2, 4) as usize].join("  ·  "));
            play_table(args, &rest, n, &seat_decks, &mut hist)
        } else {
            println!("game {n}/{}: {human}  vs  {ai}", args.games);
            play_game(args, &rest, n, &human, &ai, &mut hist)
        };
        played += 1;
        match (&r.failure, r.won) {
            (Some(f), _) => {
                all_ok = false;
                println!("  game {n}: FAILED after {:.1}s: {f}", r.secs);
            }
            (None, won) => {
                if won == Some(true) {
                    wins += 1;
                }
                println!("  game {n}: {} winner={:?} turns={} {:.1}s", if won == Some(true) { "WON" } else { "LOST" }, r.winner.unwrap_or_default(), r.turns, r.secs);
            }
        }
    }
    println!("\n{played} games, {wins} won, {} failed", if all_ok { 0 } else { 1 });
    println!("prompt/click histogram:");
    for (k, v) in &hist {
        println!("  {k:<28} {v}");
    }
    Ok(all_ok)
}
