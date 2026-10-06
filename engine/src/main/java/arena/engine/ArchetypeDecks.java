package arena.engine;

import arena.engine.server.DeckSource;
import forge.card.ColorSet;
import forge.deck.CardArchetypeLDAGenerator;
import forge.deck.Deck;
import forge.deck.DeckFormat;
import forge.deck.DeckgenUtil;
import forge.deck.io.Archetype;
import forge.deck.io.DeckSerializer;
import forge.game.GameFormat;
import forge.item.PaperCard;

import java.io.File;
import java.util.HashMap;
import java.util.HashSet;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Set;

/**
 * Dev tool: writes one deck per Forge metagame archetype ({@code res/deckgendecks/<Format>.lda.dat}) into
 * {@code decks/archetypes/<legality>/}, for {@code java -cp forge-ws-server.jar arena.engine.ArchetypeDecks [decks/archetypes]}.
 *
 * Why committed files instead of generating at runtime: Forge's generator is random, so a deck the player
 * picked (and the client remembers by ref) would change under them on every restart. Bundled theme and
 * duel decks leave Pauper (~20 legal) and Standard thin; these fill every legality with real archetypes.
 */
public final class ArchetypeDecks {
    private ArchetypeDecks() {}

    public static void main(final String[] args) throws Exception {
        final File out = new File(args.length > 0 ? args[0] : "decks/archetypes");
        Bootstrap.init(() -> new arena.engine.server.WsGuiGame(f -> {}));
        final Set<String> vocab = vocabulary();
        for (final String legality : DeckSource.LEGALITIES) {
            final GameFormat gf = DeckSource.gameFormat(legality);
            final List<Archetype> archetypes = CardArchetypeLDAGenerator.ldaArchetypes.get(gf.getName());
            if (archetypes == null) { System.out.println(legality + ": no archetype data"); continue; }
            final File dir = new File(out, legality);
            dir.mkdirs();
            for (final File old : dir.listFiles((d, n) -> n.endsWith(".dck"))) old.delete();
            final Set<String> names = new HashSet<>();
            int written = 0;
            for (final Archetype a : archetypes) {
                final Deck d = build(a, gf, legality);
                if (d == null) { System.out.println("  skipped (no legal build): " + a.getName()); continue; }
                String name = name(a.getName(), d, vocab);
                if (!names.add(name.toLowerCase(Locale.ROOT))) {
                    // several archetypes cluster around one strategy: tell them apart by their key card
                    final PaperCard key = DeckSource.cover(d);
                    final String keyName = key == null ? "" : key.getName().replaceFirst(",.*", "");
                    if (keyName.isEmpty() || name.toLowerCase(Locale.ROOT).contains(keyName.toLowerCase(Locale.ROOT))) continue; // same deck again
                    name = name + " (" + keyName + ")";
                    if (!names.add(name.toLowerCase(Locale.ROOT))) continue;
                }
                d.setName(name);
                d.setComment(gf.getName() + " metagame · " + a.getDeckCount() + " tournament decks");
                DeckSerializer.writeDeck(d, new File(dir, slug(name) + ".dck"));
                written++;
            }
            System.out.println(legality + ": " + written + " decks from " + archetypes.size() + " archetypes");
        }
    }

    /**
     * The generator randomly drops and adds cards, so an attempt can miss the format; retry a few times. On a bad
     * build Forge silently starts over from a random archetype, so a deck not named after ours is someone else's.
     */
    private static Deck build(final Archetype a, final GameFormat gf, final String legality) {
        for (int i = 0; i < 8; i++) {
            try {
                final Deck d = DeckgenUtil.buildLDACArchetypeDeck(a, gf, true);
                if (!(a.getName() + " Generated Deck").equals(d.getName())) continue;
                if (DeckFormat.Constructed.getDeckConformanceProblem(d) == null && DeckSource.problem(d, DeckSource.CONSTRUCTED, legality) == null) return d;
            } catch (final Exception ignored) { /* retry */ }
        }
        return null;
    }

    private static final Set<String> COLOR_WORDS = Set.of("white", "blue", "black", "red", "green", "mono", "monoblack", "monored",
            "monogreen", "monowhite", "monoblue", "colorless", "color", "colors", "multicolor", "two", "three", "four", "five",
            "azorius", "dimir", "rakdos", "gruul", "selesnya", "orzhov", "izzet", "golgari", "boros", "simic",
            "esper", "grixis", "jund", "naya", "bant", "abzan", "jeskai", "sultai", "mardu", "temur", "domain",
            "w", "u", "b", "r", "g", "c", "wu", "uw", "ub", "bu", "br", "rb", "rg", "gr", "gw", "wg", "wb", "bw", "ur", "ru", "bg", "gb", "rw", "wr", "gu", "ug",
            "uwx", "ubx", "mbc", "deck", "decks", "the", "and", "of", "a", "x", "list");
    /** Archetype words that are no card's name or creature type. */
    private static final Set<String> TERMS = Set.of("aggro", "control", "midrange", "tempo", "combo", "ramp", "tokens", "burn",
            "storm", "tron", "affinity", "prowess", "landfall", "reanimator", "reanimate", "delver", "stompy", "bogles", "hexproof",
            "infect", "poison", "heroic", "lifegain", "blink", "flicker", "monarch", "initiative", "cascade", "madness", "metalcraft",
            "sacrifice", "aristocrats", "pile", "fish", "weenie", "sligh", "post", "depths", "dredge", "blitz", "flash", "convoke",
            "mill", "discard", "voltron", "auras", "equipment", "artifacts", "enchantments", "gates", "lands", "lessons", "spells",
            "toolbox", "turbo", "highlander", "cycling", "evoke", "flashback", "delirium", "omniscience", "devotion", "graveyard",
            "rogues", "spirits", "otters", "dragons", "eldrazi", "slivers", "elementals", "monument", "synthesizer", "walls", "fog");

