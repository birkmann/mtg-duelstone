package arena.engine.server;

import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;

import java.io.IOException;
import java.net.URI;
import java.net.URLEncoder;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.time.Duration;
import java.util.HexFormat;
import java.util.Locale;

/** Missing card art for desktop installs: fetch only what is viewed and retain it across launches.
 * The bundled offline pack stays read-only; all downloads go to the user's writable data directory.
 */
final class DownloadedImages {
    record Response(int status, byte[] body) {}
    @FunctionalInterface interface Transport { Response get(URI uri) throws IOException; }

    private final Path root;
    private final Transport transport;
    private final Object[] locks = new Object[64];
    private long nextApiRequest;
    private volatile long retryAfter;

    DownloadedImages(Path root) {
        this(root, httpTransport());
    }

    DownloadedImages(Path root, Transport transport) {
        this.root = root;
        this.transport = transport;
        for (int i = 0; i < locks.length; i++) locks[i] = new Object();
    }

    Path get(String name, boolean back, String version) throws IOException {
        String key = name.trim().toLowerCase(Locale.ROOT);
        // Match the offline pack's aliases for Forge tokens and rebalanced cards.
        if (key.endsWith(" token")) key = key.substring(0, key.length() - 6);
        if (key.startsWith("a-")) key = key.substring(2);
        synchronized (locks[Math.floorMod(key.hashCode(), locks.length)]) {
            final Path metadata = root.resolve("cards").resolve(hash(key) + ".json");
            JsonObject card = readCard(metadata);
            if (card == null) {
                final URI uri = URI.create("https://api.scryfall.com/cards/named?exact=" + URLEncoder.encode(key, StandardCharsets.UTF_8));
                final Response response = download(uri, true);
                if (response.status() == 404) return null;
                requireSuccess(response, uri);
                try {
                    card = JsonParser.parseString(new String(response.body(), StandardCharsets.UTF_8)).getAsJsonObject();
                    if (!card.has("name")) throw new IllegalArgumentException("missing card name");
                } catch (RuntimeException e) {
                    throw new IOException("Invalid Scryfall card metadata", e);
                }
                write(metadata, response.body());
            }
            final String url = imageUrl(card, key, back, version);
            if (url == null) return null;
            final URI uri = URI.create(url);
            // Metadata is cached locally. Never let its image URL fetch arbitrary local/network files.
            if (!"https".equals(uri.getScheme()) || !"cards.scryfall.io".equals(uri.getHost()) || uri.getPort() != -1) {
                throw new IOException("Unexpected card image host");
            }
            final Path image = root.resolve("images").resolve(hash(url) + ".jpg");
            if (Files.isRegularFile(image) && jpeg(Files.readAllBytes(image))) return image;
            final Response response = download(uri, false);
            if (response.status() == 404) return null;
            requireSuccess(response, uri);
            if (!jpeg(response.body())) throw new IOException("Scryfall returned an invalid JPEG");
            write(image, response.body());
            return image;
        }
    }

    private static JsonObject readCard(Path path) {
        try {
            final JsonObject card = JsonParser.parseString(Files.readString(path)).getAsJsonObject();
            return card.has("name") ? card : null;
        } catch (IOException | RuntimeException e) {
            return null; // A partial/corrupt old cache must not prevent a fresh download.
        }
    }

    static String imageUrl(JsonObject card, String name, boolean back, String version) {
        JsonObject face = card;
        if (!card.has("image_uris") && card.has("card_faces")) {
            final var faces = card.getAsJsonArray("card_faces");
            if (faces.isEmpty()) return null;
            face = faces.get(back && faces.size() > 1 ? 1 : 0).getAsJsonObject();
            if (!back) {
                for (JsonElement element : faces) {
                    final JsonObject candidate = element.getAsJsonObject();
                    if (candidate.get("name").getAsString().equalsIgnoreCase(name)) face = candidate;
                }
            }
        }
        if (!face.has("image_uris")) return null;
        final JsonElement url = face.getAsJsonObject("image_uris").get("art_crop".equals(version) ? "art_crop" : "large");
        return url == null || url.isJsonNull() ? null : url.getAsString();
    }

    private Response download(URI uri, boolean api) throws IOException {
        if (System.nanoTime() < retryAfter) throw new IOException("Card image service temporarily unavailable; try again shortly");
        if (api) paceApi();
        if (System.nanoTime() < retryAfter) throw new IOException("Card image service temporarily unavailable");
        try {
            final Response response = transport.get(uri);
            if (response.status() == 429 || response.status() >= 500) {
                retryAfter = System.nanoTime() + Duration.ofSeconds(60).toNanos();
            }
            return response;
        } catch (IOException e) {
            retryAfter = System.nanoTime() + Duration.ofSeconds(5).toNanos();
            throw e;
        }
    }

    /** Keep named-card lookups below two per second, leaving headroom for other API users.
     * All servlet workers share this budget; CDN image downloads do not consume it. */
    private synchronized void paceApi() throws IOException {
        final long wait = nextApiRequest - System.nanoTime();
        if (wait > 0) {
            try { java.util.concurrent.TimeUnit.NANOSECONDS.sleep(wait); }
            catch (InterruptedException e) { Thread.currentThread().interrupt(); throw new IOException(e); }
        }
        nextApiRequest = System.nanoTime() + Duration.ofMillis(600).toNanos();
    }

    private static Transport httpTransport() {
        final HttpClient client = HttpClient.newBuilder().connectTimeout(Duration.ofSeconds(5)).build();
        return uri -> {
            final HttpRequest request = HttpRequest.newBuilder(uri).timeout(Duration.ofSeconds(15))
                    .header("User-Agent", "MTGDuelstone/0.3.1 (https://github.com/birkmann/mtg-duelstone)")
                    .header("Accept", "application/json,image/jpeg").build();
            try {
                final HttpResponse<byte[]> response = client.send(request, HttpResponse.BodyHandlers.ofByteArray());
                return new Response(response.statusCode(), response.body());
            } catch (InterruptedException e) {
                Thread.currentThread().interrupt();
                throw new IOException(e);
            }
        };
    }

    private static void requireSuccess(Response response, URI uri) throws IOException {
        if (response.status() != 200) throw new IOException("Card image download: HTTP " + response.status() + " from " + uri.getHost());
    }

    private static boolean jpeg(byte[] bytes) {
        return bytes.length > 4 && (bytes[0] & 255) == 255 && (bytes[1] & 255) == 216
                && (bytes[bytes.length - 2] & 255) == 255 && (bytes[bytes.length - 1] & 255) == 217;
    }

    private static String hash(String text) {
        try { return HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(text.getBytes(StandardCharsets.UTF_8))); }
        catch (NoSuchAlgorithmException e) { throw new AssertionError(e); }
    }

    private static void write(Path path, byte[] bytes) throws IOException {
        Files.createDirectories(path.getParent());
        final Path temp = Files.createTempFile(path.getParent(), "download-", ".tmp");
        try {
            Files.write(temp, bytes);
            Files.move(temp, path, StandardCopyOption.REPLACE_EXISTING);
        } finally {
            Files.deleteIfExists(temp);
        }
    }
}
