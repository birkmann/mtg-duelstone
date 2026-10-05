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

/// One printing whose art we show. `set` pins the *original* artwork (reprints often have new art).
pub struct Curated {
    pub name: &'static str,
    pub set: &'static str,
    pub fact: Option<&'static str>,
}

const fn c(name: &'static str, set: &'static str, fact: Option<&'static str>) -> Curated {
    Curated { name, set, fact }
}

pub const CURATED: &[Curated] = &[
    c("Black Lotus", "lea", Some("Christopher Rush, one of Alpha's original artists, also drew Magic's first mana symbols.")),
    c("Lightning Bolt", "lea", Some("Three damage for one red mana. Every red burn spell since 1993 has been measured against Lightning Bolt.")),
    c("Shivan Dragon", "lea", None),
    c("Serra Angel", "lea", Some("Vigilance didn't have a name yet in 1993. Serra Angel's Alpha text just read \"Does not tap when attacking.\"")),
    c("Birds of Paradise", "lea", Some("The first bird-of-paradise skins that reached Europe had their feet removed, so Linnaeus named one species Paradisaea apoda: \"legless\".")),
    c("Sol Ring", "lea", Some("Restricted in Vintage, and still one of the most-played cards in Commander more than 30 years after this painting.")),
    c("Time Walk", "lea", Some("Time Walk is one of the \"Power Nine\", the Alpha-era cards widely considered the strongest ever printed.")),
    c("Mox Sapphire", "lea", Some("Dan Frazier painted all five Alpha Moxen. With Black Lotus, Time Walk, Ancestral Recall and Timetwister they make up the Power Nine.")),
    c("Ancestral Recall", "lea", Some("Three cards for a single blue mana. Ancestral Recall is one of the Power Nine.")),
    c("Timetwister", "lea", Some("Mark Tedin painted Timetwister for Alpha in 1993. Seventeen years later he painted Emrakul, the Aeons Torn.")),
    c("Juzám Djinn", "arn", Some("Arabian Nights (1993), Magic's first expansion, drew its cards from the One Thousand and One Nights.")),
    c("Nicol Bolas", "leg", Some("Legends (1994) introduced legendary creatures, the five Elder Dragons among them. Nicol Bolas went on to become one of Magic's great villains.")),
    c("Brainstorm", "ice", Some("This Brainstorm comes from the same brush as Black Lotus and Lightning Bolt: Christopher Rush.")),
    c("Swords to Plowshares", "lea", None),
    c("Dark Confidant", "rav", Some("\"Bob\" is named after Bob Maher, the pro player who designed the card after winning the Magic Invitational.")),
    c("Meddling Mage", "pls", Some("Pro player Chris Pikula designed Meddling Mage as his prize for winning the Magic Invitational.")),
    c("Island", "unh", Some("John Avon painted Unhinged's full-art basic lands, which collectors have sought out ever since.")),
    c("Path to Exile", "con", Some("Todd Lockwood is also well known for his Dungeons & Dragons art.")),
    c("Bitterblossom", "mor", Some("Rebecca Guay is known for her delicate, watercolor-driven style.")),
    c("Jace, the Mind Sculptor", "wwk", Some("Banned in Standard in 2011 at the height of the \"Caw-Blade\" era.")),
    c("Stoneforge Mystic", "wwk", Some("Banned in Standard in 2011, alongside Jace, the Mind Sculptor.")),
    c("Tarmogoyf", "fut", Some("Tarmogoyf's power is the number of card types among all cards in all graveyards.")),
    c("Emrakul, the Aeons Torn", "roe", Some("Emrakul can't be countered, and casting it gives you an extra turn. Mark Tedin painted it, 17 years after painting for Alpha.")),
    c("Elesh Norn, Grand Cenobite", "nph", Some("Elesh Norn leads the Machine Orthodoxy, the white faction of New Phyrexia.")),
    c("Griselbrand", "avr", Some("Griselbrand was sealed in the Helvault alongside Avacyn. When the Helvault broke, both of them got out.")),
    c("Craterhoof Behemoth", "avr", None),
    c("Avacyn, Angel of Hope", "avr", None),
    c("Snapcaster Mage", "isd", None),
    c("Liliana of the Veil", "isd", None),
    c("Ugin, the Spirit Dragon", "frf", Some("Ugin is Nicol Bolas's twin brother.")),
    c("Baneslayer Angel", "m10", None),
    c("Primeval Titan", "m11", None),
    c("Sheoldred, the Apocalypse", "dmu", None),
    c("Ragavan, Nimble Pilferer", "mh2", Some("Ragavan first appeared in Aether Revolt as a monkey token created by Kari Zev.")),
    c("Goblin Guide", "zen", None),
    c("Wrath of God", "lea", None),
    c("Llanowar Elves", "lea", None),
    c("Counterspell", "lea", None),
    c("Thoughtseize", "lrw", None),
    c("Dark Ritual", "lea", None),
];

/// What the splash screen shows for one artwork.
#[derive(Debug, Clone)]
pub struct Showpiece {
    pub name: String,
    pub artist: String,
    pub set_name: String,
    pub year: String,
    /// Hand-written fact, falling back to the printing's flavor text (quoted).
    pub blurb: Option<String>,
    /// Data-derived line, e.g. "On the Reserved List · Mark Tedin has 246 artworks on Magic cards".
    pub stat: Option<String>,
    /// art_crop JPEG bytes (~626×457)
    pub art: Vec<u8>,
}

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
