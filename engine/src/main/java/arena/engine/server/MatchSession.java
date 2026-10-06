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
import forge.gui.interfaces.IGuiGame;
import forge.interfaces.IGameController;
import forge.player.GamePlayerUtil;
import forge.player.LobbyPlayerHuman;
import forge.util.MyRandom;
import org.eclipse.jetty.websocket.api.Session;

import java.util.ArrayList;
import java.util.HashSet;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Random;
import java.util.Set;

/**
 * One match: up to {@link #MAX_SEATS} {@link Seat}s, the Forge {@link HostedMatch}, and the sockets bound
 * to each seat.
 *
 * <p>Two lives: a <em>classic</em> match ({@code POST /api/match}: one human vs. one AI, started at once;
 * a socket without a token is the human — the protocol every v2 client speaks), and a <em>lobby</em>
 * ({@code POST /api/lobby}): seat 0 is the host, the others are open until players join over the invite
 * link ({@code POST /api/lobby/{id}/join} → a seat token) or the host fills them with AI. Before the host
 * sends {@code start}, sockets receive {@code lobby} frames; after it, each socket gets the usual
 * {@code hello} + snapshot of <em>its</em> seat's {@link WsGuiGame}.
 */
public final class MatchSession {
    public static final int MAX_SEATS = 4;
    /** A seated human whose sockets are all gone this long during a game concedes (the table can't wait forever). */
    static final long ABANDON_MS = 120_000L;

    public final String id;
    public final String format;
    /** Format + legality + variants (see GameSetup). */
    final GameSetup setup;
    /** Event tables start by themselves once every seated human has connected. */
    private volatile boolean autoStart;
    /** Called once when the game ends (events record results). */
    private volatile Runnable onOver;
    public final boolean lobby;
    public final long createdAt = System.currentTimeMillis();
    private final List<Seat> seats = new ArrayList<>();
    private final HostedMatch hosted = new HostedMatch();
    private final String inviteBase;
    private volatile boolean started, over;
    private volatile long finishedAt;
    private volatile Long seed;
    /** Seat that may edit seats and start; moves on if the host leaves the lobby. */
    private volatile int hostSeat = 0;

    private MatchSession(final String id, final GameSetup setup, final boolean lobby, final String inviteBase) {
        this.id = id;
        this.setup = setup;
        this.format = setup.format;
        this.lobby = lobby;
        this.inviteBase = inviteBase;
    }

    /** 1 human vs. 1 AI, started immediately (the original single-player flow). */
    static MatchSession classic(final String id, final String humanDeckRef, final String aiDeckRef, final GameSetup setup, final Long seed, final String humanName, final JsonObject humanAvatar) {
        final MatchSession m = new MatchSession(id, setup, false, null);
        if (setup.variants.contains("teams")) {
            // 2v2 against the AI: you + an AI partner vs two AI opponents (seats alternate teams)
            final Seat h = m.addSeat(Seat.Kind.HUMAN);
            h.name = humanName == null || humanName.isBlank() ? "You" : humanName.trim();
            h.deckRef = humanDeckRef;
            h.avatar = Avatars.clean(humanAvatar);
            final String[] names = { "Forge AI", "Forge Ally", "Forge AI (2)" };
            for (final String n : names) { final Seat a = m.addSeat(Seat.Kind.AI); a.name = n; a.deckRef = "random"; }
            m.seats.get(1).deckRef = aiDeckRef;
            m.seed = seed;
            m.start();
            return m;
        }
        final Seat human = m.addSeat(Seat.Kind.HUMAN);
        human.name = humanName == null || humanName.isBlank() ? "You" : humanName.trim();
        human.deckRef = humanDeckRef;
        human.avatar = Avatars.clean(humanAvatar);
        final Seat ai = m.addSeat(Seat.Kind.AI);
        ai.name = "Forge AI";
        ai.deckRef = aiDeckRef;
        m.seed = seed;
        m.start();
        return m;
    }

