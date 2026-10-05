//! A playing policy for the human seat, shared by the `autopilot` soak bot and the arena app's
//! `--autoplay` debug flag. It should make *interesting* boards (play lands, cast spells, attack),
//! not just pass, while never stalling: every input/prompt gets an answer.

use mtg_protocol::{ClientFrame, Game, InputFrame, PlayerId, PromptFrame};

#[derive(Debug, Clone, Default)]
pub struct AutoPolicy {
    pub seed: u64,
}

impl AutoPolicy {
    /// Reply to the current non-blocking input, or None to wait (e.g. waiting for the opponent).
    pub fn on_input(&mut self, _input: &InputFrame, _game: &Game, _you: Option<PlayerId>) -> Option<ClientFrame> {
        None
    }
    /// Reply to a blocking prompt (always answers).
    pub fn on_prompt(&mut self, prompt: &PromptFrame, _game: &Game, _you: Option<PlayerId>) -> ClientFrame {
        ClientFrame::answer(prompt.id, serde_json::Value::Null)
    }
}
