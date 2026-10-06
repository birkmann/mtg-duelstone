//! Fills the offline pack (`offline/`) from Scryfall, once, while online. After this the game
//! never touches the network: the engine serves card images and rulings from the pack, and the
//! splash showcase reads it directly (see `mtg_client::offline`).
//!
//! Why Scryfall's bulk files instead of the per-card API the engine used to call: one download
//! each gives every card's image URLs, rulings and artists, and the images themselves come from
//! the `cards.scryfall.io` CDN, which has no API rate limit. Why one printing per card (the
//! `oracle_cards` pick): every printing would be ~10× the size for art nobody chose; the engine
//! already fell back to "any printing" when a set had no image.
//!
//! Resumable: files already on disk are skipped, so re-running only fetches what's new or failed.
//!
//! usage: fetch-offline [--out DIR] [--forge CARDSFOLDER] [--jobs N] [--refresh] [--no-images]

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use mtg_client::scryfall::{self, CURATED, CardMeta};
use serde::{Deserialize, Serialize};
use ureq::Agent;

const UA: &str = concat!("MTGDuelstone/", env!("CARGO_PKG_VERSION"), " (personal Forge client; offline pack)");
const BULK_INDEX: &str = "https://api.scryfall.com/bulk-data";
/// The two sizes the client asks for. Every other version (`normal`, `png`, ...) is served from `large`.
const VERSIONS: [&str; 2] = ["large", "art_crop"];

struct Args {
    out: PathBuf,
    forge: Option<PathBuf>,
    jobs: usize,
    refresh: bool,
    images: bool,
}

fn parse_args() -> Result<Args> {
    let mut a = Args { out: default_out(), forge: None, jobs: 8, refresh: false, images: true };
    let mut it = std::env::args().skip(1);
    while let Some(k) = it.next() {
        match k.as_str() {
            "--out" => a.out = it.next().context("--out DIR")?.into(),
            "--forge" => a.forge = Some(it.next().context("--forge DIR")?.into()),
            "--jobs" => a.jobs = it.next().context("--jobs N")?.parse()?,
            "--refresh" => a.refresh = true,
            "--no-images" => a.images = false,
            "-h" | "--help" => {
                println!("usage: fetch-offline [--out DIR] [--forge CARDSFOLDER] [--jobs N] [--refresh] [--no-images]");
                std::process::exit(0);
            }
            _ => bail!("unknown arg {k}"),
        }
    }
    a.jobs = a.jobs.max(1);
    Ok(a)
}

/// `offline/` of the source checkout we're run from, else the user data dir the game also searches.
fn default_out() -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_default();
    cwd.ancestors().find(|d| d.join("Cargo.lock").is_file() && d.join("crates").is_dir()).map(|d| d.join("offline")).unwrap_or_else(mtg_client::offline::user_dir)
}

// ---- Scryfall bulk shapes (only the fields we keep) -------------------------------------------

#[derive(Deserialize)]
struct BulkList {
    data: Vec<BulkEntry>,
}

#[derive(Deserialize)]
struct BulkEntry {
    #[serde(rename = "type")]
    kind: String,
    /// gzipped JSON Lines, one object per line
    jsonl_download_uri: String,
}

#[derive(Deserialize, Default, Clone)]
struct ImageUris {
    large: Option<String>,
    art_crop: Option<String>,
}

impl ImageUris {
    fn get(&self, version: &str) -> Option<&str> {
        match version {
            "large" => self.large.as_deref(),
            _ => self.art_crop.as_deref(),
        }
    }
}

#[derive(Deserialize)]
struct Face {
    name: String,
    #[serde(default)]
    flavor_name: Option<String>,
    #[serde(default)]
    printed_name: Option<String>,
    #[serde(default)]
    oracle_id: Option<String>,
    #[serde(default)]
    image_uris: Option<ImageUris>,
}

