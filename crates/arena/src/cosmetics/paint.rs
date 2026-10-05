//! CPU painters that turn a [`Design`] into textures: sleeve backs, the deckbox atlas, deckbox
//! thumbnails (a 3/4 view drawn in 2D) and playmats.
//!
//! Why on the CPU: a design is a few cheap analytic fields (value noise, polar patterns, SDF
//! emblems). Results are cached per design fingerprint and painted on worker threads, and a plain
//! `Image` works everywhere (3D materials and UI nodes) with no extra shader pipeline.
//!
//! Every surface is painted by one function, [`surface`], in face-local coordinates, so a sleeve,
//! the side of a box and a playmat share the same look at any resolution.

use std::f32::consts::{PI, TAU};

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use super::design::{Design, DesignKind, Emblem, Finish, Frame, Pattern, Rgb};

/// Sleeve texture size (same as a card image, so it maps onto the shared card mesh).
pub const SLEEVE_PX: (usize, usize) = (488, 680);
const PLAYMAT_PX: (usize, usize) = (1536, 990);
/// Deckbox proportions (width, height, depth) in world units; the mesh uses the same numbers.
pub const BOX_SIZE: Vec3 = Vec3::new(0.72, 1.0, 0.42);
/// Fraction of the box height taken by the lid.
pub const BOX_LID: f32 = 0.26;
const BOX_ATLAS_H: usize = 512;
pub const THUMB_BOX_PX: (usize, usize) = (220, 250);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PaintTarget {
    /// Texture for the 3D object (sleeve back, deckbox atlas, playmat).
    Full,
    /// Small UI picture (deckboxes: a 3/4 view on a transparent background).
    Thumb,
}

pub fn render(d: &Design, t: PaintTarget) -> Image {
    match (d.kind, t) {
        (DesignKind::Sleeve, PaintTarget::Full) => finish_image(paint_sleeve(d, SLEEVE_PX.0, SLEEVE_PX.1), SLEEVE_PX, true),
        (DesignKind::Sleeve, PaintTarget::Thumb) => finish_image(paint_sleeve(d, 122, 170), (122, 170), false),
        (DesignKind::Deckbox, PaintTarget::Full) => {
            let (px, size) = paint_box_atlas(d);
            finish_image(px, size, true)
        }
        (DesignKind::Deckbox, PaintTarget::Thumb) => finish_image(paint_box_thumb(d, THUMB_BOX_PX.0, THUMB_BOX_PX.1), THUMB_BOX_PX, false),
        (DesignKind::Playmat, PaintTarget::Full) => finish_image(paint_playmat(d, PLAYMAT_PX.0, PLAYMAT_PX.1), PLAYMAT_PX, true),
        (DesignKind::Playmat, PaintTarget::Thumb) => finish_image(paint_playmat(d, 192, 124), (192, 124), false),
    }
}

fn finish_image(rgba: Vec<u8>, (w, h): (usize, usize), mips: bool) -> Image {
    let mut img = Image::new(
        Extent3d { width: w as u32, height: h as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        rgba,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    if mips {
        crate::images::with_mips(&mut img);
    }
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: if mips { 8 } else { 1 },
        ..default()
    });
    img
}

// ---- colour helpers -------------------------------------------------------------------------

type C3 = [f32; 3];

fn lin(c: Rgb) -> C3 {
    let f = |v: u8| (v as f32 / 255.0).powf(2.2);
    [f(c[0]), f(c[1]), f(c[2])]
}

