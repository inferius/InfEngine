//! A3.8: the `adv-import` command line tool.
//!
//! Two jobs: survey an unknown data directory, and run a backend over a
//! known one. Nothing it reads is redistributed - it converts data the
//! user already owns into our format on their own machine.

#![forbid(unsafe_code)]
// A command line tool's entire purpose is to print things.
#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use infengine_advimport::{bm, kyra, survey};

#[derive(Parser, Debug)]
#[command(
    name = "adv-import",
    about = "Convert adventure-game data into InfEngine rooms",
    long_about = "Offline importer. Reads game data you already own and writes InfEngine \
rooms and images. The engine itself never reads foreign formats."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,

    /// Print per-file detail while working.
    #[arg(long, short, global = true)]
    verbose: bool,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Report what a data directory contains, without converting it.
    ///
    /// Groups files by extension and sniffs one of each: header bytes,
    /// entropy, whether the head reads like an offset table. Use this on
    /// a game with no backend yet.
    Survey {
        /// Directory to inspect, searched recursively.
        path: PathBuf,
    },

    /// Import Black Mirror 2 or 3.
    ///
    /// Both games share a scene format, so one backend reads either.
    Bm {
        /// Game directory, or the `Scenes` directory inside it.
        source: PathBuf,
        /// Where to write `rooms/`.
        #[arg(long, short)]
        out: PathBuf,
    },

    /// Import The Legend of Kyrandia.
    ///
    /// A translated release ships the same archives with the dialogue
    /// rewritten, so run this once per language against each copy and
    /// the lines land under matching ids.
    Kyra {
        /// Directory holding the retail `.PAK` files. A GOG install puts
        /// them in `DAT`.
        source: PathBuf,
        /// Where to write `rooms/`, `images/` and `loca/`.
        #[arg(long, short)]
        out: PathBuf,
        /// Language tag for the text in these archives.
        #[arg(long, short, default_value = "en")]
        language: String,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let level = if cli.verbose { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| level.into()),
        )
        .without_time()
        .init();

    match run(cli.command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            let mut source = std::error::Error::source(&*err);
            while let Some(inner) = source {
                eprintln!("  caused by: {inner}");
                source = inner.source();
            }
            ExitCode::FAILURE
        }
    }
}

fn run(command: Command) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        Command::Survey { path } => {
            let report = survey::survey_dir(&path)?;
            if report.files_seen == 0 {
                eprintln!("warning: no files found under {}", path.display());
            }
            print!("{report}");
            Ok(())
        }
        Command::Bm { source, out } => {
            let report = bm::import_dir(&source, &out)?;
            println!("{report}");
            if !report.is_empty() {
                println!();
                println!("written to {}", out.display());
                println!("note: layer images are referenced by name only - extracting the");
                println!("embedded textures from the .dat files is a separate step.");
            }
            Ok(())
        }
        Command::Kyra {
            source,
            out,
            language,
        } => {
            let report = kyra::import_dir(&source, &out, &language)?;
            println!("{report}");
            if report.is_empty() {
                eprintln!(
                    "warning: nothing was imported - is {} the directory with the .PAK files?",
                    source.display()
                );
            } else {
                println!("\nwritten to {}", out.display());
            }
            Ok(())
        }
    }
}
