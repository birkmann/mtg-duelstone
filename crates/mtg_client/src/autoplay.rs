//! A playing policy for the human seat, shared by the `autopilot` soak bot and the arena app's
//! `--autoplay` debug flag. It should make *interesting* boards (play lands, cast spells, attack),
//! not just pass, while never stalling: every input/prompt gets an answer.
//!
//! Driving contract (both callers follow it):
//! - call [`AutoPolicy::on_input`] whenever an `input` frame arrives (it de-duplicates repaints of
//!   an input it already acted on, so calling it extra times is harmless);
//! - call [`AutoPolicy::on_prompt`] for every `prompt` frame;
//! - call [`AutoPolicy::on_flash`] when the engine answers `flash` (it rejected our click) and then
//!   `on_input` again with the last input, and [`AutoPolicy::on_stall`] + `on_input` if no new input
//!   arrived a few seconds after we replied.
//!
//! The engine never lists hand cards in `input.selectable` at priority — legality is decided by the
//! engine, which rejects with `flash`. So we *try* plausible cards (cheap affordability filter, at
//! most twice per card per turn) instead of waiting for `selectable`.

use std::collections::HashMap;

use mtg_protocol::{Card, CardId, ChoiceOption, ClientFrame, Game, InputFrame, Player, PlayerId, PromptBody, PromptFrame};
use serde_json::{Value, json};

/// After this many answers to the same input `seq` without the seq changing we fall back to OK/cancel.
const SAME_SEQ_LIMIT: u32 = 4;

#[derive(Debug, Clone, Default)]
pub struct AutoPolicy {
    pub seed: u64,
    rng: u64,
    /// Fingerprint of the last input we acted on (suppresses duplicate repaints).
    acted_key: Option<u64>,
    seq: i64,
    seq_answers: u32,
    /// Escalation set by `on_stall`/`on_flash`: forces the OK/cancel fallback.
    force_fallback: bool,
    turn: i32,
    /// Cards we clicked this turn → click count.
    tried: HashMap<CardId, u8>,
    last_card_click: Option<CardId>,
    /// Card we most recently tried to play from priority; cancelling its cost payment blacklists it for the turn.
    last_play: Option<CardId>,
    /// Turn in which we already sent alphaStrike / finished choosing attackers.
    attack_turn: Option<i32>,
    blocks_made: u32,
    /// The engine said our attack declaration was invalid (a creature must attack): attack with everything.
    declaration_invalid: u32,
    /// Declare attackers/blockers inputs seen this turn: a legal declaration needs ≤ ~3, more means
    /// the engine keeps rejecting us even if the caller never forwards `message` frames.
    declaration_inputs: u32,
}

impl AutoPolicy {
    pub fn new(seed: u64) -> Self {
        Self { seed, ..Default::default() }
    }