#[derive(Deserialize)]
struct Card {
    id: String,
    #[serde(default)]
    oracle_id: Option<String>,
    name: String,
    /// Universes Beyond / Secret Lair name printed on this card ("Blanka, Ferocious Friend"); Forge
    /// knows some cards only by it. Scryfall uses `flavor_name` or, for some Secret Lairs,
    /// `printed_name` (default_cards is English-only, so that's never a translation).
    #[serde(default)]
    flavor_name: Option<String>,
    #[serde(default)]
    printed_name: Option<String>,
    layout: String,
    #[serde(default)]
    set: String,
    #[serde(default)]
    set_name: String,
    #[serde(default)]
    released_at: String,
    #[serde(default)]
    artist: Option<String>,
    #[serde(default)]
    illustration_id: Option<String>,
    #[serde(default)]
    flavor_text: Option<String>,
    #[serde(default)]
    reserved: bool,
    #[serde(default)]
    scryfall_uri: Option<String>,
    #[serde(default)]
    image_uris: Option<ImageUris>,
    #[serde(default)]
    card_faces: Option<Vec<Face>>,
}

impl Card {
    /// Image URIs per face: one entry for single-image cards (split, adventure, flip included),
    /// two for cards whose faces are printed on both sides.
    fn face_images(&self) -> Vec<ImageUris> {
        if let Some(i) = &self.image_uris {
            return vec![i.clone()];
        }
        self.card_faces.iter().flatten().filter_map(|f| f.image_uris.clone()).collect()
    }

    fn oracle(&self) -> Option<&str> {
        self.oracle_id.as_deref().or_else(|| self.card_faces.as_ref()?.first()?.oracle_id.as_deref())
    }

    /// (flavor name, face index) pairs printed on this card.
    fn flavor_names(&self) -> Vec<(&str, usize)> {
        let mut v: Vec<(&str, usize)> = [&self.flavor_name, &self.printed_name].into_iter().flatten().map(|n| (n.as_str(), 0)).collect();
        for (i, f) in self.card_faces.iter().flatten().enumerate() {
            v.extend([&f.flavor_name, &f.printed_name].into_iter().flatten().map(|n| (n.as_str(), i)));
        }
        v
    }

    fn is_token(&self) -> bool {
        matches!(self.layout.as_str(), "token" | "double_faced_token" | "emblem")
    }
}

#[derive(Deserialize)]
struct Ruling {
    oracle_id: String,
    published_at: String,
    comment: String,
}

// ---- pack outputs -------------------------------------------------------------------------------

/// One `img/index.json` entry. The engine reads this with Gson; keep the field names in sync with
/// `ImageCache.java`.
#[derive(Serialize, Clone)]
struct IndexEntry {
    /// File stem under `img/<version>/<id[0..2]>/`.
    id: String,
    /// Full card name, the key into `data/rulings.json`.
    name: String,
    /// 1 when this key is the card's back face name.
    face: u8,
    /// Whether `<id>-back.jpg` exists.
    back: bool,
}

#[derive(Serialize)]
struct RulingOut<'a> {
    #[serde(rename = "scryfallUri")]
    scryfall_uri: Option<&'a str>,
    rulings: Vec<RulingLine<'a>>,
}

#[derive(Serialize)]
struct RulingLine<'a> {
    date: &'a str,
    comment: &'a str,
}

/// `data/showcase.json` with sorted maps, so re-running the fetch gives a stable diff.
#[derive(Serialize)]
struct ShowcaseOut {
    cards: BTreeMap<String, CardMeta>,
    artists: BTreeMap<String, u32>,
}

fn main() -> std::process::ExitCode {
    let r = run();
    // what returning the Result would print, but before the console may close
    if let Err(e) = &r {
        eprintln!("Error: {e:?}");
    }
    hold_console();
    if r.is_ok() { std::process::ExitCode::SUCCESS } else { std::process::ExitCode::FAILURE }
}

/// Windows: started by double-click, from a Start menu shortcut or from the windowed game, this is
/// the only process on its console, which closes with it. Keep it open so the result can be read.
#[cfg(windows)]
fn hold_console() {
    let mut pids = [0u32; 2];
    if unsafe { windows_sys::Win32::System::Console::GetConsoleProcessList(pids.as_mut_ptr(), 2) } == 1 {
        print!("Press Enter to close this window.");
        let _ = std::io::stdout().flush();
        let _ = std::io::stdin().read_line(&mut String::new());
    }
}

