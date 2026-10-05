//! Persisted user settings (`~/.config/mtg-arena/settings.json`; in the browser: localStorage).
//!
//! Why here: every other module just reads `Res<Settings>`; this plugin loads it before any
//! Startup system runs, writes it back (debounced) after menus edit it, and applies the
//! settings that need an engine-side effect (fullscreen, master volume).

#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

use bevy::audio::{GlobalVolume, Volume};
use bevy::prelude::*;
use bevy::window::{MonitorSelection, PrimaryWindow, WindowMode};

use crate::shared::Settings;

pub struct SettingsPlugin;

impl Plugin for SettingsPlugin {
    fn build(&self, app: &mut App) {
        let s = load();
        app.insert_resource(s)
            .init_resource::<SaveTimer>()
            .add_systems(Update, (apply_settings, save_debounced).chain());
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn settings_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "mtg-arena").map(|d| d.config_dir().join("settings.json"))
}

const WEB_KEY: &str = "mtg-arena.settings";

#[cfg(target_arch = "wasm32")]
fn load() -> Settings {
    crate::platform::storage_get(WEB_KEY).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

#[cfg(target_arch = "wasm32")]
fn save(s: &Settings) {
    if let Ok(text) = serde_json::to_string(s)
        && !crate::platform::storage_set(WEB_KEY, &text)
    {
        warn!("settings: localStorage unavailable");
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn load() -> Settings {
    let Some(path) = settings_path() else { return Settings::default() };
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
            warn!("settings: {} unreadable ({e}); using defaults", path.display());
            Settings::default()
        }),
        Err(_) => Settings::default(),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn save(s: &Settings) {
    let _ = WEB_KEY;
    let Some(path) = settings_path() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    match serde_json::to_string_pretty(s) {
        Ok(text) => {
            if let Err(e) = std::fs::write(&path, text) {
                warn!("settings: cannot write {}: {e}", path.display());
            }
        }
        Err(e) => warn!("settings: serialize failed: {e}"),
    }
}

#[derive(Resource, Default)]
struct SaveTimer {
    /// seconds since the last change; `None` = clean
    dirty_for: Option<f32>,
    seen_first: bool,
}

fn apply_settings(
    settings: Res<Settings>,
    mut volume: ResMut<GlobalVolume>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut timer: ResMut<SaveTimer>,
) {
    if !settings.is_changed() {
        return;
    }
    // The very first "change" is the initial insert: apply but don't write the file back.
    if timer.seen_first {
        timer.dirty_for = Some(0.0);
    }
    timer.seen_first = true;

    volume.volume = Volume::Linear(settings.master_volume.clamp(0.0, 1.0));
    if let Ok(mut w) = windows.single_mut() {
        let want = if settings.fullscreen {
            WindowMode::BorderlessFullscreen(MonitorSelection::Current)
        } else {
            WindowMode::Windowed
        };
        if w.mode != want {
            w.mode = want;
        }
    }
}

fn save_debounced(time: Res<Time>, settings: Res<Settings>, mut timer: ResMut<SaveTimer>) {
    let Some(t) = timer.dirty_for.as_mut() else { return };
    *t += time.delta_secs();
    if *t >= 0.75 {
        timer.dirty_for = None;
        save(&settings);
    }
}

