//! W67.1 - redstone signal logic (MVP).
//!
//! A bounded BFS power solver over the connected redstone component
//! around an edit. Sources emit power 15 into adjacent wire; wire
//! (`redstone_dust`) carries it, losing 1 per step; lamps light when
//! a neighbouring source or powered wire touches them.
//!
//! Implemented (W67.1):
//! - sources: redstone_block (constant), lever (toggle via RMB),
//!   redstone_torch (constant - the invert-on-input behaviour is a
//!   follow-up).
//! - wire: redstone_dust, power 0..=15 with per-step decay.
//! - consumer: redstone_lamp <-> lit_redstone_lamp block swap (the
//!   lit form also emits light through the existing emission table).
//!
//! Follow-ups: button / pressure-plate pulses, repeater delay + lock,
//! comparator modes, pistons, wire-brightness render, and strong /
//! weak block powering through solid blocks.
//!
//! MC mirror: `net/minecraft/world/level/block/RedStoneWireBlock` +
//! `RedstoneTorchBlock` + `LeverBlock`; the per-cell power solve
//! mirrors `RedStoneWireBlock.calculateTargetStrength`.

use std::collections::{HashMap, HashSet, VecDeque};

use infengine_voxel::World;

use crate::block::{
    LEVER_ID, LIT_REDSTONE_LAMP_ID, REDSTONE_BLOCK_ID, REDSTONE_DUST_ID,
    REDSTONE_LAMP_ID, REDSTONE_TORCH_ID,
};
use crate::net::NetRole;
use crate::{BlockId, Voxel};

/// Safety cap on the flooded component size so a pathological build
/// can't stall a frame.
const MAX_COMPONENT: usize = 8192;

/// Per-wire power level (0..=15) keyed by cell.
type PowerMap = HashMap<[i32; 3], u8>;
/// Lamp cells paired with their desired lit state.
type LampStates = Vec<([i32; 3], bool)>;

/// The 6 orthogonal neighbours of a cell.
fn neighbors6(c: [i32; 3]) -> [[i32; 3]; 6] {
    [
        [c[0] + 1, c[1], c[2]],
        [c[0] - 1, c[1], c[2]],
        [c[0], c[1] + 1, c[2]],
        [c[0], c[1] - 1, c[2]],
        [c[0], c[1], c[2] + 1],
        [c[0], c[1], c[2] - 1],
    ]
}

/// True for the wire block (`redstone_dust`).
fn is_wire(id: u16) -> bool {
    id == REDSTONE_DUST_ID
}

/// True for either lamp state.
fn is_lamp(id: u16) -> bool {
    id == REDSTONE_LAMP_ID || id == LIT_REDSTONE_LAMP_ID
}

/// Power a source cell emits, or `None` if `cell` isn't a source.
/// Levers only emit while toggled on.
fn source_power(world: &World, lever_on: &HashSet<[i32; 3]>, cell: [i32; 3]) -> Option<u8> {
    match world.get(cell).0 {
        x if x == REDSTONE_BLOCK_ID => Some(15),
        x if x == REDSTONE_TORCH_ID => Some(15),
        x if x == LEVER_ID => lever_on.contains(&cell).then_some(15),
        _ => None,
    }
}

/// True for any block that participates in a redstone network (wire,
/// source, or lamp). Drives the component flood + the place / break
/// re-solve triggers.
pub(crate) fn is_redstone_block(id: u16) -> bool {
    is_wire(id)
        || is_lamp(id)
        || id == REDSTONE_BLOCK_ID
        || id == REDSTONE_TORCH_ID
        || id == LEVER_ID
}

/// Solve the redstone component reachable from `seeds`. Returns the
/// per-wire power map plus each lamp cell with its desired powered
/// state. Pure over the world snapshot + lever set, so it unit-tests
/// without a live `Voxel`.
pub(crate) fn solve(
    world: &World,
    lever_on: &HashSet<[i32; 3]>,
    seeds: &[[i32; 3]],
) -> (PowerMap, LampStates) {
    // 1. Flood the connected component of redstone cells from seeds.
    let mut comp: HashSet<[i32; 3]> = HashSet::new();
    let mut q: VecDeque<[i32; 3]> = VecDeque::new();
    for &s in seeds {
        if is_redstone_block(world.get(s).0) && comp.insert(s) {
            q.push_back(s);
        }
    }
    while let Some(c) = q.pop_front() {
        if comp.len() >= MAX_COMPONENT {
            break;
        }
        for n in neighbors6(c) {
            if is_redstone_block(world.get(n).0) && comp.insert(n) {
                q.push_back(n);
            }
        }
    }
    // 2. Power BFS: every source pushes 15 into its adjacent wire,
    // then wire carries it outward losing 1 per step.
    let mut power: HashMap<[i32; 3], u8> = HashMap::new();
    let mut pq: VecDeque<([i32; 3], u8)> = VecDeque::new();
    for &c in &comp {
        if source_power(world, lever_on, c).is_some() {
            for n in neighbors6(c) {
                if is_wire(world.get(n).0)
                    && 15 > power.get(&n).copied().unwrap_or(0)
                {
                    power.insert(n, 15);
                    pq.push_back((n, 15));
                }
            }
        }
    }
    while let Some((c, p)) = pq.pop_front() {
        if p == 0 {
            continue;
        }
        for n in neighbors6(c) {
            if is_wire(world.get(n).0) {
                let np = p - 1;
                if np > power.get(&n).copied().unwrap_or(0) {
                    power.insert(n, np);
                    pq.push_back((n, np));
                }
            }
        }
    }
    // 3. A lamp lights when a neighbouring source or powered wire
    // touches it.
    let mut lamps: Vec<([i32; 3], bool)> = Vec::new();
    for &c in &comp {
        if is_lamp(world.get(c).0) {
            let powered = neighbors6(c).iter().any(|&n| {
                source_power(world, lever_on, n).is_some()
                    || power.get(&n).copied().unwrap_or(0) > 0
            });
            lamps.push((c, powered));
        }
    }
    (power, lamps)
}

