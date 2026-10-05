//! Blocking engine prompts as Arena-styled modals, the non-modal `trayCards` pick tray and the
//! mulligan overlay.
//!
//! The engine decides everything; every dialog just collects an answer in the exact JSON shape
//! protocol.md documents and sends `ClientFrame::Answer`. `PromptUi` holds the in-progress
//! selection; widgets whose appearance depends on it (selected borders, steppers, the Done
//! button) are updated in place, while reorder lists are simply rebuilt (rare, tiny).

mod debug;
mod logic;
mod tray;

pub use tray::is_mulligan as is_mulligan_input;

use std::collections::BTreeMap;

use bevy::input::keyboard::KeyboardInput;
use bevy::picking::Pickable;
use bevy::prelude::*;
use mtg_client::ImageVersion;
use mtg_protocol::{Card, ChoiceOption, ClientFrame, PromptBody, PromptFrame};
use serde_json::{Value, json};

use crate::hud::kit::*;
use crate::shared::{ArenaFonts, PendingPrompt, SendCmd};
use logic::*;

pub struct PromptsPlugin;

impl Plugin for PromptsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PromptUi>()
            .init_resource::<PendingPrompt>()
            .init_resource::<ArenaFonts>()
            .add_systems(
                Update,
                (sync_prompt, handle_acts, prompt_keys, refresh_in_place).chain(),
            )
            .add_plugins((tray::TrayPlugin, debug::PromptDebugPlugin));
    }
}

#[derive(Resource, Default)]
pub struct PromptUi {
    id: Option<i64>,
    last_answered: Option<i64>,
    root: Option<Entity>,
    sel: Vec<i64>,
    src: Vec<i64>,
    dst: Vec<i64>,
    remember: bool,
    vals: BTreeMap<String, i64>,
    ids: Vec<i64>,
    text: String,
    dirty: bool,
    changed: bool,
}

/// True while a modal prompt is on screen (interact disables game hotkeys).
pub fn prompt_open(p: &PendingPrompt) -> bool {
    p.0.is_some()
}

#[derive(Component)]
struct PromptRoot;
#[derive(Component)]
struct PickOpt(i64);
#[derive(Component)]
struct SubmitBtn;
#[derive(Component)]
struct ValText(String);
#[derive(Component)]
struct TotalText;
#[derive(Component)]
struct TextField;

fn default_title(b: &PromptBody) -> &'static str {
    match b {
        PromptBody::Confirm { .. } => "Confirm",
        PromptBody::Option { .. } => "Choose an Option",
        PromptBody::ChooseAbility { .. } => "Choose an Ability",
        PromptBody::Choices { .. } | PromptBody::ChooseEntity { .. } => "Make Your Choice",
        PromptBody::Order { .. } => "Choose the Order",
        PromptBody::AssignCombatDamage { .. } => "Assign Combat Damage",
        PromptBody::AssignAmount { .. } => "Divide Amount",
        PromptBody::ManipulateCards { .. } => "Arrange Cards",
        PromptBody::Text { .. } => "Enter a Value",
        PromptBody::Unknown => "Continue",
    }
}

// ---- sync ---------------------------------------------------------------------------------

fn init_state(ui: &mut PromptUi, p: &PromptFrame) {
    ui.sel.clear();
    ui.src.clear();
    ui.dst.clear();
    ui.vals.clear();
    ui.ids.clear();
    ui.text.clear();
    ui.remember = false;
    match &p.body {
        PromptBody::Choices { selected, .. } => ui.sel = selected.clone(),
        PromptBody::Order { options, source_count, .. } => {
            let (s, d) = split_order(options, *source_count);
            ui.src = s;
            ui.dst = d;
        }
        PromptBody::AssignCombatDamage { attacker, blockers, damage, defender, .. } => {
            if let Some(a) = attacker {
                ui.vals = default_damage(a, blockers, *damage, defender.is_some());
            }
        }
        PromptBody::AssignAmount { options, .. } => {
            ui.vals = options.iter().map(|o| (o.i.to_string(), o.current.unwrap_or(0))).collect();
        }
        PromptBody::ManipulateCards { cards, .. } => ui.ids = cards.iter().map(|c| c.id).collect(),
        PromptBody::Text { initial, options, numeric } => {
            ui.text = initial.clone().or_else(|| options.first().cloned()).unwrap_or_else(|| if *numeric { "0".into() } else { String::new() });
        }
        _ => {}
    }
}

fn sync_prompt(
    mut commands: Commands,
    pending: Res<PendingPrompt>,
    mut ui: ResMut<PromptUi>,
    fonts: Res<ArenaFonts>,
) {
    if let Some(r) = ui.root
        && commands.get_entity(r).is_err()
    {
        ui.root = None;
        ui.id = None;
    }
    let want = pending.0.as_ref().map(|p| p.id).filter(|id| Some(*id) != ui.last_answered);
    if want != ui.id || (ui.dirty && want.is_some()) {
        if let Some(r) = ui.root.take()
            && let Ok(mut ec) = commands.get_entity(r)
        {
            ec.despawn();
        }
        let fresh = want != ui.id;
        ui.dirty = false;
        ui.id = want;
        if let (Some(_), Some(p)) = (want, pending.0.as_ref()) {
            if fresh {
                init_state(&mut ui, p);
            }
            let root = build(&mut commands, &fonts, p, &ui);
            ui.root = Some(root);
            ui.changed = true;
        }
    }
}

