package arena.engine;

import arena.engine.server.EngineServer;
import arena.engine.server.WsGuiGame;

import java.io.File;

/**
 * Entry point: {@code java -jar forge-ws-server.jar [--port 8080] [--static web/dist] [--assets /path/to/forge/forge-gui]
 * [--offline /path/to/offline] [--public-url http://my-host:8080]}. Without {@code --static}, a built browser client
 * ({@code web/dist}, see {@code tools/build-web.sh}) next to the working directory or its parents is served
 * automatically, so the invite links the engine hands out work without extra flags.
 */
public final class Main {
    private static File findWebDist() {
        File cur = new File(".").getAbsoluteFile();
        for (int i = 0; i < 4 && cur != null; i++, cur = cur.getParentFile()) {
            final File cand = new File(cur, "web/dist");
            if (new File(cand, "index.html").isFile()) return cand;
        }
        return null;
    }

    public static void main(final String[] args) throws Exception {
        int port = 8080;
        File staticDir = null;
        String offline = null;
        String publicUrl = System.getenv("MTG_DUELSTONE_PUBLIC_URL");
        if (publicUrl == null) publicUrl = System.getenv("ARENA_PUBLIC_URL");
        for (int i = 0; i < args.length; i++) {
            switch (args[i]) {
                case "--port" -> port = Integer.parseInt(args[++i]);
                case "--static" -> staticDir = new File(args[++i]);
                case "--public-url" -> publicUrl = args[++i];
                case "--decks" -> arena.engine.server.DeckSource.setDecksRoot(new File(args[++i]));
                case "--assets" -> System.setProperty("forge.assets", args[++i]);
                case "--offline" -> offline = args[++i];
                case "-h", "--help" -> { System.out.println("usage: forge-ws-server [--port N] [--static DIR] [--assets FORGE_GUI_DIR] [--decks DIR] [--offline PACK_DIR] [--public-url http://HOST:PORT]"); return; }
                default -> System.err.println("ignoring unknown arg " + args[i]);
            }
        }
        Bootstrap.init(() -> new WsGuiGame(f -> {}));
        if (staticDir == null) staticDir = findWebDist();
        final EngineServer server = new EngineServer(port, staticDir, arena.engine.server.ImageCache.resolvePack(offline), publicUrl);
        server.start();
        System.out.println("[server] listening on http://localhost:" + port + "  (REST: /api/health, /api/decks, POST /api/match; WS: /ws/match/{id})");
        server.join();
    }
}