    /** A lobby with the host in seat 0 and {@code players - 1} open seats. */
    static MatchSession lobby(final String id, final GameSetup setup, final int players, final String hostName, final String hostDeck, final JsonObject hostAvatar, final String inviteBase) {
        if (players < 2 || players > MAX_SEATS) throw new IllegalArgumentException("players must be 2.." + MAX_SEATS);
        final String pp = setup.playerProblem(players);
        if (pp != null) throw new IllegalArgumentException(pp);
        final MatchSession m = new MatchSession(id, setup, true, inviteBase);
        final Seat host = m.addSeat(Seat.Kind.HUMAN);
        host.name = cleanName(hostName, "Host");
        m.setDeck(host, hostDeck);
        host.avatar = Avatars.clean(hostAvatar);
        for (int i = 1; i < players; i++) m.addSeat(Seat.Kind.OPEN);
        return m;
    }

    /** One seat of a table dealt by a limited event: a prebuilt deck, human (token issued) or AI; avatar may be null. */
    record EventPlayer(String name, Deck deck, boolean human, JsonObject avatar) {}

    /**
     * A table dealt from a draft/sealed event: decks are already built, humans get seat tokens (see
     * {@link #tokenOf}) and the game starts as soon as all of them are connected.
     */
    static MatchSession event(final String id, final List<EventPlayer> players, final Runnable onOver) {
        final MatchSession m = new MatchSession(id, GameSetup.of(DeckSource.LIMITED), true, null);
        for (final EventPlayer p : players) {
            final Seat s = m.addSeat(p.human() ? Seat.Kind.HUMAN : Seat.Kind.AI);
            s.name = p.name();
            s.deck = p.deck();
            s.deckRef = "event";
            s.deckName = p.deck().getName();
            s.avatar = Avatars.clean(p.avatar());
            if (p.human()) s.goneSince = System.currentTimeMillis();
        }
        m.autoStart = true;
        m.onOver = onOver;
        return m;
    }

    String tokenOf(final int seat) { return seats.get(seat).token; }
    int seatCount() { return seats.size(); }
    String seatName(final int seat) { return seats.get(seat).name; }
    /** Winner's name once the game is over (null while running / on a draw). */
    String winnerName() {
        final Game g = hosted.getGame();
        try { return g == null ? null : g.getView().getWinningPlayerName(); } catch (final Exception e) { return null; }
    }

    private Seat addSeat(final Seat.Kind kind) {
        final Seat s = new Seat(seats.size(), kind, this::seatOut);
        seats.add(s);
        return s;
    }

    public boolean isOver() { return over; }
    public boolean isStarted() { return started; }
    public long finishedAt() { return finishedAt; }
    public int socketCount() { int n = 0; for (final Seat s : seats) n += s.sockets.size(); return n; }
    /** The invite link for this lobby, or null (classic match / no public URL). */
    public String invite() { return lobby && inviteBase != null ? inviteBase + "/?join=" + id : null; }

    /** Seat 0's GUI — what single-player tooling (spike, tests) looks at. */
    public WsGuiGame gui() { return seats.get(0).gui; }

    // ---- lobby --------------------------------------------------------------------------------

    private static String cleanName(final String n, final String dflt) {
        if (n == null || n.isBlank()) return dflt;
        final String t = n.strip().replaceAll("\\s+", " ");
        return t.length() > 24 ? t.substring(0, 24) : t;
    }

    /** Validate (and name) a deck for this match's format. Remote players may not point at host files. */
    private void setDeck(final Seat s, final String ref) {
        final String r = ref == null || ref.isBlank() ? "random" : ref.trim();
        if (r.startsWith("file:")) throw new IllegalArgumentException("file: deck refs are not allowed in a lobby");
        if (setup.decksIgnored()) { s.deckRef = "random"; s.deckName = "Momir Basic"; return; }
        final Deck d = setup.loadDeck(r); // enforces the table's legality
        s.deckRef = r;
        s.deckName = "random".equals(r) ? "Random deck" : d.getName();
    }

