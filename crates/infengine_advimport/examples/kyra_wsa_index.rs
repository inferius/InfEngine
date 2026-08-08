//! A108: re-register room WSA animations the index lost.
//!
//! The sheets are on disk; only their index rows went missing. This
//! walks the room PAKs, decodes each `.WSA` for its frame count and
//! size, and inserts any entry the index does not already have. It
//! never overwrites an existing row and refuses to run at all if the
//! index does not parse - a half-read index written back is how the
//! rows were lost in the first place.
//!
//! ```text
//! cargo run -p infengine_advimport --release --example kyra_wsa_index -- <dir of .PAK> <project>
//! ```

#![allow(clippy::print_stdout)]

use glam::Vec2;
use infengine_adventure::anim::{self, Animation};
use infengine_advimport::kyra::{pak::Pak, wsa};

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(game), Some(project)) = (args.next(), args.next()) else {
        println!("usage: kyra_wsa_index <dir of .PAK files> <project>");
        return;
    };
    let dir = std::path::PathBuf::from(project).join(anim::ANIMATION_DIR);
    let mut index = match anim::load_index(&dir) {
        Ok(index) => index,
        Err(err) => {
            println!("refusing to touch a broken index: {err}");
            return;
        }
    };

    let mut paks: Vec<_> = std::fs::read_dir(&game)
        .expect("readable game dir")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                // The CD release keeps the rooms' animations in .APK
                // archives of the same format as the .PAKs.
                e.eq_ignore_ascii_case("pak") || e.eq_ignore_ascii_case("apk")
            })
        })
        .collect();
    paks.sort();

    // Every WSA the release has, by its lowercase stem. The rooms'
    // animations mostly live in the WSA1..WSA6 bundles, so the pak a
    // file sits in says nothing about the room - the sheet's own
    // `<room>_<stem>.png` name on disk carries that.
    let mut by_stem: std::collections::HashMap<String, Vec<u8>> = Default::default();
    for path in &paks {
        let Ok(bytes) = std::fs::read(path) else { continue };
        let Ok(pak) = Pak::parse(&bytes) else { continue };
        for (at, entry) in pak.with_extension("WSA") {
            let stem = entry
                .name
                .rsplit_once('.')
                .map(|(s, _)| s)
                .unwrap_or(&entry.name)
                .to_ascii_lowercase();
            if let Some(data) = pak.data_at(at) {
                by_stem.entry(stem).or_insert_with(|| data.to_vec());
            }
        }
    }

    let mut added = 0usize;
    let mut orphans = Vec::new();
    for file in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
        let name = file.file_name().to_string_lossy().into_owned();
        let Some(id) = name.strip_suffix(".png") else {
            continue;
        };
        if index.get(id).is_some() {
            continue;
        }
        let Some((_room, stem)) = id.split_once('_') else {
            continue;
        };
        let Some(data) = by_stem.get(stem) else {
            orphans.push(id.to_string());
            continue;
        };
        let Ok(decoded) = wsa::decode(data) else {
            println!("{id}: WSA did not decode");
            continue;
        };
        let mut row = Animation::new(
            id,
            format!("{}/{id}.png", anim::ANIMATION_DIR),
            decoded.frame_count() as u32,
            Vec2::new(decoded.width as f32, decoded.height as f32),
        );
        row.columns = decoded.sheet_columns();
        row.looping = false;
        index.insert(row);
        added += 1;
    }
    if !orphans.is_empty() {
        println!("no WSA found for: {}", orphans.join(", "));
    }
    match anim::save_index(&index, &dir) {
        Ok(()) => println!("re-registered {added} animations"),
        Err(err) => println!("could not write the index: {err}"),
    }
}
