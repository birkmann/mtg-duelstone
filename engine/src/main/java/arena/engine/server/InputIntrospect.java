package arena.engine.server;

import forge.game.Game;
import forge.game.GameEntity;
import forge.game.player.Player;
import forge.game.spellability.SpellAbility;
import forge.gamemodes.match.input.Input;
import forge.gamemodes.match.input.InputSelectEntitiesFromList;
import forge.gamemodes.match.input.InputSelectTargets;
import forge.player.PlayerControllerHuman;

import java.lang.reflect.Field;
import java.util.ArrayList;
import java.util.List;

/**
 * Forge's GUIs never mark which <em>players</em> are legal to click — the user clicks an avatar and the
 * Input validates. Our protocol wants the legal set up front, so we peek into the two Inputs that accept
 * player clicks. Reflection is confined here and degrades to "every living player" if the fields move.
 */
public final class InputIntrospect {
    private InputIntrospect() {}

    private static final Field TARGETS_SA = field(InputSelectTargets.class, "sa");
    private static final Field ENTITIES_VALID = field(InputSelectEntitiesFromList.class, "validChoices");

    private static Field field(final Class<?> c, final String name) {
        try { final Field f = c.getDeclaredField(name); f.setAccessible(true); return f; }
        catch (final Exception e) { System.err.println("[InputIntrospect] " + c.getSimpleName() + "." + name + " not found: " + e); return null; }
    }

    /** Ids of players the current Input would accept a click on; empty when the Input takes no player clicks. */
    public static List<Integer> selectablePlayers(final PlayerControllerHuman pch) {
        final List<Integer> out = new ArrayList<>();
        if (pch == null || pch.getInputProxy() == null) return out;
        final Input in = pch.getInputProxy().getInput();
        final Game game = pch.getGame();
        if (in == null || game == null) return out;
        try {
            if (in instanceof InputSelectTargets && TARGETS_SA != null) {
                final SpellAbility sa = (SpellAbility) TARGETS_SA.get(in);
                if (sa != null && sa.getTargetRestrictions() != null && sa.getTargetRestrictions().canTgtPlayer()) {
                    for (final Player p : game.getPlayers()) if (!p.hasLost() && sa.canTarget(p)) out.add(p.getId());
                }
            } else if (in instanceof InputSelectEntitiesFromList && ENTITIES_VALID != null) {
                final Object valid = ENTITIES_VALID.get(in);
                if (valid instanceof Iterable<?> it) for (final Object o : it) if (o instanceof Player p) out.add(p.getId());
            }
        } catch (final Exception e) {
            System.err.println("[InputIntrospect] failed on " + in.getClass().getSimpleName() + ": " + e);
            for (final Player p : game.getPlayers()) if (!p.hasLost()) out.add(p.getId());
        }
        return out;
    }
}
