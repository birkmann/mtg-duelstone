package arena.engine.server;

import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import forge.card.ColorSet;
import forge.deck.Deck;
import forge.deck.DeckFormat;
import forge.deck.io.DeckSerializer;
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
 *   <li>{@code "user:<name>"} — Forge's own ~/.forge/decks/{commander,constructed},</li>
 *   <li>{@code "file:<path>"} — any .dck,</li>
 *   <li>{@code "random"} — a random legal deck of the requested format (for the AI).</li>
 * </ul>
 */
public final class DeckSource {
    private DeckSource() {}

    public static final String COMMANDER = "commander", CONSTRUCTED = "constructed";

    // Forge's deck storage + CardDb lazy-load deck sections and are not thread-safe: serialize all access.
    private static final Object LOCK = new Object();

    private static volatile File decksRoot;
    private static Map<String, Deck> themeCache; // lazily loaded, guarded by LOCK

    /** "constructed" or "commander" (default, back-compat); anything unknown is rejected. */
    public static String format(final String f) {
        if (f == null || f.isBlank()) return COMMANDER;
        final String n = f.trim().toLowerCase();
        if (n.equals(COMMANDER) || n.equals(CONSTRUCTED)) return n;
        throw new IllegalArgumentException("unknown format: " + f + " (constructed|commander)");
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

    public static JsonArray list(final String format) {
        synchronized (LOCK) { return listLocked(format(format)); }
    }

    private static JsonArray listLocked(final String format) {
        final JsonArray a = new JsonArray();
        for (final File f : starterFiles(format)) {
            try {
                final Deck d = loadFile(f);
                a.add(withProblem(describe("starter:" + stem(f), d, format), d, format));
            } catch (final Throwable t) {
                final JsonObject o = new JsonObject();
                o.addProperty("ref", "starter:" + stem(f)); o.addProperty("name", stem(f)); o.add("commanders", new JsonArray());
                o.addProperty("mainCount", 0); o.addProperty("colorIdentity", ""); o.addProperty("format", format);
                o.addProperty("problem", "could not load: " + t.getMessage());
                a.add(o);
            }
        }
        if (format.equals(COMMANDER)) {
            for (final Deck d : FModel.getDecks().getCommanderPrecons()) a.add(describe("precon:" + d.getName(), d, format));
            for (final Deck d : FModel.getDecks().getCommander()) a.add(describe("user:" + d.getName(), d, format));
        } else {
            for (final Deck d : FModel.getDecks().getConstructed()) a.add(withProblem(describe("user:" + d.getName(), d, format), d, format));
            for (final Deck d : themes().values()) a.add(describe("theme:" + d.getName(), d, format));
        }
        return a;
    }

    private static JsonObject withProblem(final JsonObject o, final Deck d, final String format) {
        final String p = problem(d, format);
        o.addProperty("problem", p);
        return o;
    }

    private static String stem(final File f) { return f.getName().replaceFirst("(?i)\\.dck$", ""); }

    public static String problem(final Deck d, final String format) {
        try {
            return (format.equals(CONSTRUCTED) ? DeckFormat.Constructed : DeckFormat.Commander).getDeckConformanceProblem(d);
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
        int mask = 0;
        if (format.equals(COMMANDER)) {
            for (final PaperCard pc : d.getCommanders()) ci.add(arena.engine.view.ViewJson.colors(pc.getRules().getColorIdentity()));
            o.addProperty("colorIdentity", String.join("", ci));
        } else {
            for (final Map.Entry<PaperCard, Integer> e : d.getMain()) mask |= e.getKey().getRules().getColorIdentity().getColor();
            o.addProperty("colorIdentity", arena.engine.view.ViewJson.colors(ColorSet.fromMask(mask)));
        }
        o.addProperty("format", format);
        return o;
    }

    // ---- loading ----------------------------------------------------------------------------

    public static Deck load(final String ref) { return load(ref, null); }

    /** {@code format} disambiguates {@code user:} refs and chooses the pool for {@code random}; null = commander first, then constructed. */
    public static Deck load(final String ref, final String format) {
        synchronized (LOCK) { return loadLocked(ref, format == null ? null : format(format)); }
    }

    private static Deck loadLocked(final String ref, final String format) {
        if (ref == null) throw new IllegalArgumentException("deck ref required");
        if (ref.equals("random")) return random(format == null ? COMMANDER : format);
        final int colon = ref.indexOf(':');
        final String kind = colon < 0 ? "precon" : ref.substring(0, colon);
        final String name = colon < 0 ? ref : ref.substring(colon + 1);
        switch (kind) {
            case "precon": return require(FModel.getDecks().getCommanderPrecons(), name, ref);
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

    private static Deck random(final String format) {
        final JsonArray all = listLocked(format);
        final List<String> ok = new ArrayList<>();
        for (final var e : all) {
            final JsonObject o = e.getAsJsonObject();
            if (o.has("problem") && !o.get("problem").isJsonNull()) continue;
            ok.add(o.get("ref").getAsString());
        }
        if (ok.isEmpty()) throw new IllegalArgumentException("no usable " + format + " decks for random");
        // The big bundled theme pool would drown out the curated starters: favour starters 3:1 when present.
        final List<String> starters = new ArrayList<>();
        for (final String r : ok) if (r.startsWith("starter:")) starters.add(r);
        final List<String> pool = !starters.isEmpty() && ThreadLocalRandom.current().nextInt(4) != 0 ? starters : ok;
        return loadLocked(pool.get(ThreadLocalRandom.current().nextInt(pool.size())), format);
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