/// Re-solve the redstone network around `around` and apply lamp
/// swaps. Host-only. Seeds the flood with `around` plus its
/// neighbours so a just-broken cell still re-evaluates the wires it
/// used to touch.
pub(crate) fn resolve(vx: &mut Voxel, around: [i32; 3]) {
    if matches!(vx.net, NetRole::Client { .. }) {
        return;
    }
    let mut seeds = vec![around];
    seeds.extend_from_slice(&neighbors6(around));
    let (_power, lamps) = solve(&vx.world, &vx.lever_on, &seeds);
    for (cell, powered) in lamps {
        let want = if powered {
            LIT_REDSTONE_LAMP_ID
        } else {
            REDSTONE_LAMP_ID
        };
        if vx.world.get(cell).0 != want {
            crate::world::edit::emit(vx, cell, BlockId(want));
        }
    }
}

/// Toggle a lever's on/off state + re-solve its network. Host-only.
pub(crate) fn toggle_lever(vx: &mut Voxel, cell: [i32; 3]) {
    if matches!(vx.net, NetRole::Client { .. }) {
        return;
    }
    if vx.world.get(cell).0 != LEVER_ID {
        return;
    }
    if !vx.lever_on.remove(&cell) {
        vx.lever_on.insert(cell);
    }
    resolve(vx, cell);
}

#[cfg(test)]
mod tests {
    use super::*;
    use infengine_voxel::{Chunk, World};

    fn test_world() -> World {
        let mut w = World::new();
        for cx in 0..=1 {
            for cy in 0..=1 {
                for cz in 0..=1 {
                    w.insert_chunk([cx, cy, cz], Chunk::empty());
                }
            }
        }
        w
    }

    #[test]
    fn block_source_powers_adjacent_wire_to_15_then_decays() {
        let mut w = test_world();
        // redstone_block at x=5, wire running +x.
        w.set([5, 8, 5], BlockId(REDSTONE_BLOCK_ID));
        for x in 6..=10 {
            w.set([x, 8, 5], BlockId(REDSTONE_DUST_ID));
        }
        let (power, _) = solve(&w, &HashSet::new(), &[[6, 8, 5]]);
        assert_eq!(power.get(&[6, 8, 5]).copied(), Some(15));
        assert_eq!(power.get(&[7, 8, 5]).copied(), Some(14));
        assert_eq!(power.get(&[10, 8, 5]).copied(), Some(11));
    }

    #[test]
    fn lever_only_powers_when_on() {
        let mut w = test_world();
        w.set([5, 8, 5], BlockId(LEVER_ID));
        w.set([6, 8, 5], BlockId(REDSTONE_DUST_ID));
        // Off: no power.
        let (off, _) = solve(&w, &HashSet::new(), &[[6, 8, 5]]);
        assert_eq!(off.get(&[6, 8, 5]).copied().unwrap_or(0), 0);
        // On: 15.
        let mut on_set = HashSet::new();
        on_set.insert([5, 8, 5]);
        let (on, _) = solve(&w, &on_set, &[[6, 8, 5]]);
        assert_eq!(on.get(&[6, 8, 5]).copied(), Some(15));
    }

    #[test]
    fn lamp_lights_next_to_powered_wire() {
        let mut w = test_world();
        w.set([5, 8, 5], BlockId(REDSTONE_BLOCK_ID));
        w.set([6, 8, 5], BlockId(REDSTONE_DUST_ID));
        w.set([7, 8, 5], BlockId(REDSTONE_LAMP_ID)); // adjacent to wire
        let (_p, lamps) = solve(&w, &HashSet::new(), &[[6, 8, 5]]);
        assert_eq!(lamps.iter().find(|(c, _)| *c == [7, 8, 5]).map(|(_, on)| *on), Some(true));
    }

    #[test]
    fn lamp_dark_without_power() {
        let mut w = test_world();
        // Lamp next to unpowered wire (no source).
        w.set([6, 8, 5], BlockId(REDSTONE_DUST_ID));
        w.set([7, 8, 5], BlockId(REDSTONE_LAMP_ID));
        let (_p, lamps) = solve(&w, &HashSet::new(), &[[7, 8, 5]]);
        assert_eq!(lamps.iter().find(|(c, _)| *c == [7, 8, 5]).map(|(_, on)| *on), Some(false));
    }
}