    /** Names are how Forge reports the winner, so they must be unique at the table. */
    private String uniqueName(final String want, final Seat self) {
        final Set<String> taken = new HashSet<>();
        for (final Seat s : seats) if (s != self && s.filled() && s.name != null) taken.add(s.name.toLowerCase());
        if (!taken.contains(want.toLowerCase())) return want;
        for (int i = 2; ; i++) { final String c = want + " (" + i + ")"; if (!taken.contains(c.toLowerCase())) return c; }
    }

    /** Take the first open seat. Returns it (its token is the caller's credential). */
    public synchronized JsonObject join(final String name, final String deckRef, final JsonObject avatar) {
        if (!lobby) throw new IllegalArgumentException("not a lobby");
        if (started) throw new IllegalStateException("the game has already started");
        Seat free = null;
        for (final Seat s : seats) if (s.kind == Seat.Kind.OPEN) { free = s; break; }
        if (free == null) throw new IllegalStateException("the table is full");
        setDeck(free, deckRef); // throws on a bad deck before the seat is taken
        free.name = uniqueName(cleanName(name, "Player " + (free.index + 1)), free);
        free.kind = Seat.Kind.HUMAN;
        free.avatar = Avatars.clean(avatar);
        free.goneSince = System.currentTimeMillis(); // not connected until its socket arrives
        broadcastLobby();
        final JsonObject o = new JsonObject();
        o.addProperty("id", id);
        o.addProperty("seat", free.index);
        o.addProperty("token", free.token);
        o.addProperty("name", free.name);
        o.addProperty("format", format);
        return o;
    }

    /** Host-only credential returned by {@code POST /api/lobby}. */
    public JsonObject hostTicket() {
        final Seat h = seats.get(0);
        final JsonObject o = describe();
        o.addProperty("seat", 0);
        o.addProperty("token", h.token);
        o.addProperty("name", h.name);
        return o;
    }

    private JsonObject lobbyFrame(final Seat viewer) {
        final JsonObject f = Json.frame("lobby");
        fillLobby(f, viewer);
        return f;
    }

    private void fillLobby(final JsonObject o, final Seat viewer) {
        o.addProperty("id", id);
        setup.fill(o);
        o.addProperty("maxPlayers", seats.size());
        o.addProperty("host", hostSeat);
        o.addProperty("started", started);
        o.addProperty("invite", invite());
        if (viewer != null) o.addProperty("you", viewer.index);
        final JsonArray a = new JsonArray();
        for (final Seat s : seats) a.add(s.describe(s == viewer));
        o.add("seats", a);
        int ready = 0;
        boolean waiting = false;
        for (final Seat s : seats) {
            if (s.filled()) ready++;
            if (s.kind == Seat.Kind.HUMAN && !s.connected()) waiting = true;
        }
        o.addProperty("canStart", !started && ready >= 2 && !waiting);
    }

    private void broadcastLobby() {
        for (final Seat s : seats) if (!s.sockets.isEmpty()) { final JsonObject f = lobbyFrame(s); for (final Session so : s.sockets) send(so, f); }
    }

