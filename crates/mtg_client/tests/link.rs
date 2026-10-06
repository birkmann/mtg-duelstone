//! Transport tests against in-process fake servers (no engine needed).

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use mtg_client::record::read_recording;
use mtg_client::{EngineLink, ImageRequest, ImageVersion, LinkEvent, Rest};
use mtg_protocol::{ClientFrame, CreateMatch, Format, ServerFrame};
use tungstenite::Message;

const T: Duration = Duration::from_secs(5);

fn next_frame(link: &EngineLink) -> ServerFrame {
    loop {
        match link.rx.recv_timeout(T).expect("timed out waiting for event") {
            LinkEvent::Frame(f) => return f,
            _ => continue,
        }
    }
}

#[test]
fn ws_roundtrip_reconnect_resync_and_recording() {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let (got_tx, got_rx) = crossbeam_channel::unbounded::<String>();
    thread::spawn(move || {
        // Connection 1: greet, send junk + a state, echo what we get, then drop.
        let (s, _) = l.accept().unwrap();
        let mut ws = tungstenite::accept(s).unwrap();
        ws.send(Message::text(r#"{"t":"hello","matchId":"m1"}"#)).unwrap();
        ws.send(Message::text("this is not json")).unwrap();
        ws.send(Message::text(r#"{"t":"brandNewFrame"}"#)).unwrap();
        ws.send(Message::text(r#"{"t":"pong"}"#)).unwrap();
        if let Message::Text(t) = ws.read().unwrap() {
            got_tx.send(t.to_string()).unwrap();
        }
        drop(ws);
        // Connection 2: the client must open with a resync.
        let (s, _) = l.accept().unwrap();
        let mut ws = tungstenite::accept(s).unwrap();
        if let Message::Text(t) = ws.read().unwrap() {
            got_tx.send(t.to_string()).unwrap();
        }
        ws.send(Message::text(r#"{"t":"flash"}"#)).unwrap();
        thread::sleep(Duration::from_millis(500));
    });

    let rec = std::env::temp_dir().join(format!("mtg-client-test-{}.jsonl", std::process::id()));
    let _ = std::fs::remove_file(&rec);
    let link = EngineLink::connect(&format!("ws://127.0.0.1:{port}/ws/match/m1"), Some(&rec));
    assert!(matches!(link.rx.recv_timeout(T).unwrap(), LinkEvent::Connected));
    assert!(matches!(next_frame(&link), ServerFrame::Hello { .. }));
    // invalid json is dropped silently, unknown frames surface as Unknown
    assert!(matches!(next_frame(&link), ServerFrame::Unknown));
    assert!(matches!(next_frame(&link), ServerFrame::Pong));
    link.send(ClientFrame::click_card(5));
    assert_eq!(got_rx.recv_timeout(T).unwrap(), r#"{"t":"click","action":"card","id":5}"#);
    // server drops the socket → Disconnected, then auto-reconnect with resync (≈1.5 s)
    loop {
        match link.rx.recv_timeout(T).unwrap() {
            LinkEvent::Disconnected(_) => break,
            _ => continue,
        }
    }
    assert!(matches!(link.rx.recv_timeout(T).unwrap(), LinkEvent::Connected));
    assert_eq!(got_rx.recv_timeout(T).unwrap(), r#"{"t":"resync"}"#);
    assert!(matches!(next_frame(&link), ServerFrame::Flash));

    // The recording kept the raw frames (including the invalid one) with timestamps.
    let frames = read_recording(&rec).unwrap_or_default();
    assert!(frames.len() >= 3);
    let text = std::fs::read_to_string(&rec).unwrap();
    assert!(text.lines().next().unwrap().starts_with(r#"{"ts_ms":"#));
    let _ = std::fs::remove_file(&rec);
}

#[test]
fn connect_to_nothing_reports_disconnect_and_keeps_trying() {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    drop(l);
    let link = EngineLink::connect(&format!("ws://127.0.0.1:{port}/ws/match/x"), None);
    assert!(matches!(link.rx.recv_timeout(T).unwrap(), LinkEvent::Disconnected(_)));
    assert!(matches!(link.rx.recv_timeout(T).unwrap(), LinkEvent::Disconnected(_)));
}

#[test]
fn replay_plain_and_wrapped_files() {
    let p = std::env::temp_dir().join(format!("mtg-client-replay-{}.jsonl", std::process::id()));
    std::fs::write(
        &p,
        concat!(
            "{\"ts_ms\":0,\"frame\":{\"t\":\"hello\",\"matchId\":\"r\"}}\n",
            "{\"ts_ms\":200,\"frame\":{\"t\":\"flash\"}}\n",
            "\n",
            "{\"t\":\"pong\"}\n",
            "garbage\n",
        ),
    )
    .unwrap();
    // as fast as possible
    let link = EngineLink::replay(&p, 0.0).unwrap();
    assert!(matches!(link.rx.recv_timeout(T).unwrap(), LinkEvent::Connected));
    assert!(matches!(next_frame(&link), ServerFrame::Hello { .. }));
    assert!(matches!(next_frame(&link), ServerFrame::Flash));
    assert!(matches!(next_frame(&link), ServerFrame::Pong));
    assert!(matches!(link.rx.recv_timeout(T).unwrap(), LinkEvent::Disconnected(_)));
    link.send(ClientFrame::ok()); // accepted and dropped

    // timestamps honoured and scaled: 200 ms at speed 1 takes ≥ 200 ms; at speed 100 it is quick
    let t0 = std::time::Instant::now();
    let slow = EngineLink::replay(&p, 1.0).unwrap();
    let _ = (next_frame(&slow), next_frame(&slow));
    assert!(t0.elapsed() >= Duration::from_millis(190));
    let t0 = std::time::Instant::now();
    let fast = EngineLink::replay(&p, 100.0).unwrap();
    let _ = (next_frame(&fast), next_frame(&fast));
    assert!(t0.elapsed() < Duration::from_millis(150));
    let _ = std::fs::remove_file(&p);
}

/// One-shot-per-request HTTP server: returns the raw request text through the channel.
fn http_server(responses: Vec<(&'static str, Vec<u8>)>) -> (String, crossbeam_channel::Receiver<String>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
    let (tx, rx) = crossbeam_channel::unbounded();
    thread::spawn(move || {
        for (ctype, body) in responses {
            let (mut s, _) = l.accept().unwrap();
            let req = read_request(&mut s);
            tx.send(req).unwrap();
            let status = if ctype.starts_with("404") { "404 Not Found" } else { "200 OK" };
            let ct = ctype.trim_start_matches("404 ");
            let head = format!("HTTP/1.1 {status}\r\nContent-Type: {ct}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
            s.write_all(head.as_bytes()).unwrap();
            s.write_all(&body).unwrap();
        }
    });
    (base, rx)
}

fn read_request(s: &mut TcpStream) -> String {
    s.set_read_timeout(Some(Duration::from_millis(300))).ok();
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        match s.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(_) => break,
        }
        let t = String::from_utf8_lossy(&buf).to_string();
        if let Some(h) = t.find("\r\n\r\n") {
            let cl = t.lines().find_map(|l| l.to_lowercase().strip_prefix("content-length: ").and_then(|v| v.trim().parse::<usize>().ok())).unwrap_or(0);
            if buf.len() >= h + 4 + cl {
                break;
            }
        }
    }
    String::from_utf8_lossy(&buf).to_string()
}

#[test]
fn rest_health_decks_match_and_images() {
    let deck = br#"[{"ref":"starter:a","name":"A","commanders":[],"mainCount":60,"colorIdentity":"R","format":"constructed","problem":null},{"ref":"precon:b","name":"B","commanders":["X"],"mainCount":99,"colorIdentity":"G","format":"commander"}]"#.to_vec();
    let m = br#"{"id":"m7","humanDeck":"a","aiDeck":"b","started":false,"over":false,"sockets":0}"#.to_vec();
    let (base, rx) = http_server(vec![
        ("application/json", br#"{"ok":true,"cards":1}"#.to_vec()),
        ("application/json", deck.clone()),
        ("application/json", deck),
        ("application/json", m),
        ("application/json", b"{}".to_vec()),
        ("image/jpeg", vec![0xFF, 0xD8, 0xFF, 1, 2, 3]),
        ("404 application/json", b"{}".to_vec()),
    ]);
    let rest = Rest::new(format!("{base}/"));
    assert!(rest.health().unwrap());
    assert!(rx.recv().unwrap().starts_with("GET /api/health"));

    let c = rest.decks(Format::Constructed).unwrap();
    assert_eq!(c.len(), 1, "decks of the wrong explicit format are filtered when the server ignores ?format=");
    assert!(rx.recv().unwrap().starts_with("GET /api/decks?format=constructed"));
    assert_eq!(rest.decks(Format::Commander).unwrap()[0].deck_ref, "precon:b");
    let _ = rx.recv();

    let info = rest
        .create_match(&CreateMatch { human_deck: "a".into(), ai_deck: "b".into(), format: Format::Constructed, seed: Some(4), human_name: Some("T".into()), setup: Default::default() })
        .unwrap();
    assert_eq!(info.id, "m7");
    let req = rx.recv().unwrap();
    assert!(req.starts_with("POST /api/match "));
    let body: serde_json::Value = serde_json::from_str(req.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert_eq!(body, serde_json::json!({"humanDeck":"a","aiDeck":"b","format":"constructed","seed":4,"humanName":"T"}));
    assert_eq!(rest.ws_url("m7"), format!("{}/ws/match/m7", base.replacen("http", "ws", 1)));

    rest.delete_match("m7").unwrap();
    assert!(rx.recv().unwrap().starts_with("DELETE /api/match/m7"));

    let bytes = rest
        .image_bytes(&ImageRequest { name: "Fire // Ice".into(), set: Some("MH2".into()), back: true, version: ImageVersion::ArtCrop })
        .unwrap();
    assert_eq!(bytes, vec![0xFF, 0xD8, 0xFF, 1, 2, 3]);
    let req = rx.recv().unwrap();
    assert!(req.starts_with("GET /api/img?name=Fire%20%2F%2F%20Ice&set=MH2&back=1&version=art_crop"), "{req}");

    // 404 → error, not bytes
    assert!(rest.image_bytes(&ImageRequest { name: "Nope".into(), set: None, back: false, version: ImageVersion::Normal }).is_err());
}

#[test]
fn rest_health_is_false_when_nothing_listens() {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    drop(l);
    assert!(!Rest::new(format!("http://127.0.0.1:{port}")).health().unwrap());
}
