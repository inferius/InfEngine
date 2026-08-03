//! A67: the room's own animations, from the `.DAT` sprite scripts.
//!
//! A Kyrandia background is taller than the screen: below the visible
//! rows, the same image stores the frames of the room's ambient
//! animations - a flickering candle, a dripping vent. The room `.DAT`
//! carries small scripts that flip rectangles of that hidden area onto
//! the screen with waits and loops between. `sceneAnimOn(n)` in the
//! room's EMC starts the n-th of those scripts, which is why 486 of
//! those calls had nothing to point at until now.
//!
//! The op layout was measured from the retail files (the published
//! interpreter conflates two of the ops); everything is little-endian
//! 16-bit words:
//!
//! ```text
//!   u16 length, FF81/FF82 ... FF83      the whole animation section
//!   FF84 (id,x*8,y,w*8,h)* FF85         shape table: image rectangles
//!   FF86 <header tags> <ops> FF87       one script per anim slot
//! ```
//!
//! Each script opens with FF9B..FFA9 tags of one argument each - FF9F
//! and FFA0 carry the anim's home position. The body's idiom is
//! `FF88 sprite, FF89 x y, FF8A wait, FF8C`, with FF8C closing one
//! frame; FF8B jumps back to the start, which is what makes an
//! ambient loop.
//!
//! A script is compiled here into a flat frame list - which shape,
//! where, how long - because our engine plays sheets, not programs.
//! What that costs: per-frame delays flatten to one rate (the average),
//! and sounds fired mid-animation are noted but not kept. Scripts that
//! jump back to their start loop forever; finite loops are unrolled.

/// One rectangle of the room image, in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shape {
    pub id: u16,
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// One compiled frame: a shape shown somewhere for a while.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub shape: u16,
    pub x: i32,
    pub y: i32,
    pub flipped: bool,
    /// Ticks at sixty a second the frame stays up.
    pub ticks: u32,
}

/// One animation slot's script, compiled.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneAnim {
    /// Slot number, which is what `sceneAnimOn` passes.
    pub index: usize,
    pub frames: Vec<Frame>,
    /// Whether the script jumps back to its start when it ends.
    pub looping: bool,
}

/// Everything the section held.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SceneAnims {
    pub shapes: Vec<Shape>,
    pub anims: Vec<SceneAnim>,
}

impl SceneAnims {
    pub fn shape(&self, id: u16) -> Option<&Shape> {
        self.shapes.iter().find(|shape| shape.id == id)
    }
}

/// Cap on loop unrolling, so a long ambient cycle stays a cycle
/// instead of becoming ten thousand frames of sheet.
const MAX_UNROLL: usize = 64;
/// Cap on compiled frames per animation, same reason.
const MAX_FRAMES: usize = 256;

/// Parse the animation section that starts at `from`.
///
/// Returns an empty set rather than an error when the section is
/// absent or unreadable: a room with no ambient animations is normal,
/// and one whose section we misread should import as still rather
/// than not at all.
pub fn parse(data: &[u8], from: usize) -> SceneAnims {
    let mut out = SceneAnims::default();
    // The section announces its own length first.
    let Some(len) = data
        .get(from..from + 2)
        .map(|b| usize::from(u16::from_le_bytes([b[0], b[1]])))
    else {
        return out;
    };
    let Some(section) = data.get(from + 2..from + 2 + len) else {
        return out;
    };
    let words: Vec<u16> = section
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    if !matches!(words.first(), Some(0xFF81 | 0xFF82)) {
        return out;
    }

    let mut i = 1;
    while i < words.len() {
        match words[i] {
            0xFF83 => break,
            0xFF84 => {
                i += 1;
                while i + 4 < words.len() && words[i] < 0xFF00 {
                    out.shapes.push(Shape {
                        id: words[i],
                        x: u32::from(words[i + 1]) * 8,
                        y: u32::from(words[i + 2]),
                        w: u32::from(words[i + 3]) * 8,
                        h: u32::from(words[i + 4]),
                    });
                    i += 5;
                }
            }
            0xFF86 => {
                let start = i + 1;
                let mut end = start;
                while end < words.len() && words[end] != 0xFF87 {
                    end += 1;
                }
                let index = out.anims.len();
                out.anims.push(compile(index, &words[start..end]));
                i = end;
            }
            _ => i += 1,
        }
        i += 1;
    }
    out
}

