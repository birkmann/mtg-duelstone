package arena.engine.server;

import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import forge.card.ColorSet;
import forge.deck.Deck;
import forge.deck.DeckFormat;
import forge.deck.DeckgenUtil;
import forge.deck.io.DeckSerializer;
import forge.game.GameFormat;
import forge.game.GameType;
import forge.item.PaperCard;
import forge.localinstance.properties.ForgeConstants;
import forge.model.FModel;
import forge.util.storage.IStorage;

import java.io.File;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Comparator;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.ThreadLocalRandom;

/**
 * Where decks come from. A deck ref is
 * <ul>
 *   <li>{@code "starter:<name>"} — a {@code .dck} shipped in this repo's {@code decks/<format>/} dir,</li>
 *   <li>{@code "precon:<name>"} — Forge's shipped Commander precons,</li>
 *   <li>{@code "theme:<name>"} — Forge's bundled constructed precon decks (quest/precons, 60-card legal only),</li>
 *   <li>{@code "archetype:…"}, {@code "tournament:…"}, {@code "forge:<path>"} — the constructed {@link DeckLibrary},</li>
 *   <li>{@code "user:<name>"} — Forge's own ~/.forge/decks/{commander,constructed},</li>
 *   <li>{@code "file:<path>"} — any .dck,</li>
 *   <li>{@code "random"} — a random legal deck of the requested format (for the AI).</li>
 * </ul>
 */
public final class DeckSource {
    private DeckSource() {}

    public static final String COMMANDER = "commander", CONSTRUCTED = "constructed";
    public static final String BRAWL = "brawl", OATHBREAKER = "oathbreaker", LIMITED = "limited";
    /** Constructed legalities a match can enforce (ids are lower-case Forge format names). */
    public static final List<String> LEGALITIES = List.of("standard", "pioneer", "modern", "legacy", "vintage", "pauper");

    // Forge's deck storage + CardDb lazy-load deck sections and are not thread-safe: serialize all access.
    private static final Object LOCK = new Object();

    private static volatile File decksRoot;
    private static Map<String, Deck> themeCache; // lazily loaded, guarded by LOCK

    /** constructed|commander|brawl|oathbreaker|limited ("commander" when absent, for v1 clients); anything else is rejected. */
    public static String format(final String f) {
        if (f == null || f.isBlank()) return COMMANDER;
        final String n = f.trim().toLowerCase();
        if (n.equals(COMMANDER) || n.equals(CONSTRUCTED) || n.equals(BRAWL) || n.equals(OATHBREAKER) || n.equals(LIMITED)) return n;
        throw new IllegalArgumentException("unknown format: " + f + " (constructed|commander|brawl|oathbreaker|limited)");
    }

    /** Has a commander-style command zone (the deck's Commander section matters). */
    public static boolean hasCommander(final String format) {
        return COMMANDER.equals(format) || BRAWL.equals(format) || OATHBREAKER.equals(format);
    }

    /** Deck-construction rules of a format (size, copies, commander checks). */
    public static DeckFormat deckFormat(final String format) {
        return switch (format(format)) {
            case CONSTRUCTED -> DeckFormat.Constructed;
            case BRAWL -> DeckFormat.Brawl;
            case OATHBREAKER -> DeckFormat.Oathbreaker;
            case LIMITED -> DeckFormat.Limited;
            default -> DeckFormat.Commander;
        };
    }

    /** null/""/"casual" → null (anything goes); otherwise one of {@link #LEGALITIES}. */
    public static String legality(final String l) {
        if (l == null || l.isBlank() || l.equalsIgnoreCase("casual")) return null;
        final String n = l.trim().toLowerCase();
        if (!LEGALITIES.contains(n)) throw new IllegalArgumentException("unknown legality: " + l + " " + LEGALITIES);
        return n;
    }

    /** Forge's card-pool rules (sets, bans, restricted, rarity) for a legality id. */
    public static GameFormat gameFormat(final String legality) {
        if (legality == null) return null;
        final GameFormat gf = FModel.getFormats().getFormat(Character.toUpperCase(legality.charAt(0)) + legality.substring(1));
        if (gf == null) throw new IllegalArgumentException("Forge has no " + legality + " format data");
        return gf;
    }

