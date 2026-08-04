//! A79: pull Posel smrti's dialogue recordings into a project.
//!
//! ```text
//! cargo run -p infengine_advimport --release --example bm1_voice -- <game dir> <project>
//! ```
//!
//! The dialogue is thousands of OGGs, already a format the engine
//! plays, named by speaker and line - `bates0001.ogg`, `sam0654.ogg`,
//! `samdlg00010.ogg`. This copies them into the project under `voice/`,
//! foldered by the speaker their name begins with, so a folder of a
//! character's whole performance is a thing you can browse. The ambient
//! WAVs and the music are a separate pass; this one is the voices.

#![allow(clippy::print_stdout)]

use std::collections::BTreeMap;

use infengine_advimport::bm1::grp::Grp;

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(game), Some(project)) = (args.next(), args.next()) else {
        println!("usage: bm1_voice <game dir> <project dir>");
        return;
    };
    let game = std::path::PathBuf::from(game);
    let project = std::path::PathBuf::from(project);

    let mut by_speaker: BTreeMap<String, usize> = BTreeMap::new();
    let mut copied = 0usize;
    for archive in read_dir_sorted(&game) {
        if archive.extension().and_then(|e| e.to_str()) != Some("grp") {
            continue;
        }
        let Ok(bytes) = std::fs::read(&archive) else {
            continue;
        };
        let Ok(grp) = Grp::parse(&bytes) else { continue };

        for entry in grp.entries() {
            let lower = entry.name.to_ascii_lowercase();
            if !lower.ends_with(".ogg") {
                continue;
            }
            let speaker = speaker_of(&lower);
            let rel = format!("voice/{speaker}/{}", entry.name);
            let path = project.join(&rel);
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if std::fs::write(&path, grp.data_of(entry)).is_ok() {
                *by_speaker.entry(speaker).or_default() += 1;
                copied += 1;
            }
        }
    }

    println!("copied {copied} recordings from {} speakers", by_speaker.len());
    for (speaker, count) in by_speaker.iter().take(20) {
        println!("  {speaker}: {count}");
    }
}

/// The speaker a recording's name begins with: its leading letters,
/// before the digits that number the line.
fn speaker_of(name: &str) -> String {
    let stem = name.strip_suffix(".ogg").unwrap_or(name);
    let letters: String = stem.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    if letters.is_empty() {
        "misc".into()
    } else {
        letters
    }
}

fn read_dir_sorted(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .map(|entries| entries.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    paths.sort();
    paths
}
