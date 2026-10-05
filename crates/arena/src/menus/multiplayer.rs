//! Multiplayer entry screen: host a table (2–4 players) or join one from an invite link / code.
//!
//! Why its own screen before deck select: a joiner must learn the table's format (and which engine
//! hosts it — the link names the host's machine) before a deck can be chosen, and a host picks the
//! table size first. Both then go through the normal deck select with `MatchMode` set, and Loading
//! creates or joins the table. An invite link opened in the browser (`/?join=CODE`) or passed with
//! `--join` lands here with the code filled in and the lookup already running.

use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::text::EditableText;
use crossbeam_channel::Receiver;
use mtg_client::{Api, parse_invite};
use mtg_protocol::{Format, LobbyFrame, SeatKind};

use super::MenuAction;
use super::widgets::{Field, text_field};
use crate::shared::*;
use crate::style::*;

pub struct MultiplayerPlugin;

impl Plugin for MultiplayerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MpState>()
            .add_systems(OnEnter(AppState::Multiplayer), (spawn_screen, auto_join).chain())
            .add_systems(Update, (poll_lookup, refresh).run_if(in_state(AppState::Multiplayer)))
            .add_systems(Update, handle_mp_actions);
    }
}

#[derive(Resource)]
struct MpState {
    players: u8,
    status: String,
    error: bool,
    /// Engine base the pending lookup goes to, and the lookup itself.
    lookup: Option<(String, Receiver<Result<LobbyFrame, String>>)>,
    used_cli_join: bool,
    /// `/api/health` when boot couldn't fetch it (the note under HOST needs the public URL).
    facts: Option<Receiver<Option<mtg_protocol::ServerInfo>>>,
}

impl Default for MpState {
    fn default() -> Self {
        Self { players: 4, status: String::new(), error: false, lookup: None, used_cli_join: false, facts: None }
    }
}

#[derive(Component)]
struct SizeButton(u8);
#[derive(Component)]
struct StatusText;
#[derive(Component)]
struct JoinButton;
#[derive(Component)]
struct HostNote;

fn host_note(facts: &ServerFacts) -> String {
    let reach = facts.0.as_ref().and_then(|f| f.public_url.clone());
    let web = facts.0.as_ref().is_some_and(|f| f.web);
    match (reach, web) {
        (Some(u), true) => format!("Friends on your network open the invite link in any browser — nothing to install. This engine is reachable at {u}."),
        (Some(u), false) => format!("Invitees need this app (or build the browser client with tools/build-web.sh). Engine address: {u}."),
        _ => "Invitees connect to the engine running on this machine.".to_string(),
    }
}

