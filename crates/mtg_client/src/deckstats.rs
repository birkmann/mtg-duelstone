//! Deck analysis for the deck builder: mana curve, colour demand vs. land sources, card-type and
//! role counts with format targets, opening-hand odds and a goldfish "can I cast it on curve"
//! simulation.
//!
//! Why here (no Bevy) and not in the UI: it is pure maths over `CatalogCard`s, so it is unit-tested
//! and reusable (e.g. the autopilot could sanity-check decks). The engine still owns legality —
//! nothing here decides whether a deck is *allowed*, only how it is likely to *play*.

use std::collections::BTreeMap;

use mtg_protocol::{CardKind, CatalogCard, Format};

/// Colour letters in WUBRG order; index = colour slot used by every per-colour array here.
pub const COLORS: [char; 5] = ['W', 'U', 'B', 'R', 'G'];
/// Curve buckets 0..=6 plus "7+".
pub const CURVE_BUCKETS: usize = 8;
/// Roles shown in the stats panel, with their display labels.
pub const ROLES: [(&str, &str); 7] = [
    ("ramp", "Ramp"),
    ("draw", "Card Draw"),
    ("removal", "Removal"),
    ("wipe", "Board Wipes"),
    ("counter", "Counterspells"),
    ("tutor", "Tutors"),
    ("protection", "Protection"),
];

fn color_index(c: char) -> Option<usize> {
    COLORS.iter().position(|x| *x == c.to_ascii_uppercase())
}

/// One deck slot: the card and how many copies.
#[derive(Debug, Clone)]
pub struct Entry<'a> {
    pub card: &'a CatalogCard,
    pub qty: u32,
}

/// `{2}{G}{W/U}` → `["2","G","W/U"]`.
pub fn mana_symbols(cost: &str) -> Vec<&str> {
    cost.split('{').filter_map(|s| s.split('}').next()).filter(|s| !s.is_empty()).collect()
}

/// Coloured pips of one cost, per colour. Hybrid symbols count half to each side; Phyrexian pips
/// count fully (paying life is a real cost, but the colour is what the mana base must serve).
pub fn pips(cost: &str) -> [f32; 5] {
    let mut out = [0.0; 5];
    for sym in mana_symbols(cost) {
        let parts: Vec<char> = sym.split('/').filter_map(|p| p.chars().next()).filter(|c| color_index(*c).is_some()).collect();
        let share = if parts.is_empty() { 0.0 } else { 1.0 / parts.len() as f32 };
        for c in parts {
            out[color_index(c).unwrap()] += share;
        }
    }
    out
}