// ---- building -----------------------------------------------------------------------------

fn panel_root<'a>(commands: &'a mut Commands) -> EntityCommands<'a> {
    commands.spawn((
        fullscreen(JustifyContent::Center, AlignItems::Center),
        BackgroundColor(SHADE),
        GlobalZIndex(60),
        PromptRoot,
        HudEntity,
        // swallow pointer events beneath the modal
        Pickable { should_block_lower: true, is_hoverable: true },
    ))
}

fn panel_node() -> Node {
    Node {
        flex_direction: FlexDirection::Column,
        align_items: AlignItems::Center,
        min_width: vh(46.0),
        max_width: vw(92.0),
        max_height: vh(92.0),
        padding: UiRect::axes(vh(3.0), vh(2.4)),
        row_gap: vh(1.6),
        border: UiRect::all(vh(0.28)),
        border_radius: BorderRadius::all(vh(1.6)),
        ..default()
    }
}

fn header(p: &mut ChildSpawnerCommands, fonts: &ArenaFonts, frame: &PromptFrame, sub: Option<String>) {
    let informational = matches!(&frame.body, PromptBody::Choices { min, max, .. } | PromptBody::ChooseEntity { min, max, .. } if *min < 0 && *max < 0);
    let title = frame
        .title
        .clone()
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| if informational { "Information".to_string() } else { default_title(&frame.body).to_string() });
    p.spawn(label(fonts, title.to_uppercase(), FontKind::Title, 3.0, GOLD));
    p.spawn(deco(Node { width: vh(30.0), height: vh(0.2), ..default() }, GOLD_DIM));
    if let Some(m) = frame.message.as_ref().filter(|m| !m.trim().is_empty()) {
        p.spawn((
            label(fonts, m.trim().to_string(), FontKind::Body, 2.2, INK),
            Node { max_width: vh(110.0), ..default() },
            TextLayout { justify: Justify::Center, ..default() },
        ));
    }
    if let Some(s) = sub {
        p.spawn(label(fonts, s, FontKind::Body, 1.8, INK_DIM));
    }
}

fn footer<'a>(p: &'a mut ChildSpawnerCommands) -> EntityCommands<'a> {
    p.spawn(ghost(Node { flex_direction: FlexDirection::Row, column_gap: vh(1.4), justify_content: JustifyContent::Center, flex_wrap: FlexWrap::Wrap, margin: UiRect::top(vh(0.8)), ..default() }))
}

fn big_btn(f: &mut ChildSpawnerCommands, fonts: &ArenaFonts, text: &str, act: PromptAct, primary: bool, enabled: bool) -> Entity {
    let style = if primary { BtnStyle::ORANGE } else { BtnStyle::DARK };
    spawn_button(f, fonts, text, Act::Prompt(act), style, Val::Auto, vh(5.6), 2.4, enabled).id()
}

fn build(commands: &mut Commands, fonts: &ArenaFonts, frame: &PromptFrame, ui: &PromptUi) -> Entity {
    let mut root = panel_root(commands);
    let fonts = fonts.clone();
    root.with_children(|r| {
        let mut panel = r.spawn((
            panel_node(),
            BackgroundColor(PANEL),
            BorderColor::all(GOLD_DIM),
            BoxShadow(vec![ShadowStyle { color: Color::srgba(0.0, 0.0, 0.0, 0.8), x_offset: vh(0.0), y_offset: vh(1.0), spread_radius: vh(0.2), blur_radius: vh(5.0) }]),
        ));
        panel.with_children(|p| build_body(p, &fonts, frame, ui));
    });
    root.id()
}

fn card_cell(
    p: &mut ChildSpawnerCommands,
    fonts: &ArenaFonts,
    opt: &ChoiceOption,
    h: f32,
    act: Option<PromptAct>,
) {
    let mut cell = p.spawn((
        Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            padding: UiRect::all(vh(0.5)),
            row_gap: vh(0.4),
            border: UiRect::all(vh(0.3)),
            border_radius: BorderRadius::all(vh(1.0)),
            ..default()
        },
        BackgroundColor(Color::NONE),
        BorderColor::all(Color::NONE),
        PickOpt(opt.i),
    ));
    if let Some(a) = act {
        cell.insert(OnClick(Act::Prompt(a)));
    }
    cell.with_children(|c| {
        if let Some(card) = opt.card.as_ref().filter(|c| !c.hidden && c.state.is_some()) {
            spawn_card_pic(c, fonts, card, ImageVersion::Normal, h).insert(Pickable::IGNORE);
        }
        c.spawn((
            label(fonts, opt.label.clone(), FontKind::Body, 1.7, INK),
            Node { max_width: vh(h * 63.0 / 88.0 + 4.0), ..default() },
            TextLayout { justify: Justify::Center, ..default() },
        ));
    });
}

fn text_opt(
    p: &mut ChildSpawnerCommands,
    fonts: &ArenaFonts,
    opt: &ChoiceOption,
    act: Option<PromptAct>,
    enabled: bool,
) {
    let glyph = match opt.kind.as_str() {
        _ => "",
    };
    let text = format!("{glyph}{}", opt.label);
    let mut e = spawn_button(
        p,
        fonts,
        &text,
        act.map(Act::Prompt).unwrap_or(Act::CloseInspect),
        BtnStyle::DARK,
        Val::Percent(100.0),
        Val::Auto,
        2.2,
        enabled,
    );
    e.insert(PickOpt(opt.i));
    e.entry::<Node>().and_modify(|mut n| {
        n.justify_content = JustifyContent::FlexStart;
        n.padding = UiRect::axes(vh(2.0), vh(1.2));
    });
}

