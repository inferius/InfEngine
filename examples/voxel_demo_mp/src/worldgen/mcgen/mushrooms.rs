//! W372 - huge mushrooms (MC `AbstractHugeMushroomFeature` family).
//!
//! `huge_brown_mushroom` / `huge_red_mushroom` entries in the
//! vegetal lists (dark_forest_vegetation, mushroom_island_vegetation)
//! previously fell through `TreeRef::Unsupported` to the list default
//! (dark forests grew plain oaks where vanilla grows giant
//! mushrooms). Shapes per the 26.1.2 source:
//!
//! - height: `2 + nextInt(3)`, doubled on a 1-in-12 roll, +1 cap cell
//!   (the visible stem runs 4..6, rarely up to 11).
//! - brown cap: one flat 7x7 layer at the top with the corners cut
//!   (`HugeBrownMushroomFeature.makeCap`).
//! - red cap: the top three layers - the lower two are a radius-2
//!   ring with cut corners, the top a full 3x3
//!   (`HugeRedMushroomFeature.makeCap`).
//!
//! Writes go through the callers' set_log/set_leaf-style guards via
//! the closures passed in, so chunk clipping + replaceable rules
//! stay identical to trees.

use super::features::block_id_by_name;
use super::noise::SplitMix64;

/// Place one huge mushroom through a single writer closure
/// `(world_cell, block_id, is_stem)` - the caller clips to its
/// chunk and picks log-vs-leaf write rules by the flag.
pub(crate) fn place_huge_mushroom(
    rng: &mut SplitMix64,
    red: bool,
    base: [i32; 3],
    set: &mut dyn FnMut([i32; 3], u16, bool),
) {
    let Some(stem) = block_id_by_name("mushroom_stem") else {
        return;
    };
    let cap = if red {
        block_id_by_name("red_mushroom_block")
    } else {
        block_id_by_name("brown_mushroom_block")
    };
    let Some(cap) = cap else { return };

    let mut h = 4 + (rng.next_u64() % 3) as i32;
    if rng.next_u64().is_multiple_of(12) {
        h *= 2;
    }
    let (bx, by, bz) = (base[0], base[1], base[2]);
    for dy in 0..h {
        set([bx, by + dy, bz], stem, true);
    }
    if red {
        // Two ring layers + a solid 3x3 top.
        for layer in 0..2 {
            let y = by + h - 2 + layer;
            for dx in -2i32..=2 {
                for dz in -2i32..=2 {
                    let on_ring = dx.abs() == 2 || dz.abs() == 2;
                    let corner = dx.abs() == 2 && dz.abs() == 2;
                    if on_ring && !corner {
                        set([bx + dx, y, bz + dz], cap, false);
                    }
                }
            }
        }
        for dx in -1i32..=1 {
            for dz in -1i32..=1 {
                set([bx + dx, by + h, bz + dz], cap, false);
            }
        }
    } else {
        // One flat 7x7 layer with cut corners.
        for dx in -3i32..=3 {
            for dz in -3i32..=3 {
                if dx.abs() == 3 && dz.abs() == 3 {
                    continue;
                }
                set([bx + dx, by + h, bz + dz], cap, false);
            }
        }
    }
}
