package arena.engine.server;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import forge.StaticData;
import forge.item.PaperCard;
import org.eclipse.jetty.server.Server;
import org.eclipse.jetty.server.handler.HandlerList;
import org.eclipse.jetty.servlet.DefaultServlet;
import org.eclipse.jetty.servlet.ServletContextHandler;
import org.eclipse.jetty.servlet.ServletHolder;
import org.eclipse.jetty.websocket.api.Session;
import org.eclipse.jetty.websocket.api.WebSocketAdapter;
import org.eclipse.jetty.websocket.servlet.ServletUpgradeRequest;
import org.eclipse.jetty.websocket.servlet.ServletUpgradeResponse;
import org.eclipse.jetty.websocket.servlet.WebSocketCreator;
import org.eclipse.jetty.websocket.servlet.WebSocketServlet;
import org.eclipse.jetty.websocket.servlet.WebSocketServletFactory;

import javax.servlet.http.HttpServlet;
import javax.servlet.http.HttpServletRequest;
import javax.servlet.http.HttpServletResponse;
import java.io.File;
import java.io.IOException;
import java.nio.charset.StandardCharsets;

/**
 * Embedded Jetty: REST under /api, the game WebSocket at /ws/match/{id}, and (optionally) the built
 * frontend as static files. Designed for one local user; CORS is wide open for the Vite dev server.
 */
public final class EngineServer {
    private final MatchManager matches = new MatchManager();
    private final Server server;
    private final ImageCache images;

    public EngineServer(final int port, final File staticDir) throws IOException {
        images = new ImageCache(ImageCache.defaultDir());
        System.out.println("[server] image cache: " + ImageCache.defaultDir());
        server = new Server(port);
        final ServletContextHandler ctx = new ServletContextHandler(ServletContextHandler.SESSIONS);
        ctx.setContextPath("/");
        ctx.addServlet(new ServletHolder(new ApiServlet()), "/api/*");
        ctx.addServlet(new ServletHolder(new MatchWsServlet()), "/ws/*");
        if (staticDir != null && staticDir.isDirectory()) {
            final ServletHolder st = new ServletHolder("static", DefaultServlet.class);
            st.setInitParameter("resourceBase", staticDir.getAbsolutePath());
            st.setInitParameter("dirAllowed", "false");
            st.setInitParameter("welcomeServlets", "true");
            ctx.addServlet(st, "/");
            System.out.println("[server] serving static files from " + staticDir.getAbsolutePath());
        }
        final HandlerList handlers = new HandlerList();
        handlers.addHandler(ctx);
        server.setHandler(handlers);
    }

    public void start() throws Exception { server.start(); }
    public void join() throws InterruptedException { server.join(); }
    public MatchManager matches() { return matches; }

    // ---- REST -------------------------------------------------------------------------------

    private final class ApiServlet extends HttpServlet {
        private final arena.engine.deck.DeckStore deckStore = new arena.engine.deck.DeckStore();
        private final arena.engine.deck.DeckImporter importer = new arena.engine.deck.DeckImporter();
        private arena.engine.deck.CollectionStore collection;

        private synchronized arena.engine.deck.CollectionStore collection() throws Exception {
            if (collection == null) collection = new arena.engine.deck.CollectionStore(arena.engine.deck.CollectionStore.defaultFile());
            return collection;
        }
        private void cors(final HttpServletResponse resp) {
            resp.setHeader("Access-Control-Allow-Origin", "*");
            resp.setHeader("Access-Control-Allow-Methods", "GET, POST, PUT, DELETE, OPTIONS");
            resp.setHeader("Access-Control-Allow-Headers", "Content-Type");
        }
        private void json(final HttpServletResponse resp, final int status, final JsonElement body) throws IOException {
            cors(resp);
            resp.setStatus(status);
            resp.setContentType("application/json;charset=utf-8");
            resp.getWriter().write(Json.GSON.toJson(body));
        }
        private void text(final HttpServletResponse resp, final String body, final String filename) throws IOException {
            cors(resp);
            resp.setStatus(200);
            resp.setContentType("text/plain;charset=utf-8");
            if (filename != null) resp.setHeader("Content-Disposition", "attachment; filename=\"" + filename.replace("\"", "") + "\"");
            resp.getWriter().write(body);
        }
        private JsonObject err(final String msg) { final JsonObject o = new JsonObject(); o.addProperty("error", msg); return o; }
        private JsonObject body(final HttpServletRequest req) throws IOException {
            final String s = new String(req.getInputStream().readAllBytes(), StandardCharsets.UTF_8);
            return s.isBlank() ? new JsonObject() : Json.parse(s);
        }
        private int intParam(final HttpServletRequest req, final String k, final int dflt) {
            try { return req.getParameter(k) == null ? dflt : Integer.parseInt(req.getParameter(k)); } catch (final NumberFormatException e) { return dflt; }
        }