    fn next(&mut self) -> u64 {
        if self.rng == 0 {
            self.rng = self.seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03;
            if self.rng == 0 {
                self.rng = 0x1234_5678_9ABC_DEF1;
            }
        }
        // xorshift64*
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn chance(&mut self, pct: u64) -> bool {
        self.next() % 100 < pct
    }

    /// The engine rejected our last action: allow a retry of the same input, never with that card.
    pub fn on_flash(&mut self) {
        if let Some(c) = self.last_card_click.take() {
            self.tried.insert(c, 9);
        }
        self.acted_key = None;
    }

    /// A `message` frame arrived. "Attack declaration invalid" means passing with no attackers was
    /// illegal (e.g. "attacks each combat if able"); the engine then re-asks with a fresh `seq`, so
    /// without this we would answer OK forever.
    pub fn on_message(&mut self, text: &str) {
        let t = text.to_lowercase();
        if t.contains("declaration invalid") || t.contains("cannot be blocked with") || t.contains("must be blocked") {
            self.declaration_invalid += 1;
            self.acted_key = None;
        }
    }

    /// Nothing happened after our reply: act again on the same input, escalating to OK/cancel.
    pub fn on_stall(&mut self) {
        self.acted_key = None;
        self.force_fallback = true;
    }

    /// Reply to the current non-blocking input, or None to wait (e.g. waiting for the opponent).
    pub fn on_input(&mut self, input: &InputFrame, game: &Game, you: Option<PlayerId>) -> Option<ClientFrame> {
        if !input.ok.enabled && !input.cancel.enabled && input.selectable.is_empty() && input.selectable_players.is_empty() {
            return None; // engine is busy / waiting on the opponent
        }
        let key = fingerprint(input, find_me(game, you));
        if self.acted_key == Some(key) {
            return None;
        }
        if game.turn != self.turn {
            self.turn = game.turn;
            self.tried.clear();
            self.attack_turn = None;
            self.blocks_made = 0;
            self.declaration_invalid = 0;
            self.declaration_inputs = 0;
        }
        if input.seq != self.seq {
            self.seq = input.seq;
            self.seq_answers = 0;
            self.force_fallback = false;
        }
        let frame = self.decide_input(input, game, you);
        if matches!(frame, ClientFrame::Click { action: mtg_protocol::ClickAction::Cancel, .. })
            && let Some(c) = self.last_play
        {
            self.tried.insert(c, 9); // couldn't pay for it (colours): don't try again this turn
        }
        self.seq_answers += 1;
        self.acted_key = Some(key);
        if let ClientFrame::Click { action: mtg_protocol::ClickAction::Card, id: Some(id) } = &frame {
            self.last_card_click = Some(*id);
        } else {
            self.last_card_click = None;
        }
        Some(frame)
    }

    fn decide_input(&mut self, input: &InputFrame, game: &Game, you: Option<PlayerId>) -> ClientFrame {
        if self.force_fallback || self.seq_answers >= SAME_SEQ_LIMIT {
            // Alternate OK / cancel so a stuck input eventually resolves one way or the other.
            let flip = (self.seq_answers + u32::from(self.force_fallback)).is_multiple_of(2);
            if input.cancel.enabled && (flip || !input.ok.enabled) {
                return ClientFrame::cancel();
            }
            if input.ok.enabled {
                return ClientFrame::ok();
            }
        }
        let me = find_me(game, you);
        let msg = input.message.as_str();
        let is_priority = msg.starts_with("Priority:");

        if is_priority {
            if let Some(me) = me
                && input.ok.enabled
                && is_my_main_empty_stack(msg, me)
                && let Some(id) = self.pick_play(me)
            {
                *self.tried.entry(id).or_default() += 1;
                self.last_play = Some(id);
                return ClientFrame::click_card(id);
            }
            self.last_play = None;
            return fallback(input, None);
        }

        // Declare attackers / blockers. `selectable` is empty for both (the engine validates clicks and
        // answers `flash`), and the message is the only discriminator.
        if let Some(me) = me {
            if msg.starts_with("Select creatures to attack") || msg.starts_with("Select creatures to block") {
                self.declaration_inputs += 1;
                if self.declaration_inputs == 6 && self.declaration_invalid == 0 {
                    self.declaration_invalid = 1;
                }
            }
            if msg.starts_with("Select creatures to attack") {
                return self.decide_attack(input, game, me);
            }
            if msg.starts_with("Select creatures to block") {
                return self.decide_block(input, game, me);
            }
        }

        // Mana payment / confirmations / mulligan etc: just accept ("Auto", "Keep", "Play", "OK").
        if input.ok.enabled {
            return ClientFrame::ok();
        }
        // Targeting / selecting: prefer an opponent creature, then an opponent player, then anything.
        if let Some(c) = self.pick_target(input, game, me) {
            return c;
        }
        fallback(input, me)
    }

    /// Land first (one per turn), then the most expensive affordable spell.
    fn pick_play(&mut self, me: &Player) -> Option<CardId> {
        let max_lands = me.max_land_plays.max(1);
        let tried = |s: &Self, c: &Card| s.tried.get(&c.id).copied().unwrap_or(0) >= 2;
        let can_land = me.lands_played_this_turn < max_lands;
        let lands: Vec<&Card> = me.zones.hand.iter().filter(|c| is_land(c) && !tried(self, c)).collect();
        if can_land && !lands.is_empty() {
            let i = (self.next() as usize) % lands.len();
            return Some(lands[i].id);
        }
        let mana = available_mana(me);
        let mut spells: Vec<(i32, &Card)> = me
            .zones
            .hand
            .iter()
            .chain(me.zones.command.iter())
            .filter(|c| !is_land(c) && !c.hidden && !tried(self, c))
            .map(|c| (cmc(c), c))
            .filter(|(v, _)| *v <= mana)
            .collect();
        // Most expensive first (bigger boards), random tie-break.
        let jitter: Vec<u64> = spells.iter().map(|_| self.next() % 100).collect();
        let mut keyed: Vec<(i32, u64, &Card)> = spells.drain(..).zip(jitter).map(|((v, c), j)| (v, j, c)).collect();
        keyed.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
        keyed.first().map(|(_, _, c)| c.id)
    }

    fn decide_attack(&mut self, input: &InputFrame, game: &Game, me: &Player) -> ClientFrame {
        if self.declaration_invalid > 0 {
            // OK with this selection was rejected. Alpha strike satisfies every "must attack" requirement;
            // if that was rejected too, click creatures one by one, then give up to the OK fallback.
            let n = self.declaration_invalid;
            self.declaration_invalid += 1;
            if n <= 2 && input.cancel.enabled {
                return ClientFrame::Click { action: mtg_protocol::ClickAction::AlphaStrike, id: None };
            }
            if let Some(c) = me.zones.battlefield.iter().find(|c| is_creature(c) && !c.attacking && !c.tapped && self.tried.get(&c.id).copied().unwrap_or(0) < 3) {
                let id = c.id;
                *self.tried.entry(id).or_default() += 1;
                return ClientFrame::click_card(id);
            }
            return ClientFrame::ok();
        }
        let already = me.zones.battlefield.iter().any(|c| c.attacking);
        if already || self.attack_turn == Some(game.turn) {
            self.attack_turn = Some(game.turn);
            return ClientFrame::ok();
        }
        let opp_untapped: Vec<&Card> = game
            .players
            .iter()
            .filter(|p| p.id != me.id)
            .flat_map(|p| p.zones.battlefield.iter())
            .filter(|c| is_creature(c) && !c.tapped)
            .collect();
        let max_opp_power = opp_untapped.iter().map(|c| power(c)).max().unwrap_or(0);
        let ready = |c: &&Card| is_creature(c) && !c.sick && !c.tapped && !c.attacking && self.tried.get(&c.id).copied().unwrap_or(0) < 2;
        let candidates: Vec<&Card> = me.zones.battlefield.iter().filter(ready).collect();
        if candidates.is_empty() {
            self.attack_turn = Some(game.turn);
            return ClientFrame::ok();
        }
        if input.cancel.enabled && (opp_untapped.is_empty() || self.chance(20)) {
            self.attack_turn = Some(game.turn);
            return ClientFrame::Click { action: mtg_protocol::ClickAction::AlphaStrike, id: None };
        }
        // Otherwise send only creatures that survive any single blocker.
        if let Some(c) = candidates.iter().find(|c| toughness(c) > max_opp_power) {
            let id = c.id;
            *self.tried.entry(id).or_default() += 1;
            return ClientFrame::click_card(id);
        }
        self.attack_turn = Some(game.turn);
        ClientFrame::ok()
    }

    /// `highlighted[0]` is the attacker being blocked; clicking one of our creatures blocks it.
    fn decide_block(&mut self, input: &InputFrame, game: &Game, me: &Player) -> ClientFrame {
        if self.declaration_invalid > 0 {
            // The engine rejected our blocks (e.g. menace needs two blockers): take the blocks back
            // by clicking each blocker again, then OK with none.
            self.declaration_invalid += 1;
            if let Some(c) = me.zones.battlefield.iter().find(|c| c.blocking && self.tried.get(&c.id).copied().unwrap_or(0) < 6) {
                let id = c.id;
                *self.tried.entry(id).or_default() += 3;
                return ClientFrame::click_card(id);
            }
            self.blocks_made = 2;
            return ClientFrame::ok();
        }
        if me.zones.battlefield.iter().any(|c| c.blocking) || self.blocks_made >= 2 {
            return ClientFrame::ok();
        }
        let attacker_power = input.highlighted.first().and_then(|id| game.card(*id)).map(power).unwrap_or(0);
        let tried = self.tried.clone();
        let ready = |c: &&Card| is_creature(c) && !c.tapped && !c.blocking && tried.get(&c.id).copied().unwrap_or(0) < 2;
        let mut pick = me.zones.battlefield.iter().filter(ready).find(|c| toughness(c) > attacker_power).map(|c| c.id);
        if pick.is_none() && self.chance(15) {
            pick = me.zones.battlefield.iter().filter(ready).map(|c| c.id).next();
        }
        match pick {
            Some(id) => {
                *self.tried.entry(id).or_default() += 1;
                self.blocks_made += 1;
                ClientFrame::click_card(id)
            }
            _ => ClientFrame::ok(),
        }
    }

    fn pick_target(&mut self, input: &InputFrame, game: &Game, me: Option<&Player>) -> Option<ClientFrame> {
        let pending: Vec<CardId> = input.selectable.iter().copied().filter(|id| !input.highlighted.contains(id)).collect();
        let my_id = me.map(|m| m.id);
        let opp_creature = pending.iter().copied().find(|id| {
            game.players
                .iter()
                .filter(|p| Some(p.id) != my_id)
                .any(|p| p.zones.battlefield.iter().any(|c| c.id == *id && is_creature(c)))
        });
        if let Some(id) = opp_creature {
            return Some(ClientFrame::click_card(id));
        }
        if let Some(pid) = input.selectable_players.iter().copied().find(|p| Some(*p) != my_id) {
            return Some(ClientFrame::click_player(pid));
        }
        if let Some(id) = pending.first() {
            return Some(ClientFrame::click_card(*id));
        }
        input.selectable_players.first().map(|p| ClientFrame::click_player(*p))
    }

    /// Reply to a blocking prompt (always answers).
    pub fn on_prompt(&mut self, prompt: &PromptFrame, game: &Game, you: Option<PlayerId>) -> ClientFrame {
        let _ = (game, you);
        ClientFrame::answer(prompt.id, self.prompt_value(&prompt.body))
    }

    /// The raw answer value for a prompt body (exposed for tests).
    pub fn prompt_value(&mut self, body: &PromptBody) -> Value {
        match body {
            PromptBody::ChooseAbility { options, .. } => options
                .iter()
                .find(|o| o.ability.as_ref().is_none_or(|a| a.can_play))
                .map(|o| json!(o.i))
                .unwrap_or(Value::Null),
            PromptBody::Confirm { default_yes, .. } => json!(*default_yes),
            PromptBody::Option { default, .. } => json!(*default),
            PromptBody::Text { initial, options, numeric } => {
                let v = initial.clone().filter(|s| !s.is_empty()).or_else(|| options.first().cloned()).unwrap_or_else(|| if *numeric { "1".into() } else { String::new() });
                json!(v)
            }
            PromptBody::Choices { min, max, options, .. } | PromptBody::ChooseEntity { min, max, options, .. } => json!(first_n(*min, *max, options)),
            PromptBody::Order { remain_min, options, .. } => {
                // Everything in the given order, except what must stay behind (soak-tested port of ws-autopilot).
                let keep = (*remain_min).max(0) as usize;
                let order: Vec<i64> = options.iter().skip(keep.min(options.len())).map(|o| o.i).collect();
                json!({ "order": order, "remember": false })
            }
            PromptBody::AssignCombatDamage { blockers, damage, defender, .. } => assign_combat_damage(blockers, *damage, defender.is_some()),
            PromptBody::AssignAmount { amount, at_least_one, options, .. } => assign_amount(*amount, *at_least_one, options),
            PromptBody::ManipulateCards { cards, .. } => json!(cards.iter().map(|c| c.id).collect::<Vec<_>>()),
            PromptBody::Unknown => Value::Null,
        }
    }
}

/// Input content plus our attacking/blocking creature ids: declaring attackers/blockers re-sends the
/// same input text while the board changes, and each such change deserves its own decision.
fn fingerprint(i: &InputFrame, me: Option<&Player>) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    if let Some(me) = me {
        me.zones.battlefield.iter().filter(|c| c.attacking || c.blocking).map(|c| c.id).collect::<Vec<_>>().hash(&mut h);
    }
    (i.seq, &i.message, &i.ok.label, i.ok.enabled, &i.cancel.label, i.cancel.enabled, &i.selectable, &i.highlighted, &i.selectable_players, i.select_min, i.select_max).hash(&mut h);
    h.finish()
}

