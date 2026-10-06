//! Async, platform-neutral client for the engine's `/api/*` — what the Bevy app uses on desktop *and* in
//! the browser. Drive calls with [`crate::spawn`]; never `.await` them on Bevy's main schedule.
//!
//! [`crate::Rest`] (blocking, ureq) stays for native tools (autopilot, launcher); this type mirrors the
//! endpoints the game needs, on top of [`crate::http::request`].

use std::time::Duration;

use anyhow::{Context, Result};
use mtg_protocol::{
    CardQuery, CatalogCard, CreateEvent, CreateLobby, CreateMatch, Cube, CubeCard, CubeInfo, DeckBody, DeckFull, DeckInfo,
    EventFrame, Format, FormatsInfo, JoinLobby, LobbyFrame, MatchInfo, RecommendRequest, Recommendations, SeatTicket,
    ServerInfo, SetInfo,
};

use crate::http::{HttpResponse, query, request};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ImageVersion {
    /// ~488×680 full card
    #[default]
    Normal,
    /// ~672×936 full card (hover preview)
    Large,
    /// art only (battlefield frames)
    ArtCrop,
    /// full card, downscaled by the client to ~168 px wide after decoding (whole-cube image grids,
    /// where hundreds of full-size textures would not fit in memory); fetched as `normal`
    Thumb,
}

impl ImageVersion {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Normal | Self::Thumb => "normal",
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
pub struct Api {
    /// e.g. `http://127.0.0.1:8080` (in the browser: the page's origin)
    pub base: String,
}

impl Api {
    pub fn new(base: impl Into<String>) -> Self {
        Self { base: base.into().trim_end_matches('/').to_string() }
    }
    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }
    /// `ws://…/ws/match/{id}[?token=…]` for this server (`wss://` behind https).
    pub fn ws_url(&self, match_id: &str, token: Option<&str>) -> String {
        let mut u = format!("{}/ws/match/{match_id}", self.base.replacen("http", "ws", 1));
        if let Some(t) = token.filter(|t| !t.is_empty()) {
            u.push_str(&query(&[("token", t)]));
        }
        u
    }

    async fn get(&self, path: &str, q: &[(&str, &str)], secs: u64) -> Result<HttpResponse> {
        request("GET", &format!("{}{}", self.url(path), query(q)), None, Duration::from_secs(secs)).await
    }
    async fn post<B: serde::Serialize>(&self, path: &str, body: &B, secs: u64) -> Result<HttpResponse> {
        let json = serde_json::to_string(body).context("encode body")?;
        request("POST", &self.url(path), Some(json), Duration::from_secs(secs)).await
    }

    /// `GET /api/health` → true when the engine answers `{ok:true}`. Connection errors are `false`.
    pub async fn health(&self) -> bool {
        self.server_info().await.is_ok_and(|i| i.ok)
    }
    pub async fn server_info(&self) -> Result<ServerInfo> {
        self.get("/api/health", &[], 3).await?.ok("health")?.json("health")
    }
    /// Decks usable in `format` (decks that declare a different format are dropped client-side).
    pub async fn decks(&self, format: Format) -> Result<Vec<DeckInfo>> {
        self.decks_for(format, None).await
    }
    /// Decks for a format and Constructed legality; illegal ones come back with `problem` set.
    pub async fn decks_for(&self, format: Format, legality: Option<&str>) -> Result<Vec<DeckInfo>> {
        let mut q = vec![("format", format.as_str())];
        if let Some(l) = legality {
            q.push(("legality", l));
        }
        // a fresh engine indexes its deck library first (~15 s on a cold cache, longer on slow disks)
        let all: Vec<DeckInfo> = self.get("/api/decks", &q, 90).await?.ok("decks")?.json("decks")?;
        Ok(all.into_iter().filter(|d| d.format.is_none_or(|f| f == format)).collect())
    }
    pub async fn create_match(&self, req: &CreateMatch) -> Result<MatchInfo> {
        self.post("/api/match", req, 30).await?.ok("create match")?.json("matchInfo")
    }
    /// `token` = the seat token from [`Api::create_match`]; a public engine refuses the delete without it.
    pub async fn delete_match(&self, id: &str, token: Option<&str>) -> Result<()> {
        let q = token.map(|t| query(&[("token", t)])).unwrap_or_default();
        request("DELETE", &self.url(&format!("/api/match/{id}{q}")), None, Duration::from_secs(5)).await?.ok("delete match").map(|_| ())
    }
    /// Raw JPEG bytes from the engine's offline pack (`/api/img`); `Err` (404) when the pack has no image.
    pub async fn image_bytes(&self, req: &ImageRequest) -> Result<Vec<u8>> {
        let mut q = vec![("name", req.name.as_str()), ("version", req.version.as_str())];
        if let Some(set) = req.set.as_deref().filter(|s| !s.is_empty()) {
            q.push(("set", set));
        }
        if req.back {
            q.push(("back", "1"));
        }
        Ok(self.get("/api/img", &q, 30).await?.ok(&format!("image {}", req.name))?.body)
    }
    /// Import a pasted decklist; returns the engine's ImportResult JSON.
    pub async fn import_deck(&self, text: &str, format: Format) -> Result<serde_json::Value> {
        let body = serde_json::json!({ "text": text, "format": format.as_str() });
        self.post("/api/deck/import", &body, 60).await?.ok("import deck")?.json("import result")
    }
    /// Save a deck body (DeckBody JSON, see engine/docs/protocol.md).
    pub async fn save_deck(&self, body: &serde_json::Value, format: Format) -> Result<serde_json::Value> {
        let mut body = body.clone();
        if let Some(o) = body.as_object_mut() {
            o.entry("format").or_insert_with(|| format.as_str().into());
        }
        self.post("/api/deck", &body, 30).await?.ok("save deck")?.json("save result")
    }

