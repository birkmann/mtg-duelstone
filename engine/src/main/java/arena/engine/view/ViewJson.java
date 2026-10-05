package arena.engine.view;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonNull;
import com.google.gson.JsonObject;
import forge.card.ColorSet;
import forge.card.MagicColor;
import forge.card.mana.ManaCost;
import forge.game.GameEntityView;
import forge.game.GameView;
import forge.game.card.CardView;
import forge.game.card.CardView.CardStateView;
import forge.game.card.CounterType;
import forge.game.combat.CombatView;
import forge.game.player.PlayerView;
import forge.game.spellability.SpellAbilityView;
import forge.game.spellability.StackItemView;
import forge.game.zone.ZoneType;
import forge.util.collect.FCollectionView;

import com.google.common.collect.Multiset;
import forge.game.keyword.KeywordView;

import java.util.Collection;

/**
 * Serializes Forge view objects ({@link GameView}, {@link PlayerView}, {@link CardView}, ...) to JSON
 * for one viewer. Hidden information is enforced here: any card the viewer may not look at is
 * emitted as a card back ({@code {"id":..,"hidden":true,...}}) — the engine's own
 * {@link CardView#canBeShownTo(PlayerView)} decides, never us.
 *
 * Ids are the engine's trackable ids and stay stable for the whole game, so the frontend can use
 * them as keys and the prompt protocol can reference them.
 */
public final class ViewJson {
    private final PlayerView viewer; // null = omniscient (debug/spectator)

    public ViewJson(final PlayerView viewer) { this.viewer = viewer; }

    public boolean mayView(final CardView c) {
        return viewer == null || c.canBeShownTo(viewer);
    }

    // ---- game -----------------------------------------------------------------------------

    public JsonObject game(final GameView gv) {
        final JsonObject o = new JsonObject();
        o.addProperty("turn", gv.getTurn());
        o.addProperty("phase", gv.getPhase() == null ? null : gv.getPhase().name());
        o.add("playerTurn", refOrNull(gv.getPlayerTurn()));
        o.addProperty("isCommander", gv.isCommander());
        o.addProperty("gameOver", gv.isGameOver());
        o.addProperty("matchOver", gv.isMatchOver());
        o.addProperty("stormCount", gv.getStormCount());
        if (gv.isGameOver()) o.addProperty("winner", gv.getWinningPlayerName());

        final JsonArray players = new JsonArray();
        for (final PlayerView p : gv.getPlayers()) players.add(player(p));
        o.add("players", players);

        final JsonArray stack = new JsonArray();
        final FCollectionView<StackItemView> st = gv.getStack();
        if (st != null) for (final StackItemView si : st) stack.add(stackItem(si));
        o.add("stack", stack);

        o.add("combat", combat(gv.getCombat()));
        return o;
    }

    // ---- player ---------------------------------------------------------------------------

    public JsonObject player(final PlayerView p) {
        final JsonObject o = new JsonObject();
        o.addProperty("id", p.getId());
        o.addProperty("name", p.getLobbyPlayerName());
        o.addProperty("isAI", p.isAI());
        o.addProperty("isViewer", viewer != null && viewer.getId() == p.getId());
        o.addProperty("life", p.getLife());
        o.addProperty("hasPriority", p.getHasPriority());
        o.addProperty("hasLost", p.getHasLost());
        o.addProperty("maxHandSize", p.getMaxHandSize());
        o.addProperty("unlimitedHandSize", p.hasUnlimitedHandSize());
        o.addProperty("landsPlayedThisTurn", p.getNumLandThisTurn());
        o.addProperty("maxLandPlays", p.getMaxLandPlay());
        o.addProperty("isExtraTurn", p.getIsExtraTurn());
        o.add("counters", counters(p.getCounters()));

        final JsonObject mana = new JsonObject();
        mana.addProperty("W", p.getMana(MagicColor.WHITE));
        mana.addProperty("U", p.getMana(MagicColor.BLUE));
        mana.addProperty("B", p.getMana(MagicColor.BLACK));
        mana.addProperty("R", p.getMana(MagicColor.RED));
        mana.addProperty("G", p.getMana(MagicColor.GREEN));
        mana.addProperty("C", p.getMana(MagicColor.COLORLESS));
        o.add("manaPool", mana);

        final JsonArray commanders = new JsonArray();
        final JsonObject cmdDamage = new JsonObject();
        final JsonObject cmdCast = new JsonObject();
        if (p.getCommanders() != null) {
            for (final CardView c : p.getCommanders()) {
                commanders.add(c.getId());
                cmdCast.addProperty(String.valueOf(c.getId()), p.getCommanderCast(c));
            }
        }
        // damage dealt TO this player BY each commander in the game
        for (final PlayerView other : p.getOpponents() == null ? java.util.List.<PlayerView>of() : p.getOpponents()) {
            if (other.getCommanders() == null) continue;
            for (final CardView c : other.getCommanders()) {
                final int dmg = p.getCommanderDamage(c);
                if (dmg > 0) cmdDamage.addProperty(String.valueOf(c.getId()), dmg);
            }
        }
        o.add("commanders", commanders);
        o.add("commanderCast", cmdCast);
        o.add("commanderDamage", cmdDamage);

        final JsonObject zones = new JsonObject();
        zones.add("battlefield", cards(p.getBattlefield()));
        zones.add("hand", cards(p.getHand()));
        zones.add("graveyard", cards(p.getGraveyard()));
        zones.add("exile", cards(p.getExile()));
        zones.add("command", cards(p.getCommand()));
        zones.add("library", cards(p.getLibrary()));
        o.add("zones", zones);
        return o;
    }

