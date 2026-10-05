package arena.engine.server;

import java.io.File;
import java.io.IOException;
import java.net.URI;
import java.net.URLEncoder;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.time.Duration;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.locks.ReentrantLock;

/**
 * Card images: fetched from Scryfall once, cached on disk forever, served locally. Follows Scryfall's
 * API guidelines (identifying User-Agent, ≥100ms between requests, cache aggressively) and keeps the
 * browser from hitting their rate limit — and keeps games rendering offline after the first load.
 */
public final class ImageCache {
    private static final String UA = "CommanderArena/0.1 (personal Forge client; +https://github.com/)";
    private final Path dir;
    private final HttpClient http = HttpClient.newBuilder().followRedirects(HttpClient.Redirect.ALWAYS).connectTimeout(Duration.ofSeconds(10)).build();
    private final ReentrantLock fetchLock = new ReentrantLock(true); // serialize upstream requests
    private long lastFetchAt = 0;
    private final Map<String, Long> negative = new ConcurrentHashMap<>(); // key → time of 404

    public ImageCache(final Path dir) throws IOException {
        this.dir = dir;
        Files.createDirectories(dir);
    }

    public static Path defaultDir() {
        final String xdg = System.getenv("XDG_CACHE_HOME");
        final Path base = xdg != null && !xdg.isBlank() ? Path.of(xdg) : Path.of(System.getProperty("user.home"), ".cache");
        return base.resolve("commander-arena").resolve("img");
    }

    /** Returns the cached file, fetching it if needed; null if Scryfall has no such image. */
    public static String version(final String v) {
        if (v == null) return "normal";
        return switch (v.toLowerCase()) {
            case "normal", "large", "art_crop", "png", "small", "border_crop" -> v.toLowerCase();
            default -> throw new IllegalArgumentException("unknown image version: " + v + " (normal|large|art_crop|png)");
        };
    }

    public File get(final String name, final String set, final boolean back) throws IOException, InterruptedException { return get(name, set, back, "normal"); }

    public File get(final String name, final String set, final boolean back, final String version) throws IOException, InterruptedException {
        final String key = key(name, set, back, version);
        final File f = dir.resolve(key + ("png".equals(version) ? ".png" : ".jpg")).toFile();
        if (f.isFile() && f.length() > 0) return f;
        final Long neg = negative.get(key);
        if (neg != null && System.currentTimeMillis() - neg < 6 * 3600_000L) return null;

        fetchLock.lock();
        try {
            if (f.isFile() && f.length() > 0) return f; // another thread fetched it meanwhile
            byte[] bytes = fetch(name, set, back, version);
            if (bytes == null && set != null && !set.isBlank()) bytes = fetch(name, null, back, version); // other printing
            // Forge names tokens "Goblin Token"; Scryfall's token printings are just "Goblin".
            if (bytes == null && name.endsWith(" Token")) bytes = fetch(name.substring(0, name.length() - " Token".length()), null, back, version);
            if (bytes == null) { negative.put(key, System.currentTimeMillis()); return null; }
            final Path tmp = dir.resolve(key + ".tmp");
            Files.write(tmp, bytes);
            Files.move(tmp, f.toPath(), java.nio.file.StandardCopyOption.REPLACE_EXISTING, java.nio.file.StandardCopyOption.ATOMIC_MOVE);
            return f;
        } finally {
            fetchLock.unlock();
        }
    }

    private byte[] fetch(final String name, final String set, final boolean back, final String version) throws IOException, InterruptedException {
        final StringBuilder url = new StringBuilder("https://api.scryfall.com/cards/named?format=image&version=" + version + "&exact=")
                .append(URLEncoder.encode(name, StandardCharsets.UTF_8));
        if (set != null && !set.isBlank()) url.append("&set=").append(URLEncoder.encode(set.toLowerCase(), StandardCharsets.UTF_8));
        if (back) url.append("&face=back");
        for (int attempt = 0; attempt < 3; attempt++) {
            final long wait = 120 - (System.currentTimeMillis() - lastFetchAt);
            if (wait > 0) Thread.sleep(wait);
            lastFetchAt = System.currentTimeMillis();
            final HttpRequest req = HttpRequest.newBuilder(URI.create(url.toString()))
                    .header("User-Agent", UA).header("Accept", "image/*,*/*;q=0.5").timeout(Duration.ofSeconds(20)).GET().build();
            final HttpResponse<byte[]> resp = http.send(req, HttpResponse.BodyHandlers.ofByteArray());
            final int code = resp.statusCode();
            if (code == 200) {
                final String ct = resp.headers().firstValue("content-type").orElse("");
                if (ct.startsWith("image/")) return resp.body();
                return null;
            }
            if (code == 404 || code == 400) return null;
            if (code == 429 || code >= 500) {
                final long retry = resp.headers().firstValue("retry-after").map(v -> { try { return Long.parseLong(v.trim()) * 1000; } catch (final NumberFormatException e) { return 1000L; } }).orElse(1000L);
                Thread.sleep(Math.min(retry, 5000) + 250L * attempt);
                continue;
            }
            return null;
        }
        return null;
    }

