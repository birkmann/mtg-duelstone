package arena.engine.server;

import com.google.gson.JsonArray;

import java.security.SecureRandom;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.Executors;
import java.util.concurrent.ScheduledExecutorService;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicInteger;

/**
 * Registry of live matches and lobbies. One JVM hosts many (each on its own Forge game thread).
 * A lobby's id doubles as its invite code, so it is short, unambiguous to read aloud, and unguessable
 * enough for a LAN party (joining also needs a free seat; playing needs the seat token).
 */
public final class MatchManager {
    private final Map<String, MatchSession> matches = new ConcurrentHashMap<>();
    private final AtomicInteger counter = new AtomicInteger();
    private final SecureRandom rng = new SecureRandom();
    private final ScheduledExecutorService reaper = Executors.newSingleThreadScheduledExecutor(r -> {
        final Thread t = new Thread(r, "match-reaper");
        t.setDaemon(true);
        return t;
    });

    private static final long KEEP_FINISHED_MS = 10 * 60_000L;
    private static final long KEEP_IDLE_LOBBY_MS = 30 * 60_000L;
    private static final String CODE_ALPHABET = "ABCDEFGHJKLMNPQRSTUVWXYZ23456789"; // no 0/O, 1/I

    public MatchManager() {
        reaper.scheduleAtFixedRate(() -> {
            try {
                final long now = System.currentTimeMillis();
                for (final MatchSession m : matches.values()) m.reapAbandoned(now);
            } catch (final Throwable t) { System.err.println("[matches] reaper: " + t); }
        }, 2, 2, TimeUnit.SECONDS); // also the pace an abandoned seat answers at
    }

    /** Drop finished matches and stale lobbies nobody is looking at anymore; a match holds a whole Forge Game. */
    private void prune() {
        final long now = System.currentTimeMillis();
        for (final MatchSession m : matches.values()) {
            final boolean done = m.isOver() && m.socketCount() == 0 && now - m.finishedAt() > KEEP_FINISHED_MS;
            final boolean staleLobby = !m.isStarted() && m.socketCount() == 0 && now - m.createdAt > KEEP_IDLE_LOBBY_MS;
            if (done || staleLobby) { matches.remove(m.id); m.close(); }
        }
    }

    public MatchSession create(final String humanDeckRef, final String aiDeckRef, final String format, final Long seed, final String humanName) {
        prune();
        final String id = "m" + counter.incrementAndGet() + "-" + Long.toString(System.currentTimeMillis() % 100000, 36);
        final MatchSession m = MatchSession.classic(id, humanDeckRef, aiDeckRef, format, seed, humanName);
        matches.put(id, m);
        return m;
    }

    public MatchSession createLobby(final String format, final int players, final String hostName, final String hostDeck, final String inviteBase) {
        prune();
        String id;
        do {
            final StringBuilder sb = new StringBuilder(6);
            for (int i = 0; i < 6; i++) sb.append(CODE_ALPHABET.charAt(rng.nextInt(CODE_ALPHABET.length())));
            id = sb.toString();
        } while (matches.containsKey(id));
        final MatchSession m = MatchSession.lobby(id, format, players, hostName, hostDeck, inviteBase);
        matches.put(id, m);
        return m;
    }

    /** Lookup by id; lobby codes are case-insensitive because people type them. */
    public MatchSession get(final String id) {
        if (id == null) return null;
        final MatchSession m = matches.get(id);
        return m != null ? m : matches.get(id.trim().toUpperCase());
    }

    public JsonArray list() {
        final JsonArray a = new JsonArray();
        for (final MatchSession m : matches.values()) a.add(m.describe());
        return a;
    }

    public void remove(final String id) {
        final MatchSession m = matches.remove(id);
        if (m != null) m.close();
    }
}
