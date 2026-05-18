//! End-to-end smoke test for the headless server binary.
//!
//! Spawns the freshly-built `voxel_demo_server` exe with a temp
//! data directory + a non-default port, lets it bind, then opens a
//! `NetClient` against it and confirms the connection survives the
//! UDP handshake. After a short tick window the child is terminated
//! and the test verifies the world files landed on disk.
//!
//! Phase B.7b's smoke test is intentionally narrow - it answers
//! "does the bin boot and bind?". A richer integration suite (chat
//! commands, multi-client roster, save/load round-trip) lands once
//! the headless-parity follow-up brings mob AI / chat dispatch over
//! from the GUI demo.
//!
//! Tests deliberately echo every line the server prints so a CI
//! failure surface includes the server's own log lines next to the
//! assertion. The workspace lint config forbids `print_stderr` in
//! production code; tests get an allow at the crate level here.

#![allow(clippy::print_stderr)]
#![allow(clippy::print_stdout)]

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use infengine_net::NetClient;

/// How long the smoke test waits for the bind line on stderr before
/// giving up. The server prints "net server listening" after
/// `NetServer::bind` returns; the bind itself is instant on
/// 127.0.0.1.
const BIND_TIMEOUT: Duration = Duration::from_secs(8);

/// How long the test waits for the handshake to complete before
/// failing.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(4);

/// Pick a port for the smoke test. Hard-coding 7780 (one above the
/// dev demo's default) keeps `cargo test --workspace` reproducible
/// without a port allocator; collisions on shared CI runners will
/// re-run the test.
const TEST_PORT: u16 = 7780;

#[test]
fn server_boots_and_accepts_one_client() {
    let data_dir = tempfile::tempdir().expect("tempdir");
    // Spawn the server pointing at the temp data dir + a free-ish
    // port. `--bind 127.0.0.1` keeps the test loopback-only so two
    // concurrent CI runners on the same machine don't clash on the
    // wildcard interface.
    let exe = env!("CARGO_BIN_EXE_voxel_demo_server");
    let mut child = Command::new(exe)
        .arg("--port")
        .arg(TEST_PORT.to_string())
        .arg("--bind")
        .arg("127.0.0.1")
        .arg("--world")
        .arg("smoke")
        .arg("--worldgen")
        .arg("flat")
        .arg("--seed")
        .arg("1")
        .arg("--autosave-secs")
        .arg("3")
        .arg("--data-dir")
        .arg(data_dir.path())
        .env("RUST_LOG", "info")
        .stderr(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn server");

    // Wait for the bind line on stderr so we don't race the listener.
    let stderr = child.stderr.take().expect("stderr pipe");
    let bind_seen = wait_for_line(stderr, "net server listening", BIND_TIMEOUT);
    assert!(
        bind_seen,
        "server did not print 'net server listening' within {:?}",
        BIND_TIMEOUT
    );

    // Connect a client + poll until the handshake completes (or
    // timeout). The client send-side fires its Hello from `poll`
    // so we keep calling it on a small interval.
    let mut client = NetClient::connect(
        &format!("127.0.0.1:{TEST_PORT}"),
        "smoke-test-client",
    )
    .expect("client connect");
    let start = Instant::now();
    while start.elapsed() < HANDSHAKE_TIMEOUT && !client.handshake_done() {
        let _ = client.poll();
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        client.handshake_done(),
        "handshake did not complete within {:?}",
        HANDSHAKE_TIMEOUT
    );

    // Give the server a beat to autosave then take it down. We're
    // not asserting on the autosave file count here - that's covered
    // by the unit tests in `world_save/chunks_tests.rs`; this smoke
    // test only proves the bin boots, binds, accepts a client, and
    // exits cleanly.
    std::thread::sleep(Duration::from_millis(200));
    let _ = child.kill();
    let _ = child.wait();

    // The world directory should at least have the meta + world json.
    let world_dir = data_dir.path().join("smoke");
    assert!(
        world_dir.join("meta.json").exists(),
        "meta.json missing under {}",
        world_dir.display()
    );
    assert!(
        world_dir.join("world.json").exists(),
        "world.json missing under {}",
        world_dir.display()
    );
}

/// Drain `reader` line by line until a line containing `needle`
/// shows up or the deadline elapses. Returns whether the needle was
/// seen. The reader half of a piped child stderr does not implement
/// non-blocking reads on Windows, so we shift the read onto a worker
/// thread and poll a channel from the main test thread.
fn wait_for_line<R: std::io::Read + Send + 'static>(
    reader: R,
    needle: &str,
    timeout: Duration,
) -> bool {
    let needle = needle.to_string();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let buf = BufReader::new(reader);
        for line in buf.lines().map_while(Result::ok) {
            // Forward every line for debugging; the test asserts on
            // a marker, but we want the raw lines visible when the
            // assert fires.
            eprintln!("[server] {line}");
            if line.contains(&needle) && tx.send(()).is_err() {
                break;
            }
        }
    });
    rx.recv_timeout(timeout).is_ok()
}
