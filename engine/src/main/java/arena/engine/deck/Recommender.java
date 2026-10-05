package arena.engine.deck;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import forge.card.CardType;
import forge.deck.CardArchetypeLDAGenerator;
import forge.deck.CardRelationMatrixGenerator;
import forge.deck.io.Archetype;
import forge.deck.io.CardThemedLDAIO;
import forge.deck.io.CardThemedMatrixIO;
import forge.item.PaperCard;
import org.apache.commons.lang3.tuple.Pair;

import java.util.ArrayList;
import java.util.Comparator;
import java.util.HashMap;
import java.util.HashSet;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Set;
import java.util.concurrent.ConcurrentHashMap;
import java.util.function.Predicate;
import java.util.function.ToDoubleFunction;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

/**
 * EDHREC-style deck-builder suggestions from data Forge already ships offline in {@code res/deckgendecks}:
 * <ul>
 *   <li>{@code Commander.dat} — per commander, how many scraped decks ran each card. Inclusion = count / decks,
 *       synergy = inclusion − the card's mean inclusion over every commander that could play it (EDHREC's definition).</li>
 *   <li>{@code <Format>.raw.dat} — LDA archetypes of 60-card formats (card probability per archetype + deck count).
 *       A constructed deck is matched to archetypes and their cards become the suggestions.</li>
 * </ul>
 * Everything else (themes, roles, lands) comes from {@link CardTags}. The data is loaded once, lazily (or by
 * {@link #warmUpAsync()} at server start), and is immutable afterwards, so requests need no locking. Missing data
 * files only degrade suggestions to tag + colour based ones.
 */
public final class Recommender {
    private static volatile Recommender instance;

    /** Blocks on the first call while the data loads (a few seconds); cheap afterwards. */
    public static Recommender get() {
        Recommender r = instance;
        if (r == null) synchronized (Recommender.class) { r = instance; if (r == null) instance = r = new Recommender(); }
        return r;
    }

    /** Kick the load off the request path; the server calls this right after start. */
    public static void warmUpAsync() {
        final Thread t = new Thread(() -> { try { get(); } catch (final Throwable e) { System.err.println("[recommend] load failed: " + e); e.printStackTrace(); } }, "recommender-load");
        t.setDaemon(true);
        t.start();
    }

    /** 0..1 rank-normalised popularity; 0 while the data is not loaded yet (never forces the load). */
    public static double popularity(final String name) {
        final Recommender r = instance;
        if (r == null || name == null) return 0;
        final Card c = r.byName.get(name);
        return c == null ? 0 : c.pop;
    }

    public static boolean isLoaded() { return instance != null; }

    // ---- data -------------------------------------------------------------------------------------------

    /** Per-card facts precomputed once so a request is a single pass over plain fields. */
    private static final class Card {
        final PaperCard pc; final String name; final String text; final byte ci; final int tags;
        final boolean land, basic, creature, banned; final Set<String> ctypes;
        double base;      // commander: mean inclusion over commanders whose identity covers this card
        double baseCon;   // constructed: deck-weighted presence over all archetypes
        double popCmd, popCon, pop;
        Card(final PaperCard pc, final CardCatalog cat) {
            this.pc = pc; name = pc.getName();
            final CardType t = pc.getRules().getType();
            text = CardTags.text(pc.getRules());
            ci = pc.getRules().getColorIdentity().getColor();
            tags = CardTags.mask(pc);
            land = t != null && t.isLand(); basic = t != null && t.isBasicLand(); creature = t != null && t.isCreature();
            banned = cat.isBanned(pc);
            ctypes = t == null ? Set.of() : new HashSet<>(t.getCreatureTypes());
        }
    }

    private record Cmdr(String name, byte ci, int decks, Map<String, Double> inc) {}

    /** {@code q} = p / max p, i.e. "share of this archetype's decks running the card" with the signature card at 1. */
    private record Arch(String name, String format, int decks, Map<String, Double> p, Map<String, Double> q, double norm) {}

    private static final String[] LDA_FORMATS = {"Standard", "Pioneer", "Historic", "Modern", "Pauper", "Legacy", "Vintage"};

