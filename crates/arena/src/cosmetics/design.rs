//! Cosmetic designs: the data behind sleeves, deckboxes and playmats, plus the built-in presets.
//!
//! Why data instead of images: a design is a handful of choices (pattern, three colours, emblem,
//! frame, finish) that `paint` turns into textures at any resolution. The saved file stays tiny,
//! the studio can preview every edit live, and deck tiles get crisp thumbnails for free. Card art
//! stays data too: a design names the card, and the painter fetches its art crop.

use std::hash::{DefaultHasher, Hash, Hasher};

use serde::{Deserialize, Serialize};

pub type Rgb = [u8; 3];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DesignKind {
    #[default]
    Sleeve,
    Deckbox,
    Playmat,
}

impl DesignKind {
    pub const ALL: [DesignKind; 3] = [DesignKind::Sleeve, DesignKind::Deckbox, DesignKind::Playmat];
    pub fn label(self) -> &'static str {
        match self {
            DesignKind::Sleeve => "SLEEVES",
            DesignKind::Deckbox => "DECKBOXES",
            DesignKind::Playmat => "PLAYMATS",
        }
    }
    pub fn singular(self) -> &'static str {
        match self {
            DesignKind::Sleeve => "sleeve",
            DesignKind::Deckbox => "deckbox",
            DesignKind::Playmat => "playmat",
        }
    }
}

/// Generates `ALL` and `label()` for the small option enums the studio shows as chips.
macro_rules! options {
    ($name:ident { $($v:ident => $label:literal),+ $(,)? } default $d:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
        #[serde(rename_all = "lowercase")]
        pub enum $name {
            #[default]
            $d,
            $($v),+
        }
        impl $name {
            pub const ALL: &'static [$name] = &[$name::$d, $($name::$v),+];
            pub fn label(self) -> &'static str {
                match self {
                    $name::$d => stringify!($d),
                    $($name::$v => $label),+
                }
            }
        }
    };
}

options!(Pattern {
    Swirl => "Swirl",
    Sunburst => "Sunburst",
    Filigree => "Filigree",
    Stripes => "Stripes",
    Hexes => "Hexes",
    Scales => "Scales",
    Nebula => "Nebula",
    Marble => "Marble",
} default Gradient);

options!(Emblem {
    Star => "Star",
    Sun => "Sun",
    Moon => "Moon",
    Drop => "Drop",
    Flame => "Flame",
    Leaf => "Leaf",
    Diamond => "Diamond",
    Spark => "Spark",
} default None);

options!(Frame { Thin => "Thin", Ornate => "Ornate" } default None);

options!(Finish { Gloss => "Gloss", Foil => "Foil" } default Matte);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Design {
    pub id: String,
    pub name: String,
    pub kind: DesignKind,
    pub pattern: Pattern,
    /// Ground colour (sRGB).
    pub base: Rgb,
    /// Colour of the pattern's body.
    pub detail: Rgb,
    /// Highlights, frame lines and the emblem.
    pub accent: Rgb,
    pub emblem: Emblem,
    pub frame: Frame,
    pub finish: Finish,
    /// Pattern scale: 0.5 = fine, 2.0 = bold.
    pub scale: f32,
    pub seed: u32,
    /// A card whose art crop is printed where the pattern would be (sleeve back, deckbox front and
    /// back, playmat); frame, colours, emblem and finish still go on top.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub art: Option<String>,
    /// Shipped artwork used instead of painting (asset path). Presets only, never saved.
    #[serde(skip)]
    pub image: Option<&'static str>,
    #[serde(skip)]
    pub builtin: bool,
}

impl Default for Design {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: "Untitled".into(),
            kind: DesignKind::Sleeve,
            pattern: Pattern::Swirl,
            base: [0x2a, 0x12, 0x06],
            detail: [0xc4, 0x56, 0x1a],
            accent: [0xf2, 0xc4, 0x6b],
            emblem: Emblem::Star,
            frame: Frame::Thin,
            finish: Finish::Matte,
            scale: 1.0,
            seed: 1,
            art: None,
            image: None,
            builtin: false,
        }
    }
}

impl Design {
    /// Changes whenever anything that affects the painted texture changes (not the name).
    pub fn fingerprint(&self) -> u64 {
        let mut h = DefaultHasher::new();
        (self.kind, self.pattern, self.base, self.detail, self.accent, self.emblem, self.frame, self.finish).hash(&mut h);
        (self.scale.to_bits(), self.seed, &self.art, self.image).hash(&mut h);
        h.finish()
    }

