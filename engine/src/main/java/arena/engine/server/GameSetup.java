package arena.engine.server;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import forge.deck.CardPool;
import forge.deck.Deck;
import forge.deck.DeckSection;
import forge.deck.DeckgenUtil;
import forge.game.GameType;
import forge.game.player.RegisteredPlayer;
import forge.item.PaperCard;

import java.util.EnumSet;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Set;

/**
 * What kind of game a match is: format (deck construction), Constructed legality (card pool), and casual
 * variants — and how that becomes Forge's {@link RegisteredPlayer}s.
 *
 * <p>Why one class: Forge's own lobby (GameLobby#startGame) starts every variant as a base GameType plus a
 * set of applied variants and builds each player with {@link RegisteredPlayer#forVariants}; the variant
 * rules (life totals, command zone, planar/scheme decks, Momir avatar) all key off the applied set. Keeping
 * the mapping, the exclusions and the per-seat extras here means MatchSession just asks for players.
 *
 * <p>"teams" is honest team play (2v2: a team wins together). Forge has no Two-Headed Giant rules —
 * no shared life or shared turns — so teammates keep their own.
 */
final class GameSetup {
    static final List<String> VARIANTS = List.of("planechase", "archenemy", "momir", "teams");

    final String format;
    /** Constructed only; null = casual. */
    final String legality;
    final Set<String> variants;

    private GameSetup(final String format, final String legality, final Set<String> variants) {
        this.format = format;
        this.legality = legality;
        this.variants = variants;
    }

    static GameSetup of(final String format) { return of(format, null, null); }

    /** Validate and normalise; throws IllegalArgumentException with a user-readable reason. */
    static GameSetup of(final String format0, final String legality0, final JsonArray variants0) {
        String format = DeckSource.format(format0);
        String legality = DeckSource.legality(legality0);
        final Set<String> vs = new LinkedHashSet<>();
        if (variants0 != null) for (final JsonElement e : variants0) {
            final String v = e.getAsString().trim();
            final String id = v.equalsIgnoreCase("twoHeadedGiant") ? "teams" : v.toLowerCase();
            if (!VARIANTS.contains(id)) throw new IllegalArgumentException("unknown variant: " + v + " " + VARIANTS);
            vs.add(id);
        }
        if (vs.contains("archenemy") && vs.contains("teams")) throw new IllegalArgumentException("archenemy and teams both pick sides: choose one");
        if (vs.contains("momir")) {
            // Momir replaces the decklist: commander formats and legalities don't apply
            if (DeckSource.hasCommander(format)) throw new IllegalArgumentException("Momir Basic is played with generated 60-card decks, not " + format);
            if (DeckSource.LIMITED.equals(format)) throw new IllegalArgumentException("Momir Basic can't use limited decks");
            legality = null;
        }
        if (!DeckSource.CONSTRUCTED.equals(format)) legality = null;
        return new GameSetup(format, legality, vs);
    }

    static GameSetup fromJson(final JsonObject b) {
        return of(Json.str(b, "format"), Json.str(b, "legality"), b.has("variants") && b.get("variants").isJsonArray() ? b.getAsJsonArray("variants") : null);
    }

    boolean decksIgnored() { return variants.contains("momir"); }
    boolean limited() { return DeckSource.LIMITED.equals(format); }

    /** Seat count rules of the variants; null when fine. */
    String playerProblem(final int players) {
        if (variants.contains("teams") && players != 4) return "2v2 teams need exactly 4 players (" + players + " seated)";
        return null;
    }

    /** Base type: decides sideboarding between games; the variants decide the rules. */
    GameType baseType() {
        return switch (format) {
            case DeckSource.COMMANDER -> GameType.Commander;
            case DeckSource.BRAWL -> GameType.Brawl;
            case DeckSource.OATHBREAKER -> GameType.Oathbreaker;
            case DeckSource.LIMITED -> GameType.Draft;
            default -> GameType.Constructed;
        };
    }

    Set<GameType> applied() {
        final Set<GameType> s = EnumSet.noneOf(GameType.class);
        switch (format) {
            case DeckSource.COMMANDER -> s.add(GameType.Commander);
            case DeckSource.BRAWL -> s.add(GameType.Brawl);
            case DeckSource.OATHBREAKER -> s.add(GameType.Oathbreaker);
            default -> {}
        }
        if (variants.contains("planechase")) s.add(GameType.Planechase);
        if (variants.contains("archenemy")) s.add(GameType.Archenemy);
        if (variants.contains("momir")) s.add(GameType.MomirBasic);
        return s;
    }

    /**
     * One player at the table. {@code order} is the seat's position among the filled seats (0 = first,
     * the host's seat in a lobby): the archenemy is seat order 0, teams are even vs. odd seats.
     */
    RegisteredPlayer register(final Deck deck0, final int players, final int order) {
        final Set<GameType> applied = applied();
        final Deck deck = decksIgnored() ? GameType.MomirBasic.autoGenerateDeck(null) : deck0;
        final boolean archenemy = variants.contains("archenemy") && order == 0;
        final List<PaperCard> schemes = archenemy ? schemePool() : null;
        final List<PaperCard> planes = variants.contains("planechase") ? DeckgenUtil.generatePlanarPool().toFlatList() : null;
        final CardPool avatar = decksIgnored() ? deck.get(DeckSection.Avatar) : null;
        final RegisteredPlayer rp = applied.isEmpty() ? new RegisteredPlayer(deck)
                : RegisteredPlayer.forVariants(players, applied, deck, schemes, archenemy, planes, avatar);
        if (limited()) rp.assignConspiracies();
        if (variants.contains("teams")) rp.setTeamNumber(order % 2);
        else if (variants.contains("archenemy")) rp.setTeamNumber(archenemy ? 0 : 1);
        return rp;
    }

    /**
     * Schemes that set themselves "in motion again" read Player#getActiveScheme, which Forge 2.0.14 never
     * assigns: the game thread dies with an NPE the first time one resolves. Leave them out.
     */
    private static final Set<String> BROKEN_SCHEMES = Set.of("My Laughter Echoes");

    /** Like DeckgenUtil.generateSchemePool (20 schemes, at most 2 of each) minus {@link #BROKEN_SCHEMES}. */
    private static List<PaperCard> schemePool() {
        final List<PaperCard> all = new java.util.ArrayList<>();
        for (final PaperCard pc : forge.model.FModel.getArchenemyCards().toFlatList()) if (!BROKEN_SCHEMES.contains(pc.getName())) all.add(pc);
        final CardPool pool = new CardPool();
        int left = 20, attempts = 200;
        while (left > 0 && attempts-- > 0 && !all.isEmpty()) {
            final PaperCard pc = all.get(forge.util.MyRandom.getRandom().nextInt(all.size()));
            if (pool.count(pc) < 2) { pool.add(pc); left--; }
        }
        return pool.toFlatList();
    }

    /** Where to load a seat's deck from, honouring the legality. */
    Deck loadDeck(final String ref) {
        if (decksIgnored()) return GameType.MomirBasic.autoGenerateDeck(null);
        return DeckSource.load(ref, format, legality);
    }

    JsonObject toJson() {
        final JsonObject o = new JsonObject();
        o.addProperty("format", format);
        o.addProperty("legality", legality);
        final JsonArray a = new JsonArray();
        for (final String v : variants) a.add(v);
        o.add("variants", a);
        return o;
    }

    /** Copy the setup fields onto a frame / description (format, legality, variants). */
    void fill(final JsonObject o) {
        for (final var e : toJson().entrySet()) o.add(e.getKey(), e.getValue());
    }
}
