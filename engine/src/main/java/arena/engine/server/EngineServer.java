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
 * frontend as static files (the wasm client, so invitees only need a browser). Listens on all interfaces so
 * LAN players can join; CORS is wide open for dev servers.
 */
public final class EngineServer {
    private final MatchManager matches = new MatchManager();
    private final Server server;
    private final ImageCache images;
    /** Base URL other machines use to reach us (invite links); see {@link #detectPublicUrl}. */
    private final String publicUrl;
    private final boolean servesWeb;

    public EngineServer(final int port, final File staticDir, final java.nio.file.Path offlinePack) throws IOException { this(port, staticDir, offlinePack, null); }

    public EngineServer(final int port, final File staticDir, final java.nio.file.Path offlinePack, final String publicUrl) throws IOException {
        this.publicUrl = publicUrl != null && !publicUrl.isBlank() ? publicUrl.replaceAll("/+$", "") : detectPublicUrl(port);
        this.servesWeb = staticDir != null && new File(staticDir, "index.html").isFile();
        System.out.println("[server] invite links use " + this.publicUrl + (servesWeb ? "" : "  (no web client: pass --static web/dist so invitees can play in a browser)"));
        images = new ImageCache(offlinePack);
        System.out.println("[server] offline pack: " + offlinePack + " (" + images.size() + " card names with images)");
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
            st.setInitParameter("cacheControl", "no-cache"); // a rebuilt wasm bundle must not be shadowed by a stale one
            // tools/build-web.sh writes .gz twins: the ~60 MB wasm goes over the wire as ~13 MB
            st.setInitParameter("precompressed", "gzip=.gz");
            // Browsers only stream-compile wasm served as application/wasm.
            ctx.getMimeTypes().addMimeMapping("wasm", "application/wasm");
            ctx.getMimeTypes().addMimeMapping("ogg", "audio/ogg");
            ctx.getMimeTypes().addMimeMapping("ttf", "font/ttf");
            ctx.getMimeTypes().addMimeMapping("wgsl", "text/plain");
            ctx.addServlet(st, "/");
            System.out.println("[server] serving static files from " + staticDir.getAbsolutePath());
        }
        final HandlerList handlers = new HandlerList();
        handlers.addHandler(ctx);
        server.setHandler(handlers);
        arena.engine.deck.Recommender.warmUpAsync();   // ~3 s of data/tag loading off the first deck-builder request
    }

    public void start() throws Exception { server.start(); }
    public void join() throws InterruptedException { server.join(); }
    public MatchManager matches() { return matches; }

    /**
     * Best guess at an address other machines on the LAN can reach: the first up, non-loopback,
     * site-local IPv4 (192.168/10/172.16), else any non-loopback IPv4, else localhost. For play across
     * the internet pass {@code --public-url} (e.g. a tunnel or port-forwarded address).
     */
    static String detectPublicUrl(final int port) {
        String fallback = null;
        try {
            for (final java.net.NetworkInterface ni : java.util.Collections.list(java.net.NetworkInterface.getNetworkInterfaces())) {
                if (!ni.isUp() || ni.isLoopback() || ni.isVirtual()) continue;
                final String n = ni.getName();
                if (n.startsWith("docker") || n.startsWith("br-") || n.startsWith("veth") || n.startsWith("virbr")) continue;
                for (final java.net.InetAddress a : java.util.Collections.list(ni.getInetAddresses())) {
                    if (!(a instanceof java.net.Inet4Address) || a.isLoopbackAddress() || a.isLinkLocalAddress()) continue;
                    if (a.isSiteLocalAddress()) return "http://" + a.getHostAddress() + ":" + port;
                    if (fallback == null) fallback = "http://" + a.getHostAddress() + ":" + port;
                }
            }
        } catch (final Exception ignored) {}
        return fallback != null ? fallback : "http://localhost:" + port;
    }

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
                case "GET /health" -> {
                    final JsonObject o = new JsonObject();
                    o.addProperty("ok", true); o.addProperty("protocol", 3);
                    o.addProperty("cards", StaticData.instance().getCommonCards().getAllCards().size());
                    o.addProperty("publicUrl", publicUrl); o.addProperty("web", servesWeb);
                    o.addProperty("maxPlayers", MatchSession.MAX_SEATS);
                    json(resp, 200, o);
                }
                case "GET /img" -> {
                    final String name = req.getParameter("name");
                    if (name == null || name.isBlank()) throw new IllegalArgumentException("name required");
                    final String version = ImageCache.version(req.getParameter("version"));
                    final File f = images.get(name, req.getParameter("set"), "1".equals(req.getParameter("back")), version);
                    cors(resp);
                    if (f == null) { resp.setStatus(404); return true; }
                    // the pack is all JPEG, whatever version was asked for
                    resp.setStatus(200); resp.setContentType("image/jpeg");
                    resp.setHeader("Cache-Control", "public, max-age=31536000, immutable");
                    resp.setContentLengthLong(f.length());
                    java.nio.file.Files.copy(f.toPath(), resp.getOutputStream());
                }
                case "GET /rulings" -> {
                    final String r = images.rulings(req.getParameter("name"));
                    cors(resp);
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
                    json(resp, 201, matches.create(human, ai, GameSetup.fromJson(b), seedI == null ? null : seedI.longValue(), Json.str(b, "humanName"), Avatars.clean(b.get("humanAvatar"))).describe());
                }
                case "POST /lobby" -> { // {format, players: 2..4, name, deck, avatar?} → host ticket (seat 0 token + invite)
                    final JsonObject b = body(req);
                    final Integer players = Json.integer(b, "players");
                    final MatchSession lm = matches.createLobby(GameSetup.fromJson(b), players == null ? 2 : players,
                            Json.str(b, "name"), Json.str(b, "deck"), Avatars.clean(b.get("avatar")), publicUrl);
                    json(resp, 201, lm.hostTicket());
                }
                // ---- game setup / cubes / events (v3) ----------------------------------------------
                case "GET /formats" -> json(resp, 200, formatsJson());
                case "GET /cubes" -> json(resp, 200, CubeStore.list());
                case "GET /cube" -> json(resp, 200, CubeStore.get(req.getParameter("ref")));
                case "POST /cube" -> json(resp, 200, CubeStore.save(body(req)));
                case "DELETE /cube" -> { CubeStore.delete(req.getParameter("name")); json(resp, 200, new JsonObject()); }
                case "POST /cube/import" -> { final JsonObject b = body(req); json(resp, 200, CubeStore.importText(Json.str(b, "text") == null ? "" : Json.str(b, "text"))); }
                case "GET /sets" -> json(resp, 200, CubeStore.draftableSets());
                case "POST /event" -> json(resp, 201, matches.createEvent(body(req), publicUrl).hostTicket());
                // ---- cards ----------------------------------------------------------------------------
                case "GET /cards/search" -> {
                    final Integer cmc = req.getParameter("cmc") == null || req.getParameter("cmc").isBlank() ? null : intParam(req, "cmc", 0);
                    final var found = catalog.search(req.getParameter("q") == null ? "" : req.getParameter("q"), req.getParameter("ci"), req.getParameter("type"), req.getParameter("text"), "1".equals(req.getParameter("commander")),
                            cmc, req.getParameter("tag"), req.getParameter("sort"), Math.max(0, intParam(req, "offset", 0)), Math.min(200, intParam(req, "limit", 60)));
                    json(resp, 200, catalog.cardsJson(found));
                }
                case "POST /deck/recommend" -> json(resp, 200, arena.engine.deck.Recommender.get().recommend(body(req)));
                case "GET /cards/get" -> {
                    final PaperCard pc = catalog.resolveExact(req.getParameter("name"));
                    if (pc == null) json(resp, 404, err("unknown card")); else json(resp, 200, catalog.cardJson(pc));
                }
                // ---- decks ----------------------------------------------------------------------------
                case "GET /decks" -> { // lobby list; ?format=constructed|commander (default commander); ?kind=user|all
                    final String fmt = DeckSource.format(req.getParameter("format"));
                    final String kind = req.getParameter("kind") == null ? "all" : req.getParameter("kind");
                    if ("user".equals(kind)) json(resp, 200, deckStore.list(fmt));
                    else json(resp, 200, DeckSource.list(fmt, req.getParameter("legality")));
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
                    } else if (path.startsWith("/event/")) { // GET /event/{code} · POST /event/{code}/join {name, avatar?}
                        final String[] parts = path.substring("/event/".length()).split("/");
                        final EventSession ev = matches.event(parts[0]);
                        if (ev == null) { json(resp, 404, err("no event with code " + parts[0])); return true; }
                        if ("GET".equals(m) && parts.length == 1) json(resp, 200, ev.describe());
                        else if ("POST".equals(m) && parts.length == 2 && "join".equals(parts[1])) {
                            final JsonObject b = body(req);
                            try { json(resp, 200, ev.join(Json.str(b, "name"), Avatars.clean(b.get("avatar")))); }
                            catch (final IllegalStateException e) { json(resp, 409, err(e.getMessage())); }
                        } else return false;
                    } else if (path.startsWith("/lobby/")) { // GET /lobby/{id} (public info) · POST /lobby/{id}/join {name, deck, avatar?}
                        final String[] parts = path.substring("/lobby/".length()).split("/");
                        final MatchSession lm = matches.get(parts[0]);
                        if (lm == null || !lm.lobby) { json(resp, 404, err("no lobby with code " + parts[0])); return true; }
                        if ("GET".equals(m) && parts.length == 1) json(resp, 200, lm.describe());
                        else if ("POST".equals(m) && parts.length == 2 && "join".equals(parts[1])) {
                            final JsonObject b = body(req);
                            try { json(resp, 200, lm.join(Json.str(b, "name"), Json.str(b, "deck"), Avatars.clean(b.get("avatar")))); }
                            catch (final IllegalStateException e) { json(resp, 409, err(e.getMessage())); }
                        } else return false;
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
                    final String path = req.getRequestPath(); // /ws/match/{id} · /ws/event/{id}
                    final java.util.List<String> etok = req.getParameterMap().get("token");
                    if (path.startsWith("/ws/event/")) {
                        final EventSession ev = matches.event(path.substring("/ws/event/".length()));
                        final EventSession.ESeat es = ev == null ? null : ev.seatFor(etok == null || etok.isEmpty() ? null : etok.get(0));
                        if (es == null) { try { resp.sendForbidden("unknown event or seat token"); } catch (final IOException ignored) {} return null; }
                        return new EventSocket(ev, es);
                    }
                    final String prefix = "/ws/match/";
                    if (!path.startsWith(prefix)) return null;
                    final MatchSession m = matches.get(path.substring(prefix.length()));
                    if (m == null) return null;
                    final java.util.List<String> tok = req.getParameterMap().get("token");
                    final Seat seat = m.seatFor(tok == null || tok.isEmpty() ? null : tok.get(0));
                    if (seat == null) {
                        try { resp.sendForbidden("unknown seat token"); } catch (final IOException ignored) {}
                        return null;
                    }
                    return new MatchSocket(m, seat);
                }
            });
        }
    }

    private static final class EventSocket extends WebSocketAdapter {
        private final EventSession ev;
        private final EventSession.ESeat seat;
        EventSocket(final EventSession ev, final EventSession.ESeat seat) { this.ev = ev; this.seat = seat; }
        @Override public void onWebSocketConnect(final Session sess) { super.onWebSocketConnect(sess); ev.attach(seat, sess); }
        @Override public void onWebSocketText(final String message) {
            try { ev.onFrame(seat, getSession(), Json.parse(message)); } catch (final Exception e) { e.printStackTrace(); }
        }
        @Override public void onWebSocketClose(final int statusCode, final String reason) {
            if (getSession() != null) ev.detach(seat, getSession());
            super.onWebSocketClose(statusCode, reason);
        }
        @Override public void onWebSocketError(final Throwable cause) { if (getSession() != null) ev.detach(seat, getSession()); }
    }

    /** Everything a client can set up: formats, legalities (with Forge's names), casual variants. */
    private static JsonObject formatsJson() {
        final JsonObject o = new JsonObject();
        final JsonArray f = new JsonArray(), l = new JsonArray(), v = new JsonArray();
        for (final String[] x : new String[][] { {"constructed", "Constructed"}, {"commander", "Commander"}, {"brawl", "Brawl"}, {"oathbreaker", "Oathbreaker"} }) f.add(named(x[0], x[1], null));
        for (final String id : DeckSource.LEGALITIES) {
            final forge.game.GameFormat gf = DeckSource.gameFormat(id);
            l.add(named(id, gf.getName(), gf.getBannedCardNames().size() + " banned"));
        }
        for (final String[] x : new String[][] { {"planechase", "Planechase"}, {"archenemy", "Archenemy"}, {"momir", "Momir Basic"}, {"teams", "2v2 Teams"} }) v.add(named(x[0], x[1], null));
        o.add("formats", f); o.add("legalities", l); o.add("variants", v);
        return o;
    }

    private static JsonObject named(final String id, final String name, final String desc) {
        final JsonObject o = new JsonObject();
        o.addProperty("id", id); o.addProperty("name", name); o.addProperty("description", desc);
        return o;
    }

    private static final class MatchSocket extends WebSocketAdapter {
        private final MatchSession match;
        private final Seat seat;
        MatchSocket(final MatchSession match, final Seat seat) { this.match = match; this.seat = seat; }

        @Override public void onWebSocketConnect(final Session sess) {
            super.onWebSocketConnect(sess);
            match.attach(seat, sess);
        }
        @Override public void onWebSocketText(final String message) {
            try { match.onFrame(seat, getSession(), Json.parse(message)); }
            catch (final Exception e) {
                e.printStackTrace();
                final JsonObject f = Json.frame("error"); f.addProperty("text", String.valueOf(e));
                try { getSession().getRemote().sendStringByFuture(Json.GSON.toJson(f)); } catch (final Exception ignored) {}
            }
        }
        @Override public void onWebSocketClose(final int statusCode, final String reason) {
            if (getSession() != null) match.detach(seat, getSession());
            super.onWebSocketClose(statusCode, reason);
        }
        @Override public void onWebSocketError(final Throwable cause) {
            if (getSession() != null) match.detach(seat, getSession());
        }
    }
}