    // ---- repo decks dir ---------------------------------------------------------------------

    public static void setDecksRoot(final File dir) { decksRoot = dir; }

    /** --decks DIR, else walk up from cwd (and from the jar) looking for a {@code decks/} dir with a format subfolder. */
    public static File decksRoot() {
        File r = decksRoot;
        if (r != null) return r;
        final List<File> starts = new ArrayList<>();
        starts.add(new File(".").getAbsoluteFile());
        try {
            final File jar = new File(DeckSource.class.getProtectionDomain().getCodeSource().getLocation().toURI());
            starts.add(jar.getAbsoluteFile().getParentFile());
        } catch (final Exception ignored) {}
        for (File cur : starts) {
            for (int i = 0; i < 8 && cur != null; i++, cur = cur.getParentFile()) {
                final File cand = new File(cur, "decks");
                if (new File(cand, CONSTRUCTED).isDirectory() || new File(cand, COMMANDER).isDirectory()) { decksRoot = cand; return cand; }
            }
        }
        return null;
    }

    private static List<File> starterFiles(final String format) {
        final File root = decksRoot();
        if (root == null) return List.of();
        final File[] fs = new File(root, format).listFiles((d, n) -> n.toLowerCase().endsWith(".dck"));
        if (fs == null) return List.of();
        final List<File> out = new ArrayList<>(Arrays.asList(fs));
        out.sort(Comparator.comparing(File::getName));
        return out;
    }

    private static Deck loadFile(final File f) {
        final Deck d = DeckSerializer.fromFile(f);
        if (d == null) throw new IllegalArgumentException("cannot parse deck file: " + f);
        return d;
    }

    // ---- Forge's constructed theme decks ----------------------------------------------------

    private static Map<String, Deck> themes() {
        if (themeCache != null) return themeCache;
        final Map<String, Deck> m = new LinkedHashMap<>();
        final File dir = new File(ForgeConstants.RES_DIR + "quest/precons");
        final File[] fs = dir.listFiles((d, n) -> n.toLowerCase().endsWith(".dck"));
        if (fs != null) {
            Arrays.sort(fs);
            for (final File f : fs) {
                try {
                    final Deck d = DeckSerializer.fromFile(f);
                    if (d == null || d.getMain().countAll() < 60 || d.getMain().countAll() > 80) continue;
                    if (problem(d, CONSTRUCTED) != null) continue;
                    m.put(d.getName(), d);
                } catch (final Throwable ignored) { /* unloadable precon: skip */ }
            }
        }
        System.out.println("[decks] " + m.size() + " bundled constructed theme decks usable");
        return themeCache = m;
    }

    // ---- listing ----------------------------------------------------------------------------

    public static JsonArray list() { return list(COMMANDER); }

    public static JsonArray list(final String format) { return list(format, null); }

    public static JsonArray list(final String format, final String legality) {
        synchronized (LOCK) { return listLocked(format(format), legality(legality)); }
    }

    private static JsonArray listLocked(final String format) { return listLocked(format, null); }

