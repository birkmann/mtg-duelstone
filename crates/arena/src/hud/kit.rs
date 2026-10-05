//! Small bevy_ui toolkit shared by hud / prompts / stack: Arena-ish palette, text + button
//! helpers, mana pips, a card-picture widget and scroll areas.
//!
//! Why a mini toolkit: every widget here is spawned dynamically (prompts differ per kind), so
//! buttons carry a plain-data `OnClick(Act)` and ONE global observer turns pointer clicks into
//! `Acted` messages that each domain system consumes. That keeps spawn code free of closures and
//! makes "disabled" a single marker component.
//!
//! Sizes use `Val::Vh` / `FontSize::Vh` so the whole UI scales with the window height.

use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::picking::Pickable;
use bevy::prelude::*;
use bevy::ui::{ComputedNode, UiGlobalTransform};
use bevy::window::PrimaryWindow;
use mtg_client::ImageVersion;
use mtg_protocol::{Card, PhaseType, PlayerId};

use crate::shared::{ArenaFonts, CardImages};

// ---- palette ------------------------------------------------------------------------------
pub const GOLD: Color = Color::srgb(0.86, 0.69, 0.36);
pub const GOLD_DIM: Color = Color::srgb(0.55, 0.43, 0.22);
pub const INK: Color = Color::srgb(0.94, 0.91, 0.84);
pub const INK_DIM: Color = Color::srgb(0.66, 0.63, 0.58);
pub const PANEL: Color = Color::srgba(0.045, 0.05, 0.075, 0.94);
pub const PANEL_SOFT: Color = Color::srgba(0.07, 0.075, 0.10, 0.78);
pub const SHADE: Color = Color::srgba(0.0, 0.0, 0.0, 0.62);
pub const ORANGE: Color = Color::srgb(0.96, 0.46, 0.14);
pub const ORANGE_HI: Color = Color::srgb(1.0, 0.62, 0.30);
pub const BLUE_GLOW: Color = Color::srgb(0.30, 0.68, 1.0);
pub const RED: Color = Color::srgb(0.90, 0.22, 0.20);
pub const GREEN: Color = Color::srgb(0.30, 0.82, 0.42);

pub const CLEAR: Color = Color::NONE;


#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FontKind {
    Title,
    Body,
    Num,
}

/// Text font from the shared `ArenaFonts` (default handle = Bevy's built-in font, which is fine).
pub fn tfont(fonts: &ArenaFonts, kind: FontKind, size_vh: f32) -> TextFont {
    let h = match kind {
        FontKind::Title => fonts.title.clone(),
        FontKind::Body => fonts.body.clone(),
        FontKind::Num => fonts.numbers.clone(),
    };
    TextFont { font: bevy::text::FontSource::Handle(h), font_size: bevy::text::FontSize::Vh(size_vh), ..default() }
}

/// A non-interactive text node.
pub fn label(fonts: &ArenaFonts, s: impl Into<String>, kind: FontKind, size_vh: f32, color: Color) -> impl Bundle {
    (
        Text::new(s),
        tfont(fonts, kind, size_vh),
        TextColor(color),
        TextShadow { offset: Vec2::new(1.0, 1.5), color: Color::srgba(0.0, 0.0, 0.0, 0.85) },
        Pickable::IGNORE,
    )
}

/// An invisible layout node that never blocks the pointer.
pub fn ghost(node: Node) -> impl Bundle {
    (node, Pickable::IGNORE)
}

/// A decorative filled node that never blocks the pointer.
pub fn deco(node: Node, bg: Color) -> impl Bundle {
    (node, BackgroundColor(bg), Pickable::IGNORE)
}

// ---- actions ------------------------------------------------------------------------------

