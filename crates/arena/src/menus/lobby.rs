//! The table before the game: who sits where, the invite link to share, and (for the host) seat
//! controls and START.
//!
//! Why a screen of its own rather than a modal over deck select: by now our seat is taken and the
//! socket is open — the engine pushes a `lobby` frame on every change (someone joins, a seat is
//! filled with AI, a player's connection drops), and the screen is a pure function of the latest one
//! (`LobbyView`). The host's START turns the same socket into the game: `hello` + the first `state`
//! arrive and app_state moves on to InMatch.

use bevy::prelude::*;
use mtg_protocol::{LobbyFrame, SeatInfo, SeatKind};

use super::MenuAction;
use crate::shared::*;
use crate::style::*;

pub struct LobbyPlugin;

impl Plugin for LobbyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CopyFeedback>()
            .add_systems(OnEnter(AppState::Lobby), spawn_screen)
            .add_systems(Update, (rebuild_seats, refresh_texts).run_if(in_state(AppState::Lobby)));
    }
}

/// Result of the last COPY LINK press: Some(true) copied, Some(false) clipboard unavailable.
#[derive(Resource, Default)]
pub struct CopyFeedback(pub Option<bool>);

#[derive(Component)]
struct Title;
#[derive(Component)]
struct InviteText;
#[derive(Component)]
struct InviteHint;
#[derive(Component)]
struct SeatList;
#[derive(Component)]
struct StatusText;
#[derive(Component)]
struct StartButton;
#[derive(Component)]
struct CopyButton;

fn spawn_screen(mut commands: Commands, fonts: Res<ArenaFonts>, mut copied: ResMut<CopyFeedback>) {
    copied.0 = None;
    commands
        .spawn((
            Name::new("Lobby"),
            DespawnOnExit(AppState::Lobby),
            Node { flex_direction: FlexDirection::Column, padding: UiRect::all(Val::Px(28.0)), row_gap: Val::Px(20.0), align_items: AlignItems::Center, ..fullscreen_root() },
        ))
        .with_children(|root| {
            root.spawn((Title, heading(&fonts, "TAKING YOUR SEAT…", 44.0)));
            // ---- invite
            root.spawn(ornate_panel(Node { width: Val::Px(900.0), max_width: Val::Percent(100.0), padding: UiRect::all(Val::Px(20.0)), flex_direction: FlexDirection::Column, row_gap: Val::Px(10.0), ..default() }))
                .with_children(|p| {
                    p.spawn(ui_text(&fonts.title, "INVITE", 20.0, GOLD));
                    p.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(16.0), ..default() }).with_children(|r| {
                        r.spawn((
                            Node {
                                flex_grow: 1.0,
                                padding: UiRect::axes(Val::Px(14.0), Val::Px(10.0)),
                                border: UiRect::all(Val::Px(2.0)),
                                border_radius: BorderRadius::all(Val::Px(8.0)),
                                ..default()
                            },
                            BackgroundColor(Color::srgba(0.03, 0.02, 0.015, 0.9)),
                            BorderColor::all(GOLD_DIM),
                        ))
                        .with_child((InviteText, ui_text(&fonts.body, "…", 22.0, PARCHMENT)));
                        let b = spawn_button(r, &fonts, ButtonKind::Secondary, "COPY LINK", Vec2::new(170.0, 52.0));
                        r.commands().entity(b).insert((MenuAction::CopyInvite, CopyButton));
                    });
                    p.spawn((InviteHint, ui_text(&fonts.body, "", 15.0, TEXT_DIM)));
                });
            // ---- seats
            root.spawn(ornate_panel(Node { width: Val::Px(900.0), max_width: Val::Percent(100.0), padding: UiRect::all(Val::Px(20.0)), flex_direction: FlexDirection::Column, row_gap: Val::Px(10.0), ..default() }))
                .with_children(|p| {
                    p.spawn(ui_text(&fonts.title, "SEATS", 20.0, GOLD));
                    p.spawn(divider());
                    p.spawn((SeatList, Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(8.0), width: Val::Percent(100.0), ..default() }));
                });
            root.spawn(Node { flex_grow: 1.0, ..default() });
            root.spawn(Node { width: Val::Px(900.0), max_width: Val::Percent(100.0), align_items: AlignItems::Center, column_gap: Val::Px(16.0), ..default() }).with_children(|bar| {
                let b = spawn_button(bar, &fonts, ButtonKind::Secondary, "LEAVE", Vec2::new(150.0, 56.0));
                bar.commands().entity(b).insert(MenuAction::LeaveTable);
                bar.spawn((StatusText, Node { flex_grow: 1.0, ..default() }, ui_text(&fonts.body, "", 17.0, TEXT_DIM)));
                let b = spawn_button(bar, &fonts, ButtonKind::Primary, "START GAME", Vec2::new(260.0, 66.0));
                bar.commands().entity(b).insert((MenuAction::StartTable, StartButton));
            });
        });
}

