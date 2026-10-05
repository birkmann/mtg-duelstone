# MTG Arena (Rust + Bevy)

A desktop **Magic: The Gathering** client that looks and plays like *MTG Arena*: a tilted 3D table,
art-crop battlefield cards, blue-glow playable cards, sweeping turn banners, floating damage
numbers, targeting arrows and a proper menu flow. You play 1v1 against the **Forge** AI in
**60-card Constructed (20 life)** or **1v1 Commander (40 life)**.

The client never decides what is legal. All rules, combat, the stack and the AI come from the
[Forge](https://github.com/Card-Forge/forge) engine (~33 000 scripted cards) running headless behind
a small WebSocket/JSON server in [`engine/`](engine/). The Bevy app renders the state it is given,
highlights what the engine says is selectable and answers prompts by index.

<!-- Screenshots: drop PNGs in docs/ and uncomment.
![Main menu](docs/menu.png)
![Deck select](docs/deck-select.png)
![Match](docs/match.png)
-->

## Features

- **Arena-style board**: perspective table with themed playmats (Sanctum, Jungle, Storm), fanned
  hand, life orbs, phase track, stack, graveyard/library piles, command zone, grouped lands/tokens
  ("x4" badges), hover preview.
- **Battlefield frames**: custom WGSL card material (art crop in a frame coloured by the card's colours,
  pale name bar, tapped/damage/hover states, pulsing selectable rim). Hand and stack keep full card faces.
- **Effects & sound**: floating damage/life numbers, hit shake, spark bursts, land-drop dust ring,
  cast flash, "YOUR TURN" banner, victory/defeat overlay; synthesized SFX and two ambient loops that
  crossfade between menus and matches.
- **Menus**: boot splash with engine status and retry, main menu, deck select with deck tiles
  (commander art), AI opponent picker (or Random), deck import (paste a list, see unresolved lines and
  suggestions, save), settings, VS loading screen, game-over screen with rematch.
- **Both formats**, selectable per game; decks come from the engine (starter decks, precons, your imports).

## Setup

Requirements: Rust (stable, edition 2024), a C toolchain/ALSA/udev headers as for any Bevy app,
JDK 17 + Maven for the engine, ~1 GB disk for Forge.

```bash
# 1. JDK 17 + Maven without root (installs to ~/.local/opt); skip if you already have them
tools/install-toolchain.sh
source tools/env.sh

# 2. Forge: clone the pinned tag into ./forge and install its modules into ~/.m2 ...
tools/setup-forge.sh
#    ... or reuse an existing checkout instead:  ln -s /path/to/forge forge

# 3. Build the engine server
(cd engine && mvn -q -B package)          # -> engine/target/forge-ws-server.jar

# 4. Run (the app autolaunches the engine jar on port 8080 if nothing answers /api/health)
./run.sh                                   # release build, rebuilds the engine jar if stale
# or:
cargo run --release -p arena
```

Useful flags (`cargo run -p arena -- --help`):

| flag | meaning |
|---|---|
| `--quick` | skip the menus, straight into a match with random decks |
| `--format constructed\|commander`, `--deck REF`, `--ai-deck REF`, `--seed N` | choose the match |
| `--engine http://127.0.0.1:8080`, `--no-autolaunch` | use an engine that is already running |
| `--autoplay` | let the built-in policy play your seat (demos, screenshots) |
| `--screenshot out.png --exit-after SECS` | save a frame and quit |
| `--record f.jsonl` / `--replay f.jsonl` | record / replay engine frames, no engine needed |

Dev aids: `ARENA_MENU_SCREEN=deck|commander|import|settings|play` jumps to a menu screen (or presses PLAY)
and `ARENA_FX_DEMO=<secs>` fires one of every effect, for unattended screenshots.

Card images are fetched at runtime through the engine's Scryfall proxy and cached on disk, so the
first match needs network access. `cargo run -p arena --features dev` enables dynamic linking for
faster rebuilds.

Settings are stored in `~/.config/mtg-arena/settings.json` (engine URL, volumes, animation speed,
battlefield theme, fullscreen, player name, last decks).

## Controls

| input | action |
|---|---|
| Left click | select / play a card, use the big orange button |
| Hover a card | large preview on the right |
| `Space` / `Enter` | primary button (Next / Pass / Attack / Done) |
| `Ctrl+Space`, `F` | pass to the next turn / skip |
| `Z` | undo (when the engine allows it) |
| `A` | attack with everything (when offered) |
| `L` | toggle the game log |
| `Esc` | close preview / panels / cancel |
| `F8` | toggle battlefield card name plates |
| `F10` | settings overlay (any screen) |
| `F12` | save a screenshot to `screenshots/` |

## Architecture

```
crates/
  mtg_protocol/   serde types for every engine frame (no Bevy)
  mtg_client/     WS link thread, REST, engine launcher, recorder, autopilot bot (no Bevy)
  arena/          the Bevy app (bin `mtg-arena`), one Plugin per module:
    app_state net view images cards layout motion table hand_camera   board core
    interact hud prompts targeting combat stack                       interaction & HUD
    menus settings style fx audio card_material                       meta & polish
    shared.rs     the cross-plugin contract (resources / messages / marker components)
    assets/       fonts, textures, shaders, sfx (+ assets/tools: Python generators)
engine/           forge-ws-server (Java 17, Jetty) — Forge + format/art-crop/event extensions
decks/            starter decks
tools/            toolchain / Forge setup scripts
```

Plugins talk only through `crates/arena/src/shared.rs`. `view` diffs engine `state` frames into
`BoardEvent`s which `fx`, `audio` and `motion` consume. See [PLAN.md](PLAN.md) for the full design
and [CLAUDE.md](CLAUDE.md) for working notes.

### Regenerating assets

Fonts are OFL (Cinzel, Oswald, Inter; licences next to the files). Textures, icons and sounds are
procedural and reproducible:

```bash
python3 crates/arena/assets/tools/gen_textures.py   # card back, playmats, mana/keyword icons (numpy + Pillow)
python3 crates/arena/assets/tools/gen_sfx.py        # sound effects and ambient loops (numpy + ffmpeg)
```

## Licensing

This project is licensed under the **GNU General Public License v3.0** (see [LICENSE](LICENSE)),
because the engine in `engine/` links against Forge, which is GPL-3. The Bevy client communicates
with it over a socket, but the repository as a whole is distributed under the GPL.

Magic: The Gathering, its card names, art and images are © Wizards of the Coast. This is an
unofficial fan project, not affiliated with or endorsed by Wizards of the Coast, and is intended to
fall under the Fan Content Policy. **No card images are included or redistributed**: they are
fetched at runtime from [Scryfall](https://scryfall.com) for personal use. "MTG Arena" is used here
only as a descriptive reference to the interface style that inspired this client.
