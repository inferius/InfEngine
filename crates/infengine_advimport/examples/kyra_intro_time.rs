//! A115: the intro's animations timed exactly as the original played them.
//!
//! A WSA holds frames; the order and speed they play at live in the
//! executable's sequence bytecode - including loops that replay a span
//! of frames, which a linear sheet cannot express. So this interprets
//! each sequence (loops and all), collects for every animation the
//! frames in the order they were shown and the ticks that passed, and
//! re-bakes the sheet in played order with the exact rate. The engine's
//! plain "frame after frame" playback then reproduces the original
//! frame for frame.
//!
//! ```text
//! cargo run -p infengine_advimport --release --example kyra_intro_time -- <game dir> <project>
//! ```

#![allow(clippy::print_stdout)]

use std::path::PathBuf;

use glam::Vec2;
use infengine_advimport::kyra::palette::Palette;
use infengine_advimport::kyra::wsa;

/// Argument bytes per opcode, a `2` meaning a little-endian word.
const SIZES: &[&[u8]] = &[
    &[1, 1], &[1], &[1, 1, 2, 1], &[1], &[1], &[1, 2, 1], &[2], &[2], &[], &[],
    &[1], &[1, 2], &[1], &[1], &[], &[1], &[1, 2, 1, 1], &[], &[], &[],
    &[], &[1, 1], &[1], &[2, 1, 2, 1, 1, 1], &[1], &[1], &[], &[], &[], &[],
];

/// The sequences and the pieces they animate: our animation id, the
/// wsa file list index the stream uses, and the stream's byte range.
struct Scene {
    at: usize,
    end: usize,
    pieces: &'static [(&'static str, usize, &'static str, &'static str)],
}

/// WSA list order in the executable: kallak, tree1, tree2, westwood,
/// mal-kal, destruct, kyrandia, shore, ...
const SCENES: &[Scene] = &[
    Scene { at: 0x2dcfe, end: 0x2e4f3, pieces: &[("intro3_kallak", 0, "KALLAK.WSA", "KALLAK.COL")] },
    Scene {
        at: 0x2e4f3,
        end: 0x2e709,
        pieces: &[
            ("intro2_tree1", 1, "TREE1.WSA", "TREE_EXP.COL"),
            ("intro2_tree2", 2, "TREE2.WSA", "TREE_EXP.COL"),
        ],
    },
    Scene { at: 0x2e709, end: 0x2e754, pieces: &[("intro1_westwood", 3, "WESTWOOD.WSA", "WW.COL")] },
    Scene { at: 0x2e754, end: 0x2e7b0, pieces: &[("intro1_kyrandia", 6, "KYRANDIA.WSA", "WW.COL")] },
    Scene { at: 0x2e7b0, end: 0x2ea17, pieces: &[("intro4_mal-kal", 4, "MAL-KAL.WSA", "MAL-KAL.COL")] },
];

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(game), Some(project)) = (args.next(), args.next()) else {
        println!("usage: kyra_intro_time <game dir> <project>");
        return;
    };
    let game = PathBuf::from(game);
    let project = PathBuf::from(project);
    let Ok(exe) = std::fs::read(game.join("MAIN.EXE")) else {
        println!("no MAIN.EXE");
        return;
    };

    let anim_dir = project.join(infengine_adventure::anim::ANIMATION_DIR);
    let mut index = match infengine_adventure::anim::load_index(&anim_dir) {
        Ok(index) => index,
        Err(err) => {
            println!("refusing to touch a broken index: {err}");
            return;
        }
    };

    // TREE2 needs TREE1's last frame as its screen; brought over below.
    let mut tree_last: Option<Vec<u8>> = None;

    for scene in SCENES {
        let plays = played(&exe, scene.at, scene.end);
        for (id, wsa_at, file, col) in scene.pieces {
            let Some((order, ticks)) = plays.get(wsa_at) else {
                println!("{id}: the stream never plays it");
                continue;
            };
            let Ok(data) = std::fs::read(game.join(file)) else { continue };
            let decoded = if *file == "TREE2.WSA" {
                wsa::decode_over(&data, tree_last.as_deref())
            } else {
                wsa::decode(&data)
            };
            let Ok(anim) = decoded else {
                println!("{id}: undecodable");
                continue;
            };
            if *file == "TREE1.WSA" {
                tree_last = anim.frames.last().cloned();
            }

            let palette = std::fs::read(game.join(col))
                .ok()
                .and_then(|data| Palette::from_vga_6bit(&data));
            let played: Vec<&Vec<u8>> = order
                .iter()
                .filter_map(|frame| anim.frames.get(*frame))
                .collect();
            if played.is_empty() {
                continue;
            }
            let fps = 60.0 * played.len() as f32 / (*ticks).max(1) as f32;
            let sheet = bake(&played, anim.width, anim.height, palette.as_ref());
            let rel = format!("animations/{id}.png");
            if let Err(err) = save(&project.join(&rel), sheet.0, sheet.1, &sheet.2) {
                println!("{id}: {err}");
                continue;
            }
            let old = index.get(id).cloned();
            let mut row = infengine_adventure::anim::Animation::new(
                *id,
                &rel,
                played.len() as u32,
                Vec2::new(anim.width as f32, anim.height as f32),
            );
            row.columns =
                infengine_adventure::Animation::columns_for(anim.width as f32, played.len() as u32);
            row.fps = fps;
            row.looping = false;
            if let Some(old) = old {
                row.room = old.room;
                row.at = old.at;
            }
            index.insert(row);
            println!(
                "{id}: {} frames played over {:.1}s ({:.2} fps)",
                played.len(),
                *ticks as f32 / 60.0,
                fps
            );
        }
    }
    if let Err(err) = infengine_adventure::anim::save_index(&index, &anim_dir) {
        println!("could not write the index: {err}");
    }
}

