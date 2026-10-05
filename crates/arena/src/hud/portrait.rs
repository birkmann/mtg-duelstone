//! Procedurally generated avatar portraits (no art assets needed): a hooded silhouette with a
//! rim light on a tinted radial backdrop, seeded by the player's name so each seat looks distinct.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

fn hash(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn smooth(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn hsv(h: f32, s: f32, v: f32) -> Vec3 {
    let h = h.rem_euclid(1.0) * 6.0;
    let c = v * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    Vec3::new(r + v - c, g + v - c, b + v - c)
}

fn sd_circle(p: Vec2, c: Vec2, r: f32) -> f32 {
    p.distance(c) - r
}
fn sd_ellipse(p: Vec2, c: Vec2, r: Vec2) -> f32 {
    // cheap approximation, good enough for a silhouette
    let q = (p - c) / r;
    (q.length() - 1.0) * r.min_element()
}
fn sd_box(p: Vec2, c: Vec2, h: Vec2) -> f32 {
    let d = (p - c).abs() - h;
    d.max(Vec2::ZERO).length() + d.x.max(d.y).min(0.0)
}
fn sd_tri(p: Vec2, a: Vec2, b: Vec2, c: Vec2) -> f32 {
    let e0 = b - a;
    let e1 = c - b;
    let e2 = a - c;
    let v0 = p - a;
    let v1 = p - b;
    let v2 = p - c;
    let pq0 = v0 - e0 * (v0.dot(e0) / e0.dot(e0)).clamp(0.0, 1.0);
    let pq1 = v1 - e1 * (v1.dot(e1) / e1.dot(e1)).clamp(0.0, 1.0);
    let pq2 = v2 - e2 * (v2.dot(e2) / e2.dot(e2)).clamp(0.0, 1.0);
    let s = (e0.x * e2.y - e0.y * e2.x).signum();
    let d = Vec2::new(pq0.dot(pq0), s * (v0.x * e0.y - v0.y * e0.x))
        .min(Vec2::new(pq1.dot(pq1), s * (v1.x * e1.y - v1.y * e1.x)))
        .min(Vec2::new(pq2.dot(pq2), s * (v2.x * e2.y - v2.y * e2.x)));
    -d.x.sqrt() * d.y.signum()
}

/// `seed` text → portrait image (square, `size` px).
pub fn make_portrait(seed: &str, size: u32) -> Image {
    let h = hash(seed);
    let hue = (h % 1000) as f32 / 1000.0;
    let style = (h >> 12) % 3;
    let accent = hsv(hue, 0.65, 1.0);
    let back_a = hsv(hue + 0.04, 0.55, 0.42);
    let back_b = hsv(hue - 0.06, 0.7, 0.10);
    let mut data = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let p = Vec2::new(x as f32 / size as f32, y as f32 / size as f32);
            // backdrop: glow behind the head, vignette to the edge
            let to_c = p.distance(Vec2::new(0.5, 0.42));
            let mut col = back_b.lerp(back_a, (1.0 - to_c * 1.7).clamp(0.0, 1.0).powf(1.6));
            // faint diagonal rays
            let ang = (p.y - 0.42).atan2(p.x - 0.5);
            col += accent * 0.05 * ((ang * 7.0).sin() * 0.5 + 0.5) * (1.0 - to_c).clamp(0.0, 1.0);
            // silhouette
            let head = sd_circle(p, Vec2::new(0.5, 0.40), 0.165);
            let neck = sd_box(p, Vec2::new(0.5, 0.58), Vec2::new(0.07, 0.10));
            let shoulders = sd_ellipse(p, Vec2::new(0.5, 1.02), Vec2::new(0.43, 0.40));
            let mut d = head.min(neck).min(shoulders);
            match style {
                0 => {
                    // hood
                    d = d.min(sd_ellipse(p, Vec2::new(0.5, 0.36), Vec2::new(0.23, 0.26)).max(-(0.0f32 - head) * 0.0));
                }
                1 => {
                    // horns
                    d = d.min(sd_tri(p, Vec2::new(0.36, 0.30), Vec2::new(0.22, 0.05), Vec2::new(0.43, 0.27)));
                    d = d.min(sd_tri(p, Vec2::new(0.64, 0.30), Vec2::new(0.78, 0.05), Vec2::new(0.57, 0.27)));
                }
                _ => {
                    // crown spikes
                    d = d.min(sd_tri(p, Vec2::new(0.40, 0.27), Vec2::new(0.44, 0.10), Vec2::new(0.48, 0.25)));
                    d = d.min(sd_tri(p, Vec2::new(0.47, 0.25), Vec2::new(0.50, 0.06), Vec2::new(0.53, 0.25)));
                    d = d.min(sd_tri(p, Vec2::new(0.52, 0.25), Vec2::new(0.56, 0.10), Vec2::new(0.60, 0.27)));
                }
            }
            let inside = 1.0 - smooth(-0.004, 0.004, d);
            // face shadow + rim light from the upper left
            let lit = ((-(p.x - 0.5) - (p.y - 0.5) * 0.6) * 1.4 + 0.5).clamp(0.0, 1.0);
            let body = Vec3::splat(0.035) + back_a * 0.10;
            let rim = (1.0 - smooth(0.0, 0.035, -d)) * inside * lit;
            let sil = body + accent * (rim * 1.4) + Vec3::splat(0.05) * lit * inside;
            col = col.lerp(sil, inside);
            // glowing eyes line for the hood/horns styles
            if style != 2 {
                let eyes = sd_box(p, Vec2::new(0.5, 0.405), Vec2::new(0.085, 0.008));
                col += accent * 0.9 * (1.0 - smooth(0.0, 0.02, eyes)) * inside * 0.0 + accent * 0.7 * (1.0 - smooth(-0.002, 0.012, eyes));
            }
            let i = ((y * size + x) * 4) as usize;
            data[i] = (col.x.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8;
            data[i + 1] = (col.y.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8;
            data[i + 2] = (col.z.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8;
            data[i + 3] = 255;
        }
    }
    Image::new(
        Extent3d { width: size, height: size, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}
