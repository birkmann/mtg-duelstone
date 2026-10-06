//! Cube-screen logic with no Bevy in it: how cards fall into colour columns, type blocks, guilds and
//! curve buckets, the view filter, undo, and the balance numbers of the STATS view.
//!
//! Why apart from the views: every view (table, spoiler, curve, stacks, stats) slices the same cards the
//! same way, and these rules — "Dryad Arbor is a land", "WU is Azorius", "MV 0 and 1 share a column" —
//! are what a cube designer checks, so they live where they can be unit-tested.

use mtg_client::deckstats::ROLES;
use mtg_protocol::{CardKind, CubeCard};

/// The columns of the table and stacks views (CubeCobra's order).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ColorGroup {
    White,
    Blue,
    Black,
    Red,
    Green,
    Colorless,
    Multi,
    Land,
}

impl ColorGroup {
    pub const ALL: [ColorGroup; 8] =
        [ColorGroup::White, ColorGroup::Blue, ColorGroup::Black, ColorGroup::Red, ColorGroup::Green, ColorGroup::Colorless, ColorGroup::Multi, ColorGroup::Land];

    /// Lands first, whatever their colour (Dryad Arbor sits with the lands, as on CubeCobra).
    pub fn of(c: &CubeCard) -> Self {
        if c.primary_type() == CardKind::Land {
            return ColorGroup::Land;
        }
        let colors: Vec<char> = "WUBRG".chars().filter(|l| c.colors.contains(*l)).collect();
        match colors.as_slice() {
            [] => ColorGroup::Colorless,
            ['W'] => ColorGroup::White,
            ['U'] => ColorGroup::Blue,
            ['B'] => ColorGroup::Black,
            ['R'] => ColorGroup::Red,
            ['G'] => ColorGroup::Green,
            _ => ColorGroup::Multi,
        }
    }
    pub fn index(self) -> usize {
        self as usize
    }
    pub fn label(self) -> &'static str {
        match self {
            ColorGroup::White => "White",
            ColorGroup::Blue => "Blue",
            ColorGroup::Black => "Black",
            ColorGroup::Red => "Red",
            ColorGroup::Green => "Green",
            ColorGroup::Colorless => "Colourless",
            ColorGroup::Multi => "Multicolour",
            ColorGroup::Land => "Lands",
        }
    }
    /// Mana letter of a mono-coloured group (filter chips draw its pip).
    pub fn mana(self) -> Option<char> {
        match self {
            ColorGroup::White => Some('W'),
            ColorGroup::Blue => Some('U'),
            ColorGroup::Black => Some('B'),
            ColorGroup::Red => Some('R'),
            ColorGroup::Green => Some('G'),
            ColorGroup::Colorless => Some('C'),
            _ => None,
        }
    }
}

/// Two- and three-colour pairs in the usual order (allied guilds, enemy guilds, shards, wedges).
pub const COMBOS: [(&str, &str); 20] = [
    ("WU", "Azorius"),
    ("UB", "Dimir"),
    ("BR", "Rakdos"),
    ("RG", "Gruul"),
    ("WG", "Selesnya"),
    ("WB", "Orzhov"),
    ("UR", "Izzet"),
    ("BG", "Golgari"),
    ("WR", "Boros"),
    ("UG", "Simic"),
    ("WUG", "Bant"),
    ("WUB", "Esper"),
    ("UBR", "Grixis"),
    ("BRG", "Jund"),
    ("WRG", "Naya"),
    ("WBG", "Abzan"),
    ("WUR", "Jeskai"),
    ("UBG", "Sultai"),
    ("WBR", "Mardu"),
    ("URG", "Temur"),
];

/// `colors` in WUBRG order (the keys of [`COMBOS`]).
fn canonical(colors: &str) -> String {
    "WUBRG".chars().filter(|l| colors.contains(*l)).collect()
}

/// Sort rank and name of a multicolour card's colour pair / triple.
pub fn combo(colors: &str) -> (usize, &'static str) {
    let key = canonical(colors);
    if let Some(i) = COMBOS.iter().position(|(k, _)| *k == key) {
        return (i, COMBOS[i].1);
    }
    match key.len() {
        4 => (COMBOS.len(), "Four-colour"),
        5 => (COMBOS.len() + 1, "Five-colour"),
        _ => (COMBOS.len() + 2, "Other"),
    }
}

