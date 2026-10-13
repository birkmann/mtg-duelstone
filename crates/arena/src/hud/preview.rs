//! Bounded, click-through card preview on the right screen edge. The right-click inspector
//! uses the same safe column with an explicit Close button. Shows the Large scryfall image; rules text is drawn as a fallback until
//! the image arrives; double-faced / split cards show the other face too.

use bevy::picking::Pickable;
use bevy::prelude::*;
use mtg_client::ImageVersion;
use mtg_protocol::{Card, CardId};

use super::kit::*;
use crate::interact::InspectCard;
use crate::shared::*;

#[derive(Component)]
pub struct PreviewRoot {
    pub key: Option<(CardId, bool)>,
    pub viewport: Vec2,
}
#[derive(Component)]
pub struct FallbackText {
    pub pic: Entity,
}

pub fn spawn_preview(mut commands: Commands) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            right: vw(1.2),
            top: vh(12.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: vh(1.0),
            ..default()
        },
        UiTransform::IDENTITY,
        GlobalZIndex(20),
        PreviewRoot { key: None, viewport: Vec2::ZERO },
        Name::new("UI/CardPreview"),
        HudEntity,
        Pickable::IGNORE,
        Visibility::Hidden,
    ));
}

fn preview_height(aspect: f32, inspecting: bool, alternate: bool, landscape: bool) -> f32 {
    let width_limit = 22.0 * aspect / if landscape { 1.0 } else { 63.0 / 88.0 };
    let height_limit = if alternate { 52.0 / 1.42 } else if inspecting { 52.0 } else { 44.0 };
    width_limit.min(height_limit)
}

fn oracle_text(c: &Card) -> String {
    let st = c.state.as_ref();
    let mut s = String::new();
    if let Some(st) = st {
        if let Some(m) = &st.mana_cost {
            s.push_str(m);
            s.push('\n');
        }
        if let Some(t) = &st.type_line {
            s.push_str(t);
            s.push('\n');
        }
        if let Some(t) = st.oracle_text.as_ref().or(st.rules_text.as_ref()) {
            s.push_str(t);
        }
        if let (Some(p), Some(t)) = (st.power, st.toughness) {
            s.push_str(&format!("\n{p}/{t}"));
        }
    }
    s
}