    private synchronized void onLobbyFrame(final Seat from, final Session sock, final String t, final JsonObject f) {
        if (started) { send(sock, error("the game has already started")); return; }
        final boolean host = from.index == hostSeat;
        switch (t) {
            case "lobbyDeck" -> { // {deck}: change your own deck
                try { setDeck(from, Json.str(f, "deck")); } catch (final RuntimeException e) { send(sock, error(String.valueOf(e.getMessage()))); return; }
            }
            case "lobbyAvatar" -> { // {avatar}: change your own portrait (null/invalid clears it → derived from the deck at start)
                from.avatar = Avatars.clean(f.get("avatar"));
            }
            case "lobbySeat" -> { // host: {seat, kind: "ai"|"open", deck?}
                if (!host) { send(sock, error("only the host can change seats")); return; }
                final Integer idx = Json.integer(f, "seat");
                if (idx == null || idx < 0 || idx == hostSeat || idx >= seats.size()) { send(sock, error("bad seat")); return; }
                final Seat s = seats.get(idx);
                final String kind = String.valueOf(Json.str(f, "kind"));
                if ("ai".equals(kind)) {
                    try { setDeck(s, Json.str(f, "deck")); } catch (final RuntimeException e) { send(sock, error(String.valueOf(e.getMessage()))); return; }
                    if (s.kind == Seat.Kind.HUMAN) kick(s);
                    s.kind = Seat.Kind.AI;
                    s.name = uniqueName("Forge AI", s);
                    s.avatar = null; // the AI's face comes from its deck at start
                } else if ("open".equals(kind)) {
                    if (s.kind == Seat.Kind.HUMAN) kick(s);
                    s.kind = Seat.Kind.OPEN;
                    s.name = null; s.deckRef = null; s.deckName = null; s.avatar = null;
                } else { send(sock, error("kind must be ai or open")); return; }
            }
            case "start" -> {
                if (!host) { send(sock, error("only the host can start")); return; }
                final JsonObject probe = new JsonObject();
                fillLobby(probe, null);
                if (!Json.bool(probe, "canStart", false)) { send(sock, error("need at least 2 players, all connected")); return; }
                final Integer sd = Json.integer(f, "seed");
                if (sd != null) seed = sd.longValue();
                try { start(); }
                catch (final RuntimeException e) {
                    e.printStackTrace();
                    send(sock, error("could not start: " + e.getMessage()));
                }
                return; // hello + snapshots replace the lobby frames
            }
            default -> { send(sock, error("unknown lobby frame: " + t)); return; }
        }
        broadcastLobby();
    }

    /** Drop a human from their seat: their sockets close and their token stops working. */
    private void kick(final Seat s) {
        s.token = Seat.newToken();
        for (final Session so : s.sockets) {
            send(so, error("you were removed from the table"));
            try { so.close(1000, "removed by host"); } catch (final Exception ignored) {}
        }
        s.sockets.clear();
    }

    // ---- game ----------------------------------------------------------------------------------

    private synchronized void start() {
        if (started) return;
        if (seed != null) MyRandom.setRandom(new Random(seed));
        int filled = 0;
        for (final Seat s : seats) if (s.filled()) filled++;
        final String pp = setup.playerProblem(filled);
        if (pp != null) throw new IllegalArgumentException(pp);
        final List<RegisteredPlayer> players = new ArrayList<>();
        final Map<RegisteredPlayer, IGuiGame> guis = new LinkedHashMap<>();
        int aiN = 0, order = 0;
        for (final Seat s : seats) {
            if (!s.filled()) continue;
            final Deck d = s.deck != null ? s.deck : setup.loadDeck(s.deckRef);
            if (s.deckName == null || "random".equals(s.deckRef)) s.deckName = d.getName();
            if (s.avatar == null) s.avatar = Avatars.fromDeck(d);
            final RegisteredPlayer rp = setup.register(d, filled, order++);
            if (s.kind == Seat.Kind.HUMAN) {
                rp.setPlayer(new LobbyPlayerHuman(s.name));
                guis.put(rp, s.gui);
            } else {
                rp.setPlayer(GamePlayerUtil.createAiPlayer(s.name == null ? "Forge AI" : s.name, aiN++));
            }
            players.add(rp);
        }
        started = true;
        // Everyone already in the lobby learns the game is on before the first state frame lands.
        for (final Seat s : seats) for (final Session so : s.sockets) send(so, hello(s));
        hosted.setEndGameHook(() -> {
            over = true; finishedAt = System.currentTimeMillis();
            for (final Seat s : seats) if (s.kind == Seat.Kind.HUMAN) s.gui.flush();
            final Runnable r = onOver;
            if (r != null) try { r.run(); } catch (final Exception e) { System.err.println("[match " + id + "] onOver: " + e); }
        });
        hosted.startMatch(setup.baseType(), setup.applied(), players, guis);
        final Game g = hosted.getGame();
        if (g != null) for (final Seat s : seats) if (s.kind == Seat.Kind.HUMAN) g.subscribeToEvents(s.gui.eventSubscriber());
    }