fn find_me(game: &Game, you: Option<PlayerId>) -> Option<&Player> {
    you.and_then(|id| game.player(id)).or_else(|| game.players.iter().find(|p| p.is_viewer))
}

/// "Priority: Tester\nTurn: 3 (Tester)\nPhase: Main phase, precombat\nStack: Empty"
fn is_my_main_empty_stack(msg: &str, me: &Player) -> bool {
    msg.contains("Main phase") && msg.contains(&format!("({})", me.name)) && msg.contains("Stack: Empty")
}

fn fallback(input: &InputFrame, me: Option<&Player>) -> ClientFrame {
    let _ = me;
    if input.ok.enabled {
        ClientFrame::ok()
    } else if input.cancel.enabled {
        ClientFrame::cancel()
    } else if let Some(id) = input.selectable.iter().find(|id| !input.highlighted.contains(id)) {
        ClientFrame::click_card(*id)
    } else if let Some(p) = input.selectable_players.first() {
        ClientFrame::click_player(*p)
    } else {
        ClientFrame::ok()
    }
}

fn is_land(c: &Card) -> bool {
    c.state.as_ref().is_some_and(|s| s.is_land)
}
fn is_creature(c: &Card) -> bool {
    c.state.as_ref().is_some_and(|s| s.is_creature)
}
fn power(c: &Card) -> i32 {
    c.state.as_ref().and_then(|s| s.power).unwrap_or(0)
}
fn toughness(c: &Card) -> i32 {
    c.state.as_ref().and_then(|s| s.toughness).unwrap_or(0)
}

