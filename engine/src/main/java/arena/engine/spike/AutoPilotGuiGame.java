package arena.engine.spike;

import arena.engine.view.ViewJson;
import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import forge.LobbyPlayer;
import forge.deck.CardPool;
import forge.game.GameEntityView;
import forge.game.GameState;
import forge.game.card.CardView;
import forge.game.phase.PhaseType;
import forge.game.player.DelayedReveal;
import forge.game.player.IHasIcon;
import forge.game.player.PlayerView;
import forge.game.spellability.SpellAbilityView;
import forge.game.zone.ZoneType;
import forge.gamemodes.match.AbstractGuiGame;
import forge.gui.FThreads;
import forge.interfaces.IGameController;
import forge.item.PaperCard;
import forge.localinstance.skin.FSkinProp;
import forge.player.PlayerZoneUpdate;
import forge.player.PlayerZoneUpdates;
import forge.trackable.TrackableCollection;
import forge.util.FSerializableFunction;
import forge.util.ITriggerEvent;

import java.util.ArrayList;
import java.util.Collection;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.TreeMap;
import java.util.concurrent.atomic.AtomicBoolean;

/**
 * Phase-0 spike GUI: a headless {@link forge.gui.interfaces.IGuiGame} that answers every prompt the
 * engine (via {@link forge.player.PlayerControllerHuman}) throws at it with "first option / OK / pass",
 * logging each prompt kind so we can size the real protocol surface. It also proves the bootstrap,
 * threading model and JSON view serialization end to end.
 *
 * Two kinds of prompt reach a GUI:
 * <ul>
 *   <li><b>Blocking calls</b> (getChoices, confirm, order, ...): the game thread calls us and waits
 *       for the return value. We answer immediately.</li>
 *   <li><b>Input prompts</b> (pass priority, select cards, attack, block, pay mana...): the game thread
 *       parks on a latch after pushing an Input that paints itself through showPromptMessage /
 *       updateButtons / setSelectables; the GUI later "clicks" through the {@link IGameController}.
 *       We emulate a click on the EDT as soon as the buttons are painted.</li>
 * </ul>
 */
public class AutoPilotGuiGame extends AbstractGuiGame {
    private static final Gson GSON = new GsonBuilder().create();

    private final Map<String, Integer> promptHistogram = new TreeMap<>();
    private final Set<CardView> selectablesCopy = new LinkedHashSet<>();
    private final AtomicBoolean clickScheduled = new AtomicBoolean(false);
    private final boolean verbose;
    private volatile String lastPrompt = "";
    private volatile String[] lastButtons = { "", "", "false", "false" };
    private int clicks = 0;

    public AutoPilotGuiGame(final boolean verbose) { this.verbose = verbose; }

    public Map<String, Integer> getPromptHistogram() { return promptHistogram; }
    public int getClickCount() { return clicks; }

    private void count(final String kind, final String detail) {
        promptHistogram.merge(kind, 1, Integer::sum);
        if (verbose) System.out.println("  [prompt] " + kind + (detail == null ? "" : " :: " + detail));
    }

    private static String trunc(final Object o) {
        final String s = String.valueOf(o).replace('\n', ' ');
        return s.length() > 90 ? s.substring(0, 90) + "…" : s;
    }

    // ---- lifecycle / state pushes ---------------------------------------------------------

    @Override protected void updateCurrentPlayer(final PlayerView player) { }

    @Override public void openView(final TrackableCollection<PlayerView> myPlayers) {
        System.out.println("[gui] openView for " + myPlayers);
    }

    @Override public void updateTurn(final PlayerView player) {
        System.out.printf("[gui] ---- Turn %d (%s) ----%n", getGameView().getTurn(), player == null ? "?" : player.getLobbyPlayerName());
    }

    @Override public void updatePhase(final boolean saveState) {
        if (verbose) System.out.println("[gui] phase " + getGameView().getPhase());
    }

    @Override public void finishGame() {
        System.out.println("[gui] finishGame; winner=" + getGameView().getWinningPlayerName());
    }