fn card_h(n: usize) -> f32 {
    match n {
        0..=3 => 38.0,
        4..=5 => 32.0,
        6..=8 => 25.0,
        9..=14 => 20.0,
        _ => 16.0,
    }
}

fn build_body(p: &mut ChildSpawnerCommands, fonts: &ArenaFonts, frame: &PromptFrame, ui: &PromptUi) {
    match &frame.body {
        PromptBody::Confirm { card, yes, no, default_yes } => {
            header(p, fonts, frame, None);
            if let Some(c) = card.as_ref().filter(|c| !c.hidden) {
                spawn_card_pic(p, fonts, c, ImageVersion::Normal, 40.0);
            }
            footer(p).with_children(|f| {
                let yes_l = if yes.is_empty() { "Yes" } else { yes };
                let no_l = if no.is_empty() { "No" } else { no };
                big_btn(f, fonts, no_l, PromptAct::Answer(json!(false)), !*default_yes, true);
                big_btn(f, fonts, yes_l, PromptAct::Answer(json!(true)), *default_yes, true);
            });
        }
        PromptBody::Option { options, default: dflt } => {
            header(p, fonts, frame, None);
            p.spawn(ghost(Node { flex_direction: FlexDirection::Column, row_gap: vh(1.0), min_width: vh(46.0), ..default() }))
                .with_children(|l| {
                    for (i, o) in options.iter().enumerate() {
                        let st = if i as i64 == *dflt { BtnStyle::ORANGE } else { BtnStyle::DARK };
                        spawn_button(l, fonts, o, Act::Prompt(PromptAct::Answer(json!(i))), st, Val::Percent(100.0), Val::Auto, 2.4, true)
                            .entry::<Node>()
                            .and_modify(|mut n| n.padding = UiRect::axes(vh(2.0), vh(1.3)));
                    }
                });
        }
        PromptBody::ChooseAbility { host_card, options } => {
            header(p, fonts, frame, Some("Choose what to do".into()));
            p.spawn(ghost(Node { flex_direction: FlexDirection::Row, column_gap: vh(3.0), align_items: AlignItems::Center, ..default() }))
                .with_children(|row| {
                    if let Some(c) = host_card.as_ref().filter(|c| !c.hidden) {
                        spawn_card_pic(row, fonts, c, ImageVersion::Normal, 42.0);
                    }
                    row.spawn(ghost(Node { flex_direction: FlexDirection::Column, row_gap: vh(1.0), width: vh(58.0), ..default() }))
                        .with_children(|l| {
                            for o in options {
                                let can = o.ability.as_ref().map(|a| a.can_play).unwrap_or(true);
                                let mut opt = o.clone();
                                if let Some(a) = o.ability.as_ref().filter(|a| !a.description.is_empty()) {
                                    opt.label = a.description.clone();
                                }
                                let mut e = spawn_button(
                                    l,
                                    fonts,
                                    &opt.label.to_string(),
                                    Act::Prompt(PromptAct::Answer(json!(o.i))),
                                    BtnStyle::DARK,
                                    Val::Percent(100.0),
                                    Val::Auto,
                                    2.0,
                                    can,
                                );
                                e.entry::<Node>().and_modify(|mut n| {
                                    n.justify_content = JustifyContent::FlexStart;
                                    n.padding = UiRect::axes(vh(2.0), vh(1.4));
                                });
                            }
                        });
                });
            footer(p).with_children(|f| {
                big_btn(f, fonts, "Cancel", PromptAct::Answer(Value::Null), false, true);
            });
        }
        PromptBody::Choices { min, max, options, .. } => {
            build_pick(p, fonts, frame, *min, *max, options, None, ui);
        }
        PromptBody::ChooseEntity { min, max, options, reveal } => {
            build_pick(p, fonts, frame, *min, *max, options, reveal.as_ref(), ui);
        }
        PromptBody::Order { top, remain_min, remain_max, options, show_remember, ref_card, .. } => {
            let sub = if *remain_min <= 0 && *remain_max < 0 {
                "Click an item to move it between the lists, then order the right-hand list.".to_string()
            } else {
                format!("Click an item to move it between lists. Keep {remain_min} to {remain_max} on the left.")
            };
            header(p, fonts, frame, Some(sub));
            let by = |i: i64| options.iter().find(|o| o.i == i).cloned();
            p.spawn(ghost(Node { flex_direction: FlexDirection::Row, column_gap: vh(3.0), align_items: AlignItems::FlexStart, ..default() }))
                .with_children(|row| {
                    if let Some(c) = ref_card.as_ref().filter(|c| !c.hidden) {
                        spawn_card_pic(row, fonts, c, ImageVersion::Normal, 34.0);
                    }
                    row.spawn(ghost(Node { flex_direction: FlexDirection::Column, row_gap: vh(0.8), min_width: vh(40.0), ..default() }))
                        .with_children(|col| {
                            col.spawn(label(fonts, "Available", FontKind::Title, 1.8, INK_DIM));
                            for &i in &ui.src {
                                if let Some(o) = by(i) {
                                    text_opt(col, fonts, &o, Some(PromptAct::MoveAcross(i)), true);
                                }
                            }
                        });
                    row.spawn(ghost(Node { flex_direction: FlexDirection::Column, row_gap: vh(0.8), min_width: vh(48.0), ..default() }))
                        .with_children(|col| {
                            let t = if top.is_empty() { "In order" } else { top.as_str() };
                            col.spawn(label(fonts, t, FontKind::Title, 1.8, GOLD));
                            for (k, &i) in ui.dst.iter().enumerate() {
                                let Some(o) = by(i) else { continue };
                                col.spawn(ghost(Node { flex_direction: FlexDirection::Row, column_gap: vh(0.6), align_items: AlignItems::Center, ..default() }))
                                    .with_children(|r2| {
                                        r2.spawn((label(fonts, format!("{}.", k + 1), FontKind::Num, 2.2, GOLD), Node { width: vh(3.0), ..default() }));
                                        r2.spawn(ghost(Node { flex_grow: 1.0, ..default() })).with_children(|c3| {
                                            text_opt(c3, fonts, &o, Some(PromptAct::MoveAcross(i)), true);
                                        });
                                        spawn_chevron_button(r2, Dir::Up, Act::Prompt(PromptAct::Shift { list: 1, idx: k, dir: -1 }), 4.6, k > 0);
                                        spawn_chevron_button(r2, Dir::Down, Act::Prompt(PromptAct::Shift { list: 1, idx: k, dir: 1 }), 4.6, k + 1 < ui.dst.len());
                                    });
                            }
                        });
                });
            footer(p).with_children(|f| {
                if *show_remember {
                    let t = if ui.remember { "Remember this order: ON" } else { "Remember this order: OFF" };
                    big_btn(f, fonts, t, PromptAct::ToggleRemember, false, true);
                }
                let ok = order_valid(ui.src.len(), *remain_min, *remain_max);
                let id = big_btn(f, fonts, "OK", PromptAct::Submit, true, ok);
                f.commands().entity(id).insert(SubmitBtn);
            });
        }
        PromptBody::AssignCombatDamage { attacker, blockers, damage, defender, may_skip, .. } => {
            let aname = attacker.as_ref().map(|a| a.display_name().to_string()).unwrap_or_else(|| "Attacker".into());
            header(p, fonts, frame, Some(format!("{aname} deals {damage} combat damage. Divide it among the blockers.")));
            p.spawn(ghost(Node { flex_direction: FlexDirection::Row, column_gap: vh(2.4), align_items: AlignItems::Center, flex_wrap: FlexWrap::Wrap, justify_content: JustifyContent::Center, ..default() }))
                .with_children(|row| {
                    if let Some(a) = attacker {
                        row.spawn(ghost(Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, row_gap: vh(0.6), ..default() }))
                            .with_children(|c| {
                                spawn_card_pic(c, fonts, a, ImageVersion::Normal, 30.0);
                                c.spawn(label(fonts, format!("{damage} damage"), FontKind::Title, 2.2, RED));
                            });
                    }
                    spawn_chevron(row, Dir::Right, 6.0, GOLD);
                    let hh = card_h(blockers.len() + 1).min(30.0);
                    for b in blockers {
                        stepper_column(row, fonts, Some(b), &b.id.to_string(), hh, attacker.as_ref(), None);
                    }
                    if let Some(d) = defender {
                        let nm = if d.name.is_empty() { "Defender".to_string() } else { d.name.clone() };
                        stepper_column(row, fonts, None, DEFENDER, hh, None, Some(format!("{nm}\n(trample over)")));
                    }
                });
            footer(p).with_children(|f| {
                if *may_skip {
                    big_btn(f, fonts, "Assign Another Attacker First", PromptAct::Answer(Value::Null), false, true);
                }
                big_btn(f, fonts, "Lethal First", PromptAct::AutoFill, false, true);
                let id = big_btn(f, fonts, "Done", PromptAct::Submit, true, false);
                f.commands().entity(id).insert(SubmitBtn);
            });
        }
        PromptBody::AssignAmount { source, amount, at_least_one, label: lbl, options } => {
            let unit = if lbl.is_empty() { String::new() } else { format!(" {lbl}") };
            let sub = format!("Distribute {amount}{unit}{}", if *at_least_one { " (at least 1 each)" } else { "" });
            header(p, fonts, frame, Some(sub));
            p.spawn(ghost(Node { flex_direction: FlexDirection::Row, column_gap: vh(3.0), align_items: AlignItems::Center, ..default() }))
                .with_children(|row| {
                    if let Some(c) = source.as_ref().filter(|c| !c.hidden) {
                        spawn_card_pic(row, fonts, c, ImageVersion::Normal, 32.0);
                    }
                    row.spawn(ghost(Node { flex_direction: FlexDirection::Column, row_gap: vh(0.9), min_width: vh(46.0), ..default() }))
                        .with_children(|col| {
                            for o in options {
                                col.spawn((
                                    Node {
                                        flex_direction: FlexDirection::Row,
                                        align_items: AlignItems::Center,
                                        column_gap: vh(1.2),
                                        padding: UiRect::axes(vh(1.6), vh(0.8)),
                                        border: UiRect::all(vh(0.2)),
                                        border_radius: BorderRadius::all(vh(1.0)),
                                        ..default()
                                    },
                                    BackgroundColor(PANEL_SOFT),
                                    BorderColor::all(GOLD_DIM),
                                    Pickable::IGNORE,
                                ))
                                .with_children(|r2| {
                                    if let Some(c) = o.card.as_ref().filter(|c| !c.hidden && c.state.is_some()) {
                                        spawn_card_pic(r2, fonts, c, ImageVersion::Normal, 9.0);
                                    }
                                    r2.spawn((label(fonts, o.label.clone(), FontKind::Body, 2.2, INK), Node { flex_grow: 1.0, ..default() }));
                                    stepper(r2, fonts, &o.i.to_string());
                                });
                            }
                        });
                });
            footer(p).with_children(|f| {
                let id = big_btn(f, fonts, "OK", PromptAct::Submit, true, false);
                f.commands().entity(id).insert(SubmitBtn);
            });
        }
        PromptBody::ManipulateCards { cards, manipulable, to_top, to_bottom, to_anywhere } => {
            let mut hint = String::from("Top of the library is on the left.");
            if *to_top || *to_bottom || *to_anywhere {
                hint.push_str(" Use the arrows to reorder; Top / Bottom send a card to either end.");
            }
            header(p, fonts, frame, Some(hint));
            let n = ui.ids.len();
            let h = card_h(n).min(34.0);
            p.spawn(scroll_node(Node { flex_direction: FlexDirection::Row, column_gap: vh(1.6), max_width: vw(88.0), padding: UiRect::all(vh(1.0)), ..default() }))
                .insert(ScrollArea)
                .with_children(|row| {
                    for (k, id) in ui.ids.iter().enumerate() {
                        let Some(card) = cards.iter().find(|c| c.id == *id) else { continue };
                        let movable = manipulable.contains(id);
                        row.spawn(ghost(Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, row_gap: vh(0.6), ..default() }))
                            .with_children(|c| {
                                c.spawn(label(fonts, format!("{}", k + 1), FontKind::Num, 2.2, GOLD));
                                spawn_card_pic(c, fonts, card, ImageVersion::Normal, h);
                                if movable {
                                    c.spawn(ghost(Node { flex_direction: FlexDirection::Row, column_gap: vh(0.5), ..default() })).with_children(|b| {
                                        if *to_top || *to_anywhere {
                                            spawn_button(b, fonts, "Top", Act::Prompt(PromptAct::Edge { idx: k, top: true }), BtnStyle::DARK, Val::Auto, vh(4.4), 1.5, k > 0);
                                        }
                                        spawn_chevron_button(b, Dir::Left, Act::Prompt(PromptAct::Shift { list: 2, idx: k, dir: -1 }), 4.4, k > 0);
                                        spawn_chevron_button(b, Dir::Right, Act::Prompt(PromptAct::Shift { list: 2, idx: k, dir: 1 }), 4.4, k + 1 < n);
                                        if *to_bottom || *to_anywhere {
                                            spawn_button(b, fonts, "Bottom", Act::Prompt(PromptAct::Edge { idx: k, top: false }), BtnStyle::DARK, Val::Auto, vh(4.4), 1.5, k + 1 < n);
                                        }
                                    });
                                } else {
                                    c.spawn(label(fonts, "fixed", FontKind::Body, 1.5, INK_DIM));
                                }
                            });
                    }
                });
            footer(p).with_children(|f| {
                big_btn(f, fonts, "OK", PromptAct::Submit, true, true);
            });
        }
        PromptBody::Text { options, numeric, .. } => {
            header(p, fonts, frame, None);
            if !options.is_empty() {
                p.spawn(ghost(Node { flex_direction: FlexDirection::Row, flex_wrap: FlexWrap::Wrap, column_gap: vh(0.8), row_gap: vh(0.8), justify_content: JustifyContent::Center, max_width: vh(90.0), ..default() }))
                    .with_children(|r| {
                        for o in options {
                            spawn_button(r, fonts, o, Act::Prompt(PromptAct::SetText(o.clone())), BtnStyle::DARK, Val::Auto, vh(4.6), 1.9, true);
                        }
                    });
            }
            p.spawn(ghost(Node { flex_direction: FlexDirection::Row, column_gap: vh(1.0), align_items: AlignItems::Center, ..default() }))
                .with_children(|r| {
                    if *numeric {
                        spawn_button(r, fonts, "-", Act::Prompt(PromptAct::Step { key: "text".into(), delta: -1 }), BtnStyle::DARK, Val::Auto, vh(5.6), 2.8, true);
                    }
                    r.spawn((
                        Node { min_width: vh(30.0), height: vh(5.8), justify_content: JustifyContent::Center, align_items: AlignItems::Center, border: UiRect::all(vh(0.2)), border_radius: BorderRadius::all(vh(0.8)), padding: UiRect::horizontal(vh(1.6)), ..default() },
                        BackgroundColor(Color::srgb(0.02, 0.02, 0.03)),
                        BorderColor::all(GOLD),
                        Pickable::IGNORE,
                    ))
                    .with_children(|b| {
                        b.spawn((label(fonts, format!("{}|", ui.text), FontKind::Num, 2.8, INK), TextField));
                    });
                    if *numeric {
                        spawn_button(r, fonts, "+", Act::Prompt(PromptAct::Step { key: "text".into(), delta: 1 }), BtnStyle::DARK, Val::Auto, vh(5.6), 2.8, true);
                    }
                });
            footer(p).with_children(|f| {
                big_btn(f, fonts, "OK", PromptAct::Submit, true, true);
            });
        }
        PromptBody::Unknown => {
            header(p, fonts, frame, Some("The engine asked something this client does not understand yet.".into()));
            footer(p).with_children(|f| {
                big_btn(f, fonts, "Continue", PromptAct::Answer(Value::Null), true, true);
            });
        }
    }
}

