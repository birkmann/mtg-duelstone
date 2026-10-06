//! Sound: one-shot SFX from `BoardEvent`s / button interaction / `PlaySfx`, plus two ambient
//! loops (menu, match) that crossfade with the app state.
//!
//! Why rate limiting: a mass-damage event or a 7-card opening draw would otherwise stack dozens of
//! identical samples into a loud smear. Each sound kind has a minimum gap.
//!
//! Why takes: every sound ships as several recordings (`sfx/<name>_<n>.ogg`, built from CC0 foley
//! by `assets/tools/build_sfx.py`). Each play picks one other than the last and, for foley, nudges
//! pitch and level, so a draw step or a combat doesn't sound like one sample on repeat. Their
//! loudness is balanced in the build script; `tuning` only adds rate limits and fine trims.
//!
//! Volumes: `GlobalVolume` (= master, set by `settings`) is applied by Bevy when a sound is
//! spawned, so one-shots only multiply by the SFX slider; music sinks are driven every frame so
//! slider changes are audible immediately (and so they fade smoothly between states).

use std::collections::HashMap;

use bevy::audio::{AudioSinkPlayback, Volume};
use bevy::prelude::*;
use mtg_protocol::ZoneKind;

use crate::shared::*;
use crate::style::ArenaButton;

pub struct AudioPlugin;

impl Plugin for AudioPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<PlaySfx>()
            .init_resource::<SfxLimiter>()
            .add_systems(Startup, (load_sfx, start_music))
            .add_systems(Update, (events_to_sfx, button_sfx, play_sfx, drive_music).chain());
    }
}

#[derive(Resource)]
struct SfxAssets(HashMap<Sfx, Vec<Handle<AudioSource>>>);

/// Per sound: when it last played and which take, plus the random state for picking takes.
#[derive(Resource)]
struct SfxLimiter {
    last: HashMap<Sfx, (f32, usize)>,
    rng: u64,
}

impl Default for SfxLimiter {
    fn default() -> Self {
        Self { last: HashMap::new(), rng: mtg_client::now_millis() | 1 }
    }
}

impl SfxLimiter {
    /// xorshift64*: plenty for picking takes, and works the same in the browser.
    fn next(&mut self) -> f32 {
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        (self.rng.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40) as f32 / (1u64 << 24) as f32
    }
}

const ALL_SFX: [Sfx; 15] = [
    Sfx::CardDraw,
    Sfx::CardPlay,
    Sfx::LandDrop,
    Sfx::SpellCast,
    Sfx::Resolve,
    Sfx::AttackWhoosh,
    Sfx::HitImpact,
    Sfx::LifeLoss,
    Sfx::LifeGain,
    Sfx::TurnStart,
    Sfx::ButtonClick,
    Sfx::HoverTick,
    Sfx::Victory,
    Sfx::Defeat,
    Sfx::CardFan,
];

/// File stem and number of takes, as `build_sfx.py` writes them (`sfx/<stem>_<1..=n>.ogg`).
fn takes(s: Sfx) -> (&'static str, usize) {
    match s {
        Sfx::CardDraw => ("card_draw", 6),
        Sfx::CardPlay => ("card_play", 4),
        Sfx::LandDrop => ("land_drop", 4),
        Sfx::SpellCast => ("spell_cast", 3),
        Sfx::Resolve => ("resolve", 3),
        Sfx::AttackWhoosh => ("attack_whoosh", 3),
        Sfx::HitImpact => ("hit_impact", 5),
        Sfx::LifeLoss => ("life_loss", 3),
        Sfx::LifeGain => ("life_gain", 3),
        Sfx::TurnStart => ("turn_start", 2),
        Sfx::ButtonClick => ("button_click", 3),
        Sfx::HoverTick => ("hover_tick", 3),
        Sfx::Victory => ("victory", 2),
        Sfx::Defeat => ("defeat", 2),
        Sfx::CardFan => ("card_fan", 2),
    }
}

