//! A27.4: the audio system reads what the importer writes.
//!
//! An importer that produces files the engine cannot open is worse than
//! one that produces nothing, because nothing fails loudly. This walks
//! the imported soundtrack if it is present and decodes every track.

// The workspace forbids printing from library code; a test that
// explains why it skipped is the exception the lint is not for.
#![allow(clippy::print_stderr)]

use std::path::PathBuf;

/// Where the Kyrandia import puts the rendered MT-32 soundtrack.
fn soundtrack() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../_assets/kyrandia/music_mt32")
}

#[test]
fn every_rendered_track_decodes() {
    let dir = soundtrack();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        // The assets are derived from a game the developer owns and are
        // not in the repository, so a clean checkout has nothing to
        // check. Skipping beats failing on someone else's machine.
        eprintln!("no soundtrack under {} - skipping", dir.display());
        return;
    };

    let mut decoded = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("ogg") {
            continue;
        }
        infengine_audio::decode_sound(&path)
            .unwrap_or_else(|err| panic!("{}: {err}", path.display()));
        decoded += 1;
    }
    assert!(decoded > 0, "the directory exists but holds no tracks");
    eprintln!("decoded {decoded} tracks");
}
