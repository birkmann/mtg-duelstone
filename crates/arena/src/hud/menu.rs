//! Gear menu (log, stops, auto-yields, settings, concede), the game-log drawer and the concede
//! confirmation. All state is local UI state; the only engine traffic is `Concede` / `ClearYields`.

use bevy::picking::Pickable;
use bevy::prelude::*;
use mtg_protocol::ClientFrame;

use super::kit::*;
use crate::shared::*;

#[derive(Resource, Default)]
pub struct MenuUi {
    pub gear_open: bool,
    pub log_open: bool,
    pub confirm_concede: bool,
}

#[derive(Component)]
pub struct GearMenu;
#[derive(Component)]
pub struct LogDrawer;
#[derive(Component)]
pub struct LogList;
#[derive(Component)]
pub struct ConcedeDialog;

pub fn spawn_menu(mut commands: Commands, fonts: Res<ArenaFonts>, mut ui: ResMut<MenuUi>) {
    *ui = MenuUi::default();
    // gear button (top-right)
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: vw(1.4),
                top: vh(1.4),
                width: vh(5.4),
                height: vh(5.4),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(vh(0.2)),
                border_radius: BorderRadius::MAX,
                ..default()
            },
            BackgroundColor(BtnStyle::DARK.base),
            BorderColor::all(GOLD_DIM),
            BtnStyle::DARK,
            OnClick(Act::ToggleGear),
            GlobalZIndex(12),
            HudEntity,
        ))
        .with_children(|g| {
            // cog: ring + four teeth built from bars
            g.spawn((
                Node { width: vh(2.3), height: vh(2.3), border: UiRect::all(vh(0.55)), border_radius: BorderRadius::MAX, ..default() },
                BorderColor::all(INK),
                Pickable::IGNORE,
            ));
            for deg in [0.0, 45.0, 90.0, 135.0] {
                g.spawn((
                    Node { position_type: PositionType::Absolute, width: vh(0.7), height: vh(3.6), border_radius: BorderRadius::all(vh(0.2)), ..default() },
                    UiTransform::from_rotation(Rot2::degrees(deg)),
                    BackgroundColor(INK),
                    Pickable::IGNORE,
                ));
            }
        });

    // dropdown
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: vw(1.4),
                top: vh(7.6),
                flex_direction: FlexDirection::Column,
                row_gap: vh(0.6),
                padding: UiRect::all(vh(1.0)),
                border: UiRect::all(vh(0.2)),
                border_radius: BorderRadius::all(vh(1.2)),
                width: vh(27.0),
                display: Display::None,
                ..default()
            },
            BackgroundColor(PANEL),
            BorderColor::all(GOLD_DIM),
            GlobalZIndex(13),
            GearMenu,
            HudEntity,
        ))
        .with_children(|m| {
            for (t, a) in [
                ("Game Log", Act::ToggleLog),
                ("Phase Stops", Act::ToggleStops),
                ("Clear Auto-Yields", Act::ClearYields),
                ("Settings", Act::OpenSettings),
                ("Concede", Act::Concede),
            ] {
                let mut b = spawn_button(m, &fonts, t, a, BtnStyle::DARK, Val::Percent(100.0), Val::Auto, 2.2, true);
                b.entry::<Node>().and_modify(|mut n| n.padding = UiRect::axes(vh(1.4), vh(1.0)));
            }
        });

    // log drawer
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: vw(1.0),
                top: vh(11.0),
                bottom: vh(24.0),
                width: vw(25.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(vh(1.2)),
                row_gap: vh(0.8),
                border: UiRect::all(vh(0.2)),
                border_radius: BorderRadius::all(vh(1.2)),
                display: Display::None,
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.025, 0.04, 0.88)),
            BorderColor::all(GOLD_DIM),
            GlobalZIndex(11),
            LogDrawer,
            HudEntity,
        ))
        .with_children(|d| {
            d.spawn(label(&fonts, "GAME LOG", FontKind::Title, 2.0, GOLD));
            d.spawn((
                scroll_node(Node { flex_direction: FlexDirection::Column, row_gap: vh(0.5), flex_grow: 1.0, min_height: vh(5.0), ..default() }),
                ScrollArea,
                LogList,
                Pickable::default(),
            ));
        });
}

