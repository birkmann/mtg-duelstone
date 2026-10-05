<div align="center">

# MTG Arena · Rust + Bevy

**A desktop Magic: The Gathering client that looks and plays like *MTG Arena*:<br>
a tilted 3D table, art-framed cards, targeting arrows and the full rules engine behind it.**

Play 1v1 against the Forge AI in **60-card Constructed** or **Commander**.

![Rust](https://img.shields.io/badge/Rust-2024-b7410e?logo=rust)
![Bevy](https://img.shields.io/badge/Bevy-0.19-232326)
![Engine](https://img.shields.io/badge/rules-Forge-6b4f2a)
![License](https://img.shields.io/badge/license-GPL--3.0-blue)

![A Constructed match in progress](docs/screenshots/match.png)

</div>

---

## Gallery

| | |
|:---:|:---:|
| ![Main menu](docs/screenshots/menu.png) | ![Deck selection](docs/screenshots/decks.png) |
| **Main menu.** Boot splash, then Play / Decks / Settings | **Choose your deck.** Constructed ↔ Commander, AI opponent or Random, deck import |
| ![Opening hand](docs/screenshots/mulligan.png) | ![Hover preview](docs/screenshots/hover.png) |
| **Opening hand.** Keep or mulligan | **Hover a card.** Large preview with rules text on the right edge |
| ![Combat](docs/screenshots/combat.png) | ![Targeting](docs/screenshots/targeting.png) |
| **Combat.** Attackers push forward, blocker ribbons | **The stack.** Spells fan out right, arrows show their targets |
| ![Commander](docs/screenshots/commander.png) | ![Combat damage](docs/screenshots/damage.png) |
| **Commander.** 40 life, command zone, commander damage | **Prompts.** Every engine question has a dialog, e.g. damage assignment |

<p align="center"><img src="docs/screenshots/gameover.png" width="70%" alt="Game over screen"><br><b>Victory / Defeat</b> with Rematch</p>

## Features

- **Full Magic rules.** The [Forge](https://github.com/Card-Forge/forge) engine (~33 000 scripted cards) runs
  headless and decides everything: priority, the stack, combat, triggers, layers, mulligans and the AI. The
  client never computes legality. It renders what the engine says and answers its questions.
- **Arena-style table.** Perspective camera over a themed playmat (Sanctum, Jungle, Storm), fanned hand on its
  own overlay camera, lands and tokens grouped into "x4" stacks, piles with counters, a stack column and a big
  hover preview.
- **Art-framed battlefield cards.** A custom WGSL material puts the art crop in a frame coloured by the card's
  colours, with name bars, P/T plates (buffs blue, damage red), loyalty and counter badges, and glow rims for
  playable, selected and targetable cards.
- **Readable interaction.** Glowing bezier targeting arrows, attack and block ribbons, the Arena orange action
  button ("Next / To Combat", "Attack with 3", "No Blocks"…), phase track with stop toggles, auto-yield per
  stack item, a game log and undo.
- **Every prompt kind.** Confirm, options, ability choice, card trays, ordering triggers, combat damage
  assignment, distribute amounts, scry/surveil, X values and the mulligan screen.
- **Feel.** Spring-damped card motion, floating damage numbers, hit shake, spark bursts, land-drop dust, turn
  banners, victory confetti, synthesized sound effects and ambient music that crossfades between menus and
  matches.
- **Decks.** Four Constructed and two Commander starter decks, Forge's precon and theme decks, and an importer
  for pasted lists (Arena/MTGO/Moxfield formats) that shows unresolved names with suggestions.

## Quick start

You need Rust (edition 2024), the usual Bevy system libraries (ALSA, udev, Vulkan), JDK 17 + Maven for the
engine and about 1 GB of disk for Forge.

```bash
tools/install-toolchain.sh        # JDK 17 + Maven into ~/.local/opt (skip if you have them)
source tools/env.sh
tools/setup-forge.sh              # clone Forge (pinned tag) into ./forge and install it to ~/.m2
                                  #   or reuse a checkout:  ln -s /path/to/forge forge
./run.sh                          # builds the engine jar if stale, then runs the game
```

The game starts the engine for you (port 8080) and stops it when you quit. Card images are fetched through the
engine's Scryfall proxy on first use and cached on disk, so the first match needs network access.

Skip the menus and jump straight into a game:

```bash
./run.sh --quick --format constructed --deck starter:gruul-dinosaurs --ai-deck random
./run.sh --quick --format commander --deck starter:lathril-elves
```

<details>
<summary><b>All command-line flags</b></summary>

| flag | meaning |
|---|---|
| `--quick` | skip the menus, straight into a match (uses your last format; random decks if none given) |
| `--format constructed\|commander` | game format |
| `--deck REF`, `--ai-deck REF` | decks as listed by the engine (`starter:…`, `theme:…`, `precon:…`, `user:…`, `random`) |
| `--seed N` | engine RNG seed |
| `--engine URL`, `--no-autolaunch` | use an engine that is already running |
| `--autoplay` | a built-in bot plays your seat (demos, screenshots) |
| `--screenshot out.png --exit-after SECS` | save a frame and quit |
| `--record f.jsonl`, `--replay f.jsonl` | record engine frames / replay them with no engine |

`cargo run -p arena --features dev` enables Bevy dynamic linking for fast rebuilds.
</details>

## Controls

| input | action |
|---|---|
| **Click** a card | play it, activate it, or pick it as a target (the engine decides what's legal) |
| **Drag** a hand card onto the table | play it |
| **Hover** / **right-click** | large preview / inspector |
| `Space` / `Enter` | the orange action button (Next, Pass, Attack, Done) |
| `Ctrl+Space`, `F` | pass until something happens |
| `A` | attack with everything |
| `Z` | undo, where the engine allows it |
| `L` | game log |
| `Esc` | close preview or menu; cancel |
| `F8` / `F10` / `F12` | name plates / settings / screenshot |

## How it works

```
┌─────────────────────────── Bevy app (crates/arena) ───────────────────────────┐
│ net → view ──diff──▶ BoardEvent ──▶ fx · audio · motion                       │
│        │ GameView / CurrentInput / PendingPrompt                              │
│        ▼                                                                      │
│ layout (pure fns) → TargetPlacement → motion (springs) → cards on 2 cameras   │
│ interact · hud · prompts · targeting · combat · stack ──▶ SendCmd ──▶ net     │
└──────────────────────────────┬────────────────────────────────────────────────┘
               mtg_client: WS thread + REST + launcher   (mtg_protocol: serde types)
                               │  ws://…/ws/match/{id}   JSON frames: state · input · prompt · events
┌──────────────────────────────▼────────────────────────────────────────────────┐
│ engine/  forge-ws-server (Java 17, Jetty) ─ Forge rules engine + Forge AI      │
└───────────────────────────────────────────────────────────────────────────────┘
```

- The engine sends a full `state` snapshot (hidden information redacted), an `input` describing what it's
  waiting for, blocking `prompt`s answered by index, and `events` animation cues. See
  [engine/docs/protocol.md](engine/docs/protocol.md).
- `view` diffs consecutive snapshots into `BoardEvent`s (card moved, damaged, life changed…), which drive
  animation, effects and sound.
- `layout` is a set of pure, unit-tested functions from game state to card poses. `motion` springs every card
  toward its pose, so drawing, casting, resolving and dying are all just "the target moved".
- Plugins only talk through [`crates/arena/src/shared.rs`](crates/arena/src/shared.rs), so each module can be
  read and changed on its own.

```
crates/mtg_protocol   serde types for every frame (no Bevy)
crates/mtg_client     WS link, REST, engine launcher, recorder/replayer, autoplay policy, autopilot bot
crates/arena          the game: one Plugin per module + assets (fonts, textures, shaders, sfx)
engine/               forge-ws-server: Forge + format selection, art crops, event cues
decks/                starter decks
```

## Development

```bash
cargo test --workspace                                                   # protocol, client, layout, view, prompts
cargo run -p mtg_client --bin autopilot -- --launch --format constructed --games 10   # headless soak vs the AI
cargo run -p arena -- --replay crates/arena/tests/replays/commander_mid.jsonl --engine http://127.0.0.1:8080
python3 crates/arena/assets/tools/gen_textures.py                        # regenerate card back, playmats, icons
python3 crates/arena/assets/tools/gen_sfx.py                             # regenerate sound effects and music
```

See [PLAN.md](PLAN.md) for the design and roadmap and [CLAUDE.md](CLAUDE.md) for working notes.

## License & legal

This project is licensed under the **GNU GPL v3.0** (see [LICENSE](LICENSE)) because `engine/` links against
Forge, which is GPL-3. Fonts are OFL (Cinzel, Oswald, Inter); textures, icons and sounds are generated by the
scripts in `crates/arena/assets/tools/`.

Magic: The Gathering, its card names, art and images are © Wizards of the Coast. This is an unofficial fan
project, not affiliated with or endorsed by Wizards of the Coast, and is intended to fall under the
[Fan Content Policy](https://company.wizards.com/en/legal/fancontentpolicy). **No card images are included or
redistributed**: they are fetched at runtime from [Scryfall](https://scryfall.com). "MTG Arena" refers only to
the interface style that inspired this client.
