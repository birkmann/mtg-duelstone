package arena.engine.server;

import arena.engine.deck.CardCatalog;
import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import forge.card.CardEdition;
import forge.deck.CardPool;
import forge.deck.Deck;
import forge.deck.DeckSection;
import forge.gamemodes.limited.BoosterDraft;
import forge.gamemodes.limited.CustomLimited;
import forge.gamemodes.limited.DraftPack;
import forge.gamemodes.limited.IBoosterDraft;
import forge.gamemodes.limited.LimitedPlayer;
import forge.gamemodes.limited.LimitedPlayerAI;
import forge.gamemodes.limited.LimitedPoolType;
import forge.gamemodes.limited.SealedDeckBuilder;
import forge.item.PaperCard;
import forge.item.SealedTemplate;
import forge.item.generation.IUnOpenedProduct;
import forge.item.generation.UnOpenedProduct;
import forge.model.FModel;
import org.eclipse.jetty.websocket.api.Session;

import java.util.ArrayList;
import java.util.HashSet;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.concurrent.CopyOnWriteArraySet;

/**
 * A limited event: a pod of up to 8 drafters (humans over invite links + Forge AI drafters) doing a cube
 * draft, a set draft or sealed; then everyone builds a 40-card deck and the host deals games from them.
 *
 * <p>Why our own driver around Forge's {@link BoosterDraft}: Forge's single-player flow assumes one local
 * human at seat 0 and GUI prompts for every choice; its network host is welded to its Netty lobby. We
 * reuse the model (packs, {@link LimitedPlayer}s, {@link LimitedPlayerAI} picks and deck building) and
 * replay the network host's loop: every seat picks at its own pace, a pick passes the pack left/right,
 * AI seats pick whenever they hold a pack, a round ends when every queue is empty.
 *
 * <p>Forge keeps the draft's land set and the AI pick-rankings file in static fields, so every touch of
 * draft state happens under {@link #DRAFT_LOCK} with those statics set for this event first.
 *
 * <p>Pick ids: {@code packId * 1000 + index} into the seat's head pack. A head pack only changes when its
 * holder picks, so an id a client saw stays valid until that client's own pick.
 */
public final class EventSession {
    static final Object DRAFT_LOCK = new Object();
    static final int MAX_POD = 8, MAX_HUMANS = 4, MIN_DECK = 40;
    /** A drafter who left keeps their seat this long, then their picks are made for them. */
    static final long PICK_GRACE_MS = 60_000L;

    enum Kind { CUBE_DRAFT, SET_DRAFT, SEALED }

    static final class ESeat {
        final int index;
        volatile Seat.Kind kind;
        volatile String name;
        volatile String token = Seat.newToken();
        final Set<Session> sockets = new CopyOnWriteArraySet<>();
        volatile long goneSince;
        final List<PoolCard> pool = new ArrayList<>();
        JsonObject deckJson;   // submitted LimitedDeck
        Deck deck;             // built (humans: from deckJson; AI: Forge's builder)
        int picks;

        ESeat(final int index, final Seat.Kind kind) { this.index = index; this.kind = kind; }
        boolean connected() { return !sockets.isEmpty(); }
    }

    record PoolCard(int id, PaperCard card) {}

    /** Forge's draft with our pod size and products (its factories prompt through the GUI). */
    private static final class ArenaDraft extends BoosterDraft {
        ArenaDraft(final int pod, final List<IUnOpenedProduct> products) {
            super(LimitedPoolType.Custom, pod);
            product.addAll(products);
        }
    }

    final MatchManager mgr;
    final String id;
    final Kind kind;
    final String source, sourceName;
    private final String inviteBase;
    private final List<ESeat> seats = new ArrayList<>();
    private volatile int hostSeat = 0;
    private volatile String phase = "lobby";
    private volatile long lastActivity = System.currentTimeMillis();
    private final Long seed;

