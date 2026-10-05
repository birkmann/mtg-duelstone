package arena.engine.server;

import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonNull;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;

import java.util.List;

/** Tiny Gson conveniences shared by the server. */
public final class Json {
    public static final Gson GSON = new GsonBuilder().disableHtmlEscaping().serializeNulls().create();
    private Json() {}

    public static JsonObject obj() { return new JsonObject(); }

    /** {"t": type, ...} frame builder. */
    public static JsonObject frame(final String type) {
        final JsonObject o = new JsonObject();
        o.addProperty("t", type);
        return o;
    }

    public static JsonObject parse(final String s) { return JsonParser.parseString(s).getAsJsonObject(); }

    public static JsonArray strings(final List<String> ss) {
        final JsonArray a = new JsonArray();
        if (ss != null) for (final String s : ss) a.add(s);
        return a;
    }

    public static String str(final JsonObject o, final String k) {
        final JsonElement e = o.get(k);
        return e == null || e.isJsonNull() ? null : e.getAsString();
    }
    public static Integer integer(final JsonObject o, final String k) {
        final JsonElement e = o.get(k);
        return e == null || e.isJsonNull() ? null : e.getAsInt();
    }
    public static boolean bool(final JsonObject o, final String k, final boolean dflt) {
        final JsonElement e = o.get(k);
        return e == null || e.isJsonNull() ? dflt : e.getAsBoolean();
    }
    public static JsonElement nn(final JsonElement e) { return e == null ? JsonNull.INSTANCE : e; }
}