fn is_host(l: &LobbyFrame) -> bool {
    l.you == Some(l.host)
}

fn rebuild_seats(mut commands: Commands, lobby: Res<LobbyView>, fonts: Res<ArenaFonts>, list: Query<(Entity, Option<&Children>), With<SeatList>>, added: Query<(), Added<SeatList>>) {
    if !lobby.is_changed() && added.is_empty() {
        return;
    }
    let Ok((e, children)) = list.single() else { return };
    for c in children.into_iter().flatten() {
        commands.entity(*c).despawn();
    }
    let Some(l) = lobby.frame.as_ref() else { return };
    let host = is_host(l);
    commands.entity(e).with_children(|list| {
        for s in &l.seats {
            spawn_seat_row(list, &fonts, s, s.seat == l.host, host);
        }
    });
}

fn spawn_seat_row(list: &mut ChildSpawnerCommands, fonts: &ArenaFonts, s: &SeatInfo, seat_is_host: bool, viewer_is_host: bool) {
    let (name, name_col) = match s.kind {
        SeatKind::Open => ("Open seat — waiting for a player".to_string(), TEXT_DIM),
        _ => (s.name.clone().unwrap_or_default(), if s.you { GOLD_BRIGHT } else { PARCHMENT }),
    };
    let tag = match (s.kind, seat_is_host, s.you) {
        (SeatKind::Ai, ..) => "AI",
        (SeatKind::Human, true, true) => "HOST · YOU",
        (SeatKind::Human, true, false) => "HOST",
        (SeatKind::Human, false, true) => "YOU",
        (SeatKind::Human, ..) => "PLAYER",
        (SeatKind::Open, ..) => "",
    };
    let online = s.kind == SeatKind::Open || s.connected;
    list.spawn((
        Node {
            width: Val::Percent(100.0),
            padding: UiRect::axes(Val::Px(14.0), Val::Px(10.0)),
            align_items: AlignItems::Center,
            column_gap: Val::Px(14.0),
            border: UiRect::all(Val::Px(1.5)),
            border_radius: BorderRadius::all(Val::Px(10.0)),
            ..default()
        },
        BackgroundColor(if s.you { Color::srgba(0.95, 0.46, 0.13, 0.14) } else { Color::srgba(1.0, 1.0, 1.0, 0.04) }),
        BorderColor::all(if s.you { ORANGE } else { Color::NONE }),
    ))
    .with_children(|r| {
        r.spawn(ui_text(&fonts.title, format!("{}", s.seat + 1), 22.0, GOLD));
        // presence dot: green online, grey open, red for a seated player whose socket is gone
        r.spawn((
            Node { width: Val::Px(12.0), height: Val::Px(12.0), border_radius: BorderRadius::MAX, ..default() },
            BackgroundColor(match (s.kind, online) {
                (SeatKind::Open, _) => Color::srgba(1.0, 1.0, 1.0, 0.18),
                (_, true) => GREEN_HEAL,
                (_, false) => RED_DAMAGE,
            }),
        ));
        r.spawn(Node { flex_direction: FlexDirection::Column, flex_grow: 1.0, row_gap: Val::Px(2.0), ..default() }).with_children(|c| {
            c.spawn(ui_text(&fonts.title, name, 20.0, name_col));
            let sub = match s.kind {
                SeatKind::Open => String::new(),
                _ if !online => "connection lost — waiting for them to come back".into(),
                _ => s.deck_name.clone().unwrap_or_default(),
            };
            c.spawn(ui_text(&fonts.body, sub, 14.0, TEXT_DIM));
        });
        if !tag.is_empty() {
            r.spawn(chip(Node { padding: UiRect::axes(Val::Px(12.0), Val::Px(4.0)), ..default() })).with_child(ui_text(&fonts.title, tag, 14.0, GOLD));
        }
        if viewer_is_host && !seat_is_host {
            if s.kind != SeatKind::Ai {
                let b = spawn_button(r, fonts, ButtonKind::Secondary, "ADD AI", Vec2::new(120.0, 42.0));
                r.commands().entity(b).insert(MenuAction::SeatAi(s.seat));
            }
            if s.kind != SeatKind::Open {
                let label = if s.kind == SeatKind::Human { "REMOVE" } else { "OPEN" };
                let b = spawn_button(r, fonts, ButtonKind::Ghost, label, Vec2::new(120.0, 42.0));
                r.commands().entity(b).insert(MenuAction::SeatOpen(s.seat));
            }
        }
    });
}

