//! Art showcase for the splash/loading screens: famous card artworks plus who painted them.
//!
//! Why straight to Scryfall instead of the engine's `/api/img` proxy: the showcase has to fill
//! the Boot screen *while* the engine is still starting. Following Scryfall's API guidelines
//! (identifying User-Agent, ≥100 ms between requests, cache aggressively), everything is kept on
//! disk under the platform cache dir. After the first run the showcase works offline, and
//! Scryfall sees about one request per card, ever.
//!
//! Why a curated list: Scryfall supplies the artist, set, year and flavor text. The "did you
//! know" lines are hand-written, and each one was checked against the artist/set Scryfall
//! returns for that exact printing.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use ureq::Agent;

const UA: &str = concat!("MTGArenaFanClient/", env!("CARGO_PKG_VERSION"), " (personal Forge client)");
const API: &str = "https://api.scryfall.com";
/// Artist portfolio sizes grow slowly; refresh them once a month.
const ARTIST_TTL_SECS: u64 = 30 * 24 * 3600;

pub use crate::curated::{CURATED, Curated, Showpiece};


/// The slice of a Scryfall card object we keep on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CardMeta {
    name: String,
    set: String,
    artist: String,
    set_name: String,
    released_at: String,
    #[serde(default)]
    flavor_text: Option<String>,
    #[serde(default)]
    reserved: bool,
    art_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ArtistCount {
    works: u32,
    fetched: u64,
}

/// `~/.cache/mtg-arena/showcase` (platform cache dir).
pub fn cache_dir() -> PathBuf {
    directories::ProjectDirs::from("", "", "mtg-arena")
        .map(|d| d.cache_dir().to_path_buf())
        .unwrap_or_else(std::env::temp_dir)
        .join("showcase")
}

