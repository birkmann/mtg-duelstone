# MTG Duelstone (Rust + Bevy) — Implementation Plan

## Context

The goal is a desktop Magic: The Gathering client with a polished 3D table, written in **Rust + Bevy**. It covers 1v1 against an AI opponent in both **60-card Constructed (20 life)** and **1v1 Commander (40 life)**.

Writing a full comprehensive-rules engine is a multi-year effort. The existing `~/Gits/mtg-cmd` repo already solves that problem. It runs **Forge** (GPL-3, about 33k scripted cards, full rules, a competent AI) headless behind a Java WebSocket/JSON server (`forge-ws-server`). That server has a documented protocol (`mtg-cmd/docs/protocol.md`), a Scryfall image proxy, deck import and a soak-tested autopilot. A React client already plays complete games over it.

**Decisions (confirmed with the user):**
- **Rules:** Forge via the existing WS protocol. The Bevy client never computes legality: it renders `state`, highlights what `input.selectable` says, and answers `prompt`s by index.
- **Formats:** Constructed and Commander from the start.
- **Rendering:** a 3D tilted table with a perspective camera. Cards are textured quads; HUD and dialogs use `bevy_ui`.

The client's job is everything the player sees: board layout, card motion, hover previews, targeting arrows, combat presentation, the phase track and action button, dialogs, menus, effects and sound.

---

## 1. Visual target (from `inspiration/`)

Common layout across all eight screenshots, with the camera tilted about 55–60° toward the table:

| Element | Local player (bottom) | Opponent (top, mirrored) |
|---|---|---|
| Avatar + life orb | bottom-centre, above the hand; big life number | top-centre |
| Phase track | small diamonds left/right of the avatar (Main1 · Combat · Main2) | same, around the opponent avatar |
| Creatures row | closest to the table centre | closest to the table centre |
| Lands row | between creatures and hand. Identical lands stack with offset and an **"xN" badge**; tapped lands show a grey tap overlay or rotation | top row |
| Other permanents | artifacts/enchantments to the right of the lands; planeswalkers right of creatures | mirrored |
| Hand | fanned, partly off-screen at the bottom; cost pips above each card; hover lifts and enlarges | card backs at the top edge, or count only |
| Library / graveyard / exile | bottom-left: graveyard top card face-up, library as a card-back pile | top-right |
| Command zone (Commander) | left of the library pile | top-right next to the library |
| Stack | right-middle, enlarged spell cards fanned (newest on top) | — |
| Hover preview | large full card on the right edge | — |
| Action button | bottom-right orange pill: "Next / To Combat", "Pass", "End Turn", "Attack", "No Blocks", "Done"; small secondary button and a fast-forward toggle | — |
| Battlefield cards | **art-crop frames**: coloured name bar, art, P/T plate bottom-right, keyword badges bottom-left, counters as round badges | same |
| Token stacks | identical tokens grouped as "x10" | same |

Effects seen in the shots: targeting/ability arrows (glowing bezier ribbons), selectable cards with a blue glow, attackers pushed toward the centre with "claw" damage marks, red damage numbers floating, impact particles, and themed battlefields (jungle, storm, stone).

---

## 2. Architecture

```
┌──────────────────────── mtg-duelstone (this repo) ───────────────────────────┐
│                                                                              │
│  crates/arena  (Bevy 0.19 app, bin "mtg-duelstone")                          │
│    plugins: net · view · cards · layout · motion · interact · hud ·          │
│             prompts · combat · stack · fx · audio · menus · settings         │
│                 ▲ ServerFrame messages        │ ClientCmd messages            │
│  crates/mtg_client  (no Bevy) WS thread (tungstenite) + REST (ureq),         │
│                     engine process launcher, frame recorder/replayer,        │
│                     bin "autopilot" (headless soak bot)                       │
│                 ▲                                                            │
│  crates/mtg_protocol (no Bevy) serde types for every frame in protocol.md    │
└─────────────────┼────────────────────────────────────────────────────────────┘
                  │  ws://127.0.0.1:8080/ws/match/{id}   + REST /api/*
┌─────────────────▼────────────────────────────────────────────────────────────┐
│ engine/  = forge-ws-server copied from mtg-cmd (Java 17, Jetty, GPL-3)        │
│          + small extensions: `format` (constructed|commander), art_crop       │
│            images, optional `events` frame for animation cues                │
│ forge/   = Forge checkout pinned at forge-2.0.14 (gitignored, via setup script)│
└──────────────────────────────────────────────────────────────────────────────┘
```

