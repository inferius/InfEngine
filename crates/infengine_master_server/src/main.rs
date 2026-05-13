//! Master-server daemon for InfEngine projects.
//!
//! Run this on a public host (e.g. the user's `91.232.214.139`). It
//! listens on UDP port `27800` by default (override with argv[1]),
//! reads `MasterRequest` packets, mutates the live-server directory,
//! replies, and offers an interactive console so the operator can
//! see what's going on and run admin commands.
//!
//! Usage:
//!
//! ```sh
//! cargo run -p infengine_master_server                  # 0.0.0.0:27800
//! cargo run -p infengine_master_server -- 0.0.0.0:5500  # custom port
//! ```
//!
//! Console commands (type at the `master>` prompt + Enter):
//!
//! ```text
//! help          show this list
//! status        uptime + bound address + entry count
//! list          dump every registered server
//! kick <addr>   forget every entry matching addr / substring
//! clear         wipe the entire directory
//! quit          shut the daemon down cleanly
//! ```

#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::io::{BufRead, Write};
use std::process::ExitCode;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use infengine_net::master::{MASTER_PORT, MasterServer};
use tracing::{error, warn};

/// One operator command parsed from stdin.
#[derive(Debug)]
enum Command {
    Help,
    Status,
    List,
    Kick(String),
    Clear,
    Quit,
    Unknown(String),
}

fn parse_command(line: &str) -> Command {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Command::Unknown(String::new());
    }
    let mut parts = trimmed.splitn(2, char::is_whitespace);
    let head = parts.next().unwrap_or("").to_lowercase();
    let rest = parts.next().unwrap_or("").trim();
    match head.as_str() {
        "help" | "?" => Command::Help,
        "status" | "info" => Command::Status,
        "list" | "ls" => Command::List,
        "kick" | "drop" => Command::Kick(rest.to_string()),
        "clear" | "reset" => Command::Clear,
        "quit" | "exit" | "stop" => Command::Quit,
        _ => Command::Unknown(trimmed.to_string()),
    }
}

fn print_prompt() {
    print!("master> ");
    let _ = std::io::stdout().flush();
}

fn print_help() {
    println!(
        "\
commands:
  help / ?            show this list
  status / info       uptime + bound address + entry count
  list / ls           dump every registered server
  kick <addr>         forget every entry whose addr contains <addr>
  clear / reset       wipe the entire directory
  quit / exit / stop  shut down"
    );
}

fn main() -> ExitCode {
    // The console relies on stdout for the prompt + command output,
    // so use a tracing layer that writes to stderr to avoid the
    // prompt being interleaved with log lines.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
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
    let started = Instant::now();

    println!("InfEngine master server listening on {bind}");
    println!("Type 'help' for commands.");
    print_prompt();

    // Reader thread pumps stdin lines into a channel so the main
    // loop can poll the UDP socket without blocking on input.
    let (tx, rx) = mpsc::channel::<Command>();
    thread::spawn(move || {
        let stdin = std::io::stdin();
        let lock = stdin.lock();
        for line in lock.lines().map_while(Result::ok) {
            let cmd = parse_command(&line);
            if tx.send(cmd).is_err() {
                break;
            }
        }
    });

    let tick = Duration::from_millis(16);
    let mut last_status = Instant::now();
    loop {
        server.poll();

        // Periodic status banner so an idle operator still has a
        // heartbeat to look at.
        if last_status.elapsed() > Duration::from_secs(60) {
            eprintln!(
                "[master] uptime={:.0}s entries={}",
                started.elapsed().as_secs_f32(),
                server.entry_count()
            );
            last_status = Instant::now();
        }

        // Drain any console commands queued since last tick.
        let mut got_cmd = false;
        while let Ok(cmd) = rx.try_recv() {
            got_cmd = true;
            match cmd {
                Command::Help => print_help(),
                Command::Status => {
                    println!(
                        "bound={bind} uptime={:.0}s entries={}",
                        started.elapsed().as_secs_f32(),
                        server.entry_count()
                    );
                }
                Command::List => {
                    let entries = server.snapshot_entries();
                    if entries.is_empty() {
                        println!("(no entries)");
                    } else {
                        println!(
                            "{:>6}  {:<22}  {:<24}  {:>5}/{:<5}  MODE",
                            "TOKEN", "ADDR", "NAME", "P", "MAX"
                        );
                        for (token, info) in entries {
                            println!(
                                "{:>6}  {:<22}  {:<24}  {:>5}/{:<5}  {}",
                                token,
                                info.addr,
                                truncate(&info.name, 24),
                                info.players,
                                info.max_players,
                                info.mode
                            );
                        }
                    }
                }
                Command::Kick(needle) => {
                    if needle.is_empty() {
                        println!("usage: kick <addr substring>");
                    } else {
                        let removed = server.kick_matching(&needle);
                        println!("dropped {removed} entry(ies) matching '{needle}'");
                    }
                }
                Command::Clear => {
                    let n = server.clear();
                    println!("cleared {n} entry(ies)");
                    warn!(removed = n, "directory cleared by console");
                }
                Command::Quit => {
                    println!("shutting down");
                    return ExitCode::SUCCESS;
                }
                Command::Unknown(s) => {
                    if !s.is_empty() {
                        println!("unknown: '{s}' - type 'help'");
                    }
                }
            }
        }
        if got_cmd {
            print_prompt();
        }

        thread::sleep(tick);
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('~');
        out
    }
}
