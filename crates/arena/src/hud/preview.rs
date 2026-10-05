//! Large hover preview on the right screen edge and the
//! right-click inspector. Shows the Large scryfall image; rules text is drawn as a fallback until
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
            top: Val::Percent(50.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: vh(1.0),
            ..default()
        },
        UiTransform::from_translation(Val2::percent(0.0, -50.0)),
        GlobalZIndex(20),
        PreviewRoot { key: None },
        HudEntity,
        Pickable::IGNORE,
        Visibility::Hidden,
    ));
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
    fonts: Res<ArenaFonts>,
    mut roots: Query<(Entity, &mut PreviewRoot, &mut Node, &mut Visibility, &mut UiTransform)>,
) {
    let Ok((e, mut root, mut node, mut vis, mut tf)) = roots.single_mut() else { return };
    let inspecting = inspect.0.is_some();
    let id = inspect.0.or(hover.0);
    let card = id.and_then(|id| view.card(id).or_else(|| cache.0.get(&id))).filter(|c| !c.hidden && c.state.is_some());
    let want_key = card.map(|c| (c.id, inspecting));
    let hide = card.is_none() || prompt.0.is_some();
    let want_vis = if hide { Visibility::Hidden } else { Visibility::Inherited };
    if *vis != want_vis {
        *vis = want_vis;
    }
    if root.key == want_key {
        return;
    }
    root.key = want_key;
    commands.entity(e).despawn_children();
    let Some(card) = card else { return };
    // layout: inspector is centred and bigger, hover sits on the right edge
    if inspecting {
        node.right = Val::Auto;
        node.left = Val::Percent(50.0);
        *tf = UiTransform::from_translation(Val2::percent(-50.0, -50.0));
    } else {
        node.left = Val::Auto;
        node.right = vw(1.2);
        *tf = UiTransform::from_translation(Val2::percent(0.0, -50.0));
    }
    let main_h = if inspecting { 84.0 } else { 66.0 };
    let alt = card.alt_state.as_ref().filter(|a| !a.name.is_empty() && Some(&a.name) != card.state.as_ref().map(|s| &s.name)).cloned();
    let fonts = fonts.clone();
    let card = card.clone();
    commands.entity(e).with_children(|r| {
        let pic = spawn_card_pic(r, &fonts, &card, ImageVersion::Large, main_h).id();
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
