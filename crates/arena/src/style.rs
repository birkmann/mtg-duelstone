//! UI style kit: fonts, the game palette and ornate panel/button builders.
//!
//! Why a shared kit: menus, HUD, prompts and the in-world overlays should read as one product.
//! Everything here is plain functions returning bundles (no hidden systems beyond the button
//! hover animation), so any module can `use crate::style::*` and compose.

use bevy::prelude::*;
use bevy::text::{FontSize, FontSource};
use bevy::ui::{BackgroundGradient, ColorStop, LinearGradient};

use crate::shared::ArenaFonts;

// ---------------------------------------------------------------------------------------------
// Palette
// ---------------------------------------------------------------------------------------------

pub const GOLD: Color = Color::srgb(0.86, 0.68, 0.30);
pub const GOLD_BRIGHT: Color = Color::srgb(1.0, 0.86, 0.50);
pub const GOLD_DIM: Color = Color::srgb(0.55, 0.42, 0.18);
pub const BROWN_DEEP: Color = Color::srgb(0.075, 0.045, 0.03);
pub const BROWN: Color = Color::srgb(0.17, 0.10, 0.06);
pub const BROWN_LIGHT: Color = Color::srgb(0.30, 0.19, 0.10);
pub const PARCHMENT: Color = Color::srgb(0.93, 0.87, 0.72);
pub const TEXT: Color = Color::srgb(0.95, 0.92, 0.84);
pub const TEXT_DIM: Color = Color::srgb(0.66, 0.60, 0.50);
/// The "Next / Pass" pill.
pub const ORANGE: Color = Color::srgb(0.95, 0.46, 0.13);
pub const ORANGE_HI: Color = Color::srgb(1.0, 0.64, 0.28);
pub const ORANGE_LO: Color = Color::srgb(0.72, 0.28, 0.07);
pub const BLUE_GLOW: Color = Color::srgb(0.25, 0.65, 1.0);
pub const RED_DAMAGE: Color = Color::srgb(0.95, 0.16, 0.12);
pub const GREEN_HEAL: Color = Color::srgb(0.35, 0.92, 0.45);
pub const SCRIM: Color = Color::srgba(0.02, 0.012, 0.008, 0.72);

/// WUBRG(C) colour of a mana symbol / card colour letter, for pips and frames.
pub fn mana_color(c: char) -> Color {
    match c.to_ascii_uppercase() {
        'W' => Color::srgb(0.96, 0.93, 0.78),
        'U' => Color::srgb(0.30, 0.55, 0.88),
        'B' => Color::srgb(0.30, 0.24, 0.30),
        'R' => Color::srgb(0.88, 0.26, 0.18),
        'G' => Color::srgb(0.22, 0.60, 0.30),
        _ => Color::srgb(0.70, 0.70, 0.74),
    }
}

pub fn mana_icon_path(c: char) -> String {
    format!("textures/icons/mana_{}.png", c.to_ascii_lowercase())
}

// ---------------------------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------------------------

pub struct StylePlugin;

impl Plugin for StylePlugin {
    fn build(&self, app: &mut App) {
        // Loaded in `build` so the resource exists before any Startup system of any plugin runs.
        let assets = app.world().resource::<AssetServer>().clone();
        app.insert_resource(ArenaFonts {
            title: assets.load("fonts/Cinzel-Bold.ttf"),
            body: assets.load("fonts/Inter-Regular.ttf"),
            numbers: assets.load("fonts/Oswald-SemiBold.ttf"),
        })
        .add_systems(Startup, spawn_ui_camera)
        .add_systems(Update, animate_buttons);
    }
}

/// Dedicated overlay camera for every `bevy_ui` tree (menus, HUD). Rendered after the table and
/// hand cameras, never clears, so UI always sits on top of the 3D scene.
#[derive(Component)]
pub struct UiCamera;

fn spawn_ui_camera(mut commands: Commands) {
    commands.spawn((
        Name::new("UiCamera"),
        UiCamera,
        Camera2d,
        Camera { order: 100, clear_color: ClearColorConfig::None, ..default() },
        IsDefaultUiCamera,
    ));
}

// ---------------------------------------------------------------------------------------------
// Text
// ---------------------------------------------------------------------------------------------

pub fn text_font(font: &Handle<Font>, size: f32) -> TextFont {
    TextFont { font: FontSource::Handle(font.clone()), font_size: FontSize::Px(size), ..default() }
}

/// A text node. Pick the face from [`ArenaFonts`]: `title` (Cinzel) for headings/card names,
/// `numbers` (Oswald) for life/P-T/counters, `body` (Inter) for prose.
pub fn ui_text(font: &Handle<Font>, s: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    (Text::new(s), text_font(font, size), TextColor(color))
}

/// Heading with a soft dark shadow so it reads on any background.
pub fn heading(fonts: &ArenaFonts, s: impl Into<String>, size: f32) -> impl Bundle {
    (
        Text::new(s),
        text_font(&fonts.title, size),
        TextColor(GOLD_BRIGHT),
        TextShadow { offset: Vec2::new(0.0, 2.0), color: Color::srgba(0.0, 0.0, 0.0, 0.8) },
    )
}

