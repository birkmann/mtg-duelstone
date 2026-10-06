//! Player avatars: ornate circular portrait, big life orb, flash on life change, glow when it is
//! that player's turn / when targetable, poison & mana-pool pips. Positioned each frame by
//! projecting the layout's avatar anchors through the table camera (so the HUD follows the 3D
//! table when the window resizes).
//!
//! Portraits are card art (`AvatarArt`): yours from settings, everyone else's from the `hello`
//! frame's seats. The procedural silhouette stays underneath as the fallback while art loads.
//!
//! Every widget is keyed by *seat slot* (`GameView::seat_order`: 0 = you, 1.. = opponents left →
//! right), so a four-player table is the same code as a duel: four avatars are spawned, the unused
//! ones stay hidden, and opponents at a crowded table shrink to their column's `ui_scale`.

use bevy::picking::Pickable;
use bevy::prelude::*;
use mtg_protocol::{Avatar, Player, PlayerId};

use super::kit::*;
use super::portrait::make_portrait;
use crate::shared::*;

/// Most seats at a table (the engine's cap).
pub const MAX_SEATS: usize = 4;

#[derive(Component)]
pub struct AvatarBox {
    pub mine: bool,
    pub slot: usize,
}
#[derive(Component)]
pub struct AvatarGlow(pub usize);
#[derive(Component)]
pub struct AvatarPortrait(pub usize);
#[derive(Component)]
pub struct AvatarFlash(pub usize);
#[derive(Component)]
pub struct LifeText(pub usize);
#[derive(Component)]
pub struct LifeOrb(pub usize);
#[derive(Component)]
pub struct AvatarName(pub usize);
#[derive(Component)]
pub struct AvatarBadges(pub usize);
#[derive(Component)]
pub struct ManaRow {
    pub slot: usize,
    pub key: String,
}
#[derive(Component)]
pub struct PileLabel {
    pub kind: PileKind,
    pub slot: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PileKind {
    Library,
    Graveyard,
    Exile,
    Hand,
}

#[derive(Default, Clone, Copy)]
pub struct LifeAnim {
    pub shown: f32,
    pub target: i32,
    pub flash: f32,
    pub gain: bool,
    pub init: bool,
}

#[derive(Resource, Default)]
pub struct AvatarAnim {
    /// per seat slot
    pub slots: [LifeAnim; MAX_SEATS],
    pub portraits_ready: bool,
}

pub const AV: f32 = 12.5; // portrait diameter in vh

pub fn spawn_avatars(mut commands: Commands, mut images: ResMut<Assets<Image>>, fonts: Res<ArenaFonts>, mut anim: ResMut<AvatarAnim>) {
    *anim = AvatarAnim::default();
    for slot in 0..MAX_SEATS {
        // distinct seeds give every seat its own procedural portrait
        let seed = ["you", "opponent", "rival", "challenger"][slot];
        let portrait = images.add(make_portrait(seed, 256));
        spawn_avatar(&mut commands, &fonts, slot, portrait);
    }
}

fn spawn_avatar(commands: &mut Commands, fonts: &ArenaFonts, slot: usize, portrait: Handle<Image>) {
    let mine = slot == 0;
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: vh(AV),
                height: vh(AV),
                left: Val::Px(-1000.0),
                top: Val::Px(-1000.0),
                ..default()
            },
            UiTransform::from_translation(Val2::percent(-50.0, -50.0)),
            AvatarBox { mine, slot },
            GlobalZIndex(8),
            HudEntity,
            Visibility::Hidden,
        ))
        .with_children(|a| {
            // soft glow ring behind the portrait
            a.spawn((
                Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), border_radius: BorderRadius::MAX, ..default() },
                BoxShadow(vec![ShadowStyle { color: Color::NONE, x_offset: Val::Px(0.0), y_offset: Val::Px(0.0), spread_radius: vh(0.8), blur_radius: vh(3.0) }]),
                AvatarGlow(slot),
                Pickable::IGNORE,
            ));
            // portrait with gold ring (clickable when targetable)
            a.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    border: UiRect::all(vh(0.55)),
                    border_radius: BorderRadius::MAX,
                    overflow: Overflow::clip(),
                    ..default()
                },
                ImageNode::new(portrait.clone()),
                AvatarArt { avatar: None, fallback: portrait },
                BorderColor::all(GOLD),
                AvatarPortrait(slot),
                OnClick(Act::ClickPlayer(0)),
            ));
            // outer ornament ring
            a.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: vh(-0.9),
                    top: vh(-0.9),
                    width: vh(AV + 1.8),
                    height: vh(AV + 1.8),
                    border: UiRect::all(vh(0.18)),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BorderColor::all(Color::srgba(0.86, 0.69, 0.36, 0.45)),
                Pickable::IGNORE,
            ));
            // flash overlay
            a.spawn((
                Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), border_radius: BorderRadius::MAX, ..default() },
                BackgroundColor(Color::NONE),
                AvatarFlash(slot),
                Pickable::IGNORE,
            ));
            // life orb
            a.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: vh((AV - 6.6) / 2.0),
                    top: vh(AV - 3.4),
                    width: vh(6.6),
                    height: vh(6.6),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border: UiRect::all(vh(0.4)),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BackgroundGradient::from(RadialGradient::new(
                    UiPosition::CENTER,
                    RadialGradientShape::ClosestSide,
                    vec![ColorStop::new(Color::srgb(0.23, 0.24, 0.30), Val::Percent(0.0)), ColorStop::new(Color::srgb(0.04, 0.045, 0.07), Val::Percent(100.0))],
                )),
                BorderColor::all(GOLD),
                BoxShadow(vec![ShadowStyle { color: Color::srgba(0.0, 0.0, 0.0, 0.7), x_offset: Val::Px(0.0), y_offset: vh(0.3), spread_radius: Val::Px(0.0), blur_radius: vh(1.0) }]),
                LifeOrb(slot),
                Pickable::IGNORE,
            ))
            .with_children(|o| {
                o.spawn((label(fonts, "20", FontKind::Num, 4.0, INK), LifeText(slot)));
            });
            // name + status badges below the orb
            a.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    top: vh(AV + 3.7),
                    left: vh(AV / 2.0 - 12.0),
                    width: vh(24.0),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: vh(0.2),
                    ..default()
                },
                Pickable::IGNORE,
            ))
            .with_children(|n| {
                n.spawn((label(fonts, "", FontKind::Title, 1.7, INK_DIM), AvatarName(slot), Visibility::Hidden));
                n.spawn((label(fonts, "", FontKind::Body, 1.6, GREEN), AvatarBadges(slot)));
                n.spawn((
                    Node { flex_direction: FlexDirection::Row, column_gap: vh(0.4), ..default() },
                    ManaRow { slot, key: String::new() },
                    Pickable::IGNORE,
                ));
            });
        });
}

