//! W143 - stateful rail shapes (the real MC `RailState` port).
//!
//! Until now rail shapes were a pure function of the neighbourhood,
//! recomputed on every remesh (block/rail.rs `compute_shape`). That
//! made every junction re-bend whenever ANY rail appeared nearby -
//! in MC a rail's shape is part of its block state: it is decided
//! when the rail is PLACED, and a neighbour with two established
//! connections is "full" and never re-bends (`RailState.canConnectTo`).
//!
//! This module is the pure logic: given read access to the rail
//! world (which cells hold rails + their stored shapes), compute the
//! shape for a newly placed rail and the minimal set of neighbour
//! reshapes (MC `RailState.place` + `connectTo`). The caller applies
//! the results to the live maps / save / net.
//!
//! Direction convention matches block/rail.rs: north = -Z,
//! south = +Z, east = +X, west = -X.

use super::rail::{
    can_curve, SHAPE_ASC_E, SHAPE_ASC_N, SHAPE_ASC_S, SHAPE_ASC_W, SHAPE_EW, SHAPE_NE, SHAPE_NS,
    SHAPE_NW, SHAPE_SE, SHAPE_SW,
};

/// Read access to the rail world. Implemented over the live World +
/// `rail_shapes` map by the runtime, and over plain HashMaps in tests.
pub(crate) trait RailAccess {
    /// Block id of the rail at `p`, `None` when the cell holds no rail.
    fn rail_id(&self, p: [i32; 3]) -> Option<u16>;
    /// Stored shape of the rail at `p` (placed-time state). May be
    /// `None` for legacy rails from pre-W143 saves; those fall back
    /// to the stateless derivation at remesh and behave as before
    /// until first touched.
    fn shape(&self, p: [i32; 3]) -> Option<u8>;
}

/// The runtime adapter: live voxel world + the stored shape map.
pub(crate) struct LiveRailAccess<'a> {
    pub(crate) world: &'a infengine_voxel::World,
    pub(crate) shapes: &'a std::collections::HashMap<[i32; 3], u8>,
}

impl RailAccess for LiveRailAccess<'_> {
    fn rail_id(&self, p: [i32; 3]) -> Option<u16> {
        let id = self.world.get(p).0;
        super::rail::is_rail_block(id).then_some(id)
    }
    fn shape(&self, p: [i32; 3]) -> Option<u8> {
        self.shapes.get(&p).copied()
    }
}

/// Horizontal directions in fixed order: E(+X), W(-X), N(-Z), S(+Z).
const DIR_E: usize = 0;
const DIR_W: usize = 1;
const DIR_N: usize = 2;
const DIR_S: usize = 3;
const DIR_VEC: [[i32; 3]; 4] = [[1, 0, 0], [-1, 0, 0], [0, 0, -1], [0, 0, 1]];

fn opposite(d: usize) -> usize {
    match d {
        DIR_E => DIR_W,
        DIR_W => DIR_E,
        DIR_N => DIR_S,
        _ => DIR_N,
    }
}

/// The two horizontal directions a shape connects toward.
fn shape_dirs(shape: u8) -> [usize; 2] {
    match shape & 0x7F {
        SHAPE_EW | SHAPE_ASC_E | SHAPE_ASC_W => [DIR_E, DIR_W],
        SHAPE_SE => [DIR_S, DIR_E],
        SHAPE_SW => [DIR_S, DIR_W],
        SHAPE_NW => [DIR_N, DIR_W],
        SHAPE_NE => [DIR_N, DIR_E],
        // NS + ascending NS + anything unknown.
        _ => [DIR_N, DIR_S],
    }
}

/// Straight shape along `d`, optionally ascending toward it.
fn straight_toward(d: usize, ascending: bool) -> u8 {
    match (d, ascending) {
        (DIR_E, false) | (DIR_W, false) => SHAPE_EW,
        (DIR_E, true) => SHAPE_ASC_E,
        (DIR_W, true) => SHAPE_ASC_W,
        (DIR_N, true) => SHAPE_ASC_N,
        (DIR_S, true) => SHAPE_ASC_S,
        _ => SHAPE_NS,
    }
}