        @Override protected void doOptions(final HttpServletRequest req, final HttpServletResponse resp) { cors(resp); resp.setStatus(204); }

        @Override protected void service(final HttpServletRequest req, final HttpServletResponse resp) throws IOException {
            if ("OPTIONS".equals(req.getMethod())) { doOptions(req, resp); return; }
            final String path = req.getPathInfo() == null ? "/" : req.getPathInfo();
            final String m = req.getMethod();
            try {
                if (!route(m, path, req, resp)) json(resp, 404, err("unknown endpoint " + m + " " + path));
            } catch (final IllegalArgumentException e) {
                json(resp, 400, err(e.getMessage()));
            } catch (final Exception e) {
                e.printStackTrace();
                json(resp, 500, err(String.valueOf(e)));
            }
        }

        private boolean route(final String m, final String path, final HttpServletRequest req, final HttpServletResponse resp) throws Exception {
            final arena.engine.deck.CardCatalog catalog = arena.engine.deck.CardCatalog.get();
            switch (m + " " + path) {
                // ---- engine / images -------------------------------------------------------------
                case "GET /health" -> { final JsonObject o = new JsonObject(); o.addProperty("ok", true); o.addProperty("protocol", 2); o.addProperty("cards", StaticData.instance().getCommonCards().getAllCards().size()); json(resp, 200, o); }
                case "GET /img" -> {
                    final String name = req.getParameter("name");
                    if (name == null || name.isBlank()) throw new IllegalArgumentException("name required");
                    final String version = ImageCache.version(req.getParameter("version"));
                    final File f = images.get(name, req.getParameter("set"), "1".equals(req.getParameter("back")), version);
                    cors(resp);
                    if (f == null) { resp.setStatus(404); return true; }
                    resp.setStatus(200); resp.setContentType("png".equals(version) ? "image/png" : "image/jpeg");
                    resp.setHeader("Cache-Control", "public, max-age=31536000, immutable");
                    resp.setContentLengthLong(f.length());
                    java.nio.file.Files.copy(f.toPath(), resp.getOutputStream());
                }
                case "GET /rulings" -> {
                    final String r = images.rulings(req.getParameter("name"));
                    cors(resp);
                    if (r == null) { json(resp, 404, err("no rulings")); return true; }
                    resp.setStatus(200); resp.setContentType("application/json;charset=utf-8");
                    resp.setHeader("Cache-Control", "public, max-age=86400");
                    resp.getWriter().write(r);
                }
                case "GET /engine/cards" -> { // ?names=a|b  → can Forge play these?
                    final JsonObject o = new JsonObject();
                    for (final String n : arena.engine.deck.CollectionStore.split(req.getParameter("names"))) o.addProperty(n.trim(), catalog.resolveExact(n.trim()) != null);
                    json(resp, 200, o);
                }
                // ---- matches -------------------------------------------------------------------------
                case "GET /matches" -> json(resp, 200, matches.list());
                case "POST /match" -> {
                    final JsonObject b = body(req);
                    final String human = Json.str(b, "humanDeck"), ai = Json.str(b, "aiDeck");
                    if (human == null || ai == null) throw new IllegalArgumentException("humanDeck and aiDeck refs required");
                    final Integer seedI = b.has("seed") && !b.get("seed").isJsonNull() ? b.get("seed").getAsInt() : null;
                    final String fmt = DeckSource.format(Json.str(b, "format"));
                    json(resp, 201, matches.create(human, ai, fmt, seedI == null ? null : seedI.longValue(), Json.str(b, "humanName")).describe());
                }
                // ---- cards ----------------------------------------------------------------------------
                case "GET /cards/search" -> {
                    final var found = catalog.search(req.getParameter("q") == null ? "" : req.getParameter("q"), req.getParameter("ci"), req.getParameter("type"), req.getParameter("text"), "1".equals(req.getParameter("commander")), Math.min(200, intParam(req, "limit", 60)));
                    json(resp, 200, catalog.cardsJson(found));
                }
                case "GET /cards/get" -> {
                    final PaperCard pc = catalog.resolveExact(req.getParameter("name"));
                    if (pc == null) json(resp, 404, err("unknown card")); else json(resp, 200, catalog.cardJson(pc));
                }
                // ---- decks ----------------------------------------------------------------------------
                case "GET /decks" -> { // lobby list; ?format=constructed|commander (default commander); ?kind=user|all
                    final String fmt = DeckSource.format(req.getParameter("format"));
                    final String kind = req.getParameter("kind") == null ? "all" : req.getParameter("kind");
                    if ("user".equals(kind)) json(resp, 200, deckStore.list(fmt));
                    else json(resp, 200, DeckSource.list(fmt));
                }
                case "GET /deck" -> {
                    final String ref = req.getParameter("ref");
                    if (ref == null) throw new IllegalArgumentException("ref required");
                    final String fmt = req.getParameter("format") == null ? null : DeckSource.format(req.getParameter("format"));
                    final forge.deck.Deck d = deckStore.get(ref, fmt);
                    json(resp, 200, deckStore.full(ref, d, fmt != null ? fmt : d.getCommanders().isEmpty() ? DeckSource.CONSTRUCTED : DeckSource.COMMANDER));
                }
                case "GET /deck/export" -> {
                    final String ref = req.getParameter("ref");
                    final forge.deck.Deck d = deckStore.get(ref, req.getParameter("format") == null ? null : DeckSource.format(req.getParameter("format")));
                    text(resp, deckStore.export(d), d.getName() + ".dck");
                }
                case "POST /deck" -> { final JsonObject b = body(req); json(resp, 200, deckStore.save(b, Json.str(b, "previousName"))); }
                case "POST /deck/validate" -> json(resp, 200, deckStore.validate(body(req)));
                case "DELETE /deck" -> { deckStore.delete(req.getParameter("name"), DeckSource.format(req.getParameter("format"))); json(resp, 200, new JsonObject()); }
                case "POST /deck/import" -> { final JsonObject b = body(req); json(resp, 200, importer.parse(Json.str(b, "text") == null ? "" : Json.str(b, "text"), DeckSource.format(Json.str(b, "format")))); }
                // ---- collection ------------------------------------------------------------------------
                case "GET /collection" -> {
                    final JsonObject o = new JsonObject();
                    o.add("summary", collection().summary());
                    o.add("cards", collection().list(req.getParameter("q"), Math.min(5000, intParam(req, "limit", 500))));
                    json(resp, 200, o);
                }
                case "GET /collection/owned" -> {
                    final JsonObject o = new JsonObject();
                    collection().owned(arena.engine.deck.CollectionStore.split(req.getParameter("names"))).forEach(o::addProperty);
                    json(resp, 200, o);
                }
                case "POST /collection/import" -> { final JsonObject b = body(req); json(resp, 200, collection().importCsv(Json.str(b, "csv") == null ? "" : Json.str(b, "csv"), Json.bool(b, "replace", false))); }
                case "PUT /collection" -> { final JsonObject b = body(req); collection().setQty(Json.str(b, "name"), Json.str(b, "set"), Json.str(b, "finish"), b.get("qty").getAsInt()); json(resp, 200, collection().summary()); }
                default -> {
                    if ("GET".equals(m) && path.startsWith("/match/")) {
                        final MatchSession ms = matches.get(path.substring("/match/".length()));
                        if (ms == null) json(resp, 404, err("no such match")); else json(resp, 200, ms.describe());
                    } else if ("DELETE".equals(m) && path.startsWith("/match/")) {
                        matches.remove(path.substring("/match/".length())); json(resp, 200, new JsonObject());
                    } else return false;
                }
            }
            return true;
        }
    }

