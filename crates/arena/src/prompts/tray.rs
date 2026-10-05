//! Non-modal overlays driven by the engine *input* rather than a prompt frame:
//! the `trayCards` pick tray (library digs, graveyard/exile picks) and the mulligan screen.

use bevy::picking::Pickable;
use bevy::prelude::*;
use mtg_client::ImageVersion;
use mtg_protocol::{Card, ClientFrame};

use crate::hud::kit::*;
use crate::shared::{ArenaFonts, CurrentInput, GameView, SendCmd};

pub struct TrayPlugin;

impl Plugin for TrayPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (sync_tray, tray_clicks, refresh_tray, sync_mulligan, mulligan_visuals).chain());
    }
}

#[derive(Component)]
struct TrayRoot {
    key: (i64, usize),
}
/// One tray tile: every card id it stands for (identical cards are grouped).
#[derive(Component)]
struct TrayCell(Vec<i64>);
#[derive(Component)]
struct MulliganRoot {
    key: Vec<i64>,
}
#[derive(Component)]
struct MulliganBtn(bool);

/// Is this input the opening-hand keep/mulligan decision?
pub fn is_mulligan(input: &CurrentInput) -> bool {
    input.frame.as_ref().is_some_and(|f| f.ok.label.eq_ignore_ascii_case("keep"))
}

fn where_label(cards: &[Card], view: &GameView) -> String {
    let mut labels: Vec<String> = Vec::new();
    for c in cards {
        let who = match c.controller.or(c.owner) {
            Some(p) if view.is_me(p) => "your".to_string(),
            Some(p) => view.game.player(p).map(|pl| format!("{}'s", pl.name)).unwrap_or_else(|| "opponent's".into()),
            None => String::new(),
        };
        let l = match c.zone.as_deref() {
            Some(z) => format!("{}{}", if who.is_empty() { String::new() } else { format!("{who} ") }, z.to_lowercase()),
            None => "outside the game".into(),
        };
        if !labels.contains(&l) {
            labels.push(l);
        }
    }
    labels.join(" / ")
}

/// Identical cards (same name, printing, zone and controller) collapse into one tile with an "×N"
/// badge — a basic-land search otherwise shows fifteen identical Forests.
fn group_cards(cards: &[Card]) -> Vec<(&Card, Vec<i64>)> {
    let same = |a: &Card, b: &Card| {
        a.display_name() == b.display_name()
            && a.state.as_ref().and_then(|s| s.set_code.as_ref()) == b.state.as_ref().and_then(|s| s.set_code.as_ref())
            && a.hidden == b.hidden
            && a.zone == b.zone
            && a.controller == b.controller
            && a.counters == b.counters
    };
    let mut groups: Vec<(&Card, Vec<i64>)> = Vec::new();
    for c in cards {
        match groups.iter_mut().find(|(rep, _)| same(rep, c)) {
            Some((_, ids)) => ids.push(c.id),
            None => groups.push((c, vec![c.id])),
        }
    }
    groups
}

/// Which id a click on a grouped tile should send: the first not-yet-picked copy, or (when every
/// copy is picked) the last one, so clicking again unpicks.
fn next_click(ids: &[i64], highlighted: &[i64]) -> i64 {
    ids.iter().copied().find(|id| !highlighted.contains(id)).unwrap_or_else(|| *ids.last().unwrap_or(&0))
}

