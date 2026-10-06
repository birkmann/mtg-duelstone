//! Cosmetics: sleeves, deckboxes and playmats. Owns the design library (presets + the player's
//! own designs in `~/.config/mtg-duelstone/designs.json`), paints designs into textures, and publishes
//! what is currently equipped as [`Cosmetics`] for the table, the cards and the menus.
//!
//! Why a texture cache keyed by design fingerprint: the studio edits designs many times a second
//! and every deck tile wants a thumbnail. Painting runs on worker threads; [`Paints::get`] always
//! answers immediately with the newest *finished* texture, so nothing ever renders a texture that
//! isn't there yet (Bevy skips meshes whose texture is missing, which would make card backs blink).
//!
//! What is equipped lives in `Settings` (`sleeve`, `deckbox`, per-deck `deck_looks`, and `theme`
//! for the playmat, kept for compatibility with the original battlefield themes).

pub mod design;
pub mod mesh;
pub mod paint;

use std::collections::HashMap;
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

use bevy::prelude::*;
use crossbeam_channel::Receiver;

use crate::shared::{CardImages, MatchConfig, Settings};
pub use design::{Design, DesignKind, Emblem, Finish, Frame, PALETTE, Pattern, Rgb};
pub use paint::PaintTarget;

pub struct CosmeticsPlugin;

impl Plugin for CosmeticsPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(DesignLibrary::load())
            .init_resource::<Paints>()
            .init_resource::<Cosmetics>()
            .init_resource::<LibrarySave>()
            .add_systems(Startup, preload_art)
            .add_systems(Update, (resolve_equipped, pump_paints, save_library).chain());
    }
}

// ---- library --------------------------------------------------------------------------------

/// Every design: presets first, then the player's own. Owner: cosmetics (load/save); the design
/// studio (menus) adds, edits and removes custom designs.
#[derive(Resource, Debug, Clone, Default)]
pub struct DesignLibrary {
    pub designs: Vec<Design>,
}

#[cfg(not(target_arch = "wasm32"))]
fn designs_path() -> Option<PathBuf> {
    crate::settings::settings_path().map(|p| p.with_file_name("designs.json"))
}

/// The saved custom designs (JSON), and where they came from for warnings.
#[cfg(not(target_arch = "wasm32"))]
fn read_designs() -> Option<(String, String)> {
    let path = designs_path()?;
    std::fs::read_to_string(&path).ok().map(|t| (t, path.display().to_string()))
}

#[cfg(not(target_arch = "wasm32"))]
fn write_designs(text: &str) {
    let Some(path) = designs_path() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = std::fs::write(&path, text) {
        warn!("cosmetics: cannot write {}: {e}", path.display());
    }
}

#[cfg(target_arch = "wasm32")]
const WEB_KEY: &str = "mtg-duelstone.designs";

#[cfg(target_arch = "wasm32")]
fn read_designs() -> Option<(String, String)> {
    crate::platform::storage_get(WEB_KEY).map(|t| (t, "localStorage".to_string()))
}

#[cfg(target_arch = "wasm32")]
fn write_designs(text: &str) {
    if !crate::platform::storage_set(WEB_KEY, text) {
        warn!("cosmetics: localStorage unavailable");
    }
}

impl DesignLibrary {
    fn load() -> Self {
        let mut designs = design::presets();
        if let Some((text, from)) = read_designs() {
            match serde_json::from_str::<Vec<Design>>(&text) {
                Ok(custom) => designs.extend(custom.into_iter().filter(|d| !d.id.is_empty()).map(|mut d| {
                    d.builtin = false;
                    d
                })),
                Err(e) => warn!("cosmetics: {from} unreadable ({e}); custom designs skipped"),
            }
        }
        Self { designs }
    }

