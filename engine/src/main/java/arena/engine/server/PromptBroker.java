package arena.engine.server;

import com.google.gson.JsonElement;
import com.google.gson.JsonObject;

import java.util.concurrent.CompletableFuture;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.ExecutionException;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.function.Consumer;

/**
 * Blocking prompt plumbing. The game thread calls {@link #ask}, which sends a {@code prompt} frame
 * and parks until a matching {@code answer} frame arrives on a socket thread. At most one prompt
 * is outstanding per player at any time (the engine is single-threaded per game), so a reconnecting
 * client can simply be re-sent {@link #pending()}.
 */
public final class PromptBroker {
    private final AtomicInteger seq = new AtomicInteger();
    private final ConcurrentHashMap<Integer, CompletableFuture<JsonElement>> waiting = new ConcurrentHashMap<>();
    private volatile JsonObject pending; // last sent, unanswered prompt (for reconnects)
    private final Consumer<JsonObject> sender;

    public PromptBroker(final Consumer<JsonObject> sender) { this.sender = sender; }

    /** Build a prompt frame of the given kind; caller fills in fields then calls {@link #ask}. */
    public JsonObject newPrompt(final String kind) {
        final JsonObject p = Json.frame("prompt");
        p.addProperty("id", seq.incrementAndGet());
        p.addProperty("kind", kind);
        return p;
    }

    /** Send the prompt and block the calling (game) thread until answered. Returns the raw answer value. */
    public JsonElement ask(final JsonObject prompt) {
        final int id = prompt.get("id").getAsInt();
        final CompletableFuture<JsonElement> f = new CompletableFuture<>();
        waiting.put(id, f);
        pending = prompt;
        sender.accept(prompt);
        try {
            return f.get();
        } catch (final InterruptedException e) {
            Thread.currentThread().interrupt();
            throw new RuntimeException("interrupted while waiting for prompt " + id, e);
        } catch (final ExecutionException e) {
            throw new RuntimeException(e.getCause());
        } finally {
            waiting.remove(id);
            if (pending == prompt) pending = null;
        }
    }

    /** Deliver an answer from the client. Unknown/stale ids are ignored (returns false). */
    public boolean answer(final int id, final JsonElement value) {
        final CompletableFuture<JsonElement> f = waiting.get(id);
        if (f == null) return false;
        f.complete(value == null ? com.google.gson.JsonNull.INSTANCE : value);
        return true;
    }

    public JsonObject pending() { return pending; }

    /** Fail every outstanding prompt (match torn down). */
    public void cancelAll() {
        for (final CompletableFuture<JsonElement> f : waiting.values()) f.completeExceptionally(new IllegalStateException("match closed"));
        waiting.clear();
        pending = null;
    }
}
