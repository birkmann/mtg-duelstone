package arena.engine;

import arena.engine.server.DeckSource;
import forge.deck.CardArchetypeLDAGenerator;
import forge.deck.CardRelationMatrixGenerator;
import forge.deck.Deck;
import forge.deck.DeckFormat;
import forge.deck.DeckgenUtil;
import forge.deck.io.DeckSerializer;
import forge.item.PaperCard;
import forge.localinstance.properties.ForgeConstants;
import forge.model.FModel;

import java.io.File;
import java.net.http.HttpClient;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.HashSet;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;

/**
 * Dev tool (goes online; the engine never does): fills {@code decks/community/<format>/} for the command-zone formats,
 * {@code java -cp forge-ws-server.jar arena.engine.CommunityDecks brawl|oathbreaker|commander [decks/community [target [max]]]}.
 *
 * First the lists content creators published (Forge's {@code res/lists/net-decks-<format>.txt}, zips on GitHub) that
 * Forge's rules for the format still accept; then, while the format has fewer than {@code target} decks, one deck per
 * commander generated from Forge's play data ({@code deckgendecks/}: Oathbreaker's own, Standard's for Brawl), most
 * played commanders first. Why: Forge ships no Brawl or Oathbreaker decks, so those modes offered "Random" alone.
 */
public final class CommunityDecks {
    private CommunityDecks() {}

    public static void main(final String[] args) throws Exception {
        final String format = DeckSource.format(args.length > 0 ? args[0] : "oathbreaker");
        final File out = new File(args.length > 1 ? args[1] : "decks/community", format);
        final int target = args.length > 2 ? Integer.parseInt(args[2]) : 120;
        final int max = args.length > 3 ? Integer.parseInt(args[3]) : 300;
        Bootstrap.init(() -> new arena.engine.server.WsGuiGame(f -> {}));
        out.mkdirs();
        for (final File old : out.listFiles((d, n) -> n.endsWith(".dck"))) old.delete();

        final Set<String> seen = new HashSet<>(), files = new HashSet<>(), leaders = new HashSet<>();
        final HttpClient http = HttpClient.newBuilder().followRedirects(HttpClient.Redirect.NORMAL).build();
        final int[] n = {0, 0};
        for (final String line : Files.readAllLines(new File(ForgeConstants.RES_DIR, "lists/net-decks-" + format + ".txt").toPath(), StandardCharsets.UTF_8)) {
            final int bar = line.indexOf('|');
            if (bar < 0 || n[0] >= max) continue;
            final String source = line.substring(0, bar).trim();
            TournamentDecks.zipDecks(http, line.substring(bar + 1).trim(), (stem, d) -> {
                if (d == null || DeckSource.problem(d, format) != null || !seen.add(TournamentDecks.contentKey(d))) { n[1]++; return true; }
                // "SaffronOlive - Ayula Bears": the author is credited in the description, the title names the deck
                final String raw = d.getName() == null ? stem : d.getName();
                final int dash = raw.indexOf(" - ");
                final String title = dash < 0 ? raw : raw.substring(dash + 3).trim();
                d.setName(title.isEmpty() ? leader(d) : title);
                d.setComment(source + (dash < 0 ? "" : " · " + raw.substring(0, dash).trim()));
                leaders.add(leader(d));
                write(d, out, files);
                return ++n[0] < max;
            });
        }
        final int fromNet = n[0];
        int generated = 0;
        final DeckFormat df = DeckSource.deckFormat(format);
        final List<PaperCard> candidates = candidates(format, df);
        // a second round gives the best-documented commanders another build (another partner / signature spell)
        // when the format has fewer commanders with data than decks wanted
        final Set<String> pairs = new HashSet<>();
        for (int round = 0; round < 3 && n[0] < target; round++) for (final PaperCard leader : candidates) {
            if (n[0] >= target) break;
            if (round == 0 && leaders.contains(leader.getName())) continue;
            final Deck d = generate(leader, df, format);
            if (d == null || !seen.add(TournamentDecks.contentKey(d))) continue;
            final List<String> extra = new ArrayList<>();
            for (final PaperCard c : d.getCommanders()) if (!c.getName().equals(leader.getName())) extra.add(c.getName());
            if (!pairs.add(leader.getName() + "|" + extra)) continue;
            d.setName(round == 0 || extra.isEmpty() ? leader.getName() : leader.getName() + " (" + String.join(", ", extra) + ")");
            d.setComment((extra.isEmpty() ? "" : (DeckSource.OATHBREAKER.equals(format) ? "Signature spell: " : "With ") + String.join(", ", extra) + " · ")
                    + "built from Forge's " + (DeckSource.BRAWL.equals(format) ? "Standard" : df.toString()) + " play data");
            leaders.add(leader.getName());
            write(d, out, files);
            n[0]++;
            generated++;
        }
        System.out.printf("%s: %d decks (%d published lists, %d generated; %d lists refused or duplicate)%n", format, n[0], fromNet, generated, n[1]);
    }

    /** The generator can miss the format's rules (colour identity, size): retry a few times. */
    private static Deck generate(final PaperCard leader, final DeckFormat df, final String format) {
        for (int i = 0; i < 4; i++) {
            try {
                final Deck d = DeckgenUtil.generateRandomCommanderDeck(leader, df, true, true);
                if (d != null && DeckSource.problem(d, format) == null) return d;
            } catch (final Exception ignored) { /* retry */ }
        }
        return null;
    }

    /** Commanders Forge has play data for, the best-documented first (bigger card pool = more decks behind it). */
    private static List<PaperCard> candidates(final String format, final DeckFormat df) {
        final Map<String, Integer> weight = new LinkedHashMap<>();
        if (DeckSource.BRAWL.equals(format)) {
            final var pools = CardArchetypeLDAGenerator.ldaPools.get(FModel.getFormats().getStandard().getName());
            if (pools != null) pools.forEach((name, archetypes) -> weight.put(name, archetypes.size()));
        } else {
            final var pools = CardRelationMatrixGenerator.cardPools.get(df.toString());
            if (pools != null) pools.forEach((name, cards) -> weight.put(name, cards.size()));
        }
        final List<PaperCard> out = new ArrayList<>();
        for (final String name : weight.keySet()) {
            final PaperCard pc = FModel.getMagicDb().getCommonCards().getUniqueByName(name);
            if (pc != null && df.isLegalCommander(pc.getRules()) && df.isLegalCard(pc)) out.add(pc);
        }
        out.sort(Comparator.comparingInt((PaperCard pc) -> -weight.get(pc.getName())).thenComparing(PaperCard::getName));
        return out;
    }

    /** The deck's face: its commander, or the planeswalker of an Oathbreaker pair. */
    private static String leader(final Deck d) {
        String first = null;
        for (final PaperCard c : d.getCommanders()) {
            if (first == null) first = c.getName();
            if (c.getRules().getType().isPlaneswalker() || c.getRules().getType().isCreature()) return c.getName();
        }
        return first == null ? d.getName() : first;
    }

    private static void write(final Deck d, final File out, final Set<String> files) {
        String file = ArchetypeDecks.slug(d.getName());
        for (int i = 2; !files.add(file); i++) file = ArchetypeDecks.slug(d.getName()) + "-" + i;
        DeckSerializer.writeDeck(d, new File(out, file + ".dck"));
    }
}
