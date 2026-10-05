//! Cross-plugin CONTRACT. Every plugin talks to the others only through the types in this file:
//! resources (written by exactly one owner, read by anyone), messages, and marker components.
//!
//! Rules: append-only — add fields/variants/types, never rename or remove. Each item names its
//! single writer ("Owner:"). Keep this file free of systems.

use std::collections::HashMap;
use std::path::PathBuf;

use bevy::prelude::*;
use mtg_protocol::{
    Card, CardId, ClientFrame, EventCue, Format, Game, InputFrame, LogEntry, PhaseType, Player, PlayerId,
    PromptFrame, ServerFrame, Stops, ZoneKind,
};

// =============================================================================================
// App flow
// =============================================================================================

/// Owner: app_state (transitions), menus (MainMenu/DeckSelect/GameOver buttons).
#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AppState {
    /// Connect to / autolaunch the engine (splash).
    #[default]
    Boot,
    MainMenu,
    DeckSelect,
    /// Create match, open socket, wait for first `state`, preload images.
    Loading,
    InMatch,
    /// Overlay over the final board; InMatch entities are despawned when leaving this state.
    GameOver,
}

/// Parsed command line. Owner: main.rs (inserted before the app runs).
#[derive(Resource, Debug, Clone, Default)]
pub struct CliArgs {
    /// `--format constructed|commander`
    pub format: Option<Format>,
    /// `--deck <ref>` / `--ai-deck <ref>` (deck refs as listed by `/api/decks`)
    pub deck: Option<String>,
    pub ai_deck: Option<String>,
    /// `--seed N`
    pub seed: Option<i64>,
    /// `--quick`: skip menus, go straight to a match (random decks if none given)
    pub quick: bool,
    /// `--replay file.jsonl`: drive the board from a recording, no engine
    pub replay: Option<PathBuf>,
    /// `--record file.jsonl`: tee inbound frames
    pub record: Option<PathBuf>,
    /// `--engine http://127.0.0.1:8080`
    pub engine_url: Option<String>,
    /// `--no-autolaunch`
    pub no_autolaunch: bool,
    /// `--screenshot out.png`: save a screenshot after `--exit-after` seconds, then exit
    pub screenshot: Option<PathBuf>,
    /// `--exit-after SECS`
    pub exit_after: Option<f32>,
    /// `--autoplay`: the human seat is played by `mtg_client::autoplay` (debug / screenshots)
    pub autoplay: bool,
}

/// Engine connection settings. Owner: app_state (from CliArgs + Settings).
#[derive(Resource, Debug, Clone)]
pub struct EngineConfig {
    pub base_url: String,
    pub autolaunch: bool,
    pub jar: PathBuf,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            base_url: "http://127.0.0.1:8080".into(),
            autolaunch: true,
            jar: PathBuf::from("engine/target/forge-ws-server.jar"),
        }
    }
}

/// What to play. Owner: menus (DeckSelect) or app_state (CLI `--quick`). Read by Loading.
#[derive(Resource, Debug, Clone, Default)]
pub struct MatchConfig {
    pub format: Format,
    pub human_deck: String,
    pub ai_deck: String,
    pub seed: Option<i64>,
}

/// Status line for splash/loading screens. Owner: app_state.
#[derive(Resource, Debug, Clone, Default)]
pub struct BootStatus {
    pub message: String,
    pub error: Option<String>,
}

// =============================================================================================
// Network
// =============================================================================================

/// Every inbound server frame, in order. Owner: net.
#[derive(Message, Debug, Clone)]
pub struct ServerMsg(pub ServerFrame);

/// Outbound command — ANY plugin may write; net sends it. (Buttons, clicks, prompt answers.)
#[derive(Message, Debug, Clone)]
pub struct SendCmd(pub ClientFrame);

/// Owner: net.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LinkStatus {
    #[default]
    Disconnected,
    Connecting,
    Connected,
}

// =============================================================================================
// Game view (owner: view) — the latest engine truth, never mutated by anyone else
// =============================================================================================

#[derive(Resource, Debug, Clone, Default)]
pub struct GameView {
    /// `state.seq` of the frame this view came from (0 = no state yet).
    pub seq: i64,
    pub you: Option<PlayerId>,
    pub format: Format,
    pub stops: Stops,
    pub auto_yields: Vec<String>,
    pub game: Game,
}

impl GameView {
    pub fn has_state(&self) -> bool {
        self.seq > 0 || !self.game.players.is_empty()
    }
    pub fn me(&self) -> Option<&Player> {
        let you = self.you?;
        self.game.player(you)
    }
    pub fn opponent(&self) -> Option<&Player> {
        let you = self.you;
        self.game.players.iter().find(|p| Some(p.id) != you)
    }
    pub fn is_me(&self, id: PlayerId) -> bool {
        self.you == Some(id)
    }
    pub fn card(&self, id: CardId) -> Option<&Card> {
        self.game.card(id).or_else(|| {
            self.game.stack.iter().filter_map(|s| s.source.as_ref()).find(|c| c.id == id)
        })
    }
    pub fn my_turn(&self) -> bool {
        self.you.is_some() && self.game.player_turn == self.you
    }
    pub fn phase(&self) -> Option<PhaseType> {
        self.game.phase
    }
}