pub fn handle_menu_acts(
    mut acts: MessageReader<Acted>,
    mut ui: ResMut<MenuUi>,
    mut out: MessageWriter<SendCmd>,
    mut settings: MessageWriter<OpenSettings>,
    mut commands: Commands,
    fonts: Res<ArenaFonts>,
    dialog: Query<Entity, With<ConcedeDialog>>,
) {
    for Acted(a) in acts.read() {
        match a {
            Act::ToggleGear => ui.gear_open = !ui.gear_open,
            Act::ToggleLog => {
                ui.log_open = !ui.log_open;
                ui.gear_open = false;
            }
            Act::ToggleStops | Act::OpenSettings | Act::ClearYields | Act::ClearYieldsAct => {
                ui.gear_open = false;
                if matches!(a, Act::OpenSettings) {
                    settings.write(OpenSettings);
                }
                if matches!(a, Act::ClearYields | Act::ClearYieldsAct) {
                    out.write(SendCmd(ClientFrame::ClearYields));
                }
            }
            Act::Concede => {
                ui.gear_open = false;
                if dialog.is_empty() {
                    spawn_concede(&mut commands, &fonts);
                    ui.confirm_concede = true;
                }
            }
            Act::ConfirmConcede => {
                out.write(SendCmd(ClientFrame::Concede));
                for e in &dialog {
                    commands.entity(e).despawn();
                }
                ui.confirm_concede = false;
            }
            Act::CloseInspect => {
                // reused as "cancel" for the concede dialog
                for e in &dialog {
                    commands.entity(e).despawn();
                }
                ui.confirm_concede = false;
            }
            _ => {}
        }
    }
}

fn spawn_concede(commands: &mut Commands, fonts: &ArenaFonts) {
    commands
        .spawn((
            fullscreen(JustifyContent::Center, AlignItems::Center),
            BackgroundColor(SHADE),
            GlobalZIndex(70),
            ConcedeDialog,
            HudEntity,
        ))
        .with_children(|r| {
            r.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: vh(2.0),
                    padding: UiRect::axes(vh(4.0), vh(3.0)),
                    border: UiRect::all(vh(0.28)),
                    border_radius: BorderRadius::all(vh(1.6)),
                    ..default()
                },
                BackgroundColor(PANEL),
                BorderColor::all(GOLD_DIM),
            ))
            .with_children(|p| {
                p.spawn(label(fonts, "CONCEDE THE GAME?", FontKind::Title, 3.2, GOLD));
                p.spawn(label(fonts, "You will lose this match.", FontKind::Body, 2.1, INK));
                p.spawn(ghost(Node { flex_direction: FlexDirection::Row, column_gap: vh(2.0), ..default() })).with_children(|f| {
                    spawn_button(f, fonts, "Keep Playing", Act::CloseInspect, BtnStyle::DARK, Val::Auto, vh(5.6), 2.4, true);
                    spawn_button(f, fonts, "Concede", Act::ConfirmConcede, BtnStyle::ORANGE, Val::Auto, vh(5.6), 2.4, true);
                });
            });
        });
}

pub fn update_menu(
    ui: Res<MenuUi>,
    log: Res<GameLog>,
    fonts: Res<ArenaFonts>,
    mut commands: Commands,
    mut gear: Query<&mut Node, (With<GearMenu>, Without<LogDrawer>)>,
    mut drawer: Query<&mut Node, (With<LogDrawer>, Without<GearMenu>)>,
    list: Query<(Entity, Option<&Children>), With<LogList>>,
    mut sp: Query<(&ComputedNode, &mut ScrollPosition), With<LogList>>,
    mut shown: Local<usize>,
) {
    for mut n in &mut gear {
        let d = if ui.gear_open { Display::Flex } else { Display::None };
        if n.display != d {
            n.display = d;
        }
    }
    for mut n in &mut drawer {
        let d = if ui.log_open { Display::Flex } else { Display::None };
        if n.display != d {
            n.display = d;
        }
    }
    let Ok((e, children)) = list.single() else { return };
    if log.0.len() < *shown || children.map(|c| c.len()).unwrap_or(0) > log.0.len() {
        commands.entity(e).despawn_children();
        *shown = 0;
    }
    if log.0.len() > *shown {
        let fonts = fonts.clone();
        let entries: Vec<_> = log.0[*shown..].to_vec();
        commands.entity(e).with_children(|l| {
            for en in entries {
                let col = match en.kind.as_str() {
                    k if k.contains("damage") => Color::srgb(0.95, 0.55, 0.5),
                    k if k.contains("turn") => GOLD,
                    _ => INK,
                };
                l.spawn((label(&fonts, en.text.clone(), FontKind::Body, 1.55, col), Node { max_width: Val::Percent(100.0), ..default() }));
            }
        });
        *shown = log.0.len();
        // follow the tail
        for (cn, mut s) in &mut sp {
            s.y = (cn.content_size().y + 4000.0) * cn.inverse_scale_factor();
        }
    }
}
