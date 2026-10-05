package arena.engine;

import forge.gamemodes.match.HostedMatch;
import forge.gui.download.GuiDownloadService;
import forge.gui.interfaces.IGuiBase;
import forge.gui.interfaces.IGuiGame;
import forge.item.PaperCard;
import forge.localinstance.skin.FSkinProp;
import forge.localinstance.skin.ISkinImage;
import forge.sound.IAudioClip;
import forge.sound.IAudioMusic;
import forge.util.FSerializableFunction;
import forge.util.ImageFetcher;
import org.jupnp.UpnpServiceConfiguration;

import java.io.File;
import java.util.ArrayList;
import java.util.Collection;
import java.util.List;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.Future;
import java.util.function.Consumer;
import java.util.function.Supplier;

/**
 * Headless {@link IGuiBase}: no images, no sounds, no dialogs. Supplies the two things Forge
 * needs from a "GUI" to run the engine: the assets directory (res/ with card scripts) and an
 * "EDT" — a single thread on which the input/prompt machinery expects GUI work to happen.
 *
 * Dialog-ish methods here are only reached from GUI-level code paths we never trigger; they
 * answer with sensible defaults so nothing can block the process.
 */
public final class HeadlessGuiBase implements IGuiBase {
    private final String assetsDir;
    private final Supplier<IGuiGame> guiGameFactory;
    private final ExecutorService edt;
    private volatile Thread edtThread;

    public HeadlessGuiBase(final String assetsDir, final Supplier<IGuiGame> guiGameFactory) {
        this.assetsDir = assetsDir.endsWith(File.separator) ? assetsDir : assetsDir + File.separator;
        this.guiGameFactory = guiGameFactory;
        this.edt = Executors.newSingleThreadExecutor(r -> {
            final Thread t = new Thread(r, "Headless-EDT");
            t.setDaemon(true);
            edtThread = t;
            return t;
        });
        // materialize the thread so isGuiThread() works before the first submit
        invokeInEdtAndWait(() -> {});
    }

    // ---- threading -----------------------------------------------------------------------

    @Override public boolean isGuiThread() { return Thread.currentThread() == edtThread; }

    @Override public void invokeInEdtNow(final Runnable runnable) {
        if (isGuiThread()) { runnable.run(); } else { invokeInEdtAndWait(runnable); }
    }

    @Override public void invokeInEdtLater(final Runnable runnable) { edt.submit(guard(runnable)); }

    @Override public void invokeInEdtAndWait(final Runnable proc) {
        if (isGuiThread()) { proc.run(); return; }
        final Future<?> f = edt.submit(guard(proc));
        try { f.get(); }
        catch (final InterruptedException e) { Thread.currentThread().interrupt(); }
        catch (final Exception e) { throw new RuntimeException(e); }
    }

    @Override public void runBackgroundTask(final String message, final Runnable task) {
        final Thread t = new Thread(guard(task), "Headless-BG");
        t.setDaemon(true);
        t.start();
    }

    private static Runnable guard(final Runnable r) {
        return () -> {
            try { r.run(); }
            catch (final Throwable t) { System.err.println("[HeadlessGuiBase] uncaught in EDT task: " + t); t.printStackTrace(); }
        };
    }

    // ---- environment ---------------------------------------------------------------------

    @Override public boolean isRunningOnDesktop() { return true; }
    @Override public boolean isLibgdxPort() { return false; }
    @Override public String getCurrentVersion() { return "arena-headless"; }
    @Override public String getAssetsDir() { return assetsDir; }
    @Override public IGuiGame getNewGuiGame() { return guiGameFactory.get(); }
    @Override public HostedMatch hostMatch() { return new HostedMatch(); }
    @Override public boolean hasNetGame() { return false; }
    @Override public UpnpServiceConfiguration getUpnpPlatformService() { return null; }
    @Override public float getScreenScale() { return 1f; }
    @Override public int getAvatarCount() { return 0; }
    @Override public int getSleevesCount() { return 0; }
    @Override public void preventSystemSleep(final boolean preventSleep) { }
    @Override public void copyToClipboard(final String text) { }
    @Override public void browseToUrl(final String url) { }
    @Override public void clearImageCache() { }

    /** Card text uses {W}{U}... mana symbols; we ship them through unchanged and let the frontend render them. */
    @Override public String encodeSymbols(final String str, final boolean formatReminderText) { return str; }

    // ---- images / sound (none) -----------------------------------------------------------

    @Override public ImageFetcher getImageFetcher() { return null; }
    @Override public ISkinImage getSkinIcon(final FSkinProp skinProp) { return null; }
    @Override public ISkinImage getUnskinnedIcon(final String path) { return null; }
    @Override public ISkinImage getCardArt(final PaperCard card, final boolean backFace) { return null; }
    @Override public ISkinImage createLayeredImage(final PaperCard card, final FSkinProp background, final String overlayFilename, final float opacity) { return null; }
    @Override public void download(final GuiDownloadService service, final Consumer<Boolean> callback) { if (callback != null) callback.accept(false); }
    @Override public boolean isSupportedAudioFormat(final File file) { return false; }
    @Override public IAudioClip createAudioClip(final String filename) { return null; }
    @Override public IAudioMusic createAudioMusic(final String filename) { return null; }
    @Override public void startAltSoundSystem(final String filename, final boolean isSynchronized) { }

    // ---- app-level dialogs (never shown headless; answer defaults) -----------------------

    @Override public void showCardList(final String title, final String message, final List<PaperCard> list) { }
    @Override public boolean showBoxedProduct(final String title, final String message, final List<PaperCard> list) { return false; }
    @Override public void showBugReportDialog(final String title, final String text, final boolean showExitAppBtn) {
        System.err.println("[Forge bug report] " + title + "\n" + text);
    }
    @Override public void showImageDialog(final ISkinImage image, final String message, final String title) { }
    @Override public int showOptionDialog(final String message, final String title, final FSkinProp icon, final List<String> options, final int defaultOption) { return defaultOption; }
    @Override public String showInputDialog(final String message, final String title, final FSkinProp icon, final String initialInput, final List<String> inputOptions, final boolean isNumeric) { return initialInput; }
    @Override public String showFileDialog(final String title, final String defaultDir) { return null; }
    @Override public File getSaveFile(final File defaultFile) { return defaultFile; }
    @Override public <T> List<T> order(final String title, final String top, final int remainingObjectsMin, final int remainingObjectsMax, final List<T> sourceChoices, final List<T> destChoices) {
        return new ArrayList<>(sourceChoices);
    }
    @Override public <T> List<T> getChoices(final String message, final int min, final int max, final Collection<T> choices, final Collection<T> selected, final FSerializableFunction<T, String> display) {
        final List<T> out = new ArrayList<>();
        for (final T c : choices) { if (out.size() >= Math.max(min, Math.min(1, max))) break; out.add(c); }
        return out;
    }
    @Override public PaperCard chooseCard(final String title, final String message, final List<PaperCard> list) { return list.isEmpty() ? null : list.get(0); }
    @Override public void showSpellShop() { }
    @Override public void showBazaar() { }
}
