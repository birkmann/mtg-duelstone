package arena.engine.server;

import com.google.gson.JsonParser;
import org.junit.Rule;
import org.junit.Test;
import org.junit.rules.TemporaryFolder;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;

import static org.junit.Assert.*;

public class ImageCacheTest {
    @Rule public TemporaryFolder temp = new TemporaryFolder();
    private static final byte[] JPEG = {(byte) 255, (byte) 216, 1, 2, (byte) 255, (byte) 217};
    private static final String CARD = """
            {"name":"Plains","image_uris":{"large":"https://cards.scryfall.io/large/plains.jpg",
            "art_crop":"https://cards.scryfall.io/art_crop/plains.jpg"}}
            """;

    private static DownloadedImages.Response json(String text) {
        return new DownloadedImages.Response(200, text.getBytes(StandardCharsets.UTF_8));
    }

    @Test public void freshInstallDownloadsBothSizesAndReusesThemAfterRestartOffline() throws Exception {
        final Path pack = temp.newFolder("bundle").toPath();
        Files.createDirectories(pack.resolve("data")); // release contents: data, no img/index.json
        final Path cache = temp.newFolder("user-data").toPath();
        final List<String> urls = new ArrayList<>();
        final ImageCache images = new ImageCache(pack, new DownloadedImages(cache, uri -> {
            urls.add(uri.toString());
            return uri.getHost().equals("api.scryfall.com") ? json(CARD) : new DownloadedImages.Response(200, JPEG);
        }));
        final Path large = images.get("Plains", "LEA", false, "normal").toPath();
        final Path art = images.get("Plains", null, false, "art_crop").toPath();
        assertArrayEquals(JPEG, Files.readAllBytes(large));
        assertArrayEquals(JPEG, Files.readAllBytes(art));
        assertNotEquals(large, art);
        assertTrue(large.startsWith(cache));
        assertFalse(Files.exists(pack.resolve("img")));
        assertEquals(3, urls.size()); // one metadata request for both images
        final ImageCache restarted = new ImageCache(pack, new DownloadedImages(cache, uri -> {
            throw new AssertionError("Cached images must work without network: " + uri);
        }));
        assertEquals(large.toFile(), restarted.get("PLAINS", null, false, "large"));
        assertEquals(art.toFile(), restarted.get("Plains", null, false, "art_crop"));
    }

    @Test public void offlinePackWinsAndPartialPacksFetchMissingFiles() throws Exception {
        final Path pack = temp.newFolder("pack").toPath();
        Files.createDirectories(pack.resolve("img/large/ab"));
        Files.writeString(pack.resolve("img/index.json"), """
                {"plains":{"id":"abc","name":"Plains","face":0,"back":false}}
                """);
        final Path existing = pack.resolve("img/large/ab/abc.jpg");
        Files.write(existing, JPEG);
        final List<String> urls = new ArrayList<>();
        final ImageCache images = new ImageCache(pack, new DownloadedImages(temp.newFolder("cache").toPath(), uri -> {
            urls.add(uri.toString());
            return uri.getHost().equals("api.scryfall.com") ? json(CARD) : new DownloadedImages.Response(200, JPEG);
        }));
        assertEquals(existing.toFile(), images.get("Plains", null, false, "large"));
        assertTrue(urls.isEmpty());
        assertNotNull(images.get("Plains", null, false, "art_crop"));
        assertEquals(2, urls.size());
        assertNull(new ImageCache(pack).get("Unknown", null, false, "normal"));
    }

    @Test public void doubleFacedAndSplitCardsSelectTheRightImage() {
        final var card = JsonParser.parseString("""
                {"card_faces":[
                  {"name":"Front","image_uris":{"large":"front","art_crop":"front-art"}},
                  {"name":"Back","image_uris":{"large":"back","art_crop":"back-art"}}]}
                """).getAsJsonObject();
        assertEquals("front", DownloadedImages.imageUrl(card, "Front // Back", false, "normal"));
        assertEquals("back", DownloadedImages.imageUrl(card, "Front // Back", true, "large"));
        assertEquals("back-art", DownloadedImages.imageUrl(card, "back", false, "art_crop"));
        card.add("image_uris", JsonParser.parseString("{\"large\":\"split\"}"));
        assertEquals("split", DownloadedImages.imageUrl(card, "Back", false, "large"));
    }

    @Test public void aliasesAreEncodedAndMissingCardsDoNotCreateFiles() throws Exception {
        final Path root = temp.newFolder("aliases").toPath();
        final List<String> queries = new ArrayList<>();
        final DownloadedImages images = new DownloadedImages(root, uri -> {
            queries.add(uri.getRawQuery());
            return new DownloadedImages.Response(404, new byte[0]);
        });
        assertNull(images.get("Goblin Token", false, "large"));
        assertNull(images.get("A-Fire // Ice", false, "large"));
        assertEquals(List.of("exact=goblin", "exact=fire+%2F%2F+ice"), queries);
        try (var files = Files.list(root)) { assertEquals(0, files.count()); }
    }

    @Test public void failedDownloadsCanRetryAndNeverCacheAnErrorPage() throws Exception {
        final Path root = temp.newFolder("retry").toPath();
        final DownloadedImages bad = new DownloadedImages(root, uri ->
                uri.getHost().equals("api.scryfall.com") ? json(CARD) : json("<html>error</html>"));
        assertThrows(IOException.class, () -> bad.get("Plains", false, "large"));
        assertFalse(Files.exists(root.resolve("images")));
        final DownloadedImages good = new DownloadedImages(root, uri -> {
            assertEquals("cards.scryfall.io", uri.getHost()); // successfully cached metadata survives
            return new DownloadedImages.Response(200, JPEG);
        });
        assertNotNull(good.get("Plains", false, "large"));
    }

    @Test public void rateLimitStopsFurtherDownloads() throws Exception {
        final int[] requests = {0};
        final DownloadedImages images = new DownloadedImages(temp.newFolder("limited").toPath(), uri -> {
            requests[0]++;
            return new DownloadedImages.Response(429, new byte[0]);
        });
        assertThrows(IOException.class, () -> images.get("Plains", false, "large"));
        assertThrows(IOException.class, () -> images.get("Island", false, "large"));
        assertEquals(1, requests[0]);
    }
}
