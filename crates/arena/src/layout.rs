//! Pure board layout: `&GameView` (+ viewport aspect) → a pose for every card, plus anchors.
//!
//! Why pure: layout is the thing we want to unit-test and iterate on without a GPU. The ECS part
//! (bottom of the file) only copies the result into `TargetPlacement` components.
//!
//! Coordinate system (table / TABLE_LAYER): X right, Y up, +Z toward the local player. The mat is
//! `TABLE_HALF_W × TABLE_HALF_D` around the origin. The local player owns z>0, the opponent z<0.
//! Cards lie flat (rotation `Rx(-90°)`: front faces +Y, card "up" points to -Z for both seats —
//! Arena does not turn the opponent's cards around). Tapped = extra `-90°` yaw.
//!
//! Multiplayer (3–4 players): the far half is split into one column per opponent, left → right in
//! turn order (`GameView::seat_order`), each a scaled-down copy of the duel layout with its piles and
//! avatar along the back edge. A duel is exactly the original layout (one full-width "column").
//!
//! HAND_LAYER is a separate orthographic world: `HAND_VIEW_H` units tall (y∈[-H/2,H/2], centred on
//! 0), `HAND_VIEW_H*aspect` wide, cards face +Z.

use std::collections::{HashMap, HashSet};
use std::f32::consts::{FRAC_PI_2, PI};

use bevy::prelude::*;
use mtg_protocol::{Card, CardId, PlayerId, ZoneKind};

use crate::shared::*;

// ---- constants others may read -------------------------------------------------------------

pub const TABLE_HALF_W: f32 = 4.8;
pub const TABLE_HALF_D: f32 = 3.3;
/// |z| of the creature row (nearest the centre) and lands row.
pub const Z_CREATURES: f32 = 0.66;
pub const Z_LANDS: f32 = 1.68;
/// |z| of the piles / avatar line.
pub const Z_PILES: f32 = 2.55;
/// Widest a battlefield row may get before it compresses.
pub const ROW_MAX_W: f32 = 9.0;
pub const CARD_GAP: f32 = 0.12;
/// Lowest Y of anything lying on the mat (just above it).
pub const Y_MAT: f32 = 0.012;
pub const STACK_X: f32 = 3.9;
pub const STACK_SCALE: f32 = 2.1;

pub const HAND_VIEW_H: f32 = 10.0;
pub const HAND_SCALE: f32 = 3.0;
pub const OPP_HAND_SCALE: f32 = 1.5;

const GROUP_DX: f32 = 0.10;
const GROUP_DZ: f32 = 0.07;
const GROUP_VISIBLE: usize = 4;
const LAND_PILE_SCALE: f32 = 0.92;

#[derive(Debug, Clone, Default)]
pub struct LayoutOut {
    pub placements: HashMap<CardId, TargetPlacement>,
    /// top card of a group → number of identical permanents in it
    pub groups: HashMap<CardId, usize>,
    pub anchors: BoardAnchors,
}

pub fn flat_rot(tapped: bool) -> Quat {
    let yaw = if tapped { -FRAC_PI_2 } else { 0.0 };
    Quat::from_rotation_y(yaw) * Quat::from_rotation_x(-FRAC_PI_2)
}

/// A card the table should draw, with the zone list it came from.
#[derive(Debug, Clone, Copy)]
pub struct Seen<'a> {
    pub card: &'a Card,
    pub zone: ZoneKind,
    /// Player whose zone list holds it (None = a spell on the stack).
    pub player: Option<PlayerId>,
    pub index: usize,
}