**Why copy `forge-ws-server` into `engine/` instead of depending on `mtg-cmd`:**
- It keeps this repo self-contained.
- We need server changes (format selection, art-crop images, event cues).
- The protocol stays backward compatible, so mtg-cmd's React UI could still use it.

**Licensing:** `engine/` links Forge, so it is GPL-3. The Bevy client only talks over a socket. Simplest is GPL-3 for the whole repo. Card images are downloaded once from Scryfall by `tools/fetch-offline.sh` into the gitignored `offline/` folder (per the WotC Fan Content Policy) and never committed. Desktop builds download missing card art on demand into a persistent user cache; a complete offline pack avoids those requests.

### Repo layout

```
mtg-duelstone/
  PLAN.md  README.md  CLAUDE.md  LICENSE  run.sh  .gitignore
  Cargo.toml                       # workspace: resolver = "3", edition 2024
  crates/
    mtg_protocol/  src/{lib,frames,card,game,input,prompt,cmd}.rs  tests/fixtures/*.json
    mtg_client/    src/{lib,ws,rest,launcher,record}.rs  src/bin/autopilot.rs
    arena/         src/main.rs src/lib.rs src/<plugin modules…>  assets/
  engine/          pom.xml src/main/java/arena/engine/…   (from mtg-cmd/forge-ws-server)
  tools/           setup-forge.sh install-toolchain.sh env.sh   (from mtg-cmd/tools)
  decks/           constructed/*.dck  commander/*.dck   (starter decks we ship)
  forge/           (gitignored)
  inspiration/
```

### Cargo setup (matches the eu-ui conventions)

