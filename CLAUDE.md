# MTG Duelstone (Rust + Bevy) — working notes

3D MTG client in Bevy 0.19 on top of the Forge rules engine (Java, `engine/`). Read `PLAN.md` first.

## Layout
- `crates/mtg_protocol` — serde mirror of `engine/docs/protocol.md`. No Bevy.
- `crates/mtg_client` — WS link (thread + channels), REST, engine launcher, recorder/replayer, `autoplay` policy, `autopilot` bin. No Bevy.
- `crates/arena` — the Bevy app (bin `mtg-duelstone`). One `Plugin` per module; modules talk ONLY through `src/shared.rs` (the contract — append-only, each item names its owner).
- `engine/` — forge-ws-server (Java 17, GPL-3), copied from ~/Gits/mtg-cmd and extended (format, art_crop, events).
- `forge/` — Forge checkout (symlink to ~/Gits/mtg-cmd/forge or clone via `tools/setup-forge.sh`; gitignored).

## Rules
- The client never computes legality: highlight what `input.selectable` says, send clicks, answer prompts by index.
- Bevy 0.19 APIs: verify against the sources in `~/.cargo/registry/src/*/bevy_*-0.19.1/` (grep), not memory.
  Messages (`#[derive(Message)]`, `MessageReader/Writer`), observers (`On<Pointer<Click>>`), `DespawnOnExit`.
- Every module starts with a `//!` doc comment explaining why. Comments explain why, not what.
- Keep the tree compiling: `cargo check -p arena` before leaving a file.

## Commands
```bash
source tools/env.sh                                   # JDK 17 + Maven (user-local)
(cd engine && mvn -q -B package)                      # → engine/target/forge-ws-server.jar
java -jar engine/target/forge-ws-server.jar --port 8080 --assets forge/forge-gui
cargo run -p arena -- --quick                         # straight into a match (autolaunches engine)
cargo run -p arena -- --quick --autoplay --screenshot shots/a.png --exit-after 40   # unattended screenshot
cargo run -p mtg_client --bin autopilot -- --format constructed --games 5
cargo test --workspace
./run.sh                                              # build engine if stale + run the game
tools/fetch-offline.sh                                # once, online: fill offline/ (images, rulings, vendor, m2)
```

## Offline
Nothing touches the network at runtime: `/api/img` and `/api/rulings` are served from `offline/` (see
`offline/README.md`), and the splash showcase reads it directly. Only `fetch-offline` (bin) downloads. Don't add
runtime network calls; put new data into the fetcher. After adding a crate, re-run `tools/fetch-offline.sh`.
