package arena.engine.server;

import com.google.common.eventbus.Subscribe;
import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import forge.game.GameEntityView;
import forge.game.card.CardView;
import forge.game.event.GameEvent;
import forge.game.event.GameEventAttackersDeclared;
import forge.game.event.GameEventBlockersDeclared;
import forge.game.event.GameEventCardChangeZone;
import forge.game.event.GameEventCardCounters;
import forge.game.event.GameEventCardDamaged;
import forge.game.event.GameEventCardSacrificed;
import forge.game.event.GameEventCardTapped;
import forge.game.event.GameEventPlayerDamaged;
import forge.game.event.GameEventPlayerLivesChanged;
import forge.game.event.GameEventSpellAbilityCast;
import forge.game.event.GameEventTurnBegan;
import forge.game.event.IGameEventVisitor;
import forge.game.player.PlayerView;
import forge.game.spellability.StackItemView;
import forge.game.zone.ZoneType;
import forge.game.zone.ZoneView;

import java.util.ArrayList;
import java.util.HashSet;
import java.util.List;
import java.util.Set;
import java.util.concurrent.ConcurrentLinkedQueue;

/**
 * Protocol v2 {@code events}: subscribes to Forge's game event bus (Guava {@code @Subscribe}, the same hook
 * forge-gui's FControlGameEventHandler uses) and turns the events into small animation cues. Runs on the
 * game thread, so it only builds JSON and enqueues — {@link WsGuiGame#flush()} drains the queue on the EDT
 * and sends one batched frame just before the state frame that reflects the changes. Never blocks.
 *
 * Forge's CardDestroyed / TokenCreated events carry no payload, so those cues are derived from zone changes
 * (battlefield → graveyard that was not announced as a sacrifice; token arriving on the battlefield).
 */
final class EventCollector extends IGameEventVisitor.Base<Void> {
    private static final int MAX_QUEUE = 4000; // safety valve if the EDT is wedged
    private final ConcurrentLinkedQueue<JsonObject> queue = new ConcurrentLinkedQueue<>();
    private final Set<Integer> sacrificed = new HashSet<>(); // game thread only
    private final Runnable onEvent;

    EventCollector(final Runnable onEvent) { this.onEvent = onEvent; }

    @Subscribe
    public void receive(final GameEvent ev) {
        try { ev.visit(this); }
        catch (final Throwable t) { System.err.println("[events] " + ev.getClass().getSimpleName() + ": " + t); }
    }

    /** Everything queued so far as one {@code items} array, or null if nothing happened. */
    JsonArray drain() {
        JsonObject o;
        JsonArray a = null;
        while ((o = queue.poll()) != null) { if (a == null) a = new JsonArray(); a.add(o); }
        return a;
    }

    private JsonObject cue(final String kind) {
        final JsonObject o = new JsonObject();
        o.addProperty("kind", kind);
        return o;
    }
    private void emit(final JsonObject o) {
        if (queue.size() < MAX_QUEUE) queue.add(o);
        onEvent.run();
    }
    private static void card(final JsonObject o, final String k, final CardView c) { if (c != null) o.addProperty(k, c.getId()); }

    private static void entity(final JsonObject o, final GameEntityView e) {
        if (e instanceof CardView c) o.addProperty("targetCard", c.getId());
        else if (e instanceof PlayerView p) o.addProperty("targetPlayer", p.getId());
    }

