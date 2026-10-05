package arena.engine.server;

import arena.engine.view.ViewJson;
import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import forge.LobbyPlayer;
import forge.deck.CardPool;
import forge.game.GameEntityView;
import forge.game.GameLogEntry;
import forge.game.GameState;
import forge.game.GameView;
import forge.game.card.CardView;
import forge.game.phase.PhaseType;
import forge.game.player.DelayedReveal;
import forge.game.player.IHasIcon;
import forge.game.player.PlayerView;
import forge.game.spellability.SpellAbilityView;
import forge.game.zone.ZoneType;
import forge.gamemodes.match.AbstractGuiGame;
import forge.gui.FThreads;
import forge.interfaces.IGameController;
import forge.item.PaperCard;
import forge.localinstance.skin.FSkinProp;
import forge.player.PlayerZoneUpdate;
import forge.player.PlayerZoneUpdates;
import forge.trackable.TrackableCollection;
import forge.trackable.TrackableTypes;
import forge.util.FSerializableFunction;
import forge.util.ITriggerEvent;

import java.util.ArrayList;
import java.util.Collection;
import java.util.EnumSet;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.function.Consumer;

/**
 * The WebSocket/JSON {@link forge.gui.interfaces.IGuiGame}. The engine talks to it through
 * {@link forge.player.PlayerControllerHuman}; we translate into frames:
 * <ul>
 *   <li>{@code state}  — full game view for the human seat (hidden info redacted), coalesced after
 *       bursts of update calls from the EDT, and always right before a prompt.</li>
 *   <li>{@code input}  — what the current engine Input is asking (message, OK/Cancel labels+enabled,
 *       selectable / highlighted card ids). Non-blocking; the client acts with {@code click} frames
 *       routed to {@link forge.interfaces.IGameController}.</li>
 *   <li>{@code prompt} — a blocking dialog (choices, confirm, order, targets, damage...); the game
 *       thread waits for the matching {@code answer} frame.</li>
 *   <li>{@code log}, {@code message}, {@code gameOver}.</li>
 * </ul>
 */
public class WsGuiGame extends AbstractGuiGame {
    private final Consumer<JsonObject> out;
    private final PromptBroker prompts;
    private volatile PlayerView me;
    private volatile ViewJson vj = new ViewJson(null);

    // phase stops (MTGO-style). Defaults mirror Forge's PHASE_HUMAN_* / PHASE_AI_* preferences.
    private final Set<PhaseType> myStops = EnumSet.of(PhaseType.MAIN1, PhaseType.COMBAT_DECLARE_BLOCKERS, PhaseType.MAIN2);
    private final Set<PhaseType> oppStops = EnumSet.of(PhaseType.COMBAT_BEGIN, PhaseType.COMBAT_DECLARE_ATTACKERS, PhaseType.COMBAT_DECLARE_BLOCKERS, PhaseType.END_OF_TURN);

    // current Input painting
    private volatile String inputMessage = "";
    private volatile Integer inputCard = null;
    private volatile CardView inputCardView = null;
    private volatile String okLabel = "OK", cancelLabel = "Cancel";
    private volatile boolean okEnabled, cancelEnabled;
    /** Selectable cards in engine order (ids → views; views are needed for the pick tray, see {@link #inputFrame}). */
    private final Map<Integer, CardView> selectables = new LinkedHashMap<>();
    private final Set<Integer> highlightedIds = new LinkedHashSet<>();
    private final Set<Integer> weakIds = new LinkedHashSet<>();
    private volatile int selMin, selMax;
    /** Bumped every time a new engine Input becomes active (InputProxy → setCurrentPlayer → updateCurrentPlayer).
     *  Lets clients distinguish "new prompt with identical content" from a repaint of the same prompt. */
    private final java.util.concurrent.atomic.AtomicLong inputSeq = new java.util.concurrent.atomic.AtomicLong();

    private final AtomicBoolean flushScheduled = new AtomicBoolean(false);
    private volatile boolean stateDirty, inputDirty;
    private int logSent = 0;
    private long stateSeq = 0;

    public WsGuiGame(final Consumer<JsonObject> out) {
        this.out = out;
        this.prompts = new PromptBroker(out);
    }

    public PlayerView me() { return me; }
    public PromptBroker prompts() { return prompts; }