    private final CardCatalog catalog = CardCatalog.get();
    private final List<Card> cards = new ArrayList<>();
    private final Map<String, Card> byName = new HashMap<>();
    private final Map<String, Cmdr> commanders = new HashMap<>();
    private final List<Arch> archetypes = new ArrayList<>();
    private final double[] tagShare = new double[CardTags.keys().size()];   // share of all non-basic cards with each tag
    private final Map<String, String> typeWords = new HashMap<>();          // "elves"/"elf" → "Elf"
    private final Pattern typeWordRx;
    private final Map<String, Double> tribalShare = new ConcurrentHashMap<>();
    private final Map<String, List<String>> synergyCache = new ConcurrentHashMap<>();

    private Recommender() {
        final long t0 = System.currentTimeMillis();
        for (final PaperCard pc : catalog.all()) { final Card c = new Card(pc, catalog); cards.add(c); byName.put(c.name, c); }
        int nonBasic = 0;
        final int[] tagCount = new int[tagShare.length];
        for (final Card c : cards) {
            if (c.basic) continue;
            nonBasic++;
            for (int i = 0; i < tagCount.length; i++) if ((c.tags & (1 << i)) != 0) tagCount[i]++;
        }
        for (int i = 0; i < tagCount.length; i++) tagShare[i] = nonBasic == 0 ? 0 : (double) tagCount[i] / nonBasic;
        final List<String> words = new ArrayList<>();
        for (final String ty : CardType.getAllCreatureTypes()) {
            for (final String w : List.of(ty, CardType.getPluralType(ty))) { final String lw = w.toLowerCase(Locale.ROOT); typeWords.put(lw, ty); words.add(Pattern.quote(lw)); }
        }
        words.sort((a, b) -> b.length() - a.length());
        typeWordRx = Pattern.compile("\\b(" + String.join("|", words) + ")\\b");
        final long t1 = System.currentTimeMillis();
        loadCommander();
        final long t2 = System.currentTimeMillis();
        loadArchetypes();
        final long t3 = System.currentTimeMillis();
        computePopularity();
        System.out.printf("[recommend] %d cards tagged in %d ms; %d commanders in %d ms; %d archetypes in %d ms; total %d ms%n",
                cards.size(), t1 - t0, commanders.size(), t2 - t1, archetypes.size(), t3 - t2, System.currentTimeMillis() - t0);
    }

    /**
     * Commander.dat counts, per commander, the decks containing each card (Forge's generator increments once per
     * card per deck; basics show up with counts on the same scale as Sol Ring, so they are presence counts, not
     * summed copies). The file does not store how many decks each commander had, so the denominator is the largest
     * count in the list: every deck runs either its colour's basic (mono-colour) or Command Tower / Sol Ring
     * (multicolour), so the max is a close lower bound of the deck count. Verified on the shipped file: the max is
     * always a basic land, Command Tower or Sol Ring-tier staple, never an outlier.
     */
    private void loadCommander() {
        Map<String, List<Map.Entry<PaperCard, Integer>>> raw = CardRelationMatrixGenerator.cardPools.get("Commander");  // FModel may have loaded it already
        if (raw == null) {
            try { if (CardThemedMatrixIO.getMatrixFile("Commander").isFile()) raw = CardThemedMatrixIO.loadMatrix("Commander"); }
            catch (final Throwable e) { System.err.println("[recommend] Commander.dat unreadable: " + e); }
        }
        if (raw == null) { System.out.println("[recommend] no Commander.dat — commander suggestions fall back to tags"); return; }
        for (final var e : raw.entrySet()) {
            final PaperCard cmd = catalog.resolveExact(e.getKey());
            if (cmd == null || e.getValue() == null || e.getValue().isEmpty()) continue;
            int max = 0;
            for (final var x : e.getValue()) max = Math.max(max, x.getValue());
            if (max <= 0) continue;
            final Map<String, Double> inc = new HashMap<>(e.getValue().size() * 2);
            for (final var x : e.getValue()) inc.merge(x.getKey().getName(), (double) x.getValue() / max, Math::max);
            commanders.put(cmd.getName(), new Cmdr(cmd.getName(), cmd.getRules().getColorIdentity().getColor(), max, inc));
        }
        // base(X): mean inclusion over commanders whose identity ⊇ CI(X). A card is only listed for commanders that
        // can run it, so summing inclusions and dividing by the number of eligible commanders gives the mean
        // including the zeros. Partner lists also carry the partner's colours, hence the explicit subset check.
        final int[] eligible = new int[32];
        for (final Cmdr c : commanders.values()) for (int m = 0; m < 32; m++) if ((m & ~c.ci) == 0) eligible[m]++;
        final Map<String, Double> sum = new HashMap<>();
        for (final Cmdr c : commanders.values()) for (final var x : c.inc.entrySet()) {
            final Card card = byName.get(x.getKey());
            if (card != null && (card.ci & ~c.ci) == 0) sum.merge(x.getKey(), x.getValue(), Double::sum);
        }
        for (final var e : sum.entrySet()) { final Card c = byName.get(e.getKey()); final int n = eligible[c.ci & 31]; c.base = n == 0 ? 0 : e.getValue() / n; }
    }

