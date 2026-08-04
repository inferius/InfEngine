//! A78: pull Posel smrti's pictures into a project.
//!
//! ```text
//! cargo run -p infengine_advimport --release --example bm1_extract -- <game dir> <project>
//! ```
//!
//! The archives hold the art as plain BMPs and FLC animations; this
//! takes the BMPs - the backgrounds and the objects painted over them -
//! decodes them, and writes PNGs into the project. Full-screen images
//! land under `images/` as scene backgrounds; the smaller cut-outs go
//! under `sprites/`, the same split the other imports use. The FLCs and
//! the dialogue are separate passes; this one is the pictures.

#![allow(clippy::print_stdout)]

use infengine_advimport::bm1::grp::Grp;

/// Wider or taller than this and the picture is a scene backdrop
/// rather than an object cut out to paint over one.
const SCENE_MIN: u32 = 640;

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(game), Some(project)) = (args.next(), args.next()) else {
        println!("usage: bm1_extract <game dir> <project dir>");
        return;
    };
    let game = std::path::PathBuf::from(game);
    let project = std::path::PathBuf::from(project);

    let (mut backgrounds, mut objects, mut skipped) = (0usize, 0usize, 0usize);
    for archive in read_dir_sorted(&game) {
        if archive.extension().and_then(|e| e.to_str()) != Some("grp") {
            continue;
        }
        let Ok(bytes) = std::fs::read(&archive) else {
            continue;
        };
        let Ok(grp) = Grp::parse(&bytes) else { continue };

        for entry in grp.entries() {
            if !entry.name.to_ascii_lowercase().ends_with(".bmp") {
                continue;
            }
            let image = match image::load_from_memory(grp.data_of(entry)) {
                Ok(image) => image.to_rgba8(),
                Err(_) => {
                    skipped += 1;
                    continue;
                }
            };
            let scene = image.width() >= SCENE_MIN || image.height() >= SCENE_MIN;
            let dir = if scene { "images" } else { "sprites" };
            let stem = entry.name.trim_end_matches(|c| c != '.').trim_end_matches('.');
            let rel = format!("{dir}/{stem}.png");
            let path = project.join(&rel);
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if image.save(&path).is_ok() {
                if scene {
                    backgrounds += 1;
                } else {
                    objects += 1;
                }
            } else {
                skipped += 1;
            }
        }
    }
    println!(
        "extracted {backgrounds} backgrounds and {objects} objects, {skipped} skipped"
    );
}

fn read_dir_sorted(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .map(|entries| entries.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    paths.sort();
    paths
}