/// Every card the table should draw: all player zones, plus spells on the stack.
pub fn collect_cards(view: &GameView) -> Vec<Seen<'_>> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for p in &view.game.players {
        let z = &p.zones;
        let lists: [(ZoneKind, &Vec<Card>); 6] = [
            (ZoneKind::Battlefield, &z.battlefield),
            (ZoneKind::Hand, &z.hand),
            (ZoneKind::Graveyard, &z.graveyard),
            (ZoneKind::Exile, &z.exile),
            (ZoneKind::Command, &z.command),
            (ZoneKind::Library, &z.library),
        ];
        for (zone, list) in lists {
            for (index, card) in list.iter().enumerate() {
                if seen.insert(card.id) {
                    out.push(Seen { card, zone, player: Some(p.id), index });
                }
            }
        }
    }
    for (index, item) in view.game.stack.iter().enumerate() {
        if item.is_ability || item.is_trigger {
            continue;
        }
        if let Some(card) = &item.source {
            if seen.insert(card.id) {
                out.push(Seen { card, zone: ZoneKind::Stack, player: None, index });
            }
        }
    }
    out
}

/// Spell cards on the stack (top of stack first) that are not present in any player zone.
pub fn stack_spells(view: &GameView) -> Vec<&Card> {
    let in_zones: HashSet<CardId> = view.game.all_cards().map(|c| c.id).collect();
    view.game
        .stack
        .iter()
        .filter(|s| !s.is_ability && !s.is_trigger)
        .filter_map(|s| s.source.as_ref())
        .filter(|c| !in_zones.contains(&c.id))
        .collect()
}

// ---- grouping & rows -----------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Item {
    /// members, bottom first; last = top card
    ids: Vec<CardId>,
    tapped: bool,
}

impl Item {
    fn extra(&self) -> f32 {
        (self.ids.len().min(GROUP_VISIBLE) - 1) as f32 * GROUP_DX
    }
    fn width(&self) -> f32 {
        (if self.tapped { CARD_H } else { CARD_W }) + self.extra()
    }
}

/// Where one player's board sits: a column of the table, cards scaled by `scale`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SeatFrame {
    /// x of the column centre
    pub cx: f32,
    /// half the usable column width (world units)
    pub half_w: f32,
    /// card scale (1 = duel size)
    pub scale: f32,
}

/// Seat `slot` of `players` (slot 0 = viewer). Opponents split the far half into equal columns.
pub fn seat_frame(slot: usize, players: usize) -> SeatFrame {
    let opps = players.saturating_sub(1).max(1);
    if slot == 0 || opps == 1 {
        return SeatFrame { cx: 0.0, half_w: ROW_MAX_W / 2.0, scale: 1.0 };
    }
    let col_w = 2.0 * TABLE_HALF_W / opps as f32;
    let cx = -TABLE_HALF_W + col_w * (slot as f32 - 0.5);
    let scale = if opps == 2 { 0.72 } else { 0.56 };
    SeatFrame { cx, half_w: col_w / 2.0 - 0.12, scale }
}

#[derive(Clone, Copy)]
enum Align {
    Center,
    Left,
    Right,
}

fn group_key(c: &Card) -> Option<String> {
    let st = c.state.as_ref()?;
    // Only vanilla duplicates group: lands and tokens with no attachments and no counters.
    if !c.attached_cards.is_empty() || c.attached_to.is_some() || !c.counters.is_empty() || c.face_down {
        return None;
    }
    if !(st.is_land || c.token) {
        return None;
    }
    Some(format!(
        "{}|{}|{:?}/{:?}|{}|{}|{}",
        st.name, c.token, st.power, st.toughness, c.tapped, c.attacking, c.damage
    ))
}

fn build_items(cards: &[&Card]) -> Vec<Item> {
    let mut items: Vec<Item> = Vec::new();
    let mut by_key: HashMap<String, usize> = HashMap::new();
    for c in cards {
        if let Some(k) = group_key(c) {
            if let Some(&i) = by_key.get(&k) {
                items[i].ids.push(c.id);
                continue;
            }
            by_key.insert(k, items.len());
        }
        items.push(Item { ids: vec![c.id], tapped: c.tapped });
    }
    items
}

struct RowCtx<'a> {
    z: f32,
    max_w: f32,
    /// card scale of the seat (multiplayer columns are smaller)
    base: f32,
    align: Align,
    x_anchor: f32,
    out: &'a mut LayoutOut,
}

