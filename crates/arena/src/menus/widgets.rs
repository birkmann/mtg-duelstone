//! Reusable menu widgets: styled text fields (bevy's `EditableText`), mana-cost pips and settings sliders.

use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::text::{EditableText, FontSize, FontSource};
use bevy::ui::{BackgroundGradient, ColorStop, LinearGradient, RelativeCursorPosition};

use mtg_client::deckstats::mana_symbols;

use crate::shared::{ArenaFonts, Settings};
use crate::style::*;

/// Identifies which setting/field a text input edits.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    EngineUrl,
    PlayerName,
    ImportText,
    ImportName,
    /// Design studio: name of the design being edited, and the artwork picker's card search.
    DesignName,
    ArtSearch,
    /// Deck builder: deck name, card-name search, rules-text search.
    BuilderName,
    BuilderSearch,
    BuilderText,
    /// Multiplayer: pasted invite link or table code.
    Invite,
    /// Event setup: filter the cube / set list.
    SourceFilter,
    /// Cube designer: card search, cube name, pasted list, the views' filter, the cube library filter.
    CubeSearch,
    CubeName,
    CubeImportText,
    CubeFilter,
    CubeLibraryFilter,
    /// Avatar studio: card search.
    AvatarSearch,
    /// Deck select: filter the deck grid by name / description.
    DeckFilter,
}

pub struct WidgetsPlugin;

impl Plugin for WidgetsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (field_focus_style, commit_fields, slider_input, slider_visual, wheel_scroll));
    }
}

/// A single- or multi-line text input in the game's style. `lines` = visible lines.
pub fn text_field(fonts: &ArenaFonts, field: Field, initial: &str, width: Val, lines: f32, multiline: bool) -> impl Bundle {
    let mut et = EditableText::new(initial);
    et.visible_lines = Some(lines);
    et.allow_newlines = multiline;
    (
        field,
        Node {
            width,
            padding: UiRect::axes(Val::Px(12.0), Val::Px(8.0)),
            border: UiRect::all(Val::Px(2.0)),
            border_radius: BorderRadius::all(Val::Px(8.0)),
            overflow: Overflow::clip(),
            ..default()
        },
        BackgroundColor(Color::srgba(0.03, 0.02, 0.015, 0.9)),
        BorderColor::all(GOLD_DIM),
        et,
        TextFont { font: FontSource::Handle(fonts.body.clone()), font_size: FontSize::Px(17.0), ..default() },
        TextColor(TEXT),
    )
}

fn field_focus_style(focus: Res<InputFocus>, mut q: Query<(Entity, &mut BorderColor), With<EditableText>>) {
    for (e, mut b) in &mut q {
        let want = if focus.get() == Some(e) { GOLD_BRIGHT } else { GOLD_DIM };
        if b.top != want {
            *b = BorderColor::all(want);
        }
    }
}

fn commit_fields(mut settings: ResMut<Settings>, q: Query<(&Field, &EditableText), Changed<EditableText>>) {
    for (f, t) in &q {
        let v = t.value().to_string();
        match f {
            Field::EngineUrl if settings.engine_url != v => settings.engine_url = v,
            Field::PlayerName if settings.player_name != v => settings.player_name = v,
            _ => {}
        }
    }
}

