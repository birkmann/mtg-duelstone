//! Locates the offline pack (`offline/`): every card image, the rulings and the showcase data.
//!
//! Why a pack instead of a cache: the game must run with no internet and without Scryfall, so
//! nothing is fetched lazily at runtime. `tools/fetch-offline.sh` (the `fetch-offline` bin) fills
//! the pack once while online; the engine serves `/api/img` + `/api/rulings` from it and the
//! splash showcase reads it directly. Images are gitignored (Wizards' art is not redistributed);
//! the small JSON under `offline/data/` is committed.

use std::path::{Path, PathBuf};

use crate::launcher::bundle_dirs;

/// Marker that a directory is a pack: the committed data folder.
const MARKER: &str = "data";

/// `$MTG_DUELSTONE_OFFLINE`, then `offline/` next to a packaged build, then the platform data
/// dir (`~/.local/share/mtg-duelstone/offline`, `AppData\Local\…` on Windows, where a release user's
/// `fetch-offline` writes), then `offline/` in the cwd or any parent (a source checkout).
pub fn dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("MTG_DUELSTONE_OFFLINE").map(PathBuf::from) {
        return Some(d);
    }
    let mut cands: Vec<PathBuf> = bundle_dirs().into_iter().map(|d| d.join("offline")).collect();
    cands.push(user_dir());
    cands.extend(ancestors_of(std::env::current_dir().ok()));
    cands.extend(ancestors_of(Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")))));
    // Prefer a pack that has images; fall back to one with just the committed data.
    let is_pack = |p: &Path| p.join(MARKER).is_dir();
    cands.iter().find(|p| is_pack(p) && has_images(p)).or_else(|| cands.iter().find(|p| is_pack(p))).cloned()
}

/// `~/.local/share/mtg-duelstone/offline` (platform data dir): the default target of `fetch-offline`
/// outside a source checkout. The local data dir: on Windows the default one roams, and 9 GB of
/// images must not follow a user between machines.
pub fn user_dir() -> PathBuf {
    directories::ProjectDirs::from("", "", "mtg-duelstone")
        .map(|d| d.data_local_dir().to_path_buf())
        .unwrap_or_else(std::env::temp_dir)
        .join("offline")
}

/// Whether `fetch-offline` has finished writing the image index into this pack.
pub fn has_images(pack: &Path) -> bool {
    pack.join("img/index.json").is_file()
}

fn ancestors_of(start: Option<PathBuf>) -> Vec<PathBuf> {
    start.map(|s| s.ancestors().map(|a| a.join("offline")).collect()).unwrap_or_default()
}
