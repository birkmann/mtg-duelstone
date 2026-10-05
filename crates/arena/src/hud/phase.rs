//! Phase track: compact diamonds / card / swords icons flanking each avatar, with the
//! current phase glowing orange. Hovering MY track expands a full 13-phase strip whose pills toggle
//! stops (`ClientFrame::Settings`).

use bevy::picking::Pickable;
use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use mtg_protocol::{ClientFrame, PhaseType, StopsPatch};

use super::avatar::{AV, AvatarBox};
use super::kit::*;
use super::model::*;
use crate::shared::*;

#[derive(Component)]
pub struct PhaseZone;
#[derive(Component)]
pub struct PhaseIcon {
    pub mine: bool,
    pub group: Group,
}
#[derive(Component)]
pub struct PhaseShape;
#[derive(Component)]
pub struct StopDot;
#[derive(Component)]
pub struct StopStrip;
#[derive(Component)]
pub struct StopBtn {
    pub phase: PhaseType,
    pub mine_turn: bool,
}

#[derive(Resource, Default)]
pub struct PhaseUi {
    pub pinned: bool,
}

const ICON: f32 = 4.6;

fn rep_phase(g: Group) -> PhaseType {
    match g {
        Group::Begin => PhaseType::Upkeep,
        Group::Main1 => PhaseType::Main1,
        Group::Combat => PhaseType::CombatDeclareAttackers,
        Group::Main2 => PhaseType::Main2,
        Group::End => PhaseType::EndOfTurn,
    }
}

fn shape_bundle(group: Group) -> Vec<(Node, f32)> {
    // (node, rotation degrees)
    let s = ICON;
    match group {
        Group::Begin | Group::End => vec![(
            Node { width: vh(s * 0.42), height: vh(s * 0.42), border: UiRect::all(vh(0.16)), border_radius: BorderRadius::all(vh(0.15)), ..default() },
            45.0,
        )],
        Group::Main1 | Group::Main2 => vec![(
            Node { width: vh(s * 0.52), height: vh(s * 0.74), border: UiRect::all(vh(0.2)), border_radius: BorderRadius::all(vh(0.3)), ..default() },
            0.0,
        )],
        Group::Combat => vec![
            (Node { width: vh(s * 0.13), height: vh(s * 0.95), position_type: PositionType::Absolute, border_radius: BorderRadius::all(vh(0.2)), ..default() }, 45.0),
            (Node { width: vh(s * 0.13), height: vh(s * 0.95), position_type: PositionType::Absolute, border_radius: BorderRadius::all(vh(0.2)), ..default() }, -45.0),
        ],
    }
}

fn spawn_group(p: &mut ChildSpawnerCommands, mine: bool, groups: &[Group]) {
    p.spawn((
        Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, column_gap: vh(0.5), ..default() },
        PhaseZone,
        Pickable::IGNORE,
    ))
    .with_children(|row| {
        for &g in groups {
            let mut w = row.spawn((
                Node {
                    width: vh(ICON),
                    height: vh(ICON),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BoxShadow(vec![ShadowStyle { color: Color::NONE, x_offset: Val::Px(0.0), y_offset: Val::Px(0.0), spread_radius: vh(0.2), blur_radius: vh(1.4) }]),
                PhaseIcon { mine, group: g },
            ));
            if mine {
                w.insert((OnClick(Act::ToggleStopHere(rep_phase(g))), BtnStyle::GHOST, BackgroundColor(Color::NONE)));
            } else {
                w.insert(Pickable::IGNORE);
            }
            w.with_children(|c| {
                for (n, rot) in shape_bundle(g) {
                    c.spawn((n, UiTransform::from_rotation(Rot2::degrees(rot)), BackgroundColor(Color::NONE), BorderColor::all(GOLD_DIM), PhaseShape, Pickable::IGNORE));
                }
                // stop marker
                c.spawn((
                    Node { position_type: PositionType::Absolute, top: vh(-0.2), width: vh(0.8), height: vh(0.8), border_radius: BorderRadius::MAX, ..default() },
                    BackgroundColor(Color::NONE),
                    StopDot,
                    Pickable::IGNORE,
                ));
            });
        }
    });
}