    // ---- lookups for incoming client frames -----------------------------------------------

    public CardView cardById(final int id) {
        final GameView gv = getGameView();
        return gv == null || gv.getTracker() == null ? null : gv.getTracker().getObj(TrackableTypes.CardViewType, id);
    }
    public PlayerView playerById(final int id) {
        final GameView gv = getGameView();
        if (gv == null) return null;
        for (final PlayerView p : gv.getPlayers()) if (p.getId() == id) return p;
        return null;
    }

    public void setStops(final boolean myTurn, final Collection<PhaseType> phases) {
        final Set<PhaseType> target = myTurn ? myStops : oppStops;
        synchronized (target) { target.clear(); target.addAll(phases); }
    }
    public JsonObject stopsJson() {
        final JsonObject o = new JsonObject();
        final JsonArray a = new JsonArray(), b = new JsonArray();
        synchronized (myStops) { for (final PhaseType p : myStops) a.add(p.name()); }
        synchronized (oppStops) { for (final PhaseType p : oppStops) b.add(p.name()); }
        o.add("myTurn", a); o.add("oppTurn", b);
        return o;
    }

    // ---- state push / coalescing ----------------------------------------------------------

    private void markState() { stateDirty = true; scheduleFlush(); }
    private void markInput() { inputDirty = true; scheduleFlush(); }
    private void scheduleFlush() {
        // Double-hop: Inputs are constructed on the game thread (setSelectables) but painted by a
        // showMessage task already queued on the EDT; hopping twice lets that paint land first so the
        // client sees one coherent input frame instead of a transient half-painted one.
        if (flushScheduled.compareAndSet(false, true)) FThreads.invokeInEdtLater(() -> FThreads.invokeInEdtLater(this::flush));
    }

    /** Push whatever is dirty. Safe from EDT (snapshot collections) and from the game thread. */
    public synchronized void flush() {
        flushScheduled.set(false);
        if (getGameView() == null) return;
        if (stateDirty) { stateDirty = false; out.accept(stateFrame()); }
        if (inputDirty) { inputDirty = false; out.accept(inputFrame()); }
        final JsonObject log = newLogFrame();
        if (log != null) out.accept(log);
    }

    /** Everything a (re)connecting client needs to render right now. */
    public synchronized List<JsonObject> snapshot() {
        final List<JsonObject> frames = new ArrayList<>();
        if (getGameView() == null) return frames;
        frames.add(stateFrame());
        frames.add(inputFrame());
        final JsonObject log = logFrame(0);
        if (log != null) frames.add(log);
        final JsonObject p = prompts.pending();
        if (p != null) frames.add(p);
        return frames;
    }

    public JsonObject stateFrame() {
        final JsonObject f = Json.frame("state");
        f.addProperty("seq", ++stateSeq);
        f.add("you", ViewJson.refOrNull(me));
        f.add("game", vj.game(getGameView()));
        f.add("stops", stopsJson());
        // keys the player auto-yields to (see YieldController); shown as toggles on stack items
        final JsonArray yields = new JsonArray();
        final IGameController ctl = me == null ? null : getGameController(me);
        if (ctl != null) try { for (final String k : ctl.getYieldController().getAutoYields()) yields.add(k); } catch (final Exception ignored) {}
        f.add("autoYields", yields);
        return f;
    }

