//! Stack presentation. Spell cards on the stack are ordinary 3D `CardEntity`s placed by layout;
//! here we add what has no card of its own:
//!   * "ability cards" (UI) for triggers/abilities: source art crop + text, order badge, auto-yield
//!     toggle (`ClientFrame::Yield`),
//!   * order badges on stacked spell cards,
//!   * persistent ribbons from each stack item to its targets,
//!   * a "clear auto-yields" chip while any yield is armed.

use std::collections::HashMap;

use bevy::picking::Pickable;
use bevy::prelude::*;
use mtg_client::ImageVersion;
use mtg_protocol::{Card, ClientFrame, StackItem};

use crate::hud::kit::*;
use crate::shared::*;
use crate::targeting::{Arrow, ArrowQueue, card_anchor, player_anchor};

pub struct StackPlugin;

impl Plugin for StackPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<StackUiMap>().init_resource::<BoardAnchors>().add_systems(
            Update,
            (sync_ability_cards, stack_acts, stack_arrows).chain().run_if(in_state(AppState::InMatch).or_else(in_state(AppState::GameOver))),
        );
        app.add_systems(
            PostUpdate,
            position_stack_ui
                .after(bevy::transform::TransformSystems::Propagate)
                .before(bevy::ui::UiSystems::Prepare)
                .run_if(in_state(AppState::InMatch).or_else(in_state(AppState::GameOver))),
        );
    }
}

#[derive(Component)]
pub struct AbilityCard {
    pub item: i64,
    pub index: usize,
    pub key: String,
    pub age: f32,
}
#[derive(Component)]
pub struct SpellBadge {
    pub card: mtg_protocol::CardId,
}
#[derive(Component)]
pub struct YieldChip;

#[derive(Resource, Default)]
pub struct StackUiMap(pub HashMap<i64, Entity>);

fn is_ability(s: &StackItem) -> bool {
    s.is_ability || s.is_trigger
}

fn item_key(s: &StackItem, yielded: bool, idx: usize) -> String {
    format!("{}|{}|{}|{}|{}", s.id, s.text, yielded, idx, s.is_trigger)
}

fn border_for(c: Option<&Card>) -> Color {
    let colors = c.and_then(|c| c.state.as_ref()).map(|s| s.colors.as_str()).unwrap_or("");
    match colors.len() {
        0 => Color::srgb(0.6, 0.6, 0.62),
        1 => match colors {
            "W" => Color::srgb(0.95, 0.92, 0.75),
            "U" => Color::srgb(0.35, 0.65, 0.95),
            "B" => Color::srgb(0.55, 0.45, 0.65),
            "R" => Color::srgb(0.92, 0.38, 0.28),
            _ => Color::srgb(0.35, 0.75, 0.42),
        },
        _ => Color::srgb(0.86, 0.69, 0.36),
    }
}

fn sync_ability_cards(
    mut commands: Commands,
    view: Res<GameView>,
    fonts: Res<ArenaFonts>,
    mut map: ResMut<StackUiMap>,
    mut cards: Query<(&mut AbilityCard, &mut UiTransform)>,
    time: Res<Time>,
) {
    let stack = &view.game.stack;
    // wanted: ability items with their stack index (0 = top)
    let wanted: Vec<(usize, &StackItem)> = stack.iter().enumerate().filter(|(_, s)| is_ability(s)).collect();
    let stale: Vec<i64> = map.0.keys().filter(|id| !wanted.iter().any(|(_, s)| s.id == **id)).copied().collect();
    for id in stale {
        if let Some(e) = map.0.remove(&id) {
            commands.entity(e).despawn();
        }
    }
    for (idx, item) in wanted {
        let yielded = view.auto_yields.contains(&item.key);
        let key = item_key(item, yielded, idx);
        if let Some(&e) = map.0.get(&item.id) {
            if let Ok((mut ac, mut tf)) = cards.get_mut(e) {
                ac.age += time.delta_secs();
                let s = (0.55 + 0.45 * (ac.age * 7.0).min(1.0)).min(1.0);
                tf.scale = Vec2::splat(s);
                if ac.key == key {
                    continue;
                }
            } else {
                map.0.remove(&item.id);
                // stale entity (match torn down): fall through and respawn
            }
            if let Ok(mut ec) = commands.get_entity(e) {
                ec.despawn();
            }
        }
        let e = spawn_ability_card(&mut commands, &fonts, item, idx, yielded, key);
        map.0.insert(item.id, e);
    }
}

