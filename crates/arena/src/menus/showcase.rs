//! Art showcase behind the Boot and Loading screens: famous card artworks shown full-screen with
//! a slow Ken Burns drift, crossfading every few seconds, with a placard naming the card and its
//! artist plus a short fact.
//!
//! Why it's a separate layer (GlobalZIndex between the backdrop and the screens): Boot and Loading
//! keep their own widgets and despawn on exit, while the reel keeps going across both. Why a
//! thread + bounded(1) channel: `mtg_client::scryfall::Showcase` blocks on disk and JPEG
//! decoding is CPU work. The thread stays one image ahead and then waits, so only what's on screen
//! (plus the next one) is held in memory. If the offline pack has no showcase art, the layer never
//! appears and the animated backdrop shows as before.

use std::time::{SystemTime, UNIX_EPOCH};

use bevy::asset::RenderAssetUsages;
use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::ui::{BackgroundGradient, ColorStop, LinearGradient};
use bevy::window::PrimaryWindow;
use crossbeam_channel::{Receiver, bounded};
use mtg_client::scryfall::{Showcase, Showpiece};

use crate::shared::{AppState, ArenaFonts};
use crate::style::*;

/// Seconds each artwork stays before the next one may replace it.
const DWELL: f32 = 9.0;
const CROSSFADE: f32 = 1.4;
const CAPTION_OUT: f32 = 0.45;
const CAPTION_IN: f32 = 0.9;

pub struct ShowcasePlugin;

impl Plugin for ShowcasePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Reel>()
            .add_systems(Startup, (start_feed, spawn_layer))
            .add_systems(Update, (toggle_layer, (advance_reel, animate_layer).chain().run_if(showcase_visible_state)).chain());
    }
}

fn showcase_visible_state(state: Res<State<AppState>>) -> bool {
    matches!(state.get(), AppState::Boot | AppState::Loading)
}

#[derive(Resource)]
struct Feed(Receiver<(Showpiece, Image)>);

struct Pending {
    piece: Showpiece,
    image: Handle<Image>,
    size: Vec2,
}

#[derive(Resource, Default)]
struct Reel {
    /// Which of the two art slots is on top.
    active: usize,
    switched_at: f32,
    /// Set while the old caption fades out; the actual swap happens when it finishes.
    swapping_since: Option<f32>,
    pending: Option<Pending>,
    shown: bool,
}

#[derive(Component)]
struct ShowcaseRoot;

#[derive(Component)]
struct ArtSlot {
    idx: usize,
    /// false until it gets its first art (an empty ImageNode would draw solid white)
    filled: bool,
    born: f32,
    size: Vec2,
    /// Ken Burns pan direction (px over one dwell)
    drift: Vec2,
}

#[derive(Component, Clone, Copy, PartialEq)]
enum Part {
    Name,
    Credit,
    Set,
    Blurb,
    Stat,
}

/// A caption element whose alpha follows the reel; `.1` is its full-opacity color.
#[derive(Component)]
struct Caption(Option<Part>, Color);

fn start_feed(mut commands: Commands) {
    let (tx, rx) = bounded(1);
    let seed = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(7);
    std::thread::Builder::new()
        .name("showcase".into())
        .spawn(move || {
            let mut seed = seed;
            // Loop with a fresh order each round so a very long boot never runs out of art.
            loop {
                let mut any = false;
                for piece in Showcase::new(seed) {
                    let Ok(img) = decode(&piece.art, piece.year.as_str() < "2003") else { continue };
                    any = true;
                    if tx.send((piece, img)).is_err() {
                        return;
                    }
                }
                if !any {
                    return;
                }
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            }
        })
        .expect("spawn showcase thread");
    commands.insert_resource(Feed(rx));
}

fn decode(bytes: &[u8], descreen: bool) -> Result<Image, String> {
    let mut img = Image::from_buffer(bytes, ImageType::MimeType("image/jpeg"), CompressedImageFormats::NONE, true, ImageSampler::linear(), RenderAssetUsages::RENDER_WORLD)
        .map_err(|e| e.to_string())?;
    if descreen && img.texture_descriptor.format == TextureFormat::Rgba8UnormSrgb {
        let (w, h) = (img.width() as usize, img.height() as usize);
        if let Some(data) = img.data.as_mut() {
            blur(data, w, h);
        }
    }
    Ok(img)
}