/// Run one script into a flat frame list.
fn compile(index: usize, ops: &[u16]) -> SceneAnim {
    let mut anim = SceneAnim {
        index,
        frames: Vec::new(),
        looping: false,
    };
    // The header tags: one argument each. FF9B carries the sprite the
    // animation starts showing, FF9F/FFA0 its home position; the rest
    // are slot bookkeeping.
    let (mut home_x, mut home_y) = (0i32, 0i32);
    let mut init_sprite: Option<u16> = None;
    let mut i = 0usize;
    while i + 1 < ops.len() && (0xFF9B..=0xFFA9).contains(&ops[i]) {
        match ops[i] {
            0xFF9B => init_sprite = Some(ops[i + 1]),
            0xFF9F => home_x = i32::from(ops[i + 1]),
            0xFFA0 => home_y = i32::from(ops[i + 1]),
            _ => {}
        }
        i += 2;
    }

    let (mut x, mut y) = (home_x, home_y);
    let mut sprite: Option<u16> = None;
    let mut flipped = false;
    let mut wait: u32 = 6;
    let mut loops: Vec<(usize, usize)> = Vec::new();
    let mut steps = 0usize;

    while i < ops.len() && anim.frames.len() < MAX_FRAMES && steps < 20_000 {
        steps += 1;
        let arg = |k: usize| ops.get(i + k).copied().unwrap_or(0);
        match ops[i] {
            0xFF88 | 0xFF8D => {
                sprite = Some(arg(1));
                flipped = ops[i] == 0xFF8D;
                i += 2;
            }
            0xFF90 | 0xFF91 => {
                sprite = Some(arg(1));
                flipped = ops[i] == 0xFF91;
                x = home_x;
                y = home_y;
                i += 2;
            }
            0xFF89 => {
                x = i32::from(arg(1));
                y = i32::from(arg(2));
                i += 3;
            }
            0xFF8A => {
                wait = u32::from(arg(1)).max(1);
                i += 2;
            }
            // A random wait reads as its middle; the jitter is the
            // original's flavour, not its structure.
            0xFFB3 => {
                wait = (u32::from(arg(1)) + u32::from(arg(2))).div_ceil(2).max(1);
                i += 3;
            }
            // The frame boundary: whatever is set now shows for the
            // current wait.
            0xFF8C => {
                if let Some(shape) = sprite {
                    anim.frames.push(Frame {
                        shape,
                        x,
                        y,
                        flipped,
                        ticks: wait,
                    });
                }
                i += 1;
            }
            0xFF8B => {
                anim.looping = true;
                break;
            }
            0xFF8E => {
                loops.push((i + 2, usize::from(arg(1)).min(MAX_UNROLL)));
                i += 2;
            }
            0xFF8F => {
                if let Some((back, left)) = loops.pop()
                    && left > 1
                {
                    loops.push((back, left - 1));
                    i = back;
                    continue;
                }
                i += 1;
            }
            0xFF97 => {
                home_x = i32::from(arg(1));
                x = home_x;
                i += 2;
            }
            0xFF98 => {
                home_y = i32::from(arg(1));
                y = home_y;
                i += 2;
            }
            0xFF92 => {
                x += i32::from(arg(1));
                i += 2;
            }
            0xFF94 => {
                x -= i32::from(arg(1));
                i += 2;
            }
            0xFF93 => {
                y += i32::from(arg(1));
                i += 2;
            }
            0xFF95 => {
                y -= i32::from(arg(1));
                i += 2;
            }
            // Sounds, other-slot control, Brandon: known, not carried
            // into a sheet. Skipped with their arguments so the stream
            // stays aligned.
            0xFFB0 | 0xFF96 | 0xFFA7 | 0xFFAD | 0xFFAE | 0xFFAF => i += 2,
            0xFFB4 => i += 3,
            0xFF99 | 0xFF9A | 0xFFAA | 0xFFAB | 0xFFB1 | 0xFFB2 => i += 1,
            // An op this does not know: stop reading rather than emit
            // frames from misaligned words.
            _ => break,
        }
    }
    // A body of nothing but the jump is real and common: the header's
    // sprite shown forever - a cave's variant decoration. One frame
    // says exactly that.
    if anim.frames.is_empty()
        && let Some(sprite) = init_sprite
    {
        anim.frames.push(Frame {
            shape: sprite,
            x: home_x,
            y: home_y,
            flipped: false,
            ticks: wait,
        });
    }
    anim
}