/// Converted mana cost from a `{2}{G}`-style string (X = 0, hybrid/phyrexian = 1 each).
pub fn cmc(c: &Card) -> i32 {
    let Some(cost) = c.state.as_ref().and_then(|s| s.mana_cost.as_deref()) else { return 0 };
    cost.split('}')
        .filter_map(|p| p.strip_prefix('{'))
        .map(|sym| match sym.parse::<i32>() {
            Ok(n) => n,
            Err(_) if sym == "X" => 0,
            Err(_) => 1,
        })
        .sum()
}

/// Untapped lands plus floating mana: a cheap "can I plausibly cast this" bound.
fn available_mana(me: &Player) -> i32 {
    let lands = me.zones.battlefield.iter().filter(|c| is_land(c) && !c.tapped).count() as i32;
    lands + me.mana_pool.values().sum::<i32>()
}

fn first_n(min: i64, max: i64, options: &[ChoiceOption]) -> Vec<i64> {
    if min < 0 || max < 0 {
        return Vec::new(); // informational list
    }
    let n = (min.max(max.min(1)) as usize).min(options.len());
    options.iter().take(n).map(|o| o.i).collect()
}

fn assign_combat_damage(blockers: &[Card], damage: i64, has_defender: bool) -> Value {
    let mut out = serde_json::Map::new();
    let mut left = damage;
    for b in blockers {
        if left <= 0 {
            break;
        }
        let lethal = i64::from((toughness(b) - b.damage).max(1));
        let give = lethal.min(left);
        out.insert(b.id.to_string(), json!(give));
        left -= give;
    }
    if left > 0 {
        if has_defender || blockers.is_empty() {
            out.insert("defender".into(), json!(left));
        } else if let Some(last) = blockers.last() {
            let e = out.entry(last.id.to_string()).or_insert(json!(0));
            *e = json!(e.as_i64().unwrap_or(0) + left);
        }
    }
    Value::Object(out)
}

fn assign_amount(amount: i64, at_least_one: bool, options: &[ChoiceOption]) -> Value {
    let mut out = serde_json::Map::new();
    if options.is_empty() || amount <= 0 {
        return Value::Object(out);
    }
    let mut left = amount;
    if at_least_one {
        for o in options.iter().take(amount as usize) {
            out.insert(o.i.to_string(), json!(1));
            left -= 1;
        }
    }
    if left > 0 {
        let first = options[0].i.to_string();
        let e = out.entry(first).or_insert(json!(0));
        *e = json!(e.as_i64().unwrap_or(0) + left);
    }
    Value::Object(out)
}