/// (minimum seconds between plays, gain trim, pitch/level variation 0..1). Musical cues get no
/// variation: a detuned victory sting sounds wrong, a detuned card slide sounds like another card.
fn tuning(s: Sfx) -> (f32, f32, f32) {
    match s {
        Sfx::CardDraw => (0.09, 1.0, 1.0),
        Sfx::CardPlay => (0.12, 1.0, 1.0),
        Sfx::LandDrop => (0.15, 1.0, 1.0),
        Sfx::SpellCast => (0.25, 1.0, 0.5),
        Sfx::Resolve => (0.18, 1.0, 0.3),
        Sfx::AttackWhoosh => (0.4, 1.0, 1.0),
        Sfx::HitImpact => (0.07, 1.0, 1.0),
        Sfx::LifeLoss => (0.3, 1.0, 0.6),
        Sfx::LifeGain => (0.3, 1.0, 0.0),
        Sfx::TurnStart => (1.0, 1.0, 0.0),
        Sfx::ButtonClick => (0.05, 1.0, 0.6),
        Sfx::HoverTick => (0.05, 1.0, 0.6),
        Sfx::Victory => (2.0, 1.0, 0.0),
        Sfx::Defeat => (2.0, 1.0, 0.0),
        Sfx::CardFan => (1.0, 1.0, 0.3),
    }
}

fn load_sfx(mut commands: Commands, assets: Res<AssetServer>) {
    let map = ALL_SFX
        .into_iter()
        .map(|s| {
            let (stem, n) = takes(s);
            (s, (1..=n).map(|i| assets.load(format!("sfx/{stem}_{i}.ogg"))).collect())
        })
        .collect();
    commands.insert_resource(SfxAssets(map));
}

fn play_sfx(
    mut commands: Commands,
    mut reqs: MessageReader<PlaySfx>,
    assets: Option<Res<SfxAssets>>,
    settings: Res<Settings>,
    time: Res<Time>,
    mut limiter: ResMut<SfxLimiter>,
) {
    let Some(assets) = assets else { return };
    let now = time.elapsed_secs();
    for PlaySfx(s) in reqs.read() {
        let (gap, gain, vary) = tuning(*s);
        let prev = limiter.last.get(s).copied();
        if prev.is_some_and(|(t, _)| now - t < gap) {
            continue;
        }
        let Some(hs) = assets.0.get(s).filter(|v| !v.is_empty()) else { continue };
        // any take but the one just heard
        let mut pick = (limiter.next() * hs.len() as f32) as usize % hs.len();
        if hs.len() > 1 && prev.is_some_and(|(_, last)| last == pick) {
            pick = (pick + 1) % hs.len();
        }
        limiter.last.insert(*s, (now, pick));
        // ±2.5 % pitch and ±1.5 dB at full variation: a different card, not a different sound
        let speed = 1.0 + (limiter.next() * 2.0 - 1.0) * 0.025 * vary;
        let level = 10f32.powf((limiter.next() * 2.0 - 1.0) * 1.5 * vary / 20.0);
        let vol = (settings.sfx_volume * gain * level).clamp(0.0, 1.5);
        if vol <= 0.001 {
            continue;
        }
        commands.spawn((AudioPlayer::<AudioSource>(hs[pick].clone()), PlaybackSettings::DESPAWN.with_volume(Volume::Linear(vol)).with_speed(speed)));
    }
}

