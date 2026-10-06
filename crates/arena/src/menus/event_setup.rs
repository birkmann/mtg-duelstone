//! Event setup: choose what to draft or open (a cube — Legacy, Vintage, Pauper, Arena… or your own —
//! or a real set), how many drafters sit in the pod, and open it.
//!
//! Why the pod opens before anyone else joins: the pod itself is the lobby (friends join it with its
//! invite link, empty seats become Forge AI drafters at the start), so this screen only needs the
//! product and the pod size; everything after happens on the event screen.

use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::RelativeCursorPosition;
use crossbeam_channel::Receiver;
use mtg_client::Api;
use mtg_protocol::{CreateEvent, CubeInfo, EventKind, SeatTicket, SetInfo};

use super::MenuAction;
use super::widgets::{Field, text_field};
use crate::shared::*;
use crate::style::*;

pub struct EventSetupPlugin;

impl Plugin for EventSetupPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SetupState>()
            .add_systems(OnEnter(AppState::EventSetup), (start_loading, spawn_screen))
            .add_systems(Update, (poll, rebuild_list, handle, refresh, poll_create).chain().run_if(in_state(AppState::EventSetup)));
    }
}

/// The seat ticket for the pod we opened or joined. Writer: event setup / multiplayer. Reader: event.
#[derive(Resource, Debug, Clone)]
pub struct EventTicket(pub SeatTicket);

#[derive(Resource, Default)]
struct SetupState {
    cubes: Vec<CubeInfo>,
    sets: Vec<SetInfo>,
    rx: Option<Receiver<(Result<Vec<CubeInfo>, String>, Result<Vec<SetInfo>, String>)>>,
    selected: Option<String>,
    pod: u8,
    /// sealed: open packs of a set instead of a cube
    sealed_sets: bool,
    filter: String,
    status: String,
    error: bool,
    create: Option<Receiver<Result<SeatTicket, String>>>,
    shown_key: String,
}

#[derive(Component)]
struct SourceList;
#[derive(Component)]
struct PodButton(u8);
#[derive(Component)]
struct StatusText;
#[derive(Component)]
struct OpenButton;
#[derive(Component)]
struct SealedToggle(bool);

fn start_loading(mut st: ResMut<SetupState>, cfg: Res<EngineConfig>, choice: Res<EventSetupChoice>) {
    st.status = "Loading…".into();
    st.error = false;
    st.create = None;
    st.filter.clear();
    st.shown_key.clear();
    if st.pod == 0 {
        st.pod = 8;
    }
    st.sealed_sets = false;
    st.selected = choice.1.clone();
    let api = Api::new(cfg.base_url.clone());
    st.rx = Some(mtg_client::spawn(async move {
        let cubes = api.cubes().await.map_err(|e| format!("{e:#}"));
        let sets = api.sets().await.map_err(|e| format!("{e:#}"));
        (cubes, sets)
    }));
}

fn title(kind: EventKind) -> &'static str {
    match kind {
        EventKind::CubeDraft => "CUBE DRAFT",
        EventKind::SetDraft => "SET DRAFT",
        EventKind::Sealed => "SEALED",
    }
}