    private static JsonArray listLocked(final String format, final String legality) {
        final JsonArray a = new JsonArray();
        // The engine can always make a legal deck (generated when no saved deck qualifies, e.g. Pauper).
        if (!LIMITED.equals(format)) {
            final JsonObject rnd = new JsonObject();
            rnd.addProperty("ref", "random");
            rnd.addProperty("name", "Random deck");
            rnd.add("commanders", new JsonArray());
            rnd.addProperty("mainCount", hasCommander(format) && !COMMANDER.equals(format) ? 60 : COMMANDER.equals(format) ? 100 : 60);
            rnd.addProperty("colorIdentity", "WUBRG");
            rnd.addProperty("cover", RANDOM_COVER);
            rnd.addProperty("format", format);
            rnd.add("problem", com.google.gson.JsonNull.INSTANCE);
            rnd.addProperty("generated", true);
            a.add(rnd);
        }
        for (final File f : starterFiles(format)) {
            try {
                final Deck d = loadFile(f);
                a.add(withProblem(describe("starter:" + stem(f), d, format), d, format, legality));
            } catch (final Throwable t) {
                final JsonObject o = new JsonObject();
                o.addProperty("ref", "starter:" + stem(f)); o.addProperty("name", stem(f)); o.add("commanders", new JsonArray());
                o.addProperty("mainCount", 0); o.addProperty("colorIdentity", ""); o.addProperty("format", format);
                o.addProperty("problem", "could not load: " + t.getMessage());
                a.add(o);
            }
        }
        switch (format) {
            case COMMANDER -> {
                for (final Deck d : FModel.getDecks().getCommanderPrecons()) a.add(describe("precon:" + d.getName(), d, format));
                for (final Deck d : FModel.getDecks().getCommander()) a.add(describe("user:" + d.getName(), d, format));
            }
            case BRAWL -> { for (final Deck d : FModel.getDecks().getBrawl()) a.add(withProblem(describe("user:" + d.getName(), d, format), d, format, null)); }
            case OATHBREAKER -> { for (final Deck d : FModel.getDecks().getOathbreaker()) a.add(withProblem(describe("user:" + d.getName(), d, format), d, format, null)); }
            case CONSTRUCTED -> {
                for (final Deck d : FModel.getDecks().getConstructed()) a.add(withProblem(describe("user:" + d.getName(), d, format), d, format, legality));
                // themes are pre-filtered to legal 60-card decks; only a legality needs a check
                for (final Deck d : themes().values()) {
                    final JsonObject o = describe("theme:" + d.getName(), d, format);
                    a.add(legality == null ? o : withProblem(o, d, format, legality));
                }
                // thousands of decks: only the playable ones are listed, not marked
                for (final DeckLibrary.Entry e : DeckLibrary.entries().values()) if (e.legalIn(legality)) a.add(describe(e));
            }
            default -> {}
        }
        return a;
    }

    private static JsonObject withProblem(final JsonObject o, final Deck d, final String format) { return withProblem(o, d, format, null); }

    private static JsonObject withProblem(final JsonObject o, final Deck d, final String format, final String legality) {
        o.addProperty("problem", problem(d, format, legality));
        return o;
    }

    private static String stem(final File f) { return f.getName().replaceFirst("(?i)\\.dck$", ""); }

    public static String problem(final Deck d, final String format) { return problem(d, format, null); }

    /**
     * Why a deck can't be played in a format (+ legality), or null. Forge splits the check in two:
     * {@link DeckFormat} = construction (size, copies, commander), {@link GameFormat} = card pool (sets,
     * bans, restricted list, Pauper's rarity rule).
     */
    public static String problem(final Deck d, final String format, final String legality) {
        try {
            final String p = deckFormat(format).getDeckConformanceProblem(d);
            if (p != null || legality == null) return p;
            final String q = gameFormat(legality).getDeckConformanceProblem(d);
            return q == null ? null : "not " + legality + "-legal: " + q;
        } catch (final Exception e) { return "could not validate: " + e.getMessage(); }
    }

    public static JsonObject describe(final String ref, final Deck d) { return describe(ref, d, d.getCommanders().isEmpty() ? CONSTRUCTED : COMMANDER); }

    public static JsonObject describe(final String ref, final Deck d, final String format) {
        final JsonObject o = new JsonObject();
        o.addProperty("ref", ref);
        o.addProperty("name", d.getName());
        final JsonArray cmd = new JsonArray();
        for (final PaperCard pc : d.getCommanders()) cmd.add(pc.getName());
        o.add("commanders", cmd);
        o.addProperty("mainCount", d.getMain().countAll());
        final List<String> ci = new ArrayList<>();
        if (hasCommander(format)) {
            for (final PaperCard pc : d.getCommanders()) ci.add(arena.engine.view.ViewJson.colors(pc.getRules().getColorIdentity()));
            o.addProperty("colorIdentity", String.join("", ci));
        } else {
            o.addProperty("colorIdentity", arena.engine.view.ViewJson.colors(spellColors(d)));
        }
        o.addProperty("format", format);
        final PaperCard cover = cover(d);
        if (cover != null) o.addProperty("cover", cover.getName());
        return o;
    }