fn project(
    cams: &Query<(&Camera, &GlobalTransform), With<TableCamera>>,
    world: Vec3,
) -> Option<Vec2> {
    let (cam, gt) = cams.single().ok()?;
    world_to_ui(cam, gt, world)
}

pub fn position_avatars(
    cams: Query<(&Camera, &GlobalTransform), With<TableCamera>>,
    anchors: Res<BoardAnchors>,
    view: Res<GameView>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    hovered_hand: Query<&CardLocation, With<Hovered>>,
    mut q: Query<(&AvatarBox, &mut Node, &mut Visibility, &mut UiTransform)>,
) {
    let Ok(win) = windows.single() else { return };
    let have = view.has_state();
    let seated = view.seat_order().len();
    // a raised hand card should cover my avatar (UI would otherwise draw over it)
    let hand_hot = hovered_hand.iter().any(|l| l.zone == mtg_protocol::ZoneKind::Hand && l.mine);
    for (a, mut n, mut vis, mut tf) in &mut q {
        let seat = anchors.seats.get(a.slot);
        let world = seat.map(|s| s.avatar).unwrap_or_else(|| anchors.avatar(a.mine));
        let fallback = Vec2::new(win.width() * 0.5, if a.mine { win.height() * 0.80 } else { win.height() * 0.10 });
        let pos = if world == Vec3::ZERO { fallback } else { project(&cams, world).unwrap_or(fallback) };
        n.left = Val::Px(pos.x);
        n.top = Val::Px(pos.y);
        let scale = Vec2::splat(seat.map(|s| s.ui_scale).filter(|s| *s > 0.0).unwrap_or(1.0));
        if tf.scale != scale {
            tf.scale = scale;
        }
        let present = a.slot < seated.max(2);
        let want = if have && present && !(a.mine && hand_hot) { Visibility::Inherited } else { Visibility::Hidden };
        if *vis != want {
            *vis = want;
        }
    }
}

