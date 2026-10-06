package arena.engine.server;

import forge.deck.Deck;
import forge.deck.DeckFormat;
import forge.deck.io.DeckSerializer;
import forge.item.PaperCard;
import forge.localinstance.properties.ForgeConstants;
import forge.util.BuildInfo;

import java.io.BufferedReader;
import java.io.File;
import java.io.IOException;
import java.io.PrintWriter;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collections;
import java.util.HashMap;
import java.util.HashSet;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.TreeMap;
import java.util.concurrent.CompletableFuture;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

/**
 * The big pool of ready-made 60-card decks behind the constructed deck list, indexed by legality so the
 * deck screen can offer hundreds of playable decks for every format instead of four starters:
 * <ul>
 *   <li>{@code archetype:<legality>/<stem>} - this repo's {@code decks/archetypes/} (see {@code arena.engine.ArchetypeDecks}),
 *       one deck per Forge metagame archetype; the only real source of Pauper and most of Standard,</li>
 *   <li>{@code forge:<path>} - Forge's quest duel, quest world and genetic-AI decks under {@code res/} (~9k files).</li>
 * </ul>
 * Why an index: checking every deck against six legalities takes ~15 s, far too slow per request. It is built
 * once in the background at startup and cached in Forge's cache dir, keyed by each file's size + mtime and the
 * Forge build (whose format files decide legality), so later starts only stat the files.
 * Refs load only when the index knows them: a client can't name arbitrary files through {@code forge:}.
 */
public final class DeckLibrary {
    private DeckLibrary() {}

    /** One indexed deck. {@code legal} has bit i set when the deck is legal in {@code DeckSource.LEGALITIES[i]}. */
    public record Entry(String ref, File file, String name, String description, String colors, String cover, int mainCount, int legal) {
        public boolean legalIn(final String legality) {
            return legality == null || (legal & (1 << DeckSource.LEGALITIES.indexOf(legality))) != 0;
        }
    }

    private static final String ARCHETYPE = "archetype:", FORGE = "forge:";
    /** Bumped when the cache's columns or the naming rules change. */
    private static final String CACHE_VERSION = "2";
    private static final int REJECTED = -1;

    private static final CompletableFuture<Map<String, Entry>> INDEX = new CompletableFuture<>();
    private static volatile boolean started;

    /** Builds the index on a daemon thread; the first constructed deck listing waits for it. */
    public static synchronized void warm() {
        if (started) return;
        started = true;
        final Thread t = new Thread(() -> {
            try { INDEX.complete(build()); }
            catch (final Throwable e) {
                System.err.println("[decks] library index failed: " + e);
                INDEX.complete(Map.of());
            }
        }, "deck-library");
        t.setDaemon(true);
        t.start();
    }

    /** All decks, in display priority (archetypes, then Forge's), deduplicated by card list. */
    public static Map<String, Entry> entries() {
        warm();
        return INDEX.join();
    }

    public static boolean owns(final String ref) { return ref.startsWith(ARCHETYPE) || ref.startsWith(FORGE); }

    public static Deck load(final String ref) {
        final Entry e = entries().get(ref);
        if (e == null) throw new IllegalArgumentException("deck not found: " + ref);
        final Deck d = DeckSerializer.fromFile(e.file());
        if (d == null) throw new IllegalArgumentException("cannot parse deck file: " + e.file());
        d.setName(e.name());
        return d;
    }

    // ---- building ---------------------------------------------------------------------------

    private record Source(String ref, File file, String group) {}

    private static List<Source> sources() {
        final List<Source> out = new ArrayList<>();
        final File repo = DeckSource.decksRoot();
        if (repo != null) {
            for (final String l : DeckSource.LEGALITIES) {
                for (final File f : dcks(new File(repo, "archetypes/" + l))) out.add(new Source(ARCHETYPE + l + "/" + stem(f), f, "archetype"));
            }
        }
        final File res = new File(ForgeConstants.RES_DIR);
        for (final File f : dcks(new File(res, "quest/duels"))) out.add(new Source(FORGE + "quest/duels/" + stem(f), f, "duel"));
        final File[] worlds = new File(res, "quest/world").listFiles(File::isDirectory);
        if (worlds != null) {
            Arrays.sort(worlds);
            for (final File w : worlds) {
                for (final File f : dcks(new File(w, "duels"))) out.add(new Source(FORGE + "quest/world/" + w.getName() + "/duels/" + stem(f), f, "world:" + w.getName()));
            }
        }
        for (final File f : dcks(new File(res, "geneticaidecks"))) out.add(new Source(FORGE + "geneticaidecks/" + stem(f), f, "genetic"));
        return out;
    }

