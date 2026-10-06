# MTG Duelstone (Rust + Bevy) — working notes

3D MTG client in Bevy 0.19 on top of the Forge rules engine (Java, `engine/`). Read `PLAN.md` first.

## Layout
- `crates/mtg_protocol` — serde mirror of `engine/docs/protocol.md`. No Bevy.
- `crates/mtg_client` — WS link (thread + channels; `ws_web.rs` in the browser), async `Api` + `spawn` (thread natively, event loop on wasm), blocking `Rest` (native tools), engine launcher, recorder/replayer, `autoplay` policy, `autopilot` bin. No Bevy.
- `crates/arena` — the Bevy app (bin `mtg-duelstone`). One `Plugin` per module; modules talk ONLY through `src/shared.rs` (the contract — append-only, each item names its owner).
- `engine/` — forge-ws-server (Java 17, GPL-3), copied from ~/Gits/mtg-cmd and extended (format, art_crop, events, multiplayer lobbies/seats, serves `web/dist`).
- `web/` — page shell for the wasm build; `tools/build-web.sh` → `web/dist` (gitignored).
- `forge/` — Forge checkout (symlink to ~/Gits/mtg-cmd/forge or clone via `tools/setup-forge.sh`; gitignored).

## Rules
- The client never computes legality: highlight what `input.selectable` says, send clicks, answer prompts by index.
- Bevy 0.19 APIs: verify against the sources in `~/.cargo/registry/src/*/bevy_*-0.19.1/` (grep), not memory.
  Messages (`#[derive(Message)]`, `MessageReader/Writer`), observers (`On<Pointer<Click>>`), `DespawnOnExit`.
- Every module starts with a `//!` doc comment explaining why. Comments explain why, not what.
- Keep the tree compiling: `cargo check -p arena` before leaving a file — and `cargo check -p arena --target wasm32-unknown-unknown`
  for anything that runs on the browser too. In code shared with wasm: no `std::thread`, `SystemTime::now`, `Instant::now`
  or files — use `mtg_client::spawn(async { api.x().await })`, `mtg_client::now_millis()`, `crate::platform`.
- Modes (v3): `GameSetup` (engine) maps format + legality + variants onto Forge's `RegisteredPlayer.forVariants`;
  limited events are `EventSession` (engine, own socket `/ws/event/{id}`) → `menus/event.rs`; cubes are `CubeStore`
  (bundled res/draft + `~/.forge/duelstone-cubes/*.json`) → `menus/cubes.rs`. Forge's draft statics (land set, AI rankings)
  are global: touch draft state only under `EventSession.DRAFT_LOCK`. Dev jumps: `ARENA_MENU_SCREEN=modes|cubes|draft|avatar`
  (+ `ARENA_DRAFT_STOP=pick:N|build`, `ARENA_CUBE=<ref>`), `ARENA_LOBBY_HOLD` with `--quick --players N`, and for game shots
  `ARENA_AUTOPLAY_PAUSE=<turn>` + `ARENA_SHOT_WHEN=paused` (+ `ARENA_HOVER_COMMAND`) under `--autoplay`.
- Multiplayer: never assume one opponent. Seats are slots (`GameView::seat_order`: 0 = you), anchors per seat
  (`BoardAnchors::seats` / `avatar_of`). Engine-side, every human seat is its own `WsGuiGame`.
- Avatars are data, not images: `mtg_protocol::Avatar {card, x, y, zoom}` = a circle on that card's art crop. The engine
  relays each seat's (`hello.seats`, by player name) and picks one from the deck for seats without; UI shows them via `AvatarArt`.

## Commands
```bash
source tools/env.sh                                   # JDK 17 + Maven (user-local)
(cd engine && mvn -q -B package)                      # → engine/target/forge-ws-server.jar
java -jar engine/target/forge-ws-server.jar --port 8080 --assets forge/forge-gui
cargo run -p arena -- --quick                         # straight into a match (autolaunches engine)
cargo run -p arena -- --quick --autoplay --screenshot shots/a.png --exit-after 40   # unattended screenshot
cargo run -p mtg_client --bin autopilot -- --format constructed --games 5
cargo run -p mtg_client --bin autopilot -- --engine http://127.0.0.1:8080 --players 4 --games 2   # multiplayer soak
cargo run -p mtg_client --bin autopilot -- --engine http://127.0.0.1:8080 --legality pauper --variant planechase  # modes soak
tools/build-web.sh [--dev]                            # browser client → web/dist (served by the engine)
./run.sh --serve                                      # engine only; play at http://<host>:8080/
cargo test --workspace
./run.sh                                              # build engine if stale + run the game
tools/fetch-offline.sh                                # once, online: fill offline/ (images, rulings, vendor, m2)
```

## Offline
Nothing touches the network at runtime: `/api/img` and `/api/rulings` are served from `offline/` (see
`offline/README.md`), and the splash showcase reads it directly. Only `fetch-offline` (bin) downloads. Don't add
runtime network calls; put new data into the fetcher. After adding a crate, re-run `tools/fetch-offline.sh`.