    // draft / pool state (guarded by DRAFT_LOCK)
    private ArenaDraft draft;
    private String landSet;     // basic lands' set, null = Forge picks
    private String rankings;    // custom AI rankings file (cubes)
    private int rounds, initialPackSize;
    private int nextCardId = 1;
    private final List<JsonObject> games = new ArrayList<>();

    EventSession(final MatchManager mgr, final String id, final JsonObject b, final String inviteBase) {
        this.mgr = mgr;
        this.id = id;
        this.inviteBase = inviteBase;
        final String k = String.valueOf(Json.str(b, "kind"));
        this.kind = switch (k) {
            case "cubeDraft" -> Kind.CUBE_DRAFT;
            case "setDraft" -> Kind.SET_DRAFT;
            case "sealed" -> Kind.SEALED;
            default -> throw new IllegalArgumentException("kind must be cubeDraft|setDraft|sealed");
        };
        this.source = Json.str(b, "source");
        if (source == null || source.isBlank()) throw new IllegalArgumentException("source (cube ref or set code) required");
        this.sourceName = isCube() ? CubeStore.nameOf(source) : setName(source);
        final Integer pod = Json.integer(b, "podSize");
        final int n = Math.max(2, Math.min(MAX_POD, pod == null ? 8 : pod));
        final Integer sd = Json.integer(b, "seed");
        this.seed = sd == null ? null : sd.longValue();
        for (int i = 0; i < n; i++) seats.add(new ESeat(i, i == 0 ? Seat.Kind.HUMAN : Seat.Kind.OPEN));
        seats.get(0).name = cleanName(Json.str(b, "name"), "Host");
        seats.get(0).goneSince = System.currentTimeMillis();
        if (isCube()) checkCubeSize(CubeStore.limited(source), n);
    }

    private boolean isCube() { return kind == Kind.CUBE_DRAFT || (kind == Kind.SEALED && source.contains(":")); }

    private static String setName(final String code) {
        final CardEdition e = FModel.getMagicDb().getEditions().get(code);
        if (e == null || FModel.getMagicDb().getBoosters().get(e.getCode()) == null) throw new IllegalArgumentException("no boosters for set " + code);
        return e.getName();
    }

    private void checkCubeSize(final CustomLimited cl, final int pod) {
        final int per = kind == Kind.SEALED ? sealedPacks(cl) : cl.getNumPacks();
        int packSize = 0;
        for (final var s : cl.getSealedProductTemplate().getSlots()) packSize += s.getRight();
        final int need = per * packSize * pod;
        if (cl.getCardPool().countAll() < need)
            throw new IllegalArgumentException(cl.getName() + " has " + cl.getCardPool().countAll() + " cards; a pod of " + pod + " needs " + need + " (add cards or make the pod smaller)");
    }

    private static int sealedPacks(final CustomLimited cl) { return Math.max(3, Math.min(6, cl.getNumPacks() * 2)); }

    private static String cleanName(final String n, final String dflt) {
        if (n == null || n.isBlank()) return dflt;
        final String t = n.strip().replaceAll("\\s+", " ");
        return t.length() > 24 ? t.substring(0, 24) : t;
    }

    long lastActivity() { return lastActivity; }
    int socketCount() { int n = 0; for (final ESeat s : seats) n += s.sockets.size(); return n; }
    String invite() { return inviteBase == null ? null : inviteBase + "/?event=" + id; }

    // ---- joining -------------------------------------------------------------------------------

    JsonObject hostTicket() {
        final JsonObject o = ticket(seats.get(0));
        o.addProperty("invite", invite());
        return o;
    }

    private JsonObject ticket(final ESeat s) {
        final JsonObject o = new JsonObject();
        o.addProperty("id", id);
        o.addProperty("seat", s.index);
        o.addProperty("token", s.token);
        o.addProperty("name", s.name);
        o.addProperty("format", DeckSource.LIMITED);
        return o;
    }

