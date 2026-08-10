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

    /// Render the Roland sound effects and pick every effect's source.
    ///
    /// With a Roland attached, Kyrandia plays its effects as sequences
    /// on the module - the same numbering as the AdLib banks, in the
    /// same files as the music. This renders every effect sequence
    /// through the CM-32L, falls back to the OPL render (transcoded to
    /// ogg) for tracks the Roland score never covered, and owns every
    /// `sfx` row of the sound index, including the per-chapter banks
    /// the music switches between.
    ///
    /// Needs the `mt32` feature.
    Sfx {
        /// Directory of `.mid` files, as written by `kyra` (bank_n).
        source: PathBuf,
        /// Project root: holds `sounds/` with the OPL renders and the
        /// index, and receives `sfx_mt32/` and `sfx_opl/`.
        #[arg(long, short)]
        project: PathBuf,
        /// Directory holding CM32L control and PCM ROMs.
        #[arg(long)]
        roms: PathBuf,
        /// Stop after this many sequences, for a quick look.
        #[arg(long)]
        limit: Option<usize>,
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

/// A PCM WAV, read back for transcoding.
///
/// Only what `adl_render` writes: 16-bit little-endian samples, any
/// rate, any channel count. Returns interleaved samples.
#[cfg(feature = "mt32")]
fn read_wav(path: &std::path::Path) -> Option<(Vec<i16>, u32, u16)> {
    let data = std::fs::read(path).ok()?;
    if data.len() < 12 || &data[..4] != b"RIFF" || &data[8..12] != b"WAVE" {
        return None;
    }
    let (mut rate, mut channels, mut bits) = (0u32, 0u16, 0u16);
    let mut samples = None;
    let mut at = 12usize;
    while at + 8 <= data.len() {
        let id = &data[at..at + 4];
        let len = u32::from_le_bytes(data[at + 4..at + 8].try_into().ok()?) as usize;
        let body = data.get(at + 8..at + 8 + len)?;
        match id {
            b"fmt " if body.len() >= 16 => {
                channels = u16::from_le_bytes([body[2], body[3]]);
                rate = u32::from_le_bytes([body[4], body[5], body[6], body[7]]);
                bits = u16::from_le_bytes([body[14], body[15]]);
            }
            b"data" => {
                samples = Some(
                    body.chunks_exact(2)
                        .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
                        .collect::<Vec<_>>(),
                );
            }
            _ => {}
        }
        at += 8 + len + (len & 1);
    }
    if bits == 16 && rate > 0 && channels > 0 {
        samples.map(|samples| (samples, rate, channels))
    } else {
        None
    }
}

#[cfg(feature = "mt32")]
fn render_sfx(
    source: &std::path::Path,
    project: &std::path::Path,
    roms: &std::path::Path,
    limit: Option<usize>,
) -> Result<(), Box<dyn std::error::Error>> {
    use infengine_adventure::sound;
    use infengine_advimport::kyra::mt32;
    use infengine_advimport::ogg;

    let (control, pcm) = mt32::Module::find(roms).ok_or_else(|| {
        format!("no CM32L control and PCM ROMs under {}", roms.display())
    })?;
    let module = mt32::Module::load(&control, &pcm)?;

    // Every <bank>_<n>.mid is one track's Roland score.
    let mut sequences: Vec<(String, usize, std::path::PathBuf)> = std::fs::read_dir(source)?
        .flatten()
        .map(|entry| entry.path())
        .filter_map(|path| {
            let stem = path.file_stem()?.to_str()?;
            let ext = path.extension()?.to_str()?;
            if !ext.eq_ignore_ascii_case("mid") {
                return None;
            }
            let (bank, n) = stem.rsplit_once('_')?;
            Some((bank.to_ascii_lowercase(), n.parse::<usize>().ok()?, path.clone()))
        })
        .collect();
    sequences.sort();
    if let Some(limit) = limit {
        sequences.truncate(limit);
    }

    let index_dir = project.join(sound::SOUND_DIR);
    let mut index = sound::load_index(&index_dir)?;
    // This command owns every sfx row, so a rebuild starts from none:
    // stale rows are exactly how a renamed file keeps "working".
    index
        .sounds
        .retain(|id, _| !(id.starts_with("sfx.intro_") || id.contains(".sfx")));

    let (mut roland, mut opl, mut silent) = (0usize, 0usize, 0usize);
    for (bank, n, path) in &sequences {
        // The AdLib bank speaks first. The Roland "effects" are mostly
        // chord stingers on stock patches - thunder as an accordion
        // cluster - because the module has no effect voices to give;
        // the era's canonical setup was Roland for music, AdLib for
        // effects, and that is what this builds. The Roland sequence
        // only steps in where the OPL bank has nothing.
        let opl_rel = if bank == "intro" {
            format!("sounds/sfx{n:03}.wav")
        } else {
            format!("sounds/{bank}/sfx{n:03}.wav")
        };
        let from_opl = read_wav(&project.join(&opl_rel))
            .filter(|(samples, _, _)| samples.iter().any(|s| s.unsigned_abs() >= 16));

        let (rel, samples, rate, channels) = match from_opl {
            Some((samples, rate, channels)) => {
                opl += 1;
                (format!("sfx_opl/{bank}/sfx{n:03}.ogg"), samples, rate, channels)
            }
            None => {
                let Some(samples) = module.render(&std::fs::read(path)?).ok().filter(|s| !s.is_empty())
                else {
                    // Silent in both scores: a track the release does
                    // not have. Registering it would turn "missing"
                    // into two seconds of silence.
                    silent += 1;
                    continue;
                };
                roland += 1;
                (format!("sfx_mt32/{bank}/sfx{n:03}.ogg"), samples, mt32::RATE, 2u16)
            }
        };
        // Effects sit against full-scale music and speech, and the OPL
        // renders peak around a tenth of scale - bring every effect up
        // to a common loudness. Boost only: quiet by design stays
        // quieter than a strike, just audibly so.
        let samples = {
            let mut samples = samples;
            let peak = samples.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0);
            if peak > 0 {
                let gain = (0.71 * 32767.0) / peak as f32;
                if gain > 1.0 {
                    for sample in &mut samples {
                        *sample = ((f32::from(*sample)) * gain).clamp(-32767.0, 32767.0) as i16;
                    }
                }
            }
            samples
        };

        let target = project.join(&rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        ogg::write(&target, &samples, rate, channels)?;

        if bank == "intro" {
            index.insert(format!("sfx.intro_{n}"), rel.clone());
        } else {
            index.insert(format!("{bank}.sfx{n}"), rel.clone());
            // The unqualified id room scripts use; chapter one's bank,
            // until the music loads another.
            if bank == "kyra1a" {
                index.insert(format!("kyra.sfx{n}"), rel.clone());
            }
        }
    }

    // Which effect bank each piece of music brings with it: the bank is
    // the stem of the file the music id already points to.
    let banks: Vec<(String, String)> = index
        .sounds
        .iter()
        .filter(|(id, _)| id.starts_with("kyra.music"))
        .filter_map(|(id, file)| {
            let stem = std::path::Path::new(file).file_stem()?.to_str()?;
            let (bank, _) = stem.rsplit_once('_')?;
            Some((id.clone(), bank.to_string()))
        })
        .collect();
    index.sfx_banks = banks.into_iter().collect();

    sound::save_index(&index, &index_dir)?;
    tracing::info!(
        roland,
        opl,
        silent,
        banks = index.sfx_banks.len(),
        "sound effects rendered and indexed"
    );
    Ok(())
}

#[cfg(not(feature = "mt32"))]
fn render_sfx(
    _source: &std::path::Path,
    _project: &std::path::Path,
    _roms: &std::path::Path,
    _limit: Option<usize>,
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
        Command::Sfx {
            source,
            project,
            roms,
            limit,
        } => render_sfx(&source, &project, &roms, limit),
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