    /// Shuffle everything but the kind into a fresh, colour-coordinated design.
    pub fn randomize(&mut self, seed: u32) {
        let mut r = Lcg(seed | 1);
        let pick = |r: &mut Lcg, n: usize| (r.next() * n as f32) as usize % n;
        self.pattern = Pattern::ALL[pick(&mut r, Pattern::ALL.len())];
        // never "None" for the emblem: a random design should have a focal point
        self.emblem = Emblem::ALL[1 + pick(&mut r, Emblem::ALL.len() - 1)];
        self.frame = Frame::ALL[pick(&mut r, Frame::ALL.len())];
        self.finish = Finish::ALL[pick(&mut r, Finish::ALL.len())];
        let (dark, mid, bright) = palette_tiers();
        self.base = dark[pick(&mut r, dark.len())];
        self.detail = mid[pick(&mut r, mid.len())];
        self.accent = bright[pick(&mut r, bright.len())];
        self.scale = 0.7 + r.next() * 0.9;
        self.seed = (r.next() * 1e6) as u32;
    }
}

/// Swatches offered by the studio, roughly dark → bright.
pub const PALETTE: [Rgb; 16] = [
    [0x0b, 0x0b, 0x0d],
    [0x2a, 0x2a, 0x30],
    [0x3b, 0x21, 0x12],
    [0x0d, 0x24, 0x12],
    [0x0b, 0x1e, 0x3a],
    [0x3b, 0x1d, 0x6e],
    [0x5e, 0x0b, 0x12],
    [0xa3, 0x16, 0x1a],
    [0xc4, 0x56, 0x1a],
    [0xb0, 0x7a, 0x2a],
    [0x2f, 0x7a, 0x3a],
    [0x2a, 0x7f, 0xb8],
    [0x6f, 0xd3, 0xe0],
    [0xb4, 0x8c, 0xff],
    [0xf2, 0xc4, 0x6b],
    [0xe8, 0xdc, 0xc0],
];

fn palette_tiers() -> (&'static [Rgb], &'static [Rgb], &'static [Rgb]) {
    (&PALETTE[0..7], &PALETTE[6..13], &PALETTE[11..16])
}

const fn hex(v: u32) -> Rgb {
    [(v >> 16) as u8, (v >> 8) as u8, v as u8]
}

#[allow(clippy::too_many_arguments)]
fn preset(kind: DesignKind, id: &str, name: &str, pattern: Pattern, cols: [u32; 3], emblem: Emblem, frame: Frame, finish: Finish, scale: f32) -> Design {
    Design {
        id: id.into(),
        name: name.into(),
        kind,
        pattern,
        base: hex(cols[0]),
        detail: hex(cols[1]),
        accent: hex(cols[2]),
        emblem,
        frame,
        finish,
        scale,
        seed: id.bytes().fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32)),
        art: None,
        image: None,
        builtin: true,
    }
}

/// Path of the classic card back; `Paints` maps it to `CardImages::card_back`, which falls back to
/// a procedural back when the file is missing.
pub const CLASSIC_BACK: &str = "textures/card_back.png";

