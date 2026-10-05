//! Bottom-right action cluster (orange "Next / To Combat" pill, fast-forward, Attack All), the
//! input message bar, turn tags, and the click → `ClientFrame` translation for all of them.
//!
//! The engine drives everything: labels come from `input.ok/cancel`, mapped to Arena wording by
//! `model`; a click sends the frame and sets `CurrentInput.awaiting` so the buttons grey out until
//! the next input arrives (rule 4 in engine-integration.md).

use bevy::picking::Pickable;
use bevy::prelude::*;
use mtg_protocol::{ClickAction, ClientFrame};

use super::kit::*;
use super::model::*;
use crate::shared::*;

#[derive(Component)]
pub struct PrimaryBtn;
#[derive(Component)]
pub struct PrimaryLabel;
#[derive(Component)]
pub struct PrimarySub;
#[derive(Component)]
pub struct SecondaryPill;
#[derive(Component)]
pub struct SecondaryPillLabel;
#[derive(Component)]
pub struct FastForwardBtn;
#[derive(Component)]
pub struct MessageBar;
#[derive(Component)]
pub struct MessageText;
#[derive(Component)]
pub struct MessageMana;
#[derive(Component)]
pub struct TurnTag;
#[derive(Component)]
pub struct NameTag(pub bool);

pub fn spawn_buttons(mut commands: Commands, fonts: Res<ArenaFonts>) {
    // ---- action cluster ----
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: vw(1.8),
                bottom: vh(2.6),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexEnd,
                row_gap: vh(0.7),
                ..default()
            },
            GlobalZIndex(9),
            HudEntity,
            Pickable::IGNORE,
        ))
        .with_children(|c| {
            // Attack All / generic secondary pill (hidden unless used)
            let mut sp = spawn_button(c, &fonts, "Attack All", Act::Secondary, BtnStyle::DARK, vh(22.0), vh(5.2), 2.3, true);
            sp.insert(SecondaryPill);
            sp.entry::<Node>().and_modify(|mut n| n.display = Display::None);

            // primary pill
            let mut pb = c.spawn((
                Node {
                    width: vh(30.0),
                    height: vh(7.4),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border: UiRect::all(vh(0.25)),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BackgroundColor(BtnStyle::ORANGE.base),
                BorderColor::all(Color::srgb(1.0, 0.82, 0.55)),
                BoxShadow(vec![ShadowStyle { color: Color::srgba(1.0, 0.45, 0.1, 0.45), x_offset: Val::Px(0.0), y_offset: Val::Px(0.0), spread_radius: vh(0.2), blur_radius: vh(2.4) }]),
                BtnStyle::ORANGE,
                OnClick(Act::Primary),
                PrimaryBtn,
            ));
            pb.with_children(|b| {
                // glossy top half
                b.spawn((
                    Node { position_type: PositionType::Absolute, left: vh(1.2), right: vh(1.2), top: vh(0.35), height: vh(2.8), border_radius: BorderRadius::all(vh(1.8)), ..default() },
                    BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.16)),
                    Pickable::IGNORE,
                ));
                b.spawn((label(&fonts, "Next", FontKind::Title, 3.5, Color::WHITE), PrimaryLabel));
            });
            c.spawn((label(&fonts, "", FontKind::Body, 1.8, INK_DIM), PrimarySub, Node { margin: UiRect::right(vh(2.0)), ..default() }));

            // fast-forward + small row
            c.spawn(ghost(Node { flex_direction: FlexDirection::Row, column_gap: vh(1.0), align_items: AlignItems::Center, ..default() }))
                .with_children(|r| {
                    let mut ff = spawn_button(r, &fonts, "", Act::Secondary, BtnStyle::ORANGE, vh(7.0), vh(4.2), 2.0, true);
                    ff.insert(FastForwardBtn);
                    ff.with_children(|f| {
                        spawn_chevron(f, Dir::Right, 2.8, Color::WHITE);
                        spawn_chevron(f, Dir::Right, 2.8, Color::WHITE);
                    });
                    ff.entry::<Node>().and_modify(|mut n| {
                        n.column_gap = vh(-1.4);
                        n.padding = UiRect::ZERO;
                    });
                });
        });

    // ---- message bar ----
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(50.0),
                bottom: vh(25.0),
                padding: UiRect::axes(vh(2.6), vh(0.9)),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: vh(1.2),
                border: UiRect::all(vh(0.15)),
                border_radius: BorderRadius::MAX,
                display: Display::None,
                ..default()
            },
            UiTransform::from_translation(Val2::percent(-50.0, 0.0)),
            BackgroundColor(Color::srgba(0.02, 0.025, 0.04, 0.82)),
            BorderColor::all(GOLD_DIM),
            GlobalZIndex(9),
            MessageBar,
            HudEntity,
            Pickable::IGNORE,
        ))
        .with_children(|b| {
            b.spawn((label(&fonts, "", FontKind::Body, 2.1, INK), MessageText));
            b.spawn((ghost(Node { flex_direction: FlexDirection::Row, column_gap: vh(0.25), ..default() }), MessageMana));
        });

    // ---- name tags + turn indicator (top-left / bottom-left like Arena) ----
    for mine in [false, true] {
        commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: vw(1.2),
                    top: if mine { Val::Auto } else { vh(1.6) },
                    bottom: if mine { vh(1.6) } else { Val::Auto },
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: vh(1.0),
                    ..default()
                },
                GlobalZIndex(3),
                HudEntity,
                Pickable::IGNORE,
            ))
            .with_children(|t| {
                t.spawn((
                    Node { width: vh(2.6), height: vh(2.6), border: UiRect::all(vh(0.2)), ..default() },
                    UiTransform::from_rotation(Rot2::degrees(45.0)),
                    BackgroundColor(Color::srgba(0.05, 0.05, 0.07, 0.9)),
                    BorderColor::all(GOLD),
                    Pickable::IGNORE,
                ));
                t.spawn((label(&fonts, if mine { "You" } else { "Opponent" }, FontKind::Title, 2.6, INK), NameTag(mine)));
            });
    }
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: vw(1.2),
            top: vh(6.2),
            padding: UiRect::axes(vh(1.2), vh(0.5)),
            border_radius: BorderRadius::all(vh(0.8)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.5)),
        label(&fonts, "", FontKind::Body, 1.7, INK),
        TurnTag,
        GlobalZIndex(3),
        HudEntity,
    ));
}

