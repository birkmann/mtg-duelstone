package arena.engine.deck;

import arena.engine.view.ViewJson;
import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import forge.StaticData;
import forge.card.CardRules;
import forge.card.CardType;
import forge.deck.DeckFormat;
import forge.game.GameFormat;
import forge.item.PaperCard;
import forge.item.PaperCardPredicates;
import forge.model.FModel;

import java.text.Normalizer;
import java.util.ArrayList;
import java.util.Collection;
import java.util.Comparator;
import java.util.HashMap;
import java.util.HashSet;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Set;
import java.util.function.Predicate;

/**
 * Card metadata and name resolution, backed by Forge's own card database — the only cards that matter are the
 * ones the engine can play, and legality (banlist, commander eligibility) comes from Forge's Commander format
 * so the deck builder can never disagree with the engine.
 */
public final class CardCatalog {
    private static volatile CardCatalog instance;
    public static CardCatalog get() {
        CardCatalog c = instance;
        if (c == null) synchronized (CardCatalog.class) { c = instance; if (c == null) instance = c = new CardCatalog(); }
        return c;
    }

    private final Map<String, PaperCard> byNormName = new HashMap<>();   // normalized name → preferred printing
    private final Map<String, String> faceToName = new HashMap<>();        // normalized face/split name → card name
    private final List<PaperCard> unique;
    private final Set<String> banned = new HashSet<>();
    private final GameFormat commanderFormat;

    private CardCatalog() {
        final Collection<PaperCard> all = StaticData.instance().getCommonCards().getUniqueCards();
        unique = new ArrayList<>(all.size());
        for (final PaperCard pc : all) {
            if (pc.getRules().isUnsupported()) continue;
            unique.add(pc);
            byNormName.put(norm(pc.getName()), pc);
            for (final var face : pc.getRules().getAllFaces()) if (face != null && face.getName() != null) faceToName.putIfAbsent(norm(face.getName()), pc.getName());
        }
        unique.sort(Comparator.comparing(PaperCard::getName, String.CASE_INSENSITIVE_ORDER));
        commanderFormat = FModel.getFormats().getFormat("Commander");
        if (commanderFormat != null) banned.addAll(commanderFormat.getBannedCardNames());
        System.out.println("[catalog] " + unique.size() + " unique cards; commander banlist " + banned.size());
    }

    // ---- normalization / resolution ---------------------------------------------------------

    public static String norm(final String s) {
        if (s == null) return "";
        String n = Normalizer.normalize(s, Normalizer.Form.NFD).replaceAll("\\p{M}", "");
        n = n.toLowerCase(Locale.ROOT).replace('’', '\'').replace("æ", "ae");
        n = n.replaceAll("[^a-z0-9/ ]", " ").replaceAll("\\s+", " ").trim();
        return n;
    }

    /** Exact (normalized) lookup by full name or by one face's name ("Fire" → "Fire // Ice"). */
    public PaperCard resolveExact(final String name) {
        if (name == null) return null;
        final String n = norm(name);
        PaperCard pc = byNormName.get(n);
        if (pc != null) return pc;
        final String full = faceToName.get(n.contains("//") ? n.substring(0, n.indexOf("//")).trim() : n);
        if (full != null) return byNormName.get(norm(full));
        // "Name / Other" or "Name // Other" written with a single slash
        if (n.contains("/")) {
            final String first = n.substring(0, n.indexOf('/')).trim();
            final String f = faceToName.get(first);
            if (f != null) return byNormName.get(norm(f));
        }
        return null;
    }

    /** Best-effort suggestions for a misspelled name. */
    public List<PaperCard> suggest(final String name, final int limit) {
        final String n = norm(name);
        final List<PaperCard> out = new ArrayList<>();
        if (n.isEmpty()) return out;
        // prefix / contains first
        for (final PaperCard pc : unique) { if (norm(pc.getName()).startsWith(n)) { out.add(pc); if (out.size() >= limit) return out; } }
        for (final PaperCard pc : unique) { if (!out.contains(pc) && norm(pc.getName()).contains(n)) { out.add(pc); if (out.size() >= limit) return out; } }
        // edit distance on names of similar length
        final List<PaperCard> fuzzy = new ArrayList<>();
        final Map<PaperCard, Integer> dist = new HashMap<>();
        for (final PaperCard pc : unique) {
            final String m = norm(pc.getName());
            if (Math.abs(m.length() - n.length()) > 3) continue;
            final int d = levenshtein(n, m, 3);
            if (d <= 3) { fuzzy.add(pc); dist.put(pc, d); }
        }
        fuzzy.sort(Comparator.comparingInt(dist::get));
        for (final PaperCard pc : fuzzy) { if (!out.contains(pc)) { out.add(pc); if (out.size() >= limit) break; } }
        return out;
    }