fn sync_tray(
    mut commands: Commands,
    input: Res<CurrentInput>,
    view: Res<GameView>,
    fonts: Res<ArenaFonts>,
    existing: Query<(Entity, &TrayRoot)>,
) {
    let want = input.frame.as_ref().filter(|f| !f.tray_cards.is_empty()).map(|f| (f.seq, f.tray_cards.len()));
    let have = existing.iter().next().map(|(_, r)| r.key);
    if want == have {
        return;
    }
    for (e, _) in &existing {
        commands.entity(e).despawn();
    }
    let (Some(key), Some(f)) = (want, input.frame.as_ref()) else { return };
    let cards = &f.tray_cards;
    let title = format!("Choose from {}", where_label(cards, &view));
    let groups = group_cards(cards);
    let h = if groups.len() > 12 { 17.0 } else { 24.0 };
    let fonts = fonts.clone();
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: vw(12.0),
                right: vw(22.0),
                bottom: vh(13.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                padding: UiRect::axes(vh(2.0), vh(1.4)),
                row_gap: vh(0.8),
                border: UiRect::all(vh(0.22)),
                border_radius: BorderRadius::all(vh(1.4)),
                max_height: vh(60.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.03, 0.035, 0.06, 0.93)),
            BorderColor::all(GOLD_DIM),
            GlobalZIndex(30),
            TrayRoot { key },
            HudEntity,
        ))
        .with_children(|p| {
            p.spawn(label(&fonts, title, FontKind::Title, 2.2, GOLD));
            p.spawn(scroll_node(Node { flex_direction: FlexDirection::Row, flex_wrap: FlexWrap::Wrap, justify_content: JustifyContent::Center, column_gap: vh(0.8), row_gap: vh(0.8), max_height: vh(44.0), width: Val::Percent(100.0), ..default() }))
                .insert(ScrollArea)
                .with_children(|row| {
                    for (c, ids) in &groups {
                        let count = ids.len();
                        row.spawn((
                            Node {
                                flex_direction: FlexDirection::Column,
                                align_items: AlignItems::Center,
                                padding: UiRect::all(vh(0.4)),
                                border: UiRect::all(vh(0.3)),
                                border_radius: BorderRadius::all(vh(0.9)),
                                ..default()
                            },
                            BackgroundColor(Color::NONE),
                            BorderColor::all(Color::NONE),
                            OnClick(Act::ClickCard(ids[0])),
                            TrayCell(ids.clone()),
                        ))
                        .with_children(|cell| {
                            spawn_card_pic(cell, &fonts, c, ImageVersion::Normal, h);
                            if count > 1 {
                                cell.spawn((
                                    Node {
                                        position_type: PositionType::Absolute,
                                        top: vh(0.2),
                                        right: vh(0.2),
                                        padding: UiRect::axes(vh(0.7), vh(0.15)),
                                        border: UiRect::all(vh(0.18)),
                                        border_radius: BorderRadius::all(vh(1.0)),
                                        ..default()
                                    },
                                    BackgroundColor(Color::srgba(0.05, 0.05, 0.08, 0.92)),
                                    BorderColor::all(GOLD_DIM),
                                    Pickable::IGNORE,
                                ))
                                .with_children(|b| {
                                    b.spawn(label(&fonts, format!("x{count}"), FontKind::Title, 1.9, GOLD));
                                });
                            }
                        });
                    }
                });
        });
}

fn refresh_tray(input: Res<CurrentInput>, mut cells: Query<(&TrayCell, &mut OnClick, &mut BorderColor, &mut BackgroundColor)>) {
    let Some(f) = input.frame.as_ref() else { return };
    for (c, mut click, mut bc, mut bg) in &mut cells {
        let hi = c.0.iter().any(|id| f.highlighted.contains(id));
        let sel = c.0.iter().any(|id| f.selectable.contains(id));
        let next = Act::ClickCard(next_click(&c.0, &f.highlighted));
        if !matches!((click.0, next), (Act::ClickCard(a), Act::ClickCard(b)) if a == b) {
            click.0 = next;
        }
        *bc = BorderColor::all(if hi { GOLD } else if sel { BLUE_GLOW } else { Color::NONE });
        bg.0 = if hi { Color::srgba(0.86, 0.69, 0.36, 0.25) } else { Color::NONE };
    }
}

fn tray_clicks(mut acts: MessageReader<Acted>, mut out: MessageWriter<SendCmd>, mut input: ResMut<CurrentInput>) {
    for Acted(a) in acts.read() {
        if let Act::ClickCard(id) = a {
            out.write(SendCmd(ClientFrame::click_card(*id)));
            input.awaiting = true;
        }
    }
}

// ---- mulligan -----------------------------------------------------------------------------