    public JsonObject describe() {
        final JsonObject o = new JsonObject();
        o.addProperty("id", id);
        final Seat h = seats.get(0);
        final Seat ai = seats.size() > 1 ? seats.get(1) : null;
        o.addProperty("humanDeck", h.deckRef);
        o.addProperty("aiDeck", ai == null ? null : ai.deckRef);
        o.addProperty("humanDeckName", h.deckName);
        o.addProperty("aiDeckName", ai == null ? null : ai.deckName);
        o.addProperty("lobby", lobby);
        o.addProperty("over", over);
        o.addProperty("sockets", socketCount());
        fillLobby(o, null);
        final Game g = hosted.getGame();
        if (g != null) {
            o.addProperty("turn", g.getPhaseHandler().getTurn());
            o.addProperty("gameOver", g.isGameOver());
        }
        return o;
    }

    // ---- sockets --------------------------------------------------------------------------

    /** Which seat a connecting socket speaks for: its token's, or (classic matches only) the human's. */
    public Seat seatFor(final String token) {
        if (token != null && !token.isBlank()) {
            for (final Seat s : seats) if (s.kind == Seat.Kind.HUMAN && token.equals(s.token)) return s;
            return null;
        }
        return lobby ? null : seats.get(0);
    }

    private JsonObject hello(final Seat s) {
        final JsonObject hello = Json.frame("hello");
        hello.addProperty("matchId", id);
        hello.addProperty("humanDeck", s.deckRef);
        hello.addProperty("humanDeckName", s.deckName);
        // "aiDeck" = the (first) opponent's deck, as single-player clients expect
        for (final Seat o : seats) if (o != s && o.filled()) { hello.addProperty("aiDeck", o.deckRef); hello.addProperty("aiDeckName", o.deckName); break; }
        setup.fill(hello);
        hello.addProperty("protocol", 3);
        hello.addProperty("seat", s.index);
        int n = 0; for (final Seat o : seats) if (o.filled()) n++;
        hello.addProperty("players", n);
        // Clients match these to the state's players by name (Forge's player name is the seat name, unique at the table).
        final JsonArray table = new JsonArray();
        for (final Seat o : seats) {
            if (!o.filled()) continue;
            final JsonObject e = new JsonObject();
            e.addProperty("seat", o.index);
            e.addProperty("name", o.name);
            e.addProperty("kind", o.kind == Seat.Kind.HUMAN ? "human" : "ai");
            final JsonObject av = o.avatar;
            if (av != null) e.add("avatar", av.deepCopy());
            table.add(e);
        }
        hello.add("seats", table);
        return hello;
    }

    public synchronized void attach(final Seat seat, final Session s) {
        seat.sockets.add(s);
        seat.goneSince = 0;
        if (!started && autoStart) {
            boolean all = true;
            for (final Seat o : seats) if (o.kind == Seat.Kind.HUMAN && !o.connected()) all = false;
            if (all) {
                try { start(); } catch (final RuntimeException e) { e.printStackTrace(); send(s, error("could not start: " + e.getMessage())); }
                if (started) return; // hello + snapshot went out from start()
            }
        }
        if (!started) { broadcastLobby(); return; }
        send(s, hello(seat));
        for (final JsonObject f : seat.gui.snapshot()) send(s, f);
    }

    public synchronized void detach(final Seat seat, final Session s) {
        if (!seat.sockets.remove(s) || !seat.sockets.isEmpty()) return;
        seat.goneSince = System.currentTimeMillis();
        if (started) return;
        // A host who left would strand everyone in the lobby: the next connected player takes over.
        if (seat.index == hostSeat) {
            for (final Seat o : seats) if (o.kind == Seat.Kind.HUMAN && o.connected()) { hostSeat = o.index; break; }
        }
        broadcastLobby();
    }