    // ---- WebSocket ---------------------------------------------------------------------------

    private final class MatchWsServlet extends WebSocketServlet {
        @Override public void configure(final WebSocketServletFactory factory) {
            factory.getPolicy().setIdleTimeout(24L * 60 * 60 * 1000); // a game can sit idle for a long time
            factory.getPolicy().setMaxTextMessageSize(4 * 1024 * 1024);
            factory.setCreator(new WebSocketCreator() {
                @Override public Object createWebSocket(final ServletUpgradeRequest req, final ServletUpgradeResponse resp) {
                    final String path = req.getRequestPath(); // /ws/match/{id}
                    final String prefix = "/ws/match/";
                    if (!path.startsWith(prefix)) return null;
                    final MatchSession m = matches.get(path.substring(prefix.length()));
                    if (m == null) return null;
                    return new MatchSocket(m);
                }
            });
        }
    }

    private static final class MatchSocket extends WebSocketAdapter {
        private final MatchSession match;
        MatchSocket(final MatchSession match) { this.match = match; }

        @Override public void onWebSocketConnect(final Session sess) {
            super.onWebSocketConnect(sess);
            match.attach(sess);
        }
        @Override public void onWebSocketText(final String message) {
            try { match.onFrame(getSession(), Json.parse(message)); }
            catch (final Exception e) {
                e.printStackTrace();
                final JsonObject f = Json.frame("error"); f.addProperty("text", String.valueOf(e));
                try { getSession().getRemote().sendStringByFuture(Json.GSON.toJson(f)); } catch (final Exception ignored) {}
            }
        }
        @Override public void onWebSocketClose(final int statusCode, final String reason) {
            if (getSession() != null) match.detach(getSession());
            super.onWebSocketClose(statusCode, reason);
        }
        @Override public void onWebSocketError(final Throwable cause) {
            if (getSession() != null) match.detach(getSession());
        }
    }
}
