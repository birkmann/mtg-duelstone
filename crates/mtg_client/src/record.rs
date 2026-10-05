//! Frame recording and replay format.
//!
//! A recording is a `.jsonl` file, one inbound server frame per line:
//! `{"ts_ms": <milliseconds since the link connected>, "frame": <the raw frame JSON as sent by the server>}`.
//! The raw JSON is kept untouched so recordings stay valid when the Rust types evolve and double as
//! protocol fixtures. [`EngineLink::replay`](crate::ws::EngineLink::replay) also accepts "plain"
//! files where each line is just the frame object (`{"t":"state",...}`); those replay without pacing.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;
use std::time::Instant;

/// Recordings stop growing past this size (a protocol livelock can emit thousands of 100 KB states per second).
const MAX_BYTES: u64 = 1 << 30;

pub struct Recorder {
    out: BufWriter<File>,
    t0: Instant,
    written: u64,
}

impl Recorder {
    pub fn create(path: &Path) -> std::io::Result<Self> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)?;
        }
        let f = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Self { out: BufWriter::new(f), t0: Instant::now(), written: 0 })
    }
    /// Append one raw frame. `raw` must be a single JSON value (it is embedded verbatim).
    pub fn write(&mut self, raw: &str) {
        let raw = raw.trim();
        if raw.is_empty() || self.written > MAX_BYTES {
            return;
        }
        self.written += raw.len() as u64 + 32;
        let ts = self.t0.elapsed().as_millis();
        let _ = writeln!(self.out, "{{\"ts_ms\":{ts},\"frame\":{raw}}}");
        let _ = self.out.flush();
    }
}

/// One line of a recording: optional timestamp + raw frame JSON text.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordedFrame {
    pub ts_ms: Option<u64>,
    pub raw: String,
}

/// Parse a recording (wrapped or plain lines). Blank / unparsable lines are skipped.
pub fn read_recording(path: &Path) -> anyhow::Result<Vec<RecordedFrame>> {
    let f = BufReader::new(File::open(path)?);
    let mut out = Vec::new();
    for line in f.lines() {
        let line = line?;
        if let Some(fr) = parse_line(&line) {
            out.push(fr);
        }
    }
    Ok(out)
}

pub fn parse_line(line: &str) -> Option<RecordedFrame> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    if let (Some(ts), Some(frame)) = (v.get("ts_ms"), v.get("frame")) {
        return Some(RecordedFrame { ts_ms: ts.as_u64(), raw: frame.to_string() });
    }
    Some(RecordedFrame { ts_ms: None, raw: v.to_string() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapped_and_plain_lines() {
        let a = parse_line(r#"{"ts_ms":12,"frame":{"t":"pong"}}"#).unwrap();
        assert_eq!(a.ts_ms, Some(12));
        assert_eq!(a.raw, r#"{"t":"pong"}"#);
        let b = parse_line(r#"{"t":"flash"}"#).unwrap();
        assert_eq!(b.ts_ms, None);
        assert!(parse_line("  ").is_none());
        assert!(parse_line("garbage").is_none());
    }
}