    synchronized JsonObject join(final String name) {
        if (!"lobby".equals(phase)) throw new IllegalStateException("the event has already started");
        int humans = 0;
        for (final ESeat s : seats) if (s.kind == Seat.Kind.HUMAN) humans++;
        if (humans >= MAX_HUMANS) throw new IllegalStateException("this pod already has " + MAX_HUMANS + " players (games seat at most 4)");
        for (final ESeat s : seats) {
            if (s.kind != Seat.Kind.OPEN) continue;
            s.kind = Seat.Kind.HUMAN;
            final Set<String> taken = new HashSet<>();
            for (final ESeat o : seats) if (o != s && o.name != null) taken.add(o.name.toLowerCase());
            String n = cleanName(name, "Player " + (s.index + 1));
            for (int i = 2; taken.contains(n.toLowerCase()); i++) n = cleanName(name, "Player") + " (" + i + ")";
            s.name = n;
            s.goneSince = System.currentTimeMillis();
            broadcast();
            return ticket(s);
        }
        throw new IllegalStateException("the pod is full");
    }

    ESeat seatFor(final String token) {
        if (token == null) return null;
        for (final ESeat s : seats) if (s.kind == Seat.Kind.HUMAN && token.equals(s.token)) return s;
        return null;
    }

    synchronized void attach(final ESeat s, final Session so) {
        s.sockets.add(so);
        s.goneSince = 0;
        lastActivity = System.currentTimeMillis();
        broadcast();
    }

    synchronized void detach(final ESeat s, final Session so) {
        if (!s.sockets.remove(so) || s.connected()) return;
        s.goneSince = System.currentTimeMillis();
        if ("lobby".equals(phase) && s.index == hostSeat) {
            for (final ESeat o : seats) if (o.kind == Seat.Kind.HUMAN && o.connected()) { hostSeat = o.index; break; }
        }
        broadcast();
    }

    // ---- frames ----------------------------------------------------------------------------------

    void onFrame(final ESeat from, final Session so, final JsonObject f) {
        final String t = Json.str(f, "t");
        if (t == null) return;
        lastActivity = System.currentTimeMillis();
        try {
            switch (t) {
                case "ping" -> send(so, Json.frame("pong"));
                case "resync" -> send(so, frame(from));
                case "eventSeat" -> onSeat(from, f);
                case "startEvent" -> start(from);
                case "pick" -> pick(from, Json.integer(f, "id"));
                case "submitDeck" -> submit(from, f.has("deck") && f.get("deck").isJsonObject() ? f.getAsJsonObject("deck") : new JsonObject());
                case "suggestDeck" -> suggest(from, so);
                case "dealGames" -> deal(from, Json.str(f, "pairing"), Json.integer(f, "tableSize"));
                default -> send(so, MatchSession.error("unknown event frame: " + t));
            }
        } catch (final IllegalArgumentException | IllegalStateException e) {
            send(so, MatchSession.error(e.getMessage()));
        } catch (final RuntimeException e) {
            e.printStackTrace();
            send(so, MatchSession.error("event error: " + e));
        }
    }

    private synchronized void onSeat(final ESeat from, final JsonObject f) {
        if (from.index != hostSeat) throw new IllegalStateException("only the host can change seats");
        if (!"lobby".equals(phase)) throw new IllegalStateException("the event has already started");
        final Integer idx = Json.integer(f, "seat");
        if (idx == null || idx < 0 || idx >= seats.size() || idx == hostSeat) throw new IllegalArgumentException("bad seat");
        final ESeat s = seats.get(idx);
        if (s.kind == Seat.Kind.HUMAN) {
            s.token = Seat.newToken();
            for (final Session so : s.sockets) { send(so, MatchSession.error("you were removed from the pod")); try { so.close(1000, "removed"); } catch (final Exception ignored) {} }
            s.sockets.clear();
        }
        s.kind = "ai".equals(Json.str(f, "kind")) ? Seat.Kind.AI : Seat.Kind.OPEN;
        s.name = s.kind == Seat.Kind.AI ? "AI Drafter " + (s.index + 1) : null;
        broadcast();
    }