fn build_pick(
    p: &mut ChildSpawnerCommands,
    fonts: &ArenaFonts,
    frame: &PromptFrame,
    min: i64,
    max: i64,
    options: &[ChoiceOption],
    reveal: Option<&mtg_protocol::Reveal>,
    _ui: &PromptUi,
) {
    let (info, lo, hi) = pick_bounds(min, max, options.len());
    let sub = if info {
        "Information".to_string()
    } else if lo == hi {
        format!("Choose {lo}")
    } else {
        format!("Choose {lo} to {hi}")
    };
    header(p, fonts, frame, Some(sub));
    if let Some(r) = reveal.filter(|r| !r.cards.is_empty()) {
        p.spawn(label(fonts, r.message.clone().unwrap_or_else(|| "Revealed".into()), FontKind::Title, 1.8, INK_DIM));
        p.spawn(ghost(Node { flex_direction: FlexDirection::Row, column_gap: vh(0.8), flex_wrap: FlexWrap::Wrap, justify_content: JustifyContent::Center, max_width: vw(80.0), ..default() }))
            .with_children(|row| {
                for c in &r.cards {
                    spawn_card_pic(row, fonts, c, ImageVersion::Normal, 14.0);
                }
            });
    }
    let n_cards = options.iter().filter(|o| o.card.as_ref().is_some_and(|c| !c.hidden && c.state.is_some())).count();
    let immediate = hi == 1 && lo <= 1 && !info;
    let act_for = |i: i64| if info { None } else if immediate && lo == 1 { Some(PromptAct::Answer(json!([i]))) } else if immediate { Some(PromptAct::Answer(json!([i]))) } else { Some(PromptAct::Toggle(i)) };
    if n_cards > 0 {
        let h = card_h(n_cards);
        p.spawn(scroll_node(Node {
            flex_direction: FlexDirection::Row,
            flex_wrap: FlexWrap::Wrap,
            justify_content: JustifyContent::Center,
            column_gap: vh(0.8),
            row_gap: vh(0.8),
            max_width: vw(88.0),
            max_height: vh(66.0),
            padding: UiRect::all(vh(0.6)),
            ..default()
        }))
        .insert(ScrollArea)
        .with_children(|row| {
            for o in options {
                if o.card.as_ref().is_some_and(|c| !c.hidden && c.state.is_some()) {
                    card_cell(row, fonts, o, h, act_for(o.i));
                } else {
                    // mixed lists: non-card entries as compact buttons
                    text_opt(row, fonts, o, act_for(o.i), !info);
                }
            }
        });
    } else {
        p.spawn(scroll_node(Node { flex_direction: FlexDirection::Column, row_gap: vh(0.9), min_width: vh(52.0), max_height: vh(60.0), ..default() }))
            .insert(ScrollArea)
            .with_children(|col| {
                for o in options {
                    text_opt(col, fonts, o, act_for(o.i), !info);
                }
            });
    }
    footer(p).with_children(|f| {
        if lo == 0 && !info && max != 1 {
            big_btn(f, fonts, "None", PromptAct::Answer(json!([])), false, true);
        }
        if lo == 0 && !info && max == 1 {
            big_btn(f, fonts, "None", PromptAct::Answer(json!([])), false, true);
        }
        if info {
            big_btn(f, fonts, "OK", PromptAct::Answer(json!([])), true, true);
        } else if !immediate {
            let id = big_btn(f, fonts, "Done", PromptAct::Submit, true, lo == 0);
            f.commands().entity(id).insert(SubmitBtn);
        }
    });
}

