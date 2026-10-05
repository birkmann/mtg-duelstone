package arena.engine.server;

import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import forge.deck.Deck;
import forge.deck.io.DeckSerializer;
import forge.item.PaperCard;
import forge.model.FModel;
import forge.util.storage.IStorage;

import java.io.File;
import java.util.ArrayList;
import java.util.List;

/**
 * Where decks come from. A deck ref is {@code "precon:<name>"} (Forge's shipped Commander precons),
 * {@code "user:<name>"} (Forge's own ~/.forge/decks/commander), or {@code "file:<path>"} (any .dck,
 * e.g. exported by our deck builder).
 */
public final class DeckSource {
    private DeckSource() {}

    // Forge's deck storage + CardDb lazy-load deck sections and are not thread-safe: serialize all access.
    private static final Object LOCK = new Object();

    public static JsonArray list() {
        synchronized (LOCK) { return listLocked(); }
    }
    private static JsonArray listLocked() {
        final JsonArray a = new JsonArray();
        for (final Deck d : FModel.getDecks().getCommanderPrecons()) a.add(describe("precon:" + d.getName(), d));
        for (final Deck d : FModel.getDecks().getCommander()) a.add(describe("user:" + d.getName(), d));
        return a;
    }

    public static JsonObject describe(final String ref, final Deck d) {
        final JsonObject o = new JsonObject();
        o.addProperty("ref", ref);
        o.addProperty("name", d.getName());
        final JsonArray cmd = new JsonArray();
        for (final PaperCard pc : d.getCommanders()) cmd.add(pc.getName());
        o.add("commanders", cmd);
        o.addProperty("mainCount", d.getMain().countAll());
        final List<String> ci = new ArrayList<>();
        for (final PaperCard pc : d.getCommanders()) ci.add(arena.engine.view.ViewJson.colors(pc.getRules().getColorIdentity()));
        o.addProperty("colorIdentity", String.join("", ci));
        return o;
    }

    public static Deck load(final String ref) {
        synchronized (LOCK) { return loadLocked(ref); }
    }
    private static Deck loadLocked(final String ref) {
        if (ref == null) throw new IllegalArgumentException("deck ref required");
        final int colon = ref.indexOf(':');
        final String kind = colon < 0 ? "precon" : ref.substring(0, colon);
        final String name = colon < 0 ? ref : ref.substring(colon + 1);
        switch (kind) {
            case "precon": return require(FModel.getDecks().getCommanderPrecons(), name, ref);
            case "user": return require(FModel.getDecks().getCommander(), name, ref);
            case "file": {
                final File f = new File(name);
                if (!f.isFile()) throw new IllegalArgumentException("deck file not found: " + name);
                final Deck d = DeckSerializer.fromFile(f);
                if (d == null) throw new IllegalArgumentException("cannot parse deck file: " + name);
                return d;
            }
            default: throw new IllegalArgumentException("unknown deck ref kind: " + kind);
        }
    }

    private static Deck require(final IStorage<Deck> store, final String name, final String ref) {
        final Deck d = store.get(name);
        if (d != null) return d;
        for (final Deck x : store) if (x.getName().equalsIgnoreCase(name)) return x;
        throw new IllegalArgumentException("deck not found: " + ref);
    }
}
