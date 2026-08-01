//! What clickable rectangles a room's script defines.
//!
//! A probe: it needs the game's `.EMC` files, and what it reports is a
//! fact about them rather than about this code.
//!
//! ```text
//! cargo run -p infengine_advimport --example emc_regions -- <dir of .EMC> [room]
//! ```

#![allow(clippy::print_stdout)]

use infengine_advimport::encoding::Encoding;
use infengine_advimport::kyra::{emc, regions, vm};

/// The play area a Kyrandia room is drawn in.
const ROOM: (i32, i32) = (320, 144);

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(dir) = args.next() else {
        println!("usage: emc_regions <dir of .EMC files> [room]");
        return;
    };
    let want = args.next().map(|s| s.to_ascii_uppercase());

    let mut files: Vec<std::path::PathBuf> = match std::fs::read_dir(&dir) {
        Ok(entries) => entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| e.eq_ignore_ascii_case("emc"))
            })
            .collect(),
        Err(err) => {
            println!("could not read {dir}: {err}");
            return;
        }
    };
    files.sort();

    let (mut rooms, mut with_test, mut total, mut fitting) = (0, 0, 0, 0);
    for path in files {
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_ascii_uppercase())
            .unwrap_or_default();
        if want.as_ref().is_some_and(|w| *w != stem) {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let Ok(script) = emc::decode(&bytes, Encoding::Latin1) else {
            continue;
        };
        rooms += 1;

        let instructions = vm::disassemble(&script.code);
        if regions::find_hit_test(&instructions).is_some() {
            with_test += 1;
        }
        let found = regions::regions(&instructions);
        total += found.len();
        fitting += found.iter().filter(|r| r.fits(ROOM.0, ROOM.1)).count();

        if want.is_some() {
            println!("{stem}: {} regions", found.len());
            for region in &found {
                println!(
                    "   {:>3},{:<3} {:>3}x{:<3}  {:>2} steps{}",
                    region.x,
                    region.y,
                    region.width,
                    region.height,
                    region.body.len(),
                    if region.fits(ROOM.0, ROOM.1) {
                        ""
                    } else {
                        "   (off the screen)"
                    }
                );
            }
        } else if !found.is_empty() {
            println!("{stem:<10} {:>3} regions", found.len());
        }
    }

    println!(
        "\n{rooms} scripts, {with_test} with a hit test, {total} regions, \
         {fitting} of them on a {}x{} screen",
        ROOM.0, ROOM.1
    );
}