/// Early art crops are scans of *printed* cards; blown up ~3× for a full screen, their halftone
/// dots turn into a noisy crosshatch. One separable [1 4 6 4 1] pass removes the screen pattern
/// and leaves the painting intact.
fn blur(px: &mut [u8], w: usize, h: usize) {
    if px.len() != w * h * 4 {
        return;
    }
    const K: [u32; 5] = [1, 4, 6, 4, 1];
    // `step` walks along the blur axis: 4 bytes for horizontal, one row for vertical.
    let pass = |src: &[u8], dst: &mut [u8], len: usize, lines: usize, step: usize, stride: usize| {
        for line in 0..lines {
            for i in 0..len {
                let base = line * stride;
                for ch in 0..3 {
                    let acc: u32 = K.iter().enumerate().map(|(k, wgt)| wgt * src[base + (i + k).saturating_sub(2).min(len - 1) * step + ch] as u32).sum();
                    dst[base + i * step + ch] = (acc / 16) as u8;
                }
            }
        }
    };
    let mut tmp = px.to_vec();
    pass(px, &mut tmp, w, h, 4, w * 4);
    pass(&tmp, px, h, w, w * 4, 4);
}

fn full() -> Node {
    Node { position_type: PositionType::Absolute, left: Val::Px(0.0), top: Val::Px(0.0), width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() }
}

fn shade(a: f32) -> Color {
    Color::srgba(0.02, 0.012, 0.008, a)
}

fn spawn_layer(mut commands: Commands, fonts: Res<ArenaFonts>) {
    let shadow = TextShadow { offset: Vec2::new(0.0, 2.0), color: Color::srgba(0.0, 0.0, 0.0, 0.9) };
    commands
        .spawn((Name::new("ArtShowcase"), ShowcaseRoot, Node { display: Display::None, overflow: Overflow::clip(), ..full() }, GlobalZIndex(-45)))
        .with_children(|root| {
            for idx in 0..2 {
                root.spawn((
                    ArtSlot { idx, filled: false, born: 0.0, size: Vec2::ONE, drift: Vec2::ZERO },
                    Node { position_type: PositionType::Absolute, ..default() },
                    ImageNode { color: Color::NONE, image_mode: NodeImageMode::Stretch, ..default() },
                    UiTransform::IDENTITY,
                    ZIndex(idx as i32),
                ));
            }
            // Scrims: top for the logo, bottom + left for the placard, so text stays readable over any painting.
            root.spawn((
                Node { height: Val::Percent(32.0), ..full() },
                ZIndex(10),
                BackgroundGradient::from(LinearGradient::to_bottom(vec![ColorStop::new(shade(0.72), Val::Percent(0.0)), ColorStop::new(shade(0.0), Val::Percent(100.0))])),
            ));
            root.spawn((
                Node { top: Val::Auto, bottom: Val::Px(0.0), height: Val::Percent(58.0), ..full() },
                ZIndex(10),
                BackgroundGradient::from(LinearGradient::to_bottom(vec![ColorStop::new(shade(0.0), Val::Percent(0.0)), ColorStop::new(shade(0.92), Val::Percent(100.0))])),
            ));
            root.spawn((
                Node { width: Val::Percent(55.0), ..full() },
                ZIndex(10),
                BackgroundGradient::from(LinearGradient::to_right(vec![ColorStop::new(shade(0.55), Val::Percent(0.0)), ColorStop::new(shade(0.0), Val::Percent(100.0))])),
            ));
            // placard
            root.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(56.0),
                    bottom: Val::Px(48.0),
                    max_width: Val::Px(660.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(4.0),
                    ..default()
                },
                ZIndex(20),
            ))
            .with_children(|p| {
                p.spawn((Caption(None, GOLD), ui_text(&fonts.numbers, "FEATURED  ARTWORK", 13.0, GOLD), shadow));
                p.spawn((Caption(Some(Part::Name), GOLD_BRIGHT), ui_text(&fonts.title, "", 40.0, GOLD_BRIGHT), shadow));
                p.spawn((Caption(Some(Part::Credit), TEXT), ui_text(&fonts.body, "", 19.0, TEXT), shadow));
                p.spawn((Caption(Some(Part::Set), TEXT_DIM), ui_text(&fonts.body, "", 15.0, TEXT_DIM), shadow));
                p.spawn((Caption(None, GOLD), TextColor::default(), Node { width: Val::Px(120.0), height: Val::Px(2.0), margin: UiRect::vertical(Val::Px(10.0)), ..default() }, BackgroundColor(GOLD)));
                p.spawn((Caption(Some(Part::Blurb), PARCHMENT), ui_text(&fonts.body, "", 18.0, PARCHMENT), shadow));
                p.spawn((Caption(Some(Part::Stat), TEXT_DIM), ui_text(&fonts.body, "", 14.0, TEXT_DIM), shadow, Node { margin: UiRect::top(Val::Px(6.0)), ..default() }));
            });
        });
}