    // ---- start: products, draft / sealed pools -----------------------------------------------------

    /** Set Forge's per-draft statics (land set, AI rankings) for this event. Call under DRAFT_LOCK. */
    private void statics() {
        IBoosterDraft.LAND_SET_CODE[0] = landSet == null ? null : FModel.getMagicDb().getEditions().get(landSet);
        IBoosterDraft.CUSTOM_RANKINGS_FILE[0] = rankings;
    }

    private void start(final ESeat from) {
        synchronized (DRAFT_LOCK) {
            synchronized (this) {
                if (from.index != hostSeat) throw new IllegalStateException("only the host can start");
                if (!"lobby".equals(phase)) throw new IllegalStateException("already started");
                for (final ESeat s : seats) if (s.kind == Seat.Kind.HUMAN && !s.connected()) throw new IllegalStateException(s.name + " is not connected");
                if (seed != null) forge.util.MyRandom.setRandom(new java.util.Random(seed));
                for (final ESeat s : seats) if (s.kind == Seat.Kind.OPEN) { s.kind = Seat.Kind.AI; s.name = "AI Drafter " + (s.index + 1); }
                final List<IUnOpenedProduct> products = new ArrayList<>();
                final int packs;
                if (isCube()) {
                    final CustomLimited cl = CubeStore.limited(source);
                    checkCubeSize(cl, seats.size());
                    final UnOpenedProduct p = new UnOpenedProduct(cl.getSealedProductTemplate(), cl.getCardPool());
                    p.setLimitedPool(true); // every card of the cube exists once per copy across all packs
                    packs = kind == Kind.SEALED ? sealedPacks(cl) : cl.getNumPacks();
                    for (int i = 0; i < packs; i++) products.add(p);
                    landSet = cl.getLandSetCode();
                    rankings = cl.getCustomRankingsFileName();
                } else {
                    final SealedTemplate tpl = FModel.getMagicDb().getBoosters().get(FModel.getMagicDb().getEditions().get(source).getCode());
                    packs = kind == Kind.SEALED ? 6 : 3;
                    for (int i = 0; i < packs; i++) products.add(new UnOpenedProduct(tpl));
                    final CardEdition ed = FModel.getMagicDb().getEditions().get(source);
                    landSet = ed != null && ed.isLargeSet() ? ed.getCode() : null;
                    rankings = null;
                }
                rounds = packs;
                statics();
                if (kind == Kind.SEALED) {
                    for (final ESeat s : seats) {
                        for (final IUnOpenedProduct p : products) for (final PaperCard pc : p.get()) s.pool.add(new PoolCard(nextCardId++, pc));
                        s.picks = s.pool.size();
                        if (s.kind == Seat.Kind.AI) s.deck = aiSealedDeck(s);
                    }
                    phase = "building";
                } else {
                    draft = new ArenaDraft(seats.size(), products);
                    final Set<Integer> humans = new HashSet<>();
                    for (final ESeat s : seats) if (s.kind == Seat.Kind.HUMAN) humans.add(s.index);
                    draft.setHumanSeats(humans);
                    for (final ESeat s : seats) draft.getAllPlayers().get(s.index).setName(s.name);
                    draft.initializeBoosters();
                    captureInitialPackSize();
                    phase = "drafting";
                    advance();
                }
                broadcast();
            }
        }
    }

    private Deck aiSealedDeck(final ESeat s) {
        final List<PaperCard> cards = new ArrayList<>();
        for (final PoolCard c : s.pool) cards.add(c.card());
        final Deck d = new SealedDeckBuilder(cards).buildDeck(landSet);
        d.setName(s.name + "'s deck");
        return d;
    }

    // ---- drafting --------------------------------------------------------------------------------

    private void captureInitialPackSize() {
        for (final LimitedPlayer pl : draft.getAllPlayers()) {
            final DraftPack p = pl.nextChoice();
            if (p != null && !p.isEmpty()) { initialPackSize = p.size(); return; }
        }
    }

