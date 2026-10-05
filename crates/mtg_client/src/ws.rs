//! WebSocket link: a dedicated thread owns a `tungstenite` socket and talks to the caller over
//! crossbeam channels. Auto-reconnects (1.5 s) and sends `resync` after a reconnect; pings every 10 s.

use std::path::Path;

use crossbeam_channel::{Receiver, Sender};
use mtg_protocol::{ClientFrame, ServerFrame};

#[derive(Debug, Clone)]
pub enum LinkEvent {
    Connected,
    Disconnected(String),
    Frame(ServerFrame),
}

pub struct EngineLink {
    pub tx: Sender<ClientFrame>,
    pub rx: Receiver<LinkEvent>,
}

impl EngineLink {
    /// Connect to `ws://host:port/ws/match/{id}`. If `record` is set, every inbound frame's raw
    /// JSON is appended as one line to that `.jsonl` file.
    pub fn connect(_ws_url: &str, _record: Option<&Path>) -> Self {
        todo!("implemented by the mtg_client work package")
    }

    /// Feed a recorded `.jsonl` back as `LinkEvent`s (no engine). `speed` scales the original
    /// pacing (if timestamps were recorded); outbound commands are accepted and dropped.
    pub fn replay(_path: &Path, _speed: f32) -> anyhow::Result<Self> {
        todo!("implemented by the mtg_client work package")
    }

    pub fn send(&self, frame: ClientFrame) {
        let _ = self.tx.send(frame);
    }
}