    private void loadArchetypes() {
        Map<String, List<Archetype>> lda = CardArchetypeLDAGenerator.ldaArchetypes;   // pruned copy FModel may have loaded
        if (lda == null || lda.isEmpty()) {
            lda = new LinkedHashMap<>();
            for (final String f : LDA_FORMATS) {
                try { if (CardThemedLDAIO.getRAWLDAFile(f).isFile()) { final List<Archetype> l = CardThemedLDAIO.loadRawLDA(f); if (l != null) lda.put(f, l); } }
                catch (final Throwable e) { System.err.println("[recommend] " + f + ".raw.dat unreadable: " + e); }
            }
        }
        final Map<String, String> canon = new HashMap<>();
        double totalDecks = 0;
        final Map<String, Double> presence = new HashMap<>();
        for (final var e : lda.entrySet()) {
            if (e.getValue() == null) continue;
            for (final Archetype a : e.getValue()) {
                if (a == null || a.getCardProbabilities() == null || a.getCardProbabilities().isEmpty()) continue;
                final Map<String, Double> p = new HashMap<>();
                for (final Pair<String, Double> cp : a.getCardProbabilities()) {
                    final String n = canon.computeIfAbsent(cp.getLeft(), k -> { final PaperCard pc = catalog.resolveExact(k); return pc == null ? "" : pc.getName(); });
                    if (!n.isEmpty() && cp.getRight() > 0) p.merge(n, cp.getRight(), Double::sum);
                }
                if (p.isEmpty()) continue;
                double max = 0, sq = 0;
                for (final double v : p.values()) { max = Math.max(max, v); sq += v * v; }
                final Map<String, Double> q = new HashMap<>();
                for (final var x : p.entrySet()) q.put(x.getKey(), x.getValue() / max);
                final int decks = a.getDeckCount() == null ? 1 : Math.max(1, a.getDeckCount());
                archetypes.add(new Arch(a.getName(), e.getKey(), decks, p, q, Math.sqrt(sq)));
                totalDecks += decks;
                for (final var x : q.entrySet()) presence.merge(x.getKey(), decks * x.getValue(), Double::sum);
            }
        }
        for (final var e : presence.entrySet()) { final Card c = byName.get(e.getKey()); if (c != null) c.baseCon = e.getValue() / totalDecks; }
    }

    /**
     * Popularity = rank percentile, separately for Commander (base inclusion) and constructed (deck-weighted
     * archetype presence), then the max of the two: a card is popular if it is a staple in either world, and rank
     * normalisation keeps the two very differently scaled sources comparable.
     */
    private void computePopularity() {
        rank(c -> c.base, (c, v) -> c.popCmd = v);
        rank(c -> c.baseCon, (c, v) -> c.popCon = v);
        for (final Card c : cards) c.pop = Math.max(c.popCmd, c.popCon);
    }
    private void rank(final ToDoubleFunction<Card> raw, final java.util.function.ObjDoubleConsumer<Card> set) {
        final List<Card> l = new ArrayList<>();
        for (final Card c : cards) if (!c.basic && raw.applyAsDouble(c) > 0) l.add(c);
        l.sort(Comparator.comparingDouble(raw).reversed());
        for (int i = 0; i < l.size(); i++) set.accept(l.get(i), 1.0 - (double) i / l.size());
    }

    // ---- tribal helpers ------------------------------------------------------------------------------------

