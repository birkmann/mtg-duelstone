//! Card image cache.
//!
//! CONTRACT: `CardImages::request(&card, version)` returns a `Handle<Image>` immediately (a
//! reserved handle that is filled when the bytes arrive from the engine's offline image pack);
//! `CardImages::card_back` is always valid. `Assets<Image>::contains(handle)` tells whether the
//! real image has arrived yet (until then, draw `card_blank` instead).
//!
//! Why tasks: fetching + JPEG decoding + mip generation are blocking CPU/IO work; each request runs
//! as an `mtg_client::spawn` task (a worker thread on desktop, the page's event loop in the browser)
//! and hands the finished `Image` back over a channel, so the main schedule only ever inserts into
//! `Assets<Image>`. At most `MAX_IN_FLIGHT` requests run at once.

use std::collections::{HashMap, HashSet};
use std::f32::consts::TAU;
use std::path::PathBuf;

use bevy::asset::{AssetHandleProvider, RenderAssetUsages};
use bevy::image::{
    CompressedImageFormats, ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor, ImageType,
};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use crossbeam_channel::{Receiver, Sender, unbounded};
use mtg_client::{Api, ImageRequest, ImageVersion};
use mtg_protocol::Card;

use crate::shared::{AppState, EngineConfig};

/// Directory holding `textures/`, `fonts/`, … — works from the repo root or the crate dir.
pub fn assets_dir() -> PathBuf {
    // A packaged build must use its own copy, even when built on this machine from a checkout.
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(p) = mtg_client::launcher::find_bundled("assets") {
        return p;
    }
    let compiled = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets");
    if compiled.exists() {
        return compiled;
    }
    for p in ["crates/arena/assets", "assets"] {
        let p = PathBuf::from(p);
        if p.exists() {
            return p;
        }
    }
    compiled
}

pub struct ImagesPlugin;

impl Plugin for ImagesPlugin {
    fn build(&self, app: &mut App) {
        let provider = app.world().resource::<Assets<Image>>().get_handle_provider();
        let mut ci = CardImages::new(provider);
        // A real card back from the polish pass wins over the procedural one (the web build always ships it).
        let file = assets_dir().join("textures/card_back.png");
        if crate::platform::WEB || file.exists() {
            ci.card_back = app.world().resource::<AssetServer>().load("textures/card_back.png");
        } else {
            ci.pending_procedural_back = true;
        }
        app.insert_resource(ci)
            .add_systems(Startup, init_procedural)
            .add_systems(PreUpdate, (pump_requests, receive_images).chain());
    }
}

type Decoded = (ImageRequest, Result<Image, String>);

#[derive(Resource)]
pub struct CardImages {
    pub card_back: Handle<Image>,
    /// Neutral blank card face shown while the real image is still downloading.
    pub card_blank: Handle<Image>,
    pub handles: HashMap<ImageRequest, Handle<Image>>,
    /// Requests not yet started.
    pub queue: Vec<ImageRequest>,
    provider: AssetHandleProvider,
    pending_procedural_back: bool,
    done_tx: Sender<Decoded>,
    done_rx: Receiver<Decoded>,
    in_flight: usize,
    failed: HashSet<ImageRequest>,
}

impl CardImages {
    fn new(provider: AssetHandleProvider) -> Self {
        let (done_tx, done_rx) = unbounded::<Decoded>();
        Self {
            card_back: provider.reserve_handle().typed(),
            card_blank: provider.reserve_handle().typed(),
            handles: HashMap::new(),
            queue: Vec::new(),
            provider,
            pending_procedural_back: false,
            done_tx,
            done_rx,
            in_flight: 0,
            failed: HashSet::new(),
        }
    }

    /// Handle for this card's face image (placeholder until loaded). Hidden cards → card back.
    pub fn request(&mut self, card: &Card, version: ImageVersion) -> Handle<Image> {
        let Some(state) = card.state.as_ref().filter(|_| !card.hidden) else {
            return self.card_back.clone();
        };
        let req = ImageRequest {
            name: if card.oracle_name.is_empty() { state.name.clone() } else { card.oracle_name.clone() },
            set: state.set_code.clone(),
            back: false,
            version,
        };
        self.request_raw(req)
    }

    pub fn request_raw(&mut self, req: ImageRequest) -> Handle<Image> {
        if let Some(h) = self.handles.get(&req) {
            return h.clone();
        }
        self.queue.push(req.clone());
        let h: Handle<Image> = self.provider.reserve_handle().typed();
        self.handles.insert(req, h.clone());
        h
    }

    /// Number of images requested but not yet arrived.
    pub fn pending(&self) -> usize {
        self.queue.len() + self.in_flight
    }

