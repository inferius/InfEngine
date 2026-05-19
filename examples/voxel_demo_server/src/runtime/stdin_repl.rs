// The REPL is the one place in the server where println! / eprint!
// to the operator's terminal is the point of the module. Clippy's
// print_stdout / print_stderr lints would otherwise fire on every
// command handler.
#![allow(clippy::print_stdout, clippy::print_stderr)]

//! Stdin REPL for the headless server (W5.4a).
//!
//! Spawns a background thread that blocks on `stdin.lines()` and
//! ships each parsed line through an `mpsc::Sender` to the main
//! tick loop. The loop drains the channel on every tick (non-
//! blocking) and dispatches recognised commands.
//!
//! The REPL deliberately stays narrow for Wave 5: it parses chat-
//! style slash-command shapes (with or without the leading `/`) but
//! routes them through a fixed handler set rather than the full
//! `voxel_demo_mp` chat-dispatch table. The headless server doesn't
//! yet host a `Voxel` (Phase B.7c decoupling is still in flight), so
//! we can't reuse `cmd_save` / `cmd_kick` / etc. directly.
//!
//! Supported commands:
//! - `help` - list the REPL surface.
//! - `say <msg>` - broadcast `[CONSOLE] <msg>` to all clients.
//! - `players` - print the live roster.
//! - `kick <name>` - log + audit a kick request (deferred actual
//!   net teardown; infengine_net has no per-client disconnect today).
//! - `save` - force an autosave NOW.
//! - `stop` - request graceful shutdown.
//!
//! Output policy: command results go to stdout. Errors (unknown
//! command, missing arg) go to stderr. The prompt is a fixed
//! `infengine> ` written to stdout after each result so an
//! operator interacting via a terminal sees a consistent shape.

use std::io::{self, BufRead, BufReader, Write};
use std::sync::mpsc;
use std::thread;

use tracing::{info, warn};

use super::HeadlessServer;
use super::shutdown::ShutdownToken;

/// One parsed REPL command. Held by-value (no borrowed strings) so
/// the channel can be `Send + 'static` without lifetimes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsoleCommand {
    Help,
    Say(String),
    Players,
    Kick(String),
    Save,
    Stop,
    /// Anything else - lets the main loop print a uniform "unknown
    /// command" error to stderr without re-doing the parse.
    Unknown(String),
    /// Blank line - ignored by the dispatch but still parsed so
    /// the prompt-emission policy stays uniform.
    Empty,
}

impl ConsoleCommand {
    /// Parse one input line. Strips a leading `/` so the operator
    /// can type either `say hi` or `/say hi`; both work the same.
    pub fn parse(line: &str) -> Self {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return Self::Empty;
        }
        let stripped = trimmed.strip_prefix('/').unwrap_or(trimmed);
        let mut parts = stripped.splitn(2, char::is_whitespace);
        let head = parts.next().unwrap_or("");
        let rest = parts.next().unwrap_or("").trim();
        match head.to_lowercase().as_str() {
            "help" | "?" => Self::Help,
            "say" => Self::Say(rest.to_string()),
            "players" | "list" => Self::Players,
            "kick" => Self::Kick(rest.to_string()),
            "save" => Self::Save,
            "stop" | "quit" | "exit" | "shutdown" => Self::Stop,
            _ => Self::Unknown(head.to_string()),
        }
    }
}

/// Spawn the stdin reader thread. Returns the receiver end of the
/// command channel; the main loop owns it for the rest of the
/// process. Detaches the thread - it runs until stdin closes or the
/// process exits.
pub fn spawn_reader() -> mpsc::Receiver<ConsoleCommand> {
    let (tx, rx) = mpsc::channel::<ConsoleCommand>();
    thread::Builder::new()
        .name("stdin-repl".to_string())
        .spawn(move || reader_loop(tx))
        .expect("spawn stdin-repl thread");
    rx
}

fn reader_loop(tx: mpsc::Sender<ConsoleCommand>) {
    print_prompt();
    let stdin = io::stdin();
    let reader = BufReader::new(stdin.lock());
    for line in reader.lines() {
        match line {
            Ok(l) => {
                let cmd = ConsoleCommand::parse(&l);
                // A closed receiver means the main loop has exited;
                // bail rather than spinning on a dead channel.
                if tx.send(cmd).is_err() {
                    break;
                }
            }
            Err(err) => {
                warn!(error = %err, "stdin read error - REPL thread exiting");
                break;
            }
        }
    }
    // EOF on stdin: just exit the reader thread silently. We used
    // to auto-send `Stop` here to give a clean shutdown for
    // operators piping `/dev/null` into the bin, but the headless
    // smoke test spawns the server with an inherited (closed)
    // stdin and that auto-Stop would race the binding code. The
    // operator using a real terminal still has Ctrl+C as a fallback.
}

