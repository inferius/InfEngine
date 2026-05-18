//! Append-only audit log for the headless server.
//!
//! Lives at `<world_dir>/audit.log`. Each line is a human-readable
//! text record - no JSON, no binary - so a sysadmin can `tail -f`
//! it without extra tooling. Sample lines:
//!
//! ```text
//! 2026-05-19T17:42:01.500Z CONNECT id=12 name=alice from=192.0.2.1:54321
//! 2026-05-19T17:42:05.211Z CHAT id=12 name=alice level=2 text="/tp host"
//! 2026-05-19T17:42:05.230Z CMD ALLOWED id=12 cmd=tp arg="host"
//! 2026-05-19T17:42:30.100Z DISCONNECT id=12 name=alice reason=client_close
//! ```
//!
//! Buffered writes: every line goes through a `BufWriter<File>` so
//! we don't burn an fsync per packet. The runtime flushes on every
//! autosave + on graceful shutdown. A process kill loses at most one
//! `BufWriter` capacity worth of lines (8 KiB by default) - the
//! design accepts that as a "non-critical" stream; the actual world
//! state goes through `atomic_save`.

use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::Path;

/// Append-only audit log writer.
///
/// Phase B.7b wires `connect` / `disconnect` / `flush` from the
/// tick loop; the other helpers (`write_line`, `chat`, `cmd`) are
/// hooked up in Phase B.7c when the stdin REPL + chat dispatch land
/// on the server. `#[allow(dead_code)]` on the unused-for-now
/// helpers keeps the surface ready without tripping the warning gate.
#[derive(Debug)]
pub struct AuditLog {
    file: BufWriter<File>,
}

impl AuditLog {
    /// Open (creating if missing) the audit log file at `path`.
    /// Always opens in append mode so previous-session lines stay
    /// intact across reboots.
    pub fn open(path: &Path) -> io::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        Ok(Self {
            file: BufWriter::new(file),
        })
    }

    /// Write one raw line. Prefer the typed helpers
    /// (`connect`, `disconnect`, `chat`, `cmd`) over this when the
    /// caller already knows the message shape; this is the escape
    /// hatch for one-off operational events.
    #[allow(dead_code)]
    pub fn write_line(&mut self, line: &str) -> io::Result<()> {
        writeln!(self.file, "{} {}", timestamp(), line)
    }

    /// `CONNECT` audit entry. Best-effort - errors are logged via
    /// `tracing::warn!` and swallowed so a transient I/O hiccup
    /// doesn't take the server down.
    pub fn connect(&mut self, id: u32, name: &str, from: &str) {
        if let Err(err) =
            writeln!(self.file, "{} CONNECT id={id} name={name} from={from}", timestamp())
        {
            tracing::warn!(error = %err, "audit write failed");
        }
    }

    /// `DISCONNECT` audit entry.
    pub fn disconnect(&mut self, id: u32, name: &str, reason: &str) {
        if let Err(err) = writeln!(
            self.file,
            "{} DISCONNECT id={id} name={name} reason={reason}",
            timestamp()
        ) {
            tracing::warn!(error = %err, "audit write failed");
        }
    }

    /// `CHAT` audit entry. `text` is wrapped in quotes; embedded
    /// quotes are not escaped (the audit log is human-readable, not
    /// a structured re-parseable format).
    #[allow(dead_code)]
    pub fn chat(&mut self, id: u32, name: &str, level: u8, text: &str) {
        if let Err(err) = writeln!(
            self.file,
            "{} CHAT id={id} name={name} level={level} text=\"{text}\"",
            timestamp()
        ) {
            tracing::warn!(error = %err, "audit write failed");
        }
    }

    /// `CMD` audit entry. `decision` is typically `ALLOWED` or
    /// `DENIED`.
    #[allow(dead_code)]
    pub fn cmd(&mut self, id: u32, decision: &str, cmd: &str, arg: &str) {
        if let Err(err) = writeln!(
            self.file,
            "{} CMD {decision} id={id} cmd={cmd} arg=\"{arg}\"",
            timestamp()
        ) {
            tracing::warn!(error = %err, "audit write failed");
        }
    }

    /// Flush the buffered writer to disk. The runtime calls this on
    /// every autosave + on graceful shutdown - between those moments
    /// the log can tolerate sitting in the buffer to keep the hot
    /// path lock-free.
    pub fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

impl Drop for AuditLog {
    fn drop(&mut self) {
        // Best-effort flush so a panic-on-exit doesn't drop the
        // last buffered lines. Ignore errors - we can't usefully
        // surface them from a destructor.
        let _ = self.file.flush();
    }
}

/// ISO 8601 UTC timestamp with millisecond resolution. Matches the
/// design doc's audit-log example format.
fn timestamp() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};

    fn read_lines(path: &Path) -> Vec<String> {
        let f = File::open(path).expect("open audit");
        BufReader::new(f).lines().map(|l| l.unwrap()).collect()
    }

    #[test]
    fn write_line_appends_with_timestamp_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.log");
        {
            let mut log = AuditLog::open(&path).unwrap();
            log.write_line("HELLO").unwrap();
            log.flush().unwrap();
        }
        let lines = read_lines(&path);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].ends_with(" HELLO"));
        // ISO 8601 prefix has a `T` after the date.
        assert!(lines[0].contains('T'));
    }

    #[test]
    fn connect_and_disconnect_record_id_name_reason() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.log");
        {
            let mut log = AuditLog::open(&path).unwrap();
            log.connect(7, "alice", "127.0.0.1:54321");
            log.disconnect(7, "alice", "client_close");
            log.flush().unwrap();
        }
        let lines = read_lines(&path);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("CONNECT id=7 name=alice from=127.0.0.1:54321"));
        assert!(lines[1].contains("DISCONNECT id=7 name=alice reason=client_close"));
    }

    #[test]
    fn chat_quotes_text_and_records_level() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.log");
        {
            let mut log = AuditLog::open(&path).unwrap();
            log.chat(12, "bob", 2, "/tp host");
            log.flush().unwrap();
        }
        let lines = read_lines(&path);
        assert!(lines[0].contains("CHAT id=12 name=bob level=2 text=\"/tp host\""));
    }

    #[test]
    fn append_mode_preserves_prior_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.log");
        {
            let mut log = AuditLog::open(&path).unwrap();
            log.write_line("FIRST").unwrap();
            log.flush().unwrap();
        }
        {
            let mut log = AuditLog::open(&path).unwrap();
            log.write_line("SECOND").unwrap();
            log.flush().unwrap();
        }
        let lines = read_lines(&path);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].ends_with(" FIRST"));
        assert!(lines[1].ends_with(" SECOND"));
    }
}