fn spawn_screen(mut commands: Commands, fonts: Res<ArenaFonts>, settings: Res<Settings>, facts: Res<ServerFacts>, cfg: Res<EngineConfig>, mut st: ResMut<MpState>) {
    st.status.clear();
    st.error = false;
    st.lookup = None;
    if facts.0.is_none() {
        let api = Api::new(cfg.base_url.clone());
        st.facts = Some(mtg_client::spawn(async move { api.server_info().await.ok() }));
    }
    let note = host_note(&facts);
    commands
        .spawn((
            Name::new("Multiplayer"),
            DespawnOnExit(AppState::Multiplayer),
            Node { flex_direction: FlexDirection::Column, padding: UiRect::all(Val::Px(28.0)), row_gap: Val::Px(22.0), align_items: AlignItems::Center, ..fullscreen_root() },
        ))
        .with_children(|root| {
            root.spawn(Node { width: Val::Percent(100.0), align_items: AlignItems::Center, column_gap: Val::Px(16.0), ..default() }).with_children(|bar| {
                let b = spawn_button(bar, &fonts, ButtonKind::Secondary, "BACK", Vec2::new(130.0, 46.0));
                bar.commands().entity(b).insert(MenuAction::ToMainMenu);
                bar.spawn(Node { flex_grow: 1.0, justify_content: JustifyContent::Center, ..default() }).with_child(heading(&fonts, "MULTIPLAYER", 44.0));
                bar.spawn(Node { width: Val::Px(130.0), ..default() });
            });
            // your name (it is what the other players see)
            root.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(12.0), ..default() }).with_children(|r| {
                r.spawn(ui_text(&fonts.body, "Your name", 18.0, TEXT_DIM));
                r.spawn(text_field(&fonts, Field::PlayerName, &settings.player_name, Val::Px(320.0), 1.0, false));
            });
            root.spawn(Node { column_gap: Val::Px(28.0), align_items: AlignItems::Stretch, ..default() }).with_children(|row| {
                // ---- host
                row.spawn(ornate_panel(Node { width: Val::Px(520.0), padding: UiRect::all(Val::Px(24.0)), flex_direction: FlexDirection::Column, row_gap: Val::Px(14.0), ..default() }))
                    .with_children(|p| {
                        p.spawn(heading(&fonts, "HOST A TABLE", 28.0));
                        p.spawn(divider());
                        p.spawn(ui_text(&fonts.body, "Players at the table (you included). Seats nobody takes can be filled with the Forge AI.", 16.0, TEXT));
                        p.spawn(Node { column_gap: Val::Px(12.0), ..default() }).with_children(|r| {
                            for n in 2..=4u8 {
                                let b = spawn_button(r, &fonts, ButtonKind::Secondary, &n.to_string(), Vec2::new(84.0, 56.0));
                                r.commands().entity(b).insert((MenuAction::SetTableSize(n), SizeButton(n)));
                            }
                        });
                        p.spawn((HostNote, ui_text(&fonts.body, note, 14.0, TEXT_DIM)));
                        p.spawn(Node { flex_grow: 1.0, ..default() });
                        let b = spawn_button(p, &fonts, ButtonKind::Primary, "CHOOSE DECK & HOST", Vec2::new(360.0, 62.0));
                        p.commands().entity(b).insert(MenuAction::HostTable);
                    });
                // ---- join
                row.spawn(ornate_panel(Node { width: Val::Px(520.0), padding: UiRect::all(Val::Px(24.0)), flex_direction: FlexDirection::Column, row_gap: Val::Px(14.0), ..default() }))
                    .with_children(|p| {
                        p.spawn(heading(&fonts, "JOIN A TABLE", 28.0));
                        p.spawn(divider());
                        p.spawn(ui_text(&fonts.body, "Paste the invite link your host sent (or just its 6-letter code), then pick a deck.", 16.0, TEXT));
                        p.spawn(text_field(&fonts, Field::Invite, "", Val::Percent(100.0), 1.0, false));
                        p.spawn(Node { flex_grow: 1.0, ..default() });
                        let b = spawn_button(p, &fonts, ButtonKind::Primary, "FIND TABLE", Vec2::new(360.0, 62.0));
                        p.commands().entity(b).insert((MenuAction::JoinTable, JoinButton));
                    });
            });
            root.spawn((StatusText, ui_text(&fonts.body, "", 18.0, TEXT_DIM)));
        });
}

/// `--join` / `?join=`: fill the field and start the lookup once.
fn auto_join(cli: Res<CliArgs>, mut st: ResMut<MpState>, cfg: Res<EngineConfig>, mut fields: Query<(&Field, &mut EditableText)>) {
    if st.used_cli_join {
        return;
    }
    let Some(invite) = cli.join.clone() else { return };
    st.used_cli_join = true;
    crate::platform::clear_query();
    for (f, mut t) in &mut fields {
        if *f == Field::Invite {
            t.editor_mut().set_text(&invite);
        }
    }
    start_lookup(&mut st, &cfg.base_url, &invite);
}

/// Resolve an invite to (engine base, lobby) in the background.
fn start_lookup(st: &mut MpState, current_base: &str, invite: &str) {
    let Some((base, code)) = parse_invite(invite) else {
        st.status = "That doesn't look like an invite link or table code.".into();
        st.error = true;
        return;
    };
    // In the browser the page's own engine is the only one we may talk to without CORS surprises —
    // and an invite opened there already came from it — so a link's host only matters on desktop.
    let base = if crate::platform::WEB { current_base.to_string() } else { base.unwrap_or_else(|| current_base.to_string()) };
    st.status = format!("Looking for table {code}…");
    st.error = false;
    let api = Api::new(base.clone());
    st.lookup = Some((base, mtg_client::spawn(async move { api.lobby(&code).await.map_err(|e| format!("{e:#}")) })));
}

