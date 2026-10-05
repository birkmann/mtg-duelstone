package arena.engine.spike;

import arena.engine.Bootstrap;
import forge.deck.Deck;
import forge.game.Game;
import forge.game.GameLogEntry;
import forge.game.GameType;
import forge.game.player.Player;
import forge.game.player.RegisteredPlayer;
import forge.gamemodes.match.HostedMatch;
import forge.model.FModel;
import forge.player.GamePlayerUtil;
import forge.player.LobbyPlayerHuman;
import forge.util.storage.IStorage;

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;

/**
 * Phase 0 deliverable: start a headless Commander game — Forge AI vs. a "human" seat driven by
 * {@link AutoPilotGuiGame} (answers every prompt with first option / pass) — and dump prompts,
 * the game log and the JSON game view to stdout.
 *
 * Usage: Phase0Spike [humanPreconName] [aiPreconName] [-v] [-json out.json] [-timeout seconds]
 */
public final class Phase0Spike {
    public static void main(final String[] args) throws Exception {
        String humanDeck = null, aiDeck = null, jsonOut = null;
        boolean verbose = false;
        int timeoutSec = 300;
        for (int i = 0; i < args.length; i++) {
            switch (args[i]) {
                case "-v" -> verbose = true;
                case "-json" -> jsonOut = args[++i];
                case "-timeout" -> timeoutSec = Integer.parseInt(args[++i]);
                default -> { if (humanDeck == null) humanDeck = args[i]; else aiDeck = args[i]; }
            }
        }
        final boolean v = verbose;

        Bootstrap.init(() -> new AutoPilotGuiGame(v));

        final IStorage<Deck> precons = FModel.getDecks().getCommanderPrecons();
        final List<String> names = new ArrayList<>();
        precons.forEach(d -> names.add(d.getName()));
        Collections.sort(names);
        System.out.println("[spike] " + names.size() + " commander precons available");
        final Deck dHuman = pick(precons, names, humanDeck, 0);
        final Deck dAi = pick(precons, names, aiDeck, 1);
        System.out.println("[spike] Human: " + dHuman.getName() + " | commanders=" + dHuman.getCommanders());
        System.out.println("[spike] AI:    " + dAi.getName() + " | commanders=" + dAi.getCommanders());

        final RegisteredPlayer human = RegisteredPlayer.forCommander(dHuman);
        human.setPlayer(new LobbyPlayerHuman("You"));
        final RegisteredPlayer ai = RegisteredPlayer.forCommander(dAi);
        ai.setPlayer(GamePlayerUtil.createAiPlayer("Forge AI", 1));

        final AutoPilotGuiGame gui = new AutoPilotGuiGame(verbose);
        final HostedMatch hm = new HostedMatch();
        final CountDownLatch done = new CountDownLatch(1);
        hm.setEndGameHook(done::countDown);

        final long t0 = System.currentTimeMillis();
        hm.startMatch(GameType.Commander, Set.of(GameType.Commander), List.of(human, ai), human, gui);

        if (!done.await(timeoutSec, TimeUnit.SECONDS)) {
            System.out.println("[spike] TIMEOUT after " + timeoutSec + "s — game did not finish. Last state follows.");
        }
        final Game game = hm.getGame();
        final long ms = System.currentTimeMillis() - t0;

        System.out.println("\n================ GAME LOG ================");
        final List<GameLogEntry> log = new ArrayList<>(game.getGameLog().getLogEntries(null));
        Collections.reverse(log);
        for (final GameLogEntry e : log) System.out.println(e);

        System.out.println("\n================ RESULT ================");
        System.out.printf("gameOver=%s turn=%d elapsed=%dms clicks=%d%n", game.isGameOver(), game.getPhaseHandler().getTurn(), ms, gui.getClickCount());
        if (game.isGameOver() && game.getOutcome() != null) {
            System.out.println("winner: " + (game.getOutcome().isDraw() ? "draw" : game.getOutcome().getWinningLobbyPlayer().getName()));
        }
        for (final Player p : game.getRegisteredPlayers()) {
            System.out.printf("  %s life=%d hand=%d bf=%d gy=%d lib=%d%n", p.getName(), p.getLife(),
                    p.getCardsIn(forge.game.zone.ZoneType.Hand).size(), p.getCardsIn(forge.game.zone.ZoneType.Battlefield).size(),
                    p.getCardsIn(forge.game.zone.ZoneType.Graveyard).size(), p.getCardsIn(forge.game.zone.ZoneType.Library).size());
        }

        System.out.println("\n================ PROMPT HISTOGRAM ================");
        for (final Map.Entry<String, Integer> e : gui.getPromptHistogram().entrySet()) System.out.printf("%6d  %s%n", e.getValue(), e.getKey());

        // JSON from the human seat's perspective (AI hand redacted)
        Player humanPlayer = null;
        for (final Player p : game.getRegisteredPlayers()) if (!p.getController().isAI()) humanPlayer = p;
        final String json = gui.stateJson(humanPlayer == null ? null : humanPlayer.getView(), true);
        if (jsonOut != null) {
            Files.writeString(Path.of(jsonOut), json);
            System.out.println("\n[spike] wrote state JSON (" + json.length() + " chars) to " + jsonOut);
        } else {
            System.out.println("\n================ STATE JSON (human view, truncated) ================");
            System.out.println(json.length() > 6000 ? json.substring(0, 6000) + "\n…(" + json.length() + " chars)" : json);
        }
        System.exit(0);
    }

    private static Deck pick(final IStorage<Deck> store, final List<String> names, final String wanted, final int fallbackIdx) {
        if (wanted != null) {
            final Deck exact = store.get(wanted);
            if (exact != null) return exact;
            for (final String n : names) if (n.toLowerCase().contains(wanted.toLowerCase())) return store.get(n);
            throw new IllegalArgumentException("No commander precon matching '" + wanted + "'");
        }
        return store.get(names.get(fallbackIdx));
    }
}
