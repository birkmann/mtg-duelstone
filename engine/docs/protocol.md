# Engine protocol (WebSocket, JSON)

Server: `forge-ws-server` (Java, wraps Forge). One match = one WebSocket endpoint `ws://host/ws/match/{id}`.
Any number of sockets may attach to a match (reconnects/tabs); every one gets a full snapshot on connect.

Multiplayer (up to 4 seats, humans and/or Forge AI): a **lobby** match is opened with `POST /api/lobby`; every human
seat has a secret **token** and its sockets connect with `ws://host/ws/match/{id}?token={token}`. Each human seat is
its own Forge GUI, so `state`/`input`/`prompt`/`log`/`events` frames are per seat (hidden info redacted for *that*
player) and only reach that seat's sockets. A socket without a token is accepted only by classic (`POST /api/match`)
matches, where it is the human. See [Multiplayer](#multiplayer).

Design rule: **the client never computes legality**. Cards are clickable when the engine says so
(`input.selectable`, or the engine accepts/rejects a click and answers with `flash`), and blocking
decisions arrive as `prompt` frames listing the legal options; the client answers by index.

## REST

| Method | Path | Body / params | Returns |
|---|---|---|---|
| GET | `/api/health` | | `{ok, cards, protocol: 2, publicUrl, web, maxPlayers}` — `publicUrl` = base other machines reach this engine at (invite links: `--public-url`, `$ARENA_PUBLIC_URL`, else the first LAN IPv4); `web` = the browser client is served at `/` |
| GET | `/api/decks` | `?format=constructed\|commander` (default `commander`), `?kind=user` | `[ {ref, name, commanders[], mainCount, colorIdentity, format, problem} ]` — `ref` = `starter:<name>` (repo `decks/<format>/*.dck`) / `precon:<name>` (commander) / `theme:<name>` (Forge's bundled 60-card constructed decks) / `user:<name>` / `file:<path>`; `problem` is null for legal decks (Forge `DeckFormat` check). The special ref `random` (in `POST /api/match`) picks a legal deck of the match format (starters favoured). |
| GET | `/api/deck`, `/api/deck/export` | `?ref=…&format=?` | full deck / `.dck` text |
| POST | `/api/deck`, `/api/deck/validate`, `/api/deck/import` | body gains `format?` (default commander); constructed decks save to `~/.forge/decks/constructed` and need no `commanders` | as before |
| DELETE | `/api/deck` | `?name=…&format=?` | |
| GET | `/api/img` | `?name=&set?&back?&version=normal\|large\|art_crop\|png` (default `normal`; `small`/`border_crop` also accepted) | the cached Scryfall image; version is part of the cache key (`png` is served as `image/png`) |
| GET | `/api/matches` | | `[ matchInfo ]` |
| POST | `/api/match` | `{humanDeck, aiDeck, format?, seed?, humanName?}` — `format` is `"constructed"` (60-card main deck, 20 life, `GameType.Constructed`) or `"commander"` (default; 100 cards, 40 life) | `matchInfo` (`{id, humanDeck, aiDeck, humanDeckName, aiDeckName, format, started, over, sockets, turn, gameOver}`) |
| GET | `/api/match/{id}` | | `matchInfo` |
| DELETE | `/api/match/{id}` | | |
| GET | `/api/engine/cards?names=a|b|c` | | `{name: bool}` — can Forge play this card? |
| POST | `/api/lobby` | `{format, players: 2..4, name, deck}` | **seat ticket** for the host (seat 0): `{id, seat: 0, token, name, invite, …lobby}` — `id` is the 6-letter table code, `invite` = `publicUrl + "/?join=" + id` |
| GET | `/api/lobby/{code}` | | lobby info (the `lobby` frame body without `you`); code is case-insensitive; 404 = no such table |
| POST | `/api/lobby/{code}/join` | `{name, deck}` | seat ticket `{id, seat, token, name, format}`; 409 when full / started. Names are made unique at the table; `file:` deck refs are refused |

## Server → client frames

All frames: `{"t": <type>, ...}`.

### `hello`
`{t:"hello", matchId, humanDeck, aiDeck, humanDeckName, aiDeckName, format: "constructed"|"commander", protocol: 2}` — first frame after connect; followed by a snapshot (`state`, `input`, `log`, and a pending `prompt` if any).

### `state` — full game view (hidden info redacted for the human seat)
```jsonc
{ "t":"state", "seq": 123, "you": <playerId>,
  "stops": { "myTurn": ["MAIN1", ...], "oppTurn": [...] },      // phases where the engine stops for you
  "game": {
    "turn": 5, "phase": "MAIN1" | "COMBAT_DECLARE_ATTACKERS" | ..., "playerTurn": <playerId>,
    "isCommander": true, "gameOver": false, "matchOver": false, "stormCount": 0, "winner"?: "name",
    "players": [ {
        "id", "name", "isAI", "isViewer", "life", "hasPriority", "hasLost",
        "maxHandSize", "unlimitedHandSize", "landsPlayedThisTurn", "maxLandPlays", "isExtraTurn",
        "counters": { "POISON": 2 }, "manaPool": { "W":0,"U":0,"B":0,"R":0,"G":0,"C":0 },
        "commanders": [cardId], "commanderCast": { "<cardId>": n }, "commanderDamage": { "<cardId>": n },
        "zones": { "battlefield": [card], "hand": [card], "graveyard": [card], "exile": [card], "command": [card], "library": [card] }
    } ],
    "stack": [ { "id", "key", "text", "isAbility", "isTrigger", "isOptionalTrigger", "source": card, "activator": playerId, "targetCards": [ids], "targetPlayers": [ids], "sub"?: stackItem } ],  // top first
    "combat": null | { "attackers": [ { "attacker": cardId, "defender": {type:"player"|"card", id, name}, "blockers": [ids], "plannedBlockers": [ids] } ] }
  } }
```
`card` (hidden = card back): `{ "id", "zone", "owner", "controller", "hidden": true, "faceDown" }`.
`card` (visible): `{ "id", "zone", "owner", "controller", "hidden": false, "name", "oracleName", "tapped", "sick", "attacking", "blocking",
"faceDown", "flipped", "phasedOut", "token", "commander", "damage", "assignedDamage", "shields", "counters": {name:n},
"attachedTo": id|null, "attachedCards": [ids], "chosenType"?, "chosenColors"?, "namedCard"?, "text"?,
"state": { "name", "stateName", "type": "Legendary Creature — Elf Druid", "manaCost": "{2}{G}", "colors": "WG", "oracleText", "rulesText", "setCode", "imageKey",
"power"?, "toughness"?, "loyalty"?, "defense"?, "isLand", "isCreature", "isArtifact", "isEnchantment", "isPlaneswalker", "isInstant", "isSorcery", "keywords": [] },
"altState"?: {...} }`.
Ids are the engine's trackable ids — stable for the whole game; use them as React keys and in clicks/answers.

### `input` — what the engine's current Input wants (non-blocking; act with `click` frames)
```jsonc
{ "t":"input", "seq": 42,            // changes whenever a NEW engine Input becomes active (same content ≠ same prompt!)
  "message": "Priority: You\nTurn: 3 (You)\nPhase: Main phase, precombat\nStack: Empty",
  "card": cardId|null,                 // card the prompt is about (e.g. targeting source)
  "ok":     { "label": "OK", "enabled": true },        // OK / Keep / Auto / Accept / Yes ...
  "cancel": { "label": "End Turn", "enabled": true },  // Cancel / Mulligan / Decline / Undo ...
  "selectable": [cardIds], "highlighted": [cardIds], "weak": [cardIds],   // clickable cards; highlighted = currently picked; weak = e.g. lands Auto would tap
  "selectMin": 0, "selectMax": 0,
  "selectablePlayers": [playerIds],    // players that accept a click (targets)
  "trayCards": [card, ...] }           // selectable cards the table has nowhere to draw: library digs / tutors, graveyard & exile
                                       // picks, a revealed opponent's hand, cards in no zone. Full card views (the engine has granted
                                       // a temporary look), engine order; render them in a pick tray and click them like any card.
```
Semantics by situation (labels are Forge's): priority → OK passes, Cancel = auto-pass to end of turn; mulligan → Keep/Mulligan;
mana payment → Auto/Cancel and lands are `weak`; declare attackers → click your creatures (or `alphaStrike`), OK when done;
declare blockers → click blocker then attacker; discard/select → `selectable` with `selectMin..selectMax`.
During priority, **clicking any card in hand or on the battlefield is how you play/activate it** — the engine decides legality
(rejects with `flash`) and, if the card has several abilities, asks with a `chooseAbility` prompt.

### `prompt` — blocking dialog; answer with `{t:"answer", id, value}`
Common fields: `id`, `kind`, `title?`, `message?`. Options are `{ i, type, label, ... }` where `type` ∈ `card` (+`id`,`card`), `player` (+`id`), `ability` (+`id`,`ability`), `stackItem`, `entity`, `number` (+`value`), `color` (+`value`), `text`.

| kind | extra fields | answer `value` |
|---|---|---|
| `chooseAbility` | `hostCard`, `options` | option index, or null to cancel |
| `confirm` | `card?`, `yes`, `no`, `defaultYes` | `true` / `false` |
| `option` | `options: [string]`, `default` | index |
| `text` | `initial`, `options`, `numeric` | string |
| `choices` | `min`, `max`, `options`, `selected: [i]` (min/max < 0 = informational list) | `[i, ...]` |
| `chooseEntity` | `min`, `max`, `options`, `reveal?: {cards, zones, owner, message}` | `[i, ...]` (empty = none) |
| `order` | `top`, `remainMin`, `remainMax`, `options` (source first `sourceCount`, then dest), `refCard`, `showRemember` | `{ order: [i...], remember: bool }` — indices of everything moved to the ordered/dest list |
| `assignCombatDamage` | `attacker`, `blockers: [card]`, `damage`, `defender`, `overrideOrder`, `maySkip` | `{ "<blockerCardId>": n, "defender"?: n }` summing to `damage`; null only when `maySkip` = "assign another attacker first" (the engine re-asks later — never answer null for every attacker) |
| `assignAmount` | `source`, `amount`, `atLeastOne`, `label`, `options` (+`current`) | `{ "<i>": n }` |
| `manipulateCards` | `cards`, `manipulable: [ids]`, `toTop`, `toBottom`, `toAnywhere` | `[cardId...]` new order |

### `events` — animation cues (protocol v2)
`{t:"events", seq, items:[ cue ]}` — derived from Forge's game event bus (the same events its desktop GUI animates from). `seq` counts events frames.
One frame per EDT flush; it is sent **before** the `state` frame that reflects the changes, so a client can start an animation and
then see the new state. Ids are the same trackable card/player ids as in `state`. Unknown kinds must be ignored. Not part of a
`resync` snapshot (cues are transient). A cue is `{kind, card?, source?, targetCard?, targetPlayer?, amount?, zoneFrom?, zoneTo?, label?}`:

| kind | fields |
|---|---|
| `damage` | `source` → `targetCard` or `targetPlayer`, `amount`; `label:"combat"` for combat damage to a player |
| `lifeChanged` | `targetPlayer`, `amount` = signed delta |
| `zoneChange` | `card`, `zoneFrom?`, `zoneTo?` (Forge `ZoneType` names: `Library`, `Hand`, `Battlefield`, `Graveyard`, `Exile`, `Stack`, `Command`…; from absent for newly created cards) |
| `spellCast` | `card` (source of the spell/ability on the stack), `label` = `spell`/`ability`/`trigger`, `targetCard`/`targetPlayer` = first target if any |
| `attack` | `card` → `targetPlayer` or `targetCard` (planeswalker/battle); one cue per attacker |
| `block` | `card` (blocker) → `targetCard` (attacker); unblocked attackers produce no cue |
| `counters` | `card`, `amount` = signed delta, `label` = counter type |
| `tap` / `untap` | `card` |
| `tokenCreated` | `card` (token entering the battlefield; also preceded by its `zoneChange`) |
| `destroyed` / `sacrificed` | `card` (battlefield → graveyard; derived from the zone change, `sacrificed` when Forge announced a sacrifice; tokens excluded) |
| `turnBegan` | `targetPlayer`, `amount` = turn number |

### `lobby` — a multiplayer table before the start (multiplayer)
`{t:"lobby", id, format, maxPlayers, host: 0, started, invite, you: <your seat>, canStart, seats: [ {seat, kind: "human"|"ai"|"open", name, deck, deckName, connected, you} ]}`
— sent to every socket of the table on every change (join, seat edit, a player's socket opening/closing). `canStart` =
≥ 2 filled seats and every seated human connected. When the host starts, each socket receives `hello` (with `seat`
and `players`) followed by its seat's normal snapshot; open seats are left out of the game.

### Others
`{t:"log", entries:[{i, type, text}]}` (incremental) · `{t:"message", title, text, error}` · `{t:"gameOver", winner, youWon}` ·
`{t:"flash"}` (rejected action) · `{t:"alert"}` (your attention needed) · `{t:"error", text}` · `{t:"pong"}`.

## Client → server frames
- `{t:"click", action:"ok"|"cancel"|"pass"|"alphaStrike"|"undo"}`
- `{t:"click", action:"card", id}` · `{t:"click", action:"player", id}`
- `{t:"answer", id, value}` — reply to a `prompt`
- `{t:"settings", stops: { myTurn?: [PhaseType...], oppTurn?: [PhaseType...] }}` — PhaseType = UNTAP, UPKEEP, DRAW, MAIN1, COMBAT_BEGIN, COMBAT_DECLARE_ATTACKERS, COMBAT_DECLARE_BLOCKERS, COMBAT_FIRST_STRIKE_DAMAGE, COMBAT_DAMAGE, COMBAT_END, MAIN2, END_OF_TURN, CLEANUP
- `{t:"concede"}` — takes effect at your seat's next input (conceding while rules resolve would race Forge's game thread); at a multiplayer table the game goes on without you
- `{t:"resync"}` (re-send snapshot; in a lobby: the `lobby` frame) · `{t:"ping"}`
- Lobby only: `{t:"lobbyDeck", deck}` (change your deck) · host: `{t:"lobbySeat", seat, kind: "ai"|"open", deck?}` (fill a seat with Forge AI, or reopen it — removes a human, whose token stops working) · host: `{t:"start", seed?}`

## Multiplayer

1. Host: `POST /api/lobby` → ticket; connect with its token; share `invite`.
2. Invitee: opens `invite` in a browser (the engine serves the wasm client at `/`, which reads `?join=`), or pastes it /
   the code into the desktop client: `GET /api/lobby/{code}` (format, free seats) → pick a deck → `POST …/join` → connect.
3. Host fills empty seats with AI (`lobbySeat`) or leaves them out, then `start`.
4. Turn order is Forge's; `state.game.players` lists everyone, each client draws its own seat at the bottom.
   A player who is out (`hasLost`) keeps receiving state until `gameOver` (spectating).
5. A seated human whose sockets stay closed for 2 minutes mid-game concedes automatically and their seat answers
   every remaining question with defaults, so the table never waits on someone who closed the tab. Reconnecting
   with the same token within that window resumes the seat (`hello` + snapshot).

## Threading notes (server)
Game thread = Forge's; it blocks inside `prompt`s and inside Inputs. Clicks are applied on the headless "EDT"
(single thread) exactly like Forge's own GUIs do. State frames are coalesced and read snapshot collections,
so they may lag a few ms behind the engine; a prompt is always preceded by a fresh state.