/// The curve view's columns: 0–1 share one (CubeCobra does the same), 7+ is the last.
pub const CURVE_LABELS: [&str; 7] = ["0–1", "2", "3", "4", "5", "6", "7+"];

pub fn curve_bucket(cmc: i32) -> usize {
    (cmc.clamp(1, 7) - 1) as usize
}

/// Stacks pile one mana value each; 8 and up share the last pile.
pub fn stack_bucket(cmc: i32) -> usize {
    cmc.clamp(0, 8) as usize
}

pub fn stack_label(bucket: usize) -> String {
    if bucket == 8 { "8+".into() } else { bucket.to_string() }
}

/// What the views show: a free-text search over name, type line and rules text (every word must
/// match), and colour columns switched off.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Filter {
    pub text: String,
    pub hidden: [bool; 8],
}

impl Filter {
    pub fn matches(&self, c: &CubeCard) -> bool {
        if self.hidden[ColorGroup::of(c).index()] {
            return false;
        }
        let words: Vec<String> = self.text.split_whitespace().map(str::to_lowercase).collect();
        if words.is_empty() {
            return true;
        }
        let hay = format!("{}\n{}\n{}", c.name, c.type_line, c.oracle_text.as_deref().unwrap_or("")).to_lowercase();
        words.iter().all(|w| hay.contains(w.as_str()))
    }
    pub fn active(&self) -> bool {
        !self.text.trim().is_empty() || self.hidden.iter().any(|h| *h)
    }
}

/// Copies a card counts for (cubes list singletons as qty 1; 0 means 1).
pub fn copies(c: &CubeCard) -> i32 {
    c.qty.max(1)
}

pub fn total(cards: &[CubeCard]) -> i32 {
    cards.iter().map(copies).sum()
}

fn by_curve(cards: &mut [&CubeCard]) {
    cards.sort_by(|a, b| (a.cmc, &a.name).cmp(&(b.cmc, &b.name)));
}

/// Matching cards per colour group, each sorted by (mana value, name).
pub fn by_group<'a>(cards: &'a [CubeCard], filter: &Filter) -> [Vec<&'a CubeCard>; 8] {
    let mut out: [Vec<&CubeCard>; 8] = Default::default();
    for c in cards.iter().filter(|c| filter.matches(c)) {
        out[ColorGroup::of(c).index()].push(c);
    }
    for g in &mut out {
        by_curve(g);
    }
    out
}