/// Colours a land can produce, from its basic land types and the mana symbols in its rules text.
/// "Any color" (Command Tower, City of Brass) and fetches that find a basic count as all colours.
pub fn land_colors(card: &CatalogCard) -> [bool; 5] {
    let mut out = [false; 5];
    let t = card.type_line.to_ascii_lowercase();
    for (i, basic) in ["plains", "island", "swamp", "mountain", "forest"].iter().enumerate() {
        if t.contains(basic) {
            out[i] = true;
        }
    }
    let text = card.oracle_text.as_deref().unwrap_or("").to_ascii_lowercase();
    if text.contains("any color") || text.contains("any one color") || text.contains("basic land card") {
        return [true; 5];
    }
    for sentence in text.split(['.', '\n']) {
        if let Some(i) = sentence.find("add ") {
            for sym in mana_symbols(&sentence[i..]) {
                for p in sym.split('/') {
                    if let Some(c) = p.chars().next().and_then(|c| color_index(c.to_ascii_uppercase())) {
                        out[c] = true;
                    }
                }
            }
        }
    }
    out
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RoleCount {
    pub key: &'static str,
    pub label: &'static str,
    pub count: u32,
    /// Format guideline (0 = no guideline for this role).
    pub target: u32,
}

#[derive(Debug, Clone, Default)]
pub struct DeckStats {
    pub format: Format,
    /// Main deck cards incl. commanders.
    pub total: u32,
    /// Required size (100 commander incl. commanders, 60 constructed minimum).
    pub required: u32,
    pub lands: u32,
    pub nonlands: u32,
    /// Nonland cards per mana value bucket, split creature / noncreature.
    pub curve_creature: [u32; CURVE_BUCKETS],
    pub curve_other: [u32; CURVE_BUCKETS],
    /// Average mana value of nonland cards.
    pub avg_cmc: f32,
    /// Coloured pips across all nonland costs (incl. commanders).
    pub pips: [f32; 5],
    /// Lands producing each colour.
    pub sources: [u32; 5],
    pub types: BTreeMap<CardKind, u32>,
    pub roles: Vec<RoleCount>,
    /// Karsten's land-count guideline for this curve.
    pub suggested_lands: f32,
    /// Hypergeometric odds for the opening seven.
    pub p_open_2_to_5_lands: f32,
    pub p_open_screw: f32,
    pub p_open_flood: f32,
    pub expected_open_lands: f32,
    /// P(at least N lands by turn N, on the draw) for turns 1..=6.
    pub p_land_drop: [f32; 6],
}

impl DeckStats {
    pub fn curve_max(&self) -> u32 {
        (0..CURVE_BUCKETS).map(|i| self.curve_creature[i] + self.curve_other[i]).max().unwrap_or(0)
    }
    /// Colours the spells ask for. "Any colour" lands make every colour look supplied, so sources
    /// alone don't make a colour relevant — unless the deck has no coloured costs at all.
    pub fn colors_used(&self) -> Vec<usize> {
        let asked: Vec<usize> = (0..5).filter(|&i| self.pips[i] > 0.0).collect();
        if asked.is_empty() { (0..5).filter(|&i| self.sources[i] > 0 && self.sources[i] < self.lands).collect() } else { asked }
    }
}

/// Deck size rules and role guidelines per format. Commander numbers are the community template
/// (≈10 ramp, 10 draw, 8–10 removal, 2–3 wipes); constructed has no fixed template.
fn role_target(format: Format, key: &str) -> u32 {
    match (format, key) {
        (Format::Commander, "ramp") => 10,
        (Format::Commander, "draw") => 10,
        (Format::Commander, "removal") => 8,
        (Format::Commander, "wipe") => 3,
        (Format::Constructed, "removal") => 6,
        _ => 0,
    }
}

pub fn analyze(format: Format, commanders: &[&CatalogCard], main: &[Entry]) -> DeckStats {
    let mut s = DeckStats { format, required: if format == Format::Commander { 100 } else { 60 }, ..Default::default() };
    let all = commanders.iter().map(|c| Entry { card: c, qty: 1 }).chain(main.iter().cloned());
    let mut cmc_sum = 0u32;
    let mut role_counts: BTreeMap<&str, u32> = BTreeMap::new();
    let mut cheap_helpers = 0u32;
    for e in all {
        let c = e.card;
        s.total += e.qty;
        *s.types.entry(c.primary_type()).or_default() += e.qty;
        for (key, _) in ROLES {
            if c.has_tag(key) {
                *role_counts.entry(key).or_default() += e.qty;
            }
        }
        if c.primary_type() == CardKind::Land {
            s.lands += e.qty;
            let lc = land_colors(c);
            for (src, has) in s.sources.iter_mut().zip(lc) {
                if has {
                    *src += e.qty;
                }
            }
            continue;
        }
        s.nonlands += e.qty;
        let b = (c.cmc.max(0) as usize).min(CURVE_BUCKETS - 1);
        if c.is_creature {
            s.curve_creature[b] += e.qty;
        } else {
            s.curve_other[b] += e.qty;
        }
        cmc_sum += c.cmc.max(0) as u32 * e.qty;
        let p = pips(c.mana_cost.as_deref().unwrap_or(""));
        for (pip, n) in s.pips.iter_mut().zip(p) {
            *pip += n * e.qty as f32;
        }
        if c.cmc <= 2 && (c.has_tag("ramp") || c.has_tag("draw")) {
            cheap_helpers += e.qty;
        }
    }
    s.avg_cmc = if s.nonlands > 0 { cmc_sum as f32 / s.nonlands as f32 } else { 0.0 };
    s.roles = ROLES
        .iter()
        .map(|(k, l)| RoleCount { key: k, label: l, count: role_counts.get(k).copied().unwrap_or(0), target: role_target(format, k) })
        .collect();
    // Frank Karsten's regressions ("How many lands do you need…", 2022): 60-card and 99-card versions.
    s.suggested_lands = match format {
        Format::Commander => 31.42 + 3.13 * s.avg_cmc - 0.28 * cheap_helpers as f32,
        Format::Constructed => 19.59 + 1.90 * s.avg_cmc - 0.28 * cheap_helpers as f32,
    }
    .max(0.0);

    // Odds use the library the game actually shuffles: commanders start in the command zone.
    let library = s.total.saturating_sub(commanders.len() as u32);
    let lands = s.lands;
    if library >= 7 {
        let p = |k: u32| hypergeom_pmf(library, lands, 7, k);
        s.p_open_2_to_5_lands = (2..=5).map(p).sum();
        s.p_open_screw = (0..=1).map(p).sum();
        s.p_open_flood = (6..=7).map(p).sum();
        s.expected_open_lands = 7.0 * lands as f32 / library as f32;
        for t in 1..=6u32 {
            // on the draw: 7 + t cards seen by turn t
            let seen = (7 + t).min(library);
            s.p_land_drop[t as usize - 1] = hypergeom_at_least(library, lands, seen, t);
        }
    }
    s
}

// ---- hypergeometric ----------------------------------------------------------------------------

fn ln_choose(n: u32, k: u32) -> f64 {
    if k > n {
        return f64::NEG_INFINITY;
    }
    let k = k.min(n - k);
    (0..k).map(|i| ((n - i) as f64).ln() - ((i + 1) as f64).ln()).sum()
}

/// P(exactly `k` successes in `draws` from `pop` cards containing `succ` successes).
pub fn hypergeom_pmf(pop: u32, succ: u32, draws: u32, k: u32) -> f32 {
    if k > succ || k > draws || draws - k > pop - succ.min(pop) || draws > pop {
        return 0.0;
    }
    (ln_choose(succ, k) + ln_choose(pop - succ, draws - k) - ln_choose(pop, draws)).exp() as f32
}

pub fn hypergeom_at_least(pop: u32, succ: u32, draws: u32, k: u32) -> f32 {
    (k..=draws.min(succ)).map(|i| hypergeom_pmf(pop, succ, draws, i)).sum::<f32>().min(1.0)
}

// ---- goldfish simulation ---------------------------------------------------------------------

/// Small deterministic PRNG (xorshift64*), so sample hands and simulations need no extra crate and
/// stats don't jitter between redraws of the same deck.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.max(1).wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n.max(1) as u64) as usize
    }
    pub fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            let j = self.below(i + 1);
            v.swap(i, j);
        }
    }
}