fn stepper(p: &mut ChildSpawnerCommands, fonts: &ArenaFonts, key: &str) {
    p.spawn(ghost(Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, column_gap: vh(0.6), ..default() }))
        .with_children(|r| {
            spawn_button(r, fonts, "-", Act::Prompt(PromptAct::Step { key: key.into(), delta: -1 }), BtnStyle::DARK, Val::Auto, vh(5.0), 2.6, true);
            r.spawn((label(fonts, "0", FontKind::Num, 3.2, INK), ValText(key.into()), Node { min_width: vh(4.2), justify_content: JustifyContent::Center, ..default() }, TextLayout { justify: Justify::Center, ..default() }));
            spawn_button(r, fonts, "+", Act::Prompt(PromptAct::Step { key: key.into(), delta: 1 }), BtnStyle::DARK, Val::Auto, vh(5.0), 2.6, true);
        });
}

fn stepper_column(
    p: &mut ChildSpawnerCommands,
    fonts: &ArenaFonts,
    card: Option<&Card>,
    key: &str,
    h: f32,
    attacker: Option<&Card>,
    note: Option<String>,
) {
    p.spawn(ghost(Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, row_gap: vh(0.7), ..default() }))
        .with_children(|c| {
            if let Some(card) = card {
                spawn_card_pic(c, fonts, card, ImageVersion::Normal, h);
                if let Some(a) = attacker {
                    let lethal = lethal_for(a, card, 99);
                    if !card.hidden && card.state.is_some() {
                        c.spawn(label(fonts, format!("lethal: {lethal}"), FontKind::Body, 1.6, INK_DIM));
                    }
                }
            } else {
                c.spawn((
                    Node { width: vh(h * 63.0 / 88.0), height: vh(h), justify_content: JustifyContent::Center, align_items: AlignItems::Center, border: UiRect::all(vh(0.25)), border_radius: BorderRadius::all(vh(1.2)), ..default() },
                    BackgroundColor(Color::srgba(0.1, 0.12, 0.2, 0.5)),
                    BorderColor::all(BLUE_GLOW),
                    Pickable::IGNORE,
                ))
                .with_children(|d| {
                    d.spawn((label(fonts, note.unwrap_or_default(), FontKind::Title, 1.9, INK), TextLayout { justify: Justify::Center, ..default() }));
                });
            }
            stepper(c, fonts, key);
        });
}