fn spawn_screen(mut commands: Commands, fonts: Res<ArenaFonts>, choice: Res<EventSetupChoice>) {
    let kind = choice.0;
    commands
        .spawn((
            Name::new("EventSetup"),
            DespawnOnExit(AppState::EventSetup),
            Node { flex_direction: FlexDirection::Column, padding: UiRect::all(Val::Px(26.0)), row_gap: Val::Px(16.0), ..fullscreen_root() },
        ))
        .with_children(|root| {
            root.spawn(Node { width: Val::Percent(100.0), align_items: AlignItems::Center, column_gap: Val::Px(16.0), ..default() }).with_children(|bar| {
                let b = spawn_button(bar, &fonts, ButtonKind::Secondary, "BACK", Vec2::new(130.0, 46.0));
                bar.commands().entity(b).insert(MenuAction::ChangeMode);
                bar.spawn(Node { flex_grow: 1.0, justify_content: JustifyContent::Center, ..default() }).with_child(heading(&fonts, title(kind), 44.0));
                let b = spawn_button(bar, &fonts, ButtonKind::Secondary, "DESIGN CUBES", Vec2::new(200.0, 46.0));
                bar.commands().entity(b).insert(MenuAction::Cubes);
            });
            root.spawn(Node { width: Val::Percent(100.0), flex_grow: 1.0, min_height: Val::Px(0.0), column_gap: Val::Px(22.0), ..default() }).with_children(|mid| {
                mid.spawn(ornate_panel(Node { flex_grow: 1.0, min_width: Val::Px(0.0), padding: UiRect::all(Val::Px(16.0)), flex_direction: FlexDirection::Column, row_gap: Val::Px(10.0), ..default() }))
                    .with_children(|p| {
                        p.spawn(Node { column_gap: Val::Px(12.0), align_items: AlignItems::Center, ..default() }).with_children(|r| {
                            r.spawn(ui_text(&fonts.title, if kind == EventKind::SetDraft { "SETS" } else { "CUBES" }, 20.0, GOLD));
                            if kind == EventKind::Sealed {
                                for (sets, label) in [(false, "CUBES"), (true, "SETS")] {
                                    let b = spawn_button(r, &fonts, ButtonKind::Secondary, label, Vec2::new(110.0, 38.0));
                                    r.commands().entity(b).insert((SealedToggle(sets), MenuAction::PickSource(if sets { "#sets".into() } else { "#cubes".into() })));
                                }
                            }
                            r.spawn(Node { flex_grow: 1.0, ..default() });
                            r.spawn(text_field(&fonts, Field::SourceFilter, "", Val::Px(320.0), 1.0, false));
                        });
                        p.spawn((
                            SourceList,
                            ScrollPosition::default(),
                            RelativeCursorPosition::default(),
                            super::widgets::WheelScroll,
                            Node { width: Val::Percent(100.0), flex_grow: 1.0, min_height: Val::Px(0.0), flex_direction: FlexDirection::Column, row_gap: Val::Px(4.0), overflow: Overflow::scroll_y(), ..default() },
                        ));
                    });
                mid.spawn(ornate_panel(Node { width: Val::Px(360.0), padding: UiRect::all(Val::Px(18.0)), flex_direction: FlexDirection::Column, row_gap: Val::Px(12.0), ..default() }))
                    .with_children(|p| {
                        p.spawn(ui_text(&fonts.title, "POD SIZE", 20.0, GOLD));
                        p.spawn(ui_text(&fonts.body, "Drafters at the table, you included. Friends join with the invite link on the next screen; every empty seat gets a Forge AI drafter.", 14.0, TEXT_DIM));
                        p.spawn(Node { flex_wrap: FlexWrap::Wrap, column_gap: Val::Px(8.0), row_gap: Val::Px(8.0), ..default() }).with_children(|r| {
                            for n in 2..=8u8 {
                                let b = spawn_button(r, &fonts, ButtonKind::Secondary, &n.to_string(), Vec2::new(64.0, 50.0));
                                r.commands().entity(b).insert((PodButton(n), MenuAction::SetPodSize(n)));
                            }
                        });
                        p.spawn(divider());
                        let how = match kind {
                            EventKind::Sealed => "Everyone opens six packs and builds a 40-card deck from them.",
                            _ => "Three packs, passed left, right, left. Pick one card at a time, then build a 40-card deck.",
                        };
                        p.spawn(ui_text(&fonts.body, how, 14.0, TEXT_DIM));
                        p.spawn(Node { flex_grow: 1.0, ..default() });
                        p.spawn((StatusText, ui_text(&fonts.body, "", 15.0, TEXT_DIM)));
                        let b = spawn_button(p, &fonts, ButtonKind::Primary, "OPEN POD", Vec2::new(320.0, 64.0));
                        p.commands().entity(b).insert((MenuAction::OpenPod, OpenButton));
                    });
            });
        });
}