/// How reliably a spell can be cast on curve, from the goldfish simulation.
#[derive(Debug, Clone, PartialEq)]
pub struct Castability {
    pub name: String,
    pub cmc: i32,
    /// P(enough lands with the right colours by turn = mana value), on the draw.
    pub on_curve: f32,
}

#[derive(Debug, Clone, Default)]
pub struct Goldfish {
    pub iterations: u32,
    /// Weighted mean of `on_curve` over nonland cards with mana value 1..=6.
    pub curve_consistency: f32,
    /// Hardest cards first.
    pub cards: Vec<Castability>,
    /// Mean number of lands in play by turn 1..=8 when playing one per turn.
    pub lands_by_turn: [f32; 8],
}

/// Can the coloured requirement `need` be paid from `lands` (one colour per land, `total` mana)?
/// Brute-force assignment is fine: needs are ≤ 6 pips and lands are a handful.
fn payable(need: &[usize], lands: &[[bool; 5]], used: &mut Vec<bool>) -> bool {
    let Some((&first, rest)) = need.split_first() else { return true };
    for i in 0..lands.len() {
        if !used[i] && lands[i][first] {
            used[i] = true;
            if payable(rest, lands, used) {
                used[i] = false;
                return true;
            }
            used[i] = false;
        }
    }
    false
}