    private static int levenshtein(final String a, final String b, final int max) {
        final int[] prev = new int[b.length() + 1], cur = new int[b.length() + 1];
        for (int j = 0; j <= b.length(); j++) prev[j] = j;
        for (int i = 1; i <= a.length(); i++) {
            cur[0] = i; int rowMin = cur[0];
            for (int j = 1; j <= b.length(); j++) {
                final int cost = a.charAt(i - 1) == b.charAt(j - 1) ? 0 : 1;
                cur[j] = Math.min(Math.min(cur[j - 1] + 1, prev[j] + 1), prev[j - 1] + cost);
                rowMin = Math.min(rowMin, cur[j]);
            }
            if (rowMin > max) return max + 1;
            System.arraycopy(cur, 0, prev, 0, cur.length);
        }
        return prev[b.length()];
    }

    // ---- search ---------------------------------------------------------------------------------

    /**
     * @param q     name substring (or empty)
     * @param ci    color identity the card must fit within, e.g. "WUB" ("C" = colorless only), or null
     * @param type  type line substring, or null
     * @param text  oracle text substring, or null
     * @param commanderOnly only cards that can be a commander
     */
    public List<PaperCard> search(final String q, final String ci, final String type, final String text, final boolean commanderOnly, final int limit) {
        final String n = norm(q);
        final byte ciMask = ci == null || ci.isBlank() ? -1 : colorMask(ci);
        final String typeN = type == null ? null : type.toLowerCase(Locale.ROOT).trim();
        final String textN = text == null ? null : text.toLowerCase(Locale.ROOT).trim();
        final Predicate<PaperCard> cmd = PaperCardPredicates.CAN_BE_COMMANDER;
        final List<PaperCard> prefix = new ArrayList<>(), contains = new ArrayList<>();
        for (final PaperCard pc : unique) {
            final CardRules r = pc.getRules();
            if (ciMask >= 0 && (r.getColorIdentity().getColor() & ~ciMask) != 0) continue;
            if (typeN != null && !typeN.isEmpty() && !r.getType().toString().toLowerCase(Locale.ROOT).contains(typeN)) continue;
            if (textN != null && !textN.isEmpty() && (r.getOracleText() == null || !r.getOracleText().toLowerCase(Locale.ROOT).contains(textN))) continue;
            if (commanderOnly && !cmd.test(pc)) continue;
            if (n.isEmpty()) { prefix.add(pc); }
            else {
                final String m = norm(pc.getName());
                if (m.startsWith(n)) prefix.add(pc); else if (m.contains(n)) contains.add(pc);
            }
            if (prefix.size() >= limit) break;
        }
        final List<PaperCard> out = new ArrayList<>(prefix);
        for (final PaperCard pc : contains) { if (out.size() >= limit) break; out.add(pc); }
        return out;
    }

    public static byte colorMask(final String ci) {
        byte m = 0;
        for (final char c : ci.toUpperCase(Locale.ROOT).toCharArray()) switch (c) {
            case 'W' -> m |= forge.card.MagicColor.WHITE; case 'U' -> m |= forge.card.MagicColor.BLUE; case 'B' -> m |= forge.card.MagicColor.BLACK;
            case 'R' -> m |= forge.card.MagicColor.RED; case 'G' -> m |= forge.card.MagicColor.GREEN; default -> {}
        }
        return m;
    }

    // ---- legality -----------------------------------------------------------------------------------

    public boolean isBanned(final PaperCard pc) { return banned.contains(pc.getName()); }
    public boolean canBeCommander(final PaperCard pc) { return PaperCardPredicates.CAN_BE_COMMANDER.test(pc) && DeckFormat.Commander.isLegalCommander(pc.getRules()); }

    // ---- JSON -----------------------------------------------------------------------------------------

    public JsonObject cardJson(final PaperCard pc) {
        final CardRules r = pc.getRules();
        final JsonObject o = new JsonObject();
        o.addProperty("name", pc.getName());
        o.addProperty("manaCost", r.getManaCost() == null || r.getManaCost().isNoCost() ? null : r.getManaCost().toString());
        o.addProperty("cmc", r.getManaCost() == null ? 0 : r.getManaCost().getCMC());
        final CardType t = r.getType();
        o.addProperty("type", t == null ? "" : t.toString());
        o.addProperty("colors", ViewJson.colors(r.getColor()));
        o.addProperty("colorIdentity", ViewJson.colors(r.getColorIdentity()));
        o.addProperty("oracleText", r.getOracleText());
        if (t != null && t.isCreature()) { o.addProperty("power", r.getPower()); o.addProperty("toughness", r.getToughness()); }
        if (t != null && t.isPlaneswalker()) o.addProperty("loyalty", r.getInitialLoyalty());
        o.addProperty("set", pc.getEdition());
        o.addProperty("cn", pc.getCollectorNumber());
        o.addProperty("rarity", pc.getRarity() == null ? null : pc.getRarity().name());
        o.addProperty("isLand", t != null && t.isLand());
        o.addProperty("isCreature", t != null && t.isCreature());
        o.addProperty("isBasicLand", t != null && t.isBasicLand());
        o.addProperty("canBeCommander", canBeCommander(pc));
        o.addProperty("banned", isBanned(pc));
        return o;
    }

    public JsonArray cardsJson(final Collection<PaperCard> pcs) {
        final JsonArray a = new JsonArray();
        for (final PaperCard pc : pcs) a.add(cardJson(pc));
        return a;
    }
}
