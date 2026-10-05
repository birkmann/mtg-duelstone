//! Card image cache. OWNER: board-core work package.
//!
//! CONTRACT: `CardImages::request(&card, version)` returns a `Handle<Image>` immediately (a
//! reserved handle that is filled when the bytes arrive from the engine's Scryfall proxy);
//! `CardImages::card_back` is always valid.

use std::collections::HashMap;

use bevy::prelude::*;
use mtg_client::{ImageRequest, ImageVersion};
use mtg_protocol::Card;

pub struct ImagesPlugin;

impl Plugin for ImagesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CardImages>();
    }
}

#[derive(Resource, Default)]
pub struct CardImages {
    pub card_back: Handle<Image>,
    pub handles: HashMap<ImageRequest, Handle<Image>>,
    /// Requests not yet started.
    pub queue: Vec<ImageRequest>,
}

impl CardImages {
    /// Handle for this card's face image (placeholder until loaded). Hidden cards → card back.
    pub fn request(&mut self, card: &Card, version: ImageVersion) -> Handle<Image> {
        let Some(state) = card.state.as_ref().filter(|_| !card.hidden) else {
            return self.card_back.clone();
        };
        let req = ImageRequest {
            name: if card.oracle_name.is_empty() { state.name.clone() } else { card.oracle_name.clone() },
            set: state.set_code.clone(),
            back: false,
            version,
        };
        self.request_raw(req)
    }

    pub fn request_raw(&mut self, req: ImageRequest) -> Handle<Image> {
        if let Some(h) = self.handles.get(&req) {
            return h.clone();
        }
        self.queue.push(req.clone());
        let h = Handle::default();
        self.handles.insert(req, h.clone());
        h
    }
}
