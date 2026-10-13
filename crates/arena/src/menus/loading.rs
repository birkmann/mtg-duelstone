//! Loading screen: "Finding opponent…" with a VS splash of both decks while the match is created.

use bevy::prelude::*;
use bevy::ui::{BackgroundGradient, ColorStop, LinearGradient};

use super::deck_select::DeckList;
use crate::shared::{AppState, ArenaFonts, BootStatus, MatchConfig, MatchDeckNames};
use crate::style::*;

pub struct LoadingPlugin;

impl Plugin for LoadingPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::Loading), spawn_loading).add_systems(Update, update_loading.run_if(in_state(AppState::Loading)));
    }
}

#[derive(Component)]
struct Dots;
#[derive(Component)]
struct StatusLine;
#[derive(Component)]
struct Slide {
    from: f32,
    delay: f32,
}
#[derive(Component)]
struct Bar;
#[derive(Resource)]
struct LoadingStarted(f32);
#[derive(Component)]
struct BackRow;

fn spawn_loading(mut commands: Commands, fonts: Res<ArenaFonts>, cfg: Res<MatchConfig>, list: Res<DeckList>, names: Res<MatchDeckNames>, time: Res<Time>) {
    commands.insert_resource(LoadingStarted(time.elapsed_secs()));
    let ai = if names.ai.is_empty() { list.name_of(&cfg.ai_deck) } else { names.ai.clone() };
    commands
        .spawn((
            Name::new("LoadingScreen"),
            DespawnOnExit(AppState::Loading),
            Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, justify_content: JustifyContent::Center, row_gap: Val::Vh(3.0), padding: UiRect::all(Val::VMin(4.0)), overflow: Overflow::clip(), ..fullscreen_root() },
        ))
        .with_children(|root| {
            root.spawn((Dots, Text::new("PREPARING YOUR MATCH"), TextFont { font_size: bevy::text::FontSize::VMin(4.0), ..text_font(&fonts.title, 36.0) }, TextColor(GOLD_BRIGHT), TextLayout { justify: Justify::Center, ..default() }));
            root.spawn(Node { width: Val::Percent(100.0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, column_gap: Val::Vw(2.0), ..default() }).with_children(|r| {
                vs_plate(r, &fonts, "YOU", &human_or(&cfg.human_deck, &list, &names.human), -400.0, 0.0);
                r.spawn((
                    Slide { from: 0.0, delay: 0.25 },
                    UiTransform::IDENTITY,
                    Text::new("VS"), TextFont { font_size: bevy::text::FontSize::VMin(6.0), ..text_font(&fonts.title, 72.0) }, TextColor(ORANGE),
                    TextShadow { offset: Vec2::new(0.0, 4.0), color: Color::srgba(0.0, 0.0, 0.0, 0.9) },
                ));
                vs_plate(r, &fonts, "OPPONENT", &ai, 400.0, 0.1);
            });
            root.spawn((BackRow, Node { display: Display::None, flex_shrink: 0.0, ..default() })).with_children(|row| {
                let b = spawn_button(row, &fonts, ButtonKind::Secondary, "BACK TO MENU", Vec2::new(200.0, 44.0));
                row.commands().entity(b).insert(super::MenuAction::Back);
            });
            root.spawn((StatusLine, ui_text(&fonts.body, "", 17.0, TEXT_DIM), TextLayout { justify: Justify::Center, ..default() }, Node { max_width: Val::Percent(90.0), max_height: Val::Vh(20.0), overflow: Overflow::scroll_y(), ..default() }, crate::hud::kit::ScrollArea));
            root.spawn((Node { width: Val::Vw(48.0), height: Val::Px(5.0), overflow: Overflow::clip(), border_radius: BorderRadius::all(Val::Px(99.0)), ..default() }, BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.08))))
                .with_children(|t| {
                    t.spawn((
                        Bar,
                        Node { position_type: PositionType::Absolute, width: Val::Percent(35.0), height: Val::Percent(100.0), ..default() },
                        BackgroundGradient::from(LinearGradient::to_right(vec![
                            ColorStop::new(Color::srgba(0.95, 0.46, 0.13, 0.0), Val::Percent(0.0)),
                            ColorStop::new(ORANGE_HI, Val::Percent(60.0)),
                            ColorStop::new(Color::srgba(0.95, 0.46, 0.13, 0.0), Val::Percent(100.0)),
                        ])),
                    ));
                });
        });
}

fn human_or(deck_ref: &str, list: &DeckList, name: &str) -> String {
    if name.is_empty() { list.name_of(deck_ref) } else { name.to_string() }
}

fn vs_plate(parent: &mut ChildSpawnerCommands, fonts: &ArenaFonts, who: &str, deck: &str, from_x: f32, delay: f32) {
    parent
        .spawn((
            Slide { from: from_x, delay },
            UiTransform::from_translation(Val2::px(from_x, 0.0)),
            ornate_panel(Node { width: Val::Vw(34.0), min_width: Val::Px(0.0), padding: UiRect::all(Val::VMin(2.0)), flex_direction: FlexDirection::Column, align_items: AlignItems::Center, row_gap: Val::Px(8.0), ..default() }),
        ))
        .with_children(|p| {
            p.spawn(ui_text(&fonts.body, who, 15.0, TEXT_DIM));
            p.spawn((Text::new(deck), TextFont { font_size: bevy::text::FontSize::VMin(3.0), ..text_font(&fonts.title, 28.0) }, TextColor(PARCHMENT), TextLayout { justify: Justify::Center, ..default() }));
        });
}

fn update_loading(
    time: Res<Time>,
    status: Res<BootStatus>,
    mut dots: Query<&mut Text, (With<Dots>, Without<StatusLine>)>,
    mut st: Query<(&mut Text, &mut TextColor), (With<StatusLine>, Without<Dots>)>,
    mut bar: Query<&mut Node, (With<Bar>, Without<BackRow>)>,
    mut back: Query<&mut Node, (With<BackRow>, Without<Bar>)>,
    mut slides: Query<(&Slide, &mut UiTransform)>,
    started: Res<LoadingStarted>,
) {
    let entered = time.elapsed_secs() - started.0;
    for mut node in &mut back { node.display = if status.error.is_some() { Display::Flex } else { Display::None }; }
    let t = time.elapsed_secs();
    let n = ((t * 2.5) as usize) % 4;
    for mut d in &mut dots {
        d.0 = format!("PREPARING YOUR MATCH{}", ".".repeat(n));
    }
    for (mut s, mut c) in &mut st {
        let (msg, col) = match &status.error {
            Some(e) => (e.clone(), RED_DAMAGE),
            None => (status.message.clone(), TEXT_DIM),
        };
        if s.0 != msg {
            s.0 = msg;
        }
        c.0 = col;
    }
    for mut n in &mut bar {
        n.left = Val::Percent((t * 0.7).fract() * 135.0 - 35.0);
    }
    for (s, mut tf) in &mut slides {
        if s.from != 0.0 {
            let p = ((entered - s.delay) / 0.55).clamp(0.0, 1.0);
            let e = 1.0 - (1.0 - p).powi(3);
            tf.translation = Val2::px(s.from * (1.0 - e), 0.0);
        }
    }
}
