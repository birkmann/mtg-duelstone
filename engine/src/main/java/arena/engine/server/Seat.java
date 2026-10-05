package arena.engine.server;

import com.google.gson.JsonObject;
import org.eclipse.jetty.websocket.api.Session;

import java.security.SecureRandom;
import java.util.Set;
import java.util.concurrent.CopyOnWriteArraySet;

/**
 * One chair at the table. A human seat owns its own {@link WsGuiGame} (Forge's per-human GUI seam), so
 * every human gets a state view redacted for <em>their</em> player, their own prompts and their own phase
 * stops; frames from that GUI go only to that seat's sockets. AI seats are just a deck; open seats are
 * lobby placeholders that never reach Forge.
 */
final class Seat {
    enum Kind { HUMAN, AI, OPEN }

    private static final SecureRandom RNG = new SecureRandom();

    final int index;
    volatile Kind kind;
    volatile String name;
    volatile String deckRef;
    volatile String deckName;
    /** Prebuilt deck (limited events); null = load {@link #deckRef}. */
    volatile forge.deck.Deck deck;
    /** Secret that binds a socket to this seat (hex). Regenerated when the seat is reopened (kick). */
    volatile String token = newToken();
    /** When the last socket of a seated human went away (0 = connected / never). */
    volatile long goneSince;
    /** The player left mid-game and stayed away; the reaper concedes and answers for them. */
    volatile boolean abandoned;
    /** The player asked to concede; it happens at their next input (see MatchSession#driveConcede). */
    volatile boolean concedeRequested;
    /** Forge has been told; from now on the seat only answers with defaults. */
    volatile boolean conceded;
    final Set<Session> sockets = new CopyOnWriteArraySet<>();
    final WsGuiGame gui;

    Seat(final int index, final Kind kind, final java.util.function.BiConsumer<Seat, JsonObject> out) {
        this.index = index;
        this.kind = kind;
        this.gui = new WsGuiGame(f -> out.accept(this, f));
    }

    static String newToken() {
        final byte[] b = new byte[16];
        RNG.nextBytes(b);
        final StringBuilder sb = new StringBuilder(32);
        for (final byte x : b) sb.append(String.format("%02x", x));
        return sb.toString();
    }

    boolean connected() { return !sockets.isEmpty(); }
    boolean filled() { return kind != Kind.OPEN; }

    JsonObject describe(final boolean forViewer) {
        final JsonObject o = new JsonObject();
        o.addProperty("seat", index);
        o.addProperty("kind", kind.name().toLowerCase());
        o.addProperty("name", name);
        o.addProperty("deck", deckRef);
        o.addProperty("deckName", deckName);
        o.addProperty("connected", kind == Kind.AI || connected());
        o.addProperty("you", forViewer);
        return o;
    }
}