fn key(name: &str, set: &str) -> String {
    let slug: String = name.chars().map(|ch| if ch.is_ascii_alphanumeric() { ch.to_ascii_lowercase() } else { '_' }).collect();
    format!("{set}_{slug}")
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn read_json<T: for<'de> Deserialize<'de> + Default>(p: &Path) -> T {
    std::fs::read(p).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

/// Write via a temp file so a crash mid-write never leaves a truncated cache entry behind.
fn write_atomic(p: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = p.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, p)?;
    Ok(())
}

/// Blocking iterator over showpieces in random order: arts already on disk first (instant, offline),
/// then ones that still need downloading. Downloads happen lazily, one per `next()`, so a short
/// boot only fetches what it actually shows. Run it on its own thread.
pub struct Showcase {
    dir: PathBuf,
    agent: Agent,
    last_request: Option<Instant>,
    cards: HashMap<String, CardMeta>,
    artists: HashMap<String, ArtistCount>,
    order: Vec<&'static Curated>,
}

impl Showcase {
    /// `seed` picks the order. A failed metadata download isn't an error: the iterator just
    /// yields whatever is cached, possibly nothing.
    pub fn new(seed: u64) -> Self {
        let dir = cache_dir();
        let _ = std::fs::create_dir_all(dir.join("art"));
        let agent: Agent = Agent::config_builder().timeout_global(Some(Duration::from_secs(20))).http_status_as_error(false).build().into();
        let mut s = Self {
            cards: read_json(&dir.join("cards.json")),
            artists: read_json(&dir.join("artists.json")),
            dir,
            agent,
            last_request: None,
            order: Vec::new(),
        };
        if let Err(e) = s.fill_metadata() {
            eprintln!("[showcase] card metadata: {e:#}");
        }
        let mut order: Vec<&'static Curated> = CURATED.iter().filter(|c| s.cards.contains_key(&key(c.name, c.set))).collect();
        shuffle(&mut order, seed);
        // stable sort: cached arts keep their shuffled order but come first
        order.sort_by_key(|c| !s.art_path(c).is_file());
        // `next()` pops from the back
        order.reverse();
        s.order = order;
        s
    }

    fn art_path(&self, c: &Curated) -> PathBuf {
        self.dir.join("art").join(format!("{}.jpg", key(c.name, c.set)))
    }

    /// Scryfall asks for 50–100 ms between requests; every request goes through here.
    fn polite(&mut self) {
        if let Some(t) = self.last_request {
            let gap = Duration::from_millis(120);
            let since = t.elapsed();
            if since < gap {
                std::thread::sleep(gap - since);
            }
        }
        self.last_request = Some(Instant::now());
    }

    /// One `/cards/collection` POST for every curated printing not yet on disk.
    fn fill_metadata(&mut self) -> Result<()> {
        let missing: Vec<&Curated> = CURATED.iter().filter(|c| !self.cards.contains_key(&key(c.name, c.set))).collect();
        if missing.is_empty() {
            return Ok(());
        }
        let ids: Vec<serde_json::Value> = missing.iter().map(|c| serde_json::json!({ "name": c.name, "set": c.set })).collect();
        self.polite();
        let mut resp = self
            .agent
            .post(format!("{API}/cards/collection"))
            .header("User-Agent", UA)
            .header("Accept", "application/json")
            .send_json(serde_json::json!({ "identifiers": ids }))
            .context("POST /cards/collection")?;
        if !resp.status().is_success() {
            return Err(anyhow!("/cards/collection: HTTP {}", resp.status()));
        }
        let body: serde_json::Value = resp.body_mut().read_json().context("parse collection")?;
        for card in body["data"].as_array().into_iter().flatten() {
            let s = |k: &str| card[k].as_str().unwrap_or_default().to_string();
            let Some(art_url) = card["image_uris"]["art_crop"].as_str() else { continue };
            // Scryfall echoes its canonical name; key by the curated spelling we asked for.
            let Some(cur) = missing.iter().find(|c| c.name.eq_ignore_ascii_case(&s("name")) && c.set == s("set")) else { continue };
            let meta = CardMeta {
                name: s("name"),
                set: s("set"),
                artist: s("artist"),
                set_name: s("set_name"),
                released_at: s("released_at"),
                flavor_text: card["flavor_text"].as_str().map(str::to_owned),
                reserved: card["reserved"].as_bool().unwrap_or(false),
                art_url: art_url.to_string(),
            };
            self.cards.insert(key(cur.name, cur.set), meta);
        }
        write_atomic(&self.dir.join("cards.json"), &serde_json::to_vec_pretty(&self.cards)?)
    }

    fn art(&mut self, c: &Curated, meta: &CardMeta) -> Result<Vec<u8>> {
        let path = self.art_path(c);
        if let Ok(b) = std::fs::read(&path)
            && !b.is_empty()
        {
            return Ok(b);
        }
        self.polite();
        let mut resp = self.agent.get(&meta.art_url).header("User-Agent", UA).header("Accept", "image/*").call().context("GET art_crop")?;
        if !resp.status().is_success() {
            return Err(anyhow!("art_crop {}: HTTP {}", meta.name, resp.status()));
        }
        let bytes = resp.body_mut().read_to_vec().context("read art_crop")?;
        write_atomic(&path, &bytes)?;
        Ok(bytes)
    }

    /// Number of distinct artworks this artist has on Magic cards (`a:"…" unique:art`).
    fn artist_works(&mut self, artist: &str) -> Option<u32> {
        if let Some(a) = self.artists.get(artist)
            && now_secs().saturating_sub(a.fetched) < ARTIST_TTL_SECS
        {
            return Some(a.works);
        }
        self.polite();
        let fresh = self
            .agent
            .get(format!("{API}/cards/search"))
            .query("q", format!("a:\"{artist}\""))
            .query("unique", "art")
            .header("User-Agent", UA)
            .header("Accept", "application/json")
            .call()
            .ok()
            .filter(|r| r.status().is_success())
            .and_then(|mut r| r.body_mut().read_json::<serde_json::Value>().ok())
            .and_then(|v| v["total_cards"].as_u64())
            .map(|n| n as u32);
        match fresh {
            Some(works) => {
                self.artists.insert(artist.to_string(), ArtistCount { works, fetched: now_secs() });
                if let Ok(b) = serde_json::to_vec_pretty(&self.artists) {
                    let _ = write_atomic(&self.dir.join("artists.json"), &b);
                }
                Some(works)
            }
            // offline: a stale count beats none
            None => self.artists.get(artist).map(|a| a.works),
        }
    }
}

impl Iterator for Showcase {
    type Item = Showpiece;

    fn next(&mut self) -> Option<Showpiece> {
        while let Some(c) = self.order.pop() {
            let Some(meta) = self.cards.get(&key(c.name, c.set)).cloned() else { continue };
            let art = match self.art(c, &meta) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("[showcase] {e:#}");
                    continue;
                }
            };
            let mut stat = Vec::new();
            if meta.reserved {
                stat.push("On the Reserved List: Wizards has promised never to reprint it".to_string());
            }
            if let Some(n) = self.artist_works(&meta.artist).filter(|&n| n > 1) {
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