    private static JsonObject describe(final DeckLibrary.Entry e) {
        final JsonObject o = new JsonObject();
        o.addProperty("ref", e.ref());
        o.addProperty("name", e.name());
        o.add("commanders", new JsonArray());
        o.addProperty("mainCount", e.mainCount());
        o.addProperty("colorIdentity", e.colors());
        o.addProperty("format", CONSTRUCTED);
        if (e.cover() != null) o.addProperty("cover", e.cover());
        if (e.description() != null) o.addProperty("description", e.description());
        o.add("problem", com.google.gson.JsonNull.INSTANCE);
        return o;
    }

    /**
     * What a 60-card deck plays: the colours of its spells, each with at least 4 copies. Colour identity would
     * count the lands too (a Raffine's Tower makes a two-colour control deck look like four), and real lists
     * carry one-off splashes (2 Prismatic Strands) that don't make a Dimir deck five-colour.
     */
    public static ColorSet spellColors(final Deck d) {
        final int[] perColor = new int[5];
        int all = 0, any = 0;
        for (final Map.Entry<PaperCard, Integer> e : d.getMain()) {
            final forge.card.CardRules r = e.getKey().getRules();
            all |= r.getColorIdentity().getColor();
            if (r.getType().isLand()) continue;
            final int c = r.getColor().getColor();
            any |= c;
            for (int i = 0; i < 5; i++) if ((c & (1 << i)) != 0) perColor[i] += e.getValue();
        }
        int main = 0;
        for (int i = 0; i < 5; i++) if (perColor[i] >= 4) main |= 1 << i;
        return ColorSet.fromMask(main != 0 ? main : any != 0 ? any : all);
    }

    /** Art for the "Random deck" tile: there are no cards to choose from until it's generated. */
    private static final String RANDOM_COVER = "Chaos Warp";

    /**
     * The card whose art stands for a deck on the deck select screen (clients may override it):
     * the commander, else the deck's signature card - the rarest non-land, creatures and
     * planeswalkers first, then the most copies, then the biggest. Deterministic, so a deck keeps its face.
     */
    public static PaperCard cover(final Deck d) {
        if (!d.getCommanders().isEmpty()) return d.getCommanders().get(0);
        PaperCard best = null;
        long bestScore = Long.MIN_VALUE;
        for (final Map.Entry<PaperCard, Integer> e : d.getMain()) {
            final PaperCard pc = e.getKey();
            final forge.card.CardType t = pc.getRules().getType();
            final long score = (t.isLand() ? 0 : 1L << 40)
                    + (long) rarityRank(pc.getRarity()) * (1L << 32)
                    + ((t.isCreature() || t.isPlaneswalker()) ? 1L << 28 : 0)
                    + Math.min(e.getValue(), 255) * (1L << 16)
                    + Math.max(0, pc.getRules().getManaCost().getCMC());
            if (score > bestScore || (score == bestScore && pc.getName().compareTo(best.getName()) < 0)) { best = pc; bestScore = score; }
        }
        return best;
    }

    private static int rarityRank(final forge.card.CardRarity r) {
        return switch (r) {
            case MythicRare -> 5;
            case Rare, Special -> 4;
            case Uncommon -> 3;
            case Common -> 2;
            default -> 1;
        };
    }

    // ---- loading ----------------------------------------------------------------------------

    public static Deck load(final String ref) { return load(ref, null); }

    /** {@code format} disambiguates {@code user:} refs and chooses the pool for {@code random}; null = commander first, then constructed. */
    public static Deck load(final String ref, final String format) {
        synchronized (LOCK) { return loadLocked(ref, format == null ? null : format(format)); }
    }

    /** Load and, for a legality, insist the deck is legal ("random" picks/generates a legal one). */
    public static Deck load(final String ref, final String format, final String legality) {
        synchronized (LOCK) {
            final String f = format == null ? null : format(format);
            final String l = legality(legality);
            if ("random".equals(ref)) return random(f == null ? COMMANDER : f, l);
            final Deck d = loadLocked(ref, f);
            if (l != null) {
                final String p = problem(d, f == null ? CONSTRUCTED : f, l);
                if (p != null) throw new IllegalArgumentException(d.getName() + ": " + p);
            }
            return d;
        }
    }