fn mix(a: C3, b: C3, t: f32) -> C3 {
    let t = t.clamp(0.0, 1.0);
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn mul(a: C3, k: f32) -> C3 {
    [a[0] * k, a[1] * k, a[2] * k]
}

fn add(a: C3, b: C3) -> C3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn luma(c: C3) -> f32 {
    c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722
}

fn smooth(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn hue(h: f32) -> C3 {
    let h = h.rem_euclid(1.0) * 6.0;
    let c = |o: f32| (((h + o) % 6.0 - 3.0).abs() - 1.0).clamp(0.0, 1.0);
    [c(0.0), c(4.0), c(2.0)]
}

fn put(out: &mut [u8], i: usize, c: C3, a: f32) {
    let s = |v: f32| (v.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0 + 0.5) as u8;
    out[i] = s(c[0]);
    out[i + 1] = s(c[1]);
    out[i + 2] = s(c[2]);
    out[i + 3] = (a.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
}

// ---- noise ----------------------------------------------------------------------------------

fn hash2(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(374761393) ^ (y as u32).wrapping_mul(668265263) ^ seed.wrapping_mul(2246822519);
    h = (h ^ (h >> 13)).wrapping_mul(1274126177);
    ((h ^ (h >> 16)) & 0xffff) as f32 / 65535.0
}

fn vnoise(x: f32, y: f32, seed: u32) -> f32 {
    let (xi, yi) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (x - x.floor(), y - y.floor());
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let a = hash2(xi, yi, seed);
    let b = hash2(xi + 1, yi, seed);
    let c = hash2(xi, yi + 1, seed);
    let d = hash2(xi + 1, yi + 1, seed);
    let top = a + (b - a) * sx;
    top + (c + (d - c) * sx - top) * sy
}

fn fbm(x: f32, y: f32, seed: u32) -> f32 {
    let mut v = 0.0;
    let (mut amp, mut f) = (0.5, 1.0);
    for o in 0..4 {
        v += vnoise(x * f, y * f, seed.wrapping_add(o * 977)) * amp;
        amp *= 0.5;
        f *= 2.03;
    }
    v / 0.9375
}

// ---- patterns -------------------------------------------------------------------------------

/// Pattern value at a point: `body` blends ground → detail colour, `glint` adds accent highlights.
/// `(cx, cy)` are centred face coordinates (polar patterns radiate from the face centre); `(px, py)`
/// continue across neighbouring faces (tiling patterns stay seamless around a box).
fn pattern(d: &Design, cx: f32, cy: f32, px: f32, py: f32) -> (f32, f32) {
    let k = 1.0 / d.scale.clamp(0.3, 3.0);
    let seed = d.seed;
    let r = (cx * cx + cy * cy).sqrt();
    let ang = cy.atan2(cx);
    match d.pattern {
        Pattern::Gradient => {
            // tooled leather: soft light from the top, fine grain, faint mottling
            let grain = vnoise(px * 90.0 * k, py * 90.0 * k, seed) * 0.5 + vnoise(px * 37.0 * k, py * 37.0 * k, seed + 3) * 0.5;
            let mottle = fbm(px * 2.2 * k, py * 2.2 * k, seed);
            ((0.55 - cy * 0.7) * 0.55 + mottle * 0.35 + (grain - 0.5) * 0.18, 0.0)
        }
        Pattern::Swirl => {
            let arms = ((ang * 3.0 + r * 9.0 * k).sin() * 0.5 + 0.5) * (1.0 - r * 0.8).max(0.0);
            let rings = ((r * 18.0 * k).sin() * 0.5 + 0.5).powf(10.0) * (1.0 - r).max(0.0);
            let n = fbm(px * 3.0, py * 3.0, seed) * 0.25;
            (arms * 0.85 + n, rings * 0.55)
        }
        Pattern::Sunburst => {
            let rays = (0.5 + 0.5 * (ang * 16.0).cos()).powf(3.0);
            let soft = (1.0 - r * 1.1).max(0.0);
            let rings = ((r * 14.0 * k).sin().abs()).powf(24.0);
            (rays * (0.25 + 0.75 * soft) + soft * 0.25, rings * 0.65 + rays * soft * soft * 0.3)
        }
        Pattern::Filigree => {
            // mirrored scrollwork: two interfering wave families drawn as thin lines
            let (ax, ay) = (cx.abs() * 7.0 * k, cy * 7.0 * k);
            let f = (ax * 1.3 + (ay * 1.7).sin() * 1.2).sin() * (ay * 1.1 + (ax * 1.9).cos() * 0.9).cos();
            let line = 1.0 - smooth(0.0, 0.14, f.abs());
            let curl = ((ax * ax + ay * ay).sqrt() * 1.6 + (ay.atan2(ax)) * 2.0).sin();
            let curl_line = 1.0 - smooth(0.0, 0.09, curl.abs());
            let fill = fbm(px * 2.0, py * 2.0, seed) * 0.3;
            (fill + line * 0.55, curl_line * line.max(0.25) * 0.8)
        }
        Pattern::Stripes => {
            let q = (px + py * 0.9) * 5.0 * k + fbm(px, py, seed) * 0.15;
            let band = q.rem_euclid(1.0);
            let wide = smooth(0.06, 0.1, band) * (1.0 - smooth(0.46, 0.5, band));
            let pin = 1.0 - smooth(0.0, 0.03, (band - 0.73).abs());
            (wide * 0.8 + 0.1, pin * 0.9)
        }
        Pattern::Hexes => {
            let s = 6.5 * k;
            let (x, y) = (px * s, py * s);
            let (hx, hy) = hex_cell(x, y);
            let shade = hash2((hx * 10.0) as i32, (hy * 10.0) as i32, seed);
            let (lx, ly) = (x - hx, y - hy);
            let edge = hex_edge(lx, ly);
            let line = 1.0 - smooth(0.035, 0.07, edge);
            let inner = 1.0 - smooth(0.0, 0.03, (edge - 0.16).abs());
            (shade * 0.55 + 0.15 + inner * 0.25, line)
        }
        Pattern::Scales => {
            let s = 7.0 * k;
            // y flipped so the rounded edges point down, like hide rather than roof tiles
            let (x, y) = (px * s, -py * s * 1.2);
            let row = y.floor();
            let mut found = (0.0, 0.0);
            // the lower row overlaps the upper one, so test it first
            'rows: for r in [row + 1.0, row] {
                let shift = if (r as i32).rem_euclid(2) == 1 { 0.5 } else { 0.0 };
                let base = (x + shift).floor() + 0.5 - shift;
                for cxs in [base, base - 1.0, base + 1.0] {
                    let (dx, dy) = (x - cxs, (y - r) * 1.0);
                    let dist = (dx * dx + dy * dy).sqrt() / 0.68;
                    if dist < 1.0 && dy < 0.25 {
                        let sheen = (1.0 - dist).powf(0.6) * 0.5 + (-dy).max(0.0) * 0.3;
                        let rim = smooth(0.8, 0.97, dist);
                        found = (0.25 + sheen * 0.75 - rim * 0.15, rim * 0.85);
                        break 'rows;
                    }
                }
            }
            found
        }
        Pattern::Nebula => {
            let warp = fbm(px * 0.8 * k, py * 0.8 * k, seed) * 1.8;
            let n = fbm(px * 1.6 * k + warp, py * 1.6 * k - warp * 0.5, seed + 11);
            let clouds = smooth(0.3, 0.85, n);
            let star = |cells: f32, thresh: f32, sd: u32| {
                let (gx, gy) = (px * cells, py * cells);
                let h = hash2(gx.floor() as i32, gy.floor() as i32, seed + sd);
                if h < thresh {
                    return 0.0;
                }
                let (fx, fy) = (gx.fract() - 0.5, gy.fract() - 0.5);
                (1.0 - (fx * fx + fy * fy).sqrt() * 3.2).max(0.0).powf(2.0) * ((h - thresh) / (1.0 - thresh) * 0.6 + 0.4)
            };
            let stars = star(70.0, 0.93, 1) + star(26.0, 0.975, 2) * 1.4;
            (clouds * 0.9, (stars + clouds * clouds * 0.15).min(1.0))
        }
        Pattern::Marble => {
            let w = fbm(px * 1.3 * k, py * 1.3 * k, seed);
            let v = ((px * 2.2 * k + py * 1.1 * k + w * 5.0) * 2.4).sin().abs();
            let vein = (1.0 - v).powf(14.0);
            let fine = (1.0 - ((px * 5.0 * k - py * 3.0 * k + w * 7.0) * 3.0).sin().abs()).powf(30.0);
            (w * 0.7 + 0.1, vein * 0.9 + fine * 0.35)
        }
    }
}

/// Centre of the hexagon cell (pointy-top, unit size) containing (x, y).
fn hex_cell(x: f32, y: f32) -> (f32, f32) {
    let (sx, sy) = (1.0, 3f32.sqrt());
    let a = ((x / sx).round() * sx, (y / sy).round() * sy);
    let b = (((x - sx * 0.5) / sx).round() * sx + sx * 0.5, ((y - sy * 0.5) / sy).round() * sy + sy * 0.5);
    let da = (x - a.0).powi(2) + (y - a.1).powi(2);
    let db = (x - b.0).powi(2) + (y - b.1).powi(2);
    if da < db { a } else { b }
}

/// Distance from a point (relative to its hex centre) to the hexagon's edge.
fn hex_edge(x: f32, y: f32) -> f32 {
    let (x, y) = (x.abs(), y.abs());
    let apothem = 0.5;
    let d = (x * 0.5 + y * 3f32.sqrt() / 2.0).max(x);
    (apothem - d).max(0.0)
}

// ---- emblems --------------------------------------------------------------------------------

/// Signed distance-ish value (negative inside, emblem radius ≈ 1) and a 0..1 "engraving" mask for
/// the inner detail lines. `y` points up.
fn emblem(e: Emblem, x: f32, y: f32) -> (f32, f32) {
    let r = (x * x + y * y).sqrt();
    let th = x.atan2(y); // 0 = straight up
    let line = |v: f32, w: f32| 1.0 - smooth(w * 0.5, w, v.abs());
    match e {
        Emblem::None => (1e3, 0.0),
        Emblem::Star => {
            let rr = 0.4 + 0.6 * (0.5 + 0.5 * (th * 5.0).cos()).powf(2.6);
            let d = (r - rr) * 0.8;
            let inner = (r - rr * 0.55) * 0.8;
            (d, line(inner, 0.05))
        }
        Emblem::Sun => {
            let rays = 0.58 + 0.42 * (0.5 + 0.5 * (th * 12.0).cos()).powf(6.0);
            let d = (r - rays) * 0.8;
            (d, line(r - 0.42, 0.06) + line(r - 0.2, 0.05))
        }
        Emblem::Moon => {
            let a = r - 0.86;
            let b = ((x - 0.38).powi(2) + (y - 0.2).powi(2)).sqrt() - 0.72;
            let d = a.max(-b);
            let crater = |cx: f32, cy: f32, rad: f32| line(((x - cx).powi(2) + (y - cy).powi(2)).sqrt() - rad, 0.05);
            (d, crater(-0.5, -0.1, 0.12) + crater(-0.25, -0.5, 0.08))
        }
        Emblem::Drop | Emblem::Flame => {
            let flame = e == Emblem::Flame;
            let (by, br) = if flame { (-0.3, 0.55) } else { (-0.25, 0.6) };
            let tip = 0.95;
            let d = if y < by {
                ((x * x) + (y - by).powi(2)).sqrt() - br
            } else {
                let t = ((tip - y) / (tip - by)).clamp(0.0, 1.0);
                let mut w = br * t.powf(if flame { 1.25 } else { 0.75 });
                if flame {
                    w *= 1.0 + 0.16 * (y * 9.0).sin();
                }
                (x.abs() - w).max(y - tip) * 0.8
            };
            // inner highlight: a smaller copy of the outline
            let (ix, iy) = (x / 0.5, (y + 0.32) / 0.5);
            let inner = if iy < by {
                (ix * ix + (iy - by).powi(2)).sqrt() - br
            } else {
                let t = ((tip - iy) / (tip - by)).clamp(0.0, 1.0);
                (ix.abs() - br * t.powf(0.9)).max(iy - tip)
            };
            (d, line(inner * 0.5, 0.05))
        }
        Emblem::Leaf => {
            let (s, c) = (-0.6f32).sin_cos();
            let (lx, ly) = (x * c - y * s, x * s + y * c);
            let d = (((lx - 0.5).powi(2) + ly * ly).sqrt() - 0.95).max(((lx + 0.5).powi(2) + ly * ly).sqrt() - 0.95);
            let rib = if ly.abs() < 0.75 { line(lx, 0.06) } else { 0.0 };
            let veins = if lx.abs() > 0.05 { line(((ly - lx.abs() * 0.9) * 4.0).rem_euclid(1.0) - 0.5, 0.12) * 0.5 } else { 0.0 };
            (d, rib + veins * smooth(0.0, -0.15, d))
        }
        Emblem::Diamond => {
            let d = (x.abs() / 0.66 + y.abs() / 0.98 - 1.0) * 0.5;
            let inner = x.abs() / 0.66 + y.abs() / 0.98 - 0.55;
            let facets = line(x, 0.04) * (1.0 - smooth(0.4, 0.55, x.abs() / 0.66 + y.abs() / 0.98)) + line(inner * 0.5, 0.04);
            (d, facets)
        }
        Emblem::Spark => {
            let (c, s) = (th.cos().abs(), th.sin().abs());
            let rr = 0.18 + 0.82 * c.powf(16.0) + 0.5 * s.powf(16.0) + 0.22 * (0.5 + 0.5 * (th * 4.0 + PI).cos()).powf(8.0);
            let d = (r - rr) * 0.7;
            (d, line(r - 0.14, 0.05))
        }
    }
}

// ---- the shared surface painter -------------------------------------------------------------

/// How a face uses the design.
#[derive(Clone, Copy)]
struct Surface {
    /// width / height of the face
    aspect: f32,
    /// emblem radius as a fraction of the face height (0 = no emblem)
    emblem: f32,
    /// draw the design's frame
    frame: bool,
    /// dark disc behind the emblem
    medallion: bool,
    /// pattern strength (playmats stay calm so cards read on top)
    contrast: f32,
    /// pixel size in face-height units (anti-aliasing width)
    px: f32,
    /// offset into the continuous pattern space (seamless box sides)
    offset: Vec2,
}

struct Colors {
    base: C3,
    detail: C3,
    accent: C3,
}

fn colors(d: &Design) -> Colors {
    Colors { base: lin(d.base), detail: lin(d.detail), accent: lin(d.accent) }
}

/// Linear colour of one point of a face. `u`, `v` ∈ [0,1], `v` down.
fn surface(d: &Design, cols: &Colors, s: &Surface, u: f32, v: f32) -> C3 {
    let (cx, cy) = ((u - 0.5) * s.aspect, v - 0.5);
    let (px, py) = (u * s.aspect + s.offset.x, v + s.offset.y);
    let (body, glint) = pattern(d, cx, cy, px, py);
    let r = (cx * cx / (s.aspect * s.aspect).max(1.0) + cy * cy).sqrt();

    // light ground in the middle, darker toward the rim
    let ground = mix(mul(cols.base, 0.62), mul(cols.base, 1.18), 1.0 - r * 1.4);
    let mut col = mix(ground, cols.detail, body.clamp(0.0, 1.0) * s.contrast);
    col = mix(col, cols.accent, glint.clamp(0.0, 1.0) * s.contrast);

    // frame
    let e = (u * s.aspect).min((1.0 - u) * s.aspect).min(v).min(1.0 - v);
    let aa = s.px * 1.2;
    let band = |lo: f32, hi: f32| smooth(lo - aa, lo, e) * (1.0 - smooth(hi, hi + aa, e));
    if s.frame {
        match d.frame {
            Frame::None => {}
            Frame::Thin => {
                col = mix(col, mul(cols.base, 0.5), 1.0 - smooth(0.03, 0.03 + aa, e));
                col = mix(col, cols.accent, band(0.036, 0.044));
            }
            Frame::Ornate => {
                col = mix(col, mix(mul(cols.base, 0.35), cols.detail, 0.15), 1.0 - smooth(0.028, 0.028 + aa, e));
                col = mix(col, cols.accent, band(0.03, 0.039));
                col = mix(col, mul(cols.accent, 0.8), band(0.052, 0.056));
                // corner arcs and studs
                let ccx = (u * s.aspect).min((1.0 - u) * s.aspect);
                let ccy = v.min(1.0 - v);
                let arc = ((ccx * ccx + ccy * ccy).sqrt() - 0.13).abs();
                if ccx > 0.03 && ccy > 0.03 {
                    col = mix(col, cols.accent, 1.0 - smooth(0.004, 0.004 + aa, arc));
                }
                let stud = (ccx - 0.085).abs() + (ccy - 0.085).abs();
                col = mix(col, cols.accent, 1.0 - smooth(0.016, 0.016 + aa, stud));
                let mid = cx.abs().min((cy.abs() - 0.5).abs() * 9.0) + (ccy - 0.042).abs();
                col = mix(col, cols.accent, (1.0 - smooth(0.012, 0.012 + aa, mid)) * (1.0 - smooth(0.05, 0.06, cx.abs())));
            }
        }
    }

    // emblem
    if s.emblem > 0.0 && d.emblem != Emblem::None {
        let (ex, ey) = (cx / s.emblem, -cy / s.emblem);
        let eaa = aa / s.emblem;
        let er = (ex * ex + ey * ey).sqrt();
        if s.medallion {
            let disc = 1.0 - smooth(1.32 - eaa, 1.32, er);
            col = mix(col, mix(mul(cols.base, 0.4), cols.detail, 0.12), disc * 0.8);
            col = mix(col, cols.accent, (1.0 - smooth(0.025, 0.025 + eaa, (er - 1.36).abs())) * 0.9);
            col = mix(col, cols.accent, (1.0 - smooth(0.01, 0.01 + eaa, (er - 1.47).abs())) * 0.5);
        }
        let (dist, engrave) = emblem(d.emblem, ex, ey);
        let glow = (-(dist.max(0.0)) * 7.0).exp() * 0.45;
        col = add(col, mul(cols.accent, glow * 0.35));
        let fill = 1.0 - smooth(-eaa, eaa, dist);
        // bevel: brighter toward the top, a darker lip near the edge
        let bevel = 0.78 + 0.32 * (ey * 0.5 + 0.5) - 0.25 * smooth(-0.12, 0.0, dist);
        let mut ink = mul(cols.accent, bevel);
        ink = mix(ink, mul(cols.base, 0.45), engrave.clamp(0.0, 1.0) * 0.75);
        col = mix(col, ink, fill);
        col = mix(col, mul(cols.base, 0.25), (1.0 - smooth(0.0, eaa * 1.5, (dist - eaa).abs())) * 0.6);
    }

    // finish
    match d.finish {
        Finish::Matte => {
            let grain = (hash2((u * 4096.0) as i32, (v * 4096.0) as i32, 9) - 0.5) * 0.04;
            col = add(col, [grain; 3]);
        }
        Finish::Gloss => {
            let g = (-((u * 0.7 * s.aspect + v * 0.9 - 0.55) * 6.0).powi(2)).exp();
            col = add(col, mul([1.0, 0.97, 0.9], g * 0.07));
        }
        Finish::Foil => {
            let n = fbm(px * 1.5, py * 1.5, d.seed + 77);
            let h = u * 0.8 * s.aspect + v * 1.3 + n * 0.6;
            let sheen = (0.5 + 0.5 * (h * TAU * 1.5).sin()).powf(2.0);
            // mostly a brightening sweep of the design's own colours, with only a hint of rainbow
            col = add(mul(col, 1.0 + 0.3 * sheen), mul(hue(h), (0.01 + 0.03 * sheen) * (0.5 + luma(col).sqrt())));
        }
    }
    let vig = 1.0 - (r - 0.35).max(0.0) * 0.45;
    mul(col, vig.clamp(0.4, 1.0))
}

// ---- sleeve ---------------------------------------------------------------------------------

fn paint_sleeve(d: &Design, w: usize, h: usize) -> Vec<u8> {
    let cols = colors(d);
    let s = Surface { aspect: w as f32 / h as f32, emblem: 0.2, frame: true, medallion: true, contrast: 1.0, px: 1.0 / h as f32, offset: Vec2::ZERO };
    let mut out = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let (u, v) = ((x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32);
            let mut col = surface(d, &cols, &s, u, v);
            // the sleeve's dark edge, so stacked cards stay distinct
            let e = (u * s.aspect).min((1.0 - u) * s.aspect).min(v).min(1.0 - v);
            col = mix(col, mul(cols.base, 0.18), 1.0 - smooth(0.008, 0.008 + s.px * 1.5, e));
            put(&mut out, (y * w + x) * 4, col, 1.0);
        }
    }
    out
}

// ---- playmat --------------------------------------------------------------------------------

fn paint_playmat(d: &Design, w: usize, h: usize) -> Vec<u8> {
    let cols = colors(d);
    let aspect = w as f32 / h as f32;
    let s = Surface { aspect, emblem: 0.17, frame: true, medallion: true, contrast: 0.55, px: 1.0 / h as f32, offset: Vec2::ZERO };
    let mut out = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let (u, v) = ((x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32);
            let mut col = surface(d, &cols, &s, u, v);
            let (cx, cy) = ((u - 0.5) * aspect, v - 0.5);
            let aa = s.px * 1.5;
            // centre line between the halves, broken by the medallion
            let gap = smooth(0.26, 0.3, cx.abs());
            col = mix(col, cols.accent, (1.0 - smooth(0.0015, 0.0015 + aa, cy.abs())) * 0.35 * gap);
            // big faint rings around the medallion
            let r = (cx * cx + cy * cy).sqrt();
            for (rad, k) in [(0.36, 0.22), (0.62, 0.12)] {
                col = mix(col, cols.accent, (1.0 - smooth(0.002, 0.002 + aa, (r - rad).abs())) * k);
            }
            // keep the busy pattern down where the cards lie
            col = mul(col, 0.85);
            put(&mut out, (y * w + x) * 4, col, 1.0);
        }
    }
    out
}

