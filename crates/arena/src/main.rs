// Windows release builds are a windowed program: no console window next to the game.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

use arena::shared::{AppState, CliArgs};
use bevy::asset::AssetMetaCheck;
use bevy::prelude::*;
use bevy::window::WindowResolution;
use mtg_protocol::Format;

fn main() {
    let cli = parse_args();
    let web = arena::platform::WEB;
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(bevy::asset::AssetPlugin {
                    // In the browser assets are fetched over HTTP from the engine (`web/dist/assets/`).
                    file_path: if web { "assets".into() } else { arena::images::assets_dir().to_string_lossy().into_owned() },
                    // We ship no .meta files; probing for them costs a 404 per asset on the web.
                    meta_check: AssetMetaCheck::Never,
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "MTG Duelstone".into(),
                        // Wayland app id / X11 WM_CLASS: matches StartupWMClass in packaging/mtg-duelstone.desktop.
                        name: Some("mtg-duelstone".into()),
                        resolution: std::env::var("ARENA_WIN").ok().and_then(|s| s.split_once('x').and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))).map(|(w, h): (u32, u32)| WindowResolution::new(w, h)).unwrap_or_else(|| WindowResolution::new(1600, 900)),
                        // web: draw into the page's canvas and follow its size
                        canvas: web.then(|| "#mtg-duelstone".into()),
                        fit_canvas_to_parent: true,
                        // keep browser shortcuts (Ctrl+V paste, F5, devtools) working
                        prevent_default_event_handling: false,
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

/// Browser: the page URL is the command line (`/?join=ABC123`, `/?quick&format=commander`).
#[cfg(target_arch = "wasm32")]
fn parse_args() -> CliArgs {
    use arena::platform::query_param;
    CliArgs {
        format: query_param("format").as_deref().and_then(Format::parse),
        deck: query_param("deck"),
        ai_deck: query_param("ai-deck"),
        seed: query_param("seed").and_then(|s| s.parse().ok()),
        quick: query_param("quick").is_some(),
        // `?join=` is a table, `?event=` a draft pod: the multiplayer screen looks up either
        join: query_param("join").or_else(|| query_param("event")).filter(|j| !j.is_empty()),
        autoplay: query_param("autoplay").is_some(),
        // there is nothing to launch: the engine served this page
        no_autolaunch: true,
        ..default()
    }
}

/// `mtg-duelstone --fetch-images [ARGS]`: run the `fetch-offline` tool packaged next to this binary.
/// Why here: an AppImage or a macOS app has one obvious executable, so this is how release users
/// download the card images (they are not redistributed) without unpacking anything.
#[cfg(not(target_arch = "wasm32"))]
fn fetch_images(args: Vec<String>) -> ! {
    let exe = std::env::current_exe().and_then(|e| e.canonicalize()).ok();
    let tool = exe.as_deref().and_then(|e| e.parent()).map(|d| d.join(if cfg!(windows) { "fetch-offline.exe" } else { "fetch-offline" }));
    let Some(tool) = tool.filter(|t| t.is_file()) else {
        eprintln!("fetch-offline is not next to this binary; from a source checkout run tools/fetch-offline.sh");
        std::process::exit(2);
    };
    match std::process::Command::new(&tool).args(args).status() {
        Ok(s) => std::process::exit(s.code().unwrap_or(1)),
        Err(e) => {
            eprintln!("cannot run {}: {e}", tool.display());
            std::process::exit(2);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn parse_args() -> CliArgs {
    if std::env::args().nth(1).as_deref() == Some("--fetch-images") {
        fetch_images(std::env::args().skip(2).collect());
    }
    let mut cli = CliArgs::default();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--format" => match args.next().as_deref().map(Format::parse) {
                Some(Some(f)) => cli.format = Some(f),
                _ => eprintln!("--format: constructed, commander, brawl or oathbreaker"),
            },
            "--legality" => cli.setup.legality = args.next(),
            "--variant" => match args.next().as_deref().and_then(mtg_protocol::Variant::parse) {
                Some(v) => cli.setup.variants.push(v),
                None => eprintln!("--variant: planechase, archenemy, momir or teams"),
            },
            "--players" => cli.players = args.next().and_then(|s| s.parse().ok()),
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
            "--join" => cli.join = args.next(),
            "-h" | "--help" => {
                println!(
                    "mtg-duelstone [--quick] [--format constructed|commander|brawl|oathbreaker] [--deck REF] [--ai-deck REF] [--seed N]\n\
                     \x20         [--legality standard|pioneer|modern|legacy|vintage|pauper] [--variant planechase|archenemy|momir|teams]...\n\
                     \x20         [--players 2..4] [--join INVITE-LINK-OR-CODE]\n\
                     \x20         [--replay FILE.jsonl] [--record FILE.jsonl] [--engine URL] [--no-autolaunch]\n\
                     \x20         [--screenshot OUT.png] [--exit-after SECS] [--autoplay]\n\
                     mtg-duelstone --fetch-images      download the card images once (about 9 GB)"
                );
                std::process::exit(0);
            }
            other => eprintln!("ignoring unknown argument {other}"),
        }
    }
    cli
}
