//! Start the Java engine as a child process when nothing answers `/api/health`.

use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, anyhow, bail};

use crate::rest::Rest;

#[derive(Debug, Clone)]
pub struct LaunchConfig {
    /// e.g. `http://127.0.0.1:8080`
    pub base_url: String,
    pub port: u16,
    /// `engine/target/forge-ws-server.jar`
    pub jar: PathBuf,
    /// Forge assets dir (`forge/forge-gui`); None = let the engine search upward from cwd.
    pub assets: Option<PathBuf>,
    /// Java binary; None = the bundled `jre/`, then `$JAVA_HOME/bin/java`, then
    /// `~/.local/opt/jdk-17*/bin/java`, then `java`.
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

/// `~/.local/share/mtg-duelstone/engine.log` (platform data dir).
pub fn log_path() -> PathBuf {
    directories::ProjectDirs::from("", "", "mtg-duelstone")
        .map(|d| d.data_dir().to_path_buf())
        .unwrap_or_else(std::env::temp_dir)
        .join("engine.log")
}

/// Where a packaged build keeps its resources (`assets/`, `engine/`, `forge/`, `decks/`, `jre/`):
/// next to the executable, in a macOS bundle's `Contents/Resources`, or in `../share/mtg-duelstone`
/// (AppImage and `/usr` installs). Packaging scripts lay files out to match; see `packaging/`.
pub fn bundle_dirs() -> Vec<PathBuf> {
    let exe = std::env::current_exe().and_then(|e| e.canonicalize());
    let Some(exe_dir) = exe.ok().and_then(|e| e.parent().map(Path::to_path_buf)) else {
        return Vec::new();
    };
    vec![exe_dir.join("../Resources"), exe_dir.join("../share/mtg-duelstone"), exe_dir]
}

/// `rel` inside the first [`bundle_dirs`] entry that has it.
pub fn find_bundled(rel: &str) -> Option<PathBuf> {
    bundle_dirs().into_iter().map(|d| d.join(rel)).find(|p| p.exists())
}

/// Locate a Java binary following the documented search order.
pub fn find_java(cfg: &LaunchConfig) -> PathBuf {
    if let Some(j) = &cfg.java {
        return j.clone();
    }
    // A packaged build ships its own runtime; prefer it over whatever Java the user has.
    if let Some(j) = find_bundled("jre/bin/java") {
        return j;
    }
    if let Some(h) = std::env::var_os("JAVA_HOME") {
        let p = Path::new(&h).join("bin/java");
        if p.is_file() {
            return p;
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        let opt = Path::new(&home).join(".local/opt");
        if let Ok(rd) = std::fs::read_dir(&opt) {
            let mut cands: Vec<PathBuf> = rd
                .flatten()
                .filter(|e| e.file_name().to_string_lossy().starts_with("jdk-17"))
                .map(|e| e.path().join("bin/java"))
                .filter(|p| p.is_file())
                .collect();
            cands.sort();
            if let Some(p) = cands.pop() {
                return p;
            }
        }
    }
    PathBuf::from("java")
}

/// Resolve a jar path: absolute as-is; relative against cwd and then each parent directory.
pub fn find_jar(jar: &Path) -> Option<PathBuf> {
    if jar.is_absolute() {
        return jar.is_file().then(|| jar.to_path_buf());
    }
    let mut dir = std::env::current_dir().ok();
    while let Some(d) = dir {
        let p = d.join(jar);
        if p.is_file() {
            return Some(p);
        }
        dir = d.parent().map(Path::to_path_buf);
    }
    None
}

/// Blocks until healthy or timeout. `progress` gets human-readable status lines for a splash screen.
pub fn ensure_engine(cfg: &LaunchConfig, progress: &mut dyn FnMut(&str)) -> anyhow::Result<Option<EngineProcess>> {
    let rest = Rest::new(cfg.base_url.clone());
    if rest.health().unwrap_or(false) {
        return Ok(None);
    }
    let java = find_java(cfg);
    let jar = find_jar(&cfg.jar).ok_or_else(|| anyhow!("engine jar not found: {} (build it with `cd engine && mvn -q -B package`)", cfg.jar.display()))?;
    progress("Starting engine…");

    let log = log_path();
    if let Some(dir) = log.parent() {
        std::fs::create_dir_all(dir).ok();
    }
    let out = OpenOptions::new().create(true).write(true).truncate(true).open(&log).with_context(|| format!("open {}", log.display()))?;
    let err = out.try_clone()?;
    let mut cmd = Command::new(&java);
    // Headless: the engine never opens a window, and on macOS touching AWT would otherwise put a
    // second "java" icon in the Dock.
    cmd.arg("-Djava.awt.headless=true").arg("-jar").arg(&jar).arg("--port").arg(cfg.port.to_string());
    if let Some(a) = &cfg.assets {
        let a = find_jar_dir(a);
        cmd.arg("--assets").arg(a);
    }
    cmd.stdin(Stdio::null()).stdout(Stdio::from(out)).stderr(Stdio::from(err));
    // If we die without running destructors (Ctrl+C, crash, process::exit), take the engine with
    // us instead of leaving an orphaned JVM holding the port.
    #[cfg(target_os = "linux")]
    unsafe {
        use std::os::unix::process::CommandExt;
        cmd.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
            Ok(())
        });
    }
    let child = spawn_from_long_lived_thread(cmd).with_context(|| format!("spawn {} -jar {}", java.display(), jar.display()))?;
    let mut proc = EngineProcess { child };

