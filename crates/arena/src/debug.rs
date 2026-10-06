//! Dev tooling: `--screenshot out.png --exit-after SECS` saves a frame and quits (lets agents and
//! CI eyeball the board without a human), F12 saves a screenshot to `screenshots/`.

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};

use mtg_client::autoplay::AutoPolicy;

use crate::shared::*;

pub struct DebugPlugin;

impl Plugin for DebugPlugin {
    fn build(&self, app: &mut App) {
        if std::env::var("ARENA_FPS").is_ok() {
            app.add_plugins((
                bevy::diagnostic::FrameTimeDiagnosticsPlugin::default(),
                bevy::diagnostic::LogDiagnosticsPlugin::default(),
            ));
        }
        app.add_systems(Update, (timed_exit, f12_screenshot, dump_cards))
            .add_systems(Update, autoplay.run_if(in_state(AppState::InMatch)));
    }
}

/// Dev aid for screenshots: `ARENA_AUTOPLAY_PAUSE=N` stops the bot in my first main phase of turn N or
/// later, so the board holds still (the engine waits for us) until `ARENA_SHOT_WHEN=paused` fires.
/// `ARENA_PAUSE_PHASE=attackers` stops at my declare-attackers step instead.
/// With `ARENA_PAUSE_UNTAPPED` it only stops on a turn where no land lies tapped (sideways lands
/// read as clutter in a showcase shot).
pub fn paused_for_shot(view: &GameView) -> bool {
    let Some(n) = std::env::var("ARENA_AUTOPLAY_PAUSE").ok().and_then(|s| s.parse::<i32>().ok()) else { return false };
    let g = &view.game;
    // ARENA_PAUSE_PHASE=attackers holds at my declare-attackers step instead of main phase 1
    let phase = match std::env::var("ARENA_PAUSE_PHASE").as_deref() {
        Ok("attackers") => mtg_protocol::PhaseType::CombatDeclareAttackers,
        _ => mtg_protocol::PhaseType::Main1,
    };
    let here = g.turn >= n && g.player_turn.is_some() && g.player_turn == view.you && g.phase == Some(phase);
    let untapped = std::env::var("ARENA_PAUSE_UNTAPPED").is_err()
        || !g.players.iter().flat_map(|p| &p.zones.battlefield).any(|c| c.tapped && c.state.as_ref().is_some_and(|s| s.is_land));
    here && untapped
}

/// `--autoplay`: drive the human seat with the shared bot policy, slowly enough that the board
/// animations are visible in screenshots/recordings.
fn autoplay(
    cli: Res<CliArgs>,
    time: Res<Time>,
    view: Res<GameView>,
    mut input: ResMut<CurrentInput>,
    prompt: Res<PendingPrompt>,
    mut events: MessageReader<BoardEvent>,
    mut frames: MessageReader<ServerMsg>,
    mut out: MessageWriter<SendCmd>,
    mut policy: Local<Option<AutoPolicy>>,
    mut next_at: Local<f32>,
    mut last_progress: Local<(i64, f32)>,
) {
    if !cli.autoplay {
        return;
    }
    let policy = policy.get_or_insert_with(|| AutoPolicy::new(cli.seed.unwrap_or(7) as u64));
    let now = time.elapsed_secs();
    // engine messages ("Attack declaration invalid", …) let the policy break declare livelocks
    for ServerMsg(f) in frames.read() {
        if let mtg_protocol::ServerFrame::Message { text, .. } = f {
            policy.on_message(text);
        }
    }
    for e in events.read() {
        if matches!(e, BoardEvent::Rejected { .. }) {
            policy.on_flash();
            input.awaiting = false;
        }
    }
    if view.seq != last_progress.0 {
        *last_progress = (view.seq, now);
    }
    if now < *next_at || paused_for_shot(&view) {
        return;
    }
    if let Some(p) = &prompt.0 {
        out.write(SendCmd(policy.on_prompt(p, &view.game, view.you)));
        *next_at = now + 0.4;
        return;
    }
    let Some(frame) = input.frame.clone() else { return };
    if input.awaiting {
        // nothing came back for a long while: let the policy shake things up
        if now - last_progress.1 > 8.0 {
            policy.on_stall();
            input.awaiting = false;
            last_progress.1 = now;
        }
        return;
    }
    if let Some(reply) = policy.on_input(&frame, &view.game, view.you) {
        out.write(SendCmd(reply));
        input.awaiting = true;
        *next_at = now + 0.4;
    } else {
        *next_at = now + 0.15;
    }
}

fn timed_exit(
    mut commands: Commands,
    cli: Res<CliArgs>,
    time: Res<Time>,
    mut shot_taken: Local<bool>,
    mut shot_at: Local<f32>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(after) = cli.exit_after else { return };
    let t = time.elapsed_secs();
    if t < after {
        return;
    }
    if let Some(path) = &cli.screenshot {
        if !*shot_taken {
            *shot_taken = true;
            *shot_at = t;
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path.clone()));
            return;
        }
        // give the capture a moment to be written
        if t - *shot_at < 1.0 {
            return;
        }
    }
    exit.write(AppExit::Success);
}

fn f12_screenshot(mut commands: Commands, keys: Res<ButtonInput<KeyCode>>, mut n: Local<u32>) {
    if keys.just_pressed(KeyCode::F12) {
        *n += 1;
        let _ = std::fs::create_dir_all("screenshots");
        let path = format!("screenshots/shot-{:03}.png", *n);
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
    }
}

/// `ARENA_DUMP=1`: print every card's zone / target / current pose once after `--exit-after` - 1 s.
fn dump_cards(
    cli: Res<CliArgs>,
    time: Res<Time>,
    mut done: Local<bool>,
    cards: Query<(&CardEntity, &CardLocation, Option<&TargetPlacement>, &Transform, &crate::cards::CardFace)>,
    imgs: Res<crate::images::CardImages>,
) {
    if *done || std::env::var("ARENA_DUMP").is_err() {
        return;
    }
    let Some(after) = cli.exit_after else { return };
    if time.elapsed_secs() < after - 1.0 {
        return;
    }
    *done = true;
    println!("images pending={}", imgs.pending());
    for (id, loc, tp, tf, face) in &cards {
        println!(
            "card {} {} loaded={} hidden={} {:?} mine={} tp={:?} pos={:.2?} scale={:.2}",
            id.0,
            face.name,
            face.loaded,
            face.hidden,
            loc.zone,
            loc.mine,
            tp.map(|t| (t.translation, t.layer, t.face_up)),
            tf.translation,
            tf.scale.x
        );
    }
}
