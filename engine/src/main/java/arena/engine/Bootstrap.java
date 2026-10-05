package arena.engine;

import forge.gui.GuiBase;
import forge.gui.interfaces.IGuiGame;
import forge.model.FModel;

import java.io.File;
import java.util.function.Supplier;

/**
 * One-time engine initialization: install the headless GUI shim, then load Forge's card database,
 * editions and preferences via {@link FModel#initialize}. Takes a few seconds; call once per JVM.
 */
public final class Bootstrap {
    private static volatile boolean initialized;

    private Bootstrap() {}

    /** Resolve the Forge assets dir (the folder containing {@code res/}) from -Dforge.assets, $FORGE_ASSETS, or ./forge/forge-gui. */
    public static String resolveAssetsDir() {
        String dir = System.getProperty("forge.assets");
        if (dir == null || dir.isBlank()) dir = System.getenv("FORGE_ASSETS");
        if (dir == null || dir.isBlank()) {
            // walk up from cwd looking for forge/forge-gui/res
            File cur = new File(".").getAbsoluteFile();
            for (int i = 0; i < 4 && cur != null; i++, cur = cur.getParentFile()) {
                final File cand = new File(cur, "forge/forge-gui");
                if (new File(cand, "res/cardsfolder").isDirectory()) { dir = cand.getPath(); break; }
            }
        }
        if (dir == null || !new File(dir, "res").isDirectory()) {
            throw new IllegalStateException("Cannot find Forge assets dir (expected <dir>/res/). Set -Dforge.assets=/path/to/forge/forge-gui or run tools/setup-forge.sh");
        }
        return dir;
    }

    public static synchronized void init(final Supplier<IGuiGame> guiGameFactory) {
        if (initialized) return;
        final String assets = resolveAssetsDir();
        final long t0 = System.currentTimeMillis();
        System.out.println("[Bootstrap] Forge assets: " + assets);
        GuiBase.setInterface(new HeadlessGuiBase(assets, guiGameFactory));
        FModel.initialize(null, null);
        System.out.printf("[Bootstrap] FModel initialized in %d ms; %d cards in DB%n",
                System.currentTimeMillis() - t0, FModel.getMagicDb().getCommonCards().getAllCards().size());
        initialized = true;
    }
}