    pub fn get(&self, id: &str) -> Option<&Design> {
        self.designs.iter().find(|d| d.id == id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Design> {
        self.designs.iter_mut().find(|d| d.id == id)
    }

    pub fn of_kind(&self, kind: DesignKind) -> impl Iterator<Item = &Design> {
        self.designs.iter().filter(move |d| d.kind == kind)
    }

    /// The design with this id, or the first preset of the kind when it's gone (deleted, or a
    /// settings file from another machine).
    pub fn resolve(&self, kind: DesignKind, id: &str) -> &Design {
        self.get(id).filter(|d| d.kind == kind).or_else(|| self.of_kind(kind).next()).expect("presets cover every kind")
    }

    pub fn sleeve_for(&self, settings: &Settings, deck: Option<&str>) -> &Design {
        let id = deck.and_then(|r| settings.deck_looks.get(r)).and_then(|l| l.sleeve.as_deref()).unwrap_or(&settings.sleeve);
        self.resolve(DesignKind::Sleeve, id)
    }

    pub fn deckbox_for(&self, settings: &Settings, deck: Option<&str>) -> &Design {
        let id = deck.and_then(|r| settings.deck_looks.get(r)).and_then(|l| l.deckbox.as_deref()).unwrap_or(&settings.deckbox);
        self.resolve(DesignKind::Deckbox, id)
    }

    pub fn playmat(&self, settings: &Settings) -> &Design {
        self.resolve(DesignKind::Playmat, &settings.theme)
    }

    /// The AI's sleeve: a painted preset picked from its deck ref (stable per deck, never the
    /// same as yours), so the two sides of the table are easy to tell apart.
    pub fn opponent_sleeve(&self, mine: &str, ai_deck: &str) -> &Design {
        let pool: Vec<&Design> = self.of_kind(DesignKind::Sleeve).filter(|d| d.builtin && d.image.is_none() && d.id != mine).collect();
        let h = ai_deck.bytes().fold(2166136261u32, |h, b| (h ^ b as u32).wrapping_mul(16777619));
        pool.get(h as usize % pool.len().max(1)).copied().unwrap_or_else(|| self.resolve(DesignKind::Sleeve, mine))
    }

    /// A fresh id for a custom design.
    pub fn new_id(&self, kind: DesignKind) -> String {
        let n = mtg_client::now_millis();
        let mut id = format!("custom-{}-{n:x}", kind.singular());
        while self.get(&id).is_some() {
            id.push('x');
        }
        id
    }
}

#[derive(Resource, Default)]
struct LibrarySave {
    dirty_for: Option<f32>,
    seen_first: bool,
}

fn save_library(time: Res<Time>, lib: Res<DesignLibrary>, mut st: ResMut<LibrarySave>) {
    if lib.is_changed() {
        // the initial insert counts as a change; don't write the file back for it
        if st.seen_first {
            st.dirty_for = Some(0.0);
        }
        st.seen_first = true;
    }
    let Some(t) = st.dirty_for.as_mut() else { return };
    *t += time.delta_secs();
    if *t < 0.75 {
        return;
    }
    st.dirty_for = None;
    let custom: Vec<&Design> = lib.designs.iter().filter(|d| !d.builtin).collect();
    match serde_json::to_string_pretty(&custom) {
        Ok(text) => write_designs(&text),
        Err(e) => warn!("cosmetics: serialize failed: {e}"),
    }
}

// ---- texture cache --------------------------------------------------------------------------

#[derive(Default)]
struct Entry {
    ready: Option<(u64, Handle<Image>)>,
    job: Option<(u64, Receiver<Image>)>,
    want: Option<Design>,
}

/// Painted textures per (design id, target). Owner: cosmetics. Anyone may call [`Paints::get`].
#[derive(Resource, Default)]
pub struct Paints {
    entries: HashMap<(String, PaintTarget), Entry>,
    /// shipped artwork for image presets, loaded once
    art: HashMap<&'static str, Handle<Image>>,
}

/// At most this many paint jobs run at once (the studio can request dozens of thumbnails).
const MAX_JOBS: usize = 4;

impl Paints {
    /// Newest finished texture for `d` (it may lag an edit behind while a repaint runs); `None`
    /// until the first paint lands. Queues a repaint whenever the design has changed.
    pub fn get(&mut self, d: &Design, t: PaintTarget) -> Option<Handle<Image>> {
        if let Some(path) = d.image {
            // the deckbox-style 3/4 thumbnail doesn't exist for flat art: the art is the thumbnail
            return self.art.get(path).cloned();
        }
        let fp = d.fingerprint();
        let e = self.entries.entry((d.id.clone(), t)).or_default();
        let queued = e.job.as_ref().map(|j| j.0) == Some(fp) || e.want.as_ref().is_some_and(|w| w.fingerprint() == fp);
        if e.ready.as_ref().map(|r| r.0) != Some(fp) && !queued {
            e.want = Some(d.clone());
        }
        e.ready.as_ref().map(|r| r.1.clone())
    }

    /// True once the texture for the design's *current* look is ready.
    pub fn is_current(&self, d: &Design, t: PaintTarget) -> bool {
        d.image.is_some() || self.entries.get(&(d.id.clone(), t)).and_then(|e| e.ready.as_ref()).is_some_and(|r| r.0 == d.fingerprint())
    }
}

fn preload_art(mut paints: ResMut<Paints>, assets: Res<AssetServer>, card_images: Res<CardImages>) {
    for d in design::presets() {
        if let Some(path) = d.image {
            // CardImages already holds the classic back (with its procedural fallback)
            let h = if path == design::CLASSIC_BACK { card_images.card_back.clone() } else { assets.load(path) };
            paints.art.insert(path, h);
        }
    }
}

fn pump_paints(mut paints: ResMut<Paints>, mut images: ResMut<Assets<Image>>) {
    let mut running = 0;
    for e in paints.entries.values_mut() {
        if let Some((fp, rx)) = &e.job {
            match rx.try_recv() {
                Ok(img) => {
                    e.ready = Some((*fp, images.add(img)));
                    e.job = None;
                }
                Err(crossbeam_channel::TryRecvError::Disconnected) => e.job = None,
                Err(crossbeam_channel::TryRecvError::Empty) => running += 1,
            }
        }
    }
    for ((_, target), e) in paints.entries.iter_mut() {
        if running >= MAX_JOBS {
            break;
        }
        if e.job.is_some() {
            continue;
        }
        let Some(d) = e.want.take() else { continue };
        let t = *target;
        // a worker thread on desktop; in the browser it runs on the page's event loop
        e.job = Some((d.fingerprint(), mtg_client::spawn(async move { paint::render(&d, t) })));
        running += 1;
    }
}

// ---- what's equipped ------------------------------------------------------------------------

/// Textures of what's equipped right now. Owner: cosmetics. Readers: cards (sleeves), table
/// (playmat), menus (carousel backs). `None` until the first paint lands.
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct Cosmetics {
    pub my_sleeve: Option<Handle<Image>>,
    pub my_finish: Finish,
    pub opp_sleeve: Option<Handle<Image>>,
    pub opp_finish: Finish,
    pub playmat: Option<Handle<Image>>,
    pub playmat_finish: Finish,
}

fn resolve_equipped(lib: Res<DesignLibrary>, settings: Res<Settings>, cfg: Res<MatchConfig>, mut paints: ResMut<Paints>, mut out: ResMut<Cosmetics>) {
    let deck = Some(cfg.human_deck.as_str()).filter(|d| !d.is_empty());
    let mine = lib.sleeve_for(&settings, deck);
    let opp = lib.opponent_sleeve(&mine.id, &cfg.ai_deck);
    let mat = lib.playmat(&settings);
    let next = Cosmetics {
        my_sleeve: paints.get(mine, PaintTarget::Full),
        my_finish: mine.finish,
        opp_sleeve: paints.get(opp, PaintTarget::Full),
        opp_finish: opp.finish,
        playmat: paints.get(mat, PaintTarget::Full),
        playmat_finish: mat.finish,
    };
    if *out != next {
        *out = next;
    }
}

/// Material look for a finish: matte card stock, glossy sleeves, foil with a stronger sheen.
pub fn apply_finish(m: &mut StandardMaterial, f: Finish) {
    let (rough, refl) = match f {
        Finish::Matte => (0.85, 0.08),
        Finish::Gloss => (0.42, 0.35),
        Finish::Foil => (0.3, 0.55),
    };
    m.perceptual_roughness = rough;
    m.reflectance = refl;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::DeckLook;

    #[test]
    fn per_deck_looks_override_the_default_and_unknown_ids_fall_back() {
        let lib = DesignLibrary { designs: design::presets() };
        let mut s = Settings { sleeve: "celestial".into(), ..Settings::default() };
        assert_eq!(lib.sleeve_for(&s, Some("user:A")).id, "celestial");
        s.deck_looks.insert("user:A".into(), DeckLook { sleeve: Some("dragonscale".into()), deckbox: None, ..Default::default() });
        assert_eq!(lib.sleeve_for(&s, Some("user:A")).id, "dragonscale");
        assert_eq!(lib.sleeve_for(&s, Some("user:B")).id, "celestial");
        s.sleeve = "deleted-design".into();
        assert_eq!(lib.sleeve_for(&s, None).kind, DesignKind::Sleeve);
        assert_eq!(lib.resolve(DesignKind::Playmat, "dragonscale").kind, DesignKind::Playmat);
    }

    #[test]
    fn opponent_sleeve_differs_from_mine_and_is_stable() {
        let lib = DesignLibrary { designs: design::presets() };
        for mine in ["ember-swirl", "celestial", "classic"] {
            let a = lib.opponent_sleeve(mine, "precon:Goblins");
            assert_ne!(a.id, mine);
            assert_eq!(a.id, lib.opponent_sleeve(mine, "precon:Goblins").id);
        }
    }
}