pub fn update_buttons(
    view: Res<GameView>,
    input: Res<CurrentInput>,
    mut commands: Commands,
    mut texts: Query<&mut Text>,
    mut primary: Query<(Entity, &Children, Has<BtnOff>, &mut BoxShadow), With<PrimaryBtn>>,
    plabel: Query<Entity, With<PrimaryLabel>>,
    psub: Query<Entity, With<PrimarySub>>,
    mut styles: Query<&mut BtnStyle>,
    mut pill: Query<(Entity, &Children, &mut Node, Has<BtnOff>), (With<SecondaryPill>, Without<FastForwardBtn>)>,
    mut ff: Query<(Entity, &mut Node, Has<BtnOff>), (With<FastForwardBtn>, Without<SecondaryPill>)>,
    mut vis: Query<&mut Visibility>,
) {
    let _ = &mut vis;
    let pm = primary_model(&view, &input);
    let sm = secondary_model(&input);
    let in_mull = crate::prompts::is_mulligan_input(&input);
    for (e, _children, off, mut glow) in &mut primary {
        if let Some(sh) = glow.0.first_mut() {
            sh.color = if pm.enabled && !pm.passive { Color::srgba(1.0, 0.45, 0.1, 0.5) } else { Color::NONE };
        }
        if pm.enabled && off {
            commands.entity(e).remove::<BtnOff>();
        } else if !pm.enabled && !off {
            commands.entity(e).insert(BtnOff);
        }
        if let Ok(mut st) = styles.get_mut(e) {
            let want = if pm.passive { BtnStyle::PASSIVE } else { BtnStyle::ORANGE };
            if st.base != want.base {
                *st = want;
            }
        }
        commands.entity(e).insert(BorderColor::all(if pm.enabled && !pm.passive { Color::srgb(1.0, 0.82, 0.55) } else { GOLD_DIM }));
        if in_mull {
            // the mulligan overlay owns the decision; keep the pill but dim it
            commands.entity(e).insert(BtnOff);
        }
    }
    for e in &plabel {
        if let Ok(mut t) = texts.get_mut(e)
            && t.0 != pm.label
        {
            t.0 = pm.label.clone();
        }
    }
    for e in &psub {
        let s = pm.subtitle.clone().unwrap_or_default();
        if let Ok(mut t) = texts.get_mut(e)
            && t.0 != s
        {
            t.0 = s;
        }
    }
    // secondary pill vs fast-forward
    let (show_pill, pill_label, pill_enabled, show_ff, ff_enabled) = match &sm {
        Some(s) if s.kind == SecondaryKind::FastForward => (false, String::new(), false, true, s.enabled),
        Some(s) => (true, s.label.clone(), s.enabled, false, false),
        None => (false, String::new(), false, false, false),
    };
    for (e, children, mut node, off) in &mut pill {
        let d = if show_pill { Display::Flex } else { Display::None };
        if node.display != d {
            node.display = d;
        }
        if pill_enabled && off {
            commands.entity(e).remove::<BtnOff>();
        } else if !pill_enabled && !off {
            commands.entity(e).insert(BtnOff);
        }
        if let Some(&c0) = children.first()
            && let Ok(mut t) = texts.get_mut(c0)
            && t.0 != pill_label
        {
            t.0 = pill_label.clone();
        }
        if show_pill {
            if let Ok(mut st) = styles.get_mut(e) {
                let want = if sm.as_ref().is_some_and(|s| s.kind == SecondaryKind::AttackAll) { BtnStyle::GOLD } else { BtnStyle::DARK };
                if st.base != want.base {
                    *st = want;
                }
            }
        }
    }
    for (e, mut node, off) in &mut ff {
        let d = if show_ff { Display::Flex } else { Display::None };
        if node.display != d {
            node.display = d;
        }
        if ff_enabled && off {
            commands.entity(e).remove::<BtnOff>();
        } else if !ff_enabled && !off {
            commands.entity(e).insert(BtnOff);
        }
    }
}

