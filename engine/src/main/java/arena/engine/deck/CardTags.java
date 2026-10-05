package arena.engine.deck;

import forge.card.CardRules;
import forge.card.CardType;
import forge.card.ICardFace;
import forge.item.PaperCard;

import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;
import java.util.function.Predicate;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

/**
 * Role and theme tags ("ramp", "removal", "counters", …) read off oracle text and type line. The deck builder
 * needs them for EDHREC-style sections and theme detection, and Forge has no such classification, so this is a
 * deliberately simple, tunable keyword table — one row per tag, regexes over lowercased text with reminder text
 * stripped and the card's own name replaced by {@code ~}. Tag keys are a stable contract with the Rust client.
 */
public final class CardTags {
    private CardTags() {}

    /** Card facts a rule may look at besides the text. */
    private record Info(String text, CardType type, boolean land, int mask) {}

    /**
     * One tag: applies when {@code gate} holds and either a text pattern matches or {@code alt} holds.
     * Rules run in table order and see the tags found so far ({@code Info.mask}), so later rules may depend on
     * earlier ones (artifacts skips mana rocks).
     */
    private record Rule(String key, String label, boolean role, Predicate<Info> gate, List<Alt> text, Predicate<Info> alt) {
        boolean matches(final String t) { for (final Alt a : text) if ((a.literal == null || t.contains(a.literal)) && a.rx.matcher(t).find()) return true; return false; }
    }
    /** One pattern plus a literal it cannot match without — a String.contains prefilter makes tagging ~10x faster. */
    private record Alt(String literal, Pattern rx) {}

    /** Longest run of plain characters outside groups/classes/quantifiers, i.e. text every match must contain. */
    static String literalOf(final String pat) {
        String best = "";
        final StringBuilder run = new StringBuilder();
        int depth = 0;
        for (int i = 0; i < pat.length(); i++) {
            final char c = pat.charAt(i);
            if (c == '\\' && i + 1 < pat.length()) {
                final char n = pat.charAt(++i);
                if (depth == 0 && !Character.isLetterOrDigit(n)) { run.append(n); continue; }
            } else if (c == '[') {
                while (i < pat.length() && pat.charAt(i) != ']') i += pat.charAt(i) == '\\' ? 2 : 1;
            } else if (c == '(') { depth++;
            } else if (c == ')') { depth--;
            } else if (c == '?' || c == '*' || c == '+' || c == '{') {
                if (depth == 0 && run.length() > 0) run.setLength(run.length() - 1);   // the quantified char is optional
            } else if (depth == 0 && c != '|' && c != '.' && c != '^' && c != '$') { run.append(c); continue; }
            if (run.toString().trim().length() > best.length()) best = run.toString().trim();
            run.setLength(0);
        }
        if (run.toString().trim().length() > best.length()) best = run.toString().trim();
        return best.length() >= 3 ? best : null;
    }

    private static final String NUM = "(?:a|an|one|two|three|four|five|six|seven|eight|x|that many|\\d+)";
    private static final List<Rule> RULES = new ArrayList<>();
    private static void role(final String key, final String label, final Predicate<Info> gate, final Predicate<Info> alt, final String... pats) { add(key, label, true, gate, alt, pats); }
    private static void theme(final String key, final String label, final Predicate<Info> gate, final Predicate<Info> alt, final String... pats) { add(key, label, false, gate, alt, pats); }
    /** Keyword list, one pattern per word so each gets its own prefilter literal; {@link #add} splits on the NUL. */
    private static String words(final String... ws) {
        final StringBuilder sb = new StringBuilder();
        for (final String w : ws) sb.append(sb.length() == 0 ? "" : "\0").append("\\b").append(Pattern.quote(w)).append("s?\\b");
        return sb.toString();
    }
    private static void add(final String key, final String label, final boolean role, final Predicate<Info> gate, final Predicate<Info> alt, final String... pats) {
        final List<Alt> alts = new ArrayList<>();
        for (final String ps : pats) for (final String p : ps.split("\0")) alts.add(new Alt(literalOf(p), Pattern.compile(p)));
        RULES.add(new Rule(key, label, role, gate, alts, alt));
    }