/// The labelled blocks inside one colour column: multicolour cards by guild / shard, lands as one
/// block, everything else by card type. Keeps the (mana value, name) order within each block.
pub fn blocks<'a>(group: ColorGroup, cards: &[&'a CubeCard]) -> Vec<(String, Vec<&'a CubeCard>)> {
    match group {
        ColorGroup::Land => vec![("Lands".to_string(), cards.to_vec())],
        ColorGroup::Multi => {
            let mut keyed: Vec<(usize, &'static str, Vec<&CubeCard>)> = Vec::new();
            for c in cards {
                let (rank, name) = combo(&c.colors);
                match keyed.iter_mut().find(|(r, _, _)| *r == rank) {
                    Some((_, _, v)) => v.push(c),
                    None => keyed.push((rank, name, vec![c])),
                }
            }
            keyed.sort_by_key(|(r, _, _)| *r);
            keyed.into_iter().map(|(_, n, v)| (n.to_string(), v)).collect()
        }
        _ => CardKind::ALL
            .iter()
            .filter_map(|k| {
                let v: Vec<&CubeCard> = cards.iter().copied().filter(|c| c.primary_type() == *k).collect();
                (!v.is_empty()).then(|| (k.label().to_string(), v))
            })
            .collect(),
    }
}

/// Snapshots of the card list before each edit, newest last.
#[derive(Debug, Default)]
pub struct Undo {
    stack: Vec<Vec<CubeCard>>,
}

const UNDO_DEPTH: usize = 50;

impl Undo {
    pub fn push(&mut self, cards: &[CubeCard]) {
        if self.stack.len() == UNDO_DEPTH {
            self.stack.remove(0);
        }
        self.stack.push(cards.to_vec());
    }
    pub fn pop(&mut self) -> Option<Vec<CubeCard>> {
        self.stack.pop()
    }
    pub fn clear(&mut self) {
        self.stack.clear();
    }
    pub fn is_empty(&self) -> bool {
        self.stack.is_empty()
    }
}

/// Card types the stats matrix has columns for (Battle and Other folded into the last).
pub const MATRIX_KINDS: [(&str, &[CardKind]); 7] = [
    ("Creature", &[CardKind::Creature]),
    ("Planeswalker", &[CardKind::Planeswalker]),
    ("Instant", &[CardKind::Instant]),
    ("Sorcery", &[CardKind::Sorcery]),
    ("Artifact", &[CardKind::Artifact]),
    ("Enchantment", &[CardKind::Enchantment]),
    ("Other", &[CardKind::Battle, CardKind::Other, CardKind::Land]),
];

/// Balance numbers for the STATS view (all counts include extra copies).
#[derive(Debug, Default, Clone, PartialEq)]
pub struct CubeStats {
    pub total: i32,
    pub creatures: i32,
    pub lands: i32,
    /// average mana value of nonland cards
    pub avg_mv: f32,
    /// per colour group: cards and type columns ([`MATRIX_KINDS`])
    pub groups: [i32; 8],
    pub matrix: [[i32; 7]; 8],
    /// per colour group and curve bucket: (creatures, other spells)
    pub curve: [[(i32, i32); 7]; 8],
    /// per colour group and [`ROLES`] entry
    pub roles: [[i32; 7]; 8],
    /// per [`COMBOS`] entry, by colour (not colour identity, so hybrid and lands don't skew it)
    pub combos: [i32; 20],
}

pub fn stats<'a>(cards: impl IntoIterator<Item = &'a CubeCard>) -> CubeStats {
    let mut s = CubeStats::default();
    let (mut mv_sum, mut mv_n) = (0i32, 0i32);
    for c in cards {
        let n = copies(c);
        let g = ColorGroup::of(c).index();
        let kind = c.primary_type();
        s.total += n;
        s.groups[g] += n;
        if let Some(col) = MATRIX_KINDS.iter().position(|(_, ks)| ks.contains(&kind)) {
            s.matrix[g][col] += n;
        }
        if kind == CardKind::Land {
            s.lands += n;
        } else {
            mv_sum += c.cmc * n;
            mv_n += n;
            let b = &mut s.curve[g][curve_bucket(c.cmc)];
            if kind == CardKind::Creature {
                b.0 += n;
            } else {
                b.1 += n;
            }
        }
        if kind == CardKind::Creature {
            s.creatures += n;
        }
        for (i, (key, _)) in ROLES.iter().enumerate() {
            if c.has_tag(key) {
                s.roles[g][i] += n;
            }
        }
        if kind != CardKind::Land
            && let Some(i) = COMBOS.iter().position(|(k, _)| *k == canonical(&c.colors))
        {
            s.combos[i] += n;
        }
    }
    s.avg_mv = if mv_n > 0 { mv_sum as f32 / mv_n as f32 } else { 0.0 };
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(name: &str, colors: &str, ty: &str, cmc: i32) -> CubeCard {
        CubeCard {
            name: name.into(),
            colors: colors.into(),
            type_line: ty.into(),
            cmc,
            is_land: ty.contains("Land"),
            is_creature: ty.contains("Creature"),
            qty: 1,
            ..Default::default()
        }
    }

    #[test]
    fn colour_groups_put_lands_first() {
        assert_eq!(ColorGroup::of(&card("Bolt", "R", "Instant", 1)), ColorGroup::Red);
        assert_eq!(ColorGroup::of(&card("Mox", "", "Artifact", 0)), ColorGroup::Colorless);
        assert_eq!(ColorGroup::of(&card("Dryad Arbor", "G", "Land Creature - Forest Dryad", 0)), ColorGroup::Land);
        assert_eq!(ColorGroup::of(&card("Teferi", "WU", "Legendary Planeswalker - Teferi", 5)), ColorGroup::Multi);
    }

    #[test]
    fn combos_are_named_in_any_letter_order() {
        assert_eq!(combo("UW").1, "Azorius");
        assert_eq!(combo("GBR").1, "Jund");
        assert_eq!(combo("WUBG").1, "Four-colour");
        assert_eq!(combo("WUBRG").1, "Five-colour");
        assert!(combo("WU").0 < combo("UB").0 && combo("UG").0 < combo("WUG").0);
    }

    #[test]
    fn curve_buckets_merge_zero_and_one() {
        assert_eq!([0, 1, 2, 6, 7, 12].map(curve_bucket), [0, 0, 1, 5, 6, 6]);
        assert_eq!(stack_label(stack_bucket(11)), "8+");
    }

    #[test]
    fn filter_needs_every_word_and_respects_hidden_groups() {
        let mut bolt = card("Lightning Bolt", "R", "Instant", 1);
        bolt.oracle_text = Some("Lightning Bolt deals 3 damage to any target.".into());
        let mut f = Filter { text: "bolt DAMAGE".into(), ..Default::default() };
        assert!(f.matches(&bolt) && f.active());
        f.text = "bolt flying".into();
        assert!(!f.matches(&bolt));
        f.text.clear();
        f.hidden[ColorGroup::Red.index()] = true;
        assert!(!f.matches(&bolt) && f.active());
    }

    #[test]
    fn blocks_group_by_type_and_guild() {
        let cards = [card("B", "WU", "Creature", 3), card("A", "UB", "Instant", 2), card("C", "WU", "Sorcery", 1)];
        let refs: Vec<&CubeCard> = cards.iter().collect();
        let b = blocks(ColorGroup::Multi, &refs);
        assert_eq!(b.iter().map(|(n, v)| (n.as_str(), v.len())).collect::<Vec<_>>(), vec![("Azorius", 2), ("Dimir", 1)]);
        let mono = [card("Elf", "G", "Creature - Elf", 1), card("Growth", "G", "Instant", 1)];
        let refs: Vec<&CubeCard> = mono.iter().collect();
        assert_eq!(blocks(ColorGroup::Green, &refs).iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), vec!["Creatures", "Instants"]);
    }

    #[test]
    fn by_group_sorts_by_curve_then_name() {
        let cards = vec![card("Zap", "R", "Instant", 1), card("Arc", "R", "Instant", 2), card("Burn", "R", "Instant", 1)];
        let g = by_group(&cards, &Filter::default());
        assert_eq!(g[ColorGroup::Red.index()].iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), vec!["Burn", "Zap", "Arc"]);
    }

    #[test]
    fn undo_is_bounded_and_last_in_first_out() {
        let mut u = Undo::default();
        for i in 0..60 {
            u.push(&[card(&i.to_string(), "", "Artifact", 0)]);
        }
        assert_eq!(u.pop().unwrap()[0].name, "59");
        let mut n = 1;
        while u.pop().is_some() {
            n += 1;
        }
        assert_eq!(n, UNDO_DEPTH);
    }

    #[test]
    fn stats_count_copies_roles_and_guilds() {
        let mut swords = card("Swords", "W", "Instant", 1);
        swords.tags = vec!["removal".into()];
        swords.qty = 2;
        let cards = [swords, card("Lion", "W", "Creature - Cat", 1), card("Charm", "WU", "Instant", 2), card("Island", "", "Basic Land - Island", 0)];
        let s = stats(&cards);
        assert_eq!((s.total, s.creatures, s.lands), (5, 1, 1));
        assert_eq!(s.groups[ColorGroup::White.index()], 3);
        assert_eq!(s.matrix[ColorGroup::White.index()][2], 2);
        let removal = ROLES.iter().position(|(k, _)| *k == "removal").unwrap();
        assert_eq!(s.roles[ColorGroup::White.index()][removal], 2);
        assert_eq!(s.combos[0], 1);
        assert_eq!(s.curve[ColorGroup::White.index()][0], (1, 2));
        assert!((s.avg_mv - 5.0 / 4.0).abs() < 1e-6);
    }
}