    private Set<String> mentionedTypes(final String text) {
        final Set<String> out = new LinkedHashSet<>();
        final Matcher m = typeWordRx.matcher(text);
        while (m.find()) out.add(typeWords.get(m.group(1)));
        return out;
    }
    private record TypeRx(String word, Pattern rx) {}
    private final Map<String, TypeRx> typeRx = new ConcurrentHashMap<>();
    /** A card is in a tribe if it has the creature type or its text names it ("other Elves you control"). */
    private boolean tribalMatch(final Card c, final String type) {
        if (c.ctypes.contains(type)) return true;
        final TypeRx t = typeRx.computeIfAbsent(type, k -> {
            final String s = k.toLowerCase(Locale.ROOT), p = CardType.getPluralType(k).toLowerCase(Locale.ROOT);
            // plurals of these types share the singular's stem ("elf"/"elves" share "el"), so prefilter on the shorter prefix
            int i = 0; while (i < Math.min(s.length(), p.length()) && s.charAt(i) == p.charAt(i)) i++;
            return new TypeRx(s.substring(0, Math.max(1, i)), Pattern.compile("\\b(?:" + Pattern.quote(s) + "|" + Pattern.quote(p) + ")\\b"));
        });
        return c.text.contains(t.word) && t.rx.matcher(c.text).find();
    }
    private double tribalShare(final String type) {
        return tribalShare.computeIfAbsent(type, t -> {
            int n = 0, k = 0;
            for (final Card c : cards) { if (c.basic) continue; n++; if (tribalMatch(c, t)) k++; }
            return n == 0 ? 0 : (double) k / n;
        });
    }

    // ---- request ---------------------------------------------------------------------------------------------

    /** Per-request scoring state; {@code inc} is commander inclusion or archetype presence, depending on format. */
    private final class Ctx {
        boolean commander; byte filter = -1; int decks; boolean hasData;
        final Set<String> deck = new HashSet<>();
        final Map<String, Double> inc = new HashMap<>();
        double inc(final Card c) { return inc.getOrDefault(c.name, 0.0); }
        double syn(final Card c) { final double i = inc(c); return i <= 0 ? 0 : i - (commander ? c.base : c.baseCon); }
        double pop(final Card c) { return commander ? Math.max(c.popCmd, c.pop * 0.5) : Math.max(c.popCon, c.pop * 0.5); }
        /** 0..1; any card backed by the commander/archetype data outranks a popularity-only one. */
        double score(final Card c) {
            final double i = inc(c);
            if (i > 0) return 0.3 + 0.7 * Math.min(1, 0.6 * i + 0.4 * Math.max(0, syn(c)));
            return 0.3 * pop(c);
        }
        boolean eligible(final Card c) {
            return !c.basic && !deck.contains(c.name) && (filter < 0 || (c.ci & ~filter) == 0) && !(commander && c.banned);
        }
    }