    /** Pack passes left (to seat+1) in odd rounds, right in even ones. */
    private void passToNext(final int fromSeat, final DraftPack pack) {
        final int n = draft.getAllPlayers().size();
        final int dir = draft.getRound() % 2 == 1 ? 1 : -1;
        draft.getAllPlayers().get(((fromSeat + dir) % n + n) % n).receiveOpenedPack(pack);
    }

    private void pickAndPass(final ESeat seat, final LimitedPlayer p, final PaperCard card) {
        final Boolean pass = p.draftCard(card, DeckSection.Sideboard);
        seat.picks++;
        if (seat.kind == Seat.Kind.HUMAN) seat.pool.add(new PoolCard(nextCardId++, card));
        if (!Boolean.FALSE.equals(pass)) {
            final DraftPack passed = p.passPack();
            if (passed != null && !passed.isEmpty()) passToNext(seat.index, passed);
        }
    }

    /** AI seats pick whenever they hold a pack; rounds advance when every queue is empty. Under DRAFT_LOCK + this. */
    private void advance() {
        statics();
        while ("drafting".equals(phase)) {
            if (draft.isRoundOver()) {
                if (!draft.startRound()) { finishDraft(); return; }
                captureInitialPackSize();
                continue;
            }
            boolean progressed = false;
            final long now = System.currentTimeMillis();
            for (final ESeat s : seats) {
                final LimitedPlayer p = draft.getAllPlayers().get(s.index);
                final DraftPack head = p.nextChoice();
                if (head == null || head.isEmpty()) continue;
                final boolean auto = s.kind == Seat.Kind.HUMAN && !s.connected() && s.goneSince != 0 && now - s.goneSince > PICK_GRACE_MS;
                if (p instanceof LimitedPlayerAI ai) {
                    if (p.shouldSkipThisPick()) {
                        final DraftPack sk = p.passPack();
                        if (sk != null && !sk.isEmpty()) passToNext(s.index, sk);
                    } else {
                        final PaperCard c = ai.chooseCard();
                        if (c == null) continue;
                        pickAndPass(s, p, c);
                    }
                    progressed = true;
                    break;
                }
                if (auto) { pickAndPass(s, p, head.get(0)); progressed = true; break; }
            }
            if (!progressed) return; // waiting on humans
        }
    }

    private void pick(final ESeat from, final Integer pickId) {
        if (pickId == null) throw new IllegalArgumentException("pick needs an id");
        synchronized (DRAFT_LOCK) {
            synchronized (this) {
                if (!"drafting".equals(phase)) throw new IllegalStateException("not drafting");
                statics();
                final LimitedPlayer p = draft.getAllPlayers().get(from.index);
                final DraftPack head = p.nextChoice();
                if (head == null || head.getId() != pickId / 1000 || pickId % 1000 >= head.size()) throw new IllegalStateException("that pack has moved on");
                pickAndPass(from, p, head.get(pickId % 1000));
                advance();
                broadcast();
            }
        }
    }

    private void finishDraft() {
        for (final ESeat s : seats) {
            if (s.kind != Seat.Kind.AI) continue;
            final LimitedPlayer p = draft.getAllPlayers().get(s.index);
            try {
                s.deck = ((LimitedPlayerAI) p).buildDeck(landSet);
            } catch (final RuntimeException e) {
                // Forge's builder insists on exactly 40; fall back to the sealed builder over its picks
                final List<PaperCard> cards = new ArrayList<>(p.getDeck().getOrCreate(DeckSection.Sideboard).toFlatList());
                s.deck = new SealedDeckBuilder(cards).buildDeck(landSet);
            }
            s.deck.setName(s.name + "'s deck");
            s.pool.clear();
            for (final PaperCard pc : p.getDeck().getOrCreate(DeckSection.Sideboard).toFlatList()) s.pool.add(new PoolCard(nextCardId++, pc));
        }
        phase = "building";
    }