fn spawn_ability_card(commands: &mut Commands, fonts: &ArenaFonts, item: &StackItem, idx: usize, yielded: bool, key: String) -> Entity {
    let border = border_for(item.source.as_ref());
    let fonts = fonts.clone();
    let item = item.clone();
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(-1000.0),
                top: Val::Px(-1000.0),
                width: vh(15.5),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(vh(0.5)),
                row_gap: vh(0.4),
                border: UiRect::all(vh(0.28)),
                border_radius: BorderRadius::all(vh(1.1)),
                ..default()
            },
            UiTransform::from_scale(Vec2::splat(0.55)),
            BackgroundColor(Color::srgba(0.07, 0.07, 0.09, 0.96)),
            BorderColor::all(border),
            BoxShadow(vec![ShadowStyle { color: Color::srgba(0.0, 0.0, 0.0, 0.7), x_offset: Val::Px(0.0), y_offset: vh(0.4), spread_radius: Val::Px(0.0), blur_radius: vh(1.4) }]),
            GlobalZIndex(14),
            AbilityCard { item: item.id, index: idx, key, age: 0.0 },
            HudEntity,
            Visibility::Hidden,
        ))
        .with_children(|c| {
            // name bar
            let name = item.source.as_ref().map(|s| s.display_name().to_string()).unwrap_or_else(|| "Ability".into());
            c.spawn((
                Node { padding: UiRect::axes(vh(0.7), vh(0.25)), border_radius: BorderRadius::all(vh(0.5)), ..default() },
                BackgroundColor(border.with_alpha(0.85)),
                Pickable::IGNORE,
            ))
            .with_children(|b| {
                b.spawn((label(&fonts, name, FontKind::Title, 1.55, Color::srgb(0.05, 0.05, 0.06)), Node { max_width: vh(13.5), ..default() }));
            });
            // art
            if let Some(src) = item.source.as_ref().filter(|s| !s.hidden && s.state.is_some()) {
                spawn_card_pic_sized(c, &fonts, src, ImageVersion::ArtCrop, 14.2, 8.8);
            }
            // type tag + text
            c.spawn(label(&fonts, if item.is_trigger { "TRIGGERED ABILITY" } else { "ACTIVATED ABILITY" }, FontKind::Title, 1.05, GOLD));
            c.spawn((
                Node { max_height: vh(7.4), overflow: Overflow::clip(), ..default() },
                Pickable::IGNORE,
            ))
            .with_children(|t| {
                t.spawn((label(&fonts, item.text.clone(), FontKind::Body, 1.25, INK), Node { max_width: vh(14.2), ..default() }));
            });
            // order badge
            c.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: vh(-1.2),
                    top: vh(-1.2),
                    width: vh(3.0),
                    height: vh(3.0),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border: UiRect::all(vh(0.2)),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BackgroundColor(Color::srgb(0.05, 0.05, 0.08)),
                BorderColor::all(GOLD),
                Pickable::IGNORE,
            ))
            .with_children(|b| {
                b.spawn(label(&fonts, format!("{}", idx + 1), FontKind::Num, 1.8, INK));
            });
            // auto-yield
            if !item.key.is_empty() {
                let style = if yielded { BtnStyle::GOLD } else { BtnStyle::DARK };
                let text = if yielded { "Auto-yield ON" } else { "Auto-yield" };
                spawn_button(c, &fonts, text, Act::Yield { key: item.key.clone(), auto: !yielded }, style, Val::Percent(100.0), vh(3.4), 1.35, true);
            }
        })
        .id()
}

fn stack_acts(mut acts: MessageReader<Acted>, mut out: MessageWriter<SendCmd>) {
    for Acted(a) in acts.read() {
        if let Act::Yield { key, auto } = a {
            out.write(SendCmd(ClientFrame::Yield { key: key.clone(), auto: *auto }));
        }
    }
}

