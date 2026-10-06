package arena.engine;

import arena.engine.server.DeckSource;
import forge.deck.CardArchetypeLDAGenerator;
import forge.deck.Deck;
import forge.deck.DeckFormat;
import forge.deck.io.Archetype;
import forge.deck.io.DeckSerializer;
import forge.item.PaperCard;
import forge.localinstance.properties.ForgeConstants;
import org.apache.commons.lang3.tuple.Pair;

import java.io.File;
import java.io.InputStream;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.util.ArrayList;
import java.util.Collections;
import java.util.HashMap;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.regex.Matcher;
import java.util.regex.Pattern;
import java.util.zip.ZipEntry;
import java.util.zip.ZipInputStream;

/**
 * Dev tool (goes online; the engine never does): picks real tournament lists out of Forge's MTGO deck archive
 * ({@code res/lists/net-decks-archive-<legality>.txt}, zips on GitHub) into {@code decks/tournament/<legality>/}:
 * {@code java -cp forge-ws-server.jar arena.engine.TournamentDecks [pauper [decks/tournament [perArchetype [max]]]]}.
 *
 * Why: Forge ships almost no Pauper-legal decks, and one generated deck per archetype is thin. The archive is
 * newest-last; the tool walks it backwards, keeps decks legal under Forge's current format rules, names each
 * after its nearest metagame archetype (cosine similarity to {@code deckgendecks/<Format>.lda.dat}) and caps
 * how many of one archetype it keeps, so the result is the recent metagame in breadth, not 300 copies of the top deck.
 */
public final class TournamentDecks {
    private TournamentDecks() {}

    private static final Pattern LINE = Pattern.compile("^(\\d{4}-\\d{2}-\\d{2}) \\d+ (.*?) \\(\\d+ decks\\) \\| (https://\\S+\\.zip)$");
    private static final Pattern RESULT = Pattern.compile("^(\\d+) .* (\\d+-\\d+)$|^(\\d+) ");

