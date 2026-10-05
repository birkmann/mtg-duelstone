package arena.engine.server;

import com.google.gson.JsonArray;

import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.atomic.AtomicInteger;

/** Registry of live matches. One JVM hosts many (each on its own Forge game thread), though v1 is 1 user. */
public final class MatchManager {
    private final Map<String, MatchSession> matches = new ConcurrentHashMap<>();
    private final AtomicInteger counter = new AtomicInteger();

    private static final long KEEP_FINISHED_MS = 10 * 60_000L;

    /** Drop finished matches nobody is looking at anymore; they hold a whole Forge Game each. */
    private void prune() {
        final long now = System.currentTimeMillis();
        for (final MatchSession m : matches.values()) {
            if (m.isOver() && m.socketCount() == 0 && now - m.finishedAt() > KEEP_FINISHED_MS) { matches.remove(m.id); m.close(); }
        }
    }

    public MatchSession create(final String humanDeckRef, final String aiDeckRef, final String format, final Long seed, final String humanName) {
        prune();
        final String id = "m" + counter.incrementAndGet() + "-" + Long.toString(System.currentTimeMillis() % 100000, 36);
        final MatchSession m = new MatchSession(id, humanDeckRef, aiDeckRef, format);
        matches.put(id, m);
        m.start(seed, humanName);
        return m;
    }

    public MatchSession get(final String id) { return matches.get(id); }

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
