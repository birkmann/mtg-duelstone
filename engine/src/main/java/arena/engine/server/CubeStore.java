package arena.engine.server;

import arena.engine.deck.CardCatalog;
import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import forge.StaticData;
import forge.deck.CardPool;
import forge.gamemodes.limited.CustomLimited;
import forge.item.PaperCard;
import forge.localinstance.properties.ForgeConstants;
import forge.model.FModel;
import forge.util.FileUtil;
import org.apache.commons.lang3.tuple.ImmutablePair;
import org.apache.commons.lang3.tuple.Pair;

import java.io.File;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

/**
 * Cubes for drafting: Forge's ~95 bundled ones (res/draft/*.draft → res/cube/*.dck — Legacy, Vintage,
 * Pauper, Peasant, Arena, Old School…) and the player's own, designed in the client.
 *
 * <p>Why our own JSON for custom cubes instead of Forge's .draft + .dck pair: Forge reads those from its
 * read-only resource dir and caches the list once (BoosterDraft.customs), so new files would not even
 * be seen; a small JSON per cube under {@code ~/.forge/duelstone-cubes/} is editable, renameable and turned
 * into a {@link CustomLimited} on demand, exactly the object Forge's own cube draft uses.
 *
 * <p>Refs: {@code bundled:<name>} / {@code user:<name>}.
 */
public final class CubeStore {
    private CubeStore() {}

    private static final Object LOCK = new Object();
    private static Map<String, CustomLimited> bundled; // name → parsed .draft (lazy, guarded by LOCK)

    public static File userDir() {
        final File d = new File(ForgeConstants.USER_DIR, "duelstone-cubes");
        // cubes saved before the rename lived in arena-cubes/
        final File old = new File(ForgeConstants.USER_DIR, "arena-cubes");
        if (!d.exists() && old.isDirectory() && !old.renameTo(d)) System.err.println("[cubes] could not move " + old + " to " + d);
        d.mkdirs();
        return d;
    }

    private static Map<String, CustomLimited> bundled() {
        if (bundled != null) return bundled;
        final Map<String, CustomLimited> m = new LinkedHashMap<>();
        final File[] fs = new File(ForgeConstants.DRAFT_DIR).listFiles((d, n) -> n.endsWith(".draft"));
        if (fs != null) {
            java.util.Arrays.sort(fs);
            for (final File f : fs) {
                try {
                    final CustomLimited cl = CustomLimited.parse(FileUtil.readFile(f), FModel.getDecks().getCubes());
                    if (cl.getCardPool() == null || cl.getCardPool().countAll() < 45) continue; // "all cards" / broken entries
                    m.put(cl.getName(), cl);
                } catch (final Throwable t) { System.err.println("[cubes] skipping " + f.getName() + ": " + t); }
            }
        }
        System.out.println("[cubes] " + m.size() + " bundled cubes");
        return bundled = m;
    }

    private static String slug(final String name) {
        final String s = name.trim().replaceAll("[^A-Za-z0-9 _().'-]", "").replaceAll("\\s+", " ");
        if (s.isBlank()) throw new IllegalArgumentException("cube name required");
        return s.length() > 80 ? s.substring(0, 80) : s;
    }

    private static File userFile(final String name) { return new File(userDir(), slug(name) + ".json"); }

    private static JsonObject readUser(final File f) throws IOException {
        return Json.parse(Files.readString(f.toPath(), StandardCharsets.UTF_8));
    }

    // ---- listing / loading ------------------------------------------------------------------

    public static JsonArray list() {
        synchronized (LOCK) {
            final JsonArray a = new JsonArray();
            final File[] fs = userDir().listFiles((d, n) -> n.endsWith(".json"));
            if (fs != null) {
                java.util.Arrays.sort(fs);
                for (final File f : fs) {
                    try {
                        final JsonObject c = readUser(f);
                        int n = 0;
                        for (final JsonElement e : c.getAsJsonArray("cards")) n += Math.max(1, Json.integer(e.getAsJsonObject(), "qty") == null ? 1 : Json.integer(e.getAsJsonObject(), "qty"));
                        a.add(info("user:" + Json.str(c, "name"), Json.str(c, "name"), n, true));
                    } catch (final Exception e) { System.err.println("[cubes] unreadable " + f + ": " + e); }
                }
            }
            for (final CustomLimited cl : bundled().values()) a.add(info("bundled:" + cl.getName(), cl.getName(), cl.getCardPool().countAll(), false));
            return a;
        }
    }

    private static JsonObject info(final String ref, final String name, final int size, final boolean user) {
        final JsonObject o = new JsonObject();
        o.addProperty("ref", ref);
        o.addProperty("name", name);
        o.addProperty("size", size);
        o.addProperty("user", user);
        return o;
    }