    // Tutor = "search your library for <X> card" where X is not a land: lands are ramp/fixing, not tutors.
    private static final Pattern SEARCH = Pattern.compile("search your library (?:and(?:/or)? (?:your )?graveyard )?for ([^.]*?)\\bcards?\\b");
    private static final Pattern LANDISH = Pattern.compile("\\b(land|lands|basic|forest|plains|island|swamp|mountain|gate|desert)\\b");
    // Token theme = creature tokens; a deck making only Treasures/Clues/Food is ramp/draw, not "go wide".
    private static final Pattern CREATE = Pattern.compile("(?<!controller )(?<!player )\\bcreates? ([^.]*?)\\btokens?\\b");
    private static final Pattern UTILITY_TOKEN = Pattern.compile("\\b(treasure|clue|food|blood|gold|map|powerstone|incubator|junk|shard|lander)\\b");

    static {
        // ---- roles ---------------------------------------------------------------------------------------
        role("ramp", "Ramp", i -> !i.land, null,
                "\\badds? (?:\\{|(?:one|two|three|x|that much|an amount of|an additional) mana|mana of)",
                "search your library for [^.]*\\b(?:lands?|forests?|plains|islands?|swamps?|mountains?)\\b[^.]*onto the battlefield",
                "play (?:an|two|three|any number of) additional lands?", "you may play (?:two|three) lands",
                "put [^.]*land cards? from your hand onto the battlefield", "\\bcreates? [^.]*treasure tokens?");
        role("draw", "Card Draw", null, null,
                "(?<!opponent )(?<!opponents )\\bdraws? (?:a card|an additional card|" + NUM + " (?:additional )?cards|cards equal|a number of cards|cards)",
                "exile the top [^\\n]*?you may (?:play|cast)", "look at the top [^\\n]*into your hand", "\\binvestigate\\b", "\\bclue tokens?\\b");
        role("removal", "Removal", null, null,
                "\\b(?:destroy|exile) (?:up to " + NUM + " )?(?:other |another )?target (?!cards?\\b)(?:[a-z\\-]+ )*?(?:creature|permanent|artifact|enchantment|planeswalker|battle|land)s?\\b(?! cards?\\b)(?! you control)",
                "\\bdeals? (?:\\d+|x|that much|twice x) damage to (?:any target|(?:another |up to " + NUM + " )?target (?:attacking |blocking |tapped )?(?:creature|planeswalker)|up to " + NUM + " targets?|any number of targets|another target)",
                "\\bdeals? damage equal to [^.]*? to (?:any target|another target|target (?:attacking |blocking )?(?:creature|planeswalker))",
                "target creature[^.]*? gets -(?:\\d+|x)/-(?:\\d+|x)",
                "\\breturn (?:up to " + NUM + " )?(?:another )?target (?!cards?\\b)[^.]*?to (?:its|their) owner'?s'? hands?",
                "\\bfights? (?:target|another target|up to one target|a creature)",
                "(?:each opponent|target opponent|target player|each player) sacrifices (?:a|an|one|two|x) (?:nontoken )?(?:creature|permanent|nonland permanent|attacking creature|planeswalker)",
                "\\bgain control of target", "\\bowner of target [^.]*?(?:shuffles|puts) it", "enchanted (?:creature|permanent) can't (?:attack|block|be)");
        role("wipe", "Board Wipes", null, null,
                "\\b(?:destroy|exile) all [a-z\\-, ]*?(?:creatures|permanents|artifacts|enchantments|planeswalkers|lands)\\b",
                "\\b(?:destroy|exile) each (?:other )?(?:creature|nonland permanent|artifact|enchantment|planeswalker)",
                "damage to each (?:other )?(?:creature|non[a-z\\-]* creature|player and each creature|attacking creature|creature and each)",
                "\\ball (?:other )?creatures(?: your opponents control| you don't control)? get -", "\\beach creature[^.]*? gets -",
                "\\breturn all (?:other )?(?:nonland permanents|creatures|permanents|artifacts|enchantments)[^.]*? to their owners'? hands");
        role("counter", "Counterspells", null, null, "\\bcounter (?:up to " + NUM + " )?target (?:[a-z\\-]+ )*?spells?\\b");
        role("tutor", "Tutors", null, i -> { final Matcher m = SEARCH.matcher(i.text); while (m.find()) if (!LANDISH.matcher(m.group(1)).find()) return true; return false; }, "\\btransmute\\b");
        role("protection", "Protection", null, null,
                "\\b(?:gains?|gets?|has|have) [^.]*?\\bhexproof\\b", "\\b(?:gains?|gets?|has|have) [^.]*?\\bindestructible\\b",
                "\\b(?:gains?|gets?|has|have) [^.]*?\\bshroud\\b", "\\b(?:gains?|gets?|has|have) [^.]*?\\bprotection from\\b", "\\bphases? out\\b", "\\bphasing\\b");

        // ---- themes ----------------------------------------------------------------------------------------
        theme("counters", "+1/+1 Counters", null, null,
                "\\+1/\\+1 counters?", "\\bproliferate", "twice that many [^.]*counters", "counters? of each kind", "additional \\+1/\\+1",
                words("evolve", "modular", "graft", "outlast", "adapt", "bolster", "mentor", "training", "backup", "reinforce", "monstrosity"));
        theme("proliferate", "Proliferate", null, null, "\\bproliferate");
        theme("infect", "Infect & Poison", null, null, words("infect", "toxic", "poisonous", "corrupted", "poison counter"));
        theme("tokens", "Tokens", null,
                i -> { final Matcher m = CREATE.matcher(i.text); while (m.find()) if (!UTILITY_TOKEN.matcher(m.group(1)).find() || m.group(1).contains("/")) return true; return false; },
                "\\bpopulate\\b", "\\bamass\\b", "\\bfabricate\\b", "twice that many [^.]*tokens", "\\btokens? you control\\b");
        theme("sacrifice", "Sacrifice", null, null,
                "\\bsacrifice (?:another|a|an|one|two|x|any number of|\\w+ other) (?:[a-z\\-]+ )?(?:creature|permanent|artifact|token|nontoken)s?\\b",
                "\\bwhen(?:ever)? [^.]*?\\bdies\\b", words("exploit", "emerge", "casualty", "devour", "bargain"));
        theme("lifegain", "Lifegain", null, null,
                "\\byou (?:may )?gain (?:\\d+|x|that much|an amount of|twice that much)? ?life\\b", "(?<!you )(?<!may )(?<!s )\\bgain (?:\\d+|x) life\\b", "whenever you gain life",
                "\\b(?:have|has|gains?|gain) [^.]*?\\blifelink\\b", "\\bextort\\b");
        theme("graveyard", "Graveyard", null, null,
                "\\bfrom your graveyard\\b", "\\bcards? in your graveyard\\b",
                words("flashback", "escape", "unearth", "delve", "embalm", "eternalize", "disturb", "jump-start", "retrace", "aftermath", "dredge", "encore", "scavenge", "threshold", "undergrowth", "descend", "delirium"),
                "\\breturn target [^.]*card from (?:your|a) graveyard", "from (?:a|any) graveyard onto the battlefield",
                "(?<!player )(?<!opponent )(?<!opponents )\\bmills? (?:a|one|two|three|four|five|six|x|\\d+) cards?", "\\bsurveil\\b",
                "put the top [^.]* of your library into your graveyard");
        theme("spells", "Spellslinger", null, null,
                "\\binstant (?:and|or) sorcery\\b", "\\binstants? and sorceries\\b", "\\bnoncreature spells?\\b", words("magecraft", "prowess", "storm"),
                "whenever you cast (?:a|an|your first|another) (?:instant|sorcery|noncreature)", "\\bcopy (?:target|that) (?:instant|sorcery|spell)");
        theme("artifacts", "Artifacts",
                null, i -> i.type != null && i.type.isArtifact() && !i.type.isEquipment() && !i.land && (i.mask & bit("ramp")) == 0,
                "\\bartifacts? you control\\b", "\\bfor each artifact\\b", "\\bwhenever (?:an|a|another|one or more) (?:nontoken )?artifacts?\\b",
                "\\bartifact spells?\\b", words("metalcraft", "improvise", "affinity for artifacts", "modular", "fabricate"), "\\bnumber of artifacts\\b");
        theme("enchantments", "Enchantments",
                null, i -> i.type != null && i.type.isEnchantment() && !i.type.isAura(),
                "\\benchantments? you control\\b", "\\bconstellation\\b", "\\bwhenever (?:an|a|another) (?:nontoken )?enchantment\\b",
                "\\benchantment spells?\\b", "\\bfor each enchantment\\b", "\\bnumber of enchantments\\b");
        theme("voltron", "Voltron",
                null, i -> i.type != null && i.type.isEquipment(),
                "\\bequipped creature\\b", "\\bequip\\b", "\\breconfigure\\b", "\\baura spells?\\b", "\\bauras? you control\\b", "\\bequipment you control\\b",
                "whenever (?:an|a) (?:aura|equipment)", "enchanted creature (?:gets \\+|has (?!defender)|gains)", "\\bbestow\\b");
        theme("landfall", "Landfall", null, null,
                "\\blandfall\\b", "whenever (?:a|another|one or more) (?:[a-z]+ )?lands? (?:you control )?enters?", "you may play lands from");
        theme("blink", "Blink / ETB", null, null,
                "\\bflicker", "\\bexile [^.]*(?:you control|you own)[^.]*?(?:\\. |, )?(?:then )?return (?:it|that card|them|those cards|the exiled cards?)[^.]*to the battlefield",
                "return (?:it|that card|them|those cards|the exiled cards?) to the battlefield under (?:its|their) owners?'? control",
                "triggers an additional time", "entering (?:the battlefield )?causes a triggered ability");
        theme("mill", "Mill", null, null, "(?:target player|target opponent|each opponent|each player|that player|defending player|an opponent) mills");
        theme("discard", "Discard", null, null,
                "(?:player|opponent)s? discards?\\b", "\\bmadness\\b", "\\bdiscards? that card\\b");
        theme("planeswalkers", "Superfriends",
                null, i -> i.type != null && i.type.isPlaneswalker(),
                "\\bplaneswalkers? you control\\b", "\\bloyalty (?:abilities|ability|counters?)\\b", "\\bplaneswalker spells?\\b");
        theme("flying", "Flyers", null, null, "(?<!without )(?<!with )\\bflying\\b");
    }