/// Everything a UI button can ask for. Handled by the module that owns the variant.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    Primary,
    Secondary,
    AttackAll,
    ToggleLog,
    Undo,
    PassTurn,
    OpenSettings,
    ClearYieldsAct,
    ToggleGear,
    ToggleStops,
    Concede,
    ConfirmConcede,
    ToggleStop { phase: PhaseType, mine_turn: bool },
    /// Toggle a stop for the turn context that is current right now (compact icons).
    ToggleStopHere(PhaseType),
    ClickPlayer(PlayerId),
    ClickCard(mtg_protocol::CardId),
    Prompt(PromptAct),
    Yield { key: String, auto: bool },
    ClearYields,
    CloseInspect,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PromptAct {
    /// Answer immediately with this JSON value.
    Answer(serde_json::Value),
    /// Toggle option `i` in a pick prompt.
    Toggle(i64),
    /// Submit the current selection.
    Submit,
    /// Step a numeric field by delta (damage / amount / text numeric).
    Step { key: String, delta: i64 },
    AutoFill,
    /// Move item in order lists: (index in dst list, direction).
    Shift { list: u8, idx: usize, dir: i32 },
    /// Move option between src/dst lists in an order prompt.
    MoveAcross(i64),
    ToggleRemember,
    /// Pick a suggestion into the text field.
    SetText(String),
    /// Manipulate: send card to top/bottom.
    Edge { idx: usize, top: bool },
}

#[derive(Message, Clone, Debug)]
pub struct Acted(pub Act);

/// Marks a clickable UI node and what it does.
#[derive(Component, Clone, Debug)]
pub struct OnClick(pub Act);

#[derive(Component, Clone, Copy, Debug)]
pub struct BtnStyle {
    pub base: Color,
    pub hover: Color,
    pub press: Color,
    pub off: Color,
}

impl BtnStyle {
    pub const DARK: BtnStyle = BtnStyle {
        base: Color::srgba(0.10, 0.11, 0.15, 0.95),
        hover: Color::srgba(0.18, 0.20, 0.27, 0.98),
        press: Color::srgba(0.07, 0.08, 0.11, 1.0),
        off: Color::srgba(0.08, 0.08, 0.10, 0.55),
    };
    pub const ORANGE: BtnStyle = BtnStyle {
        base: Color::srgb(0.93, 0.43, 0.13),
        hover: Color::srgb(1.0, 0.58, 0.25),
        press: Color::srgb(0.75, 0.32, 0.08),
        off: Color::srgba(0.20, 0.20, 0.24, 0.65),
    };
    pub const GOLD: BtnStyle = BtnStyle {
        base: Color::srgb(0.58, 0.44, 0.20),
        hover: Color::srgb(0.74, 0.57, 0.27),
        press: Color::srgb(0.42, 0.31, 0.13),
        off: Color::srgba(0.18, 0.17, 0.15, 0.6),
    };
    pub const PASSIVE: BtnStyle = BtnStyle {
        base: Color::srgba(0.16, 0.17, 0.21, 0.92),
        hover: Color::srgba(0.24, 0.26, 0.32, 0.95),
        press: Color::srgba(0.12, 0.13, 0.16, 0.95),
        off: Color::srgba(0.12, 0.12, 0.15, 0.6),
    };
    pub const GHOST: BtnStyle = BtnStyle {
        base: Color::srgba(1.0, 1.0, 1.0, 0.0),
        hover: Color::srgba(1.0, 1.0, 1.0, 0.12),
        press: Color::srgba(1.0, 1.0, 1.0, 0.06),
        off: Color::srgba(1.0, 1.0, 1.0, 0.0),
    };
}

#[derive(Component)]
pub struct BtnOff;
#[derive(Component)]
pub struct BtnHover;
#[derive(Component)]
pub struct BtnPress;
/// Toggled-on state (e.g. selected card option / active stop).
#[derive(Component)]
pub struct BtnOn;

/// A button with centered text. Returns the entity commands so callers can add children / markers.
pub fn spawn_button<'a>(
    p: &'a mut ChildSpawnerCommands,
    fonts: &ArenaFonts,
    text: &str,
    act: Act,
    style: BtnStyle,
    width: Val,
    height: Val,
    font_vh: f32,
    enabled: bool,
) -> EntityCommands<'a> {
    let mut e = p.spawn((
        Node {
            width,
            height,
            min_width: vh(6.0),
            padding: UiRect::axes(vh(1.4), vh(0.4)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::all(vh(0.18)),
            border_radius: BorderRadius::all(vh(0.9)),
            ..default()
        },
        BackgroundColor(if enabled { style.base } else { style.off }),
        BorderColor::all(if enabled { GOLD_DIM } else { Color::srgba(0.4, 0.4, 0.4, 0.3) }),
        style,
        OnClick(act),
    ));
    if !enabled {
        e.insert(BtnOff);
    }
    e.with_children(|b| {
        // Cinzel is all small-caps: fine for short button words, unreadable for sentences
        let kind = if text.chars().count() > 26 { FontKind::Body } else { FontKind::Title };
        let size = if kind == FontKind::Body { font_vh * 0.92 } else { font_vh };
        b.spawn(label(fonts, text, kind, size, if enabled { INK } else { INK_DIM }));
    });
    e
}