    // ---- deck building -----------------------------------------------------------------------------

    private static final Map<String, String> BASICS = new LinkedHashMap<>();
    static { BASICS.put("W", "Plains"); BASICS.put("U", "Island"); BASICS.put("B", "Swamp"); BASICS.put("R", "Mountain"); BASICS.put("G", "Forest"); }

    private void submit(final ESeat from, final JsonObject d) {
        synchronized (this) {
            if (!"building".equals(phase) && !"ready".equals(phase)) throw new IllegalStateException("decks are built after the draft");
            final Map<Integer, PoolCard> byId = new LinkedHashMap<>();
            for (final PoolCard c : from.pool) byId.put(c.id(), c);
            final Deck deck = new Deck(from.name + "'s " + sourceName + " deck");
            final CardPool main = deck.getMain();
            final Set<Integer> used = new HashSet<>();
            if (d.has("main")) for (final JsonElement e : d.getAsJsonArray("main")) {
                final PoolCard c = byId.get(e.getAsInt());
                if (c == null || !used.add(c.id())) throw new IllegalArgumentException("card " + e + " is not in your pool");
                main.add(c.card());
            }
            if (d.has("basics") && d.get("basics").isJsonObject()) {
                for (final Map.Entry<String, JsonElement> e : d.getAsJsonObject("basics").entrySet()) {
                    final String land = BASICS.get(e.getKey().toUpperCase());
                    final int n = e.getValue().getAsInt();
                    if (land == null || n < 0 || n > 60) throw new IllegalArgumentException("bad basic land entry " + e.getKey());
                    if (n == 0) continue;
                    if (landSet != null && FModel.getMagicDb().getCommonCards().getCard(land, landSet) != null) main.add(land, landSet, n);
                    else main.add(land, n);
                }
            }
            if (main.countAll() < MIN_DECK) throw new IllegalArgumentException("a limited deck needs at least " + MIN_DECK + " cards (" + main.countAll() + ")");
            final CardPool side = deck.getOrCreate(DeckSection.Sideboard);
            for (final PoolCard c : from.pool) if (!used.contains(c.id())) side.add(c.card());
            final String problem = forge.deck.DeckFormat.Limited.getDeckConformanceProblem(deck);
            if (problem != null) throw new IllegalArgumentException(problem);
            from.deck = deck;
            from.deckJson = d.deepCopy();
            boolean all = true;
            for (final ESeat s : seats) if (s.kind == Seat.Kind.HUMAN && s.deck == null) all = false;
            if (all) phase = "ready";
            broadcast();
        }
    }

    /** Forge's sealed deck builder over the seat's pool, mapped back to pool ids + basic counts. */
    private void suggest(final ESeat from, final Session so) {
        final Deck built;
        synchronized (DRAFT_LOCK) {
            statics();
            final List<PaperCard> cards = new ArrayList<>();
            synchronized (this) { for (final PoolCard c : from.pool) cards.add(c.card()); }
            if (cards.isEmpty()) throw new IllegalStateException("nothing to build from yet");
            built = new SealedDeckBuilder(cards).buildDeck(landSet);
        }
        final JsonObject deck = new JsonObject();
        final JsonArray main = new JsonArray();
        final JsonObject basics = new JsonObject();
        final List<PoolCard> left;
        synchronized (this) { left = new ArrayList<>(from.pool); }
        for (final Map.Entry<PaperCard, Integer> e : built.getMain()) {
            final String letter = BASICS.entrySet().stream().filter(b -> b.getValue().equals(e.getKey().getName())).map(Map.Entry::getKey).findFirst().orElse(null);
            for (int i = 0; i < e.getValue(); i++) {
                PoolCard hit = null;
                for (final PoolCard c : left) if (c.card().getName().equals(e.getKey().getName())) { hit = c; break; }
                if (hit != null) { left.remove(hit); main.add(hit.id()); }
                else if (letter != null) basics.addProperty(letter, (basics.has(letter) ? basics.get(letter).getAsInt() : 0) + 1);
            }
        }
        deck.add("main", main);
        deck.add("basics", basics);
        final JsonObject f = Json.frame("deckSuggestion");
        f.add("deck", deck);
        send(so, f);
    }