    public JsonObject recommend(final JsonObject req) {
        final String fmtIn = str(req, "format");
        final Ctx x = new Ctx();
        x.commander = fmtIn == null || fmtIn.isBlank() || !fmtIn.trim().toLowerCase(Locale.ROOT).startsWith("constructed");
        final int limit = Math.max(1, Math.min(60, req.has("limit") && !req.get("limit").isJsonNull() ? req.get("limit").getAsInt() : 24));

        // ---- resolve the deck
        final List<Card> cmdCards = new ArrayList<>(), deckCards = new ArrayList<>();
        final JsonArray unresolved = new JsonArray();
        if (x.commander && req.has("commanders") && req.get("commanders").isJsonArray()) for (final JsonElement e : req.getAsJsonArray("commanders")) {
            final Card c = resolve(e.isJsonObject() ? str(e.getAsJsonObject(), "name") : e.getAsString());
            if (c == null) unresolved.add(e.isJsonObject() ? str(e.getAsJsonObject(), "name") : e.getAsString());
            else if (x.deck.add(c.name)) cmdCards.add(c);
        }
        if (req.has("main") && req.get("main").isJsonArray()) for (final JsonElement e : req.getAsJsonArray("main")) {
            final String n = e.isJsonObject() ? str(e.getAsJsonObject(), "name") : e.getAsString();
            final Card c = resolve(n);
            if (c == null) { unresolved.add(n); continue; }
            if (x.deck.add(c.name) && !c.basic) deckCards.add(c);
        }
        final List<String> requested = new ArrayList<>();
        if (req.has("themes") && req.get("themes").isJsonArray()) for (final JsonElement e : req.getAsJsonArray("themes")) {
            final String t = e.getAsString().trim();
            if (t.startsWith("tribal:") && t.length() > 7) requested.add("tribal:" + Character.toUpperCase(t.charAt(7)) + t.substring(8));
            else if (CardTags.isKnown(t.toLowerCase(Locale.ROOT)) && !CardTags.isRole(t.toLowerCase(Locale.ROOT))) requested.add(t.toLowerCase(Locale.ROOT));
        }

        // ---- colour filter
        final String colors = str(req, "colors");
        if (x.commander && !cmdCards.isEmpty()) { byte m = 0; for (final Card c : cmdCards) m |= c.ci; x.filter = m; }
        else if (colors != null && !colors.isBlank()) x.filter = CardCatalog.colorMask(colors);
        else if (!deckCards.isEmpty()) { byte m = 0; for (final Card c : deckCards) if (!c.land) m |= c.ci; x.filter = m; }

        // ---- data-backed inclusion
        final List<Card> synCards = new ArrayList<>();   // the data's strongest picks, for theme detection
        final JsonArray archJson = new JsonArray();
        if (x.commander) {
            for (final Card c : cmdCards) {
                final Cmdr d = commanders.get(c.name);
                if (d == null) continue;
                x.hasData = true; x.decks = Math.max(x.decks, d.decks);
                for (final var e : d.inc.entrySet()) x.inc.merge(e.getKey(), e.getValue(), Math::max);
                for (final String n : topSynergy(d)) { final Card s = byName.get(n); if (s != null && !synCards.contains(s)) synCards.add(s); }
            }
        } else if (!deckCards.isEmpty() && !archetypes.isEmpty()) {
            matchArchetypes(x, deckCards, archJson, synCards);
        }

        // ---- themes
        final JsonArray themesJson = new JsonArray();
        final List<String> active = detectThemes(x, cmdCards, deckCards, synCards, requested, themesJson);

        // ---- candidates: one pass, scores cached
        final List<Card> pool = new ArrayList<>();
        final Map<Card, Double> score = new HashMap<>();
        for (final Card c : cards) if (x.eligible(c)) { pool.add(c); score.put(c, x.score(c)); }

        final JsonArray sections = new JsonArray();
        // synergy
        if (x.hasData) {
            if (x.commander) section(sections, x, "synergy", "High Synergy Cards", pool, c -> x.inc(c) > 0 && x.syn(c) > 0.01, x::syn, limit);
            else section(sections, x, "synergy", "High Synergy Cards", pool, c -> x.inc(c) > 0, x::inc, limit);
        } else if (!active.isEmpty()) {
            // no data: cards hitting the most active themes, then popularity
            section(sections, x, "synergy", "High Synergy Cards", pool, c -> themeHits(c, active) > 0, c -> themeHits(c, active) + 0.9 * x.pop(c), limit);
        }
        // top
        if (x.hasData && x.commander) section(sections, x, "top", "Top Cards", pool, c -> x.inc(c) > 0, x::inc, limit);
        else section(sections, x, "top", "Top Cards", pool, c -> x.pop(c) > 0, x::pop, limit);
        // themes
        for (final String t : active) {
            if (t.startsWith("tribal:")) { final String ty = t.substring(7); section(sections, x, t, CardTags.label(t), pool, c -> tribalMatch(c, ty), score::get, limit); }
            else { final int b = CardTags.bit(t); section(sections, x, "theme:" + t, CardTags.label(t), pool, c -> (c.tags & b) != 0, score::get, limit); }
        }
        // roles
        for (final String r : List.of("ramp", "draw", "removal", "wipe")) { final int b = CardTags.bit(r); section(sections, x, "role:" + r, CardTags.label(r), pool, c -> (c.tags & b) != 0, score::get, limit); }
        // lands: duals in the deck's colours first unless the commander data already ranks them
        final byte deckColors = x.filter;
        final boolean dualBonus = !(x.commander && x.hasData) && deckColors > 0 && Integer.bitCount(deckColors) >= 2;
        section(sections, x, "lands", "Lands", pool, c -> c.land,
                c -> score.get(c) + (dualBonus && Integer.bitCount(c.ci & deckColors) >= 2 ? 1 : 0), limit);

        final JsonObject o = new JsonObject();
        o.addProperty("format", x.commander ? "commander" : "constructed");
        o.addProperty("colorIdentity", x.filter < 0 ? "" : x.filter == 0 ? "C" : colors(x.filter));
        if (x.commander) o.addProperty("commanderDecks", x.decks);
        o.add("themes", themesJson);
        o.add("archetypes", archJson);
        o.add("sections", sections);
        o.add("unresolved", unresolved);
        return o;
    }