pub fn update_preview(
    mut commands: Commands,
    hover: Res<HoverPreview>,
    inspect: Res<InspectCard>,
    view: Res<GameView>,
    cache: Res<CardCache>,
    prompt: Res<PendingPrompt>,
    input: Res<CurrentInput>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    fonts: Res<ArenaFonts>,
    mut roots: Query<(Entity, &mut PreviewRoot, &mut Node, &mut Visibility, &mut UiTransform)>,
) {
    let Ok((e, mut root, mut node, mut vis, mut tf)) = roots.single_mut() else { return };
    let inspecting = inspect.0.is_some();
    let id = inspect.0.or(hover.0);
    let card = id.and_then(|id| view.card(id).or_else(|| cache.0.get(&id))).filter(|c| !c.hidden && c.state.is_some());
    let want_key = card.map(|c| (c.id, inspecting));
    let choosing = input.frame.as_ref().is_some_and(|f| f.select_max > 0 || !f.tray_cards.is_empty());
    let hide = card.is_none() || prompt.0.is_some() || crate::prompts::is_mulligan_input(&input) || choosing;
    let viewport = windows.single().map(|w| w.size()).unwrap_or(Vec2::new(1600.0, 900.0));
    let want_vis = if hide { Visibility::Hidden } else { Visibility::Inherited };
    if *vis != want_vis {
        *vis = want_vis;
    }
    if root.key == want_key && root.viewport == viewport {
        return;
    }
    root.key = want_key;
    root.viewport = viewport;
    commands.entity(e).despawn_children();
    let Some(card) = card else { return };
    // Keep the board centre, hand and bottom-right actions clear. The panel is click-through
    // except for its explicit Close button; inspecting never disables normal card actions.
    node.left = Val::Auto;
    node.right = vw(1.2);
    *tf = UiTransform::IDENTITY;
    let alt = card.alt_state.as_ref().filter(|a| !a.name.is_empty() && Some(&a.name) != card.state.as_ref().map(|s| &s.name)).cloned();
    let landscape = crate::layout::is_landscape(card);
    let main_h = preview_height(viewport.x / viewport.y.max(1.0), inspecting, alt.is_some(), landscape);
    let fonts = fonts.clone();
    let card = card.clone();
    let landscape = crate::layout::is_landscape(&card);
    commands.entity(e).with_children(|r| {
        if inspecting {
            spawn_button(r, &fonts, "Close  ·  Esc", Act::CloseInspect, BtnStyle::DARK, vh(main_h * 63.0 / 88.0), vh(3.6), 1.5, true);
        }
        let pic = if landscape {
            // A plane's scan is portrait with the card on its side: give it a landscape box and turn
            // the (overflowing, centred) portrait picture a quarter clockwise inside it.
            let mut pic = None;
            r.spawn((
                Node { width: vh(main_h), height: vh(main_h * 63.0 / 88.0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
                Pickable::IGNORE,
            ))
            .with_children(|b| {
                let id = spawn_card_pic(b, &fonts, &card, ImageVersion::Large, main_h).id();
                b.commands().entity(id).insert(UiTransform { rotation: Rot2::FRAC_PI_2, ..default() });
                pic = Some(id);
            });
            pic.expect("spawned above")
        } else {
            spawn_card_pic(r, &fonts, &card, ImageVersion::Large, main_h).id()
        };
        r.commands().entity(pic).insert(BorderColor::all(Color::srgba(0.0, 0.0, 0.0, 0.0)));
        let fb = oracle_text(&card);
        let fb_id = r
            .commands()
            .spawn((
                Node { position_type: PositionType::Absolute, left: vh(1.4), right: vh(1.4), bottom: vh(2.0), top: vh(8.0), ..default() },
                FallbackText { pic },
                Pickable::IGNORE,
            ))
            .with_children(|t| {
                t.spawn(label(&fonts, fb, FontKind::Body, 1.6, INK));
            })
            .id();
        r.commands().entity(pic).add_child(fb_id);
        if let Some(a) = alt {
            let mut alt_card = card.clone();
            alt_card.state = Some(a);
            let id = spawn_card_pic(r, &fonts, &alt_card, ImageVersion::Large, main_h * 0.42).id();
            r.commands().entity(id).entry::<CardPic>().and_modify(|mut p| p.back = true);
        }
    });
    let _ = &mut node;
}

/// Hide the text fallback once the real image is visible.
pub fn update_fallback(
    pics: Query<&Children, With<CardPic>>,
    mut fb: Query<(&FallbackText, &mut Node)>,
    img: Query<&ImageNode>,
) {
    for (f, mut n) in &mut fb {
        let Ok(children) = pics.get(f.pic) else { continue };
        let loaded = children.get(1).and_then(|c| img.get(*c).ok()).is_some_and(|i| i.color != Color::NONE);
        let d = if loaded { Display::None } else { Display::Flex };
        if n.display != d {
            n.display = d;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preview_fits_side_column_and_stays_above_actions() {
        for aspect in [0.5625, 1.0, 1.6, 16.0 / 9.0, 2.4] {
            for inspecting in [false, true] {
                for alternate in [false, true] {
                    for landscape in [false, true] {
                        let h = preview_height(aspect, inspecting, alternate, landscape);
                        let width = h * if landscape { 1.0 } else { 63.0 / 88.0 };
                        assert!(width <= 22.0 * aspect + 0.01);
                        let height = h * if alternate { 1.42 } else { 1.0 };
                        assert!(12.0 + height + 6.0 <= 72.0);
                    }
                }
            }
        }
    }
}
