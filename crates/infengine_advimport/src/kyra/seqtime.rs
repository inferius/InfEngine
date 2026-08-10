//! A132: the intro bytecode's clock, loops included.
//!
//! The flat opcode stream understates every scene: `loopInit` /
//! `loopInc` pairs repeat their span up to sixty-five times, and the
//! island's first thunder lands ten seconds in, not at a quarter of a
//! second. This walks a sequence the way the interpreter does -
//! executing loops - and reports what happened on which tick, which is
//! what both a baker (frame durations) and a script writer (effect
//! cues) need and what neither can read off the flat bytes.
//!
//! Only the opcodes that shape time or show something are reported;
//! the palette and region chatter stays inside. Loop semantics are the
//! interpreter's own: `loopInit` remembers the position after itself,
//! `loopInc` carries a 16-bit count and jumps back until it is spent.

/// Something a sequence did, with the tick it did it on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Did {
    /// A movie slot showed an explicit frame.
    Frame { slot: u8, frame: u8 },
    /// A movie slot stepped one frame forward.
    Next { slot: u8 },
    /// A movie slot stepped one frame back.
    Prev { slot: u8 },
    /// An effect track was started.
    Effect(u8),
    /// A talk line was printed.
    Talk(u8),
    /// A music track was started.
    Track(u8),
    /// A voice recording was started (CD interpreter only).
    Voc(u8),
}

/// Argument bytes per floppy opcode; `2` is a little-endian word.
const SIZES: [&[u8]; 30] = [
    &[1, 1],             // 00 wsaOpen
    &[1],                // 01 wsaClose
    &[1, 1, 2, 1],       // 02 wsaPlayFrame
    &[1],                // 03 wsaPlayNextFrame
    &[1],                // 04 wsaPlayPrevFrame
    &[1, 2, 1],          // 05 drawShape
    &[2],                // 06 waitTicks
    &[2],                // 07 copyWaitTicks
    &[],                 // 08 shuffleScreen
    &[],                 // 09 copyView
    &[1],                // 0A loopInit
    &[1, 2],             // 0B loopInc
    &[1],                // 0C loadPalette
    &[1],                // 0D loadBitmap
    &[],                 // 0E fadeToBlack
    &[1],                // 0F printText
    &[1, 2, 1, 1],       // 10 printTalkText
    &[],                 // 11 restoreTalkText
    &[],                 // 12 clearCurrentScreen
    &[],                 // 13 break
    &[],                 // 14 fadeFromBlack
    &[1, 1],             // 15 copyRegion
    &[1],                // 16 copyRegionSpecial
    &[2, 1, 2, 1, 1, 1], // 17 fillRect
    &[1],                // 18 playEffect
    &[1],                // 19 playTrack
    &[],                 // 1A allocTempBuffer
    &[],                 // 1B textDisplayEnable
    &[],                 // 1C textDisplayDisable
    &[],                 // 1D endOfScript
];

/// Walk one sequence, loops executed, and say what happened when.
///
/// `stream` is the whole file; the sequence runs from `at` to its own
/// `endOfScript`. Ticks are the interpreter's sixty-to-a-second.
pub fn expand(stream: &[u8], at: usize) -> Vec<(u32, Did)> {
    let mut out = Vec::new();
    let mut loops: [(Option<usize>, u16); 20] = [(None, 0xFFFF); 20];
    let mut p = at;
    let mut tick = 0u32;
    // A hard ceiling so a malformed stream terminates: the longest
    // real scene is a few thousand executed ops with loops.
    for _ in 0..1_000_000 {
        let Some(&op) = stream.get(p) else { break };
        let op = op as usize;
        if op >= SIZES.len() {
            break;
        }
        let mut q = p + 1;
        let mut args = [0i64; 6];
        for (i, &size) in SIZES[op].iter().enumerate() {
            if size == 1 {
                args[i] = stream[q] as i64;
                q += 1;
            } else {
                args[i] = u16::from_le_bytes([stream[q], stream[q + 1]]) as i64;
                q += 2;
            }
        }
        match op {
            0x02 => out.push((tick, Did::Frame { slot: args[0] as u8, frame: args[1] as u8 })),
            0x03 => out.push((tick, Did::Next { slot: args[0] as u8 })),
            0x04 => out.push((tick, Did::Prev { slot: args[0] as u8 })),
            0x06 | 0x07 => tick += args[0] as u32,
            0x0A => {
                let slot = (args[0] as usize).min(loops.len() - 1);
                loops[slot] = (Some(q), 0xFFFF);
            }
            0x0B => {
                let slot = (args[0] as usize).min(loops.len() - 1);
                let count = args[1] as u16;
                let (back, left) = loops[slot];
                // The interpreter nulls the pointer when a loop is
                // spent; re-arming without a fresh loopInit falls
                // through instead of jumping into the past forever.
                if left == 0xFFFF {
                    if let Some(back) = back {
                        loops[slot] = (Some(back), count.saturating_sub(1));
                        q = back;
                    }
                } else if left == 0 {
                    loops[slot] = (None, 0xFFFF);
                } else {
                    loops[slot] = (back, left - 1);
                    if let Some(back) = back {
                        q = back;
                    }
                }
            }
            0x10 => out.push((tick, Did::Talk(args[0] as u8))),
            0x18 => out.push((tick, Did::Effect(args[0] as u8))),
            0x19 => out.push((tick, Did::Track(args[0] as u8))),
            0x1D => break,
            _ => {}
        }
        p = q;
    }
    out
}