    public JsonObject inputFrame() {
        final JsonObject f = Json.frame("input");
        f.addProperty("seq", inputSeq.get());
        f.addProperty("message", inputMessage);
        f.add("card", inputCard == null ? com.google.gson.JsonNull.INSTANCE : new com.google.gson.JsonPrimitive(inputCard));
        // the card the prompt is about (e.g. the spell being cast while paying mana) — it may be in no visible zone right now
        final CardView icv = inputCardView;
        f.add("cardView", icv == null ? com.google.gson.JsonNull.INSTANCE : vj.card(icv));
        final JsonObject ok = new JsonObject(); ok.addProperty("label", okLabel); ok.addProperty("enabled", okEnabled);
        final JsonObject cancel = new JsonObject(); cancel.addProperty("label", cancelLabel); cancel.addProperty("enabled", cancelEnabled);
        f.add("ok", ok); f.add("cancel", cancel);
        // Selectable cards the table has no place to draw — library / graveyard / exile / an opponent's hand / no
        // zone at all (Hideaway digs, tutors, reanimation targets, Thoughtseize...). Forge's desktop GUI opens the
        // zone panel for these; we ship the views (the engine has already granted a temporary look via
        // tempShowCards, so they are not redacted) and the client lays them out in a pick tray by the prompt.
        final JsonArray tray = new JsonArray();
        synchronized (selectables) {
            f.add("selectable", ints(selectables.keySet()));
            for (final CardView c : selectables.values()) if (!drawnOnTable(c)) tray.add(vj.card(c));
        }
        f.add("trayCards", tray);
        synchronized (highlightedIds) { f.add("highlighted", ints(highlightedIds)); }
        synchronized (weakIds) { f.add("weak", ints(weakIds)); }
        f.addProperty("selectMin", selMin);
        f.addProperty("selectMax", selMax);
        final IGameController ctl = me == null ? null : getGameController(me);
        f.add("selectablePlayers", ints(ctl instanceof forge.player.PlayerControllerHuman pch ? InputIntrospect.selectablePlayers(pch) : List.of()));
        return f;
    }

    private JsonObject newLogFrame() {
        final JsonObject f = logFrame(logSent);
        return f;
    }
    private JsonObject logFrame(final int from) {
        final GameView gv = getGameView();
        if (gv == null || gv.getGameLog() == null) return null;
        final List<GameLogEntry> all = gv.getGameLog().getAllEntries();
        if (all.size() <= from) return null;
        final JsonArray a = new JsonArray();
        for (int i = from; i < all.size(); i++) {
            final GameLogEntry e = all.get(i);
            final JsonObject o = new JsonObject();
            o.addProperty("i", i);
            o.addProperty("type", e.type().name());
            o.addProperty("text", e.message());
            a.add(o);
        }
        logSent = all.size();
        final JsonObject f = Json.frame("log");
        f.add("entries", a);
        return f;
    }

    /** Whether the web client renders this card somewhere clickable (battlefield, own hand, commander slot). */
    private boolean drawnOnTable(final CardView c) {
        final ZoneType z = c.getZone();
        if (z == null) return false;
        switch (z) {
            case Battlefield: return true;
            case Command: return c.isCommander();
            case Hand: return me != null && c.getController() != null && c.getController().getId() == me.getId();
            default: return false;
        }
    }

    private static JsonArray ints(final Collection<Integer> ids) {
        final JsonArray a = new JsonArray();
        for (final Integer i : ids) a.add(i);
        return a;
    }

    // ---- lifecycle --------------------------------------------------------------------------

    @Override protected void updateCurrentPlayer(final PlayerView player) {
        // A new engine Input became active (InputProxy.update → setCurrentPlayer). Its showMessage() is
        // queued on the EDT right after this call, so queue our reset ahead of it: buttons are never
        // reported enabled for an Input that has not painted them yet, and the seq identifies the Input.
        FThreads.invokeInEdtLater(() -> {
            okEnabled = false; cancelEnabled = false;
            inputSeq.incrementAndGet();
            markInput();
        });
    }

    @Override public void openView(final TrackableCollection<PlayerView> myPlayers) {
        me = myPlayers.isEmpty() ? null : myPlayers.iterator().next();
        vj = new ViewJson(me);
        markState();
    }

    @Override public void finishGame() {
        flush();
        final GameView gv = getGameView();
        final JsonObject f = Json.frame("gameOver");
        f.addProperty("winner", gv.getWinningPlayerName());
        f.addProperty("youWon", me != null && gv.getWinningPlayerName() != null && gv.getWinningPlayerName().equals(me.getLobbyPlayerName()));
        out.accept(f);
    }

    @Override public void afterGameEnd() { super.afterGameEnd(); markState(); }

    // ---- display updates → state dirty ------------------------------------------------------