// ---- interaction --------------------------------------------------------------------------

fn send_answer(out: &mut MessageWriter<SendCmd>, ui: &mut PromptUi, pending: &mut PendingPrompt, id: i64, v: Value) {
    info!("prompt answer id={id} value={v}");
    out.write(SendCmd(ClientFrame::answer(id, v)));
    ui.last_answered = Some(id);
    pending.0 = None;
}

fn handle_acts(
    mut acts: MessageReader<Acted>,
    mut ui: ResMut<PromptUi>,
    mut pending: ResMut<PendingPrompt>,
    mut out: MessageWriter<SendCmd>,
) {
    for Acted(act) in acts.read() {
        let Act::Prompt(pa) = act else { continue };
        let Some(frame) = pending.0.clone() else { continue };
        let id = frame.id;
        match pa.clone() {
            PromptAct::Answer(v) => send_answer(&mut out, &mut ui, &mut pending, id, v),
            PromptAct::Toggle(i) => {
                if let PromptBody::Choices { max, options, .. } | PromptBody::ChooseEntity { max, options, .. } = &frame.body {
                    let (_, _, hi) = pick_bounds(0, *max, options.len());
                    toggle_pick(&mut ui.sel, i, hi);
                    ui.changed = true;
                }
            }
            PromptAct::Submit => submit(&frame, &mut ui, &mut pending, &mut out),
            PromptAct::Step { key, delta } => match &frame.body {
                PromptBody::AssignCombatDamage { damage, .. } => {
                    step_value(&mut ui.vals, &key, delta, Some(*damage));
                    ui.changed = true;
                }
                PromptBody::AssignAmount { amount, .. } => {
                    step_value(&mut ui.vals, &key, delta, Some(*amount));
                    ui.changed = true;
                }
                PromptBody::Text { .. } => {
                    let cur: i64 = ui.text.trim().parse().unwrap_or(0);
                    ui.text = (cur + delta).to_string();
                    ui.changed = true;
                }
                _ => {}
            },
            PromptAct::AutoFill => {
                if let PromptBody::AssignCombatDamage { attacker: Some(a), blockers, damage, defender, .. } = &frame.body {
                    ui.vals = default_damage(a, blockers, *damage, defender.is_some());
                    ui.changed = true;
                }
            }
            PromptAct::Shift { list, idx, dir } => {
                let ok = match list {
                    1 => shift(&mut ui.dst, idx, dir),
                    _ => shift(&mut ui.ids, idx, dir),
                };
                ui.dirty |= ok;
            }
            PromptAct::Edge { idx, top } => {
                move_edge(&mut ui.ids, idx, top);
                ui.dirty = true;
            }
            PromptAct::MoveAcross(i) => {
                if let Some(p) = ui.src.iter().position(|x| *x == i) {
                    ui.src.remove(p);
                    ui.dst.push(i);
                } else if let Some(p) = ui.dst.iter().position(|x| *x == i) {
                    ui.dst.remove(p);
                    ui.src.push(i);
                }
                ui.dirty = true;
            }
            PromptAct::ToggleRemember => {
                ui.remember = !ui.remember;
                ui.dirty = true;
            }
            PromptAct::SetText(t) => {
                ui.text = t;
                ui.changed = true;
            }
        }
    }
}

