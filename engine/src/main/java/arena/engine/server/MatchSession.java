package arena.engine.server;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import forge.deck.Deck;
import forge.game.Game;
import forge.game.GameType;
import forge.game.card.CardView;
import forge.game.phase.PhaseType;
import forge.game.player.PlayerView;
import forge.game.player.RegisteredPlayer;
import forge.gamemodes.match.HostedMatch;
import forge.gui.FThreads;
import forge.interfaces.IGameController;
import forge.player.GamePlayerUtil;
import forge.player.LobbyPlayerHuman;
import forge.util.MyRandom;
import org.eclipse.jetty.websocket.api.Session;

import java.util.ArrayList;
import java.util.List;
import java.util.Random;
import java.util.Set;
import java.util.concurrent.CopyOnWriteArraySet;

/**
 * One running match: the Forge {@link HostedMatch}, its {@link WsGuiGame}, and the set of
 * WebSocket sessions currently attached (any number — reconnects and extra tabs just get a snapshot).
 */
public final class MatchSession {
    public final String id;
    public final String humanDeckRef, aiDeckRef;
    public final long createdAt = System.currentTimeMillis();
    private final Set<Session> sockets = new CopyOnWriteArraySet<>();
    private final WsGuiGame gui;
    private final HostedMatch hosted = new HostedMatch();
    private volatile boolean started, over;
    private volatile long finishedAt;

    public MatchSession(final String id, final String humanDeckRef, final String aiDeckRef) {
        this.id = id;
        this.humanDeckRef = humanDeckRef;
        this.aiDeckRef = aiDeckRef;
        this.gui = new WsGuiGame(this::broadcast);
    }

    public WsGuiGame gui() { return gui; }
    public boolean isOver() { return over; }
    public long finishedAt() { return finishedAt; }
    public int socketCount() { return sockets.size(); }

    public synchronized void start(final Long seed, final String humanName) {
        if (started) return;
        started = true;
        if (seed != null) MyRandom.setRandom(new Random(seed));
        final Deck dHuman = DeckSource.load(humanDeckRef);
        final Deck dAi = DeckSource.load(aiDeckRef);
        final RegisteredPlayer human = RegisteredPlayer.forCommander(dHuman);
        human.setPlayer(new LobbyPlayerHuman(humanName == null || humanName.isBlank() ? "You" : humanName));
        final RegisteredPlayer ai = RegisteredPlayer.forCommander(dAi);
        ai.setPlayer(GamePlayerUtil.createAiPlayer("Forge AI", 1));
        hosted.setEndGameHook(() -> { over = true; finishedAt = System.currentTimeMillis(); gui.flush(); });
        hosted.startMatch(GameType.Commander, Set.of(GameType.Commander), List.of(human, ai), human, gui);
    }

    public JsonObject describe() {
        final JsonObject o = new JsonObject();
        o.addProperty("id", id);
        o.addProperty("humanDeck", humanDeckRef);
        o.addProperty("aiDeck", aiDeckRef);
        o.addProperty("started", started);
        o.addProperty("over", over);
        o.addProperty("sockets", sockets.size());
        final Game g = hosted.getGame();
        if (g != null) {
            o.addProperty("turn", g.getPhaseHandler().getTurn());
            o.addProperty("gameOver", g.isGameOver());
        }
        return o;
    }

    // ---- sockets --------------------------------------------------------------------------

    public void attach(final Session s) {
        sockets.add(s);
        final JsonObject hello = Json.frame("hello");
        hello.addProperty("matchId", id);
        hello.addProperty("humanDeck", humanDeckRef);
        hello.addProperty("aiDeck", aiDeckRef);
        send(s, hello);
        for (final JsonObject f : gui.snapshot()) send(s, f);
    }

    public void detach(final Session s) { sockets.remove(s); }

    private void broadcast(final JsonObject frame) {
        final String text = Json.GSON.toJson(frame);
        for (final Session s : sockets) sendText(s, text);
    }
    private void send(final Session s, final JsonObject frame) { sendText(s, Json.GSON.toJson(frame)); }
    private void sendText(final Session s, final String text) {
        try { if (s.isOpen()) s.getRemote().sendStringByFuture(text); }
        catch (final Exception e) { System.err.println("[match " + id + "] send failed: " + e); }
    }

    public void close() {
        gui.prompts().cancelAll();
        for (final Session s : sockets) { try { s.close(); } catch (final Exception ignored) {} }
        sockets.clear();
    }

    // ---- incoming frames -------------------------------------------------------------------