#[cfg(not(windows))]
fn hold_console() {}

fn run() -> Result<()> {
    let args = parse_args()?;
    let img = args.out.join("img");
    let data = args.out.join("data");
    let bulk = img.join(".bulk");
    for d in [&img, &data, &bulk] {
        std::fs::create_dir_all(d).with_context(|| format!("create {}", d.display()))?;
    }
    println!("offline pack: {}", args.out.display());
    let agent: Agent = Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_recv_response(Some(Duration::from_secs(60)))
        .http_status_as_error(false)
        .build()
        .into();

    let list: BulkList = get(&agent, BULK_INDEX)?.body_mut().read_json().context("parse /bulk-data")?;
    let uri = |kind: &str| list.data.iter().find(|e| e.kind == kind).map(|e| e.jsonl_download_uri.clone()).with_context(|| format!("no {kind} bulk file"));
    let oracle_path = download_bulk(&agent, &uri("oracle_cards")?, &bulk.join("oracle_cards.jsonl.gz"), args.refresh)?;
    let rulings_path = download_bulk(&agent, &uri("rulings")?, &bulk.join("rulings.jsonl.gz"), args.refresh)?;
    let default_path = download_bulk(&agent, &uri("default_cards")?, &bulk.join("default_cards.jsonl.gz"), args.refresh)?;

    println!("parsing oracle cards…");
    let mut cards: Vec<Card> = read_jsonl(&oracle_path)?;
    cards.retain(|c| c.layout != "art_series");
    // Real cards claim names before tokens: "Treasure" the card, not the token, owns that key.
    cards.sort_by_key(Card::is_token);
    println!("  {} cards", cards.len());

    println!("parsing default cards…");
    let printings: Vec<Card> = read_jsonl(&default_path)?;
    let flavored = flavor_printings(&cards, &printings);
    println!("  {} printings, {} with a flavor name Forge may use", printings.len(), flavored.len());

    write_rulings(&cards, &rulings_path, &data.join("rulings.json"))?;
    write_showcase(&agent, &printings, &args.out)?;

    if args.images {
        let all: Vec<&Card> = cards.iter().chain(flavored.iter().copied()).collect();
        download_images(&all, &img, args.jobs)?;
    }
    let index = build_index(&cards, &flavored, &img);
    write_atomic(&img.join("index.json"), &serde_json::to_vec(&index)?)?;
    println!("index: {} names → {}", index.len(), img.join("index.json").display());

    let cardsfolder = args.forge.or_else(find_cardsfolder);
    match cardsfolder {
        Some(cf) => coverage(&cf, &index, &img)?,
        None => println!("coverage: no forge/forge-gui/res/cardsfolder found (pass --forge) — skipped"),
    }
    Ok(())
}

// ---- network ------------------------------------------------------------------------------------

fn get(agent: &Agent, url: &str) -> Result<ureq::http::Response<ureq::Body>> {
    let mut last = None;
    for attempt in 0..4u64 {
        match agent.get(url).header("User-Agent", UA).header("Accept", "*/*").call() {
            Ok(r) if r.status().is_success() => return Ok(r),
            Ok(r) if r.status() == 404 => bail!("{url}: HTTP 404"),
            Ok(r) => last = Some(anyhow::anyhow!("{url}: HTTP {}", r.status())),
            Err(e) => last = Some(anyhow::anyhow!("{url}: {e}")),
        }
        std::thread::sleep(Duration::from_millis(1000 * (attempt + 1)));
    }
    Err(last.unwrap())
}

/// Streams a bulk file to disk (they are hundreds of MB). Kept between runs unless `--refresh`.
fn download_bulk(agent: &Agent, url: &str, dest: &Path, refresh: bool) -> Result<PathBuf> {
    if !refresh && dest.is_file() {
        println!("bulk: {} (cached)", dest.display());
        return Ok(dest.to_path_buf());
    }
    println!("bulk: {url}");
    let resp = get(agent, url)?;
    let tmp = dest.with_extension("tmp");
    let mut f = std::fs::File::create(&tmp)?;
    let n = std::io::copy(&mut resp.into_body().into_reader(), &mut f).with_context(|| format!("download {url}"))?;
    f.flush()?;
    std::fs::rename(&tmp, dest)?;
    println!("  {} MB", n / 1_000_000);
    Ok(dest.to_path_buf())
}