    private static Deck loadLocked(final String ref, final String format) {
        if (ref == null) throw new IllegalArgumentException("deck ref required");
        if (ref.equals("random")) return random(format == null ? COMMANDER : format, null);
        final int colon = ref.indexOf(':');
        final String kind = colon < 0 ? "precon" : ref.substring(0, colon);
        final String name = colon < 0 ? ref : ref.substring(colon + 1);
        switch (kind) {
            case "precon": return require(FModel.getDecks().getCommanderPrecons(), name, ref);
            case "archetype", "tournament", "forge": return DeckLibrary.load(ref);
            case "theme": {
                final Deck d = themes().get(name);
                if (d != null) return d;
                for (final Deck x : themes().values()) if (x.getName().equalsIgnoreCase(name)) return x;
                throw new IllegalArgumentException("deck not found: " + ref);
            }
            case "starter": {
                for (final String fmt : format == null ? List.of(COMMANDER, CONSTRUCTED) : List.of(format)) {
                    for (final File f : starterFiles(fmt)) if (stem(f).equalsIgnoreCase(name)) return loadFile(f);
                }
                throw new IllegalArgumentException("deck not found: " + ref);
            }
            case "user": {
                if (BRAWL.equals(format)) return require(FModel.getDecks().getBrawl(), name, ref);
                if (OATHBREAKER.equals(format)) return require(FModel.getDecks().getOathbreaker(), name, ref);
                if (format == null || format.equals(COMMANDER)) {
                    final Deck d = find(FModel.getDecks().getCommander(), name);
                    if (d != null) return d;
                }
                if (format == null || format.equals(CONSTRUCTED)) {
                    final Deck d = find(FModel.getDecks().getConstructed(), name);
                    if (d != null) return d;
                }
                throw new IllegalArgumentException("deck not found: " + ref);
            }
            case "file": {
                final File f = new File(name);
                if (!f.isFile()) throw new IllegalArgumentException("deck file not found: " + name);
                return loadFile(f);
            }
            default: throw new IllegalArgumentException("unknown deck ref kind: " + kind);
        }
    }

    private static Deck random(final String format, final String legality) {
        final JsonArray all = listLocked(format, legality);
        final List<String> ok = new ArrayList<>();
        for (final var e : all) {
            final JsonObject o = e.getAsJsonObject();
            if (o.has("problem") && !o.get("problem").isJsonNull()) continue;
            if ("random".equals(o.get("ref").getAsString())) continue;
            ok.add(o.get("ref").getAsString());
        }
        if (ok.isEmpty()) return generate(format, legality);
        // The big bundled theme pool would drown out the curated starters: favour starters 3:1 when present.
        final List<String> starters = new ArrayList<>();
        for (final String r : ok) if (r.startsWith("starter:")) starters.add(r);
        final List<String> pool = !starters.isEmpty() && ThreadLocalRandom.current().nextInt(4) != 0 ? starters : ok;
        return loadLocked(pool.get(ThreadLocalRandom.current().nextInt(pool.size())), format);
    }

    /** Forge's deck generator, for formats/legalities nobody has a saved deck for (Pauper, Brawl…). */
    private static Deck generate(final String format, final String legality) {
        final Deck d = switch (format) {
            case BRAWL -> DeckgenUtil.generateCommanderDeck(true, GameType.Brawl);
            case OATHBREAKER -> DeckgenUtil.generateCommanderDeck(true, GameType.Oathbreaker);
            case COMMANDER -> DeckgenUtil.generateCommanderDeck(true, GameType.Commander);
            case CONSTRUCTED -> legality == null ? DeckgenUtil.getRandomColorDeck(true) : DeckgenUtil.getRandomColorDeck(gameFormat(legality).getFilterRules(), true);
            default -> throw new IllegalArgumentException("no usable " + format + " decks for random");
        };
        if (d.getName() == null || d.getName().isBlank()) d.setName("Random " + format + (legality == null ? "" : " (" + legality + ")"));
        return d;
    }

    private static Deck find(final IStorage<Deck> store, final String name) {
        final Deck d = store.get(name);
        if (d != null) return d;
        for (final Deck x : store) if (x.getName().equalsIgnoreCase(name)) return x;
        return null;
    }

    private static Deck require(final IStorage<Deck> store, final String name, final String ref) {
        final Deck d = find(store, name);
        if (d != null) return d;
        throw new IllegalArgumentException("deck not found: " + ref);
    }
}