/// Interpret one stream: which frames each WSA showed, and the ticks
/// its span of the scene took. Loops replay their body.
fn played(data: &[u8], at: usize, end: usize) -> std::collections::HashMap<usize, (Vec<usize>, u32)> {
    use std::collections::HashMap;
    let mut current: HashMap<usize, usize> = HashMap::new();
    let mut order: HashMap<usize, (Vec<usize>, u32)> = HashMap::new();
    let mut spans: HashMap<usize, (u32, u32)> = HashMap::new();
    let mut ticks = 0u32;
    let mut p = at;
    let mut loop_from: HashMap<i64, usize> = HashMap::new();
    let mut loops_left: HashMap<i64, u16> = HashMap::new();
    while p < end {
        let op = data[p] as usize;
        let Some(sizes) = SIZES.get(op) else { break };
        p += 1;
        let mut args = Vec::new();
        for &s in *sizes {
            if s == 1 {
                args.push(data[p] as i64);
                p += 1;
            } else {
                args.push(u16::from_le_bytes([data[p], data[p + 1]]) as i64);
                p += 2;
            }
        }
        match op {
            0x06 | 0x07 => ticks += args[0] as u32,
            0x02 => {
                let (wsa, frame) = (args[0] as usize, args[1] as usize);
                current.insert(wsa, frame);
                note(&mut order, &mut spans, wsa, frame, ticks);
            }
            0x03 => {
                let wsa = args[0] as usize;
                let frame = current.get(&wsa).map(|f| f + 1).unwrap_or(0);
                current.insert(wsa, frame);
                note(&mut order, &mut spans, wsa, frame, ticks);
            }
            0x04 => {
                let wsa = args[0] as usize;
                let frame = current.get(&wsa).map(|f| f.saturating_sub(1)).unwrap_or(0);
                current.insert(wsa, frame);
                note(&mut order, &mut spans, wsa, frame, ticks);
            }
            0x0A => {
                loop_from.insert(args[0], p);
            }
            0x0B => {
                let (slot, count) = (args[0], args[1] as u16);
                let left = loops_left.remove(&slot);
                let again = match left {
                    None => count > 1,
                    Some(left) => left > 1,
                };
                if again {
                    loops_left.insert(slot, left.map(|l| l - 1).unwrap_or(count - 1));
                    if let Some(from) = loop_from.get(&slot) {
                        p = *from;
                    }
                }
            }
            _ => {}
        }
    }
    for (wsa, (first, last)) in spans {
        if let Some(entry) = order.get_mut(&wsa) {
            entry.1 = (last - first).max(entry.0.len() as u32);
        }
    }
    order
}

fn note(
    order: &mut std::collections::HashMap<usize, (Vec<usize>, u32)>,
    spans: &mut std::collections::HashMap<usize, (u32, u32)>,
    wsa: usize,
    frame: usize,
    ticks: u32,
) {
    order.entry(wsa).or_default().0.push(frame);
    let span = spans.entry(wsa).or_insert((ticks, ticks));
    span.1 = ticks;
}

/// Frames in played order, laid out as a grid, coloured by the scene's
/// palette with index 0 transparent.
fn bake(
    frames: &[&Vec<u8>],
    width: u32,
    height: u32,
    palette: Option<&Palette>,
) -> (u32, u32, Vec<u8>) {
    let owned;
    let palette = match palette {
        Some(p) => p,
        None => {
            owned = Palette::default();
            &owned
        }
    };
    let count = frames.len() as u32;
    let columns = infengine_adventure::Animation::columns_for(width as f32, count);
    let rows = count.div_ceil(columns);
    let (sw, sh) = (width * columns, height * rows);
    let mut rgba = vec![0u8; sw as usize * sh as usize * 4];
    for (i, frame) in frames.iter().enumerate() {
        let expanded = palette.expand_rgba(frame, Some(0));
        let (ox, oy) = ((i as u32 % columns) * width, (i as u32 / columns) * height);
        for y in 0..height as usize {
            let src = y * width as usize * 4;
            let dst = ((oy as usize + y) * sw as usize + ox as usize) * 4;
            let span = width as usize * 4;
            rgba[dst..dst + span].copy_from_slice(&expanded[src..src + span]);
        }
    }
    (sw, sh, rgba)
}

fn save(path: &std::path::Path, width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    image::RgbaImage::from_raw(width, height, rgba.to_vec())
        .ok_or_else(|| "pixel data does not match its size".to_string())?
        .save(path)
        .map_err(|err| err.to_string())
}