    private static final List<String> KEYS;
    private static final Map<String, String> LABELS = new ConcurrentHashMap<>();
    static {
        final List<String> k = new ArrayList<>();
        for (final Rule r : RULES) { k.add(r.key); LABELS.put(r.key, r.label); }
        KEYS = Collections.unmodifiableList(k);
    }

    /** Every tag key in table order (roles first). */
    public static List<String> keys() { return KEYS; }
    public static boolean isRole(final String key) { for (final Rule r : RULES) if (r.key.equals(key)) return r.role; return false; }
    public static boolean isKnown(final String key) { return LABELS.containsKey(key); }
    public static int bit(final String key) { final int i = KEYS.indexOf(key); return i < 0 ? 0 : 1 << i; }

    /** Human label for a tag key or a {@code tribal:<Type>} theme. */
    public static String label(final String key) {
        if (key != null && key.startsWith("tribal:")) return key.substring(7) + " Synergy";
        return LABELS.getOrDefault(key, key);
    }

    private static final Map<String, Integer> CACHE = new ConcurrentHashMap<>();
    private static final Pattern REMINDER = Pattern.compile("\\([^)]*\\)");

    public static List<String> tags(final PaperCard pc) { return keysOf(mask(pc)); }

    public static List<String> keysOf(final int mask) {
        final List<String> out = new ArrayList<>(4);
        for (int i = 0; i < KEYS.size(); i++) if ((mask & (1 << i)) != 0) out.add(KEYS.get(i));
        return out;
    }