/// The one rate a frame list flattens to, in frames a second.
pub fn fps(frames: &[Frame]) -> f32 {
    if frames.is_empty() {
        return 8.0;
    }
    let total: u32 = frames.iter().map(|f| f.ticks).sum();
    let average = (total as f32 / frames.len() as f32).max(1.0);
    (60.0 / average).clamp(1.0, 30.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(list: &[u16]) -> Vec<u8> {
        list.iter().flat_map(|w| w.to_le_bytes()).collect()
    }

    #[test]
    fn shapes_and_a_looping_two_frame_script_come_out() {
        let inner = [
            0xFF82, //
            0xFF84, 0, 2, 140, 3, 20, 1, 5, 140, 3, 20, 0xFF85, //
            0xFF86, 0xFF9F, 100, 0xFFA0, 50, //
            0xFF88, 0, 0xFF89, 100, 50, 0xFF8A, 12, 0xFF8C, //
            0xFF88, 1, 0xFF8A, 12, 0xFF8C, 0xFF8B, 0xFF87, //
            0xFF83,
        ];
        let mut data = vec![(inner.len() as u16 * 2), 0u16];
        data[1] = 0; // placeholder; length word then section
        let mut bytes = ((inner.len() * 2) as u16).to_le_bytes().to_vec();
        bytes.extend(words(&inner));
        let anims = parse(&bytes, 0);
        let _ = data;
        assert_eq!(anims.shapes.len(), 2);
        assert_eq!(
            anims.shape(0),
            Some(&Shape { id: 0, x: 16, y: 140, w: 24, h: 20 })
        );
        assert_eq!(anims.anims.len(), 1);
        let anim = &anims.anims[0];
        assert!(anim.looping);
        assert_eq!(anim.frames.len(), 2);
        assert_eq!((anim.frames[0].x, anim.frames[0].y), (100, 50));
        assert_eq!(anim.frames[0].ticks, 12);
        // The second frame keeps the position the first set.
        assert_eq!((anim.frames[1].x, anim.frames[1].y), (100, 50));
        assert!((fps(&anim.frames) - 5.0).abs() < 0.01);
    }

    #[test]
    fn a_finite_loop_unrolls_and_does_not_mark_looping() {
        let inner = [
            0xFF81, 0xFF84, 0, 0, 0, 1, 1, 0xFF85, //
            0xFF86, 0xFF8E, 3, 0xFF88, 0, 0xFF8C, 0xFF8F, 0xFF87, 0xFF83,
        ];
        let mut bytes = ((inner.len() * 2) as u16).to_le_bytes().to_vec();
        bytes.extend(words(&inner));
        let anims = parse(&bytes, 0);
        assert_eq!(anims.anims[0].frames.len(), 3);
        assert!(!anims.anims[0].looping);
    }

    #[test]
    fn a_header_only_script_shows_its_sprite_forever() {
        let inner = [
            0xFF82u16, 0xFF84, 1, 4, 139, 5, 14, 0xFF85, //
            0xFF86, 0xFF9B, 1, 0xFF9F, 128, 0xFFA0, 21, 0xFF8B, 0xFF87, 0xFF83,
        ];
        let mut bytes = ((inner.len() * 2) as u16).to_le_bytes().to_vec();
        bytes.extend(words(&inner));
        let anims = parse(&bytes, 0);
        let anim = &anims.anims[0];
        assert!(anim.looping);
        assert_eq!(anim.frames.len(), 1);
        assert_eq!(
            (anim.frames[0].shape, anim.frames[0].x, anim.frames[0].y),
            (1, 128, 21)
        );
    }

    #[test]
    fn garbage_yields_nothing_rather_than_frames() {
        assert_eq!(parse(&[1, 2, 3], 0), SceneAnims::default());
        let inner = [0xFF81u16, 0xFF86, 0x1234, 0xFF87, 0xFF83];
        let mut bytes = ((inner.len() * 2) as u16).to_le_bytes().to_vec();
        bytes.extend(words(&inner));
        assert!(parse(&bytes, 0).anims[0].frames.is_empty());
    }
}
