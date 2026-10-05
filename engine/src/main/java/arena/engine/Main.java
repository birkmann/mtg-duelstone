package arena.engine;

import arena.engine.server.EngineServer;
import arena.engine.server.WsGuiGame;

import java.io.File;

/**
 * Entry point: {@code java -jar forge-ws-server.jar [--port 8080] [--static ../web/dist] [--assets /path/to/forge/forge-gui]}
 */
public final class Main {
    public static void main(final String[] args) throws Exception {
        int port = 8080;
        File staticDir = null;
        for (int i = 0; i < args.length; i++) {
            switch (args[i]) {
                case "--port" -> port = Integer.parseInt(args[++i]);
                case "--static" -> staticDir = new File(args[++i]);
                case "--decks" -> arena.engine.server.DeckSource.setDecksRoot(new File(args[++i]));
                case "--assets" -> System.setProperty("forge.assets", args[++i]);
                case "-h", "--help" -> { System.out.println("usage: forge-ws-server [--port N] [--static DIR] [--assets FORGE_GUI_DIR] [--decks DIR]"); return; }
                default -> System.err.println("ignoring unknown arg " + args[i]);
            }
        }
        Bootstrap.init(() -> new WsGuiGame(f -> {}));
        final EngineServer server = new EngineServer(port, staticDir);
        server.start();
        System.out.println("[server] listening on http://localhost:" + port + "  (REST: /api/health, /api/decks, POST /api/match; WS: /ws/match/{id})");
        server.join();
    }
}