fn place_row(items: &[Item], ctx: &mut RowCtx) {
    if items.is_empty() {
        return;
    }
    let mut y_counter = 0.0_f32;
    let ctx_y = &mut y_counter;
    let k = items.len();
    let b = ctx.base;
    let sum: f32 = items.iter().map(|i| i.width() * b).sum();
    let gaps = (k.saturating_sub(1)) as f32;
    let total = sum + gaps * CARD_GAP * b;
    let mut scale = 1.0;
    let mut gap = CARD_GAP * b;
    if total > ctx.max_w {
        // First shrink the cards a little, then let them overlap.
        scale = (ctx.max_w / total).clamp(0.72, 1.0);
        let sum_s = sum * scale;
        gap = if k > 1 { (ctx.max_w - sum_s) / gaps } else { 0.0 };
        gap = gap.min(CARD_GAP * b);
    }
    let width_total = sum * scale + gaps * gap;
    let mut x = match ctx.align {
        Align::Center => ctx.x_anchor - width_total / 2.0,
        Align::Left => ctx.x_anchor,
        Align::Right => ctx.x_anchor - width_total,
    };
    for item in items {
        let w = item.width() * scale * b;
        let extra = item.extra() * scale * b;
        let cw = w - extra;
        let top_cx = x + extra + cw / 2.0;
        let n = item.ids.len();
        let vis = n.min(GROUP_VISIBLE);
        for (j, id) in item.ids.iter().enumerate() {
            let layer = j.min(vis - 1);
            let depth = (vis - 1 - layer) as f32;
            let y = *ctx_y;
            *ctx_y += 0.004;
            let pos = Vec3::new(top_cx - depth * GROUP_DX * scale * b, Y_MAT + y, ctx.z - depth * GROUP_DZ * b);
            ctx.out.placements.insert(
                *id,
                TargetPlacement {
                    translation: pos,
                    rotation: flat_rot(item.tapped),
                    scale: scale * b,
                    layer: TABLE_LAYER,
                    face_up: true,
                },
            );
        }
        if n > 1 {
            ctx.out.groups.insert(*item.ids.last().unwrap(), n);
        }
        x += w + gap;
    }
}

// ---- main entry ----------------------------------------------------------------------------

