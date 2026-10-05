//! Screen-space overlays that ride on battlefield and command-zone cards: P/T plate
//! (bottom-right), loyalty, counter badges, "xN" group badge. The printed P/T is unreadable at
//! table scale, so every card that has one gets a plate — greyed when it isn't a creature right
//! now (uncrewed Vehicle, God below devotion), so players see the number without a hover. Each overlay is a UI node that covers the card's projected
//! bounding box every frame, so it follows the card's motion and shrinks with row compression.
//!
//! Why UI nodes instead of 3D text: crisp text at any resolution and no render-layer juggling
//! (same projected-overlay idea as eu-ui's markers).

use std::collections::HashMap;

use bevy::picking::Pickable;
use bevy::prelude::*;
use mtg_protocol::{Card, CardId};

use super::kit::*;
use crate::shared::*;

#[derive(Component)]
pub struct CardOverlay {
    pub card: CardId,
    pub key: String,
}

#[derive(Resource, Default)]
pub struct OverlayMap(pub HashMap<CardId, Entity>);

/// What an overlay should show for a card (pure; unit-tested).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PlateInfo {
    pub pt: Option<(String, String)>,
    /// Has a P/T but isn't a creature at the moment: drawn greyed.
    pub inactive: bool,
    pub buffed: bool,
    pub damaged: bool,
    pub reduced: bool,
    pub loyalty: Option<String>,
    pub counters: Vec<String>,
    pub group: usize,
    pub sick: bool,
}

pub fn plate_info(card: &Card, group: usize) -> PlateInfo {
    let mut info = PlateInfo { group, ..default() };
    let Some(st) = card.state.as_ref() else { return info };
    // the engine names counters by display text ("+1/+1"); accept the Forge enum names too
    let get = |a: &str, b: &str| card.counters.get(a).copied().unwrap_or(0) + card.counters.get(b).copied().unwrap_or(0);
    let plus = get("P1P1", "+1/+1");
    let minus = get("M1M1", "-1/-1");
    if let (Some(p), Some(t)) = (st.power, st.toughness) {
        info.inactive = !st.is_creature;
        let t_left = t - card.damage;
        info.damaged = card.damage > 0;
        info.buffed = plus > minus;
        info.reduced = minus > plus;
        info.pt = Some((p.to_string(), t_left.to_string()));
        info.sick = card.sick && !card.tapped;
    }
    if st.is_planeswalker {
        info.loyalty = st.loyalty.clone().filter(|l| !l.is_empty());
    }
    for (k, n) in &card.counters {
        if *n <= 0 {
            continue;
        }
        match k.as_str() {
            "P1P1" | "+1/+1" if st.is_creature => info.counters.push(format!("+{n}/+{n}")),
            "M1M1" | "-1/-1" if st.is_creature => info.counters.push(format!("-{n}/-{n}")),
            "LOYALTY" => {}
            other => info.counters.push(format!("{} x{n}", pretty_counter(other))),
        }
    }
    info
}

fn pretty_counter(k: &str) -> String {
    let mut out = String::new();
    for (i, w) in k.split('_').enumerate() {
        if i > 0 {
            out.push(' ');
        }
        let lw = w.to_lowercase();
        let mut cs = lw.chars();
        if let Some(f) = cs.next() {
            out.extend(f.to_uppercase());
            out.push_str(cs.as_str());
        }
    }
    out
}

fn has_overlay(zone: mtg_protocol::ZoneKind) -> bool {
    matches!(zone, mtg_protocol::ZoneKind::Battlefield | mtg_protocol::ZoneKind::Command)
}

pub fn sync_overlays(
    mut commands: Commands,
    mut map: ResMut<OverlayMap>,
    cards: Query<(Entity, &CardEntity, &CardLocation)>,
    existing: Query<(), With<CardOverlay>>,
) {
    map.0.retain(|_, e| existing.contains(*e));
    let mut wanted: Vec<CardId> = Vec::new();
    for (_, ce, loc) in &cards {
        if has_overlay(loc.zone) && !loc.hidden {
            wanted.push(ce.0);
        }
    }
    for id in &wanted {
        if !map.0.contains_key(id) {
            let e = commands
                .spawn((
                    Node { position_type: PositionType::Absolute, left: Val::Px(-500.0), top: Val::Px(-500.0), width: Val::Px(10.0), height: Val::Px(10.0), ..default() },
                    CardOverlay { card: *id, key: String::new() },
                    GlobalZIndex(2),
                    HudEntity,
                    Pickable::IGNORE,
                    Visibility::Hidden,
                ))
                .id();
            map.0.insert(*id, e);
        }
    }
    let stale: Vec<CardId> = map.0.keys().filter(|id| !wanted.contains(id)).copied().collect();
    for id in stale {
        if let Some(e) = map.0.remove(&id) {
            commands.entity(e).despawn();
        }
    }
}

