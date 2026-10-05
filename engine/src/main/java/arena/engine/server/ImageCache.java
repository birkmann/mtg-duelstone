package arena.engine.server;

import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;

import java.io.File;
import java.io.IOException;
import java.io.Reader;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.HashMap;
import java.util.Map;

/**
 * Card images and rulings, served from the offline pack ({@code offline/}) and nothing else.
 * The game must run without internet and without depending on Scryfall, so nothing is fetched at
 * runtime: {@code tools/fetch-offline.sh} fills the pack once (one printing per card, {@code large}
 * + {@code art_crop}). A card missing from the pack answers 404 instantly, and the client falls back
 * to drawing its rules text.
 */
public final class ImageCache {
    /** {@code img/index.json} entry, written by the {@code fetch-offline} bin. */
    private record Entry(String id, String name, int face, boolean back) {}

    private final Path pack;
    private final Map<String, Entry> index = new HashMap<>();
    private volatile Map<String, String> rulings; // lowercase full name → JSON, loaded on first use

    public ImageCache(final Path pack) throws IOException {
        this.pack = pack;
        final Path idx = pack == null ? null : pack.resolve("img/index.json");
        if (idx == null || !Files.isRegularFile(idx)) {
            System.err.println("[server] WARNING: no offline image pack at " + (idx == null ? "(none found)" : idx)
                    + " — cards will show as text. Run tools/fetch-offline.sh once while online.");
            return;
        }
        try (Reader r = Files.newBufferedReader(idx, StandardCharsets.UTF_8)) {
            for (final Map.Entry<String, JsonElement> e : JsonParser.parseReader(r).getAsJsonObject().entrySet()) {
                final JsonObject o = e.getValue().getAsJsonObject();
                index.put(e.getKey(), new Entry(o.get("id").getAsString(), o.get("name").getAsString(), o.get("face").getAsInt(), o.get("back").getAsBoolean()));
            }
        }
    }

    /**
     * {@code --offline DIR}, else walk up from the cwd: the first {@code offline/} with images, else
     * the first with just the committed {@code data/} (rulings still work without images).
     */
    public static Path resolvePack(final String explicit) {
        if (explicit != null && !explicit.isBlank()) return Path.of(explicit);
        Path dataOnly = null;
        File cur = new File(".").getAbsoluteFile();
        for (int i = 0; i < 6 && cur != null; i++, cur = cur.getParentFile()) {
            final File cand = new File(cur, "offline");
            if (new File(cand, "img/index.json").isFile()) return cand.toPath();
            if (dataOnly == null && new File(cand, "data").isDirectory()) dataOnly = cand.toPath();
        }
        return dataOnly;
    }

    public int size() { return index.size(); }

    public static String version(final String v) {
        if (v == null) return "normal";
        return switch (v.toLowerCase()) {
            case "normal", "large", "art_crop", "png", "small", "border_crop" -> v.toLowerCase();
            default -> throw new IllegalArgumentException("unknown image version: " + v + " (normal|large|art_crop|png)");
        };
    }

    /**
     * The image file, or null if the pack has none. {@code set} is ignored: the pack holds one
     * printing per card. Every version but {@code art_crop} is served from {@code large}.
     */
    public File get(final String name, final String set, final boolean back, final String version) {
        final Entry e = lookup(name);
        if (e == null) return null;
        final boolean useBack = e.back() && (back || e.face() == 1);
        final String dir = "art_crop".equals(version) ? "art_crop" : "large";
        final File f = pack.resolve("img").resolve(dir).resolve(e.id().substring(0, 2)).resolve(e.id() + (useBack ? "-back" : "") + ".jpg").toFile();
        return f.isFile() ? f : null;
    }

    /** Keep in sync with {@code engine_lookup} in the fetch-offline bin (its coverage report). */
    private Entry lookup(final String name) {
        final String k = name.toLowerCase();
        Entry e = index.get(k);
        // Forge names tokens "Goblin Token"; Scryfall's token printings are just "Goblin".
        if (e == null && k.endsWith(" token")) e = index.get(k.substring(0, k.length() - " token".length()));
        // Alchemy rebalances ("A-Blood Artist") aren't on Scryfall; show the original card.
        if (e == null && k.startsWith("a-")) e = index.get(k.substring(2));
        return e;
    }

    /** Rulings as JSON text: {"rulings":[{"date","comment"}], "scryfallUri"}; an empty list if the card has none. */
    public String rulings(final String name) throws IOException {
        if (name == null || name.isBlank()) throw new IllegalArgumentException("name required");
        final Entry e = lookup(name);
        final String r = rulingsMap().get((e != null ? e.name() : name).toLowerCase());
        return r != null ? r : "{\"scryfallUri\":null,\"rulings\":[]}";
    }

    private Map<String, String> rulingsMap() throws IOException {
        Map<String, String> m = rulings;
        if (m != null) return m;
        synchronized (this) {
            if (rulings != null) return rulings;
            m = new HashMap<>();
            final Path f = pack == null ? null : pack.resolve("data/rulings.json");
            if (f != null && Files.isRegularFile(f)) {
                try (Reader r = Files.newBufferedReader(f, StandardCharsets.UTF_8)) {
                    for (final Map.Entry<String, JsonElement> e : JsonParser.parseReader(r).getAsJsonObject().entrySet()) m.put(e.getKey(), e.getValue().toString());
                }
            }
            rulings = m;
            return m;
        }
    }
}