/// One slot's frames as `(file frame, ticks shown)`, in played order.
///
/// The step events become concrete frame numbers by walking a cursor
/// the way the player does, and each show lasts until the slot shows
/// something else - the last one gets `hold` ticks so a baker has a
/// number rather than a guess for the frame nothing replaces.
pub fn slot_frames(played: &[(u32, Did)], slot: u8, hold: u32) -> Vec<(u8, u32)> {
    let mut shows: Vec<(u32, u8)> = Vec::new();
    let mut cursor = 0u8;
    for (tick, did) in played {
        match did {
            Did::Frame { slot: s, frame } if *s == slot => {
                cursor = *frame;
                shows.push((*tick, cursor));
            }
            Did::Next { slot: s } if *s == slot => {
                cursor = cursor.saturating_add(1);
                shows.push((*tick, cursor));
            }
            Did::Prev { slot: s } if *s == slot => {
                cursor = cursor.saturating_sub(1);
                shows.push((*tick, cursor));
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for (index, (tick, frame)) in shows.iter().enumerate() {
        let until = shows.get(index + 1).map_or(tick + hold, |(next, _)| *next);
        out.push((*frame, until.saturating_sub(*tick).max(1)));
    }
    out
}

/// Argument bytes per CD opcode - a renumbered table, not an extended
/// one (see the CD interpreter in ScummVM's seqplayer): two waits, two
/// `loopInc`s, `endOfScript` at 0x20, speech at 0x22.
const CD_SIZES: [&[u8]; 38] = [
    &[1, 1],             // 00 wsaOpen
    &[1],                // 01 wsaClose
    &[1, 1, 2, 1],       // 02 wsaPlayFrame
    &[1],                // 03 wsaPlayNextFrame
    &[1],                // 04 wsaPlayPrevFrame
    &[1, 2, 1],          // 05 drawShape
    &[2],                // 06 waitTicks
    &[2],                // 07 waitTicks
    &[2],                // 08 copyWaitTicks
    &[],                 // 09 shuffleScreen
    &[],                 // 0A copyView
    &[1],                // 0B loopInit
    &[1, 2],             // 0C loopInc
    &[1, 2],             // 0D loopInc
    &[1],                // 0E skip
    &[1],                // 0F loadPalette
    &[1],                // 10 loadBitmap
    &[],                 // 11 fadeToBlack
    &[1],                // 12 printText
    &[1, 2, 1, 1],       // 13 printTalkText
    &[],                 // 14 restoreTalkText
    &[],                 // 15 clearCurrentScreen
    &[],                 // 16 break
    &[],                 // 17 fadeFromBlack
    &[1, 1],             // 18 copyRegion
    &[1],                // 19 copyRegionSpecial
    &[2, 1, 2, 1, 1, 1], // 1A fillRect
    &[1],                // 1B playEffect
    &[1],                // 1C playTrack
    &[],                 // 1D allocTempBuffer
    &[],                 // 1E textDisplayEnable
    &[],                 // 1F textDisplayDisable
    &[],                 // 20 endOfScript
    &[],                 // 21 loadIntroVRM
    &[1],                // 22 playVocFile
    &[],                 // 23 miscUnk3
    &[1],                // 24 prefetchVocFile
    &[],                 // 25 textDisplayWait
];

/// Walk one CD sequence, loops executed - the talkie's own clock, for
/// the recordings' places in it.
pub fn expand_cd(stream: &[u8], at: usize) -> Vec<(u32, Did)> {
    let mut out = Vec::new();
    let mut loops: [(Option<usize>, u16); 20] = [(None, 0xFFFF); 20];
    let mut p = at;
    let mut tick = 0u32;
    for _ in 0..1_000_000 {
        let Some(&op) = stream.get(p) else { break };
        let op = op as usize;
        if op >= CD_SIZES.len() {
            break;
        }
        let mut q = p + 1;
        let mut args = [0i64; 6];
        for (i, &size) in CD_SIZES[op].iter().enumerate() {
            if size == 1 {
                args[i] = stream[q] as i64;
                q += 1;
            } else {
                args[i] = u16::from_le_bytes([stream[q], stream[q + 1]]) as i64;
                q += 2;
            }
        }
        match op {
            0x06..=0x08 => tick += args[0] as u32,
            0x0B => {
                let slot = (args[0] as usize).min(loops.len() - 1);
                loops[slot] = (Some(q), 0xFFFF);
            }
            0x0C | 0x0D => {
                let slot = (args[0] as usize).min(loops.len() - 1);
                let count = args[1] as u16;
                let (back, left) = loops[slot];
                if left == 0xFFFF {
                    if let Some(back) = back {
                        loops[slot] = (Some(back), count.saturating_sub(1));
                        q = back;
                    }
                } else if left == 0 {
                    loops[slot] = (None, 0xFFFF);
                } else {
                    loops[slot] = (back, left - 1);
                    if let Some(back) = back {
                        q = back;
                    }
                }
            }
            0x13 => out.push((tick, Did::Talk(args[0] as u8))),
            0x1B => out.push((tick, Did::Effect(args[0] as u8))),
            0x1C => out.push((tick, Did::Track(args[0] as u8))),
            0x22 => out.push((tick, Did::Voc(args[0] as u8))),
            0x20 => break,
            _ => {}
        }
        p = q;
    }
    out
}

/// Carry a CD tick onto the floppy clock by the effects both scores
/// share: piecewise linear between matching effect cues, clamped to
/// the ends. Both lists must be the same effects in the same order,
/// which the intro's scenes are.
pub fn map_by_effects(cd: &[(u32, Did)], floppy: &[(u32, Did)], cd_tick: u32) -> f32 {
    let anchors: Vec<(f32, f32)> = cd
        .iter()
        .filter_map(|(t, d)| match d {
            Did::Effect(n) => Some((*t as f32, *n)),
            _ => None,
        })
        .zip(floppy.iter().filter_map(|(t, d)| match d {
            Did::Effect(n) => Some((*t as f32, *n)),
            _ => None,
        }))
        .filter(|((_, a), (_, b))| a == b)
        .map(|((ct, _), (ft, _))| (ct, ft))
        .collect();
    let t = cd_tick as f32;
    if anchors.is_empty() {
        return t;
    }
    if t <= anchors[0].0 {
        return anchors[0].1 - (anchors[0].0 - t);
    }
    for pair in anchors.windows(2) {
        let ((c0, f0), (c1, f1)) = (pair[0], pair[1]);
        if t <= c1 {
            let span = (c1 - c0).max(1.0);
            return f0 + (t - c0) / span * (f1 - f0);
        }
    }
    let (cl, fl) = *anchors.last().expect("nonempty");
    fl + (t - cl)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_loop_multiplies_the_ticks_inside_it() {
        // loopInit 0; wait 9; loopInc 0 x3; effect 5; end.
        let stream = [
            0x0A, 0, //
            0x06, 9, 0, //
            0x0B, 0, 3, 0, //
            0x18, 5, //
            0x1D,
        ];
        let played = expand(&stream, 0);
        // The interpreter's own arithmetic runs the body count+1
        // times: the count is stored already decremented and the jump
        // happens once more when it reaches zero.
        assert_eq!(
            played.last(),
            Some(&(36, Did::Effect(5))),
            "four passes of nine ticks come before the effect"
        );
    }

    #[test]
    fn a_slots_frames_carry_their_holds() {
        // Frame 0 shown, 12 ticks pass, step forward, 3 ticks, end.
        let stream = [
            0x02, 5, 0, 0, 0, 0, //
            0x06, 12, 0, //
            0x03, 5, //
            0x06, 3, 0, //
            0x1D,
        ];
        let played = expand(&stream, 0);
        let frames = slot_frames(&played, 5, 30);
        assert_eq!(frames, vec![(0, 12), (1, 30)]);
    }
}