    /** Words that can stand in a deck's name: every card-name word and creature type, plus archetype jargon. */
    private static Set<String> vocabulary() {
        final Set<String> v = new HashSet<>(TERMS);
        for (final PaperCard pc : forge.model.FModel.getMagicDb().getCommonCards().getUniqueCards()) {
            for (final String w : pc.getName().toLowerCase(Locale.ROOT).split("[^a-z']+")) if (w.length() > 2) v.add(w);
        }
        for (final String t : forge.card.CardType.getAllCreatureTypes()) v.add(t.toLowerCase(Locale.ROOT));
        return v;
    }

    private static boolean known(final String w, final Set<String> vocab) {
        return vocab.contains(w) || (w.endsWith("s") && vocab.contains(w.substring(0, w.length() - 1)))
                || (w.endsWith("es") && vocab.contains(w.substring(0, w.length() - 2)));
    }

    /**
     * "Burn Red Deck" → "Mono-Red Burn", "Izzet Prowess Arianne" → "Izzet Prowess". Forge's LDA names are the
     * top words of the clustered decks' titles: colours in any notation, player handles, translations. The colour
     * part comes from the deck's own spells; the rest keeps the (at most two) words Magic players would recognise.
     */
    static String name(final String raw, final Deck d, final Set<String> vocab) {
        final List<String> keep = new java.util.ArrayList<>();
        for (final String w0 : raw.toLowerCase(Locale.ROOT).split("[\\s()]+")) {
            final String w = w0.replaceAll("[^a-z'-]", "").replaceFirst("'s$", "");
            if (w.isEmpty() || w.contains("-") || COLOR_WORDS.contains(w) || !known(w, vocab)) continue;
            final String stemmed = w.replaceFirst("e?s$", "");
            if (keep.stream().anyMatch(k -> k.replaceFirst("e?s$", "").equals(stemmed))) continue;
            keep.add(w);
            if (keep.size() == 2) break;
        }
        final StringBuilder b = new StringBuilder(colorName(d));
        for (final String w : keep) b.append(' ').append(Character.toUpperCase(w.charAt(0))).append(w.substring(1));
        if (keep.isEmpty()) {
            final PaperCard key = DeckSource.cover(d);
            if (key != null) b.append(' ').append(key.getName().replaceFirst(",.*", ""));
        }
        return b.toString();
    }

    private static final Map<Integer, String> GUILDS = new HashMap<>();
    static {
        final String[][] g = {{"WU", "Azorius"}, {"UB", "Dimir"}, {"BR", "Rakdos"}, {"RG", "Gruul"}, {"GW", "Selesnya"}, {"WB", "Orzhov"},
                {"UR", "Izzet"}, {"BG", "Golgari"}, {"RW", "Boros"}, {"GU", "Simic"}, {"WUB", "Esper"}, {"UBR", "Grixis"}, {"BRG", "Jund"},
                {"RGW", "Naya"}, {"GWU", "Bant"}, {"WBG", "Abzan"}, {"URW", "Jeskai"}, {"BGU", "Sultai"}, {"RWB", "Mardu"}, {"GUR", "Temur"},
                {"W", "Mono-White"}, {"U", "Mono-Blue"}, {"B", "Mono-Black"}, {"R", "Mono-Red"}, {"G", "Mono-Green"}};
        for (final String[] e : g) GUILDS.put((int) ColorSet.fromNames(e[0].toCharArray()).getColor(), e[1]);
    }

    /** The colours of the deck's spells (lands and their colour identity don't make an Izzet deck Grixis). */
    private static String colorName(final Deck d) {
        int mask = 0;
        for (final Map.Entry<PaperCard, Integer> e : d.getMain()) {
            if (!e.getKey().getRules().getType().isLand()) mask |= e.getKey().getRules().getColor().getColor();
        }
        final int n = Integer.bitCount(mask);
        return n == 0 ? "Colorless" : n == 4 ? "Four-Color" : n == 5 ? "Five-Color" : GUILDS.getOrDefault(mask, "Multicolor");
    }

    private static String slug(final String name) {
        return name.toLowerCase(Locale.ROOT).replaceAll("[^a-z0-9]+", "-").replaceAll("(^-|-$)", "");
    }
}