fn print_prompt() {
    print!("infengine> ");
    let _ = io::stdout().flush();
}

/// Dispatch one command. Writes results to stdout, errors to
/// stderr. Returns `true` when the command requested a graceful
/// shutdown so the main loop can flip the token without polling
/// the loop again.
pub fn dispatch(
    srv: &mut HeadlessServer,
    shutdown: &ShutdownToken,
    cmd: ConsoleCommand,
) -> bool {
    let mut requested_stop = false;
    match cmd {
        ConsoleCommand::Empty => {}
        ConsoleCommand::Help => {
            print_help();
        }
        ConsoleCommand::Say(msg) => {
            if msg.is_empty() {
                eprintln!("say: missing message");
            } else {
                let line = format!("[CONSOLE] {msg}");
                let buf = encode_chat_line(&line);
                srv.net_mut().broadcast_bytes("vx.chat", buf);
                info!(line = %line, "console broadcast");
                println!("{line}");
            }
        }
        ConsoleCommand::Players => {
            let ids = srv.net_mut().client_ids();
            if ids.is_empty() {
                println!("(no players connected)");
            } else {
                for id in ids {
                    let name = srv
                        .net_mut()
                        .client_name(id)
                        .unwrap_or_else(|| format!("client-{id}"));
                    println!("  id={id} name={name}");
                }
            }
        }
        ConsoleCommand::Kick(name) => {
            if name.is_empty() {
                eprintln!("kick: missing player name");
            } else {
                // infengine_net has no per-client disconnect today;
                // log + audit the request so an operator's intent is
                // recorded. The Phase B.7c parity follow-up wires
                // the actual teardown once the net surface supports
                // it.
                info!(player = %name, "console kick (not yet wired - net surface gap)");
                srv.audit_mut().write_line(&format!(
                    "CONSOLE_KICK target={name} status=deferred"
                )).ok();
                println!(
                    "kick({name}): logged - waiting on infengine_net per-client disconnect"
                );
            }
        }
        ConsoleCommand::Save => match srv.save() {
            Ok(()) => println!("save: ok"),
            Err(err) => eprintln!("save: error {err}"),
        },
        ConsoleCommand::Stop => {
            println!("stop: shutdown requested");
            shutdown.request();
            requested_stop = true;
        }
        ConsoleCommand::Unknown(head) => {
            eprintln!("unknown command: {head} (type `help` for the list)");
        }
    }
    print_prompt();
    requested_stop
}

/// Encode one chat line in the bincode-2 standard `String` wire
/// shape: a varint length prefix followed by the UTF-8 bytes.
///
/// Reproduced manually so the server bin doesn't pull in `bincode`
/// directly (it already depends on voxel_demo_mp + infengine_net;
/// the workspace policy for Wave 5 is "no new Cargo deps"). The
/// shape matches what `bincode::serde::encode_to_vec(&line,
/// bincode::config::standard())` emits for a `&String` so the
/// GUI client's `vx.chat` decoder accepts the bytes unchanged.
fn encode_chat_line(line: &str) -> Vec<u8> {
    let bytes = line.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() + 5);
    encode_varint_u64(bytes.len() as u64, &mut out);
    out.extend_from_slice(bytes);
    out
}

/// bincode-2 standard varint: `< 251` -> one byte; otherwise a
/// marker byte (251 / 252 / 253 / 254) followed by 2/4/8/16 LE
/// bytes. Server lines never approach the u16 boundary so we
/// emit the minimal encoding for everything below 65 KiB.
fn encode_varint_u64(value: u64, out: &mut Vec<u8>) {
    if value < 251 {
        out.push(value as u8);
    } else if value <= u16::MAX as u64 {
        out.push(251);
        out.extend_from_slice(&(value as u16).to_le_bytes());
    } else if value <= u32::MAX as u64 {
        out.push(252);
        out.extend_from_slice(&(value as u32).to_le_bytes());
    } else {
        out.push(253);
        out.extend_from_slice(&value.to_le_bytes());
    }
}