/// Ability cards form a column left of the spell stack; spell badges ride on the 3D cards.
fn position_stack_ui(
    mut commands: Commands,
    view: Res<GameView>,
    anchors: Res<BoardAnchors>,
    fonts: Res<ArenaFonts>,
    registry: Res<CardRegistry>,
    cams: Query<(&Camera, &GlobalTransform), With<TableCamera>>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    card_tf: Query<(&GlobalTransform, Option<&TargetPlacement>)>,
    mut abilities: Query<(&AbilityCard, &mut Node, &mut Visibility), Without<SpellBadge>>,
    mut badges: Query<(Entity, &SpellBadge, &mut Node, &mut Visibility, &Children), Without<AbilityCard>>,
    mut texts: Query<&mut Text>,
    mut chip: Local<Option<Entity>>,
    mut chip_n: Local<usize>,
) {
    let Ok((cam, cgt)) = cams.single() else { return };
    let Ok(win) = windows.single() else { return };
    let h = win.height();
    // ---- ability column ----
    let base = world_to_ui(cam, cgt, anchors.stack + Vec3::new(-2.15, 0.0, -1.3)).unwrap_or(Vec2::new(win.width() * 0.62, h * 0.2));
    let step = h * 0.205;
    let mut sorted: Vec<_> = abilities.iter_mut().collect();
    sorted.sort_by_key(|(a, _, _)| a.index);
    let n = sorted.len();
    for (i, (_ac, mut node, mut vis)) in sorted.into_iter().enumerate() {
        node.left = Val::Px(base.x - h * 0.0775);
        node.top = Val::Px((base.y + i as f32 * step * (if n > 3 { 3.0 / n as f32 } else { 1.0 })).min(h * 0.7));
        if *vis != Visibility::Inherited {
            *vis = Visibility::Inherited;
        }
    }
    // ---- spell badges ----
    let spells: Vec<(usize, &StackItem)> = view.game.stack.iter().enumerate().filter(|(_, s)| !is_ability(s)).collect();
    for (e, b, _, _, _) in &badges {
        if !spells.iter().any(|(_, s)| s.source.as_ref().is_some_and(|c| c.id == b.card)) {
            commands.entity(e).despawn();
        }
    }
    for (idx, s) in &spells {
        let Some(c) = s.source.as_ref() else { continue };
        let existing = badges.iter().any(|(_, b, _, _, _)| b.card == c.id);
        if !existing {
            let fonts = fonts.clone();
            let id = c.id;
            commands
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(-100.0),
                        top: Val::Px(-100.0),
                        width: vh(3.2),
                        height: vh(3.2),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        border: UiRect::all(vh(0.2)),
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.05, 0.05, 0.08)),
                    BorderColor::all(GOLD),
                    SpellBadge { card: id },
                    GlobalZIndex(15),
                    HudEntity,
                    Pickable::IGNORE,
                    Visibility::Hidden,
                ))
                .with_children(|b| {
                    b.spawn(label(&fonts, format!("{}", idx + 1), FontKind::Num, 1.9, INK));
                });
        }
    }
    for (_, b, mut node, mut vis, children) in &mut badges {
        let Some((idx, _)) = spells.iter().find(|(_, s)| s.source.as_ref().is_some_and(|c| c.id == b.card)) else { continue };
        let Some(&ent) = registry.0.get(&b.card) else { continue };
        let Ok((gt, tp)) = card_tf.get(ent) else { continue };
        if tp.is_some_and(|t| t.layer != TABLE_LAYER) {
            continue;
        }
        let corner = gt.transform_point(Vec3::new(-CARD_W * 0.5, CARD_H * 0.5, 0.0));
        if let Some(p) = world_to_ui(cam, cgt, corner) {
            node.left = Val::Px(p.x - h * 0.016);
            node.top = Val::Px(p.y - h * 0.016);
            if *vis != Visibility::Inherited {
                *vis = Visibility::Inherited;
            }
        }
        if let Some(&c0) = children.first()
            && let Ok(mut t) = texts.get_mut(c0)
        {
            let s = format!("{}", idx + 1);
            if t.0 != s {
                t.0 = s;
            }
        }
    }
    // ---- clear-yields chip ----
    let n_yields = view.auto_yields.len();
    if n_yields != *chip_n {
        *chip_n = n_yields;
        if let Some(e) = chip.take()
            && let Ok(mut ec) = commands.get_entity(e)
        {
            ec.despawn();
        }
        if n_yields > 0 {
            let fonts = fonts.clone();
            let e = commands
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        right: vw(1.8),
                        top: vh(10.0),
                        padding: UiRect::axes(vh(1.4), vh(0.6)),
                        border: UiRect::all(vh(0.15)),
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BackgroundColor(BtnStyle::DARK.base),
                    BorderColor::all(GOLD),
                    BtnStyle::DARK,
                    OnClick(Act::ClearYieldsAct),
                    GlobalZIndex(14),
                    YieldChip,
                    HudEntity,
                ))
                .with_children(|c| {
                    c.spawn(label(&fonts, format!("Clear auto-yields ({n_yields})"), FontKind::Body, 1.6, INK));
                })
                .id();
            *chip = Some(e);
        }
    }
}

fn stack_arrows(
    time: Res<Time>,
    view: Res<GameView>,
    anchors: Res<BoardAnchors>,
    registry: Res<CardRegistry>,
    cards: Query<(&GlobalTransform, Option<&TargetPlacement>)>,
    mut queue: ResMut<ArrowQueue>,
) {
    let pulse = 0.5 + 0.5 * (time.elapsed_secs() * 2.4).sin();
    for (i, s) in view.game.stack.iter().enumerate() {
        if s.target_cards.is_empty() && s.target_players.is_empty() {
            continue;
        }
        let from = s
            .source
            .as_ref()
            .and_then(|c| card_anchor(c.id, &registry, &cards))
            .unwrap_or(anchors.stack + Vec3::Y * 0.3);
        let mine = s.activator.is_some_and(|a| view.is_me(a));
        let color = if mine { Color::srgb(0.25, 0.92, 0.75) } else { Color::srgb(1.0, 0.5, 0.2) };
        let top = if i == 0 { 1.0 } else { 0.7 };
        let mut push = |to: Vec3| {
            let mut a = Arrow::new(from, to, color);
            a.width = 0.06;
            a.intensity = top * (0.8 + 0.2 * pulse);
            a.arc = 0.32;
            queue.0.push(a);
        };
        for t in &s.target_cards {
            if let Some(to) = card_anchor(*t, &registry, &cards) {
                push(to);
            }
        }
        for p in &s.target_players {
            push(player_anchor(&view, &anchors, *p));
        }
    }
}