/// Simulate `iterations` games drawing (on the draw, no mulligans, one land per turn) and record,
/// for every nonland card, whether its cost was payable on the turn equal to its mana value.
/// Ramp is ignored on purpose: this measures the mana base, not the deck's acceleration.
pub fn goldfish(main: &[Entry], commanders: &[&CatalogCard], iterations: u32, seed: u64) -> Goldfish {
    // library: one slot per copy; Some(colours) for lands
    let mut library: Vec<Option<[bool; 5]>> = Vec::new();
    for e in main {
        for _ in 0..e.qty {
            library.push((e.card.primary_type() == CardKind::Land).then(|| land_colors(e.card)));
        }
    }
    let mut spells: Vec<(&CatalogCard, Vec<usize>, u32)> = Vec::new();
    for e in main.iter().cloned().chain(commanders.iter().map(|c| Entry { card: c, qty: 1 })) {
        let c = e.card;
        if c.primary_type() == CardKind::Land || c.cmc < 1 || c.cmc > 6 {
            continue;
        }
        // Hybrid pips are satisfiable by either colour; requiring the first keeps the check simple and
        // conservative.
        let need: Vec<usize> = mana_symbols(c.mana_cost.as_deref().unwrap_or(""))
            .iter()
            .filter_map(|s| s.split('/').next().and_then(|p| p.chars().next()).and_then(color_index))
            .collect();
        spells.push((c, need, e.qty));
    }
    let mut out = Goldfish { iterations, ..Default::default() };
    if library.len() < 8 || iterations == 0 {
        return out;
    }
    let mut ok = vec![0u32; spells.len()];
    let mut rng = Rng::new(seed);
    let mut lands_in_play: Vec<[bool; 5]> = Vec::with_capacity(8);
    let mut used = Vec::new();
    for _ in 0..iterations {
        rng.shuffle(&mut library);
        lands_in_play.clear();
        let mut hand_lands: Vec<[bool; 5]> = Vec::new();
        let mut drawn = 7usize;
        hand_lands.extend(library[..7].iter().flatten().copied());
        for turn in 1..=8usize {
            if drawn < library.len() {
                if let Some(l) = library[drawn] {
                    hand_lands.push(l);
                }
                drawn += 1;
            }
            // Play the land that adds a colour we don't have yet, else any.
            if !hand_lands.is_empty() {
                let have = |c: usize| lands_in_play.iter().any(|l| l[c]);
                let pick = (0..hand_lands.len()).find(|&i| (0..5).any(|c| hand_lands[i][c] && !have(c))).unwrap_or(0);
                lands_in_play.push(hand_lands.swap_remove(pick));
            }
            out.lands_by_turn[turn - 1] += lands_in_play.len() as f32;
            for (i, (c, need, _)) in spells.iter().enumerate() {
                if c.cmc as usize == turn && lands_in_play.len() >= turn {
                    used.clear();
                    used.resize(lands_in_play.len(), false);
                    if payable(need, &lands_in_play, &mut used) {
                        ok[i] += 1;
                    }
                }
            }
        }
    }
    let n = iterations as f32;
    for v in &mut out.lands_by_turn {
        *v /= n;
    }
    let (mut wsum, mut w) = (0.0, 0.0);
    for (i, (c, _, qty)) in spells.iter().enumerate() {
        let p = ok[i] as f32 / n;
        wsum += p * *qty as f32;
        w += *qty as f32;
        out.cards.push(Castability { name: c.name.clone(), cmc: c.cmc, on_curve: p });
    }
    out.curve_consistency = if w > 0.0 { wsum / w } else { 0.0 };
    out.cards.sort_by(|a, b| a.on_curve.total_cmp(&b.on_curve).then(a.name.cmp(&b.name)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(name: &str, cost: Option<&str>, cmc: i32, ty: &str, text: &str) -> CatalogCard {
        CatalogCard {
            name: name.into(),
            mana_cost: cost.map(Into::into),
            cmc,
            type_line: ty.into(),
            oracle_text: Some(text.into()),
            is_land: ty.contains("Land"),
            is_creature: ty.contains("Creature"),
            ..Default::default()
        }
    }

    #[test]
    fn pips_count_hybrid_halves() {
        assert_eq!(pips("{2}{G}{G}"), [0.0, 0.0, 0.0, 0.0, 2.0]);
        assert_eq!(pips("{W/U}{B}"), [0.5, 0.5, 1.0, 0.0, 0.0]);
        assert_eq!(pips("{X}{R/P}"), [0.0, 0.0, 0.0, 1.0, 0.0]);
        assert_eq!(pips("{2/W}"), [1.0, 0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn land_colors_from_types_and_text() {
        let forest = card("Forest", None, 0, "Basic Land - Forest", "");
        assert_eq!(land_colors(&forest), [false, false, false, false, true]);
        let dual = card("Hinterland Harbor", None, 0, "Land", "~ enters tapped unless you control a Forest or an Island.\n{T}: Add {G} or {U}.");
        assert_eq!(land_colors(&dual), [false, true, false, false, true]);
        let tower = card("Command Tower", None, 0, "Land", "{T}: Add one mana of any color in your commander's color identity.");
        assert_eq!(land_colors(&tower), [true; 5]);
    }

    #[test]
    fn hypergeometric_matches_known_values() {
        // 60 cards, 24 lands, opening 7: P(exactly 3) = C(24,3)·C(36,4)/C(60,7) ≈ 0.3087
        let p = hypergeom_pmf(60, 24, 7, 3);
        assert!((p - 0.3087).abs() < 0.001, "{p}");
        let total: f32 = (0..=7).map(|k| hypergeom_pmf(60, 24, 7, k)).sum();
        assert!((total - 1.0).abs() < 1e-4);
        assert_eq!(hypergeom_pmf(10, 2, 7, 3), 0.0);
    }

    #[test]
    fn analyze_counts_curve_roles_and_lands() {
        let bolt = CatalogCard { tags: vec!["removal".into()], ..card("Lightning Bolt", Some("{R}"), 1, "Instant", "") };
        let bear = card("Grizzly Bears", Some("{1}{G}"), 2, "Creature - Bear", "");
        let mtn = card("Mountain", None, 0, "Basic Land - Mountain", "");
        let forest = card("Forest", None, 0, "Basic Land - Forest", "");
        let main = [Entry { card: &bolt, qty: 4 }, Entry { card: &bear, qty: 32 }, Entry { card: &mtn, qty: 12 }, Entry { card: &forest, qty: 12 }];
        let s = analyze(Format::Constructed, &[], &main);
        assert_eq!(s.total, 60);
        assert_eq!(s.lands, 24);
        assert_eq!(s.curve_other[1], 4);
        assert_eq!(s.curve_creature[2], 32);
        assert_eq!(s.sources, [0, 0, 0, 12, 12]);
        assert_eq!(s.roles.iter().find(|r| r.key == "removal").unwrap().count, 4);
        assert!((s.avg_cmc - (4.0 + 64.0) / 36.0).abs() < 1e-4);
        assert!(s.p_open_2_to_5_lands > 0.8);
        assert_eq!(s.types[&CardKind::Land], 24);
    }

    #[test]
    fn goldfish_flags_off_colour_cards() {
        let bear = card("Grizzly Bears", Some("{1}{G}"), 2, "Creature - Bear", "");
        let angel = card("Serra Angel", Some("{3}{W}{W}"), 5, "Creature - Angel", "");
        let forest = card("Forest", None, 0, "Basic Land - Forest", "");
        let main = [Entry { card: &bear, qty: 20 }, Entry { card: &angel, qty: 4 }, Entry { card: &forest, qty: 24 }];
        let g = goldfish(&main, &[], 400, 7);
        let angel_p = g.cards.iter().find(|c| c.name == "Serra Angel").unwrap().on_curve;
        let bear_p = g.cards.iter().find(|c| c.name == "Grizzly Bears").unwrap().on_curve;
        assert_eq!(angel_p, 0.0, "no white sources");
        assert!(bear_p > 0.8, "{bear_p}");
        assert_eq!(g.cards[0].name, "Serra Angel");
    }
}
