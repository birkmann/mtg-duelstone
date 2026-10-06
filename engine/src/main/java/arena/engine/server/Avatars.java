package arena.engine.server;

import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonPrimitive;
import forge.card.CardRules;
import forge.card.CardType;
import forge.deck.Deck;
import forge.item.PaperCard;

import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * Player portraits: {@code {card, x, y, zoom}} — a card whose art crop is the picture, the centre of the
 * circular crop in that art (0..1) and how far it is zoomed in (1 = the circle spans the art's shorter side).
 *
 * <p>Clients draw avatars; the engine only stores and relays them, so all it has to guarantee is that what
 * one player sent can't break another player's client (wrong types, absurd numbers, megabyte strings) —
 * hence {@link #clean}. Seats that never chose one (AI seats, older clients) still deserve a face at the
 * table, and the most recognisable one their deck has is its commander or its marquee legend — hence
 * {@link #fromDeck}.
 */
final class Avatars {
    private Avatars() {}

    static final double DEFAULT_X = 0.5, DEFAULT_Y = 0.4;
    private static final int MAX_CARD = 200;

    /** A sanitized copy with exactly the four fields, or null if {@code e} isn't a usable avatar. */
    static JsonObject clean(final JsonElement e) {
        if (e == null || !e.isJsonObject()) return null;
        final JsonObject in = e.getAsJsonObject();
        final JsonElement c = in.get("card");
        if (c == null || !c.isJsonPrimitive() || !c.getAsJsonPrimitive().isString()) return null;
        final String card = c.getAsString().strip();
        if (card.isEmpty() || card.length() > MAX_CARD) return null;
        return make(card, num(in, "x", DEFAULT_X, 0, 1), num(in, "y", DEFAULT_Y, 0, 1), num(in, "zoom", 1, 1, 4));
    }

    private static double num(final JsonObject o, final String k, final double dflt, final double lo, final double hi) {
        final JsonElement e = o.get(k);
        if (e == null || !e.isJsonPrimitive() || !e.getAsJsonPrimitive().isNumber()) return dflt;
        final double v = e.getAsDouble();
        if (Double.isNaN(v)) return dflt;
        return Math.max(lo, Math.min(hi, v));
    }

    private static JsonObject make(final String card, final double x, final double y, final double zoom) {
        final JsonObject o = new JsonObject();
        o.addProperty("card", card);
        o.add("x", new JsonPrimitive(x));
        o.add("y", new JsonPrimitive(y));
        o.add("zoom", new JsonPrimitive(zoom));
        return o;
    }

    /**
     * A portrait for a seat that didn't pick one: the commander, else a planeswalker, else a legendary
     * creature, else the biggest creature of the main deck; null for a deck without any of those.
     * Never throws — a missing face must not stop a game from starting.
     */
    static JsonObject fromDeck(final Deck d) {
        if (d == null) return null;
        try {
            final List<PaperCard> cmd = d.getCommanders();
            if (cmd != null && !cmd.isEmpty() && cmd.get(0) != null) return portrait(cmd.get(0).getName());
            // A deck may hold several printings of one card; copies count per name.
            final Map<String, Integer> copies = new LinkedHashMap<>();
            final Map<String, CardRules> rules = new LinkedHashMap<>();
            for (final Map.Entry<PaperCard, Integer> en : d.getMain()) {
                final PaperCard pc = en.getKey();
                if (pc == null || pc.getRules() == null) continue;
                copies.merge(pc.getName(), en.getValue(), Integer::sum);
                rules.putIfAbsent(pc.getName(), pc.getRules());
            }
            String best = null;
            int bestTier = 0, bestCopies = 0, bestCmc = 0;
            for (final Map.Entry<String, CardRules> en : rules.entrySet()) {
                final CardType t = en.getValue().getType();
                final int tier = t.isPlaneswalker() ? 3 : t.isLegendary() && t.isCreature() ? 2 : t.isCreature() ? 1 : 0;
                if (tier == 0) continue;
                final String name = en.getKey();
                final int n = copies.get(name);
                final int cmc = en.getValue().getManaCost() == null ? 0 : en.getValue().getManaCost().getCMC();
                if (best == null || better(tier, n, cmc, name, bestTier, bestCopies, bestCmc, best)) {
                    best = name; bestTier = tier; bestCopies = n; bestCmc = cmc;
                }
            }
            return best == null ? null : portrait(best);
        } catch (final RuntimeException e) {
            return null;
        }
    }

    /**
     * Legends are chosen by how much the deck leans on them (copies first); plain creatures by size first,
     * because a 4-of bear says less about a deck than its one big finisher. Name breaks ties so the same
     * deck always gets the same face.
     */
    private static boolean better(final int tier, final int n, final int cmc, final String name,
                                  final int bTier, final int bN, final int bCmc, final String bName) {
        if (tier != bTier) return tier > bTier;
        final int[] a = tier == 1 ? new int[] { cmc, n } : new int[] { n, cmc };
        final int[] b = tier == 1 ? new int[] { bCmc, bN } : new int[] { bN, bCmc };
        for (int i = 0; i < 2; i++) if (a[i] != b[i]) return a[i] > b[i];
        return name.compareTo(bName) < 0;
    }

    /** Faces in card art usually sit in the upper middle, so a derived portrait aims a little higher and closer than the defaults. */
    private static JsonObject portrait(final String card) { return make(card, 0.5, 0.3, 1.5); }
}