    private Card resolve(final String name) {
        if (name == null || name.isBlank()) return null;
        final Card c = byName.get(name);
        if (c != null) return c;
        final PaperCard pc = catalog.resolveExact(name);
        return pc == null ? null : byName.get(pc.getName());
    }

    /** The commander's 40 highest-synergy cards (any colour-legal card), cached per commander. */
    private List<String> topSynergy(final Cmdr d) {
        return synergyCache.computeIfAbsent(d.name, k -> {
            final List<Map.Entry<String, Double>> l = new ArrayList<>();
            for (final var e : d.inc.entrySet()) {
                final Card c = byName.get(e.getKey());
                if (c == null || c.basic || (c.ci & ~d.ci) != 0) continue;
                l.add(Map.entry(e.getKey(), e.getValue() - c.base));
            }
            l.sort(Map.Entry.<String, Double>comparingByValue().reversed());
            final List<String> out = new ArrayList<>();
            for (int i = 0; i < Math.min(40, l.size()) && l.get(i).getValue() > 0; i++) out.add(l.get(i).getKey());
            return out;
        });
    }

    /**
     * Archetype similarity = cosine between the deck (binary vector over its distinct non-basic cards) and the
     * archetype's card-probability vector. Cosine rather than a raw dot product so archetypes with a few very
     * concentrated cards don't win just by mass. Presence of a card X is then Σ sim(A)·q_A(X) / Σ sim(A) over the
     * best-matching archetypes (within half the top similarity, at most 15) — the spec's Σ sim·p, scaled so the
     * "inclusion" reads as a share of similar decks. Archetypes are ranked and weighted by cosine × n/(n+30)
     * (n = its deck count): a 19-deck LDA cluster is noise next to a 2700-deck one at similar cosine.
     */
    private void matchArchetypes(final Ctx x, final List<Card> deckCards, final JsonArray out, final List<Card> synCards) {
        final double sqrtN = Math.sqrt(deckCards.size());
        final List<Map.Entry<Arch, Double>> sims = new ArrayList<>();
        for (final Arch a : archetypes) {
            double dot = 0;
            for (final Card c : deckCards) dot += a.p.getOrDefault(c.name, 0.0);
            if (dot > 0) sims.add(Map.entry(a, dot / (sqrtN * a.norm)));
        }
        final Map<Arch, Double> weight = new HashMap<>();
        for (final var e : sims) weight.put(e.getKey(), e.getValue() * e.getKey().decks / (e.getKey().decks + 30.0));
        if (sims.isEmpty()) return;
        sims.sort(Comparator.comparingDouble((Map.Entry<Arch, Double> e) -> weight.get(e.getKey())).reversed());
        final double top = weight.get(sims.get(0).getKey());
        double wsum = 0;
        for (int i = 0; i < Math.min(15, sims.size()) && weight.get(sims.get(i).getKey()) >= top * 0.5; i++) {
            final Arch a = sims.get(i).getKey(); final double w = weight.get(a);
            wsum += w;
            for (final var e : a.q.entrySet()) x.inc.merge(e.getKey(), w * e.getValue(), Double::sum);
        }
        final double ws = wsum;
        x.inc.replaceAll((k, v) -> v / ws);
        x.hasData = true;
        final Map<String, JsonObject> seen = new LinkedHashMap<>();
        for (final var e : sims) {
            final Arch a = e.getKey();
            final String key = a.format + "|" + a.name;
            final JsonObject j = seen.get(key);
            if (j != null) { j.addProperty("decks", j.get("decks").getAsInt() + a.decks); continue; }
            if (seen.size() >= 5) continue;
            final JsonObject o = new JsonObject();
            o.addProperty("name", a.name); o.addProperty("format", a.format);
            o.addProperty("similarity", round(e.getValue())); o.addProperty("decks", a.decks);
            seen.put(key, o);
        }
        seen.values().forEach(out::add);
        final List<Map.Entry<String, Double>> l = new ArrayList<>(x.inc.entrySet());
        l.sort(Map.Entry.<String, Double>comparingByValue().reversed());
        for (final var e : l) {
            if (synCards.size() >= 30) break;
            final Card c = byName.get(e.getKey());
            if (c != null && !c.basic && !x.deck.contains(c.name)) synCards.add(c);
        }
    }

