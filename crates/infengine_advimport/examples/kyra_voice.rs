//! A114: the spoken lines, out of the CD release's .VRM archives.
//!
//! Every room has a `<ROOM>.VRM` - a PAK of numbered `.VOC` recordings,
//! and the number is the line's index in that room's text table, which
//! is exactly what our line ids count: `123.VOC` in `GEMCUT.VRM` is the
//! recording of `gemcut.123`. So each recording becomes a WAV under
//! `voice/<room>/`, and the matching line in the voice language's
//! locale gets pointed at it.
//!
//! ```text
//! cargo run -p infengine_advimport --release --example kyra_voice -- <dir of .VRM> <project> [language]
//! ```

#![allow(clippy::print_stdout)]

use std::path::PathBuf;

use infengine_advimport::kyra::pak::Pak;

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(game), Some(project)) = (args.next(), args.next()) else {
        println!("usage: kyra_voice <dir of .VRM files> <project> [language]");
        return;
    };
    let language = args.next().unwrap_or_else(|| "en".into());
    let project = PathBuf::from(project);

    let dir = project.join(infengine_loca::LOCALE_DIR);
    let mut locale = match infengine_loca::load_locale(&dir.join(format!("{language}.loca.ron"))) {
        Ok(locale) => locale,
        Err(err) => {
            println!("could not read the {language} locale: {err}");
            return;
        }
    };

    let mut archives: Vec<PathBuf> = std::fs::read_dir(&game)
        .expect("readable game dir")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("vrm"))
        })
        .collect();
    archives.sort();

    let (mut written, mut pointed, mut orphans) = (0usize, 0usize, 0usize);
    for path in &archives {
        let Ok(bytes) = std::fs::read(path) else { continue };
        let Ok(pak) = Pak::parse(&bytes) else {
            println!("{}: not a PAK", path.display());
            continue;
        };
        let room = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        for (at, entry) in pak.with_extension("VOC") {
            let Some(number) = entry
                .name
                .rsplit_once('.')
                .and_then(|(stem, _)| stem.parse::<u32>().ok())
            else {
                continue;
            };
            let Some(data) = pak.data_at(at) else { continue };
            let Some(samples) = decode_voc(data) else {
                println!("{room}/{number:03}: VOC did not decode");
                continue;
            };
            let rel = format!("voice/{room}/{number:03}.wav");
            let out = project.join(&rel);
            if let Some(parent) = out.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if std::fs::write(&out, wav(&samples.1, samples.0)).is_err() {
                println!("{rel}: could not write");
                continue;
            }
            written += 1;
            // Two digits is how the ids are minted, but a room can have
            // more than a hundred lines, and those keep their width.
            let id = format!("{room}.{number:02}");
            if let Some(line) = locale.lines.get_mut(&id) {
                line.voice = Some(rel);
                pointed += 1;
            } else {
                orphans += 1;
            }
        }
    }

    match infengine_loca::save_locale(&locale, &dir) {
        Ok(_) => println!(
            "{written} recordings written, {pointed} lines voiced, {orphans} with no line to hold them"
        ),
        Err(err) => println!("could not save the locale: {err}"),
    }
}

/// Creative Voice File to raw 8-bit unsigned samples and their rate.
fn decode_voc(data: &[u8]) -> Option<(u32, Vec<u8>)> {
    let header = u16::from_le_bytes([*data.get(20)?, *data.get(21)?]) as usize;
    let mut at = header;
    let mut rate = 0u32;
    let mut samples = Vec::new();
    while at < data.len() {
        let kind = data[at];
        if kind == 0 {
            break;
        }
        let len = u32::from_le_bytes([
            *data.get(at + 1)?,
            *data.get(at + 2)?,
            *data.get(at + 3)?,
            0,
        ]) as usize;
        let body = data.get(at + 4..at + 4 + len)?;
        match kind {
            // Sound data: sample-rate byte, codec, then samples.
            1 if body.len() >= 2 => {
                rate = 1_000_000 / (256 - body[0] as u32);
                if body[1] == 0 {
                    samples.extend_from_slice(&body[2..]);
                }
            }
            // Continuation data, same codec as the block before.
            2 => samples.extend_from_slice(body),
            // Silence: length in samples at the block's own rate.
            3 if body.len() >= 3 => {
                let quiet = u16::from_le_bytes([body[0], body[1]]) as usize;
                samples.extend(std::iter::repeat_n(0x80u8, quiet));
            }
            _ => {}
        }
        at += 4 + len;
    }
    (!samples.is_empty() && rate > 0).then_some((rate, samples))
}

/// The 44-byte classic around 8-bit unsigned mono.
fn wav(samples: &[u8], rate: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(44 + samples.len());
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + samples.len() as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes()); // byte rate, 1 byte a sample
    out.extend_from_slice(&1u16.to_le_bytes()); // block align
    out.extend_from_slice(&8u16.to_le_bytes()); // bits
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(samples.len() as u32).to_le_bytes());
    out.extend_from_slice(samples);
    out
}