    fn start_fetch(&mut self, base: &str, req: ImageRequest) {
        let api = Api::new(base);
        let tx = self.done_tx.clone();
        self.in_flight += 1;
        // The task reports through the shared channel; its own one-shot receiver is not needed.
        drop(mtg_client::spawn(async move {
            let res = api.image_bytes(&req).await.map_err(|e| format!("{e:#}")).and_then(|b| decode(&b));
            let _ = tx.send((req, res));
        }));
    }
}

/// Concurrent image downloads (the engine proxies Scryfall, which asks clients to stay polite).
const MAX_IN_FLIGHT: usize = 8;

fn init_procedural(mut images: ResMut<Assets<Image>>, ci: ResMut<CardImages>) {
    if ci.pending_procedural_back {
        let img = procedural_card_back();
        let _ = images.insert(ci.card_back.id(), img);
    }
    let _ = images.insert(ci.card_blank.id(), procedural_card_blank());
}

fn pump_requests(mut ci: ResMut<CardImages>, cfg: Res<EngineConfig>, state: Res<State<AppState>>) {
    if *state.get() == AppState::Boot {
        return;
    }
    if ci.queue.is_empty() {
        return;
    }
    // Hand out at most a small batch so late high-priority requests don't sit behind 100 preloads.
    let room = MAX_IN_FLIGHT.saturating_sub(ci.in_flight);
    let n = room.min(ci.queue.len());
    let batch: Vec<_> = ci.queue.drain(0..n).collect();
    for req in batch {
        ci.start_fetch(&cfg.base_url, req);
    }
}

fn receive_images(mut ci: ResMut<CardImages>, mut images: ResMut<Assets<Image>>) {
    let rx = ci.done_rx.clone();
    let mut got = 0;
    while got < 6 {
        let Ok((req, res)) = rx.try_recv() else { break };
        got += 1;
        ci.in_flight = ci.in_flight.saturating_sub(1);
        match res {
            Ok(img) => {
                if let Some(h) = ci.handles.get(&req) {
                    let _ = images.insert(h.id(), img);
                }
            }
            Err(e) => {
                if ci.failed.insert(req.clone()) {
                    warn!("card image {} ({:?}) failed: {e}", req.name, req.version);
                }
            }
        }
    }
}

// ---- decoding & mipmaps ----------------------------------------------------------------------

fn card_sampler() -> ImageSampler {
    ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 8,
        ..default()
    })
}

fn decode(bytes: &[u8]) -> Result<Image, String> {
    let mime = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) { "image/png" } else { "image/jpeg" };
    let mut img = Image::from_buffer(
        bytes,
        ImageType::MimeType(mime),
        CompressedImageFormats::NONE,
        true,
        card_sampler(),
        RenderAssetUsages::default(),
    )
    .map_err(|e| format!("decode: {e}"))?;
    with_mips(&mut img);
    img.asset_usage = RenderAssetUsages::RENDER_WORLD;
    Ok(img)
}

/// Box-filter mip chain computed in linear light (so thumbnails keep their brightness).
pub fn with_mips(img: &mut Image) {
    if img.texture_descriptor.format != TextureFormat::Rgba8UnormSrgb || img.texture_descriptor.mip_level_count > 1 {
        return;
    }
    let (mut w, mut h) = (img.width() as usize, img.height() as usize);
    let Some(base) = img.data.take() else { return };
    if base.len() != w * h * 4 {
        img.data = Some(base);
        return;
    }
    let lut: Vec<f32> = (0..256).map(|i| (i as f32 / 255.0).powf(2.2)).collect();
    let to_srgb = |v: f32| ((v.clamp(0.0, 1.0)).powf(1.0 / 2.2) * 255.0 + 0.5) as u8;
    let mut all = base.clone();
    let mut prev = base;
    let mut levels = 1;
    while w > 1 || h > 1 {
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let mut next = vec![0u8; nw * nh * 4];
        for y in 0..nh {
            for x in 0..nw {
                let (x0, x1) = ((x * 2).min(w - 1), (x * 2 + 1).min(w - 1));
                let (y0, y1) = ((y * 2).min(h - 1), (y * 2 + 1).min(h - 1));
                for c in 0..4 {
                    let at = |xx: usize, yy: usize| prev[(yy * w + xx) * 4 + c];
                    let (a, b, cc, d) = (at(x0, y0), at(x1, y0), at(x0, y1), at(x1, y1));
                    next[(y * nw + x) * 4 + c] = if c == 3 {
                        ((a as u32 + b as u32 + cc as u32 + d as u32 + 2) / 4) as u8
                    } else {
                        to_srgb((lut[a as usize] + lut[b as usize] + lut[cc as usize] + lut[d as usize]) / 4.0)
                    };
                }
            }
        }
        all.extend_from_slice(&next);
        prev = next;
        w = nw;
        h = nh;
        levels += 1;
    }
    img.data = Some(all);
    img.texture_descriptor.mip_level_count = levels;
}