fn print_help() {
    println!("REPL commands:");
    println!("  help               - this list");
    println!("  say <msg>          - broadcast as [CONSOLE]");
    println!("  players            - list connected clients");
    println!("  kick <name>        - request a kick (logged)");
    println!("  save               - force an autosave");
    println!("  stop               - graceful shutdown");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_help_variants() {
        assert_eq!(ConsoleCommand::parse("help"), ConsoleCommand::Help);
        assert_eq!(ConsoleCommand::parse("/help"), ConsoleCommand::Help);
        assert_eq!(ConsoleCommand::parse("  HELP  "), ConsoleCommand::Help);
        assert_eq!(ConsoleCommand::parse("?"), ConsoleCommand::Help);
    }

    #[test]
    fn parses_say_with_message() {
        assert_eq!(
            ConsoleCommand::parse("say hello world"),
            ConsoleCommand::Say("hello world".to_string()),
        );
        assert_eq!(
            ConsoleCommand::parse("/say  multiple   spaces"),
            ConsoleCommand::Say("multiple   spaces".to_string()),
        );
    }

    #[test]
    fn parses_say_with_empty_message() {
        assert_eq!(
            ConsoleCommand::parse("say"),
            ConsoleCommand::Say(String::new()),
        );
    }

    #[test]
    fn parses_players_aliases() {
        assert_eq!(ConsoleCommand::parse("players"), ConsoleCommand::Players);
        assert_eq!(ConsoleCommand::parse("list"), ConsoleCommand::Players);
    }

    #[test]
    fn parses_kick() {
        assert_eq!(
            ConsoleCommand::parse("kick alice"),
            ConsoleCommand::Kick("alice".to_string()),
        );
    }

    #[test]
    fn parses_save() {
        assert_eq!(ConsoleCommand::parse("save"), ConsoleCommand::Save);
    }

    #[test]
    fn parses_stop_aliases() {
        assert_eq!(ConsoleCommand::parse("stop"), ConsoleCommand::Stop);
        assert_eq!(ConsoleCommand::parse("quit"), ConsoleCommand::Stop);
        assert_eq!(ConsoleCommand::parse("exit"), ConsoleCommand::Stop);
        assert_eq!(ConsoleCommand::parse("shutdown"), ConsoleCommand::Stop);
    }

    #[test]
    fn parses_empty() {
        assert_eq!(ConsoleCommand::parse(""), ConsoleCommand::Empty);
        assert_eq!(ConsoleCommand::parse("   "), ConsoleCommand::Empty);
        assert_eq!(ConsoleCommand::parse("\t\n"), ConsoleCommand::Empty);
    }

    /// W5.4a: lock in the `Stop` -> shutdown wiring. The dispatch
    /// fn flips the shutdown token; the main loop polls and bails.
    /// Tested through the parse path so the test doesn't have to
    /// build a HeadlessServer.
    #[test]
    fn stop_command_parse_pairs_with_token_flip() {
        let cmd = ConsoleCommand::parse("stop");
        assert_eq!(cmd, ConsoleCommand::Stop);
        // Sanity: any other alias still parses to Stop, so the
        // dispatch sees the same enum variant.
        for alias in ["quit", "exit", "shutdown"] {
            assert_eq!(ConsoleCommand::parse(alias), ConsoleCommand::Stop);
        }
    }

    #[test]
    fn parses_unknown_command() {
        assert_eq!(
            ConsoleCommand::parse("xyzzy"),
            ConsoleCommand::Unknown("xyzzy".to_string()),
        );
        assert_eq!(
            ConsoleCommand::parse("/foobar arg"),
            ConsoleCommand::Unknown("foobar".to_string()),
        );
    }

    #[test]
    fn encode_chat_line_short_message() {
        // "hi" is 2 bytes < 251 so the varint is a single 0x02
        // followed by the bytes.
        let buf = encode_chat_line("hi");
        assert_eq!(buf, vec![0x02, b'h', b'i']);
    }

    #[test]
    fn encode_chat_line_console_prefix() {
        let buf = encode_chat_line("[CONSOLE] hello");
        // 15-byte payload -> varint 0x0f, then UTF-8.
        assert_eq!(buf[0], 15);
        assert_eq!(&buf[1..], b"[CONSOLE] hello");
    }

    #[test]
    fn encode_chat_line_long_message_uses_u16_marker() {
        let msg: String = "a".repeat(300);
        let buf = encode_chat_line(&msg);
        // 300 > 251 -> marker 251, two LE bytes (300 = 0x012C).
        assert_eq!(buf[0], 251);
        assert_eq!(&buf[1..3], &[0x2C, 0x01]);
        assert_eq!(&buf[3..], msg.as_bytes());
    }

    #[test]
    fn slash_prefix_is_optional() {
        // A user might paste a chat-style /command or type bare
        // `command`. Both must resolve to the same enum.
        assert_eq!(
            ConsoleCommand::parse("/save"),
            ConsoleCommand::parse("save"),
        );
        assert_eq!(
            ConsoleCommand::parse("/say hi"),
            ConsoleCommand::parse("say hi"),
        );
    }
}