pub fn attach_phase_tracks(mut commands: Commands, fonts: Res<ArenaFonts>, q: Query<(Entity, &AvatarBox), Added<AvatarBox>>) {
    for (e, a) in &q {
        let mine = a.mine;
        commands.entity(e).with_children(|av| {
            // left group, right group, both level with the orb
            av.spawn((
                Node { position_type: PositionType::Absolute, right: vh(AV + 1.8), top: vh(AV * 0.5), ..default() },
                Pickable::IGNORE,
            ))
            .with_children(|l| spawn_group(l, mine, &[Group::Begin, Group::Main1]));
            av.spawn((
                Node { position_type: PositionType::Absolute, left: vh(AV + 1.8), top: vh(AV * 0.5), ..default() },
                Pickable::IGNORE,
            ))
            .with_children(|r| spawn_group(r, mine, &[Group::Combat, Group::Main2, Group::End]));
            if mine {
                spawn_strip(av, &fonts);
            }
        });
    }
}

fn spawn_strip(av: &mut ChildSpawnerCommands, fonts: &ArenaFonts) {
    av.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: vh(AV / 2.0 - 30.0),
            top: vh(-17.5),
            width: vh(60.0),
            flex_direction: FlexDirection::Column,
            row_gap: vh(0.6),
            padding: UiRect::axes(vh(1.2), vh(0.9)),
            border: UiRect::all(vh(0.2)),
            border_radius: BorderRadius::all(vh(1.0)),
            display: Display::None,
            ..default()
        },
        BackgroundColor(Color::srgba(0.03, 0.035, 0.055, 0.93)),
        BorderColor::all(GOLD_DIM),
        StopStrip,
        PhaseZone,
    ))
    .with_children(|s| {
        for mine_turn in [true, false] {
            s.spawn(ghost(Node { flex_direction: FlexDirection::Column, row_gap: vh(0.3), ..default() })).with_children(|row| {
                row.spawn(label(fonts, if mine_turn { "STOPS · MY TURN" } else { "STOPS · OPPONENT'S TURN" }, FontKind::Title, 1.3, GOLD));
                row.spawn(ghost(Node { flex_direction: FlexDirection::Row, column_gap: vh(0.3), flex_wrap: FlexWrap::NoWrap, ..default() })).with_children(|r| {
                    for ph in PhaseType::ALL {
                        r.spawn((
                            Node {
                                width: vh(4.0),
                                height: vh(3.0),
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                border: UiRect::all(vh(0.15)),
                                border_radius: BorderRadius::all(vh(0.5)),
                                ..default()
                            },
                            BackgroundColor(Color::NONE),
                            BorderColor::all(GOLD_DIM),
                            BtnStyle::GHOST,
                            OnClick(Act::ToggleStop { phase: ph, mine_turn }),
                            StopBtn { phase: ph, mine_turn },
                        ))
                        .with_children(|b| {
                            b.spawn(label(fonts, short_label(ph), FontKind::Num, 1.15, INK));
                        });
                    }
                });
            });
        }
    });
}

fn group_index(g: Group) -> usize {
    match g {
        Group::Begin => 0,
        Group::Main1 => 1,
        Group::Combat => 2,
        Group::Main2 => 3,
        Group::End => 4,
    }
}