    /** Full cube for the designer: cards with catalog details. */
    public static JsonObject get(final String ref) {
        synchronized (LOCK) {
            final CustomLimited cl = limited(ref);
            final JsonObject o = new JsonObject();
            o.addProperty("ref", ref);
            o.addProperty("name", cl.getName());
            o.addProperty("packs", cl.getNumPacks());
            o.addProperty("packSize", packSize(cl));
            if (ref.startsWith("user:")) {
                try { o.addProperty("description", Json.str(readUser(userFile(ref.substring(5))), "description")); } catch (final IOException ignored) {}
            }
            final JsonArray cards = new JsonArray();
            final CardCatalog cat = CardCatalog.get();
            for (final Map.Entry<PaperCard, Integer> e : cl.getCardPool()) {
                final JsonObject c = cat.cardJson(e.getKey());
                c.addProperty("qty", e.getValue());
                cards.add(c);
            }
            o.add("cards", cards);
            return o;
        }
    }

    private static int packSize(final CustomLimited cl) {
        int n = 0;
        for (final Pair<String, Integer> s : cl.getSealedProductTemplate().getSlots()) n += s.getRight();
        return n;
    }

    public static String nameOf(final String ref) { synchronized (LOCK) { return limited(ref).getName(); } }

    /**
     * The cube as Forge's limited product (pool, pack layout, singleton, AI pick rankings). User cubes
     * use CubeCobra's rankings file, as Forge does for imported cubes.
     */
    public static CustomLimited limited(final String ref) {
        if (ref == null) throw new IllegalArgumentException("cube ref required");
        synchronized (LOCK) {
            if (ref.startsWith("bundled:")) {
                final CustomLimited cl = bundled().get(ref.substring(8));
                if (cl == null) throw new IllegalArgumentException("no such cube: " + ref);
                return cl;
            }
            if (!ref.startsWith("user:")) throw new IllegalArgumentException("cube ref must be bundled:… or user:…");
            final File f = userFile(ref.substring(5));
            if (!f.isFile()) throw new IllegalArgumentException("no such cube: " + ref);
            try {
                final JsonObject c = readUser(f);
                final int packSize = clamp(Json.integer(c, "packSize"), 15, 5, 30);
                final List<Pair<String, Integer>> slots = new ArrayList<>();
                slots.add(ImmutablePair.of("Any", packSize));
                final CustomLimited cl = new CustomLimited(Json.str(c, "name"), slots);
                final CardPool pool = new CardPool();
                boolean singleton = true;
                for (final JsonElement e : c.getAsJsonArray("cards")) {
                    final JsonObject o = e.getAsJsonObject();
                    final PaperCard pc = resolve(Json.str(o, "name"), Json.str(o, "set"));
                    if (pc == null) continue;
                    final int qty = clamp(Json.integer(o, "qty"), 1, 1, 99);
                    if (qty > 1) singleton = false;
                    pool.add(pc, qty);
                }
                cl.setCardPool(pool);
                cl.setNumPacks(clamp(Json.integer(c, "packs"), 3, 1, 6));
                cl.setSingleton(singleton);
                cl.setCustomRankingsFile("rankings_cubecobra.txt");
                return cl;
            } catch (final IOException e) {
                throw new IllegalArgumentException("cannot read cube: " + e.getMessage());
            }
        }
    }

    private static int clamp(final Integer v, final int dflt, final int lo, final int hi) {
        return v == null ? dflt : Math.max(lo, Math.min(hi, v));
    }

    private static PaperCard resolve(final String name, final String set) {
        if (name == null || name.isBlank()) return null;
        final StaticData sd = StaticData.instance();
        PaperCard pc = set == null || set.isBlank() ? null : sd.getCommonCards().getCard(name, set);
        if (pc == null) pc = CardCatalog.get().resolveExact(name);
        return pc;
    }

    // ---- editing ----------------------------------------------------------------------------