    @Override public void updateZones(final Iterable<PlayerZoneUpdate> zonesToUpdate) { markState(); }
    @Override public void updateCards(final Iterable<CardView> cards) { markState(); }
    @Override public void updateStack() { markState(); }
    @Override public void updatePhase(final boolean saveState) { markState(); }
    @Override public void updateTurn(final PlayerView player) { markState(); }
    @Override public void updatePlayerControl() { markState(); }
    @Override public void updateManaPool(final Iterable<PlayerView> manaPoolUpdate) { markState(); }
    @Override public void updateLives(final Iterable<PlayerView> livesUpdate) { markState(); }
    @Override public void updateShards(final Iterable<PlayerView> shardsUpdate) { }
    @Override public void refreshField() { markState(); }
    @Override public void showCombat() { markState(); }
    @Override public void showManaPool(final PlayerView player) { }
    @Override public void hideManaPool(final PlayerView player) { }
    @Override public void setPanelSelection(final CardView hostCard) { }
    @Override public void setCard(final CardView card) { }
    @Override public void setPlayerAvatar(final LobbyPlayer player, final IHasIcon ihi) { }
    @Override public GameState getGamestate() { return null; }
    @Override public void enableOverlay() { }
    @Override public void disableOverlay() { }
    @Override public void alertUser() { out.accept(Json.frame("alert")); }
    @Override public void flashIncorrectAction() { out.accept(Json.frame("flash")); }

    @Override public void message(final String message, final String title) {
        final JsonObject f = Json.frame("message"); f.addProperty("title", title); f.addProperty("text", message); f.addProperty("error", false); out.accept(f);
    }
    @Override public void showErrorDialog(final String message, final String title) {
        final JsonObject f = Json.frame("message"); f.addProperty("title", title); f.addProperty("text", message); f.addProperty("error", true); out.accept(f);
    }

    @Override public Iterable<PlayerZoneUpdate> tempShowZones(final PlayerView controller, final Iterable<PlayerZoneUpdate> zonesToUpdate) { return zonesToUpdate; }
    @Override public void hideZones(final PlayerView controller, final Iterable<PlayerZoneUpdate> zonesToUpdate) { }
    @Override public PlayerZoneUpdates openZones(final PlayerView controller, final Collection<ZoneType> zones, final Map<PlayerView, Object> players, final boolean backupLastZones) { return new PlayerZoneUpdates(); }
    @Override public void restoreOldZones(final PlayerView playerView, final PlayerZoneUpdates playerZoneUpdates) { }

    @Override public boolean isUiSetToSkipPhase(final PlayerView playerTurn, final PhaseType phase) {
        final boolean mine = me != null && playerTurn != null && playerTurn.getId() == me.getId();
        final Set<PhaseType> stops = mine ? myStops : oppStops;
        synchronized (stops) { return !stops.contains(phase); }
    }

    // ---- Input painting → input frame --------------------------------------------------------

    /**
     * Called on the EDT right before a click is dispatched to the engine. A click either finishes the
     * current Input (whose buttons must then read disabled until the next Input paints — Forge's
     * InputLockUI only does that after a 500ms delay) or makes the Input repaint synchronously, which
     * overwrites this. Either way the client never sees stale enabled buttons after clicking.
     */
    public boolean[] consumeButtons() {
        final boolean[] saved = { okEnabled, cancelEnabled };
        okEnabled = false; cancelEnabled = false;
        markInput();
        return saved;
    }
    /** Undo {@link #consumeButtons} when the click turned out to be a no-op (e.g. rejected card). */
    public void restoreButtons(final boolean[] saved) {
        okEnabled = saved[0]; cancelEnabled = saved[1];
        markInput();
    }
    private final java.util.concurrent.atomic.AtomicLong paintSeq = new java.util.concurrent.atomic.AtomicLong();
    /** Incremented on every updateButtons; lets a click handler tell whether the Input repainted. */
    public long paintSeq() { return paintSeq.get(); }

    @Override public void showPromptMessage(final PlayerView playerView, final String message, final CardView card) {
        inputMessage = message == null ? "" : message;
        inputCard = card == null ? null : card.getId();
        inputCardView = card;
        markInput();
    }

    @Override public void updateButtons(final PlayerView owner, final String label1, final String label2, final boolean enable1, final boolean enable2, final boolean focus1) {
        okLabel = label1; cancelLabel = label2; okEnabled = enable1; cancelEnabled = enable2;
        paintSeq.incrementAndGet();
        markInput();
    }

