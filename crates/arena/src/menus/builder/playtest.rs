//! Sample-hand tester: shuffle the deck, look at an opening seven, London-mulligan, draw a few
//! turns. Statistics answer "how often", this answers "what does a hand actually look like" — the
//! quickest way to feel a curve or a mana base before playing a real game.

use bevy::prelude::*;
use mtg_client::deckstats::Rng;
use mtg_client::{ImageRequest, ImageVersion};
use mtg_protocol::CatalogCard;

use super::view::{card_tile, tile_size};
use super::{BuilderAction, BuilderDeck, BuilderUi};
use crate::images::CardImages;
use crate::shared::{AppState, ArenaFonts};
use crate::style::*;

#[derive(Debug, Clone)]
pub struct Playtest {
    library: Vec<CatalogCard>,
    pub hand: Vec<CatalogCard>,
    pub mulligans: u32,
    /// Cards drawn after the opening hand (turn number on the draw = draws).
    pub draws: u32,
    rng: Rng,
    /// Bumped on every change so the overlay rebuilds.
    pub rev: u64,
}

impl Playtest {
    pub fn new(deck: &BuilderDeck) -> Self {
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1);
        let mut p = Self { library: Vec::new(), hand: Vec::new(), mulligans: 0, draws: 0, rng: Rng::new(seed), rev: 0 };
        p.deal(deck, 0);
        p
    }

    fn deal(&mut self, deck: &BuilderDeck, mulligans: u32) {
        self.library = deck.main.iter().flat_map(|(c, q)| std::iter::repeat_n(c.clone(), *q as usize)).collect();
        self.rng.shuffle(&mut self.library);
        let n = 7.min(self.library.len());
        self.hand = self.library.drain(..n).collect();
        self.mulligans = mulligans;
        self.draws = 0;
        self.rev += 1;
    }

    pub fn lands_in_hand(&self) -> usize {
        self.hand.iter().filter(|c| c.primary_type() == mtg_protocol::CardKind::Land).count()
    }

    pub fn library_len(&self) -> usize {
        self.library.len()
    }
}

pub(super) fn act(ui: &mut BuilderUi, deck: &BuilderDeck, act: &BuilderAction) {
    match act {
        BuilderAction::PtClose => ui.playtest = None,
        BuilderAction::PtNewHand => ui.playtest = Some(Playtest::new(deck)),
        BuilderAction::PtMulligan => {
            if let Some(p) = &mut ui.playtest {
                // London mulligan: draw seven again; the bottomed cards are shown by the count only
                let m = p.mulligans + 1;
                p.deal(deck, m);
            }
        }
        BuilderAction::PtDraw => {
            if let Some(p) = &mut ui.playtest
                && !p.library.is_empty()
            {
                let c = p.library.remove(0);
                p.hand.push(c);
                p.draws += 1;
                p.rev += 1;
            }
        }
        _ => {}
    }
}

#[derive(Component)]
pub(super) struct PlaytestRoot(u64);

pub(super) fn sync_playtest(
    mut commands: Commands,
    ui: Res<BuilderUi>,
    fonts: Res<ArenaFonts>,
    mut images: ResMut<CardImages>,
    roots: Query<(Entity, &PlaytestRoot)>,
) {
    let want = ui.playtest.as_ref().map(|p| p.rev);
    let have = roots.iter().next().map(|(_, r)| r.0);
    if want == have {
        return;
    }
    for (e, _) in &roots {
        commands.entity(e).despawn();
    }
    let Some(p) = &ui.playtest else { return };
    let hand_size = 7u32.saturating_sub(p.mulligans);
    let keep_note = if p.mulligans > 0 {
        format!("Mulligan {} — keep {} (put {} on the bottom)", p.mulligans, hand_size, p.mulligans.min(7))
    } else {
        "Opening hand".to_string()
    };
    let summary = format!(
        "{keep_note}   ·   {} lands in hand   ·   {}   ·   {} cards in library",
        p.lands_in_hand(),
        if p.draws == 0 { "before the first draw".to_string() } else { format!("turn {} on the draw", p.draws) },
        p.library_len()
    );
    commands
        .spawn((
            PlaytestRoot(p.rev),
            DespawnOnExit(AppState::DeckBuilder),
            GlobalZIndex(80),
            Node { justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..fullscreen_root() },
            BackgroundColor(SCRIM),
        ))
        .with_children(|scrim| {
            scrim
                .spawn(ornate_panel(Node {
                    width: Val::Percent(90.0),
                    max_height: Val::Percent(92.0),
                    padding: UiRect::all(Val::Px(24.0)),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(12.0),
                    ..default()
                }))
                .with_children(|panel| {
                    panel.spawn(heading(&fonts, "PLAYTEST", 30.0));
                    panel.spawn(ui_text(&fonts.body, summary, 16.0, TEXT_DIM));
                    panel.spawn(divider());
                    panel
                        .spawn(Node {
                            flex_wrap: FlexWrap::Wrap,
                            justify_content: JustifyContent::Center,
                            column_gap: Val::Px(12.0),
                            row_gap: Val::Px(12.0),
                            overflow: Overflow::scroll_y(),
                            min_height: Val::Px(0.0),
                            ..default()
                        })
                        .with_children(|row| {
                            // seven across in a 1600px window
                            let size = tile_size(1.2);
                            for (i, c) in p.hand.iter().enumerate() {
                                let h = images.request_raw(ImageRequest { name: c.name.clone(), set: c.set.clone(), back: false, version: ImageVersion::Normal });
                                // cards drawn after the opener get a gold rim so turns read at a glance
                                let drawn = i >= 7;
                                card_tile(row, &fonts, c, h, size, None, drawn, false);
                            }
                        });
                    panel
                        .spawn(Node { justify_content: JustifyContent::FlexEnd, column_gap: Val::Px(12.0), margin: UiRect::top(Val::Px(6.0)), ..default() })
                        .with_children(|r| {
                            for (label, kind, act, w) in [
                                ("CLOSE", ButtonKind::Ghost, BuilderAction::PtClose, 140.0),
                                ("MULLIGAN", ButtonKind::Secondary, BuilderAction::PtMulligan, 180.0),
                                ("NEW HAND", ButtonKind::Secondary, BuilderAction::PtNewHand, 180.0),
                                ("DRAW", ButtonKind::Primary, BuilderAction::PtDraw, 180.0),
                            ] {
                                let b = spawn_button(r, &fonts, kind, label, Vec2::new(w, 48.0));
                                r.commands().entity(b).insert(act);
                            }
                        });
                });
        });
}