/// Marks every top-level entity the HUD / prompts / stack spawn; wiped when a match is torn down.
#[derive(Component)]
pub struct HudEntity;

pub struct KitPlugin;

impl Plugin for KitPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<Acted>()
            .add_observer(btn_click)
            .add_observer(btn_over)
            .add_observer(btn_out)
            .add_observer(btn_press)
            .add_observer(btn_release)
            .add_systems(Update, (style_buttons, scroll_wheel, update_card_pics));
    }
}

fn btn_click(
    mut on: On<Pointer<Click>>,
    q: Query<(&OnClick, Has<BtnOff>)>,
    mut out: MessageWriter<Acted>,
) {
    let e = on.event_target();
    if std::env::var("ARENA_DBG_CLICK").is_ok() {
        info!("pointer click on {e:?} button={:?} has_onclick={}", on.event().button, q.get(e).is_ok());
    }
    let Ok((act, off)) = q.get(e) else { return };
    on.propagate(false);
    if on.event().button == PointerButton::Primary && !off {
        out.write(Acted(act.0.clone()));
    }
}

fn btn_over(on: On<Pointer<Over>>, q: Query<(), With<BtnStyle>>, mut commands: Commands) {
    let e = on.event_target();
    if q.contains(e) {
        commands.entity(e).insert(BtnHover);
    }
}
fn btn_out(on: On<Pointer<Out>>, q: Query<(), With<BtnStyle>>, mut commands: Commands) {
    let e = on.event_target();
    if q.contains(e) {
        commands.entity(e).remove::<(BtnHover, BtnPress)>();
    }
}
fn btn_press(on: On<Pointer<Press>>, q: Query<(), With<BtnStyle>>, mut commands: Commands) {
    let e = on.event_target();
    if std::env::var("ARENA_DBG_CLICK").is_ok() {
        info!("pointer press on {e:?} styled={}", q.contains(e));
    }
    if q.contains(e) {
        commands.entity(e).insert(BtnPress);
    }
}
fn btn_release(on: On<Pointer<Release>>, q: Query<(), With<BtnStyle>>, mut commands: Commands) {
    let e = on.event_target();
    if std::env::var("ARENA_DBG_CLICK").is_ok() {
        info!("pointer release on {e:?}");
    }
    if q.contains(e) {
        commands.entity(e).remove::<BtnPress>();
    }
}

fn style_buttons(
    mut q: Query<(&BtnStyle, &mut BackgroundColor, Has<BtnHover>, Has<BtnPress>, Has<BtnOff>)>,
) {
    for (s, mut bg, hover, press, off) in &mut q {
        let c = if off {
            s.off
        } else if press {
            s.press
        } else if hover {
            s.hover
        } else {
            s.base
        };
        if bg.0 != c {
            bg.0 = c;
        }
    }
}

// ---- scrolling ----------------------------------------------------------------------------

#[derive(Component)]
pub struct ScrollArea;

pub fn scroll_node(mut n: Node) -> Node {
    n.overflow = Overflow::scroll_y();
    n
}

fn scroll_wheel(
    mut wheel: MessageReader<MouseWheel>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut areas: Query<(&ComputedNode, &UiGlobalTransform, &mut ScrollPosition), With<ScrollArea>>,
) {
    let Ok(win) = windows.single() else { return };
    let Some(cursor) = win.physical_cursor_position() else { return };
    let mut dy = 0.0;
    for ev in wheel.read() {
        dy += match ev.unit {
            MouseScrollUnit::Line => ev.y * 40.0,
            MouseScrollUnit::Pixel => ev.y,
        };
    }
    if dy == 0.0 {
        return;
    }
    for (node, gt, mut sp) in &mut areas {
        if node.contains_point(*gt, cursor) {
            let max = (node.content_size().y - node.size().y).max(0.0) * node.inverse_scale_factor();
            sp.y = (sp.y - dy).clamp(0.0, max);
            break;
        }
    }
}

// ---- mana pips ----------------------------------------------------------------------------