    /**
     * Selection marks are sometimes set from the game thread while an Input is being constructed,
     * i.e. before it is active and painted (InputSelectEntitiesFromList calls setSelectables in its
     * constructor). Emitting a frame at that moment would pair fresh selectables with the previous
     * Input's message/buttons. So: record always, but only flush when on the EDT (a paint is in
     * progress, or a click handler changed highlights); the Input's own showMessage flushes the rest.
     */
    private void markInputIfPainting() { if (FThreads.isGuiThread()) markInput(); }

    @Override public void setSelectables(final Iterable<CardView> cards, final int min, final int max) {
        super.setSelectables(cards, min, max);
        synchronized (selectables) { for (final CardView c : cards) selectables.put(c.getId(), c); }
        selMin = min; selMax = max;
        markInputIfPainting();
    }
    @Override public void clearSelectables() {
        super.clearSelectables();
        synchronized (selectables) { selectables.clear(); }
        selMin = 0; selMax = 0;
        markInputIfPainting();
    }
    @Override public void setHighlighted(final Iterable<GameEntityView> entities, final boolean b) {
        super.setHighlighted(entities, b);
        synchronized (highlightedIds) { for (final GameEntityView e : entities) { if (b) highlightedIds.add(e.getId()); else highlightedIds.remove(e.getId()); } }
        markInputIfPainting();
    }
    @Override public void setWeaklySelectable(final Iterable<CardView> cards) {
        super.setWeaklySelectable(cards);
        synchronized (weakIds) { weakIds.clear(); for (final CardView c : cards) weakIds.add(c.getId()); }
        markInputIfPainting();
    }
    @Override public void clearWeaklySelectable() {
        super.clearWeaklySelectable();
        synchronized (weakIds) { weakIds.clear(); }
        markInputIfPainting();
    }

    // ---- blocking prompts ---------------------------------------------------------------------

    /** Push fresh state, then send the prompt and block for the answer. Called on the game thread. */
    private JsonElement ask(final JsonObject prompt) {
        stateDirty = true;
        flush();
        return prompts.ask(prompt);
    }

    private static int[] indices(final JsonElement answer) {
        if (answer == null || !answer.isJsonArray()) return new int[0];
        final JsonArray a = answer.getAsJsonArray();
        final int[] out = new int[a.size()];
        for (int i = 0; i < out.length; i++) out[i] = a.get(i).getAsInt();
        return out;
    }

    @Override public SpellAbilityView getAbilityToPlay(final CardView hostCard, final List<SpellAbilityView> abilities, final ITriggerEvent triggerEvent) {
        if (abilities == null || abilities.isEmpty()) return null;
        if (abilities.size() == 1) return abilities.get(0);
        final JsonObject p = prompts.newPrompt("chooseAbility");
        p.add("hostCard", vj.card(hostCard));
        p.add("options", ChoiceJson.options(abilities, null, vj));
        final JsonElement a = ask(p);
        if (a == null || a.isJsonNull()) return null;
        final int i = a.getAsInt();
        return i >= 0 && i < abilities.size() ? abilities.get(i) : null;
    }

    @Override public Map<CardView, Integer> assignCombatDamage(final CardView attacker, final List<CardView> blockers, final int damage, final GameEntityView defender, final boolean overrideOrder, final boolean maySkip) {
        final JsonObject p = prompts.newPrompt("assignCombatDamage");
        p.add("attacker", vj.card(attacker));
        p.add("blockers", vj.cards(blockers));
        p.addProperty("damage", damage);
        p.add("defender", vj.entityRef(defender));
        p.addProperty("overrideOrder", overrideOrder);
        p.addProperty("maySkip", maySkip);
        final JsonElement a = ask(p);
        if (a == null || a.isJsonNull()) return null; // auto / skip
        // answer: {"<cardId>": n, "defender": n}
        final Map<CardView, Integer> m = new LinkedHashMap<>();
        for (final Map.Entry<String, JsonElement> e : a.getAsJsonObject().entrySet()) {
            if ("defender".equals(e.getKey())) { m.put(null, e.getValue().getAsInt()); continue; }
            final int id = Integer.parseInt(e.getKey());
            for (final CardView b : blockers) if (b.getId() == id) m.put(b, e.getValue().getAsInt());
        }
        return m;
    }

