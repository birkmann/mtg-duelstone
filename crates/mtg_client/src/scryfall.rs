//! Art showcase for the splash/loading screens: famous card artworks plus who painted them.
//!
//! Why read straight from the offline pack instead of the engine's `/api/img`: the showcase has to
//! fill the Boot screen *while* the engine is still starting. Why a pack and not Scryfall: the game
//! must work without internet. `fetch-offline` resolves every [`CURATED`] printing once (artist,
//! set, year, flavor text, the artist's artwork count) into the committed
//! `offline/data/showcase.json`, and saves its art_crop under `offline/img/showcase/`.
//!
//! Why a curated list: Scryfall supplies the artist, set, year and flavor text. The "did you
//! know" lines are hand-written, and each one was checked against the artist/set Scryfall
//! returns for that exact printing.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use crate::curated::{CURATED, Curated, Showpiece};


/// The slice of a Scryfall card object the showcase needs; written by `fetch-offline`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CardMeta {
    pub name: String,
    pub set: String,
    pub artist: String,
    pub set_name: String,
    pub released_at: String,
    #[serde(default)]
    pub flavor_text: Option<String>,
    #[serde(default)]
    pub reserved: bool,
}

/// `offline/data/showcase.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ShowcaseData {
    /// [`key`] of each curated printing → its metadata.
    pub cards: HashMap<String, CardMeta>,
    /// Artist → number of distinct artworks on Magic cards (Scryfall's `unique:art`).
    pub artists: HashMap<String, u32>,
}

/// `<pack>/data/showcase.json`
pub fn data_path(pack: &Path) -> PathBuf {
    pack.join("data/showcase.json")
}

/// `<pack>/img/showcase/<key>.jpg`
pub fn art_path(pack: &Path, c: &Curated) -> PathBuf {
    pack.join("img/showcase").join(format!("{}.jpg", key(c.name, c.set)))
}

/// File stem for a curated printing, e.g. `lea_black_lotus`.
pub fn key(name: &str, set: &str) -> String {
    let slug: String = name.chars().map(|ch| if ch.is_ascii_alphanumeric() { ch.to_ascii_lowercase() } else { '_' }).collect();
    format!("{set}_{slug}")
}

/// Iterator over showpieces in random order, read from the offline pack. Yields nothing when the
/// pack (or its showcase art) is missing; the splash then keeps its animated backdrop.
pub struct Showcase {
    pack: Option<PathBuf>,
    data: ShowcaseData,
    order: Vec<&'static Curated>,
}

impl Showcase {
    /// `seed` picks the order.
    pub fn new(seed: u64) -> Self {
        let pack = crate::offline::dir();
        let data: ShowcaseData = pack
            .as_deref()
            .and_then(|p| std::fs::read(data_path(p)).ok())
            .and_then(|b| serde_json::from_slice(&b).map_err(|e| eprintln!("[showcase] showcase.json: {e}")).ok())
            .unwrap_or_default();
        let mut order: Vec<&'static Curated> = CURATED
            .iter()
            .filter(|c| data.cards.contains_key(&key(c.name, c.set)) && pack.as_deref().is_some_and(|p| art_path(p, c).is_file()))
            .collect();
        if order.is_empty() {
            eprintln!("[showcase] no showcase art in the offline pack; run tools/fetch-offline.sh");
        }
        shuffle(&mut order, seed);
        Self { pack, data, order }
    }
}

impl Iterator for Showcase {
    type Item = Showpiece;

    fn next(&mut self) -> Option<Showpiece> {
        let pack = self.pack.clone()?;
        while let Some(c) = self.order.pop() {
            let Some(meta) = self.data.cards.get(&key(c.name, c.set)).cloned() else { continue };
            let art = match std::fs::read(art_path(&pack, c)) {
                Ok(b) if !b.is_empty() => b,
                Ok(_) => continue,
                Err(e) => {
                    eprintln!("[showcase] {}: {e}", meta.name);
                    continue;
                }
            };
            let mut stat = Vec::new();
            if meta.reserved {
                stat.push("On the Reserved List: Wizards has promised never to reprint it".to_string());
            }
            if let Some(&n) = self.data.artists.get(&meta.artist).filter(|&&n| n > 1) {
                stat.push(format!("{} has {n} artworks on Magic cards", meta.artist));
            }
            let blurb = c.fact.map(str::to_owned).or_else(|| meta.flavor_text.as_ref().map(|f| format!("\u{201C}{}\u{201D}", f.trim_matches('"').replace('\n', " "))));
            return Some(Showpiece {
                name: meta.name,
                artist: meta.artist,
                set_name: meta.set_name,
                year: meta.released_at.chars().take(4).collect(),
                blurb,
                stat: (!stat.is_empty()).then(|| stat.join("  ·  ")),
                art,
            });
        }
        None
    }
}

/// Fisher–Yates with xorshift. The order only needs to vary between launches, so pulling in a rand crate isn't worth it.
fn shuffle<T>(v: &mut [T], seed: u64) {
    let mut x = seed | 1;
    for i in (1..v.len()).rev() {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        v.swap(i, (x % (i as u64 + 1)) as usize);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curated_keys_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for c in CURATED {
            assert!(seen.insert(key(c.name, c.set)), "duplicate {}", c.name);
        }
    }

    #[test]
    fn shuffle_is_a_permutation() {
        let mut v: Vec<u32> = (0..40).collect();
        shuffle(&mut v, 12345);
        let mut s = v.clone();
        s.sort();
        assert_eq!(s, (0..40).collect::<Vec<_>>());
        assert_ne!(v, s);
    }
}