pub fn mana_color(sym: &str) -> (Color, Color) {
    match sym {
        "W" => (Color::srgb(0.96, 0.94, 0.80), Color::srgb(0.2, 0.18, 0.1)),
        "U" => (Color::srgb(0.45, 0.70, 0.93), Color::srgb(0.04, 0.1, 0.2)),
        "B" => (Color::srgb(0.42, 0.37, 0.46), Color::srgb(0.95, 0.92, 0.98)),
        "R" => (Color::srgb(0.92, 0.45, 0.33), Color::srgb(0.25, 0.04, 0.02)),
        "G" => (Color::srgb(0.40, 0.70, 0.45), Color::srgb(0.04, 0.16, 0.06)),
        _ => (Color::srgb(0.78, 0.76, 0.74), Color::srgb(0.12, 0.12, 0.12)),
    }
}

pub fn mana_symbols(cost: &str) -> Vec<String> {
    cost.split('{').filter_map(|s| s.split('}').next()).filter(|s| !s.is_empty()).map(|s| s.to_string()).collect()
}

pub fn spawn_mana_pip(p: &mut ChildSpawnerCommands, fonts: &ArenaFonts, sym: &str, size_vh: f32) {
    let first = sym.split('/').next().unwrap_or(sym);
    let (bg, fg) = mana_color(first);
    p.spawn((
        Node {
            width: vh(size_vh),
            height: vh(size_vh),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::all(vh(size_vh * 0.06)),
            border_radius: BorderRadius::MAX,
            ..default()
        },
        BackgroundColor(bg),
        BorderColor::all(Color::srgba(0.0, 0.0, 0.0, 0.55)),
        Pickable::IGNORE,
    ))
    .with_children(|c| {
        c.spawn(label(fonts, sym.replace('/', ""), FontKind::Num, size_vh * 0.62, fg));
    });
}

pub fn spawn_mana_cost(p: &mut ChildSpawnerCommands, fonts: &ArenaFonts, cost: &str, size_vh: f32) {
    p.spawn(ghost(Node { flex_direction: FlexDirection::Row, column_gap: vh(0.25), align_items: AlignItems::Center, ..default() }))
        .with_children(|r| {
            for s in mana_symbols(cost) {
                spawn_mana_pip(r, fonts, &s, size_vh);
            }
        });
}

// ---- card picture widget ------------------------------------------------------------------

/// A card image that requests its texture lazily and shows a name placeholder until it arrives.
#[derive(Component)]
pub struct CardPic {
    pub card: Card,
    pub version: ImageVersion,
    /// Show the back face of a double-faced card.
    pub back: bool,
    pub handle: Option<Handle<Image>>,
    pub loaded: bool,
}

/// Spawns a card picture of height `h_vh` (63:88 aspect). Child 0 is the placeholder, child 1 the image.
pub fn spawn_card_pic<'a>(
    p: &'a mut ChildSpawnerCommands,
    fonts: &ArenaFonts,
    card: &Card,
    version: ImageVersion,
    h_vh: f32,
) -> EntityCommands<'a> {
    spawn_card_pic_sized(p, fonts, card, version, h_vh * 63.0 / 88.0, h_vh)
}

/// Like `spawn_card_pic` with an explicit box (art crops are roughly 4:3, not card-shaped).
pub fn spawn_card_pic_sized<'a>(
    p: &'a mut ChildSpawnerCommands,
    fonts: &ArenaFonts,
    card: &Card,
    version: ImageVersion,
    w: f32,
    h_vh: f32,
) -> EntityCommands<'a> {
    let name = if card.hidden { "?".to_string() } else { card.display_name().to_string() };
    let mut e = p.spawn((
        Node {
            width: vh(w),
            height: vh(h_vh),
            flex_shrink: 0.0,
            border: UiRect::all(vh(0.2)),
            border_radius: BorderRadius::all(vh(h_vh * 0.045)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(Color::srgb(0.10, 0.10, 0.13)),
        BorderColor::all(Color::srgb(0.25, 0.22, 0.16)),
        CardPic { card: card.clone(), version, back: false, handle: None, loaded: false },
        Pickable::IGNORE,
    ));
    e.with_children(|c| {
        c.spawn(label(fonts, name, FontKind::Title, (h_vh * 0.07).clamp(1.0, 3.0), INK_DIM));
        c.spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                border_radius: BorderRadius::all(vh(h_vh * 0.04)),
                ..default()
            },
            {
                let mut n = ImageNode::default().with_color(Color::NONE);
                n.image_mode = bevy::ui::widget::NodeImageMode::Stretch;
                n
            },
            Pickable::IGNORE,
        ));
    });
    e
}

