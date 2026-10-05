//! Browser WebSocket link — the wasm32 twin of `ws.rs`, with the same [`EngineLink`] / [`LinkEvent`] API.
//!
//! There are no threads in the page, so the socket lives in a thread-local registry and is driven by
//! its JS callbacks: `onmessage` parses frames straight into the event channel, `send` writes through
//! (or queues until `onopen`), `onclose` schedules the 1.5 s reconnect and the link sends `resync` once
//! it is back — the same recovery the native link does. `EngineLink` itself only holds channels and an
//! id, so it stays `Send + Sync` for Bevy's resource bounds.

use std::cell::RefCell;
use std::collections::HashMap;

use crossbeam_channel::{Receiver, Sender, unbounded};
use mtg_protocol::{ClientFrame, ServerFrame};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::Closure;
use web_sys::{CloseEvent, MessageEvent, WebSocket};

const RECONNECT_MS: i32 = 1500;

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum LinkEvent {
    Connected,
    Disconnected(String),
    Frame(ServerFrame),
}

pub struct EngineLink {
    pub tx: Sender<ClientFrame>,
    pub rx: Receiver<LinkEvent>,
    id: u32,
}

struct Live {
    url: String,
    ws: Option<WebSocket>,
    cmd_rx: Receiver<ClientFrame>,
    ev_tx: Sender<LinkEvent>,
    attempts: u32,
    // Keep the JS callbacks alive as long as the socket.
    _cbs: Vec<Box<dyn std::any::Any>>,
}

thread_local! {
    static LINKS: RefCell<HashMap<u32, Live>> = RefCell::new(HashMap::new());
    static NEXT_ID: RefCell<u32> = const { RefCell::new(1) };
}

impl EngineLink {
    /// Connect to `ws(s)://host/ws/match/{id}[?token=…]`. Never blocks; retries forever until dropped.
    /// `record` is ignored in the browser.
    pub fn connect(ws_url: &str, _record: Option<&std::path::Path>) -> Self {
        let (tx, cmd_rx) = unbounded::<ClientFrame>();
        let (ev_tx, rx) = unbounded::<LinkEvent>();
        let id = NEXT_ID.with(|n| {
            let mut n = n.borrow_mut();
            *n += 1;
            *n
        });
        LINKS.with(|l| {
            l.borrow_mut().insert(id, Live { url: ws_url.to_string(), ws: None, cmd_rx, ev_tx, attempts: 0, _cbs: Vec::new() })
        });
        dial(id);
        Self { tx, rx, id }
    }

    pub fn send(&self, frame: ClientFrame) {
        let _ = self.tx.send(frame);
        flush(self.id);
    }
}

impl Drop for EngineLink {
    fn drop(&mut self) {
        if let Some(live) = LINKS.with(|l| l.borrow_mut().remove(&self.id))
            && let Some(ws) = live.ws
        {
            ws.set_onclose(None);
            let _ = ws.close();
        }
    }
}

fn dial(id: u32) {
    let Some(url) = LINKS.with(|l| l.borrow().get(&id).map(|x| x.url.clone())) else { return };
    let ws = match WebSocket::new(&url) {
        Ok(ws) => ws,
        Err(e) => {
            emit(id, LinkEvent::Disconnected(format!("connect failed: {e:?}")));
            schedule_reconnect(id);
            return;
        }
    };
    let onopen = Closure::<dyn FnMut()>::new(move || {
        let resync = LINKS.with(|l| {
            let mut l = l.borrow_mut();
            let Some(live) = l.get_mut(&id) else { return false };
            // Commands issued while offline refer to inputs that no longer exist.
            while live.cmd_rx.try_recv().is_ok() {}
            let _ = live.ev_tx.send(LinkEvent::Connected);
            live.attempts += 1;
            live.attempts > 1
        });
        if resync && let Some(ws) = socket(id) {
            let _ = ws.send_with_str(&ClientFrame::Resync.to_json());
        }
        flush(id);
    });
    let onmessage = Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
        let Some(text) = e.data().as_string() else { return };
        match ServerFrame::parse(&text) {
            Ok(f) => emit(id, LinkEvent::Frame(f)),
            Err(err) => web_sys::console::warn_1(&format!("[link] ignoring invalid frame ({err})").into()),
        }
    });
    let onclose = Closure::<dyn FnMut(CloseEvent)>::new(move |e: CloseEvent| {
        let reason = if e.reason().is_empty() { format!("closed ({})", e.code()) } else { e.reason() };
        emit(id, LinkEvent::Disconnected(reason));
        schedule_reconnect(id);
    });
    ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));
    ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
    ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));
    LINKS.with(|l| {
        if let Some(live) = l.borrow_mut().get_mut(&id) {
            if let Some(old) = live.ws.take() {
                old.set_onclose(None);
                let _ = old.close();
            }
            live.ws = Some(ws);
            live._cbs = vec![Box::new(onopen), Box::new(onmessage), Box::new(onclose)];
        }
    });
}

fn socket(id: u32) -> Option<WebSocket> {
    LINKS.with(|l| l.borrow().get(&id).and_then(|x| x.ws.clone()))
}

fn emit(id: u32, ev: LinkEvent) {
    LINKS.with(|l| {
        if let Some(live) = l.borrow().get(&id) {
            let _ = live.ev_tx.send(ev);
        }
    });
}

/// Write queued commands if the socket is open (otherwise they wait for `onopen`).
fn flush(id: u32) {
    let Some(ws) = socket(id) else { return };
    if ws.ready_state() != WebSocket::OPEN {
        return;
    }
    let pending: Vec<ClientFrame> =
        LINKS.with(|l| l.borrow().get(&id).map(|x| x.cmd_rx.try_iter().collect()).unwrap_or_default());
    for c in pending {
        let _ = ws.send_with_str(&c.to_json());
    }
}

fn schedule_reconnect(id: u32) {
    let Some(window) = web_sys::window() else { return };
    let cb = Closure::once_into_js(move || {
        if LINKS.with(|l| l.borrow().contains_key(&id)) {
            dial(id);
        }
    });
    let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(cb.unchecked_ref(), RECONNECT_MS);
}