fn advance_reel(
    time: Res<Time>,
    feed: Option<Res<Feed>>,
    mut reel: ResMut<Reel>,
    mut images: ResMut<Assets<Image>>,
    mut slots: Query<(&mut ArtSlot, &mut ImageNode)>,
    mut texts: Query<(&Caption, &mut Text)>,
) {
    let now = time.elapsed_secs();
    let due = !reel.shown || now - reel.switched_at >= DWELL;
    if reel.pending.is_none()
        && due
        && let Some(feed) = feed
        && let Ok((piece, img)) = feed.0.try_recv()
    {
        let size = img.size_f32();
        reel.pending = Some(Pending { piece, image: images.add(img), size });
    }
    if reel.pending.is_some() && due && reel.swapping_since.is_none() {
        // nothing on screen yet → no caption to fade out first
        reel.swapping_since = Some(if reel.shown { now } else { now - CAPTION_OUT });
    }
    let Some(since) = reel.swapping_since else { return };
    if now - since < CAPTION_OUT {
        return;
    }
    let Some(p) = reel.pending.take() else { return };
    reel.swapping_since = None;
    reel.active = if reel.shown { 1 - reel.active } else { 0 };
    reel.switched_at = now;
    reel.shown = true;
    let active = reel.active;
    for (mut slot, mut node) in &mut slots {
        if slot.idx == active {
            // Alternate pan directions so consecutive arts don't all slide the same way.
            let k = (now * 7.31).sin();
            slot.drift = Vec2::new(if k > 0.0 { 1.0 } else { -1.0 } * 28.0, -14.0 * k.signum());
            slot.born = now;
            slot.size = p.size;
            slot.filled = true;
            // Replacing the handle drops the previous art two swaps back, so at most two stay loaded.
            node.image = p.image.clone();
        }
    }
    let piece = &p.piece;
    for (cap, mut text) in &mut texts {
        let Some(part) = cap.0 else { continue };
        text.0 = match part {
            Part::Name => piece.name.clone(),
            Part::Credit => format!("Illustrated by {}", piece.artist),
            Part::Set => format!("{}  ·  {}", piece.set_name, piece.year),
            Part::Blurb => piece.blurb.clone().unwrap_or_default(),
            Part::Stat => piece.stat.clone().unwrap_or_default(),
        };
    }
}

/// Shown only in Boot/Loading and only once there is art; in the main menu the card-back backdrop
/// (and its menu) would fight with the placard.
fn toggle_layer(state: Res<State<AppState>>, reel: Res<Reel>, mut root: Query<&mut Node, With<ShowcaseRoot>>) {
    let want = if reel.shown && matches!(state.get(), AppState::Boot | AppState::Loading) { Display::Flex } else { Display::None };
    for mut n in &mut root {
        if n.display != want {
            n.display = want;
        }
    }
}

fn smooth(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

fn animate_layer(
    time: Res<Time>,
    reel: Res<Reel>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut slots: Query<(&ArtSlot, &mut Node, &mut ImageNode, &mut UiTransform, &mut ZIndex)>,
    mut captions: Query<(&Caption, Has<Text>, &mut TextColor, &mut BackgroundColor)>,
) {
    if !reel.shown {
        return;
    }
    let Ok(w) = window.single() else { return };
    let (ww, wh) = (w.width(), w.height());
    let now = time.elapsed_secs();
    let since_switch = now - reel.switched_at;
    for (slot, mut node, mut img, mut tf, mut z) in &mut slots {
        let on_top = slot.idx == reel.active;
        // Old art stays fully opaque underneath until the new one has faded in — no dark dip.
        let a = if !slot.filled {
            0.0
        } else if on_top { smooth((now - slot.born) / CROSSFADE) } else if since_switch < CROSSFADE { 1.0 } else { 0.0 };
        img.color = Color::srgba(1.0, 1.0, 1.0, a);
        *z = ZIndex(if on_top { 1 } else { 0 });
        // object-fit: cover
        let scale = (ww / slot.size.x).max(wh / slot.size.y);
        let (iw, ih) = (slot.size.x * scale, slot.size.y * scale);
        node.width = Val::Px(iw);
        node.height = Val::Px(ih);
        node.left = Val::Px((ww - iw) * 0.5);
        node.top = Val::Px((wh - ih) * 0.5);
        let p = ((now - slot.born) / (DWELL + CROSSFADE + CAPTION_OUT)).clamp(0.0, 1.0);
        tf.scale = Vec2::splat(1.05 + 0.09 * p);
        tf.translation = Val2::px(slot.drift.x * p, slot.drift.y * p);
    }
    let alpha = match reel.swapping_since {
        Some(s) => 1.0 - smooth((now - s) / CAPTION_OUT),
        None => smooth((since_switch - 0.35) / CAPTION_IN),
    };
    for (cap, is_text, mut tc, mut bg) in &mut captions {
        let c = cap.1.with_alpha(cap.1.alpha() * alpha);
        // Every UI node carries a (transparent) BackgroundColor; only the gold rule is drawn with it.
        if is_text { tc.0 = c } else { bg.0 = c }
    }
}
