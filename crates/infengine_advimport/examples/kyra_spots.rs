//! Redo the script side of an import, in place.
//!
//! Decompiles each room's `.EMC` again and rewrites its clickable
//! things and its scripts. The same steps the import does, on their
//! own, so a project converted before the regions were readable - or
//! before the engine's function table was - does not have to be built
//! again. A full import rewrites every PNG and every recording for a
//! change that touches nothing but rooms and scripts.
//!
//! A script file with no `.EMC` behind it is left alone. Hand-written
//! ones live beside the imported ones and are not ours to regenerate.
//!
//! ```text
//! cargo run -p infengine_advimport --example kyra_spots -- <project> <dir of .EMC>
//! ```
//!
//! Rooms are matched to scripts by their script prefix, not their id, so
//! the thirteen forest rooms all get the hotspots `FORESTA.EMC` defines.

#![allow(clippy::print_stdout)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use infengine_advimport::encoding::Encoding;
use infengine_advimport::kyra::{decompile, emc};

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(project), Some(emc_dir)) = (args.next(), args.next()) else {
        println!("usage: kyra_spots <project> <dir of .EMC files>");
        return;
    };
    let project = PathBuf::from(project);
    let rooms_dir = project.join("rooms");
    let scripts_dir = project.join("scripts");

    let mut room_files: Vec<PathBuf> = match std::fs::read_dir(&rooms_dir) {
        Ok(entries) => entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.to_string_lossy().ends_with(".room.ron"))
            .collect(),
        Err(err) => {
            println!("could not read {}: {err}", rooms_dir.display());
            return;
        }
    };
    room_files.sort();

    // One decode per script rather than one per room: the caves would
    // otherwise read the same file seventy-six times.
    let mut cache: BTreeMap<String, Option<emc::Script>> = BTreeMap::new();
    // Scripts per prefix, written once at the end. Several rooms
    // share a prefix and would otherwise write the same file over.
    let mut new_scripts: BTreeMap<String, Vec<infengine_adventure::Script>> = BTreeMap::new();

    let (mut touched, mut spots, mut without) = (0, 0, 0);
    for path in &room_files {
        let Ok(mut room) = infengine_adventure::load(path) else {
            continue;
        };
        let prefix = room.script_prefix().to_string();
        let script = cache
            .entry(prefix.clone())
            .or_insert_with(|| load_script(Path::new(&emc_dir), &prefix));
        let Some(script) = script.as_ref() else {
            without += 1;
            continue;
        };

        // The room's own logic, read again with whatever the
        // decompiler now understands.
        let list = new_scripts.entry(prefix.clone()).or_insert_with(|| {
            decompile::decompile(&prefix, script).scripts
        });

        let size = (room.size.x as i32, room.size.y as i32);
        let found = decompile::hotspots(&prefix, script, size);
        if found.is_empty() {
            continue;
        }

        // Replace rather than append, so running this twice does not
        // give every room its hotspots twice over.
        room.hotspots.clear();
        for (hotspot, compiled) in found {
            room.hotspots.push(hotspot);
            if !compiled.is_empty() && !list.iter().any(|s| s.id == compiled.id) {
                list.push(compiled);
            }
        }
        spots += room.hotspots.len();
        touched += 1;
        if let Err(err) = infengine_adventure::save(&room, path) {
            println!("could not write {}: {err}", path.display());
        }
    }

    let mut written = 0;
    for (prefix, scripts) in &new_scripts {
        if scripts.is_empty() {
            continue;
        }
        match write_scripts(&scripts_dir, prefix, scripts) {
            Ok(()) => written += scripts.len(),
            Err(err) => println!("{prefix}: {err}"),
        }
    }

    println!(
        "{touched} rooms given {spots} clickable things, {written} scripts rewritten"
    );
    if without > 0 {
        println!("{without} rooms have no script of their own");
    }
}

/// Read `<PREFIX>.EMC`, trying the spelling the game uses.
fn load_script(dir: &Path, prefix: &str) -> Option<emc::Script> {
    let bytes = std::fs::read(dir.join(format!("{}.EMC", prefix.to_ascii_uppercase())))
        .or_else(|_| std::fs::read(dir.join(format!("{prefix}.emc"))))
        .ok()?;
    emc::decode(&bytes, Encoding::Latin1).ok()
}

/// Write a room's scripts, replacing whatever was there.
///
/// Replacing rather than merging, because everything in the file was
/// derived from the same `.EMC` and a better reading of it should not
/// have to live beside the worse one it supersedes.
fn write_scripts(
    dir: &Path,
    prefix: &str,
    scripts: &[infengine_adventure::Script],
) -> std::io::Result<()> {
    let text = ron::ser::to_string_pretty(
        &scripts,
        ron::ser::PrettyConfig::new().indentor("  ").struct_names(true),
    )
    .map_err(std::io::Error::other)?;
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join(format!("{prefix}.script.ron")), text)
}
