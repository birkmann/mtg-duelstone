//! Start the Java engine as a child process when nothing answers `/api/health`.

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct LaunchConfig {
    /// e.g. `http://127.0.0.1:8080`
    pub base_url: String,
    pub port: u16,
    /// `engine/target/forge-ws-server.jar`
    pub jar: PathBuf,
    /// Forge assets dir (`forge/forge-gui`); None = let the engine search upward from cwd.
    pub assets: Option<PathBuf>,
    /// Java binary; None = `$JAVA_HOME/bin/java`, then `~/.local/opt/jdk-17*/bin/java`, then `java`.
    pub java: Option<PathBuf>,
    pub health_timeout_secs: u64,
}

/// Child engine process; killed on drop. `None` from `ensure_engine` = an engine was already running.
pub struct EngineProcess {
    pub child: std::process::Child,
}

impl Drop for EngineProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Blocks until healthy or timeout. `progress` gets human-readable status lines for a splash screen.
pub fn ensure_engine(_cfg: &LaunchConfig, _progress: &mut dyn FnMut(&str)) -> anyhow::Result<Option<EngineProcess>> {
    todo!("implemented by the mtg_client work package")
}