fn poll(mut st: ResMut<SetupState>, choice: Res<EventSetupChoice>, cfg: Res<EngineConfig>, settings: Res<Settings>) {
    let Some(rx) = &st.rx else { return };
    let Ok((cubes, sets)) = rx.try_recv() else { return };
    st.rx = None;
    match (cubes, sets) {
        (Ok(c), Ok(s)) => {
            st.cubes = c;
            st.sets = s;
            st.status.clear();
            // dev aid (ARENA_MENU_SCREEN=draft): open a pod right away for unattended screenshots
            let auto = std::env::var("ARENA_MENU_SCREEN").as_deref() == Ok("draft");
            if st.selected.is_none() {
                // a sensible default: the newest Legacy cube for cube events, the newest set for set drafts
                st.selected = match choice.0 {
                    EventKind::SetDraft => st.sets.first().map(|s| s.code.clone()),
                    _ => st.cubes.iter().filter(|c| c.name.contains("Legacy Cube")).max_by(|a, b| a.name.cmp(&b.name)).or_else(|| st.cubes.first()).map(|c| c.cube_ref.clone()),
                };
            }
            if auto && let Some(source) = st.selected.clone() {
                let req = CreateEvent { kind: choice.0, source, pod_size: st.pod as i64, name: settings.player_name.clone(), seed: None };
                let api = Api::new(cfg.base_url.clone());
                st.create = Some(mtg_client::spawn(async move { api.create_event(&req).await.map_err(|e| format!("{e:#}")) }));
            }
        }
        (Err(e), _) | (_, Err(e)) => {
            st.status = format!("Couldn't load cubes/sets: {e}");
            st.error = true;
        }
    }
}

fn show_sets(kind: EventKind, st: &SetupState) -> bool {
    kind == EventKind::SetDraft || (kind == EventKind::Sealed && st.sealed_sets)
}

fn rebuild_list(
    mut commands: Commands,
    mut st: ResMut<SetupState>,
    choice: Res<EventSetupChoice>,
    fonts: Res<ArenaFonts>,
    fields: Query<(&Field, &EditableText)>,
    list: Query<(Entity, Option<&Children>), With<SourceList>>,
) {
    let filter = fields.iter().find(|(f, _)| **f == Field::SourceFilter).map(|(_, t)| t.value().to_string().to_lowercase()).unwrap_or_default();
    let sets = show_sets(choice.0, &st);
    let key = format!("{sets}|{filter}|{}|{}|{:?}", st.cubes.len(), st.sets.len(), st.selected);
    if key == st.shown_key {
        return;
    }
    st.shown_key = key;
    st.filter = filter.clone();
    let Ok((e, children)) = list.single() else { return };
    for c in children.into_iter().flatten() {
        commands.entity(*c).despawn();
    }
    let rows: Vec<(String, String, String, bool)> = if sets {
        st.sets.iter().filter(|s| filter.is_empty() || s.name.to_lowercase().contains(&filter) || s.code.to_lowercase() == filter).map(|s| (s.code.clone(), s.name.clone(), format!("{} · {}", s.code, s.date.clone().unwrap_or_default()), false)).collect()
    } else {
        st.cubes.iter().filter(|c| filter.is_empty() || c.name.to_lowercase().contains(&filter)).map(|c| (c.cube_ref.clone(), c.name.clone(), format!("{} cards", c.size), c.user)).collect()
    };
    let selected = st.selected.clone();
    commands.entity(e).with_children(|l| {
        for (key, name, meta, user) in rows {
            let on = selected.as_deref() == Some(key.as_str());
            l.spawn((
                Button,
                MenuAction::PickSource(key),
                Node { width: Val::Percent(100.0), padding: UiRect::axes(Val::Px(12.0), Val::Px(8.0)), column_gap: Val::Px(10.0), align_items: AlignItems::Center, border: UiRect::all(Val::Px(1.5)), border_radius: BorderRadius::all(Val::Px(8.0)), flex_shrink: 0.0, ..default() },
                BackgroundColor(if on { Color::srgba(0.95, 0.46, 0.13, 0.22) } else { Color::srgba(1.0, 1.0, 1.0, 0.04) }),
                BorderColor::all(if on { ORANGE } else { Color::NONE }),
            ))
            .with_children(|r| {
                r.spawn((Node { flex_grow: 1.0, ..default() }, ui_text(&fonts.body, name, 17.0, TEXT), Pickable::IGNORE));
                if user {
                    r.spawn((chip(Node { padding: UiRect::axes(Val::Px(10.0), Val::Px(2.0)), ..default() }), Pickable::IGNORE)).with_child(ui_text(&fonts.title, "YOURS", 12.0, GOLD));
                }
                r.spawn((ui_text(&fonts.body, meta, 14.0, TEXT_DIM), Pickable::IGNORE));
            });
        }
    });
}

