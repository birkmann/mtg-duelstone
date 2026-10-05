package arena.engine.deck;

import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import forge.item.PaperCard;

import java.nio.file.Files;
import java.nio.file.Path;
import java.sql.Connection;
import java.sql.DriverManager;
import java.sql.PreparedStatement;
import java.sql.ResultSet;
import java.sql.SQLException;
import java.sql.Statement;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;

/** Your physical/online collection in SQLite: (name, set, finish) → qty. CSV import with header auto-detection. */
public final class CollectionStore implements AutoCloseable {
    private final Connection db;
    private final CardCatalog catalog = CardCatalog.get();

    public CollectionStore(final Path file) throws SQLException, java.io.IOException {
        Files.createDirectories(file.getParent());
        db = DriverManager.getConnection("jdbc:sqlite:" + file.toAbsolutePath());
        try (Statement st = db.createStatement()) {
            st.execute("CREATE TABLE IF NOT EXISTS collection (name TEXT NOT NULL, set_code TEXT NOT NULL DEFAULT '', finish TEXT NOT NULL DEFAULT 'nonfoil', qty INTEGER NOT NULL, PRIMARY KEY (name, set_code, finish))");
            st.execute("CREATE INDEX IF NOT EXISTS collection_name ON collection(name)");
        }
    }

    public static Path defaultFile() {
        final String xdg = System.getenv("XDG_DATA_HOME");
        final Path base = xdg != null && !xdg.isBlank() ? Path.of(xdg) : Path.of(System.getProperty("user.home"), ".local", "share");
        return base.resolve("commander-arena").resolve("arena.db");
    }

    @Override public void close() throws SQLException { db.close(); }

    public synchronized JsonObject summary() throws SQLException {
        final JsonObject o = new JsonObject();
        try (Statement st = db.createStatement(); ResultSet rs = st.executeQuery("SELECT COUNT(DISTINCT name), COALESCE(SUM(qty),0) FROM collection")) {
            rs.next(); o.addProperty("distinct", rs.getInt(1)); o.addProperty("total", rs.getInt(2));
        }
        return o;
    }

    /** All owned cards (grouped by name) with metadata. */
    public synchronized JsonArray list(final String q, final int limit) throws SQLException {
        final JsonArray a = new JsonArray();
        final String sql = "SELECT name, SUM(qty) AS n, GROUP_CONCAT(DISTINCT set_code) AS sets FROM collection " + (q != null && !q.isBlank() ? "WHERE lower(name) LIKE ? " : "") + "GROUP BY name ORDER BY name LIMIT ?";
        try (PreparedStatement ps = db.prepareStatement(sql)) {
            int i = 1;
            if (q != null && !q.isBlank()) ps.setString(i++, "%" + q.toLowerCase(Locale.ROOT) + "%");
            ps.setInt(i, limit);
            try (ResultSet rs = ps.executeQuery()) {
                while (rs.next()) {
                    final JsonObject o = new JsonObject();
                    final PaperCard pc = catalog.resolveExact(rs.getString(1));
                    o.add("card", pc == null ? null : catalog.cardJson(pc));
                    o.addProperty("name", rs.getString(1));
                    o.addProperty("qty", rs.getInt(2));
                    o.addProperty("sets", rs.getString(3));
                    a.add(o);
                }
            }
        }
        return a;
    }

    public synchronized Map<String, Integer> owned(final List<String> names) throws SQLException {
        final Map<String, Integer> out = new LinkedHashMap<>();
        if (names.isEmpty()) return out;
        try (PreparedStatement ps = db.prepareStatement("SELECT SUM(qty) FROM collection WHERE name = ?")) {
            for (final String n : names) { ps.setString(1, n); try (ResultSet rs = ps.executeQuery()) { out.put(n, rs.next() ? rs.getInt(1) : 0); } }
        }
        return out;
    }

    public synchronized void setQty(final String name, final String set, final String finish, final int qty) throws SQLException {
        final PaperCard pc = catalog.resolveExact(name);
        final String canonical = pc == null ? name : pc.getName();
        if (qty <= 0) {
            try (PreparedStatement ps = db.prepareStatement("DELETE FROM collection WHERE name=? AND set_code=? AND finish=?")) { ps.setString(1, canonical); ps.setString(2, set == null ? "" : set); ps.setString(3, finish == null ? "nonfoil" : finish); ps.executeUpdate(); }
        } else {
            try (PreparedStatement ps = db.prepareStatement("INSERT INTO collection(name,set_code,finish,qty) VALUES(?,?,?,?) ON CONFLICT(name,set_code,finish) DO UPDATE SET qty=excluded.qty")) { ps.setString(1, canonical); ps.setString(2, set == null ? "" : set); ps.setString(3, finish == null ? "nonfoil" : finish); ps.setInt(4, qty); ps.executeUpdate(); }
        }
    }