    @Override public void afterGameEnd() { super.afterGameEnd(); }
    @Override public void showCombat() { }
    @Override public void flashIncorrectAction() { count("flashIncorrectAction", null); }
    @Override public void alertUser() { }
    @Override public void enableOverlay() { }
    @Override public void disableOverlay() { }
    @Override public void showManaPool(final PlayerView player) { }
    @Override public void hideManaPool(final PlayerView player) { }
    @Override public void updateShards(final Iterable<PlayerView> shardsUpdate) { }
    @Override public void setPanelSelection(final CardView hostCard) { }
    @Override public void setCard(final CardView card) { }
    @Override public void setPlayerAvatar(final LobbyPlayer player, final IHasIcon ihi) { }
    @Override public GameState getGamestate() { return null; }
    @Override public void message(final String message, final String title) { count("message", title + ": " + trunc(message)); }
    @Override public void showErrorDialog(final String message, final String title) { count("showErrorDialog", title + ": " + trunc(message)); }
    @Override public boolean isUiSetToSkipPhase(final PlayerView playerTurn, final PhaseType phase) { return false; }

    @Override public Iterable<PlayerZoneUpdate> tempShowZones(final PlayerView controller, final Iterable<PlayerZoneUpdate> zonesToUpdate) { return zonesToUpdate; }
    @Override public void hideZones(final PlayerView controller, final Iterable<PlayerZoneUpdate> zonesToUpdate) { }
    @Override public PlayerZoneUpdates openZones(final PlayerView controller, final Collection<ZoneType> zones, final Map<PlayerView, Object> players, final boolean backupLastZones) { return new PlayerZoneUpdates(); }
    @Override public void restoreOldZones(final PlayerView playerView, final PlayerZoneUpdates playerZoneUpdates) { }

    // ---- Input prompt painting → emulated click -----------------------------------------

    @Override public void showPromptMessage(final PlayerView playerView, final String message, final CardView card) {
        lastPrompt = message;
        if (verbose) System.out.println("  [input] " + trunc(message) + (card == null ? "" : " (card " + card + ")"));
    }

    @Override public void setSelectables(final Iterable<CardView> cards, final int min, final int max) {
        super.setSelectables(cards, min, max);
        for (final CardView c : cards) selectablesCopy.add(c);
    }

    @Override public void clearSelectables() {
        super.clearSelectables();
        selectablesCopy.clear();
    }

    @Override public void updateButtons(final PlayerView owner, final String label1, final String label2, final boolean enable1, final boolean enable2, final boolean focus1) {
        lastButtons = new String[] { label1, label2, String.valueOf(enable1), String.valueOf(enable2) };
        if (!enable1 && !enable2 && selectablesCopy.isEmpty()) return; // nothing clickable (yet)
        if (clickScheduled.compareAndSet(false, true)) {
            // run after the current showMessage() finishes painting, with the final button state
            FThreads.invokeInEdtLater(() -> autoClick(owner));
        }
    }

    private void autoClick(final PlayerView owner) {
        clickScheduled.set(false);
        final IGameController ctl = getGameController(owner);
        if (ctl == null) return;
        final String l1 = lastButtons[0], l2 = lastButtons[1];
        final boolean e1 = Boolean.parseBoolean(lastButtons[2]), e2 = Boolean.parseBoolean(lastButtons[3]);
        clicks++;
        if (e1) {
            count("input:" + l1, trunc(lastPrompt));
            ctl.selectButtonOk();
        } else if (!selectablesCopy.isEmpty()) {
            final CardView first = selectablesCopy.iterator().next();
            count("input:selectCard", trunc(lastPrompt) + " -> " + first);
            ctl.selectCard(first, null, null);
        } else if (e2) {
            count("input:" + l2, trunc(lastPrompt));
            ctl.selectButtonCancel();
        }
    }

    // ---- blocking dialogs: answer first/default -----------------------------------------

    @Override public SpellAbilityView getAbilityToPlay(final CardView hostCard, final List<SpellAbilityView> abilities, final ITriggerEvent triggerEvent) {
        count("getAbilityToPlay", hostCard + " x" + abilities.size());
        return abilities.isEmpty() ? null : abilities.get(0);
    }

    @Override public Map<CardView, Integer> assignCombatDamage(final CardView attacker, final List<CardView> blockers, final int damage, final GameEntityView defender, final boolean overrideOrder, final boolean maySkip) {
        count("assignCombatDamage", attacker + " dmg=" + damage);
        final Map<CardView, Integer> m = new LinkedHashMap<>();
        if (!blockers.isEmpty()) m.put(blockers.get(0), damage);
        return m;
    }

