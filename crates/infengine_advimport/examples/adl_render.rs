//! Render Kyrandia's sound effects through an emulated OPL2.
//!
//! ```text
//! cargo run -p infengine_advimport --example adl_render -- <dir of .ADL> [out dir] [track]
//! ```
//!
//! With no output directory it reports what each track would produce and
//! writes nothing, which is what to run first: a track that comes out
//! silent is either a program this does not implement enough of or a
//! track the release does not have, and the two look identical in a
//! folder of WAV files.

#![allow(clippy::print_stdout)]

use infengine_advimport::kyra::{adl, adlplay};

/// Rate the effects are written at. They are a few hundred milliseconds
/// of square-ish waves; 22 kHz is plenty and halves the files.
const RATE: u32 = 22050;

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(dir) = args.next() else {
        println!("usage: adl_render <dir of .ADL files> [out dir] [track]");
        return;
    };
    let out = args.next();
    let only: Option<usize> = args.next().and_then(|n| n.parse().ok());

    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("adl"))
        })
        .collect();
    files.sort();

    if let Some(dir) = &out
        && let Err(err) = std::fs::create_dir_all(dir)
    {
        println!("could not make {dir}: {err}");
        return;
    }

    // A track number is the same in every file, so the first file that
    // has it wins and the rest are alternatives for other chapters.
    let mut done: std::collections::BTreeSet<usize> = Default::default();
    let (mut rendered, mut silent, mut ignored) = (0usize, 0usize, 0usize);

    for path in &files {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let Ok(file) = adl::AdlFile::parse(&bytes) else {
            continue;
        };
        for (track, program) in file.tracks() {
            if only.is_some_and(|want| want != track) || !done.insert(track) {
                continue;
            }
            let Some(audio) = adlplay::render_track(&file, program as usize, RATE) else {
                continue;
            };
            ignored += audio.ignored;
            if audio.is_silent() {
                silent += 1;
            } else {
                rendered += 1;
            }

            let peak = audio.samples.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0);
            println!(
                "track {track:3}  {:>5.2}s  {:>4} steps ({:>3} not implemented)  peak {peak:>5}{}",
                audio.seconds(RATE),
                audio.steps,
                audio.ignored,
                if audio.is_silent() { "  SILENT" } else { "" }
            );

            if let Some(dir) = &out {
                let path = std::path::Path::new(dir).join(format!("sfx{track:03}.wav"));
                if let Err(err) = write_wav(&path, &audio.samples, RATE) {
                    println!("   could not write: {err}");
                }
            }
        }
    }

    println!(
        "\n{} tracks made sound, {silent} came out silent, \
         {ignored} commands not implemented",
        rendered
    );
}

/// A plain 16-bit mono WAV.
fn write_wav(path: &std::path::Path, samples: &[i16], rate: u32) -> std::io::Result<()> {
    let data = samples.len() as u32 * 2;
    let mut out = Vec::with_capacity(44 + data as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data.to_le_bytes());
    for sample in samples {
        out.extend_from_slice(&sample.to_le_bytes());
    }
    std::fs::write(path, out)
}
