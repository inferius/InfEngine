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
use infengine_advimport::{bm, kyra, survey, ImportError};

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
        /// ScummVM's `kyra.dat`, for the room graph.
        ///
        /// Kyrandia keeps which room leads where in its executable
        /// rather than in the archives, so without this the import
        /// produces rooms with no way out of them. Optional: leave it
        /// off and everything else still imports.
        #[arg(long)]
        kyra_dat: Option<PathBuf>,
    },

    /// Render the MT-32 soundtrack to audio.
    ///
    /// Kyrandia's music is a Roland MT-32 score, so synthesising it
    /// needs the module's two ROMs. They stay on this machine: the
    /// engine only ever sees the audio this produces.
    ///
    /// Needs the `mt32` feature.
    Music {
        /// Directory of `.mid` files, as written by `kyra`.
        source: PathBuf,
        /// Where to write the rendered audio.
        #[arg(long, short)]
        out: PathBuf,
        /// Directory holding MT32_CONTROL.ROM and MT32_PCM.ROM.
        #[arg(long)]
        roms: PathBuf,
        /// Stop after this many tracks. For trying it before committing
        /// an hour of rendering.
        #[arg(long)]
        limit: Option<usize>,
        /// Prefix for the sound ids written to the index.
        #[arg(long, default_value = "music")]
        prefix: String,
    },
}

/// Give the imported rooms the exits the game's own room table lists.
fn expand_rooms(dat: &std::path::Path, out: &std::path::Path) -> Result<(), ImportError> {
    let bytes = std::fs::read(dat).map_err(|source| ImportError::ReadFile {
        path: dat.display().to_string(),
        source,
    })?;
    let archive = kyra::kyradat::KyraDat::parse(&bytes).map_err(|err| ImportError::Format {
        path: dat.display().to_string(),
        detail: err.to_string(),
    })?;
    let graph = archive.kyrandia1().map_err(|err| ImportError::Format {
        path: dat.display().to_string(),
        detail: err.to_string(),
    })?;

    let (agree, total) = graph.symmetry();
    println!(
        "\nroom graph: {} rooms from {} backgrounds, {agree}/{total} links lead back",
        graph.rooms.len(),
        graph.names.len()
    );

    let dir = out.join("rooms");
    let done = kyra::rooms::expand(&dir, &graph).map_err(|source| ImportError::ReadFile {
        path: dir.display().to_string(),
        source,
    })?;
    println!(
        "expanded to {} rooms with {} exits ({} backgrounds replaced)",
        done.written, done.exits, done.replaced
    );
    if !done.missing.is_empty() {
        eprintln!(
            "warning: {} backgrounds the graph names never imported: {}",
            done.missing.len(),
            done.missing.join(", ")
        );
    }
    Ok(())
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

/// Render every MIDI file in `source` through the MT-32.
#[cfg(feature = "mt32")]
fn render_music(
    source: &std::path::Path,
    out: &std::path::Path,
    roms: &std::path::Path,
    limit: Option<usize>,
    prefix: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    use infengine_adventure::sound;
    use infengine_advimport::kyra::mt32;
    use infengine_advimport::ogg;

    let (control, pcm) = mt32::Module::find(roms).ok_or_else(|| {
        format!(
            "no MT32_CONTROL.ROM and MT32_PCM.ROM under {}",
            roms.display()
        )
    })?;
    tracing::info!(control = %control.display(), pcm = %pcm.display(), "loaded module");
    let module = mt32::Module::load(&control, &pcm)?;

    let mut midis: Vec<std::path::PathBuf> = std::fs::read_dir(source)?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("mid"))
        })
        .collect();
    midis.sort();
    if let Some(limit) = limit {
        midis.truncate(limit);
    }

    // The project root is the directory the music sits in, which is
    // where the sound index belongs. Written by merging rather than
    // replacing: rendering one soundtrack must not drop another.
    let root = out.parent().unwrap_or(out).to_path_buf();
    let index_dir = root.join(sound::SOUND_DIR);
    let mut index = sound::load_index(&index_dir).unwrap_or_default();
    let relative = out
        .strip_prefix(&root)
        .map(|rest| {
            // Forward slashes: the index is read on whatever platform
            // the game runs on, not the one it was imported on.
            rest.display()
                .to_string()
                .replace(std::path::MAIN_SEPARATOR, "/")
        })
        .unwrap_or_else(|_| out.display().to_string());

    let mut rendered = 0usize;
    let mut skipped = 0usize;
    let mut frames = 0usize;
    let mut bytes = 0u64;
    for path in &midis {
        let midi = std::fs::read(path)?;
        let samples = match module.render(&midi) {
            Ok(samples) => samples,
            Err(err) => {
                tracing::warn!(file = %path.display(), %err, "could not render");
                skipped += 1;
                continue;
            }
        };
        // A sequence that renders to nothing is one worth knowing about
        // rather than a zero-byte file in the output.
        if samples.is_empty() {
            tracing::debug!(file = %path.display(), "silent, not written");
            skipped += 1;
            continue;
        }
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("track");
        let target = out.join(format!("{stem}.ogg"));
        ogg::write(&target, &samples, mt32::RATE, 2)?;
        index.insert(
            format!("{prefix}.{stem}"),
            format!("{relative}/{stem}.ogg"),
        );
        rendered += 1;
        frames += samples.len() / 2;
        bytes += std::fs::metadata(&target).map(|meta| meta.len()).unwrap_or(0);
    }

    sound::save_index(&index, &index_dir)?;

    tracing::info!(
        rendered,
        skipped,
        seconds = frames as f64 / f64::from(mt32::RATE),
        megabytes = bytes as f64 / 1_048_576.0,
        sounds = index.len(),
        "rendered the MT-32 soundtrack"
    );
    Ok(())
}

#[cfg(not(feature = "mt32"))]
fn render_music(
    _source: &std::path::Path,
    _out: &std::path::Path,
    _roms: &std::path::Path,
    _limit: Option<usize>,
    _prefix: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    Err("built without the mt32 feature; rebuild with --features mt32".into())
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
        Command::Music {
            source,
            out,
            roms,
            limit,
            prefix,
        } => render_music(&source, &out, &roms, limit, &prefix),
        Command::Kyra {
            source,
            out,
            language,
            kyra_dat,
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
            if let Some(path) = kyra_dat {
                expand_rooms(&path, &out)?;
            } else {
                eprintln!(
                    "note: no --kyra-dat, so the rooms have no exits. \
                     Kyrandia keeps its room graph in the executable."
                );
            }
            Ok(())
        }
    }
}
