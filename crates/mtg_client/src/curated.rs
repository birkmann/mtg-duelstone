//! The curated artworks behind the splash showcase and the main-menu carousel — plain data, so the
//! browser build can use it too (the Scryfall downloader in `scryfall.rs` is desktop-only).

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