// ---- deckbox --------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxFace {
    Front,
    Right,
    Back,
    Left,
    Top,
    Bottom,
}

/// Pixel rect (x, y, w, h) of each face in the deckbox atlas, and the atlas size.
pub fn box_atlas_layout() -> ([(BoxFace, [usize; 4]); 6], (usize, usize)) {
    let h = BOX_ATLAS_H;
    let fw = (BOX_SIZE.x / BOX_SIZE.y * h as f32).round() as usize;
    let sd = (BOX_SIZE.z / BOX_SIZE.y * h as f32).round() as usize;
    (
        [
            (BoxFace::Front, [0, 0, fw, h]),
            (BoxFace::Right, [fw, 0, sd, h]),
            (BoxFace::Back, [fw + sd, 0, fw, h]),
            (BoxFace::Left, [2 * fw + sd, 0, sd, h]),
            (BoxFace::Top, [0, h, fw, sd]),
            (BoxFace::Bottom, [fw, h, fw, sd]),
        ],
        (2 * fw + 2 * sd, h + sd),
    )
}

/// One point on a deckbox face, `u`/`v` face-local (v down; for the top, v = 0 is the back edge).
fn box_face(d: &Design, cols: &Colors, face: BoxFace, u: f32, v: f32, px: f32) -> C3 {
    let (w, h, dep) = (BOX_SIZE.x, BOX_SIZE.y, BOX_SIZE.z);
    let side = |offset: f32, aspect: f32, emblem: f32| Surface { aspect, emblem, frame: true, medallion: emblem > 0.0, contrast: 1.0, px, offset: Vec2::new(offset, 0.0) };
    let mut col = match face {
        BoxFace::Front => surface(d, cols, &side(0.0, w / h, 0.17), u, v * (1.0 - 0.0)),
        BoxFace::Right => surface(d, cols, &side(w / h, dep / h, 0.0), u, v),
        BoxFace::Back => surface(d, cols, &side((w + dep) / h, w / h, 0.11), u, v),
        BoxFace::Left => surface(d, cols, &side((2.0 * w + dep) / h, dep / h, 0.0), u, v),
        BoxFace::Top => {
            let s = Surface { aspect: w / dep, emblem: 0.32, frame: true, medallion: false, contrast: 0.8, px: px * h / dep, offset: Vec2::new(0.0, 3.0) };
            mul(surface(d, cols, &s, u, v), 1.05)
        }
        BoxFace::Bottom => return mul(cols.base, 0.25),
    };
    if face != BoxFace::Top {
        // the lid: a touch darker, with a shadowed seam and a bright lip just above it
        let lid = BOX_LID;
        if v < lid {
            col = mul(col, 0.9);
        }
        let seam = 1.0 - smooth(0.0, px * 2.5, (v - lid).abs());
        col = mix(col, mul(cols.base, 0.12), seam * 0.9);
        let lip = 1.0 - smooth(0.0, px * 2.0, (v - (lid - px * 3.0)).abs());
        col = mix(col, cols.accent, lip * 0.35);
    }
    col
}