    // ---- deck builder ------------------------------------------------------------------------

    /// `GET /api/cards/search` with every builder filter; empty `q` lists cards (popular first).
    pub async fn catalog_search(&self, q: &CardQuery) -> Result<Vec<CatalogCard>> {
        let limit = q.limit.max(1).to_string();
        let (cmc, offset) = (q.cmc.map(|c| c.to_string()), q.offset.to_string());
        let mut p = vec![("q", q.q.as_str()), ("limit", limit.as_str())];
        for (k, v) in [("ci", &q.ci), ("type", &q.type_line), ("text", &q.text), ("tag", &q.tag), ("sort", &q.sort)] {
            if let Some(v) = v.as_deref().filter(|v| !v.is_empty()) {
                p.push((k, v));
            }
        }
        if let Some(c) = &cmc {
            p.push(("cmc", c));
        }
        if q.commander_only {
            p.push(("commander", "1"));
        }
        if q.offset > 0 {
            p.push(("offset", &offset));
        }
        self.get("/api/cards/search", &p, 20).await?.ok("card search")?.json("card search")
    }
    /// `GET /api/deck?ref=` — a deck with full card details.
    pub async fn deck(&self, deck_ref: &str, format: Format) -> Result<DeckFull> {
        self.get("/api/deck", &[("ref", deck_ref), ("format", format.as_str())], 20).await?.ok("load deck")?.json("deck")
    }
    /// `POST /api/deck` with a typed body; returns the saved deck.
    pub async fn save_deck_body(&self, body: &DeckBody) -> Result<DeckFull> {
        self.post("/api/deck", body, 30).await?.ok("save deck")?.json("saved deck")
    }
    /// `POST /api/deck/validate` → the engine's conformance problem, None when legal.
    pub async fn validate_deck(&self, body: &DeckBody) -> Result<Option<String>> {
        let v: serde_json::Value = self.post("/api/deck/validate", body, 30).await?.ok("validate deck")?.json("validate")?;
        Ok(v.get("problem").and_then(|p| p.as_str()).map(str::to_owned))
    }
    /// `POST /api/deck/recommend` — EDHREC-style suggestions for a deck in progress.
    pub async fn recommend(&self, req: &RecommendRequest) -> Result<Recommendations> {
        self.post("/api/deck/recommend", req, 60).await?.ok("recommend")?.json("recommendations")
    }

    // ---- multiplayer --------------------------------------------------------------------------

    /// Open a table; the answer is the host's seat-0 ticket (with the invite link).
    pub async fn create_lobby(&self, req: &CreateLobby) -> Result<SeatTicket> {
        self.post("/api/lobby", req, 30).await?.ok("create lobby")?.json("lobby ticket")
    }
    /// Public lobby info (format, seats) — what an invitee needs before picking a deck.
    pub async fn lobby(&self, code: &str) -> Result<LobbyFrame> {
        self.get(&format!("/api/lobby/{}", code.trim()), &[], 10).await?.ok("lobby")?.json("lobby")
    }
    pub async fn join_lobby(&self, code: &str, req: &JoinLobby) -> Result<SeatTicket> {
        self.post(&format!("/api/lobby/{}/join", code.trim()), req, 30).await?.ok("join")?.json("seat ticket")
    }

    // ---- v3: formats, cubes, sets, events --------------------------------------------------------

