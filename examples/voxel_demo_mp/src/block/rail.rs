//! W127 - rail shape derivation (MC `RailShape` / `RailState`).
//!
//! The engine stores NO per-rail state: a rail's shape (straight /
//! curve / ascending) is a pure function of which neighbouring
//! cells hold rails, recomputed on the main thread whenever a chunk
//! re-meshes (`Voxel::rail_state_snapshot`). That mirrors MC's
//! behaviour (neighbour updates re-bend rails) without a blockstate
//! store, save format, or net message - every peer derives the same
//! shapes from the same world.
//!
//! Shape codes (matching MC `RailShape` order):
//! 0 north_south, 1 east_west, 2 ascending_east, 3 ascending_west,
//! 4 ascending_north, 5 ascending_south, 6 south_east, 7 south_west,
//! 8 north_west, 9 north_east.
//! (north = -Z, south = +Z, east = +X, west = -X.)

use super::{ACTIVATOR_RAIL_ID, DETECTOR_RAIL_ID, POWERED_RAIL_ID, RAIL_ID};

pub(crate) const SHAPE_NS: u8 = 0;
pub(crate) const SHAPE_EW: u8 = 1;
pub(crate) const SHAPE_ASC_E: u8 = 2;
pub(crate) const SHAPE_ASC_W: u8 = 3;
pub(crate) const SHAPE_ASC_N: u8 = 4;
pub(crate) const SHAPE_ASC_S: u8 = 5;
pub(crate) const SHAPE_SE: u8 = 6;
pub(crate) const SHAPE_SW: u8 = 7;
pub(crate) const SHAPE_NW: u8 = 8;
pub(crate) const SHAPE_NE: u8 = 9;

/// True when `id` is any rail block.
pub(crate) fn is_rail_block(id: u16) -> bool {
    matches!(
        id,
        RAIL_ID | POWERED_RAIL_ID | DETECTOR_RAIL_ID | ACTIVATOR_RAIL_ID
    )
}

/// Only the plain rail can bend into curves (MC: powered/detector/
/// activator rails are straight-or-ascending only).
pub(crate) fn can_curve(id: u16) -> bool {
    id == RAIL_ID
}

/// W130: rails that take redstone power and show an `_on` texture.
/// (The detector powers from carts, not redstone - no carts yet.)
pub(crate) fn is_powerable(id: u16) -> bool {
    matches!(id, POWERED_RAIL_ID | ACTIVATOR_RAIL_ID)
}

/// Neighbour connection level: how a rail in the adjacent column
/// links to this one.
#[derive(Clone, Copy, PartialEq)]
enum Link {
    None,
    /// Rail at the same Y (or one BELOW - we slope down toward it,
    /// which doesn't change this cell's shape).
    Level,
    /// Rail one ABOVE - this cell ascends toward it.
    Above,
}