// ---------------------------------------------------------------------------------------------
// Panels
// ---------------------------------------------------------------------------------------------

/// Ornate panel: deep brown body, gold hairline border, rounded corners, soft drop shadow.
pub fn ornate_panel(node: Node) -> impl Bundle {
    (
        Node {
            border: UiRect::all(Val::Px(2.0)),
            border_radius: BorderRadius::all(Val::Px(12.0)),
            ..node
        },
        BackgroundGradient::from(LinearGradient::to_bottom(vec![
            ColorStop::new(Color::srgba(0.20, 0.12, 0.07, 0.96), Val::Percent(0.0)),
            ColorStop::new(Color::srgba(0.09, 0.055, 0.035, 0.97), Val::Percent(100.0)),
        ])),
        BorderColor::all(GOLD_DIM),
        BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.6), Val::Px(0.0), Val::Px(10.0), Val::Px(2.0), Val::Px(24.0)),
    )
}

/// Translucent dark pill for HUD chips (counters, labels).
pub fn chip(node: Node) -> impl Bundle {
    (
        Node { border: UiRect::all(Val::Px(1.0)), border_radius: BorderRadius::all(Val::Px(99.0)), ..node },
        BackgroundColor(Color::srgba(0.05, 0.03, 0.02, 0.8)),
        BorderColor::all(GOLD_DIM),
    )
}

/// Thin gold divider line.
pub fn divider() -> impl Bundle {
    (
        Node { width: Val::Percent(100.0), height: Val::Px(1.0), margin: UiRect::vertical(Val::Px(8.0)), ..default() },
        BackgroundGradient::from(LinearGradient::to_right(vec![
            ColorStop::new(Color::srgba(0.86, 0.68, 0.30, 0.0), Val::Percent(0.0)),
            ColorStop::new(Color::srgba(0.86, 0.68, 0.30, 0.9), Val::Percent(50.0)),
            ColorStop::new(Color::srgba(0.86, 0.68, 0.30, 0.0), Val::Percent(100.0)),
        ])),
    )
}

// ---------------------------------------------------------------------------------------------
// Buttons
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    /// Orange pill ("Play", "Next").
    Primary,
    /// Brown/gold plate.
    Secondary,
    /// Red.
    Danger,
    /// Transparent text-only (menu entries).
    Ghost,
}

/// Marks a styled button. `disabled` greys it out and the app should ignore its clicks
/// (use `ArenaButton::pressed` helper in handlers).
#[derive(Component, Debug, Clone, Copy)]
pub struct ArenaButton {
    pub kind: ButtonKind,
    pub disabled: bool,
}

/// Smoothed hover/press state, driven by [`animate_buttons`].
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct ButtonAnim {
    hover: f32,
    press: f32,
}

impl ArenaButton {
    pub fn new(kind: ButtonKind) -> Self {
        Self { kind, disabled: false }
    }
    /// True on the frame the button went to `Pressed`... use with `Changed<Interaction>`.
    pub fn pressed(&self, i: &Interaction) -> bool {
        !self.disabled && *i == Interaction::Pressed
    }
}

fn kind_gradient(kind: ButtonKind, h: f32, disabled: bool) -> (BackgroundGradient, Color, Color) {
    // returns (gradient, border, glow)
    let lift = |c: Color, a: f32| -> Color {
        let s = c.to_srgba();
        Color::srgba((s.red + a).min(1.0), (s.green + a).min(1.0), (s.blue + a).min(1.0), s.alpha)
    };
    let (top, bot, border, glow) = match kind {
        ButtonKind::Primary => (ORANGE_HI, ORANGE_LO, Color::srgb(1.0, 0.82, 0.55), ORANGE),
        ButtonKind::Secondary => (BROWN_LIGHT, BROWN, GOLD_DIM, GOLD),
        ButtonKind::Danger => (Color::srgb(0.75, 0.2, 0.15), Color::srgb(0.4, 0.07, 0.05), Color::srgb(1.0, 0.6, 0.5), RED_DAMAGE),
        ButtonKind::Ghost => (Color::srgba(0.0, 0.0, 0.0, 0.0), Color::srgba(0.0, 0.0, 0.0, 0.0), Color::srgba(0.0, 0.0, 0.0, 0.0), GOLD),
    };
    let (top, bot) = if disabled {
        (Color::srgba(0.22, 0.2, 0.19, 0.9), Color::srgba(0.12, 0.11, 0.1, 0.9))
    } else if kind == ButtonKind::Ghost {
        (Color::srgba(0.86, 0.68, 0.30, 0.16 * h), Color::srgba(0.86, 0.68, 0.30, 0.04 * h))
    } else {
        (lift(top, 0.12 * h), lift(bot, 0.08 * h))
    };
    let border = if disabled {
        Color::srgb(0.32, 0.3, 0.28)
    } else if kind == ButtonKind::Ghost {
        Color::srgba(0.86, 0.68, 0.30, 0.55 * h)
    } else {
        border
    };
    (
        BackgroundGradient::from(LinearGradient::to_bottom(vec![
            ColorStop::new(top, Val::Percent(0.0)),
            ColorStop::new(bot, Val::Percent(100.0)),
        ])),
        border,
        glow,
    )
}