/// Compute poses for every card on the table. `aspect` = viewport width / height (hand fan only).
pub fn compute(view: &GameView, aspect: f32) -> LayoutOut {
    let mut out = LayoutOut::default();
    out.anchors = anchors();
    let you = view.you;
    // Battlefield positions by card id for attachment pass.
    let mut attach: Vec<(CardId, CardId, i32)> = Vec::new(); // (aura, host, per-host index)
    let mut attach_count: HashMap<CardId, i32> = HashMap::new();
    let on_battlefield: HashSet<CardId> =
        view.game.players.iter().flat_map(|p| p.zones.battlefield.iter().map(|c| c.id)).collect();
    let order = view.seat_order();
    let n_players = order.len();
    out.anchors.seats = vec![SeatAnchors::default(); n_players];

    for p in &view.game.players {
        let mine = Some(p.id) == you;
        let s = if mine { 1.0 } else { -1.0 };
        let slot = order.iter().position(|&id| id == p.id).unwrap_or(0);
        let f = seat_frame(slot, n_players);
        let duel_sized = f.scale == 1.0;

        // ---- battlefield ----
        let mut creatures = Vec::new();
        let mut walkers = Vec::new();
        let mut lands = Vec::new();
        let mut others = Vec::new();
        for c in &p.zones.battlefield {
            if let Some(h) = c.attached_to.filter(|h| on_battlefield.contains(h)) {
                let n = attach_count.entry(h).or_insert(0);
                attach.push((c.id, h, *n));
                *n += 1;
                continue;
            }
            let st = c.state.as_ref();
            let is_creature = st.is_some_and(|s| s.is_creature);
            if is_creature {
                creatures.push(c);
            } else if st.is_some_and(|s| s.is_planeswalker) {
                walkers.push(c);
            } else if st.is_some_and(|s| s.is_land) {
                lands.push(c);
            } else {
                others.push(c);
            }
        }
        // Creatures then planeswalkers, one centred row.
        let mut row_items = build_items(&creatures);
        let walker_items = build_items(&walkers);
        row_items.extend(walker_items);
        place_row(
            &row_items,
            &mut RowCtx {
                z: s * Z_CREATURES,
                max_w: 2.0 * f.half_w,
                base: f.scale,
                align: Align::Center,
                x_anchor: f.cx,
                out: &mut out,
            },
        );
        // Lands left-aligned, artifacts/enchantments right-aligned on the same row. The lands get
        // two thirds of the width unless there are no others.
        let row_w = 2.0 * f.half_w;
        let other_items = build_items(&others);
        let other_w: f32 = other_items.iter().map(|i| (i.width() + CARD_GAP) * f.scale).sum();
        let other_w = other_w.min(row_w * 0.4);
        let land_items = build_items(&lands);
        let land_max = (row_w - other_w - if other_w > 0.0 { 0.6 * f.scale } else { 0.0 }).max(row_w * 0.44);
        place_row(
            &land_items,
            &mut RowCtx {
                z: s * Z_LANDS,
                max_w: land_max,
                base: f.scale,
                align: Align::Left,
                x_anchor: f.cx - f.half_w,
                out: &mut out,
            },
        );
        place_row(
            &other_items,
            &mut RowCtx {
                z: s * Z_LANDS,
                max_w: other_w.max(f.scale),
                base: f.scale,
                align: Align::Right,
                x_anchor: f.cx + f.half_w,
                out: &mut out,
            },
        );

        // ---- piles ----
        let px = if mine { -1.0 } else { 1.0 };
        let pz = s * Z_PILES;
        let a = &mut out.anchors;
        let (lib, gy, ex, cmd, avatar) = if duel_sized {
            (
                Vec3::new(px * 2.9, Y_MAT, pz),
                Vec3::new(px * 3.75, Y_MAT, pz),
                Vec3::new(px * 2.05, Y_MAT, pz),
                Vec3::new(px * 4.4, Y_MAT, pz),
                Vec3::new(0.0, Y_MAT, pz),
            )
        } else {
            // a column's back edge: command zone left, avatar left of centre, piles right
            let at = |k: f32| Vec3::new(f.cx + k * f.half_w, Y_MAT, pz);
            (at(0.56), at(0.86), at(0.28), at(-0.82), at(-0.22))
        };
        if mine {
            (a.my_library, a.my_graveyard, a.my_exile, a.my_command) = (lib, gy, ex, cmd);
        } else if slot == 1 {
            (a.opp_library, a.opp_graveyard, a.opp_exile, a.opp_command, a.opp_avatar) = (lib, gy, ex, cmd, avatar);
        }
        if let Some(seat) = a.seats.get_mut(slot) {
            *seat = SeatAnchors { player: p.id, avatar, library: lib, graveyard: gy, exile: ex, command: cmd, ui_scale: f.scale.max(0.6) };
        }
        let pile = LAND_PILE_SCALE * f.scale;
        let n_lib = p.zones.library.len();
        for (i, c) in p.zones.library.iter().enumerate() {
            let h = (n_lib - i) as f32 * 0.0025;
            out.placements.insert(c.id, pile_pose(lib + Vec3::Y * h, false, pile));
        }
        for (i, c) in p.zones.graveyard.iter().enumerate() {
            // last = most recent = top; slight random-ish jitter so the pile reads as a pile
            let j = ((c.id % 7) as f32 - 3.0) * 0.008;
            out.placements.insert(
                c.id,
                pile_pose(gy + Vec3::new(j, i as f32 * 0.003, -j), c.is_visible(), pile),
            );
        }
        for (i, c) in p.zones.exile.iter().enumerate() {
            let j = ((c.id % 5) as f32 - 2.0) * 0.01;
            out.placements.insert(
                c.id,
                pile_pose(ex + Vec3::new(j, i as f32 * 0.003, j), c.is_visible(), pile * 0.85),
            );
        }
        for (i, c) in p.zones.command.iter().enumerate() {
            out.placements
                .insert(c.id, pile_pose(cmd + Vec3::new(0.0, 0.003 * i as f32, -(i as f32) * 0.5 * f.scale), c.is_visible(), pile));
        }

        // ---- hand ----
        let n = p.zones.hand.len();
        if mine {
            let fan = hand_fan(n, aspect);
            for (c, f) in p.zones.hand.iter().zip(fan) {
                out.placements.insert(
                    c.id,
                    TargetPlacement {
                        translation: f.0,
                        rotation: Quat::from_rotation_z(f.1),
                        scale: HAND_SCALE,
                        layer: HAND_LAYER,
                        face_up: c.is_visible(),
                    },
                );
            }
        } else {
            // the duel fan sits right of centre; a column's fan hangs above that column
            let fan = if duel_sized { opp_hand_fan(n, aspect) } else { column_hand_fan(n, aspect, f) };
            let hs = if duel_sized { OPP_HAND_SCALE } else { OPP_HAND_SCALE * 0.7 };
            for (c, f) in p.zones.hand.iter().zip(fan) {
                out.placements.insert(
                    c.id,
                    TargetPlacement {
                        translation: f.0,
                        rotation: Quat::from_rotation_z(PI + f.1),
                        scale: hs,
                        layer: HAND_LAYER,
                        face_up: false,
                    },
                );
            }
        }
    }

    // ---- attachments: tuck up-left under the host (possibly a host that is itself attached) ----
    for _ in 0..3 {
        for (aura, host, idx) in &attach {
            if let Some(hp) = out.placements.get(host).copied() {
                let k = (*idx + 1) as f32;
                let mut p = hp;
                p.translation += Vec3::new(-0.2 * k, -0.01 * k, -0.24 * k) * hp.scale.min(1.0);
                p.scale = hp.scale;
                out.placements.insert(*aura, p);
            }
        }
    }
    // Tapped host: the aura keeps upright orientation tapped too (it moves with the permanent).

    // ---- stack ----
    let spells = stack_spells(view);
    let n = spells.len();
    let step = if n > 1 { (0.55_f32).min(2.2 / (n - 1) as f32) } else { 0.0 };
    for (i, c) in spells.iter().enumerate() {
        let f = i as f32;
        out.placements.insert(
            c.id,
            TargetPlacement {
                translation: Vec3::new(STACK_X - 0.12 * f, 0.55 + (n - i) as f32 * 0.04, -0.1 - step * f),
                rotation: Quat::from_rotation_y(0.05 * f) * Quat::from_rotation_x(-FRAC_PI_2 + 0.7),
                scale: STACK_SCALE,
                layer: TABLE_LAYER,
                face_up: true,
            },
        );
    }
    out.anchors.stack = Vec3::new(STACK_X, 0.55, -0.1);
    out
}