/// Compute the shape for the rail at `pos`. `rail_at(p)` reports
/// whether the world holds any rail block at `p` (the caller wraps
/// its World / chunk access).
///
/// Connection scan mirrors MC: for each horizontal direction, a
/// neighbouring rail counts at the same level, one above (ascend),
/// or one below (the LOWER rail keeps its level shape; the upper
/// one ascends). Two opposite links = straight; two perpendicular
/// links = a curve (plain rail only); one link = straight along it;
/// zero = north-south.
pub(crate) fn compute_shape(
    block_id: u16,
    pos: [i32; 3],
    rail_at: impl Fn([i32; 3]) -> bool,
) -> u8 {
    // Directions in fixed order: E(+X), W(-X), N(-Z), S(+Z).
    const DIRS: [[i32; 3]; 4] = [[1, 0, 0], [-1, 0, 0], [0, 0, -1], [0, 0, 1]];
    let link = |d: [i32; 3]| -> Link {
        let n = [pos[0] + d[0], pos[1], pos[2] + d[2]];
        if rail_at(n) || rail_at([n[0], n[1] - 1, n[2]]) {
            return Link::Level;
        }
        if rail_at([n[0], n[1] + 1, n[2]]) {
            return Link::Above;
        }
        Link::None
    };
    let e = link(DIRS[0]);
    let w = link(DIRS[1]);
    let n = link(DIRS[2]);
    let s = link(DIRS[3]);
    let connected = [e, w, n, s].iter().filter(|l| **l != Link::None).count();

    // Ascending wins for a straight run: a single Above neighbour
    // tilts the rail toward it (MC prioritises the slope).
    if e == Link::Above && w != Link::Above {
        return SHAPE_ASC_E;
    }
    if w == Link::Above && e != Link::Above {
        return SHAPE_ASC_W;
    }
    if n == Link::Above && s != Link::Above {
        return SHAPE_ASC_N;
    }
    if s == Link::Above && n != Link::Above {
        return SHAPE_ASC_S;
    }

    let ex = e != Link::None;
    let wx = w != Link::None;
    let nx = n != Link::None;
    let sx = s != Link::None;
    // Straight pairs first.
    if ex && wx && !nx && !sx {
        return SHAPE_EW;
    }
    if nx && sx && !ex && !wx {
        return SHAPE_NS;
    }
    // Perpendicular pair -> curve (plain rail only).
    if can_curve(block_id) && connected >= 2 {
        if sx && ex {
            return SHAPE_SE;
        }
        if sx && wx {
            return SHAPE_SW;
        }
        if nx && wx {
            return SHAPE_NW;
        }
        if nx && ex {
            return SHAPE_NE;
        }
    }
    // Single link -> straight along it.
    if ex || wx {
        return SHAPE_EW;
    }
    if nx || sx {
        return SHAPE_NS;
    }
    SHAPE_NS
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn world(rails: &[[i32; 3]]) -> HashSet<[i32; 3]> {
        rails.iter().copied().collect()
    }

    #[test]
    fn lone_rail_defaults_north_south() {
        let w = world(&[[0, 64, 0]]);
        assert_eq!(compute_shape(RAIL_ID, [0, 64, 0], |p| w.contains(&p)), SHAPE_NS);
    }

    #[test]
    fn straight_line_east_west() {
        let w = world(&[[-1, 64, 0], [0, 64, 0], [1, 64, 0]]);
        assert_eq!(compute_shape(RAIL_ID, [0, 64, 0], |p| w.contains(&p)), SHAPE_EW);
        // End pieces align with their single neighbour too.
        assert_eq!(compute_shape(RAIL_ID, [1, 64, 0], |p| w.contains(&p)), SHAPE_EW);
    }

    #[test]
    fn l_bend_makes_a_curve_for_plain_rail_only() {
        // Neighbours to the south (+Z) and east (+X) -> south_east curve.
        let w = world(&[[0, 64, 0], [1, 64, 0], [0, 64, 1]]);
        assert_eq!(compute_shape(RAIL_ID, [0, 64, 0], |p| w.contains(&p)), SHAPE_SE);
        // Powered rail can't curve: falls back to a straight pick.
        let got = compute_shape(POWERED_RAIL_ID, [0, 64, 0], |p| w.contains(&p));
        assert!(got == SHAPE_EW || got == SHAPE_NS, "powered must stay straight, got {got}");
    }

    #[test]
    fn all_four_curves_resolve() {
        for (others, want) in [
            ([[1, 64, 0], [0, 64, 1]], SHAPE_SE),
            ([[-1, 64, 0], [0, 64, 1]], SHAPE_SW),
            ([[-1, 64, 0], [0, 64, -1]], SHAPE_NW),
            ([[1, 64, 0], [0, 64, -1]], SHAPE_NE),
        ] {
            let mut cells = vec![[0, 64, 0]];
            cells.extend_from_slice(&others);
            let w = world(&cells);
            assert_eq!(
                compute_shape(RAIL_ID, [0, 64, 0], |p| w.contains(&p)),
                want,
            );
        }
    }

    #[test]
    fn neighbour_one_above_ascends_toward_it() {
        let w = world(&[[0, 64, 0], [1, 65, 0]]);
        assert_eq!(
            compute_shape(RAIL_ID, [0, 64, 0], |p| w.contains(&p)),
            SHAPE_ASC_E,
        );
        // The upper rail sees the lower one as Level (down-slopes
        // don't change the upper cell's shape).
        assert_eq!(
            compute_shape(RAIL_ID, [1, 65, 0], |p| w.contains(&p)),
            SHAPE_EW,
        );
    }
}