    // ---- cards ----------------------------------------------------------------------------

    public JsonArray cards(final Iterable<CardView> cards) {
        final JsonArray a = new JsonArray();
        if (cards != null) for (final CardView c : cards) a.add(card(c));
        return a;
    }

    public JsonElement card(final CardView c) {
        if (c == null) return JsonNull.INSTANCE;
        final JsonObject o = new JsonObject();
        o.addProperty("id", c.getId());
        o.addProperty("zone", c.getZone() == null ? null : c.getZone().name());
        o.add("owner", refOrNull(c.getOwner()));
        o.add("controller", refOrNull(c.getController()));
        if (!mayView(c)) {
            o.addProperty("hidden", true);
            o.addProperty("faceDown", c.isFaceDown());
            return o;
        }
        o.addProperty("hidden", false);
        o.addProperty("name", c.getName());
        o.addProperty("oracleName", c.getOracleName());
        o.addProperty("tapped", c.isTapped());
        o.addProperty("sick", c.isSick());
        o.addProperty("attacking", c.isAttacking());
        o.addProperty("blocking", c.isBlocking());
        o.addProperty("faceDown", c.isFaceDown());
        o.addProperty("flipped", c.isFlipped());
        o.addProperty("phasedOut", c.isPhasedOut());
        o.addProperty("token", c.isToken());
        o.addProperty("commander", c.isCommander());
        o.addProperty("damage", c.getDamage());
        o.addProperty("assignedDamage", c.getAssignedDamage());
        o.addProperty("shields", c.getShieldCount());
        o.add("counters", counters(c.getCounters()));
        o.add("attachedTo", refOrNull(c.getAttachedTo()));
        o.add("attachedCards", ids(c.getAttachedCards()));
        if (c.getChosenType() != null && !c.getChosenType().isEmpty()) o.addProperty("chosenType", c.getChosenType());
        if (c.getChosenColors() != null && !c.getChosenColors().isEmpty()) o.add("chosenColors", strings(c.getChosenColors()));
        if (c.getNamedCard() != null && !c.getNamedCard().isEmpty()) o.add("namedCard", strings(c.getNamedCard()));
        if (c.getText() != null && !c.getText().isEmpty()) o.addProperty("text", c.getText());

        o.add("state", state(c.getCurrentState()));
        if (c.hasAlternateState()) o.add("altState", state(c.getAlternateState()));
        return o;
    }

    public JsonElement state(final CardStateView s) {
        if (s == null) return JsonNull.INSTANCE;
        final JsonObject o = new JsonObject();
        o.addProperty("name", s.getName());
        o.addProperty("stateName", s.getState() == null ? null : s.getState().name());
        o.addProperty("type", s.getType() == null ? null : s.getType().toString());
        o.addProperty("manaCost", manaCost(s.getManaCost()));
        o.addProperty("colors", colors(s.getColors()));
        o.addProperty("oracleText", s.getOracleText());
        o.addProperty("rulesText", s.getRulesText());
        o.addProperty("setCode", s.getSetCode());
        o.addProperty("imageKey", s.getImageKey());
        // printed P/T of permanents that aren't creatures right now (Theros Gods below devotion,
        // uncrewed Vehicles, Spacecraft) too: the client shows it greyed so players can still read it
        if (s.isCreature() || s.isVehicle() || s.getPower() != 0 || s.getToughness() != 0) {
            o.addProperty("power", s.getPower());
            o.addProperty("toughness", s.getToughness());
        }
        if (s.isPlaneswalker()) o.addProperty("loyalty", s.getLoyalty());
        if (s.isBattle()) o.addProperty("defense", s.getDefense());
        o.addProperty("isLand", s.isLand());
        o.addProperty("isCreature", s.isCreature());
        o.addProperty("isArtifact", s.isArtifact());
        o.addProperty("isEnchantment", s.isEnchantment());
        o.addProperty("isPlaneswalker", s.isPlaneswalker());
        o.addProperty("isInstant", s.isInstant());
        o.addProperty("isSorcery", s.isSorcery());
        if (s.getKeywords() != null) {
            final JsonArray kws = new JsonArray();
            for (final KeywordView k : s.getKeywords()) kws.add(k.original());
            o.add("keywords", kws);
        }
        return o;
    }