fn pile_pose(pos: Vec3, face_up: bool, scale: f32) -> TargetPlacement {
    TargetPlacement { translation: pos, rotation: flat_rot(false), scale, layer: TABLE_LAYER, face_up }
}

pub fn anchors() -> BoardAnchors {
    let mut a = BoardAnchors::default();
    a.my_avatar = Vec3::new(0.0, Y_MAT, Z_PILES);
    a.opp_avatar = Vec3::new(0.0, Y_MAT, -Z_PILES);
    a.center_z = 0.0;
    a.stack = Vec3::new(STACK_X, 0.55, -0.1);
    // piles are filled by compute() per player, but give sane defaults for empty boards
    a.my_library = Vec3::new(-2.9, Y_MAT, Z_PILES);
    a.my_graveyard = Vec3::new(-3.75, Y_MAT, Z_PILES);
    a.my_exile = Vec3::new(-2.05, Y_MAT, Z_PILES);
    a.my_command = Vec3::new(-4.4, Y_MAT, Z_PILES);
    a.opp_library = Vec3::new(2.9, Y_MAT, -Z_PILES);
    a.opp_graveyard = Vec3::new(3.75, Y_MAT, -Z_PILES);
    a.opp_exile = Vec3::new(2.05, Y_MAT, -Z_PILES);
    a.opp_command = Vec3::new(4.4, Y_MAT, -Z_PILES);
    a
}