    /** {name, description?, packs?, packSize?, cards:[{name, set?, qty?}], previousName?} → saved cube. */
    public static JsonObject save(final JsonObject b) {
        synchronized (LOCK) {
            final String name = slug(String.valueOf(Json.str(b, "name")));
            if (bundled().containsKey(name)) throw new IllegalArgumentException("'" + name + "' is a bundled cube: pick another name");
            final JsonArray in = b.has("cards") && b.get("cards").isJsonArray() ? b.getAsJsonArray("cards") : new JsonArray();
            final JsonArray cards = new JsonArray();
            final List<String> unknown = new ArrayList<>();
            int total = 0;
            for (final JsonElement e : in) {
                final JsonObject o = e.getAsJsonObject();
                final PaperCard pc = resolve(Json.str(o, "name"), Json.str(o, "set"));
                if (pc == null) { unknown.add(String.valueOf(Json.str(o, "name"))); continue; }
                final JsonObject c = new JsonObject();
                c.addProperty("name", pc.getName());
                c.addProperty("set", pc.getEdition());
                final int qty = clamp(Json.integer(o, "qty"), 1, 1, 99);
                c.addProperty("qty", qty);
                total += qty;
                cards.add(c);
            }
            if (!unknown.isEmpty()) throw new IllegalArgumentException("unknown cards: " + String.join(", ", unknown.subList(0, Math.min(10, unknown.size()))));
            final int packs = clamp(Json.integer(b, "packs"), 3, 1, 6), packSize = clamp(Json.integer(b, "packSize"), 15, 5, 30);
            final JsonObject out = new JsonObject();
            out.addProperty("name", name);
            out.addProperty("description", Json.str(b, "description"));
            out.addProperty("packs", packs);
            out.addProperty("packSize", packSize);
            out.add("cards", cards);
            try {
                Files.writeString(userFile(name).toPath(), Json.GSON.toJson(out), StandardCharsets.UTF_8);
                final String prev = Json.str(b, "previousName");
                if (prev != null && !prev.isBlank() && !slug(prev).equals(name)) Files.deleteIfExists(userFile(prev).toPath());
            } catch (final IOException e) { throw new IllegalArgumentException("cannot save cube: " + e.getMessage()); }
            final JsonObject r = get("user:" + name);
            r.addProperty("size", total);
            // a full draft needs packs × packSize × 8 cards; say so rather than failing mid-draft
            r.addProperty("fullPodCards", packs * packSize * 8);
            return r;
        }
    }

    public static void delete(final String name) {
        synchronized (LOCK) {
            try { Files.deleteIfExists(userFile(name).toPath()); } catch (final IOException e) { throw new IllegalArgumentException(e.getMessage()); }
        }
    }

    private static final Pattern LINE = Pattern.compile("^\\s*(?:(\\d+)\\s*x?\\s+)?(.+?)\\s*(?:\\(([A-Za-z0-9]{2,6})\\)(?:\\s+\\S+)?)?\\s*$");

    /**
     * Paste import ("1 Lightning Bolt", "Lightning Bolt (LEA)", CubeCobra/MTGO lists): resolved cards
     * plus unresolved lines with suggestions.
     */
    public static JsonObject importText(final String text) {
        final JsonArray cards = new JsonArray(), unresolved = new JsonArray();
        final CardCatalog cat = CardCatalog.get();
        for (final String raw : text.split("\\R")) {
            final String line = raw.strip();
            if (line.isEmpty() || line.startsWith("#") || line.startsWith("//") || line.startsWith("[")) continue;
            final Matcher m = LINE.matcher(line);
            if (!m.matches()) continue;
            final int qty = m.group(1) == null ? 1 : Integer.parseInt(m.group(1));
            final PaperCard pc = resolve(m.group(2), m.group(3));
            if (pc == null) {
                final JsonObject u = new JsonObject();
                u.addProperty("line", line);
                final JsonArray sug = new JsonArray();
                for (final PaperCard s : cat.suggest(m.group(2), 3)) sug.add(s.getName());
                u.add("suggestions", sug);
                unresolved.add(u);
                continue;
            }
            final JsonObject c = cat.cardJson(pc);
            c.addProperty("qty", qty);
            cards.add(c);
        }
        final JsonObject o = new JsonObject();
        o.add("cards", cards);
        o.add("unresolved", unresolved);
        return o;
    }

    // ---- sets for set drafts ------------------------------------------------------------------

    /** Sets that have draft boosters (newest first). Conspiracy sets are left out: their draft-matters cards prompt a GUI. */
    public static JsonArray draftableSets() {
        final List<forge.card.CardEdition> eds = new ArrayList<>();
        for (final forge.card.CardEdition e : FModel.getMagicDb().getEditions()) {
            if (!e.hasBoosterTemplate() || FModel.getMagicDb().getBoosters().get(e.getCode()) == null) continue;
            if (e.getCode().equals("CNS") || e.getCode().equals("CN2")) continue;
            eds.add(e);
        }
        eds.sort((a, b) -> b.getDate().compareTo(a.getDate()));
        final JsonArray a = new JsonArray();
        final java.text.SimpleDateFormat df = new java.text.SimpleDateFormat("yyyy-MM-dd");
        for (final forge.card.CardEdition e : eds) {
            final JsonObject o = new JsonObject();
            o.addProperty("code", e.getCode());
            o.addProperty("name", e.getName());
            o.addProperty("date", df.format(e.getDate()));
            a.add(o);
        }
        return a;
    }
}