/// Current non-blocking engine Input. Owner: view (sets `frame`); hud/interact set `awaiting`
/// when they send a click (optimistic button disable) and view clears it on the next input/repaint.
#[derive(Resource, Debug, Clone, Default)]
pub struct CurrentInput {
    pub frame: Option<InputFrame>,
    /// A click was sent and no new input/repaint has arrived yet.
    pub awaiting: bool,
}

impl CurrentInput {
    pub fn is_selectable(&self, id: CardId) -> bool {
        self.frame.as_ref().is_some_and(|f| f.selectable.contains(&id))
    }
    pub fn is_highlighted(&self, id: CardId) -> bool {
        self.frame.as_ref().is_some_and(|f| f.highlighted.contains(&id))
    }
    pub fn is_weak(&self, id: CardId) -> bool {
        self.frame.as_ref().is_some_and(|f| f.weak.contains(&id))
    }
    pub fn player_selectable(&self, id: PlayerId) -> bool {
        self.frame.as_ref().is_some_and(|f| f.selectable_players.contains(&id))
    }
}

/// The blocking prompt awaiting an answer (None once answered/cleared). Owner: view (set on
/// `prompt` frames; cleared on the next `state`/`input` with a different seq or when `prompts`
/// sends the answer — prompts may set it to None after answering).
#[derive(Resource, Debug, Clone, Default)]
pub struct PendingPrompt(pub Option<PromptFrame>);

/// Owner: view.
#[derive(Resource, Debug, Clone, Default)]
pub struct GameLog(pub Vec<LogEntry>);

/// Set on `gameOver`. Owner: view.
#[derive(Resource, Debug, Clone, Default)]
pub struct GameResult(pub Option<MatchOutcome>);

#[derive(Debug, Clone, Default)]
pub struct MatchOutcome {
    pub winner: Option<String>,
    pub you_won: bool,
}

/// Last known full view of every card id ever seen this match (so a card mid-cast / briefly in
/// no zone can still be drawn). Owner: view.
#[derive(Resource, Debug, Clone, Default)]
pub struct CardCache(pub HashMap<CardId, Card>);

/// Board changes derived by diffing consecutive `state` frames (plus engine cues). Owner: view.
/// Consumers: fx, audio, motion, hud.
#[derive(Message, Debug, Clone)]
pub enum BoardEvent {
    /// A card id appeared for the first time.
    CardAppeared { id: CardId, zone: ZoneKind, owner: Option<PlayerId> },
    CardMoved { id: CardId, from: ZoneKind, to: ZoneKind, owner: Option<PlayerId> },
    CardRemoved { id: CardId },
    CardTapped { id: CardId, tapped: bool },
    /// Marked damage on a permanent increased by `amount`.
    CardDamaged { id: CardId, amount: i32 },
    LifeChanged { player: PlayerId, old: i32, new: i32 },
    TurnStarted { player: PlayerId, turn: i32 },
    PhaseChanged { phase: PhaseType, player: Option<PlayerId> },
    StackPushed { item: i64, source: Option<CardId> },
    StackResolved { item: i64, source: Option<CardId> },
    AttackersDeclared { attackers: Vec<CardId> },
    /// The engine rejected the last action (`flash`); `card` = last card the user clicked, if any.
    Rejected { card: Option<CardId> },
    /// `alert` frame.
    Alert,
    /// Raw v2 animation cue.
    Cue(EventCue),
    GameOver { you_won: bool },
}

// =============================================================================================
// Cards & board (owner: cards / layout / motion unless noted)
// =============================================================================================

/// Card size in world units (63×88 mm ratio). Table units ≈ "card widths".
pub const CARD_W: f32 = 0.63;
pub const CARD_H: f32 = 0.88;
pub const CARD_THICKNESS: f32 = 0.004;

/// Render layer for the 3D table world (cards on the battlefield, piles, stack, effects).
pub const TABLE_LAYER: usize = 0;
/// Render layer for the screen-aligned hand overlay (hand cards, dragged card).
pub const HAND_LAYER: usize = 1;

/// Root entity of a card. Owner: cards. Children: a front quad (`CardFront`) and a back quad (`CardBack`).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CardEntity(pub CardId);

/// Front face mesh entity (child of `CardEntity`). Its material may be swapped by card_material.
#[derive(Component, Debug, Clone, Copy)]
pub struct CardFront;
/// Back face mesh entity (child of `CardEntity`).
#[derive(Component, Debug, Clone, Copy)]
pub struct CardBack;

/// Where the card currently is according to the engine. Owner: cards.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct CardLocation {
    pub zone: ZoneKind,
    pub owner: Option<PlayerId>,
    pub controller: Option<PlayerId>,
    /// true when controlled by the viewing player (bottom half of the table)
    pub mine: bool,
    /// Index within its zone list (engine order).
    pub index: usize,
    pub hidden: bool,
}

/// CardId → root entity. Owner: cards.
#[derive(Resource, Debug, Default)]
pub struct CardRegistry(pub HashMap<CardId, Entity>);