    /** Bitset over {@link #keys()}; cached by card name since every printing shares the rules. */
    public static int mask(final PaperCard pc) {
        return CACHE.computeIfAbsent(pc.getName(), n -> compute(pc.getRules()));
    }

    /** Lowercased oracle text of all faces, reminder text stripped, own name(s) replaced with "~". */
    public static String text(final CardRules r) {
        final StringBuilder sb = new StringBuilder();
        final List<String> names = new ArrayList<>();
        for (final ICardFace f : r.getAllFaces()) {
            if (f == null) continue;
            if (f.getOracleText() != null) sb.append(f.getOracleText()).append('\n');
            if (f.getName() != null) names.add(f.getName());
        }
        String t = sb.toString().replace("\\n", "\n");
        t = REMINDER.matcher(t).replaceAll("");
        names.sort((a, b) -> b.length() - a.length());
        for (final String n : names) {
            t = t.replace(n, "~");
            final int comma = n.indexOf(',');   // legends are referred to by their short name ("Atraxa")
            if (comma > 2) t = t.replace(n.substring(0, comma), "~");
        }
        return t.toLowerCase(Locale.ROOT);
    }

    private static int compute(final CardRules r) {
        final CardType type = r.getType();
        final String text = text(r);
        final boolean land = type != null && type.isLand();
        int mask = 0;
        for (int i = 0; i < RULES.size(); i++) {
            final Rule rule = RULES.get(i);
            final Info info = new Info(text, type, land, mask);
            if (rule.gate != null && !rule.gate.test(info)) continue;
            if (rule.matches(text) || (rule.alt != null && rule.alt.test(info))) mask |= 1 << i;
        }
        if ((mask & bit("proliferate")) != 0) mask |= bit("counters");   // proliferate decks are counters decks
        return mask;
    }
}