fn events_to_sfx(mut events: MessageReader<BoardEvent>, mut out: MessageWriter<PlaySfx>, view: Res<GameView>, cache: Res<CardCache>) {
    for ev in events.read() {
        match ev {
            BoardEvent::CardAppeared { zone: ZoneKind::Hand, .. } => {
                out.write(PlaySfx(Sfx::CardDraw));
            }
            BoardEvent::CardMoved { id, from, to, .. } => {
                let is_land = cache.0.get(id).or_else(|| view.card(*id)).and_then(|c| c.state.as_ref()).is_some_and(|s| s.is_land);
                let s = match (*from, *to) {
                    (ZoneKind::Library, ZoneKind::Hand) => Some(Sfx::CardDraw),
                    (_, ZoneKind::Battlefield) if is_land => Some(Sfx::LandDrop),
                    (_, ZoneKind::Battlefield) => Some(Sfx::CardPlay),
                    (_, ZoneKind::Stack) => Some(Sfx::SpellCast),
                    _ => None,
                };
                if let Some(s) = s {
                    out.write(PlaySfx(s));
                }
            }
            BoardEvent::StackResolved { .. } => {
                out.write(PlaySfx(Sfx::Resolve));
            }
            BoardEvent::CardDamaged { .. } => {
                out.write(PlaySfx(Sfx::HitImpact));
            }
            BoardEvent::LifeChanged { old, new, .. } => {
                out.write(PlaySfx(if new < old { Sfx::LifeLoss } else { Sfx::LifeGain }));
            }
            BoardEvent::TurnStarted { player, .. } => {
                if view.is_me(*player) {
                    out.write(PlaySfx(Sfx::TurnStart));
                }
            }
            BoardEvent::AttackersDeclared { .. } => {
                out.write(PlaySfx(Sfx::AttackWhoosh));
            }
            BoardEvent::GameOver { you_won } => {
                out.write(PlaySfx(if *you_won { Sfx::Victory } else { Sfx::Defeat }));
            }
            BoardEvent::Rejected { .. } => {
                out.write(PlaySfx(Sfx::HoverTick));
            }
            _ => {}
        }
    }
}

/// Hover tick / click for every `Button` in the app (menus, HUD, prompts), without each plugin
/// having to know about audio.
fn button_sfx(
    q: Query<(&Interaction, Option<&ArenaButton>), (Changed<Interaction>, With<Button>)>,
    mut out: MessageWriter<PlaySfx>,
) {
    for (i, ab) in &q {
        if ab.is_some_and(|b| b.disabled) {
            continue;
        }
        match i {
            Interaction::Pressed => {
                out.write(PlaySfx(Sfx::ButtonClick));
            }
            Interaction::Hovered => {
                out.write(PlaySfx(Sfx::HoverTick));
            }
            Interaction::None => {}
        }
    }
}

// -------------------------------------------------------------------------------------------
// Music
// -------------------------------------------------------------------------------------------

#[derive(Component)]
struct Music {
    /// true = match track
    in_match: bool,
    fade: f32,
}

fn start_music(mut commands: Commands, assets: Res<AssetServer>) {
    for (in_match, path) in [(false, "sfx/ambient.ogg"), (true, "sfx/ambient_match.ogg")] {
        commands.spawn((
            Name::new(if in_match { "music match" } else { "music menu" }),
            Music { in_match, fade: 0.0 },
            AudioPlayer::<AudioSource>(assets.load(path)),
            PlaybackSettings::LOOP.with_volume(Volume::Linear(0.0)),
        ));
    }
}

fn drive_music(
    time: Res<Time>,
    state: Res<State<AppState>>,
    settings: Res<Settings>,
    mut q: Query<(&mut Music, Option<&mut AudioSink>)>,
) {
    let matchy = matches!(state.get(), AppState::InMatch | AppState::GameOver);
    let k = (time.delta_secs() / 1.6).min(1.0); // ~1.6 s crossfade
    for (mut m, sink) in &mut q {
        let target = if m.in_match == matchy { 1.0 } else { 0.0 };
        m.fade += (target - m.fade).clamp(-k, k);
        if let Some(mut sink) = sink {
            // equal-power-ish curve
            let gain = m.fade * m.fade * (3.0 - 2.0 * m.fade);
            let v = settings.master_volume * settings.music_volume * gain * if m.in_match { 0.7 } else { 1.0 };
            sink.set_volume(Volume::Linear(v.clamp(0.0, 1.5)));
        }
    }
}