pub fn update_message_bar(
    mut commands: Commands,
    input: Res<CurrentInput>,
    fonts: Res<ArenaFonts>,
    mut bar: Query<&mut Node, With<MessageBar>>,
    mut text: Query<&mut Text, With<MessageText>>,
    mana: Query<Entity, With<MessageMana>>,
    view: Res<GameView>,
    cache: Res<CardCache>,
    mut last: Local<String>,
) {
    let msg = input.frame.as_ref().map(|f| f.message.clone()).unwrap_or_default();
    let in_mull = crate::prompts::is_mulligan_input(&input);
    let subject = input.frame.as_ref().and_then(|f| f.card).and_then(|id| view.card(id).or_else(|| cache.0.get(&id))).map(|c| c.display_name().to_string());
    let cleaned = if in_mull { None } else { clean_message(&msg, subject.as_deref()) };
    let paying = if is_mana_payment(&msg) { mana_to_pay(&msg) } else { None };
    let shown = match (&cleaned, &paying) {
        (_, Some(_)) => match &subject {
            Some(n) => format!("{n}  -  Pay"),
            None => "Pay".to_string(),
        },
        (Some(c), None) => c.clone(),
        _ => String::new(),
    };
    for mut n in &mut bar {
        let d = if shown.is_empty() { Display::None } else { Display::Flex };
        if n.display != d {
            n.display = d;
        }
    }
    for mut t in &mut text {
        if t.0 != shown {
            t.0 = shown.clone();
        }
    }
    let key = format!("{}|{}", paying.clone().unwrap_or_default(), shown);
    if *last != key {
        *last = key;
        for e in &mana {
            commands.entity(e).despawn_children();
            if let Some(cost) = &paying {
                let cost = cost.clone();
                let fonts = fonts.clone();
                commands.entity(e).with_children(|r| {
                    for s in mana_symbols(&cost) {
                        spawn_mana_pip(r, &fonts, &s, 3.2);
                    }
                });
            }
        }
    }
}