fn sync_mulligan(
    mut commands: Commands,
    input: Res<CurrentInput>,
    view: Res<GameView>,
    fonts: Res<ArenaFonts>,
    existing: Query<(Entity, &MulliganRoot)>,
) {
    let hand: Vec<Card> = if is_mulligan(&input) { view.me().map(|p| p.zones.hand.clone()).unwrap_or_default() } else { vec![] };
    let want: Vec<i64> = hand.iter().map(|c| c.id).collect();
    let have = existing.iter().next().map(|(_, r)| r.key.clone());
    if want.is_empty() {
        for (e, _) in &existing {
            commands.entity(e).despawn();
        }
        return;
    }
    if have.as_ref() == Some(&want) {
        return;
    }
    for (e, _) in &existing {
        commands.entity(e).despawn();
    }
    let fonts = fonts.clone();
    let h = if hand.len() > 6 { 27.0 } else { 34.0 };
    let msg = input.frame.as_ref().map(|f| f.message.trim().to_string()).unwrap_or_default();
    commands
        .spawn((
            fullscreen(JustifyContent::Center, AlignItems::Center),
            BackgroundColor(Color::srgba(0.0, 0.0, 0.02, 0.9)),
            GlobalZIndex(40),
            MulliganRoot { key: want },
            HudEntity,
            Pickable { should_block_lower: true, is_hoverable: true },
        ))
        .with_children(|r| {
            r.spawn(ghost(Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, row_gap: vh(2.2), ..default() }))
                .with_children(|c| {
                    c.spawn(label(&fonts, "YOUR OPENING HAND", FontKind::Title, 4.6, GOLD));
                    c.spawn(deco(Node { width: vh(44.0), height: vh(0.25), ..default() }, GOLD_DIM));
                    if !msg.is_empty() {
                        let text = msg.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("  -  ");
                        c.spawn(label(&fonts, text, FontKind::Body, 2.1, INK_DIM));
                    }
                    c.spawn(ghost(Node { flex_direction: FlexDirection::Row, column_gap: vh(1.2), justify_content: JustifyContent::Center, flex_wrap: FlexWrap::Wrap, max_width: vw(94.0), ..default() }))
                        .with_children(|row| {
                            for card in &hand {
                                spawn_card_pic(row, &fonts, card, ImageVersion::Normal, h);
                            }
                        });
                    c.spawn(ghost(Node { flex_direction: FlexDirection::Row, column_gap: vh(3.0), margin: UiRect::top(vh(1.5)), ..default() }))
                        .with_children(|b| {
                            let mut m = spawn_button(b, &fonts, "Mulligan", Act::Secondary, BtnStyle::DARK, vh(26.0), vh(7.0), 3.0, true);
                            m.insert(MulliganBtn(false));
                            let mut k = spawn_button(b, &fonts, "Keep Hand", Act::Primary, BtnStyle::ORANGE, vh(26.0), vh(7.0), 3.0, true);
                            k.insert(MulliganBtn(true));
                        });
                });
        });
}

fn mulligan_visuals(
    input: Res<CurrentInput>,
    q: Query<(Entity, &MulliganBtn, Has<BtnOff>)>,
    mut commands: Commands,
) {
    let Some(f) = input.frame.as_ref() else { return };
    for (e, b, off) in &q {
        let enabled = if b.0 { f.ok.enabled } else { f.cancel.enabled } && !input.awaiting;
        if enabled && off {
            commands.entity(e).remove::<BtnOff>();
        } else if !enabled && !off {
            commands.entity(e).insert(BtnOff);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(id: i64, name: &str) -> Card {
        Card { id, name: name.into(), zone: Some("Library".into()), ..default() }
    }

    #[test]
    fn identical_cards_group_in_order() {
        let cards = vec![card(1, "Forest"), card(2, "Mountain"), card(3, "Forest"), card(4, "Forest")];
        let g = group_cards(&cards);
        assert_eq!(g.len(), 2);
        assert_eq!(g[0].1, vec![1, 3, 4]);
        assert_eq!(g[1].1, vec![2]);
    }

    #[test]
    fn grouped_click_picks_next_unpicked_then_unpicks() {
        assert_eq!(next_click(&[1, 3, 4], &[]), 1);
        assert_eq!(next_click(&[1, 3, 4], &[1]), 3);
        assert_eq!(next_click(&[1, 3, 4], &[1, 3, 4]), 4);
    }
}
