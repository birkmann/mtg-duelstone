package arena.engine.server;

/**
 * Limits for an engine that strangers reach over the internet (the website's "play in your browser").
 *
 * <p>The engine was built for one household: anyone who can reach it may list and join matches, save decks,
 * cubes and collections into {@code ~/.forge}, and a vs-AI game whose tab closed waits for its human forever.
 * On a public host all of that is either someone else's data or an unbounded pile of parked Forge games, so
 * {@code --public} narrows the server to "one visitor plays the AI": classic matches only, each bound to the
 * seat token its creator got back, a cap on live games, abandoned games conceded like a left table seat, and
 * every endpoint that writes to disk or opens a table to others refused.
 */
public final class PublicMode {
    private PublicMode() {}

    private static volatile boolean on;
    private static volatile int maxMatches = 6;

    public static void enable(final int maxLiveMatches) {
        on = true;
        maxMatches = Math.max(1, maxLiveMatches);
    }

    public static boolean on() { return on; }
    public static int maxMatches() { return maxMatches; }

    /** Visitors read this when a refused endpoint is all that stands between them and a feature. */
    static final String DESKTOP_ONLY = "Not available on the public server. Download the game to build decks, draft and play with friends.";

    /** True for endpoints a public engine refuses (see the class comment for why each kind is out). */
    static boolean refuses(final String method, final String path) {
        if (!on) return false;
        if ("GET /matches".equals(method + " " + path)) return true;      // other visitors' match ids
        if (path.startsWith("/lobby") || path.startsWith("/event")) return true; // tables and pods for others
        if (path.startsWith("/collection") && !"GET".equals(method)) return true;
        if ("GET".equals(method)) return false;
        return switch (path) {
            case "/deck", "/cube", "/cube/import" -> true;                 // write into ~/.forge
            default -> false;
        };
    }
}