    @Override public Map<Object, Integer> assignGenericAmount(final CardView effectSource, final Map<Object, Integer> target, final int amount, final boolean atLeastOne, final String amountLabel) {
        count("assignGenericAmount", effectSource + " " + amountLabel + "=" + amount);
        final Map<Object, Integer> m = new LinkedHashMap<>();
        if (!target.isEmpty()) m.put(target.keySet().iterator().next(), amount);
        return m;
    }

    @Override public boolean showConfirmDialog(final String message, final String title, final String yesButtonText, final String noButtonText, final boolean defaultYes) {
        count("showConfirmDialog", trunc(message));
        return defaultYes;
    }

    @Override public int showOptionDialog(final String message, final String title, final FSkinProp icon, final List<String> options, final int defaultOption) {
        count("showOptionDialog", trunc(message) + " " + options);
        return defaultOption;
    }

    @Override public String showInputDialog(final String message, final String title, final FSkinProp icon, final String initialInput, final List<String> inputOptions, final boolean isNumeric) {
        count("showInputDialog", trunc(message));
        return initialInput != null ? initialInput : (inputOptions != null && !inputOptions.isEmpty() ? inputOptions.get(0) : (isNumeric ? "0" : ""));
    }

    @Override public boolean confirm(final CardView c, final String question, final boolean defaultIsYes, final List<String> options) {
        count("confirm", trunc(question));
        return defaultIsYes;
    }

    @Override public <T> List<T> getChoices(final String message, final int min, final int max, final List<T> choices, final List<T> selected, final FSerializableFunction<T, String> display) {
        count("getChoices", trunc(message) + " [" + min + ".." + max + "] of " + choices.size());
        final List<T> out = new ArrayList<>();
        final int want = Math.max(min, Math.min(1, max)); // at least min, else exactly one
        for (final T c : choices) { if (out.size() >= want) break; out.add(c); }
        return out;
    }

    @Override public <T> OrderResult<T> order(final String title, final String top, final int remainingObjectsMin, final int remainingObjectsMax, final List<T> sourceChoices, final List<T> destChoices, final CardView referenceCard, final boolean sideboardingMode, final boolean showRememberCheckbox) {
        count("order", trunc(title) + " src=" + sourceChoices.size() + " remain=[" + remainingObjectsMin + ".." + remainingObjectsMax + "]");
        // keep as much as allowed in the source; move the rest (in order) to dest
        final List<T> dest = destChoices == null ? new ArrayList<>() : new ArrayList<>(destChoices);
        final List<T> src = new ArrayList<>(sourceChoices);
        while (src.size() > Math.max(0, remainingObjectsMax) && !src.isEmpty()) dest.add(src.remove(0));
        while (src.size() < remainingObjectsMin && !dest.isEmpty()) src.add(dest.remove(dest.size() - 1));
        return new OrderResult<>(dest, false);
    }

    @Override public List<PaperCard> sideboard(final CardPool sideboard, final CardPool main, final String message) {
        count("sideboard", message);
        return null; // keep deck as-is
    }

    @Override public GameEntityView chooseSingleEntityForEffect(final String title, final List<? extends GameEntityView> optionList, final DelayedReveal delayedReveal, final boolean isOptional) {
        count("chooseSingleEntityForEffect", trunc(title) + " of " + optionList.size() + (isOptional ? " (optional)" : ""));
        return optionList.isEmpty() ? null : optionList.get(0);
    }

    @Override public List<GameEntityView> chooseEntitiesForEffect(final String title, final List<? extends GameEntityView> optionList, final int min, final int max, final DelayedReveal delayedReveal) {
        count("chooseEntitiesForEffect", trunc(title) + " [" + min + ".." + max + "] of " + optionList.size());
        final List<GameEntityView> out = new ArrayList<>();
        for (final GameEntityView e : optionList) { if (out.size() >= Math.max(min, 0)) break; out.add(e); }
        return out;
    }

    @Override public List<CardView> manipulateCardList(final String title, final Iterable<CardView> cards, final Iterable<CardView> manipulable, final boolean toTop, final boolean toBottom, final boolean toAnywhere) {
        count("manipulateCardList", trunc(title));
        final List<CardView> out = new ArrayList<>();
        for (final CardView c : cards) out.add(c);
        return out;
    }

    // ---- debugging aid ---------------------------------------------------------------------

    public String stateJson(final PlayerView viewer, final boolean pretty) {
        final com.google.gson.JsonObject o = new ViewJson(viewer).game(getGameView());
        return pretty ? new GsonBuilder().setPrettyPrinting().create().toJson(o) : GSON.toJson(o);
    }
}