    @Override public Map<Object, Integer> assignGenericAmount(final CardView effectSource, final Map<Object, Integer> target, final int amount, final boolean atLeastOne, final String amountLabel) {
        final JsonObject p = prompts.newPrompt("assignAmount");
        p.add("source", vj.card(effectSource));
        p.addProperty("amount", amount);
        p.addProperty("atLeastOne", atLeastOne);
        p.addProperty("label", amountLabel);
        final List<Object> keys = new ArrayList<>(target.keySet());
        final JsonArray opts = new JsonArray();
        for (int i = 0; i < keys.size(); i++) {
            final JsonObject o = ChoiceJson.option(i, keys.get(i), null, vj);
            o.addProperty("current", target.get(keys.get(i)));
            opts.add(o);
        }
        p.add("options", opts);
        final JsonElement a = ask(p);
        final Map<Object, Integer> m = new LinkedHashMap<>();
        if (a == null || a.isJsonNull()) return m;
        for (final Map.Entry<String, JsonElement> e : a.getAsJsonObject().entrySet()) {
            final int i = Integer.parseInt(e.getKey());
            if (i >= 0 && i < keys.size()) m.put(keys.get(i), e.getValue().getAsInt());
        }
        return m;
    }

    @Override public boolean showConfirmDialog(final String message, final String title, final String yesButtonText, final String noButtonText, final boolean defaultYes) {
        final JsonObject p = prompts.newPrompt("confirm");
        p.addProperty("title", title);
        p.addProperty("message", message);
        p.addProperty("yes", yesButtonText);
        p.addProperty("no", noButtonText);
        p.addProperty("defaultYes", defaultYes);
        final JsonElement a = ask(p);
        return a == null || a.isJsonNull() ? defaultYes : a.getAsBoolean();
    }

    @Override public boolean confirm(final CardView c, final String question, final boolean defaultIsYes, final List<String> options) {
        final JsonObject p = prompts.newPrompt("confirm");
        p.add("card", vj.card(c));
        p.addProperty("message", question);
        p.addProperty("yes", options != null && options.size() > 0 ? options.get(0) : "Yes");
        p.addProperty("no", options != null && options.size() > 1 ? options.get(1) : "No");
        p.addProperty("defaultYes", defaultIsYes);
        final JsonElement a = ask(p);
        return a == null || a.isJsonNull() ? defaultIsYes : a.getAsBoolean();
    }

    @Override public int showOptionDialog(final String message, final String title, final FSkinProp icon, final List<String> options, final int defaultOption) {
        final JsonObject p = prompts.newPrompt("option");
        p.addProperty("title", title);
        p.addProperty("message", message);
        p.add("options", Json.strings(options));
        p.addProperty("default", defaultOption);
        final JsonElement a = ask(p);
        return a == null || a.isJsonNull() ? defaultOption : a.getAsInt();
    }

    @Override public String showInputDialog(final String message, final String title, final FSkinProp icon, final String initialInput, final List<String> inputOptions, final boolean isNumeric) {
        final JsonObject p = prompts.newPrompt("text");
        p.addProperty("title", title);
        p.addProperty("message", message);
        p.addProperty("initial", initialInput);
        p.add("options", Json.strings(inputOptions));
        p.addProperty("numeric", isNumeric);
        final JsonElement a = ask(p);
        return a == null || a.isJsonNull() ? initialInput : a.getAsString();
    }

    @Override public <T> List<T> getChoices(final String message, final int min, final int max, final List<T> choices, final List<T> selected, final FSerializableFunction<T, String> display) {
        final JsonObject p = prompts.newPrompt("choices");
        p.addProperty("message", message);
        p.addProperty("min", min);
        p.addProperty("max", max);
        p.add("options", ChoiceJson.options(choices, display, vj));
        final JsonArray pre = new JsonArray();
        if (selected != null) for (final T s : selected) { final int i = choices.indexOf(s); if (i >= 0) pre.add(i); }
        p.add("selected", pre);
        final List<T> out = new ArrayList<>();
        for (final int i : indices(ask(p))) if (i >= 0 && i < choices.size()) out.add(choices.get(i));
        return out;
    }