    private static Map<String, Entry> build() {
        final long t0 = System.currentTimeMillis();
        final File cacheFile = new File(ForgeConstants.CACHE_DIR, "duelstone-deck-library.tsv");
        final String stamp = CACHE_VERSION + "|" + BuildInfo.getVersionString() + "|" + String.join(",", DeckSource.LEGALITIES);
        final Map<String, String[]> cached = readCache(cacheFile, stamp);
        final List<String[]> rows = new ArrayList<>();
        final Map<String, Entry> out = new LinkedHashMap<>();
        final Set<String> seen = new HashSet<>();
        int parsed = 0;
        for (final Source s : sources()) {
            final String size = Long.toString(s.file().length()), mtime = Long.toString(s.file().lastModified());
            String[] row = cached.get(s.ref());
            if (row == null || !row[1].equals(size) || !row[2].equals(mtime)) {
                row = index(s, size, mtime);
                parsed++;
            }
            rows.add(row);
            final int legal = Integer.parseInt(row[8]);
            if (legal == REJECTED || !seen.add(row[9])) continue;
            out.put(s.ref(), new Entry(s.ref(), s.file(), row[3], row[4].isEmpty() ? null : row[4], row[5], row[6].isEmpty() ? null : row[6], Integer.parseInt(row[7]), legal));
        }
        if (parsed > 0) writeCache(cacheFile, stamp, rows);
        final Map<String, Integer> per = new TreeMap<>();
        for (final Entry e : out.values()) for (final String l : DeckSource.LEGALITIES) if (e.legalIn(l)) per.merge(l, 1, Integer::sum);
        System.out.printf("[decks] library: %d decks (%d parsed, rest cached) in %d ms; legal %s%n", out.size(), parsed, System.currentTimeMillis() - t0, per);
        return Collections.unmodifiableMap(out);
    }

    /** ref, size, mtime, name, description, colors, cover, mainCount, legal mask (or REJECTED), content key */
    private static String[] index(final Source s, final String size, final String mtime) {
        try {
            final Deck d = DeckSerializer.fromFile(s.file());
            final int n = d == null ? 0 : d.getMain().countAll();
            if (d == null || !d.getCommanders().isEmpty() || n < 60 || n > 80 || DeckFormat.Constructed.getDeckConformanceProblem(d) != null) return rejected(s, size, mtime);
            int legal = 0;
            for (int i = 0; i < DeckSource.LEGALITIES.size(); i++) {
                if (DeckSource.problem(d, DeckSource.CONSTRUCTED, DeckSource.LEGALITIES.get(i)) == null) legal |= 1 << i;
            }
            final List<String> cards = new ArrayList<>();
            for (final Map.Entry<PaperCard, Integer> e : d.getMain()) cards.add(e.getKey().getName() + "x" + e.getValue());
            Collections.sort(cards);
            final String[] nd = nameAndDescription(s, d, header(s.file()));
            final PaperCard cover = DeckSource.cover(d);
            return new String[] {s.ref(), size, mtime, clean(nd[0]), clean(nd[1]), arena.engine.view.ViewJson.colors(DeckSource.spellColors(d)),
                    cover == null ? "" : cover.getName(), Integer.toString(n), Integer.toString(legal), Integer.toHexString(String.join("|", cards).hashCode()) + n};
        } catch (final Throwable t) {
            return rejected(s, size, mtime);
        }
    }

    private static String[] rejected(final Source s, final String size, final String mtime) {
        return new String[] {s.ref(), size, mtime, "", "", "", "", "0", Integer.toString(REJECTED), s.ref()};
    }