pub fn update_phase_icons(
    time: Res<Time>,
    view: Res<GameView>,
    icons: Query<(&PhaseIcon, &Children, Entity)>,
    mut shapes: Query<(&mut BackgroundColor, &mut BorderColor), (With<PhaseShape>, Without<StopDot>)>,
    mut dots: Query<&mut BackgroundColor, (With<StopDot>, Without<PhaseShape>)>,
    mut shadows: Query<&mut BoxShadow>,
) {
    let cur = view.phase();
    let cur_group = cur.map(group_of);
    let t = time.elapsed_secs();
    for (icon, children, e) in &icons {
        // only the player whose turn it is shows progress
        let owner_active = if icon.mine { view.my_turn() } else { view.you.is_some() && !view.my_turn() && view.game.player_turn.is_some() };
        let (state, glow) = match (owner_active, cur_group) {
            (true, Some(cg)) => {
                if cg == icon.group {
                    (2, 0.75 + 0.25 * (t * 3.0).sin())
                } else if group_index(icon.group) < group_index(cg) {
                    (1, 0.0)
                } else {
                    (0, 0.0)
                }
            }
            _ => (0, 0.0),
        };
        let (fill, border) = match state {
            2 => (ORANGE_HI, Color::srgb(1.0, 0.85, 0.5)),
            1 => (Color::srgba(0.50, 0.40, 0.22, 0.95), GOLD),
            _ => (Color::srgba(0.06, 0.06, 0.08, 0.9), Color::srgba(0.82, 0.78, 0.66, 0.95)),
        };
        let stop = if icon.mine {
            let stops = if view.my_turn() { &view.stops.my_turn } else { &view.stops.opp_turn };
            stops.iter().any(|p| group_of(*p) == icon.group)
        } else {
            false
        };
        for c in children.iter() {
            if let Ok((mut bg, mut bc)) = shapes.get_mut(c) {
                // combat swords are bars: fill with the border colour; card/diamond: fill + border
                bg.0 = if icon.group == Group::Combat { border } else { fill };
                *bc = BorderColor::all(border);
            }
            if let Ok(mut d) = dots.get_mut(c) {
                d.0 = if stop { GOLD } else { Color::NONE };
            }
        }
        if let Ok(mut sh) = shadows.get_mut(e)
            && let Some(s) = sh.0.first_mut()
        {
            s.color = Color::srgba(1.0, 0.55, 0.15, glow * 0.8);
        }
    }
}

pub fn update_strip(
    view: Res<GameView>,
    ui: Res<PhaseUi>,
    hover: Res<HoverMap>,
    parents: Query<&ChildOf>,
    zones: Query<(), With<PhaseZone>>,
    mut strips: Query<&mut Node, With<StopStrip>>,
    mut btns: Query<(&StopBtn, &mut BackgroundColor, &mut BorderColor, &mut BtnStyle)>,
) {
    let mut hovered = false;
    'outer: for map in hover.values() {
        for &e in map.keys() {
            let mut cur = Some(e);
            let mut hops = 0;
            while let Some(c) = cur {
                if zones.contains(c) {
                    hovered = true;
                    break 'outer;
                }
                cur = parents.get(c).ok().map(|p| p.parent());
                hops += 1;
                if hops > 12 {
                    break;
                }
            }
        }
    }
    let show = hovered || ui.pinned;
    for mut n in &mut strips {
        let d = if show { Display::Flex } else { Display::None };
        if n.display != d {
            n.display = d;
        }
    }
    for (b, mut bg, mut bc, mut st) in &mut btns {
        let stops = if b.mine_turn { &view.stops.my_turn } else { &view.stops.opp_turn };
        let on = stops.contains(&b.phase);
        let current = view.phase() == Some(b.phase) && view.my_turn() == b.mine_turn;
        *st = if on { BtnStyle { base: Color::srgba(0.7, 0.54, 0.24, 0.95), hover: Color::srgb(0.85, 0.66, 0.3), press: Color::srgb(0.5, 0.38, 0.17), off: Color::NONE } } else { BtnStyle::GHOST };
        let _ = &mut bg;
        *bc = BorderColor::all(if current { ORANGE_HI } else if on { GOLD } else { GOLD_DIM });
    }
}

