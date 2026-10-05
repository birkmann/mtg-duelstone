//! Blocking REST client for the engine's `/api/*` (ureq). Never call from Bevy's main schedule.

use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use mtg_protocol::{CardQuery, CatalogCard, CreateMatch, DeckBody, DeckFull, DeckInfo, Format, MatchInfo, RecommendRequest, Recommendations};
use ureq::Agent;

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

fn agent(timeout: Duration) -> Agent {
    // Non-2xx statuses are inspected by hand so the error can carry the engine's `{error}` text.
    Agent::config_builder().timeout_global(Some(timeout)).http_status_as_error(false).build().into()
}

fn check(mut resp: ureq::http::Response<ureq::Body>, what: &str) -> Result<ureq::http::Response<ureq::Body>> {
    let status = resp.status().as_u16();
    if (200..300).contains(&status) {
        return Ok(resp);
    }
    let body = resp.body_mut().read_to_string().unwrap_or_default();
    let msg = serde_json::from_str::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| v.get("error").and_then(|e| e.as_str().map(str::to_owned)))
        .unwrap_or(body);
    Err(anyhow!("{what}: HTTP {status}: {}", msg.chars().take(300).collect::<String>()))
}

impl Rest {
    pub fn new(base: impl Into<String>) -> Self {
        Self { base: base.into().trim_end_matches('/').to_string() }
    }
    /// `ws://…/ws/match/{id}` for this server.
    pub fn ws_url(&self, match_id: &str) -> String {
        format!("{}/ws/match/{match_id}", self.base.replacen("http", "ws", 1))
    }
    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }
    /// `GET /api/health` → true when the engine answers `{ok:true}`. Connection errors are `Ok(false)`.
    pub fn health(&self) -> Result<bool> {
        match agent(Duration::from_secs(1)).get(self.url("/api/health")).call() {
            Ok(mut r) if r.status().is_success() => {
                let v: serde_json::Value = r.body_mut().read_json().unwrap_or_default();
                Ok(v.get("ok").and_then(|b| b.as_bool()).unwrap_or(true))
            }
            Ok(_) | Err(_) => Ok(false),
        }
    }
    /// Decks usable in `format`. Servers that ignore `?format=` return everything; in that case decks
    /// that carry an explicit different `format` are dropped client-side.
    pub fn decks(&self, format: Format) -> Result<Vec<DeckInfo>> {
        let resp = agent(Duration::from_secs(10))
            .get(self.url("/api/decks"))
            .query("format", format.as_str())
            .call()
            .context("GET /api/decks")?;
        let mut resp = check(resp, "decks")?;
        let all: Vec<DeckInfo> = resp.body_mut().read_json().context("parse decks")?;
        Ok(all.into_iter().filter(|d| d.format.is_none_or(|f| f == format)).collect())
    }
    pub fn create_match(&self, req: &CreateMatch) -> Result<MatchInfo> {
        let resp = agent(Duration::from_secs(30)).post(self.url("/api/match")).send_json(req).context("POST /api/match")?;
        let mut resp = check(resp, "create match")?;
        resp.body_mut().read_json().context("parse matchInfo")
    }
    pub fn delete_match(&self, id: &str) -> Result<()> {
        let resp = agent(Duration::from_secs(5)).delete(self.url(&format!("/api/match/{id}"))).call().context("DELETE match")?;
        check(resp, "delete match").map(|_| ())
    }
    /// Raw image bytes (jpeg/png) via the engine's Scryfall proxy `/api/img`.
    /// Servers that don't know `version` ignore it and return the normal image.
    pub fn image_bytes(&self, req: &ImageRequest) -> Result<Vec<u8>> {
        let mut r = agent(Duration::from_secs(30)).get(self.url("/api/img")).query("name", &req.name);
        if let Some(set) = req.set.as_deref().filter(|s| !s.is_empty()) {
            r = r.query("set", set);
        }
        if req.back {
            r = r.query("back", "1");
        }
        r = r.query("version", req.version.as_str());
        let resp = check(r.call().context("GET /api/img")?, &format!("image {}", req.name))?;
        let mut resp = resp;
        resp.body_mut().with_config().limit(32 * 1024 * 1024).read_to_vec().context("read image body")
    }
    /// Import a pasted decklist; returns the engine's ImportResult JSON.
    pub fn import_deck(&self, text: &str, format: Format) -> Result<serde_json::Value> {
        let body = serde_json::json!({ "text": text, "format": format.as_str() });
        let resp = agent(Duration::from_secs(60)).post(self.url("/api/deck/import")).send_json(&body).context("POST deck/import")?;
        check(resp, "import deck")?.body_mut().read_json().context("parse import result")
    }
    /// Save a deck body (DeckBody JSON, see engine/docs/protocol.md).
    pub fn save_deck(&self, body: &serde_json::Value, format: Format) -> Result<serde_json::Value> {
        let mut body = body.clone();
        if let Some(o) = body.as_object_mut() {
            o.entry("format").or_insert_with(|| format.as_str().into());
        }
        let resp = agent(Duration::from_secs(30)).post(self.url("/api/deck")).send_json(&body).context("POST deck")?;
        check(resp, "save deck")?.body_mut().read_json().context("parse save result")
    }

    // ---- deck builder ------------------------------------------------------------------------

    /// `GET /api/cards/search` — catalog search; empty `q` lists cards (popular first on new engines).
    pub fn search_cards(&self, q: &CardQuery) -> Result<Vec<CatalogCard>> {
        let mut r = agent(Duration::from_secs(20)).get(self.url("/api/cards/search")).query("q", &q.q).query("limit", q.limit.max(1).to_string());
        let opt = |r: ureq::RequestBuilder<ureq::typestate::WithoutBody>, k: &str, v: Option<&str>| match v.filter(|v| !v.is_empty()) {
            Some(v) => r.query(k, v),
            None => r,
        };
        r = opt(r, "ci", q.ci.as_deref());
        r = opt(r, "type", q.type_line.as_deref());
        r = opt(r, "text", q.text.as_deref());
        r = opt(r, "tag", q.tag.as_deref());
        r = opt(r, "sort", q.sort.as_deref());
        if let Some(c) = q.cmc {
            r = r.query("cmc", c.to_string());
        }
        if q.commander_only {
            r = r.query("commander", "1");
        }
        if q.offset > 0 {
            r = r.query("offset", q.offset.to_string());
        }
        let mut resp = check(r.call().context("GET /api/cards/search")?, "card search")?;
        resp.body_mut().with_config().limit(16 * 1024 * 1024).read_json().context("parse card search")
    }
    /// `GET /api/deck?ref=` — a deck with full card details.
    pub fn deck(&self, deck_ref: &str, format: Format) -> Result<DeckFull> {
        let r = agent(Duration::from_secs(20)).get(self.url("/api/deck")).query("ref", deck_ref).query("format", format.as_str());
        let mut resp = check(r.call().context("GET /api/deck")?, "load deck")?;
        resp.body_mut().with_config().limit(16 * 1024 * 1024).read_json().context("parse deck")
    }
    /// `POST /api/deck` with a typed body; returns the saved deck.
    pub fn save_deck_body(&self, body: &DeckBody) -> Result<DeckFull> {
        let resp = agent(Duration::from_secs(30)).post(self.url("/api/deck")).send_json(body).context("POST deck")?;
        check(resp, "save deck")?.body_mut().with_config().limit(16 * 1024 * 1024).read_json().context("parse saved deck")
    }
    /// `POST /api/deck/validate` → the engine's conformance problem, None when legal.
    pub fn validate_deck(&self, body: &DeckBody) -> Result<Option<String>> {
        let resp = agent(Duration::from_secs(30)).post(self.url("/api/deck/validate")).send_json(body).context("POST deck/validate")?;
        let v: serde_json::Value = check(resp, "validate deck")?.body_mut().read_json().context("parse validate")?;
        Ok(v.get("problem").and_then(|p| p.as_str()).map(str::to_owned))
    }
    /// `POST /api/deck/recommend` — EDHREC-style suggestions for a deck in progress.
    pub fn recommend(&self, req: &RecommendRequest) -> Result<Recommendations> {
        let resp = agent(Duration::from_secs(60)).post(self.url("/api/deck/recommend")).send_json(req).context("POST deck/recommend")?;
        check(resp, "recommend")?.body_mut().with_config().limit(32 * 1024 * 1024).read_json().context("parse recommendations")
    }
}
