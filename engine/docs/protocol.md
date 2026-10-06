# Engine protocol (WebSocket, JSON)

Server: `forge-ws-server` (Java, wraps Forge). One match = one WebSocket endpoint `ws://host/ws/match/{id}`.
Any number of sockets may attach to a match (reconnects/tabs); every one gets a full snapshot on connect.

Multiplayer (up to 4 seats, humans and/or Forge AI): a **lobby** match is opened with `POST /api/lobby`; every human
seat has a secret **token** and its sockets connect with `ws://host/ws/match/{id}?token={token}`. Each human seat is
its own Forge GUI, so `state`/`input`/`prompt`/`log`/`events` frames are per seat (hidden info redacted for *that*
player) and only reach that seat's sockets. A socket without a token is accepted only by classic (`POST /api/match`)
matches, where it is the human, and not at all by a [public engine](#public-engine). See [Multiplayer](#multiplayer).

Design rule: **the client never computes legality**. Cards are clickable when the engine says so
(`input.selectable`, or the engine accepts/rejects a click and answers with `flash`), and blocking
decisions arrive as `prompt` frames listing the legal options; the client answers by index.

## REST

| Method | Path | Body / params | Returns |
|---|---|---|---|
| GET | `/api/health` | | `{ok, cards, protocol: 2, publicUrl, web, maxPlayers}` — `publicUrl` = base other machines reach this engine at (invite links: `--public-url`, `$MTG_DUELSTONE_PUBLIC_URL`, else the first LAN IPv4); `web` = the browser client is served at `/`; `public` = a [public engine](#public-engine), which also reports `liveMatches` and `maxMatches` |
| GET | `/api/decks` | `?format=constructed\|commander` (default `commander`), `?kind=user` | `[ {ref, name, commanders[], mainCount, colorIdentity, format, problem, cover?} ]` — `cover` = card name whose art stands for the deck (commander, else its signature card; clients may override it); `ref` = `starter:<name>` (repo `decks/<format>/*.dck`) / `precon:<name>` (commander) / `theme:<name>` (Forge's bundled 60-card constructed decks) / `user:<name>` / `file:<path>`; `problem` is null for legal decks (Forge `DeckFormat` check). The special ref `random` (in `POST /api/match`) picks a legal deck of the match format (starters favoured). |
| GET | `/api/deck`, `/api/deck/export` | `?ref=…&format=?` | full deck / `.dck` text |
| POST | `/api/deck`, `/api/deck/validate`, `/api/deck/import` | body gains `format?` (default commander); constructed decks save to `~/.forge/decks/constructed` and need no `commanders` | as before |
| DELETE | `/api/deck` | `?name=…&format=?` | |
| GET | `/api/img` | `?name=&set?&back?&version=normal\|large\|art_crop\|png` (default `normal`; `small`/`border_crop` also accepted) | JPEG from the offline pack (`offline/img`, filled by `tools/fetch-offline.sh`); never goes online. One printing per card, so `set` is ignored; `art_crop` is the art, every other version is the `large` scan. 404 if the pack has no image |
| GET | `/api/rulings` | `?name=` | `{"scryfallUri","rulings":[{"date","comment"}]}` from `offline/data/rulings.json`; an empty list when the card has none |
| GET | `/api/matches` | | `[ matchInfo ]` |
| POST | `/api/match` | `{humanDeck, aiDeck, format?, seed?, humanName?, humanAvatar?}` — `humanAvatar` = your [avatar](#avatars); `format` is `"constructed"` (60-card main deck, 20 life, `GameType.Constructed`) or `"commander"` (default; 100 cards, 40 life) | `matchInfo` (`{id, humanDeck, aiDeck, humanDeckName, aiDeckName, format, started, over, sockets, turn, gameOver}`) plus `token`, the human seat's token (only in this reply); 503 when a public engine is full |
| GET | `/api/match/{id}` | | `matchInfo` |
| DELETE | `/api/match/{id}` | `?token=` (public engine only: required; the seat concedes and the match is dropped once over) | |
| GET | `/api/engine/cards?names=a|b|c` | | `{name: bool}` — can Forge play this card? |
| GET | `/api/cards/search` | `?q=&ci=WUBG&type=&text=&commander=1&cmc=&tag=&sort=name\|popular&offset=0&limit=60` (limit ≤ 200). `cmc` = exact mana value, `7` means 7+. `tag` = one [card tag](#card-tags) key (unknown → 400). `sort` defaults to `name` (prefix matches first) when `q` is non-empty and to `popular` when it is empty | `[ card ]` (see `card` below) |
| GET | `/api/cards/get` | `?name=` | `card` |
| POST | `/api/deck/recommend` | see [Deck-builder recommendations](#deck-builder-recommendations) | `{format, colorIdentity, commanderDecks?, themes[], archetypes[], sections[], unresolved[]}` |
| POST | `/api/lobby` | `{format, players: 2..4, name, deck, avatar?}` (`avatar` = the host's [avatar](#avatars)) | **seat ticket** for the host (seat 0): `{id, seat: 0, token, name, invite, …lobby}` — `id` is the 6-letter table code, `invite` = `publicUrl + "/?join=" + id` |
| GET | `/api/lobby/{code}` | | lobby info (the `lobby` frame body without `you`); code is case-insensitive; 404 = no such table |
| POST | `/api/lobby/{code}/join` | `{name, deck, avatar?}` | seat ticket `{id, seat, token, name, format}`; 409 when full / started. Names are made unique at the table; `file:` deck refs are refused |

## Deck-builder recommendations

Offline, EDHREC-style suggestions built from data Forge ships in `res/deckgendecks/` (loaded once in the
background at server start, ~1 s; a missing file only degrades suggestions to tag + popularity based ones).

### `card` (REST card object)
`{name, manaCost, cmc, type, colors, colorIdentity, oracleText, power?, toughness?, loyalty?, set, cn, rarity,
isLand, isCreature, isBasicLand, canBeCommander, banned, subtypes: ["Elf","Druid"], tags: ["ramp", …], popularity}` —
`popularity` is 0..1 (rank percentile over Commander inclusion and constructed archetype presence, the max of the
two; 0 until the data has loaded).

### Card tags
Heuristic keyword tags from oracle text / type line (`arena.engine.deck.CardTags`); keys are stable.
- Roles: `ramp` (non-land mana / land fetch / extra land drops / Treasures), `draw`, `removal` (targeted), `wipe`,
  `counter`, `tutor` (non-land search), `protection` (grants hexproof/indestructible/shroud, phasing).
- Themes: `counters` (+1/+1, includes all `proliferate`), `proliferate`, `infect` (infect/toxic/poison), `tokens`
  (creature tokens), `sacrifice`, `lifegain`, `graveyard`, `spells`, `artifacts`, `enchantments` (not Auras alone),
  `voltron` (Equipment, own-creature Auras), `landfall`, `blink`, `mill` (opponent), `discard`, `planeswalkers`, `flying`.
- Tribal themes are `tribal:<CreatureType>` (e.g. `tribal:Goblin`): cards with the type or naming it in their text.

### `POST /api/deck/recommend`
```jsonc
{ "format": "commander" | "constructed",          // default commander
  "commanders": ["Atraxa, Praetors' Voice"],        // commander only; partners allowed
  "main": [{"name": "Sol Ring", "qty": 1}],         // or plain strings
  "themes": ["counters", "tribal:Elf"],             // optional: force these themes active
  "colors": "WUBG",                                  // optional colour filter (ignored when commanders are given)
  "limit": 24 }                                      // per section, max 60
```
Colour filter: the commanders' combined identity; else `colors`; else the union identity of the main deck's
non-land cards; else none. Never recommended: cards already in the deck, basic lands, cards outside the filter,
cards Forge can't play, and (commander) banned cards.

```jsonc
{ "format": "commander", "colorIdentity": "WUBG",   // "" = no filter, "C" = colourless only
  "commanderDecks": 9674,                           // commander only: decks behind the data, 0 = commander not in the data
  "themes": [ {"key":"proliferate","label":"Proliferate","count":1,"score":1.0,"active":true}, … ],   // ≤ 8
  "archetypes": [ {"name":"Burn Red Deck","format":"Pauper","similarity":0.366,"decks":2717}, … ], // constructed, ≤ 5
  "sections": [ {"key":"synergy","title":"High Synergy Cards","cards":[
        {"card": card, "inclusion":0.69, "synergy":0.679, "score":0.78, "reason":"69% of decks · +68% synergy"} ]}, … ],
  "unresolved": ["Sol Rign"] }                      // input names that matched no card
```
- Section keys, in order (empty sections are omitted): `synergy`, `top`, `theme:<key>` / `tribal:<Type>` for each
  active theme, `role:ramp`, `role:draw`, `role:removal`, `role:wipe`, `lands`.
- Commander: `inclusion` = share of the commander's decks running the card (partners: max of the two);
  `synergy` = inclusion − the card's mean inclusion over all commanders whose identity could run it. `synergy` ranks
  by synergy, `top` by inclusion, the rest by `score`. A commander missing from the data falls back to popularity.
- Constructed: the deck is matched to LDA archetypes of Standard/Pioneer/Historic/Modern/Pauper/Legacy/Vintage by
  cosine similarity (weighted by archetype size); `inclusion` = share of similar decks running the card,
  `synergy` = inclusion − its presence across all archetypes. `top` = popularity within the colours.
- `score` (0..1): data-backed cards score 0.3–1.0, popularity-only cards 0–0.3.
- `themes`: non-role tags and tribes over-represented in deck + commander (+ the data's top synergy cards) versus
  the whole card pool; `score` is relative to the strongest theme, `count` = deck cards (incl. commanders) carrying it.
  `active` = requested themes, else the top two.

## Server → client frames

All frames: `{"t": <type>, ...}`.

### `hello`
`{t:"hello", matchId, humanDeck, aiDeck, humanDeckName, aiDeckName, format: "constructed"|"commander", protocol: 2}` — first frame after connect; followed by a snapshot (`state`, `input`, `log`, and a pending `prompt` if any).
v3 adds `seat`, `players` and `seats: [ {seat, name, kind: "human"|"ai", avatar?} ]` (every filled seat; see [Avatars](#avatars)).

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
`{t:"lobby", id, format, maxPlayers, host: 0, started, invite, you: <your seat>, canStart, seats: [ {seat, kind: "human"|"ai"|"open", name, deck, deckName, avatar?, connected, you} ]}`
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
- Lobby only: `{t:"lobbyDeck", deck}` (change your deck) · `{t:"lobbyAvatar", avatar}` (change your [avatar](#avatars); null clears it) · host: `{t:"lobbySeat", seat, kind: "ai"|"open", deck?}` (fill a seat with Forge AI, or reopen it — removes a human, whose token stops working) · host: `{t:"start", seed?}`

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

## Public engine

`--public` (or `MTG_DUELSTONE_PUBLIC=1`; cap with `--max-matches N` / `MTG_DUELSTONE_MAX_MATCHES`, default 6) is for
an engine strangers reach, like the website's "Play in your browser" (`packaging/server`). It changes:

- Only classic vs-AI matches. `GET /api/matches`, `/api/lobby*`, `/api/event*` and every write to decks, cubes and the
  collection answer 403 `{error}`; reads (decks, cards, recommendations, images) work as usual.
- `POST /api/match` answers 503 `{error}` while `maxMatches` games are unfinished. The match socket needs the `token`
  from its reply.
- A human whose sockets have all been gone for 2 minutes (or who never connected) concedes, like an abandoned
  multiplayer seat, and a finished match without sockets is dropped after 30 s. `DELETE /api/match/{id}?token=`
  starts that concession without the wait.

## Threading notes (server)
Game thread = Forge's; it blocks inside `prompt`s and inside Inputs. Clicks are applied on the headless "EDT"
(single thread) exactly like Forge's own GUIs do. State frames are coalesced and read snapshot collections,
so they may lag a few ms behind the engine; a prompt is always preceded by a fresh state.

## Protocol v3: formats, variants, limited events, cubes

`hello.protocol` / `/api/health.protocol` = 3. Everything below is additive; v2 clients keep working.

### Game setup (`POST /api/match`, `POST /api/lobby`)
Extra body fields, echoed in `hello`, `lobby` frames and `matchInfo`:
- `format`: `constructed` | `commander` | `brawl` (60-card singleton commander, Standard pool, 25 life) | `oathbreaker`
  (planeswalker + signature spell, both in the deck's Commander section) | `limited` (event tables only).
- `legality` (constructed only): `standard` | `pioneer` | `modern` | `legacy` | `vintage` | `pauper`, null = casual.
  Decks are checked twice — construction (`DeckFormat`: size, copies) and card pool (`GameFormat`: sets, bans,
  restricted list, Pauper's commons rule). An illegal deck ref is refused with 400 and the reason.
- `variants`: any of `planechase` (generated planar decks; roll the planar die by activating the "Planar Dice" card
  in your command zone), `archenemy` (seat 0 is the archenemy: generated scheme deck, 40 life, goes first; everyone
  else is one team), `momir` (generated 60-basics decks + Momir avatar; deck refs are ignored), `teams` (2v2: seats
  alternate teams, exactly 4 players; Forge has no Two-Headed Giant rules, so teammates keep their own life and turns).
  `archenemy` and `teams` exclude each other; `momir` needs `constructed`.
- `GET /api/decks?format=…&legality=…` marks illegal decks via `problem` and always lists the pseudo-deck
  `{ref:"random", generated:true}`: a random legal saved deck, or one Forge generates (Pauper, Brawl, Oathbreaker…).
- `GET /api/formats` → `{formats, legalities, variants}` (`[{id, name, description}]`).

### Avatars
A player's portrait: `{card, x, y, zoom}` — `card` = a card name whose `art_crop` is the picture, `x`/`y` = centre of the
circular crop in the art (0..1 each), `zoom` ≥ 1 (1 = the circle spans the art's shorter side). Clients draw it; the
engine only stores, sanitizes and relays it.
- Set by: `POST /api/match` `humanAvatar`, `POST /api/lobby` / `POST /api/lobby/{code}/join` / `POST /api/event` /
  `POST /api/event/{id}/join` `avatar`, and the lobby frame `{t:"lobbyAvatar", avatar}` (your own seat; rebroadcasts
  `lobby`; after the start → `error`). A seat that becomes AI or open loses its avatar. Event avatars follow the
  player into every table the host deals (`eventMatch`).
- Sanitizing: anything that is not an object with a non-blank string `card` (≤ 200 chars, stripped) counts as "none";
  `x`/`y` clamp to 0..1 (default 0.5 / 0.4 when missing or not a number), `zoom` clamps to 1..4 (default 1). The stored
  object has exactly those four fields. The engine does not check that the card exists.
- At the start every filled seat without one gets a portrait from its deck: the commander (first one), else a
  planeswalker, else a legendary creature (most copies, then highest mana value), else the main deck's highest-mana-value
  creature (then most copies); ties by name, framed `x: 0.5, y: 0.3, zoom: 1.5` (faces sit in the upper middle of most art). A deck with no creature keeps none.
- Shown in: `lobby` frames / `GET /api/lobby/{code}` / `matchInfo` (`seats[].avatar`), `event` frames
  (`seats[].avatar`), and `hello.seats[].avatar` (complete, including derived ones). `avatar` is omitted when there is
  none. Clients map `state.game.players` to avatars by `name`: Forge's player name is the seat name, unique at the table.

### Cubes
| Method | Path | Body / params | Returns |
|---|---|---|---|
| GET | `/api/cubes` | | `[{ref, name, size, user}]` — `bundled:<name>` = Forge's res/draft cubes (MTGO Legacy/Vintage, Pauper, Peasant, Arena…), `user:<name>` = `~/.forge/duelstone-cubes/*.json` |
| GET | `/api/cube` | `?ref=` | `{ref, name, description?, packs, packSize, cards:[catalog card + qty]}` |
| POST | `/api/cube` | `{name, description?, packs?(3), packSize?(15), cards:[{name, set?, qty?}], previousName?}` | the saved cube (+ `size`, `fullPodCards` = cards an 8-player draft needs); bundled names are refused |
| DELETE | `/api/cube` | `?name=` | |
| POST | `/api/cube/import` | `{text}` (one card per line: `1 Name`, `2x Name`, `Name (SET)`) | `{cards:[…], unresolved:[{line, suggestions}]}` |
| GET | `/api/sets` | | draftable sets with boosters, newest first `[{code, name, date}]` |
| GET | `/api/cards/search` | `?q=| GET | `/api/cards/search` | `?q=&limit=` | catalog cards (cube designer search) |limit=` | catalog cards (cube designer search; the full filter set is above) |

### Limited events (cube draft, set draft, sealed)
- `POST /api/event` `{kind: "cubeDraft"|"setDraft"|"sealed", source: <cube ref or set code>, podSize: 2..8, name, seed?, avatar?}`
  → the host's seat ticket `{id, seat: 0, token, invite}`; `invite` = `publicUrl/?event=<id>`.
- `GET /api/event/{id}` → the pod (event frame body without your pack/pool); `POST /api/event/{id}/join {name, avatar?}` →
  seat ticket (409 when started or 4 humans are already in: games seat at most 4).
- Socket: `ws://host/ws/event/{id}?token=…`. Frames:
  - `{t:"event", id, kind, source, sourceName, phase: "lobby"|"drafting"|"building"|"ready", host, you, invite, minDeck: 40,
    round, rounds, pick, passLeft, seats:[{seat, kind, name, avatar?, connected, you, picked, thinking, deckReady}],
    pack:[card + id], pool:[card + id], deck?: LimitedDeck, canStart, games:[{matchId, players, over, winner?}]}` —
    on every change. Cards are catalog cards (`name, manaCost, cmc, type, colors, set, rarity, …`) with an `id`.
  - `{t:"deckSuggestion", deck}` — answer to `suggestDeck` (Forge's sealed deck builder over your pool).
  - `{t:"eventMatch", id, seat, token, name, format:"limited"}` — the host dealt you a game: connect to
    `/ws/match/{id}?token=` (it starts when every seated human is connected; return to the event socket afterwards).
- Client → server: `{t:"eventSeat", seat, kind:"ai"|"open"}` (host, lobby) · `{t:"startEvent"}` (host; open seats become
  AI drafters) · `{t:"pick", id}` · `{t:"suggestDeck"}` · `{t:"submitDeck", deck: {main:[pool ids], basics:{"W":n,…}}}`
  (≥ 40 cards; resubmitting replaces) · `{t:"dealGames", pairing:"duels"|"table", tableSize: 2..4}` (host, once all decks are in;
  duels pair humans, the odd one out plays an AI drafter; table = everyone plus AI drafters up to `tableSize`).
- Drafting: three packs passed left/right/left; every seat picks at its own pace, AI drafters pick as soon as they hold a
  pack (Forge's `LimitedPlayerAI` with the cube's rankings). Pick ids are `packId*1000 + index`; a seat's head pack only
  changes on its own pick. A drafter disconnected for 60 s gets their picks made for them. Sealed: everyone opens
  six packs (cubes: 3–6 depending on the cube).