/// A gzipped JSON Lines bulk file.
fn read_jsonl<T: for<'de> Deserialize<'de>>(p: &Path) -> Result<Vec<T>> {
    use std::io::BufRead;
    let f = std::fs::File::open(p).with_context(|| format!("open {}", p.display()))?;
    let mut out = Vec::new();
    for (i, line) in std::io::BufReader::new(flate2::read::GzDecoder::new(f)).lines().enumerate() {
        let line = line.with_context(|| format!("read {}", p.display()))?;
        if !line.trim().is_empty() {
            out.push(serde_json::from_str(&line).with_context(|| format!("parse {} line {}", p.display(), i + 1))?);
        }
    }
    Ok(out)
}

fn write_atomic(p: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = p.with_extension("tmp");
    std::fs::write(&tmp, bytes).with_context(|| format!("write {}", tmp.display()))?;
    std::fs::rename(&tmp, p)?;
    Ok(())
}

// ---- images -------------------------------------------------------------------------------------

fn image_path(img: &Path, version: &str, id: &str, back: bool) -> PathBuf {
    img.join(version).join(&id[..2]).join(format!("{id}{}.jpg", if back { "-back" } else { "" }))
}

fn download_images(cards: &[&Card], img: &Path, jobs: usize) -> Result<()> {
    let mut todo: Vec<(String, PathBuf)> = Vec::new();
    let mut have = 0usize;
    for c in cards {
        for (i, uris) in c.face_images().iter().take(2).enumerate() {
            for v in VERSIONS {
                let Some(url) = uris.get(v) else { continue };
                let p = image_path(img, v, &c.id, i == 1);
                if p.metadata().map(|m| m.len() > 0).unwrap_or(false) {
                    have += 1;
                } else {
                    todo.push((url.to_string(), p));
                }
            }
        }
    }
    println!("images: {have} on disk, {} to download ({jobs} workers)", todo.len());
    if todo.is_empty() {
        return Ok(());
    }
    let total = todo.len();
    let (tx, rx) = crossbeam_channel::unbounded::<(String, PathBuf)>();
    for t in todo {
        tx.send(t).ok();
    }
    drop(tx);
    let done = AtomicUsize::new(0);
    let failed = std::sync::Mutex::new(Vec::<String>::new());
    let t0 = Instant::now();
    std::thread::scope(|s| {
        for _ in 0..jobs {
            let rx = rx.clone();
            let (done, failed) = (&done, &failed);
            s.spawn(move || {
                let agent: Agent = Agent::config_builder()
                    .timeout_connect(Some(Duration::from_secs(15)))
                    .timeout_global(Some(Duration::from_secs(60)))
                    .http_status_as_error(false)
                    .build()
                    .into();
                for (url, path) in rx {
                    if let Err(e) = fetch_to(&agent, &url, &path) {
                        failed.lock().unwrap().push(format!("{e:#}"));
                    }
                    let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                    if n % 500 == 0 || n == total {
                        let rate = n as f64 / t0.elapsed().as_secs_f64().max(0.1);
                        println!("  {n}/{total}  ({rate:.0}/s, ~{} min left)", ((total - n) as f64 / rate / 60.0).ceil());
                    }
                }
            });
        }
    });
    let failed = failed.into_inner().unwrap();
    if !failed.is_empty() {
        println!("images: {} failed (re-run to retry):", failed.len());
        for f in failed.iter().take(20) {
            println!("  {f}");
        }
    }
    Ok(())
}

fn fetch_to(agent: &Agent, url: &str, path: &Path) -> Result<()> {
    let bytes = get(agent, url)?.body_mut().with_config().limit(32 * 1024 * 1024).read_to_vec().with_context(|| format!("read {url}"))?;
    if bytes.is_empty() {
        bail!("{url}: empty body");
    }
    std::fs::create_dir_all(path.parent().unwrap())?;
    write_atomic(path, &bytes)
}