/// A button node (no label). Add children with the label via [`button_label`] or use
/// [`spawn_button`].
pub fn button(kind: ButtonKind, node: Node) -> impl Bundle {
    let (grad, border, _) = kind_gradient(kind, 0.0, false);
    let radius = if kind == ButtonKind::Primary { 99.0 } else { 9.0 };
    (
        Button,
        ArenaButton::new(kind),
        ButtonAnim::default(),
        Node {
            border: UiRect::all(Val::Px(2.0)),
            border_radius: BorderRadius::all(Val::Px(radius)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..node
        },
        grad,
        BorderColor::all(border),
        BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.5), Val::Px(0.0), Val::Px(4.0), Val::Px(0.0), Val::Px(8.0)),
        UiTransform::IDENTITY,
    )
}

/// Spawn a labelled button under `parent` and return its entity so the caller can add a marker.
pub fn spawn_button(
    parent: &mut ChildSpawnerCommands,
    fonts: &ArenaFonts,
    kind: ButtonKind,
    label: &str,
    size: Vec2,
) -> Entity {
    let font_size = (size.y * 0.42).clamp(14.0, 34.0);
    let color = match kind {
        ButtonKind::Secondary | ButtonKind::Ghost => GOLD_BRIGHT,
        _ => Color::WHITE,
    };
    parent
        .spawn(button(kind, Node { width: Val::Px(size.x), height: Val::Px(size.y), ..default() }))
        .with_children(|b| {
            b.spawn((
                Text::new(label),
                text_font(&fonts.title, font_size),
                TextColor(color),
                TextShadow { offset: Vec2::new(0.0, 1.5), color: Color::srgba(0.0, 0.0, 0.0, 0.7) },
            ));
        })
        .id()
}

fn animate_buttons(
    time: Res<Time>,
    mut q: Query<(&Interaction, &ArenaButton, &mut ButtonAnim, &mut UiTransform, &mut BackgroundGradient, &mut BorderColor, &mut BoxShadow)>,
) {
    let dt = time.delta_secs();
    for (inter, b, mut a, mut tf, mut grad, mut border, mut shadow) in &mut q {
        let (th, tp) = match (b.disabled, inter) {
            (true, _) => (0.0, 0.0),
            (_, Interaction::Pressed) => (1.0, 1.0),
            (_, Interaction::Hovered) => (1.0, 0.0),
            _ => (0.0, 0.0),
        };
        let k = (dt * 16.0).min(1.0);
        a.hover += (th - a.hover) * k;
        a.press += (tp - a.press) * k;
        let s = 1.0 + 0.045 * a.hover - 0.07 * a.press;
        tf.scale = Vec2::splat(s);
        let (g, bc, glow) = kind_gradient(b.kind, a.hover, b.disabled);
        *grad = g;
        *border = BorderColor::all(bc);
        let gs = glow.to_srgba();
        let ga = if b.disabled || b.kind == ButtonKind::Ghost { 0.0 } else { 0.55 * a.hover };
        shadow.0 = vec![
            bevy::ui::ShadowStyle {
                color: Color::srgba(0.0, 0.0, 0.0, 0.5 * (1.0 - a.hover * 0.5)),
                x_offset: Val::Px(0.0),
                y_offset: Val::Px(4.0 - 2.0 * a.press),
                spread_radius: Val::Px(0.0),
                blur_radius: Val::Px(8.0),
            },
            bevy::ui::ShadowStyle {
                color: Color::srgba(gs.red, gs.green, gs.blue, ga),
                x_offset: Val::Px(0.0),
                y_offset: Val::Px(0.0),
                spread_radius: Val::Px(1.0),
                blur_radius: Val::Px(16.0),
            },
        ];
    }
}

/// Convenience: a full-screen absolutely positioned root node (menus, overlays).
pub fn fullscreen_root() -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: Val::Px(0.0),
        top: Val::Px(0.0),
        width: Val::Percent(100.0),
        height: Val::Percent(100.0),
        ..default()
    }
}

/// Small round mana pip icon node (loads `textures/icons/mana_x.png`).
pub fn mana_pip(assets: &AssetServer, c: char, size: f32) -> impl Bundle {
    (
        Node { width: Val::Px(size), height: Val::Px(size), ..default() },
        ImageNode::new(assets.load(mana_icon_path(c))),
    )
}

/// Keyword badge icon node (`textures/icons/kw_<name>.png`).
pub fn keyword_icon(assets: &AssetServer, keyword: &str, size: f32) -> impl Bundle {
    let name = keyword.to_ascii_lowercase().replace(' ', "_");
    (
        Node { width: Val::Px(size), height: Val::Px(size), ..default() },
        ImageNode::new(assets.load(format!("textures/icons/kw_{name}.png"))),
    )
}