pub fn position_overlays(
    mut commands: Commands,
    view: Res<GameView>,
    cache: Res<CardCache>,
    fonts: Res<ArenaFonts>,
    registry: Res<CardRegistry>,
    cams: Query<(&Camera, &GlobalTransform), With<TableCamera>>,
    card_q: Query<(&GlobalTransform, Option<&TargetPlacement>, Option<&StackGroup>, &CardLocation, &Visibility), Without<CardOverlay>>,
    mut overlays: Query<(Entity, &mut CardOverlay, &mut Node, &mut Visibility), Without<CardLocation>>,
) {
    let Ok((cam, cgt)) = cams.single() else { return };
    for (e, mut ov, mut node, mut vis) in &mut overlays {
        let hide = |vis: &mut Visibility| {
            if *vis != Visibility::Hidden {
                *vis = Visibility::Hidden;
            }
        };
        let Some(&ce) = registry.0.get(&ov.card) else {
            hide(&mut vis);
            continue;
        };
        let Ok((gt, tp, grp, loc, cvis)) = card_q.get(ce) else {
            hide(&mut vis);
            continue;
        };
        if !has_overlay(loc.zone) || *cvis == Visibility::Hidden || tp.is_some_and(|t| t.layer != TABLE_LAYER || !t.face_up) {
            hide(&mut vis);
            continue;
        }
        // project the four corners of the card quad
        let mut min = Vec2::splat(f32::MAX);
        let mut max = Vec2::splat(f32::MIN);
        let mut ok = true;
        for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            let wp = gt.transform_point(Vec3::new(sx * CARD_W / 2.0, sy * CARD_H / 2.0, 0.0));
            match world_to_ui(cam, cgt, wp) {
                Some(p) => {
                    min = min.min(p);
                    max = max.max(p);
                }
                None => ok = false,
            }
        }
        if !ok {
            hide(&mut vis);
            continue;
        }
        let size = max - min;
        node.left = Val::Px(min.x);
        node.top = Val::Px(min.y);
        node.width = Val::Px(size.x);
        node.height = Val::Px(size.y);
        if *vis != Visibility::Inherited {
            *vis = Visibility::Inherited;
        }
        let Some(card) = view.card(ov.card).or_else(|| cache.0.get(&ov.card)) else { continue };
        let mut info = plate_info(card, grp.map(|g| g.count).unwrap_or(0));
        if loc.zone != mtg_protocol::ZoneKind::Battlefield {
            // a commander waiting in the command zone isn't "summoning sick"
            info.sick = false;
        }
        let font_px = (size.x.min(size.y * 1.4) * 0.17).round().clamp(9.0, 40.0);
        let key = format!("{:?}|{}", info, font_px);
        if ov.key != key {
            ov.key = key;
            commands.entity(e).despawn_children();
            let fonts = fonts.clone();
            commands.entity(e).with_children(|o| build_overlay(o, &fonts, &info, font_px));
        }
    }
}