fn player_for(view: &GameView, slot: usize) -> Option<&Player> {
    if slot == 0 { view.me() } else { view.player_in_slot(slot) }
}

/// Whose face goes in each portrait: mine is always the one in settings (so a fresh pick shows up
/// at once); others come from the engine. An engine too old to send portraits still shows a
/// commander's art in Commander.
fn portrait_of(view: &GameView, settings: &Settings, seats: &SeatAvatars, slot: usize) -> Option<Avatar> {
    let p = player_for(view, slot)?;
    if slot == 0 {
        return Some(settings.shown_avatar());
    }
    if let Some(a) = seats.0.get(&p.name) {
        return Some(a.clone());
    }
    let cmd = *p.commanders.first()?;
    let card = p.zones.iter().find(|c| c.id == cmd)?;
    Some(Avatar::framed(card.state.as_ref()?.name.clone()))
}

pub fn assign_portraits(view: Res<GameView>, settings: Res<Settings>, seats: Res<SeatAvatars>, mut q: Query<(&AvatarPortrait, &mut AvatarArt)>) {
    if !(view.is_changed() || settings.is_changed() || seats.is_changed()) {
        return;
    }
    for (slot, mut art) in &mut q {
        let want = portrait_of(&view, &settings, &seats, slot.0);
        if art.avatar != want {
            art.avatar = want;
        }
    }
}