    public static void main(final String[] args) throws Exception {
        final String legality = args.length > 0 ? args[0] : "pauper";
        final File out = new File(args.length > 1 ? args[1] : "decks/tournament", legality);
        final int perArchetype = args.length > 2 ? Integer.parseInt(args[2]) : 8;
        final int max = args.length > 3 ? Integer.parseInt(args[3]) : 600;
        Bootstrap.init(() -> new arena.engine.server.WsGuiGame(f -> {}));
        final String formatName = DeckSource.gameFormat(legality).getName();
        final List<Archetype> archetypes = CardArchetypeLDAGenerator.ldaArchetypes.getOrDefault(formatName, List.of());
        final Set<String> vocab = ArchetypeDecks.vocabulary();
        final List<String> events = Files.readAllLines(new File(ForgeConstants.RES_DIR, "lists/net-decks-archive-" + legality + ".txt").toPath(), StandardCharsets.UTF_8);
        Collections.reverse(events);

        out.mkdirs();
        for (final File old : out.listFiles((d, n) -> n.endsWith(".dck"))) old.delete();
        final HttpClient http = HttpClient.newBuilder().followRedirects(HttpClient.Redirect.NORMAL).build();
        final Map<String, Integer> perName = new HashMap<>();
        final Map<String, String> canonical = new HashMap<>(); // sorted words → first spelling seen
        final Set<String> seen = new HashSet<>(), files = new HashSet<>();
        int written = 0, scanned = 0, illegal = 0;
        for (final String line : events) {
            if (written >= max) break;
            final Matcher m = LINE.matcher(line.trim());
            if (!m.matches()) continue;
            final String date = m.group(1), event = m.group(2).replaceFirst("^(?i)(?!" + formatName + ")", formatName + " ");
            final HttpResponse<InputStream> resp = http.send(HttpRequest.newBuilder(URI.create(m.group(3))).build(), HttpResponse.BodyHandlers.ofInputStream());
            if (resp.statusCode() != 200) { System.out.println("  " + resp.statusCode() + " " + m.group(3)); continue; }
            try (ZipInputStream zip = new ZipInputStream(resp.body())) {
                for (ZipEntry e; (e = zip.getNextEntry()) != null && written < max; ) {
                    if (!e.getName().endsWith(".dck")) continue;
                    scanned++;
                    final File tmp = File.createTempFile("deck", ".dck");
                    try {
                        Files.write(tmp.toPath(), zip.readAllBytes());
                        final Deck d = DeckSerializer.fromFile(tmp);
                        if (d == null || DeckFormat.Constructed.getDeckConformanceProblem(d) != null || DeckSource.problem(d, DeckSource.CONSTRUCTED, legality) != null) { illegal++; continue; }
                        if (!seen.add(contentKey(d))) continue;
                        final Archetype a = nearest(d, archetypes);
                        // LDA archetypes often differ only in word order ("Metalcraft Synthesizer" / "Synthesizer Metalcraft")
                        final String raw = ArchetypeDecks.name(a == null ? "" : a.getName(), d, vocab);
                        final String[] words = raw.toLowerCase(java.util.Locale.ROOT).split(" ");
                        java.util.Arrays.sort(words);
                        final String name = canonical.computeIfAbsent(String.join(" ", words), k -> raw);
                        if (perName.merge(name, 1, Integer::sum) > perArchetype) continue;
                        final String stem = new File(e.getName()).getName().replaceFirst("(?i)\\.dck$", "");
                        final Matcher r = RESULT.matcher(stem);
                        final String result = !r.find() ? "" : r.group(2) != null ? " · " + r.group(2) : " · #" + Integer.parseInt(r.group(3));
                        d.setName(name);
                        d.setComment(event + " · " + date + result);
                        String file = ArchetypeDecks.slug(name) + "-" + date;
                        for (int i = 2; !files.add(file); i++) file = ArchetypeDecks.slug(name) + "-" + date + "-" + i;
                        DeckSerializer.writeDeck(d, new File(out, file + ".dck"));
                        written++;
                    } catch (final Exception ex) {
                        illegal++;
                    } finally {
                        tmp.delete();
                    }
                }
            }
        }
        System.out.printf("%s: %d decks written (%d scanned, %d illegal now or unreadable), %d archetypes%n", legality, written, scanned, illegal, perName.size());
    }

    private static String contentKey(final Deck d) {
        final List<String> cards = new ArrayList<>();
        for (final Map.Entry<PaperCard, Integer> e : d.getMain()) cards.add(e.getKey().getName() + "x" + e.getValue());
        Collections.sort(cards);
        return String.join("|", cards);
    }

    /** The archetype whose card distribution is closest to the deck's non-basic cards (cosine similarity). */
    private static Archetype nearest(final Deck d, final List<Archetype> archetypes) {
        final Map<String, Double> deck = new HashMap<>();
        for (final Map.Entry<PaperCard, Integer> e : d.getMain()) {
            if (e.getKey().getRules().getType().isBasicLand()) continue;
            deck.merge(e.getKey().getName(), (double) e.getValue(), Double::sum);
        }
        double deckNorm = 0;
        for (final double v : deck.values()) deckNorm += v * v;
        Archetype best = null;
        double bestScore = 0.15; // below this the deck is nothing the data knows: name it by colours + key card
        for (final Archetype a : archetypes) {
            double dot = 0, norm = 0;
            for (final Pair<String, Double> p : a.getCardProbabilities()) {
                norm += p.getRight() * p.getRight();
                final Double q = deck.get(p.getLeft());
                if (q != null) dot += q * p.getRight();
            }
            final double score = norm == 0 ? 0 : dot / Math.sqrt(norm * deckNorm);
            if (score > bestScore) { bestScore = score; best = a; }
        }
        return best;
    }
}
