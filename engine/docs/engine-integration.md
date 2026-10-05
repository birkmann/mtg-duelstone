# Engine integration notes

How `forge-ws-server` drives Forge, and the invariants that keep the WebSocket protocol coherent.
Read this before touching `WsGuiGame`, `MatchSession` or `InputIntrospect`.

## The seam: `IGuiGame`, not `PlayerController`

The plan proposed subclassing `forge.game.player.PlayerController` (110 abstract methods). We don't.
Forge already has a complete human controller — `forge.player.PlayerControllerHuman` — that turns every
engine decision into calls on a GUI interface, `forge.gui.interfaces.IGuiGame`, plus an *input* loop
(`InputQueue` / `InputProxy` / `Input*` classes) that paints prompts and waits for clicks delivered through
`forge.interfaces.IGameController`. Forge's own network play (`forge.gamemodes.net`, see `ProtocolMethod`)
is exactly this split serialized over Netty. So:

- **Server → client** = the ~45 `IGuiGame` calls (`RemoteClientGuiGame` is the reference; ours is `WsGuiGame`).
  ~18 of them block the game thread for an answer (`getChoices`, `confirm`, `order`, `chooseEntitiesForEffect`,
  `assignCombatDamage`, ...) → `prompt` frames answered by index. The rest are display updates → `state`/`input` frames.
- **Client → server** = `IGameController` calls (`selectButtonOk/Cancel`, `selectCard`, `selectPlayer`,
  `passPriority`, `alphaStrike`, `undoLastAction`, `concede`, yield settings) → `click`/`yield` frames.

Everything else — mulligans, priority, targeting, mana payment, attackers/blockers, triggers ordering,
auto-pass, auto-yield, phase stops, undo — is `PlayerControllerHuman`'s existing behaviour. We never
implement game logic; when a new prompt kind appears it is already one of the `IGuiGame` calls above.

`HostedMatch` (Forge's match orchestrator) is reused as-is; `HeadlessGuiBase` implements `IGuiBase`
(assets dir + a single "EDT" thread + no-op images/sounds/dialogs), `Bootstrap` runs `FModel.initialize`.

## Threads

- **Game thread** (Forge's, one per match): runs the rules engine; calls `WsGuiGame` blocking methods and
  parks inside Inputs (`CountDownLatch`) until a click arrives. Serializing views from here is safe.
- **EDT** (our single thread, `HeadlessGuiBase`): Forge's GUI code (`InputProxy.update`, `Input.showMessage`,
  `FControlGameEventHandler.processEvents`) runs here. We dispatch every client click here too
  (`MatchSession.onEdt`), exactly like Swing/libGDX do, so clicks never race each other.
- **Jetty threads**: parse frames; `answer` completes a `CompletableFuture` (`PromptBroker`) that the game thread waits on.
- Trackable view collections are replaced, not mutated (`PlayerView.updateZone` → `set(prop, newCollection)`),
  so reading `GameView` from the EDT for a `state` frame is snapshot-safe.

## Coherence rules (each one fixed a real stall found by the autopilot)

1. **`state` frames are coalesced** via a double hop on the EDT (`scheduleFlush`): Inputs are *constructed*
   on the game thread (`setSelectables` in `InputSelectEntitiesFromList`'s constructor) but *painted* by a
   `showMessage` task already queued on the EDT; hopping twice lets the paint land first.
2. **Selection marks recorded off-EDT don't flush** (`markInputIfPainting`): otherwise a frame pairs fresh
   selectables with the previous Input's message/buttons and a client acts on the wrong prompt.
3. **Every new Input gets a `seq`** and **resets the buttons** (`updateCurrentPlayer` → queued on the EDT ahead
   of that Input's `showMessage`). Two consecutive priority passes can produce byte-identical frames; the
   seq is how a client knows it's a new question.
4. **Buttons are disabled optimistically while a click is dispatched** and restored if the engine neither
   swapped the Input nor repainted it (`paintSeq`) — a rejected card click must not leave the UI dead, and
   an accepted one must not leave stale enabled buttons for the 500 ms until `InputLockUI` paints "Waiting".
5. **Before any blocking prompt, a fresh `state` is pushed** (`ask()`), so the client renders the prompt
   against current zones.
6. **`assignCombatDamage` → null means "assign another attacker first"** and re-queues the attacker; it is not "auto".
7. **Concede only while the game thread is parked on that seat's Input** (`MatchSession.driveConcede`, checked with
   `InputIntrospect.awaiting`). `PlayerControllerHuman.concede()` runs `checkGameOverCondition` → `onPlayerLost` on
   the calling thread; from the EDT while the game thread resolves anything it throws
   `ConcurrentModificationException` (found by the 4-bot multiplayer autopilot). An Input object outlives its
   release, so "the proxy has an input" is not enough — its `finished` flag must still be false.

## Multiplayer seats

`MatchSession` holds up to four `Seat`s. Every human seat owns its own `WsGuiGame` (passed to
`HostedMatch.startMatch(…, Map<RegisteredPlayer, IGuiGame>)`, exactly how Forge's own network play hands one GUI to
each remote human), its own `PromptBroker` and its own event subscriber, so hidden information, prompts, phase stops
and auto-yields are per player for free. Frames from a seat's GUI go only to that seat's sockets (`seatOut`).
The lobby (`lobby` frames, `lobbySeat`/`start`) lives entirely in `MatchSession`; Forge only sees the filled seats
once the host starts. `MatchManager` ticks every 2 s to put abandoned seats on autopilot (rule 7).

## Hidden information

The view layer does not strip hidden cards; GUIs decide via `CardView.canBeShownTo(viewer)`. `ViewJson`
is constructed with the human seat's `PlayerView` and emits card backs for anything the seat may not see
(opponent hand, libraries, face-down). Never serialize `forge.game.card.Card`.

## Known hacks / Forge-coupling

- `InputIntrospect` reflects `InputSelectTargets.sa` / `InputSelectEntitiesFromList.validChoices` to tell the
  client which *players* are legal click targets (Forge GUIs just let you click any avatar and validate).
  Degrades to "all living players" if the fields move. Candidate for an upstream getter.
- Forge's `CardDb`/deck storage are not thread-safe → `DeckSource`/`DeckStore` serialize on a lock.
- Several Forge statics are process-global (`FModel` prefs, `MyRandom`); one match at a time is the tested path.

## Upgrading Forge

Bump `FORGE_TAG` in `tools/setup-forge.sh` and `<forge.version>` in `forge-ws-server/pom.xml`, rebuild,
then run `node tools/ws-autopilot.mjs random random --cards` a dozen times and read `tools/server.log`.
Things most likely to move: `IGuiGame` signatures (compile error → follow `RemoteClientGuiGame`),
`ProtocolMethod` (documents the current surface), the two reflected fields above.