    /**
     * CSV import (Moxfield, Archidekt, Deckbox, ManaBox, Delver, plain "qty,name[,set]"). Header columns are
     * auto-detected. @param replace wipe the collection first. Returns {imported, unresolved:[...]}.
     */
    public synchronized JsonObject importCsv(final String csv, final boolean replace) throws SQLException {
        final List<String[]> rows = parseCsv(csv);
        final JsonObject out = new JsonObject();
        final JsonArray unresolved = new JsonArray();
        if (rows.isEmpty()) { out.addProperty("imported", 0); out.add("unresolved", unresolved); return out; }
        // detect header
        final String[] header = rows.get(0);
        int iQty = -1, iName = -1, iSet = -1, iFoil = -1;
        for (int i = 0; i < header.length; i++) {
            final String h = header[i].trim().toLowerCase(Locale.ROOT);
            if (iQty < 0 && (h.equals("count") || h.equals("quantity") || h.equals("qty") || h.equals("amount") || h.equals("reg qty") || h.equals("owned"))) iQty = i;
            else if (iName < 0 && (h.equals("name") || h.equals("card name") || h.equals("card"))) iName = i;
            else if (iSet < 0 && (h.equals("edition") || h.equals("set") || h.equals("set code") || h.equals("edition code") || h.equals("set_code"))) iSet = i;
            else if (iFoil < 0 && (h.equals("foil") || h.equals("finish") || h.equals("printing"))) iFoil = i;
        }
        int start = 1;
        if (iName < 0) { // headerless: assume "qty,name[,set]" or "name,qty"
            start = 0;
            if (header.length >= 2 && header[0].trim().matches("\\d+")) { iQty = 0; iName = 1; iSet = header.length > 2 ? 2 : -1; }
            else if (header.length >= 2 && header[1].trim().matches("\\d+")) { iName = 0; iQty = 1; }
            else { iName = 0; }
        }
        db.setAutoCommit(false);
        try {
            if (replace) try (Statement st = db.createStatement()) { st.execute("DELETE FROM collection"); }
            int imported = 0;
            try (PreparedStatement ps = db.prepareStatement("INSERT INTO collection(name,set_code,finish,qty) VALUES(?,?,?,?) ON CONFLICT(name,set_code,finish) DO UPDATE SET qty=qty+excluded.qty")) {
                for (int r = start; r < rows.size(); r++) {
                    final String[] row = rows.get(r);
                    if (row.length <= iName) continue;
                    final String name = row[iName].trim();
                    if (name.isEmpty()) continue;
                    int qty = 1;
                    if (iQty >= 0 && iQty < row.length) { try { qty = Integer.parseInt(row[iQty].trim().replaceAll("[^0-9]", "")); } catch (final NumberFormatException ignored) {} }
                    final String set = iSet >= 0 && iSet < row.length ? row[iSet].trim().toUpperCase(Locale.ROOT) : "";
                    final String foilRaw = iFoil >= 0 && iFoil < row.length ? row[iFoil].trim().toLowerCase(Locale.ROOT) : "";
                    final String finish = foilRaw.isEmpty() || foilRaw.equals("false") || foilRaw.equals("no") || foilRaw.equals("normal") || foilRaw.equals("nonfoil") ? "nonfoil" : "foil";
                    final PaperCard pc = catalog.resolveExact(name);
                    if (pc == null) { unresolved.add(name); continue; }
                    ps.setString(1, pc.getName()); ps.setString(2, set); ps.setString(3, finish); ps.setInt(4, qty);
                    ps.executeUpdate();
                    imported += qty;
                }
            }
            db.commit();
            out.addProperty("imported", imported);
        } catch (final SQLException e) { db.rollback(); throw e; }
        finally { db.setAutoCommit(true); }
        out.add("unresolved", unresolved);
        return out;
    }

    static List<String[]> parseCsv(final String text) {
        final List<String[]> rows = new ArrayList<>();
        for (final String line : text.split("\\r?\\n")) {
            if (line.isBlank()) continue;
            final char sep = line.indexOf('\t') >= 0 && line.indexOf(',') < 0 ? '\t' : line.indexOf(';') >= 0 && line.indexOf(',') < 0 ? ';' : ',';
            final List<String> cells = new ArrayList<>();
            final StringBuilder cur = new StringBuilder();
            boolean inQ = false;
            for (int i = 0; i < line.length(); i++) {
                final char c = line.charAt(i);
                if (inQ) { if (c == '"') { if (i + 1 < line.length() && line.charAt(i + 1) == '"') { cur.append('"'); i++; } else inQ = false; } else cur.append(c); }
                else if (c == '"') inQ = true;
                else if (c == sep) { cells.add(cur.toString()); cur.setLength(0); }
                else cur.append(c);
            }
            cells.add(cur.toString());
            rows.add(cells.toArray(new String[0]));
        }
        return rows;
    }

    public static List<String> split(final String s) { return s == null || s.isBlank() ? List.of() : Arrays.asList(s.split("\\|")); }
}