pub fn update_tags(
    view: Res<GameView>,
    mut tag: Query<&mut Text, (With<TurnTag>, Without<NameTag>)>,
    mut names: Query<(&NameTag, &mut Text), Without<TurnTag>>,
) {
    for mut t in &mut tag {
        let active = view.game.player_turn.and_then(|id| view.game.player(id)).map(|p| p.name.clone()).filter(|n| !n.is_empty());
        let who = if view.my_turn() {
            "Your turn".to_string()
        } else if view.is_multiplayer() {
            active.map(|n| format!("{n}'s turn")).unwrap_or_else(|| "Opponent's turn".into())
        } else {
            "Opponent's turn".to_string()
        };
        let ph = view.phase().map(|p| p.label()).unwrap_or("");
        let s = if view.has_state() { format!("Turn {}  -  {}  -  {}", view.game.turn, who, ph) } else { String::new() };
        if t.0 != s {
            t.0 = s;
        }
    }
    for (n, mut t) in &mut names {
        // the top tag names every opponent at a multiplayer table
        let name = if n.0 {
            view.me().map(|p| p.name.clone())
        } else {
            let names: Vec<String> = view.opponents().iter().map(|p| p.name.clone()).filter(|n| !n.is_empty()).collect();
            (!names.is_empty()).then(|| names.join("  ·  "))
        };
        if let Some(name) = name.filter(|n| !n.is_empty())
            && t.0 != name
        {
            t.0 = name;
        }
    }
}

pub fn handle_button_acts(
    mut acts: MessageReader<Acted>,
    mut input: ResMut<CurrentInput>,
    view: Res<GameView>,
    mut out: MessageWriter<SendCmd>,
) {
    for Acted(a) in acts.read() {
        if input.awaiting && matches!(a, Act::Primary | Act::Secondary | Act::AttackAll | Act::PassTurn) {
            continue;
        }
        let frame = match a {
            Act::Primary => {
                if !input.frame.as_ref().is_some_and(|f| f.ok.enabled) {
                    continue;
                }
                ClientFrame::ok()
            }
            Act::Secondary => match secondary_model(&input) {
                Some(s) if s.enabled => match s.kind {
                    SecondaryKind::AttackAll => ClientFrame::Click { action: ClickAction::AlphaStrike, id: None },
                    _ => ClientFrame::cancel(),
                },
                _ => continue,
            },
            Act::AttackAll => ClientFrame::Click { action: ClickAction::AlphaStrike, id: None },
            Act::Undo => ClientFrame::Click { action: ClickAction::Undo, id: None },
            Act::PassTurn => {
                // Forge's cancel at priority = "End Turn"; anywhere else a plain pass is safer
                let cancel_ends_turn = input
                    .frame
                    .as_ref()
                    .is_some_and(|f| f.cancel.enabled && is_priority(&f.message) && !f.cancel.label.is_empty());
                if cancel_ends_turn { ClientFrame::cancel() } else { ClientFrame::Click { action: ClickAction::Pass, id: None } }
            }
            Act::ClickPlayer(id) => {
                if *id == 0 || !input.player_selectable(*id) {
                    continue;
                }
                ClientFrame::click_player(*id)
            }
            _ => continue,
        };
        let _ = &view;
        input.awaiting = true;
        out.write(SendCmd(frame));
    }
}
