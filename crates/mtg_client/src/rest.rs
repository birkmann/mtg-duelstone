//! Blocking REST client for the engine's `/api/*` (ureq). Never call from Bevy's main schedule.

use mtg_protocol::{CreateMatch, DeckInfo, Format, MatchInfo};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ImageVersion {
    /// ~488×680 full card
    #[default]
    Normal,
    /// ~672×936 full card (hover preview)
    Large,
    /// art only (Arena-style battlefield frames)
    ArtCrop,
}

impl ImageVersion {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Large => "large",
            Self::ArtCrop => "art_crop",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ImageRequest {
    pub name: String,
    pub set: Option<String>,
    /// back face of a double-faced card
    pub back: bool,
    pub version: ImageVersion,
}

#[derive(Debug, Clone)]
pub struct Rest {
    /// e.g. `http://127.0.0.1:8080`
    pub base: String,
}

impl Rest {
    pub fn new(base: impl Into<String>) -> Self {
        Self { base: base.into().trim_end_matches('/').to_string() }
    }
    /// `ws://…/ws/match/{id}` for this server.
    pub fn ws_url(&self, match_id: &str) -> String {
        format!("{}/ws/match/{match_id}", self.base.replacen("http", "ws", 1))
    }
    pub fn health(&self) -> anyhow::Result<bool> {
        todo!()
    }
    pub fn decks(&self, _format: Format) -> anyhow::Result<Vec<DeckInfo>> {
        todo!()
    }
    pub fn create_match(&self, _req: &CreateMatch) -> anyhow::Result<MatchInfo> {
        todo!()
    }
    pub fn delete_match(&self, _id: &str) -> anyhow::Result<()> {
        todo!()
    }
    /// Raw image bytes (jpeg/png) via the engine's Scryfall proxy `/api/img`.
    pub fn image_bytes(&self, _req: &ImageRequest) -> anyhow::Result<Vec<u8>> {
        todo!()
    }
    /// Import a pasted decklist; returns the engine's ImportResult JSON.
    pub fn import_deck(&self, _text: &str, _format: Format) -> anyhow::Result<serde_json::Value> {
        todo!()
    }
    /// Save a deck body (DeckBody JSON, see engine/docs/protocol.md).
    pub fn save_deck(&self, _body: &serde_json::Value, _format: Format) -> anyhow::Result<serde_json::Value> {
        todo!()
    }
}