/// Hand fan in the ortho HAND world: (translation, roll radians) per card, left to right.
pub fn hand_fan(n: usize, aspect: f32) -> Vec<(Vec3, f32)> {
    if n == 0 {
        return vec![];
    }
    let w = CARD_W * HAND_SCALE;
    let h = CARD_H * HAND_SCALE;
    let view_w = HAND_VIEW_H * aspect;
    let avail = (view_w * 0.62).min(w * 0.82 * n as f32);
    let spacing = if n > 1 { (avail / (n - 1) as f32).min(w * 0.82) } else { 0.0 };
    let visible = h * 0.5;
    let base_y = -HAND_VIEW_H / 2.0 + visible - h / 2.0;
    let max_total_angle = 0.42_f32;
    let d_theta = if n > 1 { (max_total_angle / (n - 1) as f32).min(0.075) } else { 0.0 };
    (0..n)
        .map(|i| {
            let o = i as f32 - (n as f32 - 1.0) / 2.0;
            let theta = -o * d_theta;
            let x = o * spacing;
            let y = base_y - o * o * 0.035 + h * 0.0;
            (Vec3::new(x, y, 0.1 + i as f32 * 0.02), theta)
        })
        .collect()
}

/// Opponent's hand: backs hanging from the top edge of the screen.
pub fn opp_hand_fan(n: usize, aspect: f32) -> Vec<(Vec3, f32)> {
    if n == 0 {
        return vec![];
    }
    let w = CARD_W * OPP_HAND_SCALE;
    let h = CARD_H * OPP_HAND_SCALE;
    let view_w = HAND_VIEW_H * aspect;
    let spacing = (w * 0.7).min(view_w * 0.3 / n as f32);
    let base_y = HAND_VIEW_H / 2.0 + h / 2.0 - 0.62;
    (0..n)
        .map(|i| {
            let o = i as f32 - (n as f32 - 1.0) / 2.0;
            (Vec3::new(o * spacing + 3.0, base_y + o * o * 0.02, 0.1 + i as f32 * 0.02), o * 0.03)
        })
        .collect()
}

/// An opponent column's hand: small backs hanging from the top edge above that column.
pub fn column_hand_fan(n: usize, aspect: f32, f: SeatFrame) -> Vec<(Vec3, f32)> {
    if n == 0 {
        return vec![];
    }
    let h = CARD_H * OPP_HAND_SCALE * 0.7;
    let w = CARD_W * OPP_HAND_SCALE * 0.7;
    let view_w = HAND_VIEW_H * aspect;
    // the far edge of the table is foreshortened: map table x to ~70% of the screen half-width
    let cx = f.cx / TABLE_HALF_W * view_w * 0.36;
    let spacing = (w * 0.45).min(view_w * 0.12 / n as f32);
    let base_y = HAND_VIEW_H / 2.0 + h / 2.0 - 0.42;
    (0..n)
        .map(|i| {
            let o = i as f32 - (n as f32 - 1.0) / 2.0;
            (Vec3::new(cx + o * spacing, base_y + o * o * 0.015, 0.1 + i as f32 * 0.02), o * 0.03)
        })
        .collect()
}

// ---- ECS glue ------------------------------------------------------------------------------

pub struct LayoutPlugin;

