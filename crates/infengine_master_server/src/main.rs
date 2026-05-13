//! Master-server daemon for InfEngine projects.
//!
//! Run this on a public host (e.g. the user's `91.232.214.139`). It
//! listens on UDP port `27800` by default (override with argv[1]),
//! reads `MasterRequest` packets, mutates the live-server directory,
//! and replies. The state is purely in-memory + lazily pruned every
//! tick - no database needed for a small directory.
//!
//! Usage:
//!
//! ```sh
//! cargo run -p infengine_master_server                # 0.0.0.0:27800
//! cargo run -p infengine_master_server -- 0.0.0.0:5500  # custom port
//! ```
//!
//! Logs each register / list / drop through `tracing`.

#![allow(clippy::print_stdout)]

use std::process::ExitCode;
use std::thread;
use std::time::{Duration, Instant};

use infengine_net::master::{MASTER_PORT, MasterServer};
use tracing::{error, info};

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let bind = std::env::args()
        .nth(1)
        .unwrap_or_else(|| format!("0.0.0.0:{MASTER_PORT}"));
    let mut server = match MasterServer::bind(&bind) {
        Ok(s) => s,
        Err(err) => {
            error!(error = %err, addr = %bind, "master bind failed");
            return ExitCode::from(2);
        }
    };
    info!(addr = %bind, "InfEngine master server listening");
    // Idle tick rate: 60 Hz is plenty. The poll loop is cheap; we
    // just want short latency on register / list replies.
    let tick = Duration::from_millis(16);
    let mut last_status = Instant::now();
    loop {
        server.poll();
        // Every 30 s log the current directory size so operators
        // can watch the daemon's pulse.
        if last_status.elapsed() > Duration::from_secs(30) {
            info!(entries = server.entry_count(), "directory status");
            last_status = Instant::now();
        }
        thread::sleep(tick);
    }
}
