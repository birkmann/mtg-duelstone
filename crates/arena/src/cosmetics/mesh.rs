//! Deckbox geometry: a body and a slightly larger lid, both UV-mapped into the atlas painted by
//! `paint::paint_box_atlas`. The lid overhang casts a real seam, which sells it as a box rather
//! than a painted brick.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use super::paint::{BOX_LID, BOX_SIZE, BoxFace, box_atlas_layout};

/// Lid overhang on each side (world units).
const LID_GROW: f32 = 0.012;

/// (body, lid) meshes, base of the box at y = 0.
pub fn deckbox_meshes() -> (Mesh, Mesh) {
    let (w, h, d) = (BOX_SIZE.x / 2.0, BOX_SIZE.y, BOX_SIZE.z / 2.0);
    let split = h * (1.0 - BOX_LID);
    let body = block(Vec3::new(w, split, d), 0.0, (BOX_LID, 1.0), false);
    let lid = block(Vec3::new(w + LID_GROW, h - split + LID_GROW, d + LID_GROW), split - LID_GROW * 0.5, (0.0, BOX_LID), true);
    (body, lid)
}

/// A box of half-width `half.x`, height `half.y`, half-depth `half.z` standing on `y0`. Side faces
/// sample the vertical slice `v_range` of their atlas rect; only the lid shows the top face.
fn block(half: Vec3, y0: f32, v_range: (f32, f32), with_top: bool) -> Mesh {
    let (rects, (aw, ah)) = box_atlas_layout();
    let rect = |f: BoxFace| {
        let r = rects.iter().find(|(face, _)| *face == f).unwrap().1;
        // half-texel inset so mip sampling doesn't bleed the neighbouring face in
        let (x0, y0, x1, y1) = (r[0] as f32 + 0.5, r[1] as f32 + 0.5, (r[0] + r[2]) as f32 - 0.5, (r[1] + r[3]) as f32 - 0.5);
        (x0 / aw as f32, y0 / ah as f32, x1 / aw as f32, y1 / ah as f32)
    };
    let (w, top, d) = (half.x, y0 + half.y, half.z);
    let mut pos: Vec<[f32; 3]> = Vec::new();
    let mut nrm: Vec<[f32; 3]> = Vec::new();
    let mut uv: Vec<[f32; 2]> = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
    // corners in order bottom-left, bottom-right, top-right, top-left as seen from outside
    let mut quad = |p: [Vec3; 4], n: Vec3, face: BoxFace, vr: (f32, f32)| {
        let (u0, v0, u1, v1) = rect(face);
        let (vt, vb) = (v0 + (v1 - v0) * vr.0, v0 + (v1 - v0) * vr.1);
        let base = pos.len() as u32;
        for (c, t) in p.iter().zip([[u0, vb], [u1, vb], [u1, vt], [u0, vt]]) {
            pos.push(c.to_array());
            nrm.push(n.to_array());
            uv.push(t);
        }
        idx.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    };
    let v3 = Vec3::new;
    quad([v3(-w, y0, d), v3(w, y0, d), v3(w, top, d), v3(-w, top, d)], Vec3::Z, BoxFace::Front, v_range);
    quad([v3(w, y0, d), v3(w, y0, -d), v3(w, top, -d), v3(w, top, d)], Vec3::X, BoxFace::Right, v_range);
    quad([v3(w, y0, -d), v3(-w, y0, -d), v3(-w, top, -d), v3(w, top, -d)], Vec3::NEG_Z, BoxFace::Back, v_range);
    quad([v3(-w, y0, -d), v3(-w, y0, d), v3(-w, top, d), v3(-w, top, -d)], Vec3::NEG_X, BoxFace::Left, v_range);
    if with_top {
        quad([v3(-w, top, d), v3(w, top, d), v3(w, top, -d), v3(-w, top, -d)], Vec3::Y, BoxFace::Top, (0.0, 1.0));
    }
    quad([v3(-w, y0, -d), v3(w, y0, -d), v3(w, y0, d), v3(-w, y0, d)], Vec3::NEG_Y, BoxFace::Bottom, (0.0, 1.0));
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, nrm)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
        .with_inserted_indices(Indices::U32(idx))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::mesh::VertexAttributeValues;

    #[test]
    fn faces_wind_toward_their_normals() {
        let (body, lid) = deckbox_meshes();
        for m in [body, lid] {
            let Some(VertexAttributeValues::Float32x3(p)) = m.attribute(Mesh::ATTRIBUTE_POSITION) else { panic!() };
            let Some(VertexAttributeValues::Float32x3(n)) = m.attribute(Mesh::ATTRIBUTE_NORMAL) else { panic!() };
            let Some(Indices::U32(idx)) = m.indices() else { panic!() };
            for tri in idx.chunks(3) {
                let [a, b, c] = [tri[0], tri[1], tri[2]].map(|i| Vec3::from_array(p[i as usize]));
                let geo = (b - a).cross(c - a).normalize();
                assert!(geo.dot(Vec3::from_array(n[tri[0] as usize])) > 0.99);
            }
        }
    }
}
