//! Avatars: player portraits cut from card art (see `mtg_protocol::Avatar`).
//!
//! Why a component instead of handing out cropped images: the HUD, the main-menu chip, the lobby
//! and the avatar studio all show portraits, and each one changes them at runtime (a new pick, a
//! crop being dragged). With [`AvatarArt`] on an `ImageNode` they only say *which* avatar; this
//! module fetches the art crop through `CardImages` and points the node's `rect` at the circle's
//! square, so a crop edit costs nothing but a rect change — no image is ever re-encoded.

use bevy::prelude::*;
use mtg_client::{ImageRequest, ImageVersion};
use mtg_protocol::Avatar;

use crate::images::CardImages;
use crate::shared::AvatarArt;

pub struct AvatarsPlugin;

impl Plugin for AvatarsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostUpdate, show_avatar_art.before(bevy::ui::UiSystems::Prepare));
    }
}

/// Characters worth wearing: planeswalkers first, then the legends players know by face. The
/// studio's first tab; any other card is a search away.
pub const FEATURED: &[&str] = &[
    "Jace, the Mind Sculptor",
    "Liliana of the Veil",
    "Chandra, Torch of Defiance",
    "Nissa, Who Shakes the World",
    "Gideon, Ally of Zendikar",
    "Ajani Goldmane",
    "Sorin, Lord of Innistrad",
    "Nicol Bolas, Dragon-God",
    "Teferi, Hero of Dominaria",
    "Karn Liberated",
    "Ugin, the Spirit Dragon",
    "Elspeth, Sun's Champion",
    "Garruk Wildspeaker",
    "Vraska, Golgari Queen",
    "Tamiyo, the Moon Sage",
    "Kaya, Ghost Assassin",
    "Ral Zarek",
    "Tezzeret, Agent of Bolas",
    "Sarkhan Vol",
    "Nahiri, the Harbinger",
    "Narset, Parter of Veils",
    "Kiora, the Crashing Wave",
    "Ashiok, Nightmare Weaver",
    "Vivien Reid",
    "Saheeli Rai",
    "Wrenn and Six",
    "Oko, Thief of Crowns",
    "The Wanderer",
    "Tibalt, the Fiend-Blooded",
    "Ob Nixilis Reignited",
    "Dovin Baan",
    "Jaya Ballard",
    "Daretti, Scrap Savant",
    "Freyalise, Llanowar's Fury",
    "Estrid, the Masked",
    "Urza, Lord High Artificer",
    "Yawgmoth, Thran Physician",
    "Gix, Yawgmoth Praetor",
    "Avacyn, Angel of Hope",
    "Olivia Voldaren",
    "Edgar Markov",
    "Atraxa, Praetors' Voice",
    "Elesh Norn, Grand Cenobite",
    "Sheoldred, the Apocalypse",
    "Griselbrand",
    "Emrakul, the Aeons Torn",
    "Kozilek, Butcher of Truth",
    "Ulamog, the Infinite Gyre",
    "Krenko, Mob Boss",
    "Kenrith, the Returned King",
    "Niv-Mizzet, Parun",
    "Rakdos, Lord of Riots",
    "Kaalia of the Vast",
    "Meren of Clan Nel Toth",
    "The Ur-Dragon",
    "Korvold, Fae-Cursed King",
    "Jodah, Archmage Eternal",
    "Zur the Enchanter",
    "Omnath, Locus of Creation",
    "Muldrotha, the Gravetide",
    "Raffine, Scheming Seer",
    "Lord Windgrace",
    "Yuriko, the Tiger's Shadow",
    "Animar, Soul of Elements",
    "Chulane, Teller of Tales",
    "Isshin, Two Heavens as One",
    "Light-Paws, Emperor's Voice",
    "Breya, Etherium Shaper",
    "Minsc & Boo, Timeless Heroes",
    "Tiamat",
    "Lathliss, Dragon Queen",
    "Akroma, Angel of Wrath",
    "Kamahl, Pit Fighter",
    "Gerrard Capashen",
    "Sisay, Weatherlight Captain",
    "Squee, Goblin Nabob",
    "Mirri, Cat Warrior",
    "Crovax the Cursed",
    "Volrath the Fallen",
    "Karn, Silver Golem",
    "Thalia, Guardian of Thraben",
    "Kiki-Jiki, Mirror Breaker",
    "Brimaz, King of Oreskos",
    "Drana, Kalastria Bloodchief",
    "Baral, Chief of Compliance",
    "Feather, the Redeemed",
    "Marchesa, the Black Rose",
    "Queen Marchesa",
    "Teysa Karlov",
    "Aurelia, the Warleader",
    "Lazav, the Multifarious",
    "Tasigur, the Golden Fang",
    "Alesha, Who Smiles at Death",
    "Narset, Enlightened Master",
    "Zurgo Helmsmasher",
    "Anafenza, the Foremost",
    "Kolaghan, the Storm's Fury",
    "Selvala, Heart of the Wilds",
    "Rankle, Master of Pranks",
    "Syr Konrad, the Grim",
    "Grand Arbiter Augustin IV",
    "Arcades, the Strategist",
    "Ezuri, Renegade Leader",
];

/// The art-crop request for a card (the offline pack has one printing per name, so no set).
pub fn art_request(card: &str) -> ImageRequest {
    ImageRequest { name: card.to_string(), set: None, back: false, version: ImageVersion::ArtCrop }
}

/// Pixel size of a card's art crop once it has loaded (the crop editor needs the aspect).
pub fn art_size(card_images: &mut CardImages, images: &Assets<Image>, card: &str) -> Option<Vec2> {
    let h = card_images.request_raw(art_request(card));
    images.get(&h).map(|i| i.size().as_vec2())
}

/// Square of the art the avatar's circle covers.
pub fn crop_rect(a: &Avatar, size: Vec2) -> Rect {
    let [x0, y0, x1, y1] = a.crop(size.x, size.y);
    Rect::new(x0, y0, x1, y1)
}

fn show_avatar_art(mut card_images: ResMut<CardImages>, images: Res<Assets<Image>>, mut q: Query<(&AvatarArt, &mut ImageNode)>) {
    for (art, mut node) in &mut q {
        let shown = art.avatar.as_ref().filter(|a| !a.card.is_empty()).and_then(|a| {
            let h = card_images.request_raw(art_request(&a.card));
            let size = images.get(&h)?.size().as_vec2();
            Some((h, crop_rect(a, size)))
        });
        let (image, rect) = match shown {
            Some((h, r)) => (h, Some(r)),
            None => (art.fallback.clone(), None),
        };
        // only touch the node on a real change: change detection drives UI re-layout
        if node.image != image || node.rect != rect {
            node.image = image;
            node.rect = rect;
        }
    }
}