/// Rest pose computed by layout. Owner: layout. motion eases `Transform` toward
/// `TargetPlacement ∘ PoseOffset`.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct TargetPlacement {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: f32,
    /// Which camera draws it: TABLE_LAYER or HAND_LAYER.
    pub layer: usize,
    /// Face-up? (false = show the back)
    pub face_up: bool,
}

impl Default for TargetPlacement {
    fn default() -> Self {
        Self { translation: Vec3::ZERO, rotation: Quat::IDENTITY, scale: 1.0, layer: TABLE_LAYER, face_up: true }
    }
}

/// Additive pose tweak layered on top of the layout pose (hover lift, attack push, shake…).
/// Written by interact (hover), combat (attack push), fx (shake). Components are summed by
/// motion: translation added, scale multiplied, rotation pre-multiplied.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct PoseOffset {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: f32,
}

impl Default for PoseOffset {
    fn default() -> Self {
        Self { translation: Vec3::ZERO, rotation: Quat::IDENTITY, scale: 1.0 }
    }
}

/// While present, motion leaves the card's Transform alone (interact drives it directly).
#[derive(Component, Debug, Clone, Copy)]
pub struct Dragging;

/// Interaction flags mirrored from `CurrentInput` onto card entities. Owner: interact.
#[derive(Component, Debug, Clone, Copy)]
pub struct Selectable;
#[derive(Component, Debug, Clone, Copy)]
pub struct Highlighted;
#[derive(Component, Debug, Clone, Copy)]
pub struct WeakSelectable;
/// Pointer is over this card. Owner: interact.
#[derive(Component, Debug, Clone, Copy)]
pub struct Hovered;

/// Group of identical permanents (lands/tokens) rendered as one stack with an "xN" badge.
/// Owner: layout (on the top card of the group; removed when not grouped).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct StackGroup {
    pub count: usize,
}

/// Card shown in the large hover/inspect preview (right edge). Owner: interact.
#[derive(Resource, Debug, Clone, Default)]
pub struct HoverPreview(pub Option<CardId>);

/// Key points of the table in world space, for HUD anchoring, arrows and fx. Owner: layout.
#[derive(Resource, Debug, Clone, Default)]
pub struct BoardAnchors {
    pub my_avatar: Vec3,
    pub opp_avatar: Vec3,
    pub my_library: Vec3,
    pub opp_library: Vec3,
    pub my_graveyard: Vec3,
    pub opp_graveyard: Vec3,
    pub my_exile: Vec3,
    pub opp_exile: Vec3,
    pub my_command: Vec3,
    pub opp_command: Vec3,
    pub stack: Vec3,
    /// z of the line between the two halves of the battlefield
    pub center_z: f32,
}

impl BoardAnchors {
    pub fn avatar(&self, mine: bool) -> Vec3 {
        if mine { self.my_avatar } else { self.opp_avatar }
    }
}

/// Image cache handle requests. Owner: images. Anyone may call `request` (needs ResMut).
/// See images.rs for `CardImages::request(&Card, ImageVersion) -> Handle<Image>`.
pub use crate::images::CardImages;

/// Cameras. Owner: table (TableCamera), hand_camera (HandCamera).
#[derive(Component, Debug, Clone, Copy)]
pub struct TableCamera;
#[derive(Component, Debug, Clone, Copy)]
pub struct HandCamera;

/// Project a world point (seen by `camera`) to UI logical pixels; None if behind the camera.
pub fn world_to_ui(camera: &Camera, cam_tf: &GlobalTransform, world: Vec3) -> Option<Vec2> {
    camera.world_to_viewport(cam_tf, world).ok()
}

// =============================================================================================
// Settings & style (owner: settings / style)
// =============================================================================================

/// Persisted user settings. Owner: settings (load/save); menus edit fields.
#[derive(Resource, Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Settings {
    pub engine_url: String,
    pub autolaunch: bool,
    /// 1.0 = normal; >1 faster animations
    pub anim_speed: f32,
    pub master_volume: f32,
    pub sfx_volume: f32,
    pub music_volume: f32,
    pub fullscreen: bool,
    /// Battlefield theme name (see table/style)
    pub theme: String,
    pub player_name: String,
    pub last_format: Format,
    pub last_deck: HashMap<String, String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            engine_url: "http://127.0.0.1:8080".into(),
            autolaunch: true,
            anim_speed: 1.0,
            master_volume: 0.8,
            sfx_volume: 0.8,
            music_volume: 0.4,
            fullscreen: false,
            theme: "sanctum".into(),
            player_name: "You".into(),
            last_format: Format::Constructed,
            last_deck: HashMap::new(),
        }
    }
}

/// Fonts used by all UI. Owner: style. Falls back to Bevy's default font until loaded.
#[derive(Resource, Debug, Clone, Default)]
pub struct ArenaFonts {
    /// Display / card names / big numbers (serif, Beleren-like)
    pub title: Handle<Font>,
    /// Body text
    pub body: Handle<Font>,
    /// Bold numerals for life / P/T
    pub numbers: Handle<Font>,
}
