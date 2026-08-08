//! A104: rebuild the intro's pictures and animations from the ground up.
//!
//! The intro is sequenced by the original executable, not by a script,
//! so its placements are in no data file we translate. But every intro
//! `.WSA` was drawn to sit on one `.CPS` screen, and the frames carry
//! enough of that screen to find the spot: slide the first frame over
//! the backdrop and count where the palette indices agree.
//!
//! ```text
//! cargo run -p infengine_advimport --release --example kyra_intro_fit -- <game dir> [project]
//! ```
//!
//! Prints the best offset per animation; with a project directory also
//! writes the backdrops into `pictures/`, re-cuts every intro animation
//! sheet with its scene's palette, and updates the animation index.
//! `TREE2.WSA` stores no first frame - it continues on what `TREE1.WSA`
//! left on screen - so it is decoded over TREE1's last frame.

#![allow(clippy::print_stdout)]

use std::path::{Path, PathBuf};

use glam::Vec2;
use infengine_advimport::kyra::palette::Palette;
use infengine_advimport::kyra::{cps, wsa};

/// One intro animation: our id, its file, its scene palette, and the
/// screen it was painted onto.
struct Piece {
    id: &'static str,
    file: &'static str,
    col: &'static str,
    over: &'static str,
    /// Whether this tool bakes the sheet. The story animations belong
    /// to kyra_intro_time, which re-bakes them in played order with the
    /// executable's exact timing - overwriting those here linearised
    /// them again, which read on screen as a timing regression. Only
    /// the island's looping ambients stay linear and stay here.
    bake: bool,
}

const PIECES: &[Piece] = &[
    Piece { id: "intro1_westwood", file: "WESTWOOD.WSA", col: "WW.COL", over: "sky", bake: false },
    Piece { id: "intro1_kyrandia", file: "KYRANDIA.WSA", col: "WW.COL", over: "sky", bake: false },
    Piece { id: "intro1_shore", file: "SHORE.WSA", col: "WW.COL", over: "sky", bake: true },
    Piece { id: "intro1_destruct", file: "DESTRUCT.WSA", col: "WW.COL", over: "sky", bake: true },
    Piece { id: "intro2_tree1", file: "TREE1.WSA", col: "TREE_EXP.COL", over: "TREE.CPS", bake: false },
    Piece { id: "intro2_tree2", file: "TREE2.WSA", col: "TREE_EXP.COL", over: "TREE.CPS", bake: false },
    Piece { id: "intro3_kallak", file: "KALLAK.WSA", col: "KALLAK.COL", over: "", bake: false },
    Piece { id: "intro4_mal-kal", file: "MAL-KAL.WSA", col: "MAL-KAL.COL", over: "MAL-KAL.CPS", bake: false },
];