    /**
     * Called periodically by {@link MatchManager}: a human who closed the tab mid-game would stall every
     * other player at their next priority, so after {@link #ABANDON_MS} without a socket their seat goes
     * on autopilot — it concedes, then answers whatever the engine still asks it with the defaults.
     *
     * <p>Conceding mutates the game, so it must happen the way Forge's own concede button does it: on the
     * EDT while the game thread is parked waiting for <em>this</em> seat's input (never while rules are
     * being applied). Until that moment the seat just keeps waiting; a reconnect cancels everything.
     */
    void reapAbandoned(final long now) {
        if (!started || over) return;
        final boolean table = humanSeats() >= 2; // vs. AI only: the classic flow keeps the game for a later reconnect
        for (final Seat s : seats) {
            if (s.kind != Seat.Kind.HUMAN) continue;
            final boolean gone = table && !s.connected() && s.goneSince != 0 && now - s.goneSince >= ABANDON_MS;
            if (gone && !s.abandoned) {
                s.abandoned = true;
                System.out.println("[match " + id + "] seat " + s.index + " (" + s.name + ") abandoned; conceding for them");
            }
            if (s.abandoned || s.concedeRequested) driveConcede(s);
        }
    }

    /**
     * Concede for a seat and keep answering for it with defaults. The concede itself waits until the game
     * thread is parked on this seat's own input — the moment Forge's GUIs concede from — because doing it
     * while rules are resolving corrupts Forge's state (ConcurrentModificationException in the game thread).
     */
    private void driveConcede(final Seat s) {
        final JsonObject p = s.gui.prompts().pending();
        if (p != null && p.has("id")) s.gui.prompts().answer(p.get("id").getAsInt(), null);
        final IGameController c = controller(s);
        if (c == null || !InputIntrospect.awaiting(currentInputObj(c))) return; // not this seat's turn to act yet
        onEdt(() -> {
            // re-check on the EDT: another seat's release earlier in this tick may have woken the game thread
            if (!InputIntrospect.awaiting(currentInputObj(c))) return;
            if (!s.conceded) {
                s.conceded = true;
                c.concede();
            }
            // release the input the game thread is waiting on (a pass, "keep", "done"…)
            if (!over && InputIntrospect.awaiting(currentInputObj(c))) c.selectButtonOk();
        });
    }

    private int humanSeats() { int n = 0; for (final Seat s : seats) if (s.kind == Seat.Kind.HUMAN) n++; return n; }

    private void seatOut(final Seat seat, final JsonObject frame) {
        final String text = Json.GSON.toJson(frame);
        for (final Session s : seat.sockets) sendText(s, text);
    }
    private void send(final Session s, final JsonObject frame) { sendText(s, Json.GSON.toJson(frame)); }
    private void sendText(final Session s, final String text) {
        try { if (s.isOpen()) s.getRemote().sendStringByFuture(text); }
        catch (final Exception e) { System.err.println("[match " + id + "] send failed: " + e); }
    }

    public void close() {
        for (final Seat seat : seats) {
            seat.gui.prompts().cancelAll();
            for (final Session s : seat.sockets) { try { s.close(); } catch (final Exception ignored) {} }
            seat.sockets.clear();
        }
    }

    // ---- incoming frames -------------------------------------------------------------------