- Bevy is `bevy = "0.19"` with default features (native desktop needs ui, text, pbr, audio, png/jpeg, picking) plus `mesh_picking`. Add an optional `dev` feature that enables `bevy/dynamic_linking` for fast iteration.
- `[profile.dev] opt-level = 1` and `[profile.dev.package."*"] opt-level = 3` (Bevy's recommended setup). Release uses `lto = "thin"`.
- Other crates: `serde`/`serde_json`, `tungstenite` (sync WS on its own thread; no tokio needed), `ureq` (REST and image bytes), `crossbeam-channel`, `directories` (XDG paths), `anyhow`/`thiserror`, `tracing`.
- No tween or UI crates. Motion is a small hand-written smooth-damp, and UI is `bevy_ui`, in line with eu-ui's "hand-roll the small stuff" style.
- Code style follows eu-ui: one `Plugin` unit struct per module, a `//!` doc header per module explaining *why*, and `.chain()`ed systems plus named `SystemSet`s.
- Bevy 0.19 APIs to follow: `Message`/`MessageWriter`/`MessageReader`, observers with `On<Pointer<…>>`, and `MeshPickingPlugin`. Verify signatures on docs.rs when writing code, because they shift between releases.

---

## 3. Engine side (`engine/`)

Copy `mtg-cmd/forge-ws-server` and `mtg-cmd/tools/{setup-forge.sh,install-toolchain.sh,env.sh}`, then make these changes.

1. **Format selection** (required for Constructed).
   - `POST /api/match` accepts `format: "constructed" | "commander"`, defaulting to commander for compatibility.
   - `MatchSession.start` (currently hard-coded at `MatchSession.java:64` to `RegisteredPlayer.forCommander` + `GameType.Commander`) branches. For constructed it builds `RegisteredPlayer` from the deck's main section (normal constructor) and calls `startMatch(GameType.Constructed, …)`.
   - Echo the format in `hello` and in `matchInfo`. `state.game.isCommander` already exists.
2. **Constructed decks.**
   - `GET /api/decks?format=constructed` lists `decks/constructed/*.dck` from this repo, `~/.forge/decks/constructed`, and Forge's bundled constructed precon/theme decks. Find the exact resource directory under `forge/forge-gui/res/` before wiring it.
   - Deck validation uses Forge's `GameFormat` (Standard/Pioneer/casual 60).
   - Reuse `DeckSource`, `DeckStore`, `DeckImporter` and `CardCatalog` unchanged where possible.
3. **Art-crop images.**
   - `ImageCache` gains `version=art_crop|normal|large`, and the cache key includes the version.
   - Battlefield frames need the art crop. The hover preview uses `large`.
4. **Animation cue frame `events`** (phase M5, optional at first).
   - Subscribe to Forge's game event bus (`forge.game.event.GameEvent*`: damage dealt, zone change, spell cast, attackers declared, life changed, counters, tapped). Emit `{t:"events", seq, items:[{kind, source, target, amount}]}`.
   - The client can derive most motion from state diffs, but "who damaged whom" and "which ability fired" need cues.
   - Check the class names against the pinned Forge tag first.
5. **Protocol version.** Add `hello.protocol = 2` and document every addition in `engine/docs/protocol.md`, a copy kept in sync with the Rust types.

Keep all the coherence rules from `mtg-cmd/docs/engine-integration.md` intact: coalesced state, `seq` per input, fresh state before a prompt, and `null` combat damage meaning "assign another attacker first".

---

## 4. Rust crates

### 4.1 `mtg_protocol`

This crate holds serde mirrors of `protocol.md`, ported from `mtg-cmd/web/src/lib/protocol.ts`.

- **Server frames.** `enum ServerFrame` uses `#[serde(tag = "t", rename_all = "camelCase")]` and has variants `Hello`, `State`, `Input`, `Prompt`, `Log`, `Message`, `GameOver`, `Flash`, `Alert`, `Error`, `Pong` and `Events`, plus `#[serde(other)] Unknown` so new server frames never crash the client.
- **Card data.** `Card { id: CardId, zone, owner, controller, hidden, face_down, #[serde(flatten)] details: Option<CardDetails> }` and `CardState { name, type_line, mana_cost, colors, oracle_text, set_code, image_key, power, toughness, loyalty, is_land, is_creature, …, keywords }`.
- **Game state.** `Player`, `Zones`, `StackItem`, `Combat`, `AttackerInfo`, `PhaseType` (13 variants).
- **Input frame.** `InputFrame { seq, message, card, ok: Button, cancel: Button, selectable, highlighted, weak, select_min, select_max, selectable_players, tray_cards }`.
- **Prompts.** `Prompt { id, kind: PromptKind, title, message, … }` covers all 10 kinds: `chooseAbility`, `confirm`, `option`, `text`, `choices`, `chooseEntity`, `order`, `assignCombatDamage`, `assignAmount`, `manipulateCards`, plus `Unknown`. Options are typed as `OptionItem { i, type, label, id?, card?, value? }`.
- **Client commands.** `enum ClientCmd` covers `Click{action, id?}`, `Answer{id, value: serde_json::Value}`, `Settings{stops}`, `Yield{key, auto}`, `ClearYields`, `Concede`, `Resync` and `Ping`.
- **Defaults.** Use `#[serde(default)]` widely; the server omits optional fields.
- **Tests.** Round-trip fixtures recorded from real games, covering every frame and prompt kind. Fixtures come from `mtg_client`'s recorder.

### 4.2 `mtg_client`

This crate has no Bevy dependency, so it is reusable by the autopilot and by tests.

- **`EngineLink`**
  - Spawns a thread that owns a `tungstenite` socket.
  - Talks to the caller over `crossbeam_channel`: `Sender<ClientCmd>` in, `Receiver<LinkEvent>` out, where `LinkEvent` is `Frame(ServerFrame) | Connected | Disconnected(reason)`.
  - Auto-reconnects after 1.5s and sends `resync` on reconnect, as mtg-cmd's store does.
  - Sends a ping every 10s.
- **`Rest`** (ureq):
  - `health()`, `decks(format)`, `create_match{human_deck, ai_deck, format, seed}`, `delete_match`
  - deck import, save and validate (reuse mtg-cmd's existing `ApiServlet` routes; read `EngineServer.java` for the exact paths)
  - `image_bytes(name, set, face, version)`
- **`launcher`**
  - If `/api/health` fails, spawn `java -jar engine/target/forge-ws-server.jar --port N` as a child process.
  - Wait for health (the card DB loads in about 6s) and kill the child on drop.
  - Read the JAR path and port from config or env.
- **`record`**
  - Optionally tee every inbound frame to `~/.local/share/mtg-duelstone/replays/<ts>.jsonl`.
  - A `Replayer` feeds a recording back as `LinkEvent`s. This enables UI development without the engine and deterministic regression tests.
- **`bin/autopilot`** (port of `mtg-cmd/tools/ws-autopilot.mjs`)
  - Plays full games against Forge AI with a "first legal option" or random policy per input and prompt kind.
  - Flags: `--format constructed|commander`, `--games N`, `--seed`.
  - Reports a prompt histogram and any stalls (no new `seq` for more than 30s).
  - This is the main soak test of protocol handling.

### 4.3 `arena` (Bevy app)

#### App states (`app_state.rs`)

`AppState`: `Boot` → `MainMenu` → `DeckSelect` → `Loading` → `InMatch` → `GameOver` → `MainMenu`.
- `Boot`: launch or connect to the engine and show a splash with progress.
- `Loading`: create the match, open the socket, wait for the first `state`, and preload hand and battlefield images.
- Match entities carry `DespawnOnExit(AppState::InMatch)`, or the 0.19 equivalent of state-scoped entities.

#### Plugins (one module each, all gated `run_if(in_state(AppState::InMatch))` where relevant)

| Module | Responsibility |
|---|---|
| `net` | Owns `EngineLink`. Drains `LinkEvent`s each frame into `Message<ServerFrame>`; sends `Message<ClientCmd>` out. Shows a connection-lost banner. |
| `view` | Resources `GameView` (latest `state`), `CurrentInput`, `PendingPrompt`, `GameLog`, `ViewerId`. On each new `state` it **diffs** against the previous one by card id and emits `CardMoved{id, from, to}`, `CardChanged{id}`, `LifeChanged{player, delta}`, `CardTapped`, … messages. Also keeps a `CardCache` (id → last known `Card`) so mid-cast cards are still drawable. |
| `cards` | `CardRegistry: HashMap<CardId, Entity>`. Spawns or despawns card entities as ids appear or vanish. A card is a parent `CardEntity{id}` with front and back child quads: a rounded-rect mesh at the 63:88 ratio, an unlit `StandardMaterial`, and a shared mesh. Components: `CardFace(Handle<Image>)`, `InZone(ZoneKind, PlayerId)`, `Tapped`, `Selectable`, `Highlighted`. |
| `images` | `CardImages` cache keyed by `(name, set, face, version)` → `Handle<Image>`. Requests go to an `IoTaskPool` task that calls `Rest::image_bytes` (the engine serves the offline pack in `offline/img`) and decodes with `Image::from_buffer`. Shows the card back plus a name placeholder until the image loads. Keeps at most 4 concurrent fetches. |
| `layout` | **Pure functions** (unit-tested, no ECS) from a `GameView` snapshot plus viewport to `HashMap<CardId, Placement{translation, rotation, scale, layer}>`. Covers the hand fan, battlefield rows with land/token grouping, piles, stack, command zone, opponent mirroring, and row compression when overflowing. A system writes `TargetPlacement` components. |
| `motion` | Critically damped smooth-damp of `Transform` toward `TargetPlacement` per card. Draw, play, die and tap all fall out of this. Adds an arc lift (y-bump) for zone-to-zone moves. A speed multiplier comes from settings. |
| `interact` | `MeshPickingPlugin` observers: `Pointer<Over/Out>` → hover lift plus `HoverPreview`; `Pointer<Click>` on a `Selectable` card → `ClickCard(id)`; `Pointer<Drag*>` from the hand → the card follows the cursor, and release above the hand line sends `ClickCard`. Right-click or held Alt opens the inspector. Player avatars are pickable for `ClickPlayer`. Hotkeys: Space = OK/Next, Enter = OK, Esc = cancel/close, Z = undo, F = fast-forward toggle. |
| `hud` | `bevy_ui` overlays: avatar and life orbs with damage pulse, mana pool pips, phase track with stop toggles (`settings` frame), main and secondary action buttons (labels from `input.ok`/`input.cancel`), prompt message bar, hover preview panel (large image), pile counters, a "xN" badge for land/token stacks, P/T plates, counters and damage badges. Badges are UI nodes positioned each frame via `Camera::world_to_viewport` from card transforms, the same projected-overlay idea as eu-ui's `markers.rs`. Also a game log drawer and a concede/settings menu. |
| `prompts` | One modal per `PromptKind`. `confirm` → two buttons. `option`/`chooseAbility` → button list. `choices`/`chooseEntity` → **card tray** (a row of large card images with selection, min/max, Done). `order` → drag-reorder list. `assignCombatDamage`/`assignAmount` → per-target ± steppers that must sum to the total (and never send `null` unless "assign later"). `manipulateCards` → reorder tray. `text` → text/number field. Every answer uses the prompt `id`; the modal closes when the prompt clears. `input.trayCards` reuses the tray in non-modal mode. |
| `combat` | From `game.combat`: attackers translate toward the centre line. Blocker → attacker arrows are curved ribbon meshes with an emissive material. Planned blockers show dashed outlines. "Attack all" uses `alphaStrike`. |
| `stack` | Renders stack items enlarged at right-middle, newest on top. Abilities appear as a small "ability card" (source art plus text). Targeting arrows run from stack item to `targetCards`/`targetPlayers`. Auto-yield toggle per item (`yield` frame). |
| `targeting` | While an input has `selectableCards/Players` with a source `input.card`, draws a live arrow from the source to the cursor (a glowing bezier ribbon) and glows valid targets. |
| `fx` | Floating damage and life numbers, hit shake, impact particle bursts (billboard sprites with lifetime), death dissolve (custom WGSL in M7), land-drop dust, turn banner ("Your Turn"). Driven by `view` diff messages and, later, `events` frames. |
| `audio` | `bevy_audio` one-shots for draw, play land, cast, resolve, attack, hit, life loss, turn start, victory/defeat. CC0 sound pack under `assets/sfx/`. Volume in settings. |
| `menus` | Main menu (Play / Decks / Settings / Quit). Deck select has a **Constructed ↔ Commander** toggle, your deck, an AI deck (or random) and a seed. Deck import pastes text → engine importer → shows unresolved lines and suggestions → save. Game-over screen offers Rematch or Menu. |
| `settings` | Persisted with serde to `~/.config/mtg-duelstone/settings.json`: engine URL/port, autolaunch, animation speed, phase stops, volume, fullscreen, battlefield theme. |
| `table` | 3D scene: the table plane with a playmat texture (themeable), border props, directional light plus ambient, a **fixed perspective camera** pitched about 58° with a slight cursor parallax, and the camera rig resizing with the viewport. |
| `hand_camera` | A second **orthographic overlay camera** (higher `order`, own `RenderLayers`) for hand cards and the dragged card. The hand never clips into the table and stays screen-aligned. On drag-out, the card moves to the table layer. |

#### Card visuals (staged)

1. **M2:** every visible card uses Scryfall `normal` images everywhere. Battlefield cards are scaled down, and the hover preview uses `large`.
2. **M7 (card look):** battlefield cards use a custom `ArenaCardMaterial` (WGSL).
   - It composites the `art_crop` texture into a frame whose colour comes from `state.colors` (W/U/B/R/G/multi-gold/colourless/land).
   - It adds a rounded-corner mask, a selectable glow rim (blue), a highlighted rim (yellow), a tapped darken, and a damage-tint uniform.
   - The name bar and P/T plate come from the projected UI overlay.
   - Keyword badges (flying, trample, deathtouch, lifelink, …) come from `state.keywords`, mapped to a small icon atlas.

#### Interaction model (mirrors mtg-cmd's rule: the engine decides)

- **During priority:** clicking any hand or battlefield card sends `click card`. The engine either starts the play (cost or target input follows) or replies `flash`, which triggers a red shake on that card.
- **The action button** always reflects `input.ok`. The secondary button reflects `input.cancel`. Buttons disable optimistically on click until a new `input.seq` or repaint arrives (rule 4 in engine-integration.md).
- **Mana payment:** lands in `input.weak` glow softly; click lands to pay manually, or press "Auto".
- **Declare attackers:** click creatures to toggle them; they slide forward. "Attack All" uses `alphaStrike`; then OK.
- **Declare blockers:** click a blocker, then an attacker; an arrow appears; then OK.

---

## 5. Milestones

Each milestone ends runnable and committed.

### M0 — Bootstrap
- Create the workspace and the three crates.
- Copy the engine and tools from mtg-cmd. Add a `run.sh` that builds the engine jar if stale and runs `cargo run -p arena`.
- Add `.gitignore` (`target/`, `forge/`, `engine/target/`), `CLAUDE.md` (conventions and commands), `README.md` and a GPL-3 `LICENSE`.
- Bevy window shows the table plane, lighting, fixed tilted camera and FPS overlay in debug.

### M1 — Protocol, client, autopilot (headless)
- Engine: add the `format` param and constructed deck listing (§3 items 1–2). Add 2 starter decks per format in `decks/`.
- `mtg_protocol` types plus fixture tests.
- `mtg_client` `EngineLink`, `Rest`, `launcher` and `record`.
- `autopilot` plays 10 seeded games per format to completion with no stalls.

### M2 — Board renders a live game (read-only)
- `net`, `view`, `cards`, `images`, `layout`, `motion` and `table` plugins.
- Start a match from CLI flags (`--format`, `--deck`, `--ai-deck`). The board mirrors state live while Space passes priority.
- Cards animate between zones; tapped cards rotate; lands and tokens group with xN; opponent hand shows card backs.
- `layout` has unit tests for the hand fan, overflowing rows and grouping.

### M3 — Play the game (core interaction)
- Picking with hover lift and the large preview, click/drag-to-play, and the action/secondary buttons with live labels.
- Life orbs, phase track with stop toggles, mana pool, flash feedback, and targeting players via the avatars.
- Selectable, highlighted and weak glows (a simple emissive tint for now).
- **Acceptance:** play a full Constructed game against the AI using only prompts the input frame covers.

### M4 — All prompts
- Modals and the card tray for all 10 prompt kinds, plus `trayCards`, combat damage assignment, scry/surveil (`manipulateCards`), trigger ordering and mulligan.
- **Acceptance:** the autopilot histogram's prompt kinds are each exercised manually in at least one game, and replay fixtures render every modal.

### M5 — Combat & stack presentation
- Attack and block positioning, blocker arrows, the stack column with targeting arrows, the live targeting arrow from source to cursor, and the auto-yield toggle.
- Engine `events` frame (§3 item 4) to drive damage source → target effects.

### M6 — Menus, decks, Commander
- Boot splash with engine autolaunch, main menu, and deck select with the format toggle and AI deck choice.
- Deck import flow, game-over screen with rematch, and the settings screen with persistence.
- Commander specifics:
  - command zone pile
  - commander tax badge (`commanderCast`)
  - commander-damage readout on the opponent avatar (`commanderDamage`)
  - 40 life
- **Acceptance:** one full game of each format started from the menus.

### M7 — Visual polish
- `ArenaCardMaterial` art-crop frames, keyword badge atlas, counters and P/T plates with buff/debuff colouring (blue/red, as in the screenshots).
- Draw, cast, resolve and land-drop animations, death dissolve shader, damage numbers and particles, turn banner, and SFX.
- Fast-forward. Two or three battlefield themes (playmat texture, light colour, props).
- Fonts: OFL fonts (e.g. Cinzel for titles, Inter/Source Sans for UI) in `assets/fonts/`.

### M8 — Hardening & packaging
- Soak with the autopilot (50+ games per format).
- Replay-based regression test: load a `.jsonl`, run the app headless with `MinimalPlugins` and the non-render plugins, and assert there are no panics and entity counts match the zones.
- Performance:
  - LRU-unload images not used in the last N turns
  - mipmaps on card textures
  - one shared mesh per card size, so cards are instanced
- `cargo build --release` plus the engine jar, bundled by `run.sh`.

---

## 6. Risks & mitigations

| Risk | Mitigation |
|---|---|
| Bevy 0.19 API drift vs. remembered APIs | Check docs.rs/bevy 0.19 for picking, observers, Messages and state-scoped entities; compile early and often. eu-ui's `thw_sim` engine is a working 0.19 reference. |
| Forge `GameType.Constructed` start differs from the commander path | M1 autopilot proves it headless before any UI work. |
| Text on 3D cards | Avoided: full card images (M2) and projected `bevy_ui` overlays for dynamic numbers (P/T, counters, xN). |
| Scryfall rate limits / offline | No runtime Scryfall calls: `tools/fetch-offline.sh` fills `offline/` once from the bulk files and CDN, and the engine's `ImageCache` serves only from there. Cards missing from the pack 404 and show their rules text. |
| Prompt kinds we haven't seen | `PromptKind::Unknown` falls back to a generic "list of options" modal; the autopilot histogram reveals gaps. |
| Hand overlay camera + picking across two cameras | bevy_picking supports multiple cameras by `order`. Prototype in M2. Fallback: hand cards as children of the main camera. |
| Java dependency at runtime | `launcher` autostarts the jar; `tools/install-toolchain.sh` installs a user-local JDK 17. |

---

## 7. Verification

- **Unit:** `cargo test --workspace` covers protocol fixture round-trips, `layout` pure functions (hand fan, row compression, land/token grouping, opponent mirroring) and `view` diffing (zone moves, life deltas).
- **Engine:**
  - `cd engine && mvn -q package`, then `curl localhost:8080/api/health`.
  - `curl 'localhost:8080/api/decks?format=constructed'`.
  - `POST /api/match` with `format:"constructed"` returns a match, and `hello` shows the format.
- **Soak:** `cargo run -p mtg_client --bin autopilot -- --format constructed --games 10` and the same with `--format commander`. All games reach `gameOver`, with no stall warnings.
- **Visual and manual (`./run.sh`):**
  - Start a Constructed game from the menu and check the board: layout, hand fan, preview, button.
  - Play lands, cast a creature, attack, block, cast a targeted instant, resolve the stack, reach game over.
  - Repeat for Commander: cast the commander from the command zone, check the tax badge and 40 life.
  - Use the `run` skill or screenshots to confirm visuals at each milestone.
- **Replay:** `cargo run -p arena -- --replay <file>.jsonl` renders a recorded game with no engine running.