pub fn presets() -> Vec<Design> {
    use DesignKind::*;
    use Emblem as E;
    use Finish as F;
    use Frame as Fr;
    use Pattern as P;
    let with_image = |mut d: Design, path: &'static str| {
        d.image = Some(path);
        d
    };
    vec![
        // ---- sleeves
        with_image(preset(Sleeve, "classic", "Classic", P::Swirl, [0x2a1206, 0xc4561a, 0xf2c46b], E::Spark, Fr::Ornate, F::Gloss, 1.0), CLASSIC_BACK),
        preset(Sleeve, "ember-swirl", "Ember Swirl", P::Swirl, [0x2a1206, 0xc4561a, 0xf2c46b], E::Spark, Fr::Ornate, F::Gloss, 1.0),
        preset(Sleeve, "azure-tides", "Azure Tides", P::Marble, [0x0b1e3a, 0x2a7fb8, 0xcfe8ff], E::Drop, Fr::Thin, F::Gloss, 1.0),
        preset(Sleeve, "dragonscale", "Dragonscale", P::Scales, [0x3a0707, 0xa3161a, 0xf0b54a], E::Flame, Fr::Ornate, F::Foil, 1.0),
        preset(Sleeve, "verdant", "Verdant Filigree", P::Filigree, [0x0d2412, 0x2f7a3a, 0xd9c37a], E::Leaf, Fr::Ornate, F::Matte, 1.0),
        preset(Sleeve, "void-lattice", "Void Lattice", P::Hexes, [0x08060f, 0x3b1d6e, 0xb48cff], E::Moon, Fr::Thin, F::Gloss, 1.0),
        preset(Sleeve, "celestial", "Celestial", P::Nebula, [0x050818, 0x3a2a7a, 0xfff2c8], E::Star, Fr::Thin, F::Foil, 1.0),
        preset(Sleeve, "dawnbreak", "Dawnbreak", P::Sunburst, [0xf1e6c8, 0xd9a441, 0x8a5a14], E::Sun, Fr::Ornate, F::Gloss, 1.0),
        preset(Sleeve, "guild-banner", "Guild Banner", P::Stripes, [0x1a1a1f, 0x8c1c24, 0xd8b45a], E::Diamond, Fr::Ornate, F::Matte, 1.0),
        // ---- deckboxes
        preset(Deckbox, "leather-tome", "Leather Tome", P::Gradient, [0x3b2112, 0x6b3f20, 0xd4a64a], E::Diamond, Fr::Ornate, F::Matte, 1.0),
        preset(Deckbox, "dragonhide", "Dragonhide", P::Scales, [0x2b0505, 0x8e1414, 0xe9b04c], E::Flame, Fr::Ornate, F::Gloss, 1.2),
        preset(Deckbox, "arcane-vault", "Arcane Vault", P::Hexes, [0x081626, 0x1f5f9e, 0x9fe3ff], E::Spark, Fr::Thin, F::Foil, 1.0),
        preset(Deckbox, "grove-chest", "Grove Chest", P::Filigree, [0x10230f, 0x3c6e2a, 0xe3cf8a], E::Leaf, Fr::Ornate, F::Matte, 1.0),
        preset(Deckbox, "starlit", "Starlit Case", P::Nebula, [0x04050f, 0x2b1f5c, 0xffe9b0], E::Star, Fr::Thin, F::Gloss, 1.0),
        preset(Deckbox, "obsidian", "Obsidian Marble", P::Marble, [0x0b0b0d, 0x2a2a30, 0xd8b45a], E::Moon, Fr::Ornate, F::Gloss, 1.0),
        preset(Deckbox, "sunforged", "Sunforged", P::Sunburst, [0xe8dcc0, 0xc79a3a, 0x6e4410], E::Sun, Fr::Ornate, F::Gloss, 1.0),
        // ---- playmats (the first three are the original battlefield themes)
        with_image(preset(Playmat, "sanctum", "Sanctum", P::Swirl, [0x331a08, 0x8c5214, 0xf2c25a], E::Sun, Fr::Ornate, F::Matte, 1.4), "textures/playmat_sanctum.png"),
        with_image(preset(Playmat, "jungle", "Jungle", P::Filigree, [0x07140a, 0x1d4424, 0xb8c070], E::Leaf, Fr::Ornate, F::Matte, 1.4), "textures/playmat_jungle.png"),
        with_image(preset(Playmat, "storm", "Storm", P::Marble, [0x0a1012, 0x2a3a3c, 0x8fb3c0], E::Spark, Fr::Thin, F::Matte, 1.4), "textures/playmat_storm.png"),
        preset(Playmat, "astral-sea", "Astral Sea", P::Nebula, [0x03040c, 0x1d1846, 0xc8b8ff], E::Star, Fr::Thin, F::Matte, 1.3),
        preset(Playmat, "dragons-lair", "Dragon's Lair", P::Scales, [0x140404, 0x4a0d0a, 0xd08a3a], E::Flame, Fr::Ornate, F::Matte, 1.6),
        preset(Playmat, "ancient-grove", "Ancient Grove", P::Filigree, [0x07140a, 0x1d4424, 0xb8c070], E::Leaf, Fr::Ornate, F::Matte, 1.4),
        preset(Playmat, "marble-hall", "Marble Hall", P::Marble, [0x1c1a18, 0x4a4540, 0xc9a65a], E::Sun, Fr::Ornate, F::Gloss, 1.3),
        preset(Playmat, "tidal-depths", "Tidal Depths", P::Swirl, [0x020b14, 0x0e3b52, 0x6fd3e0], E::Drop, Fr::Thin, F::Matte, 1.5),
    ]
}

/// Tiny deterministic LCG; designs only need repeatable variety, not real randomness.
pub(crate) struct Lcg(pub u32);
impl Lcg {
    pub fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        (self.0 >> 8) as f32 / (1u32 << 24) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_ids_are_unique_and_every_kind_has_some() {
        let p = presets();
        let mut ids: Vec<_> = p.iter().map(|d| d.id.as_str()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), p.len());
        for k in DesignKind::ALL {
            assert!(p.iter().any(|d| d.kind == k));
        }
    }

    #[test]
    fn fingerprint_ignores_the_name_only() {
        let a = presets()[1].clone();
        let mut b = a.clone();
        b.name = "Renamed".into();
        assert_eq!(a.fingerprint(), b.fingerprint());
        b.accent = [1, 2, 3];
        assert_ne!(a.fingerprint(), b.fingerprint());
    }

    #[test]
    fn custom_designs_round_trip_without_preset_fields() {
        let mut d = presets()[0].clone();
        d.builtin = false;
        let json = serde_json::to_string(&d).unwrap();
        assert!(!json.contains("image"));
        let back: Design = serde_json::from_str(&json).unwrap();
        assert_eq!(back.pattern, d.pattern);
        assert_eq!(back.image, None);
    }
}