    let t0 = Instant::now();
    let timeout = Duration::from_secs(cfg.health_timeout_secs.max(1));
    let mut last_msg = "Starting engine…";
    loop {
        if let Some(status) = proc.child.try_wait()? {
            bail!("engine exited early ({status}); see {}", log.display());
        }
        if rest.health().unwrap_or(false) {
            progress("Engine ready");
            return Ok(Some(proc));
        }
        if t0.elapsed() > timeout {
            bail!("engine did not become healthy within {}s; see {}", timeout.as_secs(), log.display());
        }
        let msg = if t0.elapsed() < Duration::from_secs(1) { "Starting engine…" } else { "Loading card database…" };
        if msg != last_msg {
            progress(msg);
            last_msg = msg;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

/// `PR_SET_PDEATHSIG` fires when the *thread* that forked the child exits, not the process, so
/// children must be spawned from a thread that lives as long as the process — not from the
/// short-lived boot thread that calls [`ensure_engine`].
fn spawn_from_long_lived_thread(cmd: Command) -> std::io::Result<std::process::Child> {
    use std::sync::{Mutex, OnceLock, mpsc};
    type Req = (Command, mpsc::Sender<std::io::Result<std::process::Child>>);
    static SPAWNER: OnceLock<Mutex<mpsc::Sender<Req>>> = OnceLock::new();
    let tx = SPAWNER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Req>();
        std::thread::Builder::new()
            .name("engine-spawner".into())
            .spawn(move || {
                for (mut cmd, reply) in rx {
                    let _ = reply.send(cmd.spawn());
                }
                // Sender lives in a static, so this thread parks in `recv` for the process lifetime.
            })
            .expect("spawn engine-spawner thread");
        Mutex::new(tx)
    });
    let (reply_tx, reply_rx) = mpsc::channel();
    tx.lock().expect("spawner lock").send((cmd, reply_tx)).map_err(std::io::Error::other)?;
    reply_rx.recv().map_err(std::io::Error::other)?
}

/// Like [`find_jar`] but for directories (the Forge assets dir).
fn find_jar_dir(p: &Path) -> PathBuf {
    if p.is_absolute() {
        return p.to_path_buf();
    }
    let mut dir = std::env::current_dir().ok();
    while let Some(d) = dir {
        let c = d.join(p);
        if c.is_dir() {
            return c;
        }
        dir = d.parent().map(Path::to_path_buf);
    }
    p.to_path_buf()
}