    // ---- dealing games -------------------------------------------------------------------------------

    private void deal(final ESeat from, final String pairing, final Integer tableSize0) {
        synchronized (this) {
            if (from.index != hostSeat) throw new IllegalStateException("only the host deals games");
            if (!"ready".equals(phase)) throw new IllegalStateException("waiting for every player's deck");
            final List<ESeat> humans = new ArrayList<>(), ais = new ArrayList<>();
            for (final ESeat s : seats) (s.kind == Seat.Kind.HUMAN ? humans : ais).add(s);
            final List<List<ESeat>> tables = new ArrayList<>();
            if ("duels".equals(pairing)) {
                for (int i = 0; i < humans.size(); i += 2) {
                    final List<ESeat> t = new ArrayList<>();
                    t.add(humans.get(i));
                    if (i + 1 < humans.size()) t.add(humans.get(i + 1));
                    else if (!ais.isEmpty()) t.add(ais.get(i / 2 % ais.size()));
                    else throw new IllegalStateException("odd player out and no AI drafter to face");
                    tables.add(t);
                }
            } else {
                final int size = Math.max(Math.max(2, humans.size()), Math.min(MatchSession.MAX_SEATS, tableSize0 == null ? 2 : tableSize0));
                if (humans.size() > MatchSession.MAX_SEATS) throw new IllegalStateException("at most 4 players per table: use duels");
                final List<ESeat> t = new ArrayList<>(humans);
                for (int i = 0; t.size() < size && i < ais.size(); i++) t.add(ais.get(i));
                if (t.size() < 2) throw new IllegalStateException("need two players");
                tables.add(t);
            }
            for (final List<ESeat> t : tables) {
                final List<MatchSession.EventPlayer> ps = new ArrayList<>();
                for (final ESeat s : t) ps.add(new MatchSession.EventPlayer(s.name, s.deck, s.kind == Seat.Kind.HUMAN));
                final String mid = mgr.newCode();
                final JsonObject g = new JsonObject();
                g.addProperty("matchId", mid);
                final JsonArray names = new JsonArray();
                for (final ESeat s : t) names.add(s.name);
                g.add("players", names);
                g.addProperty("over", false);
                final MatchSession[] holder = new MatchSession[1];
                final MatchSession m = MatchSession.event(mid, ps, () -> {
                    synchronized (EventSession.this) {
                        g.addProperty("over", true);
                        g.addProperty("winner", holder[0] == null ? null : holder[0].winnerName());
                        broadcast();
                    }
                });
                holder[0] = m;
                mgr.register(m);
                games.add(g);
                for (int i = 0; i < t.size(); i++) {
                    final ESeat s = t.get(i);
                    if (s.kind != Seat.Kind.HUMAN) continue;
                    final JsonObject f = Json.frame("eventMatch");
                    f.addProperty("id", mid);
                    f.addProperty("seat", i);
                    f.addProperty("token", m.tokenOf(i));
                    f.addProperty("name", s.name);
                    f.addProperty("format", DeckSource.LIMITED);
                    for (final Session so : s.sockets) send(so, f);
                }
            }
            broadcast();
        }
    }

    // ---- reaper ----------------------------------------------------------------------------------------

    /** Called every couple of seconds: picks for drafters who left (after the grace period). */
    void tick(final long now) {
        if (!"drafting".equals(phase)) return;
        boolean due = false;
        synchronized (this) {
            for (final ESeat s : seats) if (s.kind == Seat.Kind.HUMAN && !s.connected() && s.goneSince != 0 && now - s.goneSince > PICK_GRACE_MS) due = true;
        }
        if (!due) return;
        synchronized (DRAFT_LOCK) { synchronized (this) { if ("drafting".equals(phase)) { advance(); broadcast(); } } }
    }