pub fn update_avatars(
    time: Res<Time>,
    view: Res<GameView>,
    input: Res<CurrentInput>,
    mut anim: ResMut<AvatarAnim>,
    mut texts: ParamSet<(
        Query<(&LifeText, &mut Text, &mut TextColor)>,
        Query<(&AvatarName, &mut Text, &mut Visibility)>,
        Query<(&AvatarBadges, &mut Text), Without<AvatarName>>,
    )>,
    mut flashes: Query<(&AvatarFlash, &mut BackgroundColor)>,
    mut glows: Query<(&AvatarGlow, &mut BoxShadow)>,
    mut portraits: Query<(&AvatarPortrait, &mut OnClick, &mut BorderColor)>,
    mut orbs: Query<(&LifeOrb, &mut BorderColor), Without<AvatarPortrait>>,
    mut events: MessageReader<BoardEvent>,
) {
    let dt = time.delta_secs();
    let t = time.elapsed_secs();
    for ev in events.read() {
        let _ = ev; // life is diffed below; the event stream is drained to avoid backlog
    }
    // at a crowded table you can't tell opponents apart by position alone: always name them
    let crowd = view.is_multiplayer();
    for slot in 0..MAX_SEATS {
        let mine = slot == 0;
        let a = &mut anim.slots[slot];
        let Some(p) = player_for(&view, slot) else { continue };
        if !a.init {
            a.init = true;
            a.shown = p.life as f32;
            a.target = p.life;
        }
        if p.life != a.target {
            a.gain = p.life > a.target;
            a.target = p.life;
            a.flash = 1.0;
        }
        // number rolls toward the target
        let diff = a.target as f32 - a.shown;
        a.shown += diff * (1.0 - (-dt * 9.0).exp());
        if diff.abs() < 0.05 {
            a.shown = a.target as f32;
        }
        a.flash = (a.flash - dt * 1.1).max(0.0);

        for (l, mut text, mut col) in &mut texts.p0() {
            if l.0 == slot {
                let s = a.shown.round().to_string();
                if text.0 != s {
                    text.0 = s;
                }
                let c = if a.flash > 0.0 {
                    if a.gain { GREEN } else { RED }
                } else if p.has_lost {
                    INK_DIM
                } else if p.life <= 5 {
                    RED
                } else {
                    INK
                };
                if col.0 != c {
                    col.0 = c;
                }
            }
        }
        for (n, mut text, mut vis) in &mut texts.p1() {
            if n.0 == slot {
                let mut name = if p.name.is_empty() { if mine { "You".to_string() } else { "Opponent".to_string() } } else { p.name.clone() };
                if p.has_lost {
                    name.push_str(" — out");
                }
                if text.0 != name {
                    text.0 = name;
                }
                if crowd && !mine && *vis != Visibility::Inherited {
                    *vis = Visibility::Inherited;
                }
            }
        }
        for (b, mut text) in &mut texts.p2() {
            if b.0 == slot {
                let mut parts: Vec<String> = Vec::new();
                for (k, n) in &p.counters {
                    if *n > 0 {
                        let nm = match k.as_str() {
                            "POISON" => "Poison",
                            "ENERGY" => "Energy",
                            "EXPERIENCE" => "Experience",
                            other => other,
                        };
                        parts.push(format!("{nm} {n}"));
                    }
                }
                // commander damage *this* player has taken, from the most dangerous commander
                let cmd: i32 = p.commander_damage.values().copied().max().unwrap_or(0);
                if cmd > 0 {
                    parts.push(format!("Cmdr dmg {cmd}"));
                }
                let s = parts.join("  ·  ");
                if text.0 != s {
                    text.0 = s;
                }
            }
        }
        for (f, mut bg) in &mut flashes {
            if f.0 == slot {
                let c = if a.gain { Color::srgba(0.2, 1.0, 0.4, 0.55 * a.flash) } else { Color::srgba(1.0, 0.1, 0.05, 0.6 * a.flash) };
                bg.0 = c;
            }
        }
        let targetable = input.player_selectable(p.id);
        let active = view.game.player_turn == Some(p.id);
        let pulse = 0.5 + 0.5 * (t * 4.0).sin();
        for (g, mut sh) in &mut glows {
            if g.0 == slot {
                let color = if targetable {
                    BLUE_GLOW.with_alpha(0.55 + 0.4 * pulse)
                } else if p.has_priority && active {
                    Color::srgba(1.0, 0.72, 0.28, 0.7)
                } else if active {
                    Color::srgba(1.0, 0.72, 0.28, 0.4)
                } else {
                    Color::srgba(0.0, 0.0, 0.0, 0.0)
                };
                if let Some(s) = sh.0.first_mut() {
                    s.color = color;
                    s.spread_radius = vh(if targetable { 0.9 + 0.5 * pulse } else { 0.7 });
                }
            }
        }
        for (pt, mut click, mut bc) in &mut portraits {
            if pt.0 == slot {
                click.0 = Act::ClickPlayer(p.id);
                *bc = BorderColor::all(if targetable { BLUE_GLOW } else { GOLD });
            }
        }
        for (o, mut bc) in &mut orbs {
            if o.0 == slot {
                *bc = BorderColor::all(if a.flash > 0.0 { if a.gain { GREEN } else { RED } } else { GOLD });
            }
        }
    }
}

/// Mana pool pips: rebuilt only when the pool contents change.
pub fn update_mana_pools(
    mut commands: Commands,
    view: Res<GameView>,
    fonts: Res<ArenaFonts>,
    mut rows: Query<(Entity, &mut ManaRow)>,
) {
    for (e, mut row) in &mut rows {
        let Some(p) = player_for(&view, row.slot) else { continue };
        let mut key = String::new();
        for sym in ["W", "U", "B", "R", "G", "C"] {
            let n = p.mana_pool.get(sym).copied().unwrap_or(0);
            if n > 0 {
                key.push_str(&format!("{sym}{n},"));
            }
        }
        if key == row.key {
            continue;
        }
        row.key = key;
        commands.entity(e).despawn_children();
        let pool: Vec<(String, i32)> =
            ["W", "U", "B", "R", "G", "C"].iter().filter_map(|s| p.mana_pool.get(*s).copied().filter(|n| *n > 0).map(|n| (s.to_string(), n))).collect();
        let fonts = fonts.clone();
        commands.entity(e).with_children(|r| {
            for (sym, n) in pool {
                r.spawn(ghost(Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, column_gap: vh(0.15), ..default() }))
                    .with_children(|c| {
                        spawn_mana_pip(c, &fonts, &sym, 2.6);
                        c.spawn(label(&fonts, format!("{n}"), FontKind::Num, 2.0, INK));
                    });
            }
        });
    }
}