fn refresh_texts(
    lobby: Res<LobbyView>,
    link: Res<LinkStatus>,
    copied: Res<CopyFeedback>,
    mut texts: ParamSet<(
        Query<&mut Text, With<Title>>,
        Query<&mut Text, With<InviteText>>,
        Query<&mut Text, With<InviteHint>>,
        Query<(&mut Text, &mut TextColor), With<StatusText>>,
    )>,
    mut start: Query<(&mut ArenaButton, &mut Node), With<StartButton>>,
) {
    let set = |t: &mut Text, s: String| {
        if t.0 != s {
            t.0 = s;
        }
    };
    let l = lobby.frame.as_ref();
    let title = match l {
        Some(l) => format!("TABLE {}  ·  {}", l.id, l.format.map(|f| f.as_str().to_uppercase()).unwrap_or_default()),
        None => "TAKING YOUR SEAT…".into(),
    };
    for mut t in &mut texts.p0() {
        set(&mut t, title.clone());
    }
    let invite = l.map(|l| l.invite.clone().unwrap_or_else(|| l.id.clone())).unwrap_or_else(|| "…".into());
    for mut t in &mut texts.p1() {
        set(&mut t, invite.clone());
    }
    let hint = match copied.0 {
        Some(true) => "Copied! Send it to your friends — anyone with the link can take an open seat.".to_string(),
        Some(false) => "This page can't use the clipboard (plain http) — copy the link above by hand.".to_string(),
        None => match l {
            Some(l) if l.invite.is_some() => {
                "Friends on your network open this link in a browser (or paste it in MULTIPLAYER → JOIN). For play over the internet start the engine with --public-url.".to_string()
            }
            Some(l) => format!("Friends join with MULTIPLAYER → JOIN A TABLE and code {}.", l.id),
            None => String::new(),
        },
    };
    for mut t in &mut texts.p2() {
        set(&mut t, hint.clone());
    }
    let host = l.is_some_and(is_host);
    let filled = l.map(|l| l.seats.iter().filter(|s| s.kind != SeatKind::Open).count()).unwrap_or(0);
    let (status, err) = if *link != LinkStatus::Connected {
        ("Connecting to the table…".to_string(), false)
    } else if let Some(e) = &lobby.error {
        (e.clone(), true)
    } else {
        match l {
            None => ("Waiting for the table…".into(), false),
            Some(l) if host && l.can_start => (format!("{filled} players ready — open seats are left out."), false),
            Some(_) if host && filled < 2 => ("Waiting for players to join (or add an AI).".into(), false),
            Some(_) if host => ("Waiting for everyone to connect…".into(), false),
            Some(_) => ("Waiting for the host to start the game…".into(), false),
        }
    };
    for (mut t, mut c) in &mut texts.p3() {
        set(&mut t, status.clone());
        c.0 = if err { RED_DAMAGE } else { TEXT_DIM };
    }
    for (mut b, mut n) in &mut start {
        b.disabled = !l.is_some_and(|l| l.can_start);
        let want = if host { Display::Flex } else { Display::None };
        if n.display != want {
            n.display = want;
        }
    }
}