    public void onFrame(final Session from, final JsonObject f) {
        final String t = Json.str(f, "t");
        if (t == null) return;
        switch (t) {
            case "ping" -> send(from, Json.frame("pong"));
            case "resync" -> { for (final JsonObject fr : gui.snapshot()) send(from, fr); }
            case "answer" -> {
                final Integer id = Json.integer(f, "id");
                if (id == null || !gui.prompts().answer(id, f.get("value"))) {
                    send(from, error("no pending prompt with id " + id));
                }
            }
            case "click" -> onClick(from, f);
            case "concede" -> onEdt(() -> { final IGameController c = controller(); if (c != null) c.concede(); });
            case "settings" -> onSettings(f);
            case "yield" -> { // {key, auto: bool} — MTGO-style auto-yield for an ability's yield key (StackItem.key)
                final String key = Json.str(f, "key");
                final boolean auto = Json.bool(f, "auto", true);
                if (key != null) onEdt(() -> {
                    final IGameController c = controller();
                    if (c != null) { c.setShouldAutoYield(key, auto, false); broadcast(gui.stateFrame()); }
                });
            }
            case "clearYields" -> onEdt(() -> { final IGameController c = controller(); if (c != null) { c.getYieldController().clearAutoYields(); broadcast(gui.stateFrame()); } });
            default -> send(from, error("unknown frame type: " + t));
        }
    }

    private static final boolean TRACE = Boolean.getBoolean("arena.trace");
    private String id() { return id; }
    private String currentInput() { return String.valueOf(currentInputObj()); }
    private Object currentInputObj() {
        final IGameController c = controller();
        return c instanceof forge.player.PlayerControllerHuman pch ? pch.getInputProxy().getInput() : null;
    }

    private IGameController controller() {
        IGameController c = gui.getGameController();
        if (c == null && gui.me() != null) c = gui.getGameController(gui.me());
        return c;
    }

    /** Emulate a GUI click: run on the EDT like Forge's own GUIs do, so clicks never race each other. */
    private void onEdt(final Runnable r) { FThreads.invokeInEdtLater(r); }

    private void onClick(final Session from, final JsonObject f) {
        final String action = Json.str(f, "action");
        final Integer id = Json.integer(f, "id");
        onEdt(() -> {
            final IGameController c = controller();
            if (c == null) { send(from, error("no controller yet")); return; }
            if (TRACE && !"card".equals(action)) System.out.println("[match " + id() + "] click " + action + " input=" + currentInput());
            // Optimistically disable buttons for the duration of the click; if the engine neither swapped
            // the Input nor repainted it (click was a no-op, e.g. an unplayable card), put them back.
            final Object inputBefore = currentInputObj();
            final long paintBefore = gui.paintSeq();
            final boolean[] saved = gui.consumeButtons();
            try {
                dispatchClick(from, c, action, id);
            } finally {
                if (currentInputObj() == inputBefore && gui.paintSeq() == paintBefore) gui.restoreButtons(saved);
            }
        });
    }

    private void dispatchClick(final Session from, final IGameController c, final String action, final Integer id) {
        {
            switch (action == null ? "" : action) {
                case "ok" -> c.selectButtonOk();
                case "cancel" -> c.selectButtonCancel();
                case "pass" -> c.passPriority();
                case "alphaStrike" -> c.alphaStrike();
                case "undo" -> c.undoLastAction();
                case "card" -> {
                    final CardView cv = id == null ? null : gui.cardById(id);
                    if (cv == null) { send(from, error("unknown card id " + id)); return; }
                    final boolean accepted = c.selectCard(cv, null, null);
                    if (TRACE) System.out.println("[match " + id() + "] click card " + id + " (" + cv.getName() + ") accepted=" + accepted + " input=" + currentInput());
                    if (!accepted) gui.flashIncorrectAction();
                }
                case "player" -> {
                    final PlayerView pv = id == null ? null : gui.playerById(id);
                    if (pv == null) { send(from, error("unknown player id " + id)); return; }
                    c.selectPlayer(pv, null);
                }
                default -> send(from, error("unknown click action: " + action));
            }
        }
    }

    private void onSettings(final JsonObject f) {
        final JsonElement stops = f.get("stops");
        if (stops != null && stops.isJsonObject()) {
            final JsonObject so = stops.getAsJsonObject();
            if (so.has("myTurn")) gui.setStops(true, phases(so.getAsJsonArray("myTurn")));
            if (so.has("oppTurn")) gui.setStops(false, phases(so.getAsJsonArray("oppTurn")));
        }
        broadcast(gui.stateFrame());
    }

    private static List<PhaseType> phases(final JsonArray a) {
        final List<PhaseType> out = new ArrayList<>();
        for (final JsonElement e : a) { try { out.add(PhaseType.valueOf(e.getAsString())); } catch (final IllegalArgumentException ignored) {} }
        return out;
    }

    private static JsonObject error(final String msg) {
        final JsonObject f = Json.frame("error");
        f.addProperty("text", msg);
        return f;
    }
}