    pub async fn formats(&self) -> Result<FormatsInfo> {
        self.get("/api/formats", &[], 10).await?.ok("formats")?.json("formats")
    }
    pub async fn cubes(&self) -> Result<Vec<CubeInfo>> {
        self.get("/api/cubes", &[], 60).await?.ok("cubes")?.json("cubes")
    }
    pub async fn cube(&self, cube_ref: &str) -> Result<Cube> {
        self.get("/api/cube", &[("ref", cube_ref)], 60).await?.ok("cube")?.json("cube")
    }
    pub async fn save_cube(&self, cube: &Cube) -> Result<Cube> {
        self.post("/api/cube", cube, 60).await?.ok("save cube")?.json("cube")
    }
    pub async fn delete_cube(&self, name: &str) -> Result<()> {
        let url = format!("{}{}", self.url("/api/cube"), query(&[("name", name)]));
        request("DELETE", &url, None, Duration::from_secs(10)).await?.ok("delete cube").map(|_| ())
    }
    /// Pasted list → `{cards: [CubeCard], unresolved: [{line, suggestions}]}`.
    pub async fn import_cube(&self, text: &str) -> Result<serde_json::Value> {
        self.post("/api/cube/import", &serde_json::json!({ "text": text }), 60).await?.ok("import cube")?.json("import")
    }
    /// Catalog search (name/type/text), for the cube designer.
    pub async fn search_cards(&self, q: &str, limit: u32) -> Result<Vec<CubeCard>> {
        let limit = limit.to_string();
        self.get("/api/cards/search", &[("q", q), ("limit", &limit)], 20).await?.ok("card search")?.json("cards")
    }
    pub async fn sets(&self) -> Result<Vec<SetInfo>> {
        self.get("/api/sets", &[], 20).await?.ok("sets")?.json("sets")
    }
    /// Open a pod; the host's seat ticket (with `invite`).
    pub async fn create_event(&self, req: &CreateEvent) -> Result<SeatTicket> {
        self.post("/api/event", req, 60).await?.ok("create event")?.json("event ticket")
    }
    pub async fn event(&self, code: &str) -> Result<EventFrame> {
        self.get(&format!("/api/event/{}", code.trim()), &[], 10).await?.ok("event")?.json("event")
    }
    pub async fn join_event(&self, code: &str, name: &str, avatar: Option<&mtg_protocol::Avatar>) -> Result<SeatTicket> {
        self.post(&format!("/api/event/{}/join", code.trim()), &serde_json::json!({ "name": name, "avatar": avatar }), 30).await?.ok("join event")?.json("seat ticket")
    }
    /// `ws://…/ws/event/{id}?token=…`
    pub fn event_ws_url(&self, event_id: &str, token: &str) -> String {
        format!("{}/ws/event/{event_id}{}", self.base.replacen("http", "ws", 1), query(&[("token", token)]))
    }
}

/// An invite pasted by a player: a full link (`http://host:8080/?join=ABC123`) or just the code.
/// Returns `(engine base url if the link names one, lobby code)`.
pub fn parse_invite(text: &str) -> Option<(Option<String>, String)> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    if let Some(pos) = t.find("join=").or_else(|| t.find("event=").map(|p| p + 1)) {
        let code: String = t[pos + 5..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
        if code.is_empty() {
            return None;
        }
        let base = t.find("://").map(|s| {
            let rest = &t[s + 3..];
            let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
            format!("{}{}", &t[..s + 3], &rest[..end])
        });
        return Some((base, code.to_ascii_uppercase()));
    }
    t.chars().all(|c| c.is_ascii_alphanumeric() || c == '-').then(|| (None, t.to_ascii_uppercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invite_links_and_codes() {
        assert_eq!(
            parse_invite("http://192.168.1.5:8080/?join=ab12cd"),
            Some((Some("http://192.168.1.5:8080".into()), "AB12CD".into()))
        );
        assert_eq!(parse_invite("  XYZ234 "), Some((None, "XYZ234".into())));
        assert_eq!(parse_invite("https://t.example/?x=1&join=Q2W3E4#frag"), Some((Some("https://t.example".into()), "Q2W3E4".into())));
        assert_eq!(parse_invite("hello world"), None);
        assert_eq!(parse_invite(""), None);
    }

    #[test]
    fn ws_url_carries_token() {
        let a = Api::new("http://h:1/");
        assert_eq!(a.ws_url("M1", None), "ws://h:1/ws/match/M1");
        assert_eq!(a.ws_url("M1", Some("ab")), "ws://h:1/ws/match/M1?token=ab");
        assert_eq!(Api::new("https://x").ws_url("M", None), "wss://x/ws/match/M");
    }
}
