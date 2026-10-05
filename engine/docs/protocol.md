# Engine protocol (WebSocket, JSON)

Server: `forge-ws-server` (Java, wraps Forge). One match = one WebSocket endpoint `ws://host/ws/match/{id}`.
Any number of sockets may attach to a match (reconnects/tabs); every one gets a full snapshot on connect.

Design rule: **the client never computes legality**. Cards are clickable when the engine says so
(`input.selectable`, or the engine accepts/rejects a click and answers with `flash`), and blocking
decisions arrive as `prompt` frames listing the legal options; the client answers by index.

## REST

| Method | Path | Body / params | Returns |
|---|---|---|---|
| GET | `/api/health` | | `{ok, cards, protocol: 2}` |
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
| GET | `/api/cards/search` | `?q=&ci=WUBG&type=&text=&commander=1&cmc=&tag=&sort=name\|popular&offset=0&limit=60` (limit ≤ 200). `cmc` = exact mana value, `7` means 7+. `tag` = one [card tag](#card-tags) key (unknown → 400). `sort` defaults to `name` (prefix matches first) when `q` is non-empty and to `popular` when it is empty | `[ card ]` (see `card` below) |
| GET | `/api/cards/get` | `?name=` | `card` |
| POST | `/api/deck/recommend` | see [Deck-builder recommendations](#deck-builder-recommendations) | `{format, colorIdentity, commanderDecks?, themes[], archetypes[], sections[], unresolved[]}` |

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

### Others
`{t:"log", entries:[{i, type, text}]}` (incremental) · `{t:"message", title, text, error}` · `{t:"gameOver", winner, youWon}` ·
`{t:"flash"}` (rejected action) · `{t:"alert"}` (your attention needed) · `{t:"error", text}` · `{t:"pong"}`.

## Client → server frames
- `{t:"click", action:"ok"|"cancel"|"pass"|"alphaStrike"|"undo"}`
- `{t:"click", action:"card", id}` · `{t:"click", action:"player", id}`
- `{t:"answer", id, value}` — reply to a `prompt`
- `{t:"settings", stops: { myTurn?: [PhaseType...], oppTurn?: [PhaseType...] }}` — PhaseType = UNTAP, UPKEEP, DRAW, MAIN1, COMBAT_BEGIN, COMBAT_DECLARE_ATTACKERS, COMBAT_DECLARE_BLOCKERS, COMBAT_FIRST_STRIKE_DAMAGE, COMBAT_DAMAGE, COMBAT_END, MAIN2, END_OF_TURN, CLEANUP
- `{t:"concede"}` · `{t:"resync"}` (re-send snapshot) · `{t:"ping"}`

## Threading notes (server)
Game thread = Forge's; it blocks inside `prompt`s and inside Inputs. Clicks are applied on the headless "EDT"
(single thread) exactly like Forge's own GUIs do. State frames are coalesced and read snapshot collections,
so they may lag a few ms behind the engine; a prompt is always preceded by a fresh state.