/// Mana cost as small pips: icon for coloured/colourless symbols, a numbered disc otherwise.
pub fn cost_pips(p: &mut ChildSpawnerCommands, fonts: &ArenaFonts, assets: &AssetServer, cost: &str, size: f32) {
    p.spawn((Node { column_gap: Val::Px(1.5), align_items: AlignItems::Center, flex_shrink: 0.0, ..default() }, Pickable::IGNORE)).with_children(|r| {
        for sym in mana_symbols(cost) {
            let one = sym.len() == 1 && "WUBRGC".contains(sym);
            if one {
                r.spawn((mana_pip(assets, sym.chars().next().unwrap(), size), Pickable::IGNORE));
            } else {
                let first = sym.split('/').find_map(|s| s.chars().next().filter(|c| "WUBRG".contains(*c)));
                let bg = first.map(mana_color).unwrap_or(Color::srgb(0.72, 0.70, 0.66));
                r.spawn((
                    Node { width: Val::Px(size), height: Val::Px(size), border_radius: BorderRadius::MAX, justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
                    BackgroundColor(bg),
                    Pickable::IGNORE,
                ))
                .with_child((ui_text(&fonts.numbers, sym.replace('/', ""), size * 0.66, Color::srgb(0.08, 0.06, 0.05)), Pickable::IGNORE));
            }
        }
    });
}

// -------------------------------------------------------------------------------------------
// Sliders
// -------------------------------------------------------------------------------------------

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SliderKey {
    Anim,
    Master,
    Sfx,
    Music,
}

impl SliderKey {
    fn range(self) -> (f32, f32) {
        match self {
            SliderKey::Anim => (0.5, 2.5),
            _ => (0.0, 1.0),
        }
    }
    fn get(self, s: &Settings) -> f32 {
        match self {
            SliderKey::Anim => s.anim_speed,
            SliderKey::Master => s.master_volume,
            SliderKey::Sfx => s.sfx_volume,
            SliderKey::Music => s.music_volume,
        }
    }
    fn set(self, s: &mut Settings, v: f32) {
        match self {
            SliderKey::Anim => s.anim_speed = v,
            SliderKey::Master => s.master_volume = v,
            SliderKey::Sfx => s.sfx_volume = v,
            SliderKey::Music => s.music_volume = v,
        }
    }
    fn fmt(self, v: f32) -> String {
        match self {
            SliderKey::Anim => format!("{v:.1}x"),
            _ => format!("{:.0}%", v * 100.0),
        }
    }
}

#[derive(Component)]
struct SliderFill(SliderKey);
#[derive(Component)]
struct SliderValueText(SliderKey);

/// Spawns `label ──[====----]── value` as a row under `parent`.
pub fn spawn_slider(parent: &mut ChildSpawnerCommands, fonts: &ArenaFonts, key: SliderKey, label: &str) {
    parent
        .spawn(Node { width: Val::Percent(100.0), align_items: AlignItems::Center, column_gap: Val::Px(16.0), margin: UiRect::vertical(Val::Px(6.0)), ..default() })
        .with_children(|r| {
            r.spawn((
                Node { width: Val::Px(170.0), ..default() },
                Text::new(label),
                text_font(&fonts.body, 18.0),
                TextColor(TEXT),
            ));
            r.spawn((
                Button,
                key,
                RelativeCursorPosition::default(),
                Node {
                    flex_grow: 1.0,
                    height: Val::Px(14.0),
                    border: UiRect::all(Val::Px(1.5)),
                    border_radius: BorderRadius::all(Val::Px(99.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.02, 0.012, 0.01, 0.9)),
                BorderColor::all(GOLD_DIM),
            ))
            .with_children(|t| {
                t.spawn((
                    SliderFill(key),
                    Node { width: Val::Percent(50.0), height: Val::Percent(100.0), border_radius: BorderRadius::all(Val::Px(99.0)), ..default() },
                    BackgroundGradient::from(LinearGradient::to_right(vec![
                        ColorStop::new(ORANGE_LO, Val::Percent(0.0)),
                        ColorStop::new(ORANGE_HI, Val::Percent(100.0)),
                    ])),
                ));
            });
            r.spawn((
                SliderValueText(key),
                Node { width: Val::Px(64.0), ..default() },
                Text::new(""),
                text_font(&fonts.numbers, 20.0),
                TextColor(GOLD_BRIGHT),
            ));
        });
}

fn slider_input(mut settings: ResMut<Settings>, q: Query<(&Interaction, &RelativeCursorPosition, &SliderKey)>) {
    for (i, rel, key) in &q {
        if *i != Interaction::Pressed {
            continue;
        }
        let Some(n) = rel.normalized else { continue };
        let (lo, hi) = key.range();
        let mut v = lo + (n.x + 0.5).clamp(0.0, 1.0) * (hi - lo);
        // snap to a tidy step so the saved JSON stays readable
        v = (v * 20.0).round() / 20.0;
        if (key.get(&settings) - v).abs() > 1e-4 {
            key.set(&mut settings, v);
        }
    }
}

fn slider_visual(
    settings: Res<Settings>,
    mut fills: Query<(&SliderFill, &mut Node)>,
    mut texts: Query<(&SliderValueText, &mut Text)>,
) {
    for (f, mut n) in &mut fills {
        let (lo, hi) = f.0.range();
        let p = ((f.0.get(&settings) - lo) / (hi - lo)).clamp(0.0, 1.0) * 100.0;
        if n.width != Val::Percent(p) {
            n.width = Val::Percent(p);
        }
    }
    for (t, mut text) in &mut texts {
        let s = t.0.fmt(t.0.get(&settings));
        if text.0 != s {
            text.0 = s;
        }
    }
}

/// Marks a scroll container that follows the mouse wheel while hovered (needs `ScrollPosition` and
/// `RelativeCursorPosition`; bevy_ui scrolls only programmatically).
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct WheelScroll;

fn wheel_scroll(
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    mut q: Query<(&RelativeCursorPosition, &ComputedNode, &mut ScrollPosition), With<WheelScroll>>,
) {
    use bevy::input::mouse::MouseScrollUnit;
    let mut dy = 0.0;
    for e in wheel.read() {
        dy += match e.unit {
            MouseScrollUnit::Line => e.y * 42.0,
            MouseScrollUnit::Pixel => e.y,
        };
    }
    if dy == 0.0 {
        return;
    }
    for (rel, node, mut sp) in &mut q {
        if !rel.cursor_over {
            continue;
        }
        let max = ((node.content_size().y - node.size().y) * node.inverse_scale_factor()).max(0.0);
        sp.y = (sp.y - dy).clamp(0.0, max);
    }
}
