//! Player portraits. An avatar is a card name plus a circular crop of its art, not an uploaded
//! image: every client already has the art crop of every card in its offline pack, so a seat's
//! portrait travels as a few bytes and every player at the table sees the same one.

use serde::{Deserialize, Serialize};

/// The art crop of `card`, seen through a circle centred at (`x`, `y`) (fractions of the art's
/// width/height) whose diameter is the art's shorter side divided by `zoom`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Avatar {
    pub card: String,
    pub x: f32,
    pub y: f32,
    pub zoom: f32,
}

impl Default for Avatar {
    fn default() -> Self {
        // faces in card art sit a little above the middle more often than not
        Self { card: String::new(), x: 0.5, y: 0.4, zoom: 1.0 }
    }
}

impl Avatar {
    pub const MAX_ZOOM: f32 = 4.0;

    pub fn of(card: impl Into<String>) -> Self {
        Self { card: card.into(), ..Self::default() }
    }

    /// A fresh pick: zoomed in on the upper middle, where the character's face usually is.
    pub fn framed(card: impl Into<String>) -> Self {
        Self { card: card.into(), x: 0.5, y: 0.3, zoom: 1.5 }
    }

    /// The square of a `w`×`h` art that the circle covers, as `[min_x, min_y, max_x, max_y]` in
    /// pixels. The centre is clamped so the circle never leaves the art (no empty corners).
    pub fn crop(&self, w: f32, h: f32) -> [f32; 4] {
        let zoom = if self.zoom.is_finite() { self.zoom.clamp(1.0, Self::MAX_ZOOM) } else { 1.0 };
        let side = w.min(h) / zoom;
        let r = side * 0.5;
        let cx = (self.x * w).clamp(r, (w - r).max(r));
        let cy = (self.y * h).clamp(r, (h - r).max(r));
        [cx - r, cy - r, cx + r, cy + r]
    }

    /// The same avatar with its centre moved to where [`Self::crop`] actually puts it, so an
    /// editor's handle never drifts past the edge of the art.
    pub fn clamped(&self, w: f32, h: f32) -> Self {
        let [x0, y0, x1, y1] = self.crop(w, h);
        Self { card: self.card.clone(), x: (x0 + x1) * 0.5 / w, y: (y0 + y1) * 0.5 / h, zoom: self.zoom.clamp(1.0, Self::MAX_ZOOM) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crop_stays_inside_the_art() {
        let a = Avatar { card: "x".into(), x: 0.0, y: 1.0, zoom: 1.0 };
        assert_eq!(a.crop(626.0, 457.0), [0.0, 0.0, 457.0, 457.0]);
        let z = Avatar { zoom: 2.0, x: 0.5, y: 0.5, ..a.clone() };
        let [x0, y0, x1, y1] = z.crop(600.0, 400.0);
        assert_eq!((x1 - x0, y1 - y0), (200.0, 200.0));
        assert_eq!((x0, y0), (200.0, 100.0));
        // out-of-range zoom never produces a crop bigger than the art
        let wild = Avatar { zoom: 0.1, ..a };
        assert_eq!(wild.crop(600.0, 400.0)[3], 400.0);
    }

    #[test]
    fn missing_fields_take_defaults() {
        let a: Avatar = serde_json::from_str(r#"{"card":"Griselbrand"}"#).unwrap();
        assert_eq!(a, Avatar::of("Griselbrand"));
        let c = Avatar { x: -3.0, ..a }.clamped(600.0, 400.0);
        assert_eq!(c.x, 200.0 / 600.0);
    }
}
