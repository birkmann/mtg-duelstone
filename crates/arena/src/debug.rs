//! Dev tooling: `--screenshot out.png --exit-after SECS` saves a frame and quits (lets agents and
//! CI eyeball the board without a human), F12 saves a screenshot to `screenshots/`.

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};

use crate::shared::CliArgs;

pub struct DebugPlugin;

impl Plugin for DebugPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (timed_exit, f12_screenshot));
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
