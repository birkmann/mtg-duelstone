package arena.engine.deck;

import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import forge.deck.Deck;
import forge.deck.DeckSection;
import forge.deck.io.DeckSerializer;
import forge.item.PaperCard;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

/**
 * Parses pasted decklists — plain text ("1 Sol Ring"), Moxfield/Archidekt/TappedOut exports
 * ("1 Sol Ring (C21) 263 *F*", "1x Sol Ring (c21) 263 [Artifact]"), MTGO/Arena, and Forge .dck —
 * and resolves every line against Forge's card DB. Nothing is saved: the result lists what resolved,
 * what didn't (with suggestions) and which cards look like commanders, for the user to confirm.
 */
public final class DeckImporter {
    private static final Pattern LINE = Pattern.compile("^\\s*(?:(\\d+)\\s*[xX]?\\s+)?(.+?)\\s*$");
    private static final Pattern SET_SUFFIX = Pattern.compile("\\s*[\\(\\[]([A-Za-z0-9]{2,6})[\\)\\]]\\s*([A-Za-z0-9\\-★]+)?\\s*(\\*F\\*|\\*E\\*|\\*foil\\*)?\\s*$", Pattern.CASE_INSENSITIVE);
    private static final Pattern CATEGORY = Pattern.compile("\\s*\\[([^\\]]+)\\]\\s*$"); // archidekt "[Commander{top}]" / "[Land]"
    private static final Pattern HEADER = Pattern.compile("^(?://\\s*)?(commander|commanders|command zone|main|mainboard|main deck|deck|sideboard|side|maybeboard|maybe|companion|tokens?|lands?|creatures?|instants?|sorcer(?:y|ies)|artifacts?|enchantments?|planeswalkers?|battles?|spells?|other|nonland|non-land)s?\\s*(?:\\(\\d+\\))?\\s*:?\\s*$", Pattern.CASE_INSENSITIVE);

    private final CardCatalog catalog = CardCatalog.get();

    public JsonObject parse(final String text) {
        final Map<String, Integer> main = new LinkedHashMap<>(), side = new LinkedHashMap<>();
        final List<String> commanders = new ArrayList<>();
        final List<JsonObject> unresolved = new ArrayList<>();
        final List<String> warnings = new ArrayList<>();

        if (text.contains("[Main]") || text.contains("[metadata]") || text.contains("[Commander]")) {
            return fromDck(text);
        }
        String section = "main";
        for (String raw : text.split("\\r?\\n")) {
            String line = raw.trim();
            if (line.isEmpty()) { if (section.equals("commander")) section = "main"; continue; } // blank line ends a commander block (Moxfield/Archidekt)
            if (line.startsWith("#")) continue;
            final Matcher h = HEADER.matcher(line);
            if (h.matches()) {
                final String hd = h.group(1).toLowerCase(Locale.ROOT);
                section = hd.startsWith("command") ? "commander" : hd.startsWith("side") ? "side" : hd.startsWith("maybe") ? "maybe" : hd.startsWith("companion") ? "side" : "main";
                continue;
            }
            if (section.equals("maybe")) continue;
            boolean asCommander = section.equals("commander");
            boolean asSide = section.equals("side");
            if (line.toUpperCase(Locale.ROOT).startsWith("SB:")) { asSide = true; line = line.substring(3).trim(); }
            if (line.toUpperCase(Locale.ROOT).startsWith("CMDR:")) { asCommander = true; line = line.substring(5).trim(); }
            // archidekt category tag at end
            final Matcher cat = CATEGORY.matcher(line);
            if (cat.find() && !line.matches(".*\\[[A-Za-z0-9]{2,6}\\]\\s*$")) {
                final String c = cat.group(1).toLowerCase(Locale.ROOT);
                if (c.startsWith("commander")) asCommander = true;
                if (c.startsWith("sideboard") || c.startsWith("maybe")) asSide = true;
                line = line.substring(0, cat.start()).trim();
            }
            // moxfield "*CMDR*" marker
            if (line.contains("*CMDR*")) { asCommander = true; line = line.replace("*CMDR*", "").trim(); }
            final Matcher m = LINE.matcher(line);
            if (!m.matches()) continue;
            int qty = m.group(1) == null ? 1 : Integer.parseInt(m.group(1));
            String name = m.group(2);
            String set = null;
            final Matcher s = SET_SUFFIX.matcher(name);
            if (s.find() && s.start() > 0) { set = s.group(1); name = name.substring(0, s.start()).trim(); }
            name = name.replaceAll("\\s+\\*[A-Za-z]+\\*$", "").trim(); // "*F*" style markers without set
            final PaperCard pc = catalog.resolveExact(name);
            if (pc == null) {
                final JsonObject u = new JsonObject();
                u.addProperty("line", raw);
                u.addProperty("name", name);
                u.addProperty("qty", qty);
                u.addProperty("section", asCommander ? "commander" : asSide ? "side" : "main");
                final JsonArray sug = new JsonArray();
                for (final PaperCard c : catalog.suggest(name, 5)) sug.add(c.getName());
                u.add("suggestions", sug);
                unresolved.add(u);
                continue;
            }
            if (asCommander) { commanders.add(pc.getName()); continue; }
            (asSide ? side : main).merge(pc.getName(), qty, Integer::sum);
        }
        // No explicit commander: suggest legal commanders found in the list (legendary creatures etc.)
        final JsonArray candidates = new JsonArray();
        if (commanders.isEmpty()) for (final String n : main.keySet()) { final PaperCard pc = catalog.resolveExact(n); if (pc != null && catalog.canBeCommander(pc)) candidates.add(n); }
        final int total = main.values().stream().mapToInt(Integer::intValue).sum() + commanders.size();
        if (total != 100 && unresolved.isEmpty()) warnings.add("Deck has " + total + " cards (Commander needs 100 including the commander).");
        return result(commanders, main, side, unresolved, candidates, warnings);
    }

