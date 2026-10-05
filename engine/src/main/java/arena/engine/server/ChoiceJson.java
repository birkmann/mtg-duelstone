package arena.engine.server;

import arena.engine.view.ViewJson;
import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import forge.game.GameEntityView;
import forge.game.card.CardView;
import forge.game.player.DelayedReveal;
import forge.game.player.PlayerView;
import forge.game.spellability.SpellAbilityView;
import forge.game.spellability.StackItemView;
import forge.util.FSerializableFunction;

import java.util.List;

/**
 * Generic choice options for prompts. Forge hands the GUI arbitrary {@code List<T>} (cards, players,
 * abilities, strings, integers, enums...). We describe each option as {@code {i, type, label, ...}}
 * and the client answers with indices, so the server never has to reconstruct engine objects.
 */
public final class ChoiceJson {
    private ChoiceJson() {}

    public static <T> JsonArray options(final List<T> choices, final FSerializableFunction<T, String> display, final ViewJson vj) {
        final JsonArray a = new JsonArray();
        if (choices == null) return a;
        int i = 0;
        for (final T c : choices) a.add(option(i++, c, display == null ? null : display.apply(c), vj));
        return a;
    }

    public static JsonObject option(final int i, final Object c, final String displayLabel, final ViewJson vj) {
        final JsonObject o = new JsonObject();
        o.addProperty("i", i);
        if (c instanceof CardView cv) {
            o.addProperty("type", "card");
            o.addProperty("id", cv.getId());
            o.addProperty("label", displayLabel != null ? displayLabel : (vj.mayView(cv) ? cv.getName() : "(hidden card)"));
            o.add("card", vj.card(cv));
        } else if (c instanceof PlayerView pv) {
            o.addProperty("type", "player");
            o.addProperty("id", pv.getId());
            o.addProperty("label", displayLabel != null ? displayLabel : pv.getLobbyPlayerName());
        } else if (c instanceof SpellAbilityView sa) {
            o.addProperty("type", "ability");
            o.addProperty("id", sa.getId());
            o.addProperty("label", displayLabel != null ? displayLabel : sa.getDescription());
            o.add("ability", vj.ability(sa));
        } else if (c instanceof StackItemView si) {
            o.addProperty("type", "stackItem");
            o.addProperty("id", si.getId());
            o.addProperty("label", displayLabel != null ? displayLabel : si.getText());
        } else if (c instanceof GameEntityView ge) {
            o.addProperty("type", "entity");
            o.addProperty("id", ge.getId());
            o.addProperty("label", displayLabel != null ? displayLabel : ge.getName());
        } else if (c instanceof Number n) {
            o.addProperty("type", "number");
            o.addProperty("value", n);
            o.addProperty("label", displayLabel != null ? displayLabel : String.valueOf(n));
        } else if (c instanceof Byte b) { // mana color bit in assignGenericAmount
            o.addProperty("type", "color");
            o.addProperty("value", b);
            o.addProperty("label", displayLabel != null ? displayLabel : String.valueOf(b));
        } else {
            o.addProperty("type", "text");
            o.addProperty("label", displayLabel != null ? displayLabel : String.valueOf(c));
            o.addProperty("javaType", c == null ? "null" : c.getClass().getSimpleName());
        }
        return o;
    }

    public static JsonObject reveal(final DelayedReveal dr, final ViewJson vj) {
        if (dr == null) return null;
        final JsonObject o = new JsonObject();
        o.add("cards", vj.cards(dr.getCards()));
        final JsonArray zones = new JsonArray();
        if (dr.getZone() != null) dr.getZone().forEach(z -> zones.add(z.name()));
        o.add("zones", zones);
        o.add("owner", ViewJson.refOrNull(dr.getOwner()));
        o.addProperty("message", dr.getMessagePrefix());
        return o;
    }
}