fn submit(frame: &PromptFrame, ui: &mut PromptUi, pending: &mut PendingPrompt, out: &mut MessageWriter<SendCmd>) {
    let id = frame.id;
    let v = match &frame.body {
        PromptBody::Choices { min, max, options, .. } | PromptBody::ChooseEntity { min, max, options, .. } => {
            let (_, lo, hi) = pick_bounds(*min, *max, options.len());
            if !pick_valid(&ui.sel, lo, hi) {
                return;
            }
            json!(ui.sel)
        }
        PromptBody::Order { remain_min, remain_max, .. } => {
            if !order_valid(ui.src.len(), *remain_min, *remain_max) {
                return;
            }
            order_answer(&ui.dst, ui.remember)
        }
        PromptBody::AssignCombatDamage { damage, .. } => {
            if total(&ui.vals) != *damage {
                return;
            }
            damage_answer(&ui.vals)
        }
        PromptBody::AssignAmount { amount, at_least_one, .. } => {
            if !amount_valid(&ui.vals, *amount, *at_least_one) {
                return;
            }
            amount_answer(&ui.vals)
        }
        PromptBody::ManipulateCards { .. } => json!(ui.ids),
        PromptBody::Text { .. } => json!(ui.text),
        _ => Value::Null,
    };
    send_answer(out, ui, pending, id, v);
}