    private JsonObject fromDck(final String text) {
        final Map<String, List<String>> sections = new LinkedHashMap<>();
        String cur = "main";
        for (final String raw : text.split("\\r?\\n")) {
            final String line = raw.trim();
            if (line.isEmpty()) continue;
            if (line.startsWith("[") && line.endsWith("]")) { cur = line.substring(1, line.length() - 1).toLowerCase(Locale.ROOT); sections.computeIfAbsent(cur, k -> new ArrayList<>()); continue; }
            sections.computeIfAbsent(cur, k -> new ArrayList<>()).add(line);
        }
        final Deck d = DeckSerializer.fromSections(sections);
        final Map<String, Integer> main = new LinkedHashMap<>(), side = new LinkedHashMap<>();
        final List<String> commanders = new ArrayList<>();
        if (d != null) {
            for (final PaperCard pc : d.getCommanders()) commanders.add(pc.getName());
            for (final Map.Entry<PaperCard, Integer> e : d.getMain()) main.merge(e.getKey().getName(), e.getValue(), Integer::sum);
            if (d.get(DeckSection.Sideboard) != null) for (final Map.Entry<PaperCard, Integer> e : d.get(DeckSection.Sideboard)) side.merge(e.getKey().getName(), e.getValue(), Integer::sum);
        }
        final JsonObject r = result(commanders, main, side, List.of(), new JsonArray(), new ArrayList<>());
        if (d != null && d.getName() != null) r.addProperty("name", d.getName());
        return r;
    }

    private JsonObject result(final List<String> commanders, final Map<String, Integer> main, final Map<String, Integer> side, final List<JsonObject> unresolved, final JsonArray candidates, final List<String> warnings) {
        final JsonObject r = new JsonObject();
        final JsonArray cmd = new JsonArray();
        for (final String c : commanders) cmd.add(catalog.cardJson(catalog.resolveExact(c)));
        r.add("commanders", cmd);
        r.add("main", entries(main));
        r.add("sideboard", entries(side));
        final JsonArray u = new JsonArray(); unresolved.forEach(u::add); r.add("unresolved", u);
        r.add("commanderCandidates", candidates);
        final JsonArray w = new JsonArray(); warnings.forEach(w::add); r.add("warnings", w);
        return r;
    }
    private JsonArray entries(final Map<String, Integer> m) {
        final JsonArray a = new JsonArray();
        for (final Map.Entry<String, Integer> e : m.entrySet()) { final JsonObject o = new JsonObject(); o.add("card", catalog.cardJson(catalog.resolveExact(e.getKey()))); o.addProperty("qty", e.getValue()); a.add(o); }
        return a;
    }
}