    // ---- views -----------------------------------------------------------------------------------------

    private JsonObject card(final int id, final PaperCard pc) {
        final JsonObject c = CardCatalog.get().cardJson(pc);
        c.addProperty("id", id);
        c.addProperty("qty", 1);
        return c;
    }

    synchronized JsonObject frame(final ESeat viewer) {
        final JsonObject f = Json.frame("event");
        describeInto(f, viewer);
        return f;
    }

    synchronized JsonObject describe() { final JsonObject o = new JsonObject(); describeInto(o, null); return o; }

    private void describeInto(final JsonObject f, final ESeat viewer) {
        f.addProperty("id", id);
        f.addProperty("kind", switch (kind) { case CUBE_DRAFT -> "cubeDraft"; case SET_DRAFT -> "setDraft"; case SEALED -> "sealed"; });
        f.addProperty("source", source);
        f.addProperty("sourceName", sourceName);
        f.addProperty("phase", phase);
        f.addProperty("host", hostSeat);
        if (viewer != null) f.addProperty("you", viewer.index);
        f.addProperty("invite", invite());
        f.addProperty("minDeck", MIN_DECK);
        final boolean drafting = "drafting".equals(phase) && draft != null;
        f.addProperty("round", drafting ? draft.getRound() : 0);
        f.addProperty("rounds", rounds);
        f.addProperty("passLeft", drafting && draft.getRound() % 2 == 1);
        final JsonArray ss = new JsonArray();
        boolean canStart = "lobby".equals(phase);
        for (final ESeat s : seats) {
            final JsonObject o = new JsonObject();
            o.addProperty("seat", s.index);
            o.addProperty("kind", s.kind.name().toLowerCase());
            o.addProperty("name", s.name);
            o.addProperty("connected", s.kind == Seat.Kind.AI || s.connected());
            o.addProperty("you", s == viewer);
            o.addProperty("picked", s.picks);
            boolean thinking = false;
            if (drafting) { final DraftPack h = draft.getAllPlayers().get(s.index).nextChoice(); thinking = h != null && !h.isEmpty(); }
            o.addProperty("thinking", thinking);
            o.addProperty("deckReady", s.deck != null);
            ss.add(o);
            if (s.kind == Seat.Kind.HUMAN && !s.connected()) canStart = false;
        }
        f.add("seats", ss);
        f.addProperty("canStart", canStart);
        final JsonArray pack = new JsonArray(), pool = new JsonArray();
        int pick = 0;
        if (viewer != null) {
            if (drafting) {
                final DraftPack head = draft.getAllPlayers().get(viewer.index).nextChoice();
                if (head != null) {
                    for (int i = 0; i < head.size(); i++) pack.add(card(head.getId() * 1000 + i, head.get(i)));
                    pick = Math.max(1, initialPackSize - head.size() + 1);
                }
            }
            for (final PoolCard c : viewer.pool) pool.add(card(c.id(), c.card()));
            if (viewer.deckJson != null) f.add("deck", viewer.deckJson);
        }
        f.addProperty("pick", pick);
        f.add("pack", pack);
        f.add("pool", pool);
        final JsonArray gs = new JsonArray();
        for (final JsonObject g : games) gs.add(g);
        f.add("games", gs);
    }

    private void broadcast() {
        for (final ESeat s : seats) {
            if (s.sockets.isEmpty()) continue;
            final String text = Json.GSON.toJson(frame(s));
            for (final Session so : s.sockets) sendText(so, text);
        }
    }

    private void send(final Session s, final JsonObject f) { sendText(s, Json.GSON.toJson(f)); }
    private void sendText(final Session s, final String text) {
        try { if (s.isOpen()) s.getRemote().sendStringByFuture(text); } catch (final Exception e) { System.err.println("[event " + id + "] send failed: " + e); }
    }
}