fn build_overlay(o: &mut ChildSpawnerCommands, fonts: &ArenaFonts, info: &PlateInfo, fpx: f32) {
    let px = Val::Px;
    let pad = (fpx * 0.35).round();
    // ---- P/T plate ----
    if let Some((p, t)) = &info.pt {
        let grey = Color::srgb(0.62, 0.62, 0.62);
        let (pc, tc) = if info.inactive { (grey, grey) } else { (
            if info.buffed { Color::srgb(0.55, 0.78, 1.0) } else if info.reduced { Color::srgb(1.0, 0.45, 0.4) } else { Color::WHITE },
            if info.damaged || info.reduced { Color::srgb(1.0, 0.4, 0.35) } else if info.buffed { Color::srgb(0.55, 0.78, 1.0) } else { Color::WHITE },
        ) };
        o.spawn((
            Node {
                position_type: PositionType::Absolute,
                right: px(-pad * 0.6),
                bottom: px(-pad * 0.7),
                padding: UiRect::axes(px(pad * 1.1), px(pad * 0.25)),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                border: UiRect::all(px(1.5)),
                border_radius: BorderRadius::all(px(fpx * 0.5)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.08, 0.08, 0.1, if info.inactive { 0.8 } else { 0.94 })),
            BorderColor::all(if info.inactive { Color::srgb(0.4, 0.4, 0.4) } else if info.damaged { Color::srgb(0.85, 0.25, 0.2) } else { Color::srgb(0.75, 0.72, 0.65) }),
            Pickable::IGNORE,
        ))
        .with_children(|b| {
            let font = |c: Color| (tfont(fonts, FontKind::Num, 1.0), TextColor(c));
            let mk = |txt: String, c: Color| {
                let (mut f, col) = font(c);
                f.font_size = bevy::text::FontSize::Px(fpx);
                (Text::new(txt), f, col, Pickable::IGNORE)
            };
            b.spawn(mk(p.clone(), pc));
            b.spawn(mk("/".into(), Color::srgb(0.8, 0.78, 0.7)));
            b.spawn(mk(t.clone(), tc));
        });
    } else if let Some(l) = &info.loyalty {
        o.spawn((
            Node {
                position_type: PositionType::Absolute,
                right: px(-pad * 0.4),
                bottom: px(-pad * 0.5),
                min_width: px(fpx * 1.9),
                height: px(fpx * 1.9),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(2.0)),
                border_radius: BorderRadius::all(px(fpx * 0.5)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.07, 0.07, 0.1, 0.95)),
            BorderColor::all(GOLD),
            Pickable::IGNORE,
        ))
        .with_children(|b| {
            let mut f = tfont(fonts, FontKind::Num, 1.0);
            f.font_size = bevy::text::FontSize::Px(fpx * 1.15);
            b.spawn((Text::new(l.clone()), f, TextColor(Color::WHITE), Pickable::IGNORE));
        });
    }
    // ---- counters (top-left column) ----
    if !info.counters.is_empty() {
        o.spawn((
            Node { position_type: PositionType::Absolute, left: px(-pad * 0.4), top: px(pad * 0.6), flex_direction: FlexDirection::Column, row_gap: px(pad * 0.4), ..default() },
            Pickable::IGNORE,
        ))
        .with_children(|c| {
            for s in info.counters.iter().take(4) {
                c.spawn((
                    Node { padding: UiRect::axes(px(pad), px(pad * 0.2)), border: UiRect::all(px(1.5)), border_radius: BorderRadius::all(px(fpx)), ..default() },
                    BackgroundColor(Color::srgba(0.1, 0.22, 0.45, 0.95)),
                    BorderColor::all(Color::srgb(0.6, 0.8, 1.0)),
                    Pickable::IGNORE,
                ))
                .with_children(|b| {
                    let mut f = tfont(fonts, FontKind::Num, 1.0);
                    f.font_size = bevy::text::FontSize::Px(fpx * 0.85);
                    b.spawn((Text::new(s.clone()), f, TextColor(Color::WHITE), Pickable::IGNORE));
                });
            }
        });
    }
    // ---- group badge ----
    if info.group > 1 {
        o.spawn((
            Node {
                position_type: PositionType::Absolute,
                right: px(-pad * 0.3),
                top: px(-pad * 1.1),
                padding: UiRect::axes(px(pad), px(pad * 0.2)),
                border: UiRect::all(px(1.5)),
                border_radius: BorderRadius::all(px(fpx * 0.5)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.04, 0.04, 0.06, 0.95)),
            BorderColor::all(Color::srgb(0.85, 0.85, 0.85)),
            Pickable::IGNORE,
        ))
        .with_children(|b| {
            let mut f = tfont(fonts, FontKind::Num, 1.0);
            f.font_size = bevy::text::FontSize::Px(fpx * 0.95);
            b.spawn((Text::new(format!("x{}", info.group)), f, TextColor(Color::WHITE), Pickable::IGNORE));
        });
    }
    // ---- summoning sickness: tiny "zz" tag bottom-left ----
    if info.sick {
        o.spawn((
            Node { position_type: PositionType::Absolute, left: px(pad * 0.2), bottom: px(pad * 0.2), padding: UiRect::axes(px(pad * 0.7), px(pad * 0.1)), border_radius: BorderRadius::all(px(fpx)), ..default() },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
            Pickable::IGNORE,
        ))
        .with_children(|b| {
            let mut f = tfont(fonts, FontKind::Body, 1.0);
            f.font_size = bevy::text::FontSize::Px(fpx * 0.65);
            b.spawn((Text::new("zz"), f, TextColor(Color::srgb(0.7, 0.8, 1.0)), Pickable::IGNORE));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mtg_protocol::CardState;

    fn creature(p: i32, t: i32, dmg: i32, ctr: &[(&str, i32)]) -> Card {
        Card {
            damage: dmg,
            counters: ctr.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
            state: Some(CardState { is_creature: true, power: Some(p), toughness: Some(t), ..default() }),
            ..default()
        }
    }

    #[test]
    fn damage_reduces_displayed_toughness() {
        let i = plate_info(&creature(3, 4, 1, &[]), 0);
        assert_eq!(i.pt, Some(("3".into(), "3".into())));
        assert!(i.damaged);
    }

    #[test]
    fn counters_and_group() {
        let i = plate_info(&creature(3, 3, 0, &[("P1P1", 2), ("CHARGE", 3)]), 6);
        assert!(i.buffed);
        assert_eq!(i.group, 6);
        assert!(i.counters.contains(&"+2/+2".to_string()));
        assert!(i.counters.contains(&"Charge x3".to_string()));
    }

    #[test]
    fn non_creature_with_printed_pt_is_inactive() {
        let mut god = creature(5, 5, 0, &[]);
        god.state.as_mut().unwrap().is_creature = false;
        let i = plate_info(&god, 0);
        assert_eq!(i.pt, Some(("5".into(), "5".into())));
        assert!(i.inactive);
        assert!(!plate_info(&creature(2, 2, 0, &[]), 0).inactive);
    }
}