// ---- pile counters ---------------------------------------------------------------------------

pub fn spawn_pile_labels(mut commands: Commands, fonts: Res<ArenaFonts>) {
    for slot in 0..MAX_SEATS {
        let mine = slot == 0;
        for kind in [PileKind::Library, PileKind::Graveyard, PileKind::Exile, PileKind::Hand] {
            if kind == PileKind::Hand && mine {
                continue; // my hand is right there on screen
            }
            commands.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(-500.0),
                    top: Val::Px(-500.0),
                    padding: UiRect::axes(vh(0.9), vh(0.25)),
                    border: UiRect::all(vh(0.12)),
                    border_radius: BorderRadius::all(vh(0.8)),
                    ..default()
                },
                UiTransform::from_translation(Val2::percent(-50.0, 0.0)),
                BackgroundColor(Color::srgba(0.03, 0.03, 0.05, 0.78)),
                BorderColor::all(GOLD_DIM),
                PileLabel { kind, slot },
                GlobalZIndex(4),
                HudEntity,
                Pickable::IGNORE,
                Visibility::Hidden,
            ))
            .with_children(|p| {
                p.spawn(label(&fonts, "", FontKind::Num, 1.7, INK));
            });
        }
    }
}

pub fn update_pile_labels(
    cams: Query<(&Camera, &GlobalTransform), With<TableCamera>>,
    anchors: Res<BoardAnchors>,
    view: Res<GameView>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mut q: Query<(&PileLabel, &mut Node, &mut Visibility, &Children)>,
    mut texts: Query<&mut Text>,
) {
    let Ok(win) = windows.single() else { return };
    for (pl, mut node, mut vis, children) in &mut q {
        let mine = pl.slot == 0;
        let Some(p) = player_for(&view, pl.slot) else {
            if *vis != Visibility::Hidden {
                *vis = Visibility::Hidden;
            }
            continue;
        };
        let Some(seat) = anchors.seats.get(pl.slot) else { continue };
        let crowd = seat.ui_scale < 1.0;
        let (world, n, tag) = match pl.kind {
            PileKind::Library => (seat.library, p.zones.library.len(), if crowd { "Lib" } else { "Library" }),
            PileKind::Graveyard => (seat.graveyard, p.zones.graveyard.len(), if crowd { "GY" } else { "Graveyard" }),
            PileKind::Exile => (seat.exile, p.zones.exile.len(), if crowd { "Ex" } else { "Exile" }),
            PileKind::Hand => (seat.avatar, p.zones.hand.len(), "Hand"),
        };
        let hide = world == Vec3::ZERO || (n == 0 && pl.kind != PileKind::Library);
        let pos = if hide { None } else { cams.single().ok().and_then(|(c, g)| world_to_ui(c, g, world)) };
        let want = if pos.is_some() { Visibility::Inherited } else { Visibility::Hidden };
        if *vis != want {
            *vis = want;
        }
        if let Some(pos) = pos {
            // below the pile for me, above it for the opponent so labels stay on the visible side
            let dy = if mine { win.height() * 0.055 } else { -win.height() * 0.075 * seat.ui_scale.max(0.6) };
            if pl.kind == PileKind::Hand && crowd {
                // left of the (smaller) avatar's phase track: below it are the name and badges
                node.left = Val::Px(pos.x - win.height() * 0.2 * seat.ui_scale);
                node.top = Val::Px(pos.y - win.height() * 0.018);
            } else if pl.kind == PileKind::Hand {
                // beside the opponent's phase track, clear of the library pile
                node.left = Val::Px(pos.x + win.height() * 0.28);
                node.top = Val::Px(pos.y - win.height() * 0.02);
            } else {
                node.left = Val::Px(pos.x);
                node.top = Val::Px((pos.y + dy).clamp(0.0, win.height()));
            }
        }
        if let Some(&c0) = children.first()
            && let Ok(mut t) = texts.get_mut(c0)
        {
            let s = format!("{tag} {n}");
            if t.0 != s {
                t.0 = s;
            }
        }
    }
}

#[allow(dead_code)]
pub fn player_id(view: &GameView, slot: usize) -> Option<PlayerId> {
    player_for(view, slot).map(|p| p.id)
}
