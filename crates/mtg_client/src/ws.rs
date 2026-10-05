//! WebSocket link: a dedicated thread owns a `tungstenite` socket and talks to the caller over
//! crossbeam channels. Auto-reconnects (1.5 s) and sends `resync` after a reconnect; pings every 10 s.
//!
//! The socket's TCP read timeout is short (50 ms) so the same thread can alternate between reading
//! frames and flushing outbound commands without latency; tungstenite keeps partial frames buffered
//! across timeouts, so nothing is lost.

use std::io::ErrorKind;
use std::net::TcpStream;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, TryRecvError, unbounded};
use mtg_protocol::{ClientFrame, ServerFrame};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

use crate::record::{Recorder, read_recording};

const RECONNECT_DELAY: Duration = Duration::from_millis(1500);
const PING_EVERY: Duration = Duration::from_secs(10);
const READ_TIMEOUT: Duration = Duration::from_millis(50);

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)] // part of the contract; frames are moved through channels, never stored in bulk
pub enum LinkEvent {
    Connected,
    Disconnected(String),
    Frame(ServerFrame),
}

pub struct EngineLink {
    pub tx: Sender<ClientFrame>,
    pub rx: Receiver<LinkEvent>,
}

type Socket = WebSocket<MaybeTlsStream<TcpStream>>;

impl EngineLink {
    /// Connect to `ws://host:port/ws/match/{id}`. If `record` is set, every inbound frame's raw
    /// JSON is appended as one line to that `.jsonl` file (format: see [`crate::record`]).
    /// Never blocks: connection attempts (and retries) happen on the link thread.
    pub fn connect(ws_url: &str, record: Option<&Path>) -> Self {
        let (tx, cmd_rx) = unbounded::<ClientFrame>();
        let (ev_tx, rx) = unbounded::<LinkEvent>();
        let url = ws_url.to_string();
        let recorder = record.and_then(|p| match Recorder::create(p) {
            Ok(r) => Some(r),
            Err(e) => {
                eprintln!("[link] cannot open recording {}: {e}", p.display());
                None
            }
        });
        thread::Builder::new()
            .name("engine-link".into())
            .spawn(move || run_link(&url, cmd_rx, ev_tx, recorder))
            .expect("spawn engine-link thread");
        Self { tx, rx }
    }

    /// Feed a recorded `.jsonl` back as `LinkEvent`s (no engine). `speed` scales the original
    /// pacing (if timestamps were recorded); `speed <= 0` = as fast as possible. Outbound commands
    /// are accepted and dropped.
    pub fn replay(path: &Path, speed: f32) -> anyhow::Result<Self> {
        let frames = read_recording(path)?;
        let (tx, cmd_rx) = unbounded::<ClientFrame>();
        let (ev_tx, rx) = unbounded::<LinkEvent>();
        thread::Builder::new().name("engine-replay".into()).spawn(move || {
            let _ = ev_tx.send(LinkEvent::Connected);
            let start = Instant::now();
            for fr in frames {
                while cmd_rx.try_recv().is_ok() {}
                if speed > 0.0
                    && let Some(ts) = fr.ts_ms
                {
                    let due = Duration::from_secs_f64(ts as f64 / 1000.0 / speed as f64);
                    if let Some(wait) = due.checked_sub(start.elapsed()) {
                        thread::sleep(wait);
                    }
                }
                match ServerFrame::parse(&fr.raw) {
                    Ok(f) => {
                        if ev_tx.send(LinkEvent::Frame(f)).is_err() {
                            return;
                        }
                    }
                    Err(e) => eprintln!("[replay] bad frame: {e}"),
                }
            }
            let _ = ev_tx.send(LinkEvent::Disconnected("replay finished".into()));
            // Keep the command channel open so late `send`s don't error until the link is dropped.
            while cmd_rx.recv().is_ok() {}
        })?;
        Ok(Self { tx, rx })
    }