/// Curve connecting the two (perpendicular) directions.
fn curve_for(a: usize, b: usize) -> u8 {
    let has = |d: usize| a == d || b == d;
    if has(DIR_S) && has(DIR_E) {
        SHAPE_SE
    } else if has(DIR_S) && has(DIR_W) {
        SHAPE_SW
    } else if has(DIR_N) && has(DIR_W) {
        SHAPE_NW
    } else {
        SHAPE_NE
    }
}

/// Resolve the actual rail cell a connection toward `d` lands on:
/// the neighbour column at the same level, one above (we ascend to
/// it) or one below (it ascends to us). MC `getRail` checks the
/// same three cells.
fn rail_toward(w: &impl RailAccess, pos: [i32; 3], d: usize) -> Option<[i32; 3]> {
    let v = DIR_VEC[d];
    let base = [pos[0] + v[0], pos[1] + v[1], pos[2] + v[2]];
    [base, [base[0], base[1] + 1, base[2]], [base[0], base[1] - 1, base[2]]]
        .into_iter()
        .find(|&cand| w.rail_id(cand).is_some())
}

/// Does the rail at `n` point toward the column of `from`? (Y is
/// ignored - MC connections match across the +-1 slope offset.)
fn points_at(w: &impl RailAccess, n: [i32; 3], from: [i32; 3]) -> bool {
    let Some(shape) = stored_or_derived_shape(w, n) else {
        return false;
    };
    shape_dirs(shape).iter().any(|&d| {
        let v = DIR_VEC[d];
        n[0] + v[0] == from[0] && n[2] + v[2] == from[2]
    })
}

/// Stored shape, or the legacy stateless derivation for pre-W143
/// rails (so old saves keep their look until a placement near them
/// stores real state).
pub(crate) fn stored_or_derived_shape(w: &impl RailAccess, p: [i32; 3]) -> Option<u8> {
    let id = w.rail_id(p)?;
    Some(match w.shape(p) {
        Some(s) => s & 0x7F,
        None => super::rail::compute_shape(id, p, |q| w.rail_id(q).is_some()),
    })
}

/// Established (mutual) connections of the rail at `p`: its shape
/// endpoints that hold a rail pointing back. MC calls these the
/// hard connections after `removeSoftConnections`.
fn hard_connections(w: &impl RailAccess, p: [i32; 3]) -> Vec<(usize, [i32; 3])> {
    let Some(shape) = stored_or_derived_shape(w, p) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for d in shape_dirs(shape) {
        if let Some(n) = rail_toward(w, p, d)
            && points_at(w, n, p)
        {
            out.push((d, n));
        }
    }
    out
}

/// MC `RailState.canConnectTo`: a neighbour accepts a new connection
/// from `from` when it already points there, or when it still has a
/// free end (fewer than two established connections).
fn can_connect(w: &impl RailAccess, neighbor: [i32; 3], from: [i32; 3]) -> bool {
    points_at(w, neighbor, from) || hard_connections(w, neighbor).len() < 2
}

/// One neighbour reshape produced by a placement.
#[derive(Debug, PartialEq)]
pub(crate) struct ShapeUpdate {
    pub(crate) pos: [i32; 3],
    pub(crate) shape: u8,
}

/// Result of placing a rail: the new rail's shape plus the minimal
/// neighbour reshapes (only rails with a free end ever change).
#[derive(Debug)]
pub(crate) struct PlaceResult {
    pub(crate) shape: u8,
    pub(crate) neighbor_updates: Vec<ShapeUpdate>,
}

