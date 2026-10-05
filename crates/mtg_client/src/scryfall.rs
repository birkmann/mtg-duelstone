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