fn paint_box_atlas(d: &Design) -> (Vec<u8>, (usize, usize)) {
    let cols = colors(d);
    let (rects, (aw, ah)) = box_atlas_layout();
    let mut out = vec![0u8; aw * ah * 4];
    let px = 1.0 / BOX_ATLAS_H as f32;
    for (face, [x0, y0, fw, fh]) in rects {
        for y in 0..fh {
            for x in 0..fw {
                let (u, v) = ((x as f32 + 0.5) / fw as f32, (y as f32 + 0.5) / fh as f32);
                let col = box_face(d, &cols, face, u, v, px);
                put(&mut out, ((y0 + y) * aw + x0 + x) * 4, col, 1.0);
            }
        }
    }
    (out, (aw, ah))
}

/// A 3/4 view of the box drawn straight from the face functions (no atlas): front, right side
/// and top as affine parallelograms, 2×2 supersampled, on a transparent background with a soft
/// contact shadow.
fn paint_box_thumb(d: &Design, w: usize, h: usize) -> Vec<u8> {
    let cols = colors(d);
    let fh = h as f32 * 0.6;
    let fw = fh * BOX_SIZE.x / BOX_SIZE.y;
    let depth = fh * BOX_SIZE.z / BOX_SIZE.y * 0.62;
    let dv = Vec2::new(depth * 0.86, -depth * 0.5);
    let x0 = (w as f32 - fw - dv.x) * 0.5;
    let y0 = (h as f32 - fh + dv.y) * 0.5 - dv.y * 0.15;
    let px = 1.0 / fh;
    let sample = |p: Vec2| -> Option<C3> {
        // front
        let (u, v) = ((p.x - x0) / fw, (p.y - y0) / fh);
        if (0.0..1.0).contains(&u) && (0.0..1.0).contains(&v) {
            let mut c = box_face(d, &cols, BoxFace::Front, u, v, px);
            let edge = (u * fw).min(v * fh);
            c = add(c, mul(cols.accent, (1.0 - smooth(0.0, 1.6, edge)) * 0.25));
            return Some(c);
        }
        // right side: origin at the front's top-right corner, spanned by depth and height
        let s = (p.x - (x0 + fw)) / dv.x;
        let t = (p.y - y0 - s * dv.y) / fh;
        if (0.0..1.0).contains(&s) && (0.0..1.0).contains(&t) {
            return Some(mul(box_face(d, &cols, BoxFace::Right, s, t, px * 2.0), 0.58));
        }
        // top: origin at the front's top-left corner, spanned by width and depth
        let t = (p.y - y0) / dv.y;
        let s = (p.x - x0 - t * dv.x) / fw;
        if (0.0..1.0).contains(&s) && (0.0..1.0).contains(&t) {
            return Some(mul(box_face(d, &cols, BoxFace::Top, s, 1.0 - t, px * 2.0), 1.12));
        }
        None
    };
    let shadow_c = Vec2::new(x0 + fw * 0.5 + dv.x * 0.5, y0 + fh + dv.y * 0.25);
    let mut out = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let (mut acc, mut cov) = ([0.0f32; 3], 0.0);
            for (ox, oy) in [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
                if let Some(c) = sample(Vec2::new(x as f32 + ox, y as f32 + oy)) {
                    acc = add(acc, c);
                    cov += 0.25;
                }
            }
            let i = (y * w + x) * 4;
            if cov > 0.0 {
                let c = mul(acc, 0.25 / cov);
                // composite over the shadow so the silhouette edge blends into it
                let sh = shadow(x as f32, y as f32, shadow_c, fw, depth);
                let a = cov + sh * (1.0 - cov);
                put(&mut out, i, mul(c, cov / a.max(1e-4)), a);
            } else {
                put(&mut out, i, [0.0; 3], shadow(x as f32, y as f32, shadow_c, fw, depth));
            }
        }
    }
    out
}