/// MC `RailState.place`: decide the shape of the rail being placed
/// at `pos` and reshape the neighbours it connects to. `player_axis_x`
/// is the placement fallback when no neighbour connects (MC uses the
/// player's horizontal facing axis: looking east/west places an
/// east_west rail).
pub(crate) fn place_rail(
    w: &impl RailAccess,
    pos: [i32; 3],
    id: u16,
    player_axis_x: bool,
) -> PlaceResult {
    // Connectable neighbour per direction (exists AND can accept us).
    let mut conn: [Option<[i32; 3]>; 4] = [None; 4];
    for (d, slot) in conn.iter_mut().enumerate() {
        if let Some(n) = rail_toward(w, pos, d)
            && can_connect(w, n, pos)
        {
            *slot = Some(n);
        }
    }
    let (e, wst, n, s) = (
        conn[DIR_E].is_some(),
        conn[DIR_W].is_some(),
        conn[DIR_N].is_some(),
        conn[DIR_S].is_some(),
    );

    // Shape choice mirrors the vanilla if-chain (EW overrides NS,
    // exact perpendicular pairs become curves on plain rail).
    let mut shape = if player_axis_x { SHAPE_EW } else { SHAPE_NS };
    if n || s {
        shape = SHAPE_NS;
    }
    if e || wst {
        shape = SHAPE_EW;
    }
    if can_curve(id) {
        if s && e && !n && !wst {
            shape = SHAPE_SE;
        } else if s && wst && !n && !e {
            shape = SHAPE_SW;
        } else if n && wst && !s && !e {
            shape = SHAPE_NW;
        } else if n && e && !s && !wst {
            shape = SHAPE_NE;
        }
    }
    // Ascending fix-up for straight runs: a connected neighbour one
    // above tilts the new rail toward it.
    let above = |d: usize| conn[d].is_some_and(|c| c[1] == pos[1] + 1);
    if shape == SHAPE_NS {
        if above(DIR_N) {
            shape = SHAPE_ASC_N;
        } else if above(DIR_S) {
            shape = SHAPE_ASC_S;
        }
    } else if shape == SHAPE_EW {
        if above(DIR_E) {
            shape = SHAPE_ASC_E;
        } else if above(DIR_W) {
            shape = SHAPE_ASC_W;
        }
    }

    // MC `connectTo`: every neighbour the chosen shape points at (and
    // that accepted us) gains a connection toward `pos` - reshaping
    // ONLY its free end. Full junctions were already filtered out.
    let mut neighbor_updates = Vec::new();
    for d in shape_dirs(shape) {
        let Some(ncell) = conn[d] else { continue };
        if let Some(up) = connect_neighbor(w, ncell, pos, d) {
            neighbor_updates.push(up);
        }
    }
    PlaceResult {
        shape,
        neighbor_updates,
    }
}