/// Backdrops the intro composes over, with the palette that colours
/// them faithfully. MALCOLM.CPS is deliberately absent: despite the
/// extension it is a sprite sheet of the jester, not a screen.
const BACKDROPS: &[(&str, &str, &str)] = &[
    ("TREE.CPS", "TREE_EXP.COL", "tree.png"),
    ("MAL-KAL.CPS", "MAL-KAL.COL", "mal_kal.png"),
];

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(game) = args.next().map(PathBuf::from) else {
        println!("usage: kyra_intro_fit <game dir> [project]");
        return;
    };
    let project = args.next().map(PathBuf::from);

    let read = |name: &str| std::fs::read(game.join(name)).ok();
    let screen = |name: &str| read(name).and_then(|data| cps::decode(&data).ok());
    let col = |name: &str| read(name).and_then(|data| Palette::from_vga_6bit(&data));

    // The panorama: TOP.CPS above BOTTOM.CPS, 320x400. Both carry the
    // same embedded palette (it equals WW.COL), so one expansion serves.
    let sky = match (screen("TOP.CPS"), screen("BOTTOM.CPS")) {
        (Some(top), Some(bottom)) => {
            let mut indices = top.indices.clone();
            indices.extend_from_slice(&bottom.indices);
            Some((320usize, 400usize, indices, top.palette.clone()))
        }
        _ => None,
    };

    if let Some(project) = &project {
        for (name, col_name, out) in BACKDROPS {
            let Some(image) = screen(name) else {
                println!("{name}: unreadable");
                continue;
            };
            let palette = image.palette.clone().or_else(|| col(col_name));
            let rgba = image.to_rgba(palette.as_ref());
            let path = project.join("pictures").join(out);
            match save(&path, image.width, image.height, &rgba) {
                Ok(()) => println!("{name} + {col_name} -> pictures/{out}"),
                Err(err) => println!("{name}: {err}"),
            }
        }
        // The scroll strip, stitched the way seq_introLogos composes
        // its pages: the sky with the logos (TOP rows 91..195), the
        // distant island under it (TOP rows 0..64), and the vista
        // entering from BOTTOM row 64 down. TOP's rows 195..200 and
        // BOTTOM's rows 0..64 never reach the screen - gluing whole
        // files put a black bar through the pan.
        if let (Some(top), Some(bottom)) = (screen("TOP.CPS"), screen("BOTTOM.CPS")) {
            let mut indices = Vec::with_capacity(320 * 280);
            indices.extend_from_slice(&top.indices[91 * 320..195 * 320]);
            indices.extend_from_slice(&top.indices[..64 * 320]);
            indices.extend_from_slice(&bottom.indices[64 * 320..176 * 320]);
            let rgba = top
                .palette
                .as_ref()
                .or(bottom.palette.as_ref())
                .map(|p| p.expand_rgba(&indices, None))
                .unwrap_or_default();
            if !rgba.is_empty() {
                let path = project.join("pictures").join("intro_sky.png");
                match save(&path, 320, 280, &rgba) {
                    Ok(()) => println!("TOP+BOTTOM -> pictures/intro_sky.png (280-row scroll strip)"),
                    Err(err) => println!("intro_sky: {err}"),
                }
            }
        }
    }

    // TREE2 continues on what TREE1 left on screen: the forest backdrop
    // band with TREE1's last frame over it. TREE1 is 248 wide, TREE2 is
    // 320, so the seed is built at TREE2's size - backdrop rows at the
    // measured band, TREE1's pixels laid over the left of them.
    let mut tree_last: Option<Vec<u8>> = None;
    let tree_seed = |last: &[u8]| -> Option<Vec<u8>> {
        let forest = screen("TREE.CPS")?;
        let (sw, band, w1, h) = (forest.width as usize, 16usize, 248usize, 128usize);
        let mut seed = vec![0u8; sw * h];
        for y in 0..h {
            let row = (band + y) * sw;
            seed[y * sw..y * sw + sw].copy_from_slice(&forest.indices[row..row + sw]);
            for x in 0..w1 {
                let over = last[y * w1 + x];
                if over != 0 {
                    seed[y * sw + x] = over;
                }
            }
        }
        Some(seed)
    };

    let anim_dir = project
        .as_ref()
        .map(|p| p.join(infengine_adventure::anim::ANIMATION_DIR));
    let mut index = match anim_dir
        .as_ref()
        .map(|dir| infengine_adventure::anim::load_index(dir))
    {
        Some(Ok(index)) => index,
        Some(Err(err)) => {
            // Writing back a half-read index is how 700 rows were lost
            // once; a broken index stops the run instead.
            println!("refusing to touch a broken index: {err}");
            return;
        }
        None => Default::default(),
    };

    for piece in PIECES {
        let Some(data) = read(piece.file) else {
            println!("{}: unreadable", piece.file);
            continue;
        };
        let decoded = if piece.file == "TREE2.WSA" {
            let seed = tree_last.as_deref().and_then(&tree_seed);
            wsa::decode_over(&data, seed.as_deref())
        } else {
            wsa::decode(&data)
        };
        let anim = match decoded {
            Ok(anim) => anim,
            Err(err) => {
                println!("{}: {err}", piece.file);
                continue;
            }
        };
        if piece.file == "TREE1.WSA" {
            tree_last = anim.frames.last().cloned();
        }

        if let (true, Some(project), Some(dir)) = (piece.bake, &project, &anim_dir) {
            let palette = col(piece.col);
            let (w, h, rgba) = anim.to_sheet_rgba(palette.as_ref());
            let rel = format!("animations/{}.png", piece.id);
            match save(&project.join(&rel), w, h, &rgba) {
                Ok(()) => println!(
                    "{} + {} -> {rel} ({} frames of {}x{})",
                    piece.file,
                    piece.col,
                    anim.frame_count(),
                    anim.width,
                    anim.height
                ),
                Err(err) => {
                    println!("{}: {err}", piece.file);
                    continue;
                }
            }
            // A fresh entry, keeping what a person already tuned.
            let old = index.get(piece.id).cloned();
            let mut entry = infengine_adventure::anim::Animation::new(
                piece.id,
                &rel,
                anim.frame_count() as u32,
                Vec2::new(anim.width as f32, anim.height as f32),
            );
            entry.columns = anim.sheet_columns();
            entry.fps = if piece.id == "intro1_shore" { 8.0 } else { 12.0 };
            entry.looping = piece.id == "intro1_shore";
            if let Some(old) = old {
                entry.room = old.room;
                entry.at = old.at;
            }
            index.insert(entry);
            let _ = infengine_adventure::anim::save_index(&index, dir);
        }

        // Where does it sit? The first frame is how the scene opens -
        // the one that has to line up with the backdrop exactly.
        let ground = if piece.over == "sky" {
            sky.as_ref().map(|(w, h, indices, _)| (*w, *h, indices.clone()))
        } else if piece.over.is_empty() {
            None
        } else {
            screen(piece.over).map(|image| {
                (
                    image.width as usize,
                    image.height as usize,
                    image.indices,
                )
            })
        };
        let Some((sw, sh, ground)) = ground else {
            continue;
        };
        if let Some(frame) = anim.frames.first() {
            match best_fit(frame, anim.width as usize, anim.height as usize, &ground, sw, sh) {
                Some((x, y, score, of)) => println!(
                    "   {} on {}: at ({x}, {y}), {}% of {of} pixels agree",
                    piece.id,
                    piece.over,
                    score * 100 / of.max(1)
                ),
                None => println!("   {} on {}: no fit", piece.id, piece.over),
            }
        }
    }
}

/// Slide the frame over the screen; the offset where the most non-zero
/// frame pixels equal the screen's own index wins.
fn best_fit(
    frame: &[u8],
    fw: usize,
    fh: usize,
    screen: &[u8],
    sw: usize,
    sh: usize,
) -> Option<(usize, usize, usize, usize)> {
    if fw > sw || fh > sh {
        return None;
    }
    let solid = frame.iter().filter(|&&index| index != 0).count();
    if solid == 0 {
        return None;
    }
    let mut best = (0usize, 0usize, 0usize);
    for oy in 0..=(sh - fh) {
        for ox in 0..=(sw - fw) {
            let mut score = 0usize;
            for y in 0..fh {
                let f = y * fw;
                let s = (oy + y) * sw + ox;
                for x in 0..fw {
                    let index = frame[f + x];
                    if index != 0 && screen[s + x] == index {
                        score += 1;
                    }
                }
            }
            if score > best.2 {
                best = (ox, oy, score);
            }
        }
    }
    Some((best.0, best.1, best.2, solid))
}

fn save(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    image::RgbaImage::from_raw(width, height, rgba.to_vec())
        .ok_or_else(|| "pixel data does not match its size".to_string())?
        .save(path)
        .map_err(|err| err.to_string())
}