fn shadow(x: f32, y: f32, c: Vec2, fw: f32, depth: f32) -> f32 {
    let (dx, dy) = ((x - c.x) / (fw * 0.85 + depth * 0.4), (y - c.y) / (depth * 0.55 + 6.0));
    (1.0 - (dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0).powf(1.5) * 0.55
}

#[cfg(test)]
mod tests {
    use super::super::design::presets;
    use super::*;

    #[test]
    fn atlas_rects_tile_without_overlap() {
        let (rects, (w, h)) = box_atlas_layout();
        for (i, (_, a)) in rects.iter().enumerate() {
            assert!(a[0] + a[2] <= w && a[1] + a[3] <= h);
            for (_, b) in rects.iter().skip(i + 1) {
                let apart = a[0] + a[2] <= b[0] || b[0] + b[2] <= a[0] || a[1] + a[3] <= b[1] || b[1] + b[3] <= a[1];
                assert!(apart, "{a:?} overlaps {b:?}");
            }
        }
    }

    #[test]
    fn every_preset_paints_every_target() {
        // small sizes keep the test fast; the full-size path is the same code
        for d in presets() {
            let s = paint_sleeve(&d, 24, 34);
            assert_eq!(s.len(), 24 * 34 * 4);
            let t = paint_box_thumb(&d, 44, 50);
            assert!(t.chunks(4).any(|p| p[3] == 255), "{} thumb is empty", d.id);
            assert!(t.chunks(4).any(|p| p[3] == 0), "{} thumb has no transparent background", d.id);
            let m = paint_playmat(&d, 30, 20);
            assert!(m.chunks(4).all(|p| p[3] == 255));
        }
    }

    /// `PAINT_DUMP=dir cargo test -p arena dump_paint -- --ignored` writes raw RGBA dumps
    /// (`<id>_<target>_<w>x<h>.rgba`) for eyeballing the presets.
    #[test]
    #[ignore]
    fn dump_paint() {
        let Ok(dir) = std::env::var("PAINT_DUMP") else { return };
        std::fs::create_dir_all(&dir).unwrap();
        for d in presets() {
            for (t, name) in [(PaintTarget::Full, "full"), (PaintTarget::Thumb, "thumb")] {
                let img = render(&d, t);
                let (w, h) = (img.width() as usize, img.height() as usize);
                let data = img.data.unwrap();
                std::fs::write(format!("{dir}/{}_{name}_{w}x{h}.rgba", d.id), &data[..w * h * 4]).unwrap();
            }
        }
    }
}