pub fn handle_stop_acts(mut acts: MessageReader<Acted>, view: Res<GameView>, mut out: MessageWriter<SendCmd>, mut ui: ResMut<PhaseUi>) {
    for Acted(a) in acts.read() {
        let (phase, mine_turn) = match a {
            Act::ToggleStop { phase, mine_turn } => (phase, mine_turn),
            Act::ToggleStopHere(phase) => (phase, &view.my_turn()),
            Act::ToggleStops => {
                ui.pinned = !ui.pinned;
                continue;
            }
            _ => continue,
        };
        {
            {
                let mut set: Vec<PhaseType> = if *mine_turn { view.stops.my_turn.clone() } else { view.stops.opp_turn.clone() };
                if let Some(i) = set.iter().position(|p| p == phase) {
                    set.remove(i);
                } else {
                    set.push(*phase);
                    set.sort();
                }
                let patch = if *mine_turn { StopsPatch { my_turn: Some(set), opp_turn: None } } else { StopsPatch { my_turn: None, opp_turn: Some(set) } };
                out.write(SendCmd(ClientFrame::Settings { stops: patch }));
            }
        }
    }
}

// ---- phase ladder ----------------------------------------------------------------------------
// The icon track above is compact but unreadable for anyone who doesn't already know the
// turn structure, so a labelled ladder on the left edge spells out every step, which one we're
// in, whose turn it is, and where you'll be asked to act (gold dot = stop; click to toggle).

/// The steps the ladder lists, in turn order. Untap/Cleanup never give priority, and first-strike
/// damage is shown on the regular damage row.
pub const LADDER: [PhaseType; 10] = [
    PhaseType::Upkeep,
    PhaseType::Draw,
    PhaseType::Main1,
    PhaseType::CombatBegin,
    PhaseType::CombatDeclareAttackers,
    PhaseType::CombatDeclareBlockers,
    PhaseType::CombatDamage,
    PhaseType::CombatEnd,
    PhaseType::Main2,
    PhaseType::EndOfTurn,
];

fn ladder_label(p: PhaseType) -> &'static str {
    match p {
        PhaseType::CombatBegin => "Begin Combat",
        PhaseType::CombatDeclareAttackers => "Attackers",
        PhaseType::CombatDeclareBlockers => "Blockers",
        PhaseType::CombatDamage => "Damage",
        _ => p.label(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepState {
    Past,
    Current,
    Future,
}

/// Where `step` sits relative to the current phase (pure; unit-tested).
pub fn step_state(step: PhaseType, cur: Option<PhaseType>) -> StepState {
    let Some(cur) = cur else { return StepState::Future };
    let cur = if cur == PhaseType::CombatFirstStrikeDamage { PhaseType::CombatDamage } else { cur };
    match step.cmp(&cur) {
        std::cmp::Ordering::Less => StepState::Past,
        std::cmp::Ordering::Equal => StepState::Current,
        std::cmp::Ordering::Greater => StepState::Future,
    }
}

#[derive(Component)]
pub struct Ladder;
#[derive(Component)]
pub struct LadderHeader;
#[derive(Component)]
pub struct LadderRow(pub PhaseType);
#[derive(Component)]
pub struct LadderText(pub PhaseType);
#[derive(Component)]
pub struct LadderDot(pub PhaseType);

const ROW_ON: BtnStyle = BtnStyle {
    base: Color::srgba(0.93, 0.43, 0.13, 0.92),
    hover: Color::srgb(1.0, 0.58, 0.25),
    press: Color::srgb(0.8, 0.36, 0.1),
    off: Color::NONE,
};

pub fn spawn_ladder(mut commands: Commands, fonts: Res<ArenaFonts>) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: vw(1.0),
                top: vh(20.0),
                width: vh(19.5),
                flex_direction: FlexDirection::Column,
                row_gap: vh(0.25),
                padding: UiRect::axes(vh(0.7), vh(0.8)),
                border: UiRect::all(vh(0.15)),
                border_radius: BorderRadius::all(vh(1.0)),
                display: Display::None,
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.025, 0.04, 0.72)),
            BorderColor::all(GOLD_DIM),
            GlobalZIndex(3),
            Ladder,
            HudEntity,
            Pickable::IGNORE,
        ))
        .with_children(|l| {
            l.spawn((label(&fonts, "", FontKind::Title, 1.75, GOLD), LadderHeader, Node { margin: UiRect::bottom(vh(0.4)), ..default() }));
            for step in LADDER {
                l.spawn((
                    Node {
                        height: vh(2.5),
                        padding: UiRect::axes(vh(0.8), Val::ZERO),
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::SpaceBetween,
                        border_radius: BorderRadius::all(vh(0.6)),
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                    BtnStyle::GHOST,
                    OnClick(Act::ToggleStopHere(step)),
                    LadderRow(step),
                ))
                .with_children(|r| {
                    r.spawn((label(&fonts, ladder_label(step), FontKind::Body, 1.55, INK), LadderText(step)));
                    r.spawn((
                        Node { width: vh(0.9), height: vh(0.9), border: UiRect::all(vh(0.12)), border_radius: BorderRadius::MAX, ..default() },
                        BackgroundColor(Color::NONE),
                        BorderColor::all(GOLD_DIM),
                        LadderDot(step),
                        Pickable::IGNORE,
                    ));
                });
            }
            l.spawn((label(&fonts, "Dot = you stop here. Click to toggle.", FontKind::Body, 1.3, INK_DIM), Node { margin: UiRect::top(vh(0.4)), ..default() }));
        });
}