    // World duel descriptions are often the source list's path: "./Standard/Zendikar_Rising/Aaron_Eng_-_Simic_Adventure_(Standard,_#1037141).dck"
    private static final Pattern SOURCE_PATH = Pattern.compile("(?:^|/)(?:[^/]*?_-_)?([^/]+?)(?:_\\([^)]*\\))?\\.dck$");
    private static final Pattern GENETIC = Pattern.compile("^GA[A-Z]_(?:\\d+_)+(.*?)(?: Generated Deck)?_\\d+_\\d+$");

    /** A name that says what the deck plays where the files allow it, and a one-line description. */
    private static String[] nameAndDescription(final Source s, final Deck d, final Map<String, String> h) {
        final String title = h.getOrDefault("title", d.getName());
        final String desc = h.getOrDefault("description", "");
        switch (s.group()) {
            case "archetype": return new String[] {d.getName(), d.getComment() == null ? "" : d.getComment()};
            case "genetic": {
                final Matcher m = GENETIC.matcher(d.getName());
                return new String[] {m.matches() ? m.group(1) : d.getName(), "Evolved by Forge's genetic AI"};
            }
            case "duel": return new String[] {d.getName(), desc};
            default: { // world:<dir>
                final String world = s.group().substring("world:".length()).replaceFirst("^\\d{4}(-\\d{2})? ", "").replace('_', ' ');
                final Matcher m = SOURCE_PATH.matcher(desc.trim());
                if (m.find()) return new String[] {m.group(1).replace('_', ' '), title + " · " + world};
                return new String[] {title, desc.isBlank() ? world : world + " · " + desc};
            }
        }
    }

    /** The {@code [metadata]} keys Forge's Deck doesn't keep (Title, Description), lower-cased. */
    private static Map<String, String> header(final File f) throws IOException {
        final Map<String, String> h = new HashMap<>();
        try (BufferedReader r = Files.newBufferedReader(f.toPath(), StandardCharsets.UTF_8)) {
            String line;
            while ((line = r.readLine()) != null) {
                final String t = line.trim();
                if (t.equalsIgnoreCase("[main]")) break;
                final int eq = t.indexOf('=');
                if (eq > 0) h.putIfAbsent(t.substring(0, eq).trim().toLowerCase(), t.substring(eq + 1).trim());
            }
        } catch (final java.nio.charset.MalformedInputException e) { /* non-UTF-8 header: Deck's own name will do */ }
        return h;
    }

    /** Single line, no tabs (the cache is TSV). */
    private static String clean(final String s) { return s == null ? "" : s.replaceAll("[\\t\\r\\n]+", " ").trim(); }

    private static Map<String, String[]> readCache(final File f, final String stamp) {
        final Map<String, String[]> m = new HashMap<>();
        if (!f.isFile()) return m;
        try {
            final List<String> lines = Files.readAllLines(f.toPath(), StandardCharsets.UTF_8);
            if (lines.isEmpty() || !lines.get(0).equals(stamp)) return m;
            for (final String l : lines.subList(1, lines.size())) {
                final String[] row = l.split("\t", -1);
                if (row.length == 10) m.put(row[0], row);
            }
        } catch (final IOException ignored) { /* rebuild */ }
        return m;
    }

    private static void writeCache(final File f, final String stamp, final List<String[]> rows) {
        try {
            f.getParentFile().mkdirs();
            final File tmp = new File(f.getPath() + ".tmp");
            try (PrintWriter w = new PrintWriter(tmp, StandardCharsets.UTF_8)) {
                w.println(stamp);
                for (final String[] r : rows) w.println(String.join("\t", r));
            }
            Files.move(tmp.toPath(), f.toPath(), java.nio.file.StandardCopyOption.REPLACE_EXISTING);
        } catch (final IOException e) {
            System.err.println("[decks] could not write " + f + ": " + e.getMessage());
        }
    }

    private static List<File> dcks(final File dir) {
        final File[] fs = dir.listFiles((d, n) -> n.toLowerCase().endsWith(".dck"));
        if (fs == null) return List.of();
        Arrays.sort(fs);
        return Arrays.asList(fs);
    }

    private static String stem(final File f) { return f.getName().replaceFirst("(?i)\\.dck$", ""); }
}