    // ---- rulings (same client, same politeness) -----------------------------------------------

    /** Scryfall rulings for a card as JSON text: {"rulings":[{"date","comment"}], "scryfallUri"} — cached on disk. */
    public String rulings(final String name) throws IOException, InterruptedException {
        final String key = key("rulings:" + name, null, false, "normal");
        final Path f = dir.resolveSibling("rulings").resolve(key + ".json");
        if (Files.isRegularFile(f)) return Files.readString(f);
        final Long neg = negative.get(key);
        if (neg != null && System.currentTimeMillis() - neg < 6 * 3600_000L) return null;
        fetchLock.lock();
        try {
            final String card = getJson("https://api.scryfall.com/cards/named?exact=" + URLEncoder.encode(name, StandardCharsets.UTF_8));
            if (card == null) { negative.put(key, System.currentTimeMillis()); return null; }
            final com.google.gson.JsonObject co = com.google.gson.JsonParser.parseString(card).getAsJsonObject();
            final String rulingsUri = co.has("rulings_uri") ? co.get("rulings_uri").getAsString() : null;
            final com.google.gson.JsonObject out = new com.google.gson.JsonObject();
            out.addProperty("scryfallUri", co.has("scryfall_uri") ? co.get("scryfall_uri").getAsString() : null);
            final com.google.gson.JsonArray arr = new com.google.gson.JsonArray();
            if (rulingsUri != null) {
                final String r = getJson(rulingsUri);
                if (r != null) {
                    final com.google.gson.JsonObject ro = com.google.gson.JsonParser.parseString(r).getAsJsonObject();
                    if (ro.has("data")) for (final com.google.gson.JsonElement e : ro.getAsJsonArray("data")) {
                        final com.google.gson.JsonObject x = e.getAsJsonObject();
                        final com.google.gson.JsonObject y = new com.google.gson.JsonObject();
                        y.addProperty("date", x.has("published_at") ? x.get("published_at").getAsString() : "");
                        y.addProperty("comment", x.has("comment") ? x.get("comment").getAsString() : "");
                        arr.add(y);
                    }
                }
            }
            out.add("rulings", arr);
            final String text = out.toString();
            Files.createDirectories(f.getParent());
            Files.writeString(f, text);
            return text;
        } finally {
            fetchLock.unlock();
        }
    }

    private String getJson(final String url) throws IOException, InterruptedException {
        for (int attempt = 0; attempt < 3; attempt++) {
            final long wait = 120 - (System.currentTimeMillis() - lastFetchAt);
            if (wait > 0) Thread.sleep(wait);
            lastFetchAt = System.currentTimeMillis();
            final HttpRequest req = HttpRequest.newBuilder(URI.create(url)).header("User-Agent", UA).header("Accept", "application/json").timeout(Duration.ofSeconds(20)).GET().build();
            final HttpResponse<String> resp = http.send(req, HttpResponse.BodyHandlers.ofString());
            if (resp.statusCode() == 200) return resp.body();
            if (resp.statusCode() == 404) return null;
            if (resp.statusCode() == 429 || resp.statusCode() >= 500) { Thread.sleep(1000L + 500L * attempt); continue; }
            return null;
        }
        return null;
    }

    private static String key(final String name, final String set, final boolean back, final String version) {
        try {
            final MessageDigest md = MessageDigest.getInstance("SHA-1");
            final byte[] d = md.digest((name + "|" + (set == null ? "" : set.toLowerCase()) + "|" + (back ? "b" : "f") + ("normal".equals(version) ? "" : "|" + version)).getBytes(StandardCharsets.UTF_8));
            final StringBuilder sb = new StringBuilder();
            for (final byte b : d) sb.append(String.format("%02x", b));
            return sb.toString();
        } catch (final Exception e) { throw new RuntimeException(e); }
    }
}