/// Reshape the neighbour at `ncell` so it points at the new rail in
/// `pos` (which lies in direction `d` from us, i.e. `opposite(d)`
/// when seen from the neighbour). Keeps the neighbour's one
/// established connection; returns `None` when its shape already
/// fits.
fn connect_neighbor(
    w: &impl RailAccess,
    ncell: [i32; 3],
    pos: [i32; 3],
    d: usize,
) -> Option<ShapeUpdate> {
    let toward_new = opposite(d);
    let cur = stored_or_derived_shape(w, ncell)?;
    // Already points at us - nothing to do.
    if shape_dirs(cur).contains(&toward_new) && {
        // ...unless the ascend flag toward us is wrong (we sit above).
        let want_asc = pos[1] == ncell[1] + 1;
        let is_asc_toward = cur == straight_toward(toward_new, true);
        !want_asc || is_asc_toward || !matches!(cur & 0x7F, SHAPE_NS | SHAPE_EW)
    } {
        return None;
    }
    // The connection the neighbour keeps: its first established one
    // that isn't toward the new rail's column.
    let kept = hard_connections(w, ncell)
        .into_iter()
        .find(|(kd, kc)| *kd != toward_new && !(kc[0] == pos[0] && kc[2] == pos[2]));
    let new_above = pos[1] == ncell[1] + 1;
    let nid = w.rail_id(ncell)?;
    let shape = match kept {
        None => straight_toward(toward_new, new_above),
        Some((kd, kc)) => {
            if kd == opposite(toward_new) {
                // Opposite pair: straight; ascend toward whichever
                // connected rail sits one above (new rail first).
                if new_above {
                    straight_toward(toward_new, true)
                } else if kc[1] == ncell[1] + 1 {
                    straight_toward(kd, true)
                } else {
                    straight_toward(toward_new, false)
                }
            } else if can_curve(nid) && !new_above && kc[1] <= ncell[1] {
                curve_for(kd, toward_new)
            } else {
                // Non-curving rail (or a slope is involved): align to
                // the new connection, vanilla's straight fallback.
                straight_toward(toward_new, new_above)
            }
        }
    };
    if shape == cur {
        None
    } else {
        Some(ShapeUpdate { pos: ncell, shape })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::{POWERED_RAIL_ID, RAIL_ID};
    use std::collections::HashMap;

    /// Test world: every rail has an explicit stored shape, exactly
    /// like a post-W143 save.
    struct W(HashMap<[i32; 3], (u16, Option<u8>)>);
    impl RailAccess for W {
        fn rail_id(&self, p: [i32; 3]) -> Option<u16> {
            self.0.get(&p).map(|e| e.0)
        }
        fn shape(&self, p: [i32; 3]) -> Option<u8> {
            self.0.get(&p).and_then(|e| e.1)
        }
    }
    fn world(cells: &[([i32; 3], u8)]) -> W {
        W(cells
            .iter()
            .map(|&(p, s)| (p, (RAIL_ID, Some(s))))
            .collect())
    }

    #[test]
    fn lone_rail_takes_player_axis() {
        let w = world(&[]);
        assert_eq!(place_rail(&w, [0, 64, 0], RAIL_ID, true).shape, SHAPE_EW);
        assert_eq!(place_rail(&w, [0, 64, 0], RAIL_ID, false).shape, SHAPE_NS);
    }

    #[test]
    fn extends_a_line_and_reshapes_only_the_free_end() {
        // Existing EW pair: (0) - (1). Place at (2): line extends.
        let w = world(&[([0, 64, 0], SHAPE_EW), ([1, 64, 0], SHAPE_EW)]);
        let r = place_rail(&w, [2, 64, 0], RAIL_ID, false);
        assert_eq!(r.shape, SHAPE_EW);
        // (1) already points E+W - no update needed.
        assert!(r.neighbor_updates.is_empty(), "{:?}", r.neighbor_updates);
    }

    #[test]
    fn corner_place_bends_the_end_rail_into_a_curve() {
        // EW line ending at (1,0). New rail south of the end.
        let w = world(&[([0, 64, 0], SHAPE_EW), ([1, 64, 0], SHAPE_EW)]);
        let r = place_rail(&w, [1, 64, 1], RAIL_ID, false);
        assert_eq!(r.shape, SHAPE_NS);
        // The end rail keeps its W connection to (0) and bends its
        // free E end south -> north_west? No: kept W + new S = SW.
        assert_eq!(
            r.neighbor_updates,
            vec![ShapeUpdate { pos: [1, 64, 0], shape: SHAPE_SW }]
        );
    }

    /// THE W143 regression: an established junction curve must NOT
    /// re-bend when a new rail shows up nearby. (Stateless W127
    /// derivation re-bent it - the user's bug report.)
    #[test]
    fn established_junction_does_not_rebend() {
        // EW line through (1..3, 0); junction at (2,0) curves into a
        // south branch (2,1)-(2,2). Junction shape SE = connects
        // S(2,1) + E(3,0): both established -> the junction is FULL.
        let w = world(&[
            ([1, 64, 0], SHAPE_EW),
            ([2, 64, 0], SHAPE_SE),
            ([3, 64, 0], SHAPE_EW),
            ([2, 64, 1], SHAPE_NS),
            ([2, 64, 2], SHAPE_NS),
        ]);
        // New rail west of the branch, diagonally next to the junction.
        let r = place_rail(&w, [1, 64, 1], RAIL_ID, false);
        // It may connect north to (1,0) (free W end? no - (1,0) E end
        // connects (2,0), W end free toward (0,0): toward us is S which
        // it does not point at, but it has < 2 hard connections -> OK).
        // It must NOT touch the junction at (2,0).
        assert!(
            r.neighbor_updates.iter().all(|u| u.pos != [2, 64, 0]),
            "junction re-bent: {:?}",
            r.neighbor_updates
        );
        // And the branch rail (2,1) is full (points N+S to existing
        // rails) - placing east of it must not bend it either.
        let r2 = place_rail(&w, [3, 64, 1], RAIL_ID, true);
        assert!(
            r2.neighbor_updates.iter().all(|u| u.pos != [2, 64, 1]),
            "full branch rail re-bent: {:?}",
            r2.neighbor_updates
        );
    }

    #[test]
    fn t_junction_third_rail_does_not_steal_connections() {
        // Straight NS rail at (0,1) fully connected (0,0)<->(0,2).
        let w = world(&[
            ([0, 64, 0], SHAPE_NS),
            ([0, 64, 1], SHAPE_NS),
            ([0, 64, 2], SHAPE_NS),
        ]);
        // Place east of the middle: middle is full -> new rail cannot
        // connect to it; with no other neighbour it takes player axis.
        let r = place_rail(&w, [1, 64, 1], RAIL_ID, true);
        assert_eq!(r.shape, SHAPE_EW);
        assert!(r.neighbor_updates.is_empty(), "{:?}", r.neighbor_updates);
    }

    #[test]
    fn slope_placement_ascends_and_lower_neighbor_tilts() {
        // Lower rail at y=64 (lone, EW). Place the new rail one above
        // to the east at y=65: it connects DOWN toward the lower rail
        // (level shape), and the lower rail tilts up toward us.
        let w = world(&[([0, 64, 0], SHAPE_EW)]);
        let r = place_rail(&w, [1, 65, 0], RAIL_ID, true);
        assert_eq!(r.shape, SHAPE_EW, "upper rail stays level");
        assert_eq!(
            r.neighbor_updates,
            vec![ShapeUpdate { pos: [0, 64, 0], shape: SHAPE_ASC_E }]
        );
        // Mirror: placing one BELOW an existing rail ascends toward it.
        let w2 = world(&[([1, 65, 0], SHAPE_EW)]);
        let r2 = place_rail(&w2, [0, 64, 0], RAIL_ID, true);
        assert_eq!(r2.shape, SHAPE_ASC_E);
    }

    #[test]
    fn powered_rail_never_curves() {
        // Perpendicular neighbours but a powered rail stays straight
        // (EW wins the vanilla if-chain).
        let w = world(&[([1, 64, 0], SHAPE_EW), ([0, 64, 1], SHAPE_NS)]);
        let r = place_rail(&w, [0, 64, 0], POWERED_RAIL_ID, false);
        assert_eq!(r.shape, SHAPE_EW);
        // And a plain rail in the same spot curves SE.
        let r2 = place_rail(&w, [0, 64, 0], RAIL_ID, false);
        assert_eq!(r2.shape, SHAPE_SE);
    }

    #[test]
    fn legacy_rails_without_stored_shape_still_connect() {
        // Pre-W143 world: rails exist but no stored shapes. The
        // derivation fallback keeps them connectable.
        let mut m = HashMap::new();
        m.insert([0i32, 64, 0], (RAIL_ID, None));
        let w = W(m);
        let r = place_rail(&w, [1, 64, 0], RAIL_ID, false);
        assert_eq!(r.shape, SHAPE_EW);
    }
}