// ---- procedural art --------------------------------------------------------------------------

const CARD_PX_W: usize = 488;
const CARD_PX_H: usize = 680;

fn make_image(data: Vec<u8>) -> Image {
    let mut img = Image::new(
        Extent3d { width: CARD_PX_W as u32, height: CARD_PX_H as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    with_mips(&mut img);
    img.sampler = card_sampler();
    img
}

fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

/// Deep brown card back with an orange swirl and a pale frame, in the spirit of the MTG back.
pub fn procedural_card_back() -> Image {
    let mut data = vec![255u8; CARD_PX_W * CARD_PX_H * 4];
    let border = 26.0;
    for y in 0..CARD_PX_H {
        for x in 0..CARD_PX_W {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let edge = fx.min(CARD_PX_W as f32 - fx).min(fy.min(CARD_PX_H as f32 - fy));
            let mut col;
            if edge < border {
                col = [0.015, 0.012, 0.01];
            } else {
                let (dx, dy) = ((fx - CARD_PX_W as f32 / 2.0) / (CARD_PX_W as f32 / 2.0), (fy - CARD_PX_H as f32 / 2.0) / (CARD_PX_H as f32 / 2.0));
                let r = (dx * dx + dy * dy).sqrt();
                let ang = dy.atan2(dx);
                let swirl = ((ang * 3.0 + r * 9.0).sin() * 0.5 + 0.5) * (1.0 - r * 0.55).max(0.0);
                let ring = ((r * 18.0).sin() * 0.5 + 0.5) * 0.12;
                let base = lerp3([0.16, 0.07, 0.03], [0.62, 0.30, 0.10], (1.0 - r).clamp(0.0, 1.0).powf(1.3));
                col = lerp3(base, [0.95, 0.55, 0.18], (swirl * 0.35 + ring).clamp(0.0, 1.0));
                // inner pale frame line
                let d = edge - border;
                if d < 5.0 {
                    col = lerp3(col, [0.80, 0.62, 0.38], 0.9);
                } else if d < 9.0 {
                    col = lerp3(col, [0.1, 0.05, 0.02], 0.6);
                }
                // central medallion
                let rr = ((dx * 1.0).powi(2) + (dy * 0.72).powi(2)).sqrt();
                if rr < 0.36 {
                    let t = (0.36 - rr) / 0.36;
                    col = lerp3(col, [0.99, 0.80, 0.45], (t * 1.5).min(0.55) * (0.6 + 0.4 * (ang * 5.0 + TAU).sin()));
                }
            }
            let i = (y * CARD_PX_W + x) * 4;
            data[i] = (col[0].clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8;
            data[i + 1] = (col[1].clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8;
            data[i + 2] = (col[2].clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8;
        }
    }
    make_image(data)
}

/// Plain pale card frame with a dark art window: shown while the real card image downloads.
pub fn procedural_card_blank() -> Image {
    let mut data = vec![255u8; CARD_PX_W * CARD_PX_H * 4];
    for y in 0..CARD_PX_H {
        for x in 0..CARD_PX_W {
            let (u, v) = (x as f32 / CARD_PX_W as f32, y as f32 / CARD_PX_H as f32);
            let edge = u.min(1.0 - u).min(v.min(1.0 - v));
            let col: [f32; 3] = if edge < 0.045 {
                [0.02, 0.02, 0.02]
            } else if (0.1..0.58).contains(&v) && (0.09..0.91).contains(&u) {
                lerp3([0.10, 0.12, 0.16], [0.18, 0.2, 0.26], v)
            } else if (0.07..0.1).contains(&v) {
                [0.55, 0.55, 0.52]
            } else {
                lerp3([0.50, 0.47, 0.40], [0.42, 0.39, 0.33], v)
            };
            let i = (y * CARD_PX_W + x) * 4;
            for c in 0..3 {
                data[i + c] = (col[c].powf(1.0 / 2.2) * 255.0) as u8;
            }
        }
    }
    make_image(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mips_cover_the_chain() {
        let mut img = procedural_card_blank();
        let levels = img.texture_descriptor.mip_level_count;
        assert!(levels >= 9);
        let total: usize = (0..levels as usize)
            .map(|l| ((CARD_PX_W >> l).max(1)) * ((CARD_PX_H >> l).max(1)) * 4)
            .sum();
        assert_eq!(img.data.as_ref().unwrap().len(), total);
        with_mips(&mut img); // idempotent
        assert_eq!(img.texture_descriptor.mip_level_count, levels);
    }
}