    pub fn send(&self, frame: ClientFrame) {
        let _ = self.tx.send(frame);
    }
}

fn dial(url: &str) -> Result<Socket, String> {
    let (mut sock, _resp) = tungstenite::connect(url).map_err(|e| e.to_string())?;
    if let MaybeTlsStream::Plain(s) = sock.get_mut() {
        let _ = s.set_nodelay(true);
        s.set_read_timeout(Some(READ_TIMEOUT)).map_err(|e| e.to_string())?;
    }
    Ok(sock)
}

/// Outer reconnect loop. Returns when the owning `EngineLink` is dropped.
fn run_link(url: &str, cmd_rx: Receiver<ClientFrame>, ev_tx: Sender<LinkEvent>, mut rec: Option<Recorder>) {
    let mut attempts = 0u32;
    loop {
        match dial(url) {
            Ok(mut sock) => {
                // Commands issued while offline are stale (they refer to inputs that no longer exist).
                while cmd_rx.try_recv().is_ok() {}
                if ev_tx.send(LinkEvent::Connected).is_err() {
                    return;
                }
                if attempts > 0 {
                    let _ = sock.send(Message::text(ClientFrame::Resync.to_json()));
                }
                attempts += 1;
                match pump(&mut sock, &cmd_rx, &ev_tx, &mut rec) {
                    Pump::Owner => {
                        let _ = sock.close(None);
                        return;
                    }
                    Pump::Lost(reason) => {
                        if ev_tx.send(LinkEvent::Disconnected(reason)).is_err() {
                            return;
                        }
                    }
                }
            }
            Err(e) => {
                attempts += 1;
                if ev_tx.send(LinkEvent::Disconnected(format!("connect failed: {e}"))).is_err() {
                    return;
                }
            }
        }
        // Sleep in slices so a dropped link ends the thread promptly.
        let until = Instant::now() + RECONNECT_DELAY;
        while Instant::now() < until {
            if matches!(cmd_rx.try_recv(), Err(TryRecvError::Disconnected)) {
                return;
            }
            thread::sleep(Duration::from_millis(50));
        }
    }
}

enum Pump {
    /// The `EngineLink` (command sender) was dropped.
    Owner,
    Lost(String),
}

fn pump(sock: &mut Socket, cmd_rx: &Receiver<ClientFrame>, ev_tx: &Sender<LinkEvent>, rec: &mut Option<Recorder>) -> Pump {
    let mut last_ping = Instant::now();
    loop {
        // Outbound first: commands must not wait behind a read timeout.
        loop {
            match cmd_rx.try_recv() {
                Ok(c) => {
                    if let Err(e) = sock.send(Message::text(c.to_json())) {
                        return Pump::Lost(format!("send failed: {e}"));
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return Pump::Owner,
            }
        }
        if last_ping.elapsed() >= PING_EVERY {
            last_ping = Instant::now();
            if let Err(e) = sock.send(Message::text(ClientFrame::Ping.to_json())) {
                return Pump::Lost(format!("ping failed: {e}"));
            }
        }
        match sock.read() {
            Ok(Message::Text(t)) => {
                if let Some(r) = rec.as_mut() {
                    r.write(t.as_str());
                }
                match ServerFrame::parse(t.as_str()) {
                    Ok(f) => {
                        if ev_tx.send(LinkEvent::Frame(f)).is_err() {
                            return Pump::Owner;
                        }
                    }
                    Err(e) => eprintln!("[link] ignoring invalid frame ({e}): {}", t.as_str().chars().take(160).collect::<String>()),
                }
            }
            Ok(Message::Close(c)) => return Pump::Lost(format!("closed by server: {c:?}")),
            Ok(_) => {} // ping/pong/binary: tungstenite auto-answers pings
            Err(tungstenite::Error::Io(e)) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(e) => return Pump::Lost(e.to_string()),
        }
    }
}