fn prompt_keys(
    keys: Res<ButtonInput<KeyCode>>,
    mut kb: MessageReader<KeyboardInput>,
    mut ui: ResMut<PromptUi>,
    mut pending: ResMut<PendingPrompt>,
    mut out: MessageWriter<SendCmd>,
) {
    let Some(frame) = pending.0.clone() else {
        kb.clear();
        return;
    };
    let id = frame.id;
    if let PromptBody::Text { numeric, .. } = &frame.body {
        for ev in kb.read() {
            if !ev.state.is_pressed() {
                continue;
            }
            if let Some(t) = &ev.text {
                for ch in t.chars().filter(|c| !c.is_control()) {
                    if !*numeric || ch.is_ascii_digit() || (ch == '-' && ui.text.is_empty()) {
                        ui.text.push(ch);
                        ui.changed = true;
                    }
                }
            }
            if ev.key_code == KeyCode::Backspace {
                ui.text.pop();
                ui.changed = true;
            }
        }
        if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter) {
            submit(&frame, &mut ui, &mut pending, &mut out);
        }
        return;
    }
    kb.clear();
    let esc = keys.just_pressed(KeyCode::Escape);
    let enter = keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter);
    match &frame.body {
        PromptBody::Confirm { default_yes, .. } => {
            if esc {
                send_answer(&mut out, &mut ui, &mut pending, id, json!(false));
            } else if enter {
                send_answer(&mut out, &mut ui, &mut pending, id, json!(*default_yes));
            }
        }
        PromptBody::ChooseAbility { .. } if esc => send_answer(&mut out, &mut ui, &mut pending, id, Value::Null),
        PromptBody::Option { default, .. } if enter => send_answer(&mut out, &mut ui, &mut pending, id, json!(*default)),
        PromptBody::Unknown if enter || esc => send_answer(&mut out, &mut ui, &mut pending, id, Value::Null),
        PromptBody::Choices { .. } | PromptBody::ChooseEntity { .. } | PromptBody::Order { .. } | PromptBody::AssignAmount { .. } | PromptBody::AssignCombatDamage { .. } | PromptBody::ManipulateCards { .. } if enter => {
            submit(&frame, &mut ui, &mut pending, &mut out)
        }
        _ => {}
    }
}

fn refresh_in_place(
    mut ui: ResMut<PromptUi>,
    pending: Res<PendingPrompt>,
    mut picks: Query<(&PickOpt, &mut BorderColor, Option<&mut BackgroundColor>, Option<&mut BtnStyle>)>,
    mut vals: Query<(&ValText, &mut Text), (Without<TotalText>, Without<TextField>)>,
    mut field: Query<&mut Text, (With<TextField>, Without<ValText>)>,
    submit: Query<(Entity, &Children), With<SubmitBtn>>,
    mut texts: Query<&mut Text, (Without<ValText>, Without<TextField>)>,
    mut commands: Commands,
) {
    if !ui.changed {
        return;
    }
    ui.changed = false;
    let Some(frame) = pending.0.as_ref() else { return };
    for (po, mut bc, bg, style) in &mut picks {
        let on = ui.sel.contains(&po.0);
        *bc = BorderColor::all(if on { GOLD } else if style.is_some() { GOLD_DIM } else { Color::NONE });
        if let Some(mut bg) = bg
            && style.is_none()
        {
            bg.0 = if on { Color::srgba(0.86, 0.69, 0.36, 0.22) } else { Color::NONE };
        }
        if let Some(mut st) = style {
            *st = if on { BtnStyle::GOLD } else { BtnStyle::DARK };
        }
    }
    for (v, mut t) in &mut vals {
        let n = ui.vals.get(&v.0).copied().unwrap_or(0);
        t.0 = n.to_string();
    }
    for mut t in &mut field {
        t.0 = format!("{}|", ui.text);
    }
    // Submit button: validity + label
    let (valid, text): (bool, Option<String>) = match &frame.body {
        PromptBody::Choices { min, max, options, .. } | PromptBody::ChooseEntity { min, max, options, .. } => {
            let (_, lo, hi) = pick_bounds(*min, *max, options.len());
            let t = if hi > 1 { format!("Done ({}/{})", ui.sel.len(), if lo == hi { lo.to_string() } else { format!("{lo}-{hi}") }) } else { "Done".into() };
            (pick_valid(&ui.sel, lo, hi), Some(t))
        }
        PromptBody::AssignCombatDamage { damage, .. } => {
            let tot = total(&ui.vals);
            (tot == *damage, Some(format!("Done ({tot}/{damage})")))
        }
        PromptBody::AssignAmount { amount, at_least_one, .. } => {
            let tot = total(&ui.vals);
            (amount_valid(&ui.vals, *amount, *at_least_one), Some(format!("OK ({tot}/{amount})")))
        }
        PromptBody::Order { remain_min, remain_max, .. } => (order_valid(ui.src.len(), *remain_min, *remain_max), None),
        _ => (true, None),
    };
    for (e, children) in &submit {
        let mut ec = commands.entity(e);
        if valid {
            ec.remove::<BtnOff>();
            ec.insert(BackgroundColor(BtnStyle::ORANGE.base));
        } else {
            ec.insert(BtnOff);
        }
        if let (Some(t), Some(&c0)) = (text.as_ref(), children.first())
            && let Ok(mut tx) = texts.get_mut(c0)
        {
            tx.0 = t.clone();
        }
    }
}