    @Override public Void visit(final GameEventCardDamaged e) {
        final JsonObject o = cue("damage");
        card(o, "source", e.source()); card(o, "targetCard", e.card());
        o.addProperty("amount", e.amount());
        emit(o);
        return null;
    }
    @Override public Void visit(final GameEventPlayerDamaged e) {
        final JsonObject o = cue("damage");
        card(o, "source", e.source());
        if (e.target() != null) o.addProperty("targetPlayer", e.target().getId());
        o.addProperty("amount", e.amount());
        if (e.combat()) o.addProperty("label", "combat");
        emit(o);
        return null;
    }
    @Override public Void visit(final GameEventPlayerLivesChanged e) {
        if (e.newLives() == e.oldLives()) return null;
        final JsonObject o = cue("lifeChanged");
        if (e.player() != null) o.addProperty("targetPlayer", e.player().getId());
        o.addProperty("amount", e.newLives() - e.oldLives());
        emit(o);
        return null;
    }
    @Override public Void visit(final GameEventCardChangeZone e) {
        final CardView c = e.card();
        if (c == null) return null;
        final ZoneType from = zone(e.from()), to = zone(e.to());
        final JsonObject o = cue("zoneChange");
        o.addProperty("card", c.getId());
        if (from != null) o.addProperty("zoneFrom", from.name());
        if (to != null) o.addProperty("zoneTo", to.name());
        emit(o);
        if (to == ZoneType.Battlefield && from != ZoneType.Battlefield && c.isToken()) {
            final JsonObject t = cue("tokenCreated"); t.addProperty("card", c.getId()); emit(t);
        }
        if (from == ZoneType.Battlefield && to == ZoneType.Graveyard && !c.isToken()) {
            final boolean sac = sacrificed.remove(c.getId());
            final JsonObject d = cue(sac ? "sacrificed" : "destroyed");
            d.addProperty("card", c.getId());
            emit(d);
        }
        return null;
    }
    private static ZoneType zone(final ZoneView z) { return z == null ? null : z.zoneType(); }

    @Override public Void visit(final GameEventCardSacrificed e) {
        // The graveyard move that follows turns into the "sacrificed" cue; remember the id (bounded).
        if (e.card() != null) { if (sacrificed.size() > 256) sacrificed.clear(); sacrificed.add(e.card().getId()); }
        return null;
    }
    @Override public Void visit(final GameEventCardTapped e) {
        final JsonObject o = cue(e.tapped() ? "tap" : "untap");
        card(o, "card", e.card());
        emit(o);
        return null;
    }
    @Override public Void visit(final GameEventCardCounters e) {
        final int delta = e.newValue() - e.oldValue();
        if (delta == 0) return null;
        final JsonObject o = cue("counters");
        card(o, "card", e.card());
        o.addProperty("amount", delta);
        o.addProperty("label", e.type() == null ? "" : e.type().toString());
        emit(o);
        return null;
    }
    @Override public Void visit(final GameEventSpellAbilityCast e) {
        final StackItemView si = e.si();
        final JsonObject o = cue("spellCast");
        card(o, "card", si != null ? si.getSourceCard() : e.sa() == null ? null : e.sa().getHostCard());
        if (si != null) {
            o.addProperty("label", si.isTrigger() ? "trigger" : si.isAbility() ? "ability" : "spell");
            if (si.getTargetCards() != null && !si.getTargetCards().isEmpty()) o.addProperty("targetCard", si.getTargetCards().iterator().next().getId());
            else if (si.getTargetPlayers() != null && !si.getTargetPlayers().isEmpty()) o.addProperty("targetPlayer", si.getTargetPlayers().iterator().next().getId());
        }
        emit(o);
        return null;
    }
    @Override public Void visit(final GameEventAttackersDeclared e) {
        for (final var en : e.attackersMap().entries()) {
            final JsonObject o = cue("attack");
            card(o, "card", en.getValue());
            entity(o, en.getKey());
            emit(o);
        }
        return null;
    }
    @Override public Void visit(final GameEventBlockersDeclared e) {
        final List<JsonObject> out = new ArrayList<>();
        for (final var byDefender : e.blockers().values()) {
            for (final var en : byDefender.entries()) { // key = attacker, value = blocker (an unblocked attacker maps to itself)
                if (en.getKey().getId() == en.getValue().getId()) continue;
                final JsonObject o = cue("block");
                card(o, "card", en.getValue());
                card(o, "targetCard", en.getKey());
                out.add(o);
            }
        }
        out.forEach(this::emit);
        return null;
    }
    @Override public Void visit(final GameEventTurnBegan e) {
        final JsonObject o = cue("turnBegan");
        if (e.turnOwner() != null) o.addProperty("targetPlayer", e.turnOwner().getId());
        o.addProperty("amount", e.turnNumber());
        emit(o);
        return null;
    }
}
