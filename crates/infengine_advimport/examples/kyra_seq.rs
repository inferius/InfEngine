//! A106: read the intro's choreography out of MAIN.EXE.
//!
//! The intro and outro are not EMC scripts - a dedicated player inside
//! the executable steps through bytecode tables: which WSA, which
//! frame, where on screen, which palette, which music track, which
//! sound effect, and how many ticks to wait. Those tables are plain
//! data in the binary, right before the file-name and dialogue strings
//! they index. This finds and prints them.
//!
//! ```text
//! cargo run -p infengine_advimport --release --example kyra_seq -- <game dir>
//! ```
//!
//! Opcode semantics follow the floppy interpreter (0x00..0x1D); a table
//! is recognised by decoding cleanly from its first byte to an
//! `endOfScript`. The six streams this prints are, in file order:
//! Kallak writing, the forest, the two logos, Malcolm at the window,
//! and the island storm.

#![allow(clippy::print_stdout)]

/// Argument bytes per opcode; `2` marks a little-endian word.
const SIZES: &[&[u8]] = &[
    &[1, 1],          // 00 wsaOpen: file, offscreen
    &[1],             // 01 wsaClose
    &[1, 1, 2, 1],    // 02 wsaPlayFrame: file, frame, x, y
    &[1],             // 03 wsaPlayNextFrame
    &[1],             // 04 wsaPlayPrevFrame
    &[1, 2, 1],       // 05 drawShape: shape, x, y
    &[2],             // 06 waitTicks
    &[2],             // 07 copyWaitTicks
    &[],              // 08 shuffleScreen
    &[],              // 09 copyView
    &[1],             // 0A loopInit
    &[1, 2],          // 0B loopInc
    &[1],             // 0C loadPalette
    &[1],             // 0D loadBitmap
    &[],              // 0E fadeToBlack
    &[1],             // 0F printText
    &[1, 2, 1, 1],    // 10 printTalkText: text, x, y, colour
    &[],              // 11 restoreTalkText
    &[],              // 12 clearCurrentScreen
    &[],              // 13 break
    &[],              // 14 fadeFromBlack
    &[1, 1],          // 15 copyRegion
    &[1],             // 16 copyRegionSpecial
    &[2, 1, 2, 1, 1, 1], // 17 fillRect
    &[1],             // 18 playEffect
    &[1],             // 19 playTrack
    &[],              // 1A allocTempBuffer
    &[],              // 1B textDisplayEnable
    &[],              // 1C textDisplayDisable
    &[],              // 1D endOfScript
];

const NAMES: &[&str] = &[
    "wsaOpen", "wsaClose", "wsaPlayFrame", "wsaNext", "wsaPrev", "drawShape", "waitTicks",
    "copyWaitTicks", "shuffleScreen", "copyView", "loopInit", "loopInc", "loadPalette",
    "loadBitmap", "fadeToBlack", "printText", "printTalkText", "restoreTalkText", "clearScreen",
    "break", "fadeFromBlack", "copyRegion", "copyRegionSpecial", "fillRect", "playEffect",
    "playTrack", "allocTempBuffer", "textEnable", "textDisable", "endOfScript",
];

fn main() {
    let Some(game) = std::env::args().nth(1) else {
        println!("usage: kyra_seq <game dir with MAIN.EXE>");
        return;
    };
    let path = std::path::Path::new(&game).join("MAIN.EXE");
    let Ok(data) = std::fs::read(&path) else {
        println!("could not read {}", path.display());
        return;
    };

    // The tables sit shortly before the intro's string block. Scanning
    // the whole file also matches stray runs, so the search is anchored
    // to the region before the first intro string.
    let anchor = find(&data, b"mal-kal.cps").unwrap_or(data.len());
    let from = anchor.saturating_sub(0x3000);

    let mut at = from;
    while at < anchor {
        match decode_len(&data, at, anchor) {
            Some((ops, end)) if ops >= 20 => {
                println!("== sequence at {at:#x}, {ops} ops");
                print_stream(&data, at, end);
                at = end;
            }
            _ => at += 1,
        }
    }
}

fn find(data: &[u8], needle: &[u8]) -> Option<usize> {
    data.windows(needle.len()).position(|w| w == needle)
}

/// How many ops decode cleanly from `at` to an endOfScript.
fn decode_len(data: &[u8], at: usize, limit: usize) -> Option<(usize, usize)> {
    let mut p = at;
    let mut ops = 0usize;
    while p < limit {
        let op = *data.get(p)? as usize;
        let sizes = SIZES.get(op)?;
        p += 1;
        for &s in *sizes {
            p += s as usize;
        }
        if p > limit {
            return None;
        }
        ops += 1;
        if op == 0x1D {
            return Some((ops, p));
        }
    }
    None
}

fn print_stream(data: &[u8], at: usize, end: usize) {
    let mut p = at;
    let mut ticks = 0u32;
    while p < end {
        let op = data[p] as usize;
        p += 1;
        let mut args = Vec::new();
        for &s in SIZES[op] {
            if s == 1 {
                args.push(data[p] as i64);
                p += 1;
            } else {
                args.push(u16::from_le_bytes([data[p], data[p + 1]]) as i64);
                p += 2;
            }
        }
        if op == 0x06 || op == 0x07 {
            ticks += args[0] as u32;
        }
        // The chatter (frame stepping, waits) drowns the structure, so
        // only the ops that say what happens are printed, with a clock.
        if matches!(op, 0x00 | 0x02 | 0x05 | 0x0C | 0x0D | 0x0F | 0x10 | 0x18 | 0x19) {
            println!(
                "   {:6.1}s {:<14} {:?}",
                ticks as f32 / 60.0,
                NAMES[op],
                args
            );
        }
    }
}
