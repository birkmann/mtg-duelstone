//! Sound: one-shot SFX from `BoardEvent`s / button interaction / `PlaySfx`, plus two ambient
//! loops (menu, match) that crossfade with the app state.
//!
//! Why rate limiting: a mass-damage event or a 7-card opening draw would otherwise stack dozens of
//! identical samples into a loud smear. Each sound kind has a minimum gap.
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
struct SfxAssets(HashMap<Sfx, Handle<AudioSource>>);

#[derive(Resource, Default)]
struct SfxLimiter(HashMap<Sfx, f32>);

fn file(s: Sfx) -> &'static str {
    match s {
        Sfx::CardDraw => "card_draw",
        Sfx::CardPlay => "card_play",
        Sfx::LandDrop => "land_drop",
        Sfx::SpellCast => "spell_cast",
        Sfx::Resolve => "resolve",
        Sfx::AttackWhoosh => "attack_whoosh",
        Sfx::HitImpact => "hit_impact",
        Sfx::LifeLoss => "life_loss",
        Sfx::LifeGain => "life_gain",
        Sfx::TurnStart => "turn_start",
        Sfx::ButtonClick => "button_click",
        Sfx::HoverTick => "hover_tick",
        Sfx::Victory => "victory",
        Sfx::Defeat => "defeat",
    }
}

/// (minimum seconds between plays, relative gain)
fn tuning(s: Sfx) -> (f32, f32) {
    match s {
        Sfx::CardDraw => (0.11, 0.7),
        Sfx::CardPlay => (0.12, 0.9),
        Sfx::LandDrop => (0.15, 0.9),
        Sfx::SpellCast => (0.25, 0.85),
        Sfx::Resolve => (0.18, 0.7),
        Sfx::AttackWhoosh => (0.5, 0.9),
        Sfx::HitImpact => (0.07, 0.9),
        Sfx::LifeLoss => (0.3, 0.9),
        Sfx::LifeGain => (0.3, 0.8),
        Sfx::TurnStart => (1.0, 0.7),
        Sfx::ButtonClick => (0.05, 0.8),
        Sfx::HoverTick => (0.05, 0.35),
        Sfx::Victory => (2.0, 1.0),
        Sfx::Defeat => (2.0, 1.0),
    }
}

fn load_sfx(mut commands: Commands, assets: Res<AssetServer>) {
    use Sfx::*;
    let all = [CardDraw, CardPlay, LandDrop, SpellCast, Resolve, AttackWhoosh, HitImpact, LifeLoss, LifeGain, TurnStart, ButtonClick, HoverTick, Victory, Defeat];
    commands.insert_resource(SfxAssets(all.into_iter().map(|s| (s, assets.load(format!("sfx/{}.ogg", file(s))))).collect()));
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
        let (gap, gain) = tuning(*s);
        if limiter.0.get(s).is_some_and(|t| now - t < gap) {
            continue;
        }
        limiter.0.insert(*s, now);
        let Some(h) = assets.0.get(s) else { continue };
        let vol = (settings.sfx_volume * gain).clamp(0.0, 1.5);
        if vol <= 0.001 {
            continue;
        }
        // tiny pitch jitter keeps repeats from sounding machine-gunned
        let jitter = 1.0 + ((now * 997.0).sin() * 0.04);
        commands.spawn((AudioPlayer::<AudioSource>(h.clone()), PlaybackSettings::DESPAWN.with_volume(Volume::Linear(vol)).with_speed(jitter)));
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