pub fn update_ladder(
    view: Res<GameView>,
    mut root: Query<&mut Node, With<Ladder>>,
    mut header: Query<(&mut Text, &mut TextColor), (With<LadderHeader>, Without<LadderText>)>,
    mut rows: Query<(&LadderRow, &mut BtnStyle)>,
    mut texts: Query<(&LadderText, &mut TextColor), Without<LadderHeader>>,
    mut dots: Query<(&LadderDot, &mut BackgroundColor, &mut BorderColor)>,
) {
    let live = view.has_state() && view.game.player_turn.is_some();
    for mut n in &mut root {
        let d = if live { Display::Flex } else { Display::None };
        if n.display != d {
            n.display = d;
        }
    }
    if !live {
        return;
    }
    let mine = view.my_turn();
    let cur = view.phase();
    for (mut t, mut c) in &mut header {
        // turn number already sits in the top-left tag; one short line keeps the panel narrow
        let s = if mine { "Your turn" } else { "Opponent's turn" };
        if t.0 != s {
            t.0 = s.to_string();
        }
        let col = if mine { ORANGE_HI } else { Color::srgb(0.55, 0.75, 1.0) };
        if c.0 != col {
            c.0 = col;
        }
    }
    for (row, mut st) in &mut rows {
        let want = if step_state(row.0, cur) == StepState::Current { ROW_ON } else { BtnStyle::GHOST };
        if st.base != want.base {
            *st = want;
        }
    }
    for (t, mut c) in &mut texts {
        let col = match step_state(t.0, cur) {
            StepState::Current => Color::WHITE,
            StepState::Past => Color::srgba(0.66, 0.63, 0.58, 0.55),
            StepState::Future => INK,
        };
        if c.0 != col {
            c.0 = col;
        }
    }
    let stops = if mine { &view.stops.my_turn } else { &view.stops.opp_turn };
    for (d, mut bg, mut bc) in &mut dots {
        let on = stops.contains(&d.0) || (d.0 == PhaseType::CombatDamage && stops.contains(&PhaseType::CombatFirstStrikeDamage));
        let fill = if on { GOLD } else { Color::NONE };
        if bg.0 != fill {
            bg.0 = fill;
        }
        *bc = BorderColor::all(if on { GOLD } else { GOLD_DIM });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ladder_marks_past_current_future() {
        let cur = Some(PhaseType::CombatBegin);
        assert_eq!(step_state(PhaseType::Main1, cur), StepState::Past);
        assert_eq!(step_state(PhaseType::CombatBegin, cur), StepState::Current);
        assert_eq!(step_state(PhaseType::Main2, cur), StepState::Future);
    }

    #[test]
    fn first_strike_lights_the_damage_row() {
        assert_eq!(step_state(PhaseType::CombatDamage, Some(PhaseType::CombatFirstStrikeDamage)), StepState::Current);
        assert_eq!(step_state(PhaseType::CombatDeclareBlockers, Some(PhaseType::CombatFirstStrikeDamage)), StepState::Past);
    }
}