    /**
     * Themes are ranked by how far the tag is over-represented versus the whole card pool: weight W = Σ weights of
     * cards carrying it (deck card 1, commander 3, data's top-synergy card 0.5), excess = W − N·share. The commander
     * and its synergy cards make an empty deck already suggest its themes; a filled deck dominates by mass.
     */
    private List<String> detectThemes(final Ctx x, final List<Card> cmdCards, final List<Card> deckCards, final List<Card> synCards,
                                      final List<String> requested, final JsonArray out) {
        final Map<Card, Double> weight = new LinkedHashMap<>();
        for (final Card c : synCards) weight.put(c, 0.5);
        for (final Card c : deckCards) weight.put(c, 1.0);
        for (final Card c : cmdCards) weight.put(c, 3.0);
        double n = 0;
        for (final double w : weight.values()) n += w;

        record Theme(String key, String label, int count, double excess) {}
        final List<Theme> found = new ArrayList<>();
        final List<String> keys = CardTags.keys();
        for (int i = 0; i < keys.size(); i++) {
            if (CardTags.isRole(keys.get(i))) continue;
            final int b = 1 << i;
            double w = 0; int count = 0;
            for (final var e : weight.entrySet()) if ((e.getKey().tags & b) != 0) w += e.getValue();
            for (final Card c : deckCards) if ((c.tags & b) != 0) count++;
            for (final Card c : cmdCards) if ((c.tags & b) != 0) count++;
            final double ex = w - n * tagShare[i];
            if (w >= 1.5 && ex > 0) found.add(new Theme(keys.get(i), CardTags.label(keys.get(i)), count, ex));
        }
        // tribal: types the commander's text names, or that ≥5 deck creatures share. Not from synergy cards alone:
        // set-design types (every ONE card is a Phyrexian) would otherwise read as tribes. Changelings (all types) don't vote.
        final Map<String, Integer> deckTypes = new HashMap<>();
        for (final Card c : deckCards) if (c.creature && c.ctypes.size() < 20) for (final String t : c.ctypes) deckTypes.merge(t, 1, Integer::sum);
        final Set<String> cands = new LinkedHashSet<>();
        for (final Card c : cmdCards) cands.addAll(mentionedTypes(c.text));
        deckTypes.forEach((t, k) -> { if (k >= 5) cands.add(t); });
        for (final String r : requested) if (r.startsWith("tribal:")) cands.add(r.substring(7));
        for (final String t : cands) {
            double w = 0; int count = 0;
            for (final var e : weight.entrySet()) if (tribalMatch(e.getKey(), t)) w += e.getValue();
            for (final Card c : deckCards) if (tribalMatch(c, t)) count++;
            for (final Card c : cmdCards) if (tribalMatch(c, t)) count++;
            final double ex = w - n * tribalShare(t);
            if (ex > 0 || requested.contains("tribal:" + t)) found.add(new Theme("tribal:" + t, CardTags.label("tribal:" + t), count, Math.max(0, ex)));
        }
        found.sort(Comparator.comparingDouble(Theme::excess).reversed());
        // a tribe whose members are mostly members of a stronger tribe (Goblin Warriors under Goblin) adds nothing
        final List<String> keptTribes = new ArrayList<>();
        found.removeIf(t -> {
            if (!t.key.startsWith("tribal:") || requested.contains(t.key)) return false;
            final String ty = t.key.substring(7);
            int members = 0;
            final int[] shared = new int[keptTribes.size()];
            for (final Card c : weight.keySet()) {
                if (!tribalMatch(c, ty)) continue;
                members++;
                for (int i = 0; i < keptTribes.size(); i++) if (tribalMatch(c, keptTribes.get(i))) shared[i]++;
            }
            for (final int sh : shared) if (members > 0 && sh >= 0.8 * members) return true;
            keptTribes.add(ty);
            return false;
        });
        final double maxEx = found.isEmpty() ? 1 : Math.max(1e-9, found.get(0).excess);

        final List<String> active = new ArrayList<>(requested);
        if (active.isEmpty()) for (int i = 0; i < Math.min(2, found.size()); i++) active.add(found.get(i).key);
        final List<Theme> shown = new ArrayList<>();
        for (final String r : requested) {   // requested themes always listed, even when the deck has none yet
            Theme t = null;
            for (final Theme f : found) if (f.key.equals(r)) t = f;
            shown.add(t != null ? t : new Theme(r, CardTags.label(r), 0, 0));
        }
        for (final Theme f : found) { if (shown.size() >= Math.max(8, requested.size())) break; if (!requested.contains(f.key)) shown.add(f); }
        for (final Theme t : shown) {
            final JsonObject o = new JsonObject();
            o.addProperty("key", t.key); o.addProperty("label", t.label); o.addProperty("count", t.count);
            o.addProperty("score", round(t.excess / maxEx)); o.addProperty("active", active.contains(t.key));
            out.add(o);
        }
        return active;
    }

