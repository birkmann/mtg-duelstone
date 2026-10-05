use std::path::PathBuf;

use arena::shared::{AppState, CliArgs};
use bevy::prelude::*;
use bevy::window::WindowResolution;
use mtg_protocol::Format;

fn main() {
    let cli = parse_args();
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "MTG Arena".into(),
                        resolution: WindowResolution::new(1600, 900),
                        ..default()
                    }),
                    ..default()
                })
                .set(bevy::log::LogPlugin {
                    filter: "warn,arena=info,mtg_client=info,wgpu=error,naga=warn".into(),
                    ..default()
                }),
        )
        .insert_resource(cli)
        .init_state::<AppState>()
        .add_plugins(arena::ArenaPlugin)
        .run();
}

fn parse_args() -> CliArgs {
    let mut cli = CliArgs::default();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--format" => {
                cli.format = args.next().map(|f| if f == "commander" { Format::Commander } else { Format::Constructed })
            }
            "--deck" => cli.deck = args.next(),
            "--ai-deck" => cli.ai_deck = args.next(),
            "--seed" => cli.seed = args.next().and_then(|s| s.parse().ok()),
            "--quick" => cli.quick = true,
            "--replay" => cli.replay = args.next().map(PathBuf::from),
            "--record" => cli.record = args.next().map(PathBuf::from),
            "--engine" => cli.engine_url = args.next(),
            "--no-autolaunch" => cli.no_autolaunch = true,
            "--screenshot" => cli.screenshot = args.next().map(PathBuf::from),
            "--exit-after" => cli.exit_after = args.next().and_then(|s| s.parse().ok()),
            "--autoplay" => cli.autoplay = true,
            "-h" | "--help" => {
                println!(
                    "mtg-arena [--quick] [--format constructed|commander] [--deck REF] [--ai-deck REF] [--seed N]\n\
                     \x20         [--replay FILE.jsonl] [--record FILE.jsonl] [--engine URL] [--no-autolaunch]\n\
                     \x20         [--screenshot OUT.png] [--exit-after SECS] [--autoplay]"
                );
                std::process::exit(0);
            }
            other => eprintln!("ignoring unknown argument {other}"),
        }
    }
    cli
}