/// One printing per flavor name that isn't also a real card name, so a card Forge knows only as
/// "Blanka, Ferocious Friend" shows the Street Fighter printing, not the Universes Within one.
fn flavor_printings<'a>(cards: &[Card], printings: &'a [Card]) -> Vec<&'a Card> {
    let real: HashSet<String> = cards.iter().flat_map(|c| std::iter::once(c.name.to_lowercase()).chain(c.card_faces.iter().flatten().map(|f| f.name.to_lowercase()))).collect();
    let mut seen = HashSet::new();
    printings
        .iter()
        .filter(|p| p.flavor_names().iter().any(|(n, _)| !real.contains(&n.to_lowercase()) && seen.insert(n.to_lowercase())))
        .collect()
}

/// Lowercase name (full name, every face name, flavor names) → card. Entries whose image never
/// downloaded are left out, so the engine answers 404 and the client shows its rules-text fallback.
fn build_index(cards: &[Card], flavored: &[&Card], img: &Path) -> BTreeMap<String, IndexEntry> {
    let mut index = BTreeMap::new();
    let mut add = |c: &Card, key: &str, face_i: usize| {
        if !image_path(img, "large", &c.id, false).is_file() {
            return;
        }
        let back = image_path(img, "large", &c.id, true).is_file();
        // Only double-sided cards have a separate back image; a split card's halves share the front.
        let face = u8::from(back && face_i == 1);
        index.entry(key.to_lowercase()).or_insert_with(|| IndexEntry { id: c.id.clone(), name: c.name.clone(), face, back });
    };
    for c in cards {
        add(c, &c.name, 0);
        for (i, f) in c.card_faces.iter().flatten().enumerate() {
            add(c, &f.name, i);
        }
    }
    for c in flavored {
        for (n, i) in c.flavor_names() {
            add(c, n, i);
        }
    }
    // Forge spells a few names differently: "Take a Trip to . . ." and a plain colon in
    // "Ratonhnhaké:ton" (Scryfall: "Take a Trip to..." and U+A789).
    let aliases: Vec<(String, String)> = index
        .keys()
        .filter_map(|k| {
            let a = k.replace("...", " . . .").replace("  . . .", " . . .").replace('\u{A789}', ":");
            (a != *k && !index.contains_key(&a)).then(|| (a, k.clone()))
        })
        .collect();
    for (alias, k) in aliases {
        let e = index[&k].clone();
        index.insert(alias, e);
    }
    index
}

// ---- rulings ------------------------------------------------------------------------------------

/// `data/rulings.json`: one card per line (diff-friendly), only cards that have rulings.
fn write_rulings(cards: &[Card], rulings_path: &Path, dest: &Path) -> Result<()> {
    println!("parsing rulings…");
    let rulings: Vec<Ruling> = read_jsonl(rulings_path)?;
    let mut by_oracle: HashMap<&str, Vec<&Ruling>> = HashMap::new();
    for r in &rulings {
        by_oracle.entry(r.oracle_id.as_str()).or_default().push(r);
    }
    let mut out: BTreeMap<String, RulingOut> = BTreeMap::new();
    for c in cards.iter().filter(|c| !c.is_token()) {
        let Some(rs) = c.oracle().and_then(|o| by_oracle.get(o)) else { continue };
        let mut rs = rs.clone();
        rs.sort_by(|a, b| a.published_at.cmp(&b.published_at));
        out.entry(c.name.to_lowercase()).or_insert_with(|| RulingOut {
            scryfall_uri: c.scryfall_uri.as_deref(),
            rulings: rs.iter().map(|r| RulingLine { date: &r.published_at, comment: &r.comment }).collect(),
        });
    }
    let mut s = String::from("{\n");
    for (i, (k, v)) in out.iter().enumerate() {
        s.push_str(&serde_json::to_string(k)?);
        s.push(':');
        s.push_str(&serde_json::to_string(v)?);
        s.push_str(if i + 1 < out.len() { ",\n" } else { "\n" });
    }
    s.push_str("}\n");
    write_atomic(dest, s.as_bytes())?;
    println!("rulings: {} cards → {}", out.len(), dest.display());
    Ok(())
}