    private int themeHits(final Card c, final List<String> active) {
        int n = 0;
        for (final String t : active) if (t.startsWith("tribal:") ? tribalMatch(c, t.substring(7)) : (c.tags & CardTags.bit(t)) != 0) n++;
        return n;
    }

    private void section(final JsonArray out, final Ctx x, final String key, final String title, final List<Card> pool,
                         final Predicate<Card> filter, final ToDoubleFunction<Card> rank, final int limit) {
        final List<Card> l = new ArrayList<>();
        for (final Card c : pool) if (filter.test(c)) l.add(c);
        if (l.isEmpty()) return;
        final Map<Card, Double> k = new HashMap<>(l.size() * 2);
        for (final Card c : l) k.put(c, rank.applyAsDouble(c));
        l.sort((a, b) -> { final int r = Double.compare(k.get(b), k.get(a)); return r != 0 ? r : a.name.compareTo(b.name); });
        final JsonArray cs = new JsonArray();
        for (final Card c : l.subList(0, Math.min(limit, l.size()))) {
            final JsonObject o = new JsonObject();
            final double inc = x.inc(c), syn = x.syn(c);
            o.add("card", catalog.cardJson(c.pc));
            o.addProperty("inclusion", round(inc));
            o.addProperty("synergy", round(syn));
            o.addProperty("score", round(x.score(c)));
            o.addProperty("reason", reason(x, c, inc, syn));
            cs.add(o);
        }
        final JsonObject s = new JsonObject();
        s.addProperty("key", key); s.addProperty("title", title); s.add("cards", cs);
        out.add(s);
    }

    private static String reason(final Ctx x, final Card c, final double inc, final double syn) {
        if (inc > 0) {
            final long pct = Math.round(inc * 100);
            String r = (pct == 0 ? "<1" : String.valueOf(pct)) + (x.commander ? "% of decks" : "% of similar decks");
            final long s = Math.round(syn * 100);
            if (s != 0) r += " · " + (s > 0 ? "+" : "") + s + "% synergy";
            return r;
        }
        final double p = x.pop(c);
        return p > 0 ? "Popularity " + Math.round(p * 100) + "%" : "Fits your colors";
    }

    private static String colors(final byte m) {
        final StringBuilder sb = new StringBuilder();
        if ((m & forge.card.MagicColor.WHITE) != 0) sb.append('W');
        if ((m & forge.card.MagicColor.BLUE) != 0) sb.append('U');
        if ((m & forge.card.MagicColor.BLACK) != 0) sb.append('B');
        if ((m & forge.card.MagicColor.RED) != 0) sb.append('R');
        if ((m & forge.card.MagicColor.GREEN) != 0) sb.append('G');
        return sb.toString();
    }
    private static double round(final double v) { return Math.round(v * 1000) / 1000.0; }
    private static String str(final JsonObject o, final String k) { final JsonElement e = o.get(k); return e == null || e.isJsonNull() ? null : e.getAsString(); }
}