impl Plugin for LayoutPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BoardAnchors>().add_systems(
            PostUpdate,
            apply_layout.run_if(resource_exists::<GameView>),
        );
    }
}

/// Recompute poses when the view changes, when the registry changes (new entities need a pose
/// immediately) or when the window aspect changes.
fn apply_layout(
    mut commands: Commands,
    view: Res<GameView>,
    registry: Res<CardRegistry>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mut anchors: ResMut<BoardAnchors>,
    mut targets: Query<(Option<&mut TargetPlacement>, Option<&StackGroup>)>,
    mut last: Local<(i64, usize, f32, bool)>,
) {
    let aspect = windows.single().map(|w| w.width() / w.height().max(1.0)).unwrap_or(16.0 / 9.0);
    let key = (view.seq, registry.0.len(), aspect, view.has_state());
    if !view.has_state() || (key == *last && !registry.is_changed()) {
        return;
    }
    *last = key;
    let out = compute(&view, aspect);
    *anchors = out.anchors.clone();
    for (id, &e) in &registry.0 {
        let Some(p) = out.placements.get(id).copied() else { continue };
        let Ok((cur, grp)) = targets.get_mut(e) else { continue };
        match cur {
            Some(mut cur) => {
                if *cur != p {
                    *cur = p;
                }
            }
            None => {
                commands.entity(e).insert(p);
            }
        }
        match (out.groups.get(id), grp) {
            (Some(&n), Some(g)) if g.count == n => {}
            (Some(&n), _) => {
                commands.entity(e).insert(StackGroup { count: n });
            }
            (None, Some(_)) => {
                commands.entity(e).remove::<StackGroup>();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mtg_protocol::{CardState, Game, Player, Zones};

    fn land(id: i64, name: &str, tapped: bool) -> Card {
        Card {
            id,
            zone: Some("Battlefield".into()),
            tapped,
            state: Some(CardState { name: name.into(), is_land: true, ..default() }),
            ..default()
        }
    }
    fn creature(id: i64) -> Card {
        Card {
            id,
            zone: Some("Battlefield".into()),
            state: Some(CardState { name: format!("Bear{id}"), is_creature: true, ..default() }),
            ..default()
        }
    }
    fn view(me: Vec<Card>, opp: Vec<Card>, hand: usize) -> GameView {
        let hand_cards = (0..hand)
            .map(|i| Card {
                id: 500 + i as i64,
                zone: Some("Hand".into()),
                state: Some(CardState { name: "X".into(), ..default() }),
                ..default()
            })
            .collect();
        GameView {
            seq: 1,
            you: Some(1),
            game: Game {
                players: vec![
                    Player { id: 1, zones: Zones { battlefield: me, hand: hand_cards, ..default() }, ..default() },
                    Player { id: 2, zones: Zones { battlefield: opp, ..default() }, ..default() },
                ],
                ..default()
            },
            ..default()
        }
    }

    #[test]
    fn identical_lands_group() {
        let v = view((0..6).map(|i| land(i, "Forest", false)).collect(), vec![], 0);
        let out = compute(&v, 16.0 / 9.0);
        assert_eq!(out.groups.len(), 1);
        assert_eq!(*out.groups.values().next().unwrap(), 6);
        // different name / tapped state doesn't group
        let v = view(vec![land(1, "Forest", false), land(2, "Island", false), land(3, "Forest", true)], vec![], 0);
        assert!(compute(&v, 1.7).groups.is_empty());
    }

    #[test]
    fn opponent_is_mirrored_in_z_and_creatures_nearest_centre() {
        let v = view(vec![creature(1), land(2, "Forest", false)], vec![creature(3), land(4, "Island", false)], 0);
        let out = compute(&v, 1.7);
        let p = |id| out.placements[&id].translation;
        assert!(p(1).z > 0.0 && p(3).z < 0.0);
        assert!((p(1).z + p(3).z).abs() < 0.2);
        assert!(p(1).z < p(2).z, "creatures nearer centre than lands");
        assert!(p(3).z > p(4).z);
    }

    #[test]
    fn rows_compress_when_overflowing() {
        let v = view((0..30).map(creature).collect(), vec![], 0);
        let out = compute(&v, 1.7);
        let xs: Vec<f32> = (0..30).map(|i| out.placements[&i].translation.x).collect();
        let min = xs.iter().cloned().fold(f32::MAX, f32::min);
        let max = xs.iter().cloned().fold(f32::MIN, f32::max);
        assert!(max - min <= ROW_MAX_W, "span {}", max - min);
        assert!(out.placements[&0].scale < 1.0);
    }

    #[test]
    fn tapped_rotates() {
        let v = view(vec![land(1, "Forest", true)], vec![], 0);
        let out = compute(&v, 1.7);
        assert!(out.placements[&1].rotation.angle_between(flat_rot(false)) > 1.0);
    }

    #[test]
    fn hand_fan_is_centred_and_on_hand_layer() {
        let v = view(vec![], vec![], 7);
        let out = compute(&v, 16.0 / 9.0);
        let xs: Vec<f32> = (0..7).map(|i| out.placements[&(500 + i)].translation.x).collect();
        assert!(xs.windows(2).all(|w| w[0] < w[1]));
        assert!((xs[0] + xs[6]).abs() < 1e-3);
        assert_eq!(out.placements[&500].layer, HAND_LAYER);
        // each card at least partly visible, mostly below screen centre
        assert!(out.placements[&503].translation.y < -HAND_VIEW_H / 2.0 + CARD_H * HAND_SCALE);
        let fan = hand_fan(7, 16.0 / 9.0);
        assert!(fan[0].1 > fan[6].1, "left card rolls counter-clockwise");
    }

    #[test]
    fn four_players_get_separate_columns() {
        let mut v = view(vec![creature(1)], vec![creature(3)], 0);
        for id in [5, 6] {
            v.game.players.push(Player { id, zones: Zones { battlefield: vec![creature(id * 10)], ..default() }, ..default() });
        }
        let out = compute(&v, 16.0 / 9.0);
        let p = |id| out.placements[&id];
        // my board is untouched by the crowd
        assert_eq!(p(1).scale, 1.0);
        assert!(p(1).translation.z > 0.0);
        // opponents: far half, smaller, left → right in turn order (players after me first)
        let xs: Vec<f32> = [3, 50, 60].iter().map(|&id| p(id).translation.x).collect();
        assert!(xs.windows(2).all(|w| w[0] < w[1]), "{xs:?}");
        for id in [3, 50, 60] {
            assert!(p(id).translation.z < 0.0 && p(id).scale < 1.0);
        }
        assert_eq!(out.anchors.seats.len(), 4);
        assert_eq!(out.anchors.seats[0].player, 1);
        assert_eq!(out.anchors.seats[1].player, 2);
        assert!(out.anchors.seats[1].avatar.x < out.anchors.seats[3].avatar.x);
        // columns stay on the mat
        for id in [3, 50, 60] {
            assert!(p(id).translation.x.abs() < TABLE_HALF_W);
        }
    }

    #[test]
    fn duel_layout_matches_single_column() {
        assert_eq!(seat_frame(1, 2), seat_frame(0, 2));
        assert_eq!(seat_frame(1, 2).scale, 1.0);
    }

    #[test]
    fn attached_aura_follows_host() {
        let mut host = creature(1);
        host.attached_cards = vec![9];
        let mut aura = Card { id: 9, zone: Some("Battlefield".into()), attached_to: Some(1), ..default() };
        aura.state = Some(CardState { name: "Aura".into(), is_enchantment: true, ..default() });
        let v = view(vec![host, aura], vec![], 0);
        let out = compute(&v, 1.7);
        let (h, a) = (out.placements[&1].translation, out.placements[&9].translation);
        assert!(a.x < h.x && a.z < h.z);
    }
}