// ---- showcase -----------------------------------------------------------------------------------

/// Resolve every curated printing (exact set) and count each curated artist's distinct artworks.
fn write_showcase(agent: &Agent, all: &[Card], pack: &Path) -> Result<()> {
    let mut out = ShowcaseOut { cards: BTreeMap::new(), artists: BTreeMap::new() };
    for cur in CURATED {
        let Some(c) = all.iter().find(|c| c.set == cur.set && c.name.eq_ignore_ascii_case(cur.name)) else {
            println!("  showcase: {} ({}) not found", cur.name, cur.set);
            continue;
        };
        let Some(art_url) = c.image_uris.as_ref().and_then(|i| i.art_crop.as_deref()) else { continue };
        let path = scryfall::art_path(pack, cur);
        if !path.is_file()
            && let Err(e) = fetch_to(agent, art_url, &path)
        {
            println!("  showcase: {e:#}");
            continue;
        }
        let artist = c.artist.clone().unwrap_or_default();
        out.cards.insert(
            scryfall::key(cur.name, cur.set),
            CardMeta {
                name: c.name.clone(),
                set: c.set.clone(),
                artist: artist.clone(),
                set_name: c.set_name.clone(),
                released_at: c.released_at.clone(),
                flavor_text: c.flavor_text.clone(),
                reserved: c.reserved,
            },
        );
        if !out.artists.contains_key(&artist) {
            // Same notion as Scryfall's `a:"…" unique:art`: distinct illustrations crediting the artist.
            let works: HashSet<&str> = all
                .iter()
                .filter(|x| x.artist.as_deref().is_some_and(|a| a.contains(artist.as_str())))
                .filter_map(|x| x.illustration_id.as_deref())
                .collect();
            out.artists.insert(artist, works.len() as u32);
        }
    }
    let dest = scryfall::data_path(pack);
    write_atomic(&dest, &serde_json::to_vec_pretty(&out)?)?;
    println!("showcase: {}/{} printings → {}", out.cards.len(), CURATED.len(), dest.display());
    Ok(())
}

// ---- coverage -----------------------------------------------------------------------------------

fn find_cardsfolder() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    cwd.ancestors().map(|d| d.join("forge/forge-gui/res/cardsfolder")).find(|p| p.is_dir())
}

/// Mirrors `ImageCache.lookup` in the engine: exact, then without Forge's " Token" suffix, then
/// an Alchemy rebalance ("A-Blood Artist", gone from Scryfall) as its original card.
fn engine_lookup(index: &BTreeMap<String, IndexEntry>, name: &str) -> bool {
    let k = name.to_lowercase();
    index.contains_key(&k) || k.strip_suffix(" token").is_some_and(|b| index.contains_key(b)) || k.strip_prefix("a-").is_some_and(|b| index.contains_key(b))
}

/// Every Forge card name that has no image, using the engine's lookup rules. Written to
/// `img/missing.txt`.
fn coverage(cardsfolder: &Path, index: &BTreeMap<String, IndexEntry>, img: &Path) -> Result<()> {
    let mut names = Vec::new();
    let mut stack = vec![cardsfolder.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d)?.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "txt") {
                let text = std::fs::read_to_string(&p).unwrap_or_default();
                names.extend(text.lines().filter_map(|l| l.strip_prefix("Name:")).map(|n| n.trim().to_string()));
            }
        }
    }
    names.sort();
    names.dedup();
    let missing: Vec<&String> = names.iter().filter(|n| !engine_lookup(index, n)).collect();
    let report = img.join("missing.txt");
    write_atomic(&report, missing.iter().map(|n| format!("{n}\n")).collect::<String>().as_bytes())?;
    println!("coverage: {}/{} Forge card names have an image; missing → {}", names.len() - missing.len(), names.len(), report.display());
    for n in missing.iter().take(15) {
        println!("  missing: {n}");
    }
    Ok(())
}