fn poll_lookup(
    mut st: ResMut<MpState>,
    mut cfg: ResMut<EngineConfig>,
    mut settings: ResMut<Settings>,
    mut mode: ResMut<MatchMode>,
    mut next: ResMut<NextState<AppState>>,
) {
    let Some((base, rx)) = &st.lookup else { return };
    let Ok(res) = rx.try_recv() else { return };
    let base = base.clone();
    st.lookup = None;
    match res {
        Ok(l) if l.started => {
            st.status = format!("Table {} has already started.", l.id);
            st.error = true;
        }
        Ok(l) if !l.seats.iter().any(|s| s.kind == SeatKind::Open) => {
            st.status = format!("Table {} is full.", l.id);
            st.error = true;
        }
        Ok(l) => {
            // From here on decks, images and the socket all come from the host's engine.
            if cfg.base_url != base {
                info!("joining a table on {base}");
                cfg.base_url = base;
            }
            settings.last_format = l.format.unwrap_or(Format::Constructed);
            *mode = MatchMode::Join { code: l.id.clone() };
            next.set(AppState::DeckSelect);
        }
        Err(e) => {
            st.status = if e.contains("404") { "No table with that code (it may have finished).".into() } else { format!("Couldn't reach the table: {e}") };
            st.error = true;
        }
    }
}

fn refresh(
    mut st: ResMut<MpState>,
    mut facts: ResMut<ServerFacts>,
    mut note: Query<&mut Text, (With<HostNote>, Without<StatusText>)>,
    mut sizes: Query<(&SizeButton, &mut ArenaButton)>, mut status: Query<(&mut Text, &mut TextColor), With<StatusText>>, mut join: Query<&mut ArenaButton, (With<JoinButton>, Without<SizeButton>)>) {
    if let Some(rx) = &st.facts
        && let Ok(info) = rx.try_recv()
    {
        st.facts = None;
        facts.0 = info;
        for mut t in &mut note {
            t.0 = host_note(&facts);
        }
    }
    for (s, mut b) in &mut sizes {
        let want = if s.0 == st.players { ButtonKind::Primary } else { ButtonKind::Secondary };
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
    for mut b in &mut join {
        b.disabled = st.lookup.is_some();
    }
}

/// Clicks of the multiplayer / lobby buttons (everything else is `actions::handle_actions`).
fn handle_mp_actions(
    q: Query<(&Interaction, &MenuAction, Option<&ArenaButton>), Changed<Interaction>>,
    mut st: ResMut<MpState>,
    mut mode: ResMut<MatchMode>,
    mut next: ResMut<NextState<AppState>>,
    cfg: Res<EngineConfig>,
    fields: Query<(&Field, &EditableText)>,
    mut focus: ResMut<InputFocus>,
    lobby: Res<LobbyView>,
    match_cfg: Res<MatchConfig>,
    mut out: MessageWriter<SendCmd>,
    mut clipboard: ResMut<bevy::clipboard::Clipboard>,
    mut copied: ResMut<super::lobby::CopyFeedback>,
) {
    for (i, act, btn) in &q {
        if *i != Interaction::Pressed || btn.is_some_and(|b| b.disabled) {
            continue;
        }
        match act {
            MenuAction::Multiplayer => next.set(AppState::Multiplayer),
            MenuAction::SetTableSize(n) => st.players = (*n).clamp(2, 4),
            MenuAction::HostTable => {
                *mode = MatchMode::Host { players: st.players };
                next.set(AppState::DeckSelect);
            }
            MenuAction::JoinTable => {
                let text = fields.iter().find(|(f, _)| **f == Field::Invite).map(|(_, t)| t.value().to_string()).unwrap_or_default();
                focus.clear();
                start_lookup(&mut st, &cfg.base_url, &text);
            }
            MenuAction::CopyInvite => {
                if let Some(link) = lobby.frame.as_ref().and_then(|l| l.invite.clone()) {
                    copied.0 = if crate::platform::clipboard_usable() && clipboard.set_text(link).is_ok() { Some(true) } else { Some(false) };
                }
            }
            MenuAction::SeatAi(seat) => {
                let deck = Some(match_cfg.ai_deck.clone()).filter(|d| !d.is_empty());
                out.write(SendCmd(mtg_protocol::ClientFrame::LobbySeat { seat: *seat, kind: SeatKind::Ai, deck }));
            }
            MenuAction::SeatOpen(seat) => {
                out.write(SendCmd(mtg_protocol::ClientFrame::LobbySeat { seat: *seat, kind: SeatKind::Open, deck: None }));
            }
            MenuAction::StartTable => {
                out.write(SendCmd(mtg_protocol::ClientFrame::Start { seed: match_cfg.seed }));
            }
            MenuAction::LeaveTable => next.set(AppState::MainMenu),
            _ => {}
        }
    }
}