fn handle(
    q: Query<(&Interaction, &MenuAction, Option<&ArenaButton>), Changed<Interaction>>,
    mut st: ResMut<SetupState>,
    choice: Res<EventSetupChoice>,
    cfg: Res<EngineConfig>,
    settings: Res<Settings>,
) {
    for (i, act, b) in &q {
        if *i != Interaction::Pressed || b.is_some_and(|b| b.disabled) {
            continue;
        }
        match act {
            MenuAction::PickSource(s) if s == "#sets" => {
                st.sealed_sets = true;
                st.selected = st.sets.first().map(|s| s.code.clone());
            }
            MenuAction::PickSource(s) if s == "#cubes" => {
                st.sealed_sets = false;
                st.selected = st.cubes.iter().find(|c| c.name.contains("Legacy Cube")).or_else(|| st.cubes.first()).map(|c| c.cube_ref.clone());
            }
            MenuAction::PickSource(s) => st.selected = Some(s.clone()),
            MenuAction::SetPodSize(n) => st.pod = *n,
            MenuAction::OpenPod => {
                let Some(source) = st.selected.clone() else {
                    st.status = "Pick a cube or set first.".into();
                    st.error = true;
                    continue;
                };
                let req = CreateEvent { kind: choice.0, source, pod_size: st.pod as i64, name: settings.player_name.clone(), seed: None };
                let api = Api::new(cfg.base_url.clone());
                st.status = "Opening the pod…".into();
                st.error = false;
                st.create = Some(mtg_client::spawn(async move { api.create_event(&req).await.map_err(|e| format!("{e:#}")) }));
            }
            _ => {}
        }
    }
}

fn poll_create(mut commands: Commands, mut st: ResMut<SetupState>, mut next: ResMut<NextState<AppState>>, mut mode: ResMut<MatchMode>) {
    let Some(rx) = &st.create else { return };
    let Ok(res) = rx.try_recv() else { return };
    st.create = None;
    match res {
        Ok(t) => {
            *mode = MatchMode::Event { code: t.id.clone() };
            commands.insert_resource(EventTicket(t));
            next.set(AppState::Event);
        }
        Err(e) => {
            st.status = e;
            st.error = true;
        }
    }
}

fn refresh(
    st: Res<SetupState>,
    mut pods: Query<(&PodButton, &mut ArenaButton), Without<SealedToggle>>,
    mut toggles: Query<(&SealedToggle, &mut ArenaButton), Without<PodButton>>,
    mut status: Query<(&mut Text, &mut TextColor), With<StatusText>>,
    mut open: Query<&mut ArenaButton, (With<OpenButton>, Without<PodButton>, Without<SealedToggle>)>,
) {
    for (p, mut b) in &mut pods {
        let want = if p.0 == st.pod { ButtonKind::Primary } else { ButtonKind::Secondary };
        if b.kind != want {
            b.kind = want;
        }
    }
    for (t, mut b) in &mut toggles {
        let want = if t.0 == st.sealed_sets { ButtonKind::Primary } else { ButtonKind::Secondary };
        if b.kind != want {
            b.kind = want;
        }
    }
    for (mut t, mut c) in &mut status {
        if t.0 != st.status {
            t.0 = st.status.clone();
        }
        c.0 = if st.error { RED_DAMAGE } else { TEXT_DIM };
    }
    for mut b in &mut open {
        b.disabled = st.create.is_some() || st.selected.is_none();
    }
}
