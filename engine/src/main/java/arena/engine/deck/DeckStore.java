package arena.engine.deck;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import forge.deck.CardPool;
import forge.deck.Deck;
import forge.deck.DeckFormat;
import forge.deck.DeckSection;
import forge.deck.io.DeckSerializer;
import forge.item.PaperCard;
import forge.model.FModel;
import forge.util.storage.IStorage;
import arena.engine.server.DeckSource;

import java.io.File;
import java.io.IOException;
import java.nio.file.Files;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;

/**
 * User decks live as Forge {@code .dck} files in Forge's own Commander deck folder (~/.forge/decks/commander),
 * so anything built here is also playable in Forge desktop and vice-versa. Validation is Forge's
 * {@link DeckFormat#Commander} conformance check — the same rule set the engine enforces.
 */
public final class DeckStore {
    private static final Object LOCK = new Object();
    private final CardCatalog catalog = CardCatalog.get();

    private static IStorage<Deck> store(final String format) {
        return DeckSource.CONSTRUCTED.equals(format) ? FModel.getDecks().getConstructed() : FModel.getDecks().getCommander();
    }

    // ---- read ---------------------------------------------------------------------------------

    public JsonArray list(final String format) {
        synchronized (LOCK) {
            final JsonArray a = new JsonArray();
            for (final Deck d : store(format)) a.add(summary("user:" + d.getName(), d, format));
            return a;
        }
    }

    public JsonObject summary(final String ref, final Deck d, final String format) {
        final JsonObject o = DeckSource.describe(ref, d, format);
        o.addProperty("problem", problem(d, format));
        return o;
    }

    public static String problem(final Deck d, final String format) { return DeckSource.problem(d, format); }

    public JsonObject full(final String ref, final Deck d, final String format) {
        final JsonObject o = summary(ref, d, format);
        o.add("commanders", catalog.cardsJson(d.getCommanders()));
        o.add("main", pool(d.getMain()));
        o.add("sideboard", pool(d.get(DeckSection.Sideboard)));
        return o;
    }

    private JsonArray pool(final CardPool p) {
        final JsonArray a = new JsonArray();
        if (p == null) return a;
        for (final Map.Entry<PaperCard, Integer> e : p) {
            final JsonObject o = new JsonObject();
            o.add("card", catalog.cardJson(e.getKey()));
            o.addProperty("qty", e.getValue());
            a.add(o);
        }
        return a;
    }

    /** format may be null (search commander, then constructed). */
    public Deck get(final String name, final String format) {
        synchronized (LOCK) { return DeckSource.load(name.contains(":") ? name : "user:" + name, format); }
    }

    // ---- write --------------------------------------------------------------------------------

    /** Build a Deck from {name, commanders:[name], main:[{name, qty, set?}], sideboard?} without saving. */
    public Deck build(final JsonObject body) {
        final String name = body.has("name") && !body.get("name").isJsonNull() ? body.get("name").getAsString().trim() : "Untitled";
        final Deck d = new Deck(name.isEmpty() ? "Untitled" : name);
        final List<String> missing = new ArrayList<>();
        if (body.has("commanders")) for (final JsonElement e : body.getAsJsonArray("commanders")) {
            final PaperCard pc = catalog.resolveExact(e.getAsString());
            if (pc == null) missing.add(e.getAsString()); else d.getOrCreate(DeckSection.Commander).add(pc);
        }
        addPool(d.getOrCreate(DeckSection.Main), body.get("main"), missing);
        if (body.has("sideboard")) addPool(d.getOrCreate(DeckSection.Sideboard), body.get("sideboard"), missing);
        if (!missing.isEmpty()) throw new IllegalArgumentException("unknown cards: " + String.join(", ", missing));
        return d;
    }

    /** {problem: string|null} for an unsaved deck body. */
    public JsonObject validate(final JsonObject body) {
        final JsonObject o = new JsonObject();
        o.addProperty("problem", problem(build(body), formatOf(body)));
        return o;
    }

    /** Body: {name, commanders:[name], main:[{name, qty, set?}], sideboard?:[{name, qty}]} → saves and returns the full deck JSON. */
    public static String formatOf(final JsonObject body) {
        return DeckSource.format(body.has("format") && !body.get("format").isJsonNull() ? body.get("format").getAsString() : null);
    }

    public JsonObject save(final JsonObject body, final String previousName) {
        final String format = formatOf(body);
        final String name = body.get("name").getAsString().trim();
        if (name.isEmpty()) throw new IllegalArgumentException("deck name required");
        final Deck d = build(body);
        synchronized (LOCK) {
            if (previousName != null && !previousName.equals(name)) store(format).delete(previousName);
            store(format).add(d);
        }
        return full("user:" + name, d, format);
    }

    private void addPool(final CardPool pool, final JsonElement arr, final List<String> missing) {
        if (arr == null || !arr.isJsonArray()) return;
        for (final JsonElement e : arr.getAsJsonArray()) {
            final JsonObject o = e.getAsJsonObject();
            final String n = o.get("name").getAsString();
            final int qty = o.has("qty") ? o.get("qty").getAsInt() : 1;
            final String set = o.has("set") && !o.get("set").isJsonNull() ? o.get("set").getAsString() : null;
            PaperCard pc = null;
            if (set != null && !set.isBlank()) pc = forge.StaticData.instance().getCommonCards().getCard(n, set);
            if (pc == null) pc = catalog.resolveExact(n);
            if (pc == null) { missing.add(n); continue; }
            pool.add(pc, Math.max(1, qty));
        }
    }

    public void delete(final String name, final String format) {
        synchronized (LOCK) { store(format).delete(name); }
    }

    /** Forge .dck text for download/export. */
    public String export(final Deck d) throws IOException {
        final File tmp = Files.createTempFile("arena-", ".dck").toFile();
        try { DeckSerializer.writeDeck(d, tmp); return Files.readString(tmp.toPath()); }
        finally { tmp.delete(); }
    }
}