pub fn update_card_pics(
    mut pics: Query<(&mut CardPic, &Children)>,
    mut imgs: ResMut<CardImages>,
    assets: Res<Assets<Image>>,
    mut nodes: Query<&mut ImageNode>,
    mut texts: Query<&mut TextColor>,
) {
    for (mut pic, children) in &mut pics {
        if pic.card.hidden || pic.card.state.is_none() || pic.loaded || children.len() < 2 {
            continue;
        }
        // request once (the cache hands back the same reserved handle afterwards)
        if pic.handle.is_none() {
            let h = if pic.back {
                let name = if pic.card.oracle_name.is_empty() { pic.card.display_name().to_string() } else { pic.card.oracle_name.clone() };
                imgs.request_raw(mtg_client::ImageRequest {
                    name,
                    set: pic.card.state.as_ref().and_then(|s| s.set_code.clone()),
                    back: true,
                    version: pic.version,
                })
            } else {
                imgs.request(&pic.card, pic.version)
            };
            pic.handle = Some(h);
        }
        let handle = pic.handle.clone().unwrap();
        let loaded = assets.get(&handle).is_some_and(|i| i.width() > 2);
        if let Ok(mut n) = nodes.get_mut(children[1]) {
            if n.image != handle {
                n.image = handle;
            }
            if loaded {
                n.color = Color::WHITE;
            }
        }
        if loaded {
            pic.loaded = true;
            if let Ok(mut t) = texts.get_mut(children[0]) {
                t.0 = Color::NONE;
            }
        }
    }
}

/// Wrap-around helper for centred full-screen layers.
pub fn fullscreen(justify: JustifyContent, align: AlignItems) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: Val::Px(0.0),
        top: Val::Px(0.0),
        width: Val::Percent(100.0),
        height: Val::Percent(100.0),
        justify_content: justify,
        align_items: align,
        ..default()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    Right,
    Down,
    Left,
    Up,
}

/// A chevron icon drawn from two bars (no glyph dependency: the UI fonts lack arrows).
pub fn spawn_chevron(p: &mut ChildSpawnerCommands, dir: Dir, size_vh: f32, color: Color) {
    let s = size_vh;
    let (l, t) = (0.62 * s, 0.17 * s);
    let rot = match dir {
        Dir::Right => 0.0,
        Dir::Down => 90.0,
        Dir::Left => 180.0,
        Dir::Up => 270.0,
    };
    p.spawn((
        Node { width: vh(s), height: vh(s), flex_shrink: 0.0, ..default() },
        UiTransform::from_rotation(Rot2::degrees(rot)),
        Pickable::IGNORE,
    ))
    .with_children(|c| {
        let off = l / (2.0 * std::f32::consts::SQRT_2);
        let (vx, vy) = (0.62 * s, 0.5 * s);
        for (cy, deg) in [(vy - off, -45.0), (vy + off, 45.0)] {
            c.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: vh(vx - off - t / 2.0),
                    top: vh(cy - l / 2.0),
                    width: vh(t),
                    height: vh(l),
                    border_radius: BorderRadius::all(vh(t * 0.5)),
                    ..default()
                },
                UiTransform::from_rotation(Rot2::degrees(deg)),
                BackgroundColor(color),
                Pickable::IGNORE,
            ));
        }
    });
}

/// Square-ish icon button holding a chevron.
pub fn spawn_chevron_button<'a>(p: &'a mut ChildSpawnerCommands, dir: Dir, act: Act, size_vh: f32, enabled: bool) -> EntityCommands<'a> {
    let style = BtnStyle::DARK;
    let mut e = p.spawn((
        Node {
            width: vh(size_vh),
            height: vh(size_vh),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::all(vh(0.15)),
            border_radius: BorderRadius::all(vh(size_vh * 0.22)),
            ..default()
        },
        BackgroundColor(if enabled { style.base } else { style.off }),
        BorderColor::all(if enabled { GOLD_DIM } else { Color::srgba(0.4, 0.4, 0.4, 0.3) }),
        style,
        OnClick(act),
    ));
    if !enabled {
        e.insert(BtnOff);
    }
    e.with_children(|c| spawn_chevron(c, dir, size_vh * 0.62, if enabled { INK } else { INK_DIM }));
    e
}