    // ---- stack / combat / abilities -----------------------------------------------------

    public JsonObject stackItem(final StackItemView si) {
        final JsonObject o = new JsonObject();
        o.addProperty("id", si.getId());
        o.addProperty("key", si.getKey());
        o.addProperty("text", si.getText());
        o.addProperty("isAbility", si.isAbility());
        o.addProperty("isTrigger", si.isTrigger());
        o.addProperty("isOptionalTrigger", si.isOptionalTrigger());
        o.add("source", card(si.getSourceCard()));
        o.add("activator", refOrNull(si.getActivatingPlayer()));
        o.add("targetCards", ids(si.getTargetCards()));
        o.add("targetPlayers", ids(si.getTargetPlayers()));
        if (si.getSubInstance() != null) o.add("sub", stackItem(si.getSubInstance()));
        return o;
    }

    public JsonElement combat(final CombatView cv) {
        if (cv == null) return JsonNull.INSTANCE;
        final JsonObject o = new JsonObject();
        final JsonArray attackers = new JsonArray();
        for (final CardView a : cv.getAttackers()) {
            final JsonObject ao = new JsonObject();
            ao.addProperty("attacker", a.getId());
            ao.add("defender", entityRef(cv.getDefender(a)));
            ao.add("blockers", ids(cv.getBlockers(a)));
            ao.add("plannedBlockers", ids(cv.getPlannedBlockers(a)));
            attackers.add(ao);
        }
        o.add("attackers", attackers);
        return o;
    }

    public JsonObject ability(final SpellAbilityView sa) {
        final JsonObject o = new JsonObject();
        o.addProperty("id", sa.getId());
        o.addProperty("description", sa.getDescription());
        o.addProperty("isSpell", sa.isSpell());
        o.addProperty("canPlay", sa.canPlay());
        o.add("hostCard", refOrNull(sa.getHostCard()));
        return o;
    }

    /** Minimal reference to any game entity: {"type":"card"|"player","id":n}. */
    public JsonElement entityRef(final GameEntityView e) {
        if (e == null) return JsonNull.INSTANCE;
        final JsonObject o = new JsonObject();
        o.addProperty("type", e instanceof CardView ? "card" : e instanceof PlayerView ? "player" : e.getClass().getSimpleName());
        o.addProperty("id", e.getId());
        o.addProperty("name", e instanceof CardView ? (mayView((CardView) e) ? e.getName() : "(hidden)") : e.getName());
        return o;
    }

    // ---- helpers ---------------------------------------------------------------------------

    public static JsonElement refOrNull(final GameEntityView e) {
        return e == null ? JsonNull.INSTANCE : new com.google.gson.JsonPrimitive(e.getId());
    }

    public static JsonArray ids(final Iterable<? extends GameEntityView> es) {
        final JsonArray a = new JsonArray();
        if (es != null) for (final GameEntityView e : es) a.add(e.getId());
        return a;
    }

    public static JsonArray strings(final Collection<String> ss) {
        final JsonArray a = new JsonArray();
        if (ss != null) for (final String s : ss) a.add(s);
        return a;
    }

    public static JsonObject counters(final Multiset<CounterType> counters) {
        final JsonObject o = new JsonObject();
        if (counters != null) for (final Multiset.Entry<CounterType> e : counters.entrySet()) o.addProperty(e.getElement().getName(), e.getCount());
        return o;
    }

    public static String manaCost(final ManaCost mc) {
        return mc == null || mc.isNoCost() ? null : mc.toString();
    }

    public static String colors(final ColorSet cs) {
        if (cs == null) return "";
        final StringBuilder sb = new StringBuilder();
        if (cs.hasWhite()) sb.append('W');
        if (cs.hasBlue()) sb.append('U');
        if (cs.hasBlack()) sb.append('B');
        if (cs.hasRed()) sb.append('R');
        if (cs.hasGreen()) sb.append('G');
        return sb.toString();
    }
}