    public void onFrame(final Seat seat, final Session from, final JsonObject f) {
        final String t = Json.str(f, "t");
        if (t == null) return;
        if ("ping".equals(t)) { send(from, Json.frame("pong")); return; }
        if (!started) {
            if ("resync".equals(t)) { send(from, lobbyFrame(seat)); return; }
            onLobbyFrame(seat, from, t, f);
            return;
        }
        final WsGuiGame gui = seat.gui;
        switch (t) {
            case "resync" -> { for (final JsonObject fr : gui.snapshot()) send(from, fr); }
            case "answer" -> {
                final Integer id = Json.integer(f, "id");
                if (id == null || !gui.prompts().answer(id, f.get("value"))) {
                    send(from, error("no pending prompt with id " + id));
                }
            }
            case "click" -> onClick(seat, from, f);
            case "concede" -> { seat.concedeRequested = true; driveConcede(seat); } // completes at this seat's next input
            case "settings" -> onSettings(seat, f);
            case "yield" -> { // {key, auto: bool} — MTGO-style auto-yield for an ability's yield key (StackItem.key)
                final String key = Json.str(f, "key");
                final boolean auto = Json.bool(f, "auto", true);
                if (key != null) onEdt(() -> {
                    final IGameController c = controller(seat);
                    if (c != null) { c.setShouldAutoYield(key, auto, false); seatOut(seat, gui.stateFrame()); }
                });
            }
            case "clearYields" -> onEdt(() -> { final IGameController c = controller(seat); if (c != null) { c.getYieldController().clearAutoYields(); seatOut(seat, gui.stateFrame()); } });
            case "lobbyDeck", "lobbyAvatar", "lobbySeat", "start" -> send(from, error("the game has already started"));
            default -> send(from, error("unknown frame type: " + t));
        }
    }

    private static final boolean TRACE = Boolean.getBoolean("arena.trace");
    private static Object currentInputObj(final IGameController c) {
        return c instanceof forge.player.PlayerControllerHuman pch ? pch.getInputProxy().getInput() : null;
    }

    private static IGameController controller(final Seat seat) {
        final WsGuiGame gui = seat.gui;
        IGameController c = gui.getGameController();
        if (c == null && gui.me() != null) c = gui.getGameController(gui.me());
        return c;
    }

    /** Emulate a GUI click: run on the EDT like Forge's own GUIs do, so clicks never race each other. */
    private void onEdt(final Runnable r) { FThreads.invokeInEdtLater(r); }

    private void onClick(final Seat seat, final Session from, final JsonObject f) {
        final String action = Json.str(f, "action");
        final Integer id = Json.integer(f, "id");
        final WsGuiGame gui = seat.gui;
        onEdt(() -> {
            final IGameController c = controller(seat);
            if (c == null) { send(from, error("no controller yet")); return; }
            if (TRACE && !"card".equals(action)) System.out.println("[match " + this.id + "/" + seat.index + "] click " + action + " input=" + currentInputObj(c));
            // Optimistically disable buttons for the duration of the click; if the engine neither swapped
            // the Input nor repainted it (click was a no-op, e.g. an unplayable card), put them back.
            final Object inputBefore = currentInputObj(c);
            final long paintBefore = gui.paintSeq();
            final boolean[] saved = gui.consumeButtons();
            try {
                dispatchClick(gui, from, c, action, id);
            } finally {
                if (currentInputObj(c) == inputBefore && gui.paintSeq() == paintBefore) gui.restoreButtons(saved);
            }
        });
    }

    private void dispatchClick(final WsGuiGame gui, final Session from, final IGameController c, final String action, final Integer id) {
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
                if (TRACE) System.out.println("[match " + this.id + "] click card " + id + " (" + cv.getName() + ") accepted=" + accepted);
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

    private void onSettings(final Seat seat, final JsonObject f) {
        final JsonElement stops = f.get("stops");
        if (stops != null && stops.isJsonObject()) {
            final JsonObject so = stops.getAsJsonObject();
            if (so.has("myTurn")) seat.gui.setStops(true, phases(so.getAsJsonArray("myTurn")));
            if (so.has("oppTurn")) seat.gui.setStops(false, phases(so.getAsJsonArray("oppTurn")));
        }
        seatOut(seat, seat.gui.stateFrame());
    }

    private static List<PhaseType> phases(final JsonArray a) {
        final List<PhaseType> out = new ArrayList<>();
        for (final JsonElement e : a) { try { out.add(PhaseType.valueOf(e.getAsString())); } catch (final IllegalArgumentException ignored) {} }
        return out;
    }

    static JsonObject error(final String msg) {
        final JsonObject f = Json.frame("error");
        f.addProperty("text", msg);
        return f;
    }
}