    @Override public <T> OrderResult<T> order(final String title, final String top, final int remainingObjectsMin, final int remainingObjectsMax, final List<T> sourceChoices, final List<T> destChoices, final CardView referenceCard, final boolean sideboardingMode, final boolean showRememberCheckbox) {
        final JsonObject p = prompts.newPrompt("order");
        p.addProperty("title", title);
        p.addProperty("top", top);
        p.addProperty("remainMin", remainingObjectsMin);
        p.addProperty("remainMax", remainingObjectsMax);
        // options are indexed over source ++ dest
        final List<T> all = new ArrayList<>(sourceChoices);
        if (destChoices != null) all.addAll(destChoices);
        p.add("options", ChoiceJson.options(all, null, vj));
        p.addProperty("sourceCount", sourceChoices.size());
        p.add("refCard", vj.card(referenceCard));
        p.addProperty("showRemember", showRememberCheckbox);
        final JsonElement a = ask(p);
        // answer: {"order":[i...], "remember":bool}
        final List<T> ordered = new ArrayList<>();
        boolean remember = false;
        if (a != null && a.isJsonObject()) {
            for (final int i : indices(a.getAsJsonObject().get("order"))) if (i >= 0 && i < all.size()) ordered.add(all.get(i));
            remember = Json.bool(a.getAsJsonObject(), "remember", false);
        } else {
            for (final int i : indices(a)) if (i >= 0 && i < all.size()) ordered.add(all.get(i));
        }
        return new OrderResult<>(ordered, remember);
    }

    @Override public List<PaperCard> sideboard(final CardPool sideboard, final CardPool main, final String message) {
        return null; // no sideboarding vs. AI in v1
    }

    @Override public GameEntityView chooseSingleEntityForEffect(final String title, final List<? extends GameEntityView> optionList, final DelayedReveal delayedReveal, final boolean isOptional) {
        final JsonObject p = prompts.newPrompt("chooseEntity");
        p.addProperty("title", title);
        p.addProperty("min", isOptional ? 0 : 1);
        p.addProperty("max", 1);
        p.add("options", ChoiceJson.options(optionList, null, vj));
        final JsonObject rv = ChoiceJson.reveal(delayedReveal, vj);
        if (rv != null) p.add("reveal", rv);
        final int[] idx = indices(ask(p));
        return idx.length == 0 || idx[0] < 0 || idx[0] >= optionList.size() ? null : optionList.get(idx[0]);
    }

    @Override public List<GameEntityView> chooseEntitiesForEffect(final String title, final List<? extends GameEntityView> optionList, final int min, final int max, final DelayedReveal delayedReveal) {
        final JsonObject p = prompts.newPrompt("chooseEntity");
        p.addProperty("title", title);
        p.addProperty("min", min);
        p.addProperty("max", max);
        p.add("options", ChoiceJson.options(optionList, null, vj));
        final JsonObject rv = ChoiceJson.reveal(delayedReveal, vj);
        if (rv != null) p.add("reveal", rv);
        final List<GameEntityView> out = new ArrayList<>();
        for (final int i : indices(ask(p))) if (i >= 0 && i < optionList.size()) out.add(optionList.get(i));
        return out;
    }

    @Override public List<CardView> manipulateCardList(final String title, final Iterable<CardView> cards, final Iterable<CardView> manipulable, final boolean toTop, final boolean toBottom, final boolean toAnywhere) {
        final List<CardView> all = new ArrayList<>();
        for (final CardView c : cards) all.add(c);
        final Set<Integer> movable = new LinkedHashSet<>();
        for (final CardView c : manipulable) movable.add(c.getId());
        final JsonObject p = prompts.newPrompt("manipulateCards");
        p.addProperty("title", title);
        p.add("cards", vj.cards(all));
        p.add("manipulable", ints(movable));
        p.addProperty("toTop", toTop);
        p.addProperty("toBottom", toBottom);
        p.addProperty("toAnywhere", toAnywhere);
        final JsonElement a = ask(p);
        if (a == null || !a.isJsonArray()) return all;
        // answer: array of card ids in the new order
        final List<CardView> out = new ArrayList<>();
        for (final JsonElement e : a.getAsJsonArray()) for (final CardView c : all) if (c.getId() == e.getAsInt()) out.add(c);
        for (final CardView c : all) if (!out.contains(c)) out.add(c);
        return out;
    }
}
