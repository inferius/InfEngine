//! Lava-water interaction (MC's `LiquidBlock.shouldSpreadLiquid`).
//!
//! When a lava cell ticks and has a water neighbour, the lava
//! cell converts to a solid block instead of spreading:
//!
//! - lava source + ANY water neighbour -> obsidian
//! - lava flow   + ANY water neighbour -> cobblestone
//!
//! MC source reference (decompile dir):
//! `net/minecraft/world/level/block/LiquidBlock.java::shouldSpreadLiquid`
//! (lines 219..=241 in the 26.1.2 decompile shipped under
//! `J:/Temp/template-mod-template-26.1.2/...`).
//!
//! The check runs once per lava cell tick, BEFORE normal flow
//! planning. If a boundary is detected, the planner skips the
//! regular spread step entirely - matching MC's `return false`
//! in `shouldSpreadLiquid`.
//!
//! ## Behaviour diff vs MC
//!
//! - MC also has a "lava-over-soul-soil + blue-ice = basalt"
//!   case. Neither soul-soil nor blue-ice exists in our palette,
//!   so we skip it. Easy to add later by extending
//!   `lava_boundary_solid_id`.
//! - MC iterates `POSSIBLE_FLOW_DIRECTIONS` (NSEW + UP); we use
//!   the same 5-axis set (the down face never triggers a fluid
//!   conversion in MC's `shouldSpreadLiquid`).
//! - MC plays a fizz `LevelEvent`. We drop the sound for now -
//!   the visible block swap is the primary feedback.

use std::collections::HashMap;

use infengine_voxel::World;

use crate::block::{COBBLE_ID, LAVA_ID, OBSIDIAN_ID, WATER_ID};

use super::Liquid;

/// Directions checked for a water neighbour. Mirrors MC's
/// `POSSIBLE_FLOW_DIRECTIONS = [NORTH, EAST, SOUTH, WEST, UP]`:
/// horizontal + above. The down face never converts because lava
/// pours past water on its way down (MC handles that case via the
/// regular spread path, not `shouldSpreadLiquid`).
const POSSIBLE_FLOW_DIRS: [[i32; 3]; 5] = [
    [1, 0, 0],
    [-1, 0, 0],
    [0, 0, 1],
    [0, 0, -1],
    [0, 1, 0],
];

/// If `pos` holds a lava cell adjacent to water, return the solid
/// block id it should convert to:
///   - lava source (level 0) + water neighbour -> OBSIDIAN_ID
///   - lava flow             + water neighbour -> COBBLE_ID
///
/// Returns `None` if `pos` is not a lava cell or has no water
/// neighbour. The caller is expected to call this only when
/// `liquid.block_id == LAVA_ID`, but the check is cheap enough
/// to defend against misuse.
pub(crate) fn lava_boundary_solid_id(
    world: &World,
    level_map: &HashMap<[i32; 3], u8>,
    pos: [i32; 3],
    liquid: &Liquid,
) -> Option<u16> {
    // Boundary conversion is lava-specific. Water has no symmetric
    // "becomes solid on lava contact" rule in MC - the water spreads
    // and the conversion happens on the lava cell's tick.
    if liquid.block_id != LAVA_ID {
        return None;
    }
    if world.get(pos).0 != LAVA_ID {
        return None;
    }
    let has_water = POSSIBLE_FLOW_DIRS.iter().any(|d| {
        let n = [pos[0] + d[0], pos[1] + d[1], pos[2] + d[2]];
        world.get(n).0 == WATER_ID
    });
    if !has_water {
        return None;
    }
    let lava_level = level_map.get(&pos).copied().unwrap_or(0);
    if lava_level == 0 {
        Some(OBSIDIAN_ID)
    } else {
        Some(COBBLE_ID)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::liquid::LIQUIDS;
    use infengine_voxel::{BlockId, Chunk, World};

    fn world_with_chunk() -> World {
        let mut w = World::new();
        for cx in -1..=1 {
            for cy in -1..=1 {
                for cz in -1..=1 {
                    w.insert_chunk([cx, cy, cz], Chunk::empty());
                }
            }
        }
        w
    }

    fn water() -> &'static Liquid {
        LIQUIDS.iter().find(|l| l.block_id == WATER_ID).unwrap()
    }
    fn lava() -> &'static Liquid {
        LIQUIDS.iter().find(|l| l.block_id == LAVA_ID).unwrap()
    }

    #[test]
    fn lava_source_with_water_neighbour_becomes_obsidian() {
        let mut w = world_with_chunk();
        w.set([0, 0, 0], BlockId(LAVA_ID));
        w.set([1, 0, 0], BlockId(WATER_ID));
        let mut lm = HashMap::new();
        lm.insert([0, 0, 0], 0u8); // lava source
        lm.insert([1, 0, 0], 0u8); // water source
        let res = lava_boundary_solid_id(&w, &lm, [0, 0, 0], lava());
        assert_eq!(res, Some(OBSIDIAN_ID));
    }

    #[test]
    fn lava_flow_with_water_neighbour_becomes_cobble() {
        let mut w = world_with_chunk();
        w.set([0, 0, 0], BlockId(LAVA_ID));
        w.set([1, 0, 0], BlockId(WATER_ID));
        let mut lm = HashMap::new();
        lm.insert([0, 0, 0], 2u8); // lava flow (level >= 1)
        lm.insert([1, 0, 0], 0u8);
        let res = lava_boundary_solid_id(&w, &lm, [0, 0, 0], lava());
        assert_eq!(res, Some(COBBLE_ID));
    }

    #[test]
    fn lava_with_no_water_neighbour_returns_none() {
        let mut w = world_with_chunk();
        w.set([0, 0, 0], BlockId(LAVA_ID));
        let mut lm = HashMap::new();
        lm.insert([0, 0, 0], 0u8);
        let res = lava_boundary_solid_id(&w, &lm, [0, 0, 0], lava());
        assert_eq!(res, None);
    }

    #[test]
    fn water_cell_returns_none_via_water_liquid_arg() {
        // The boundary check only fires when the caller passes the
        // lava registry entry; passing water short-circuits.
        let mut w = world_with_chunk();
        w.set([0, 0, 0], BlockId(WATER_ID));
        w.set([1, 0, 0], BlockId(LAVA_ID));
        let mut lm = HashMap::new();
        lm.insert([0, 0, 0], 0u8);
        lm.insert([1, 0, 0], 0u8);
        let res = lava_boundary_solid_id(&w, &lm, [0, 0, 0], water());
        assert_eq!(res, None);
    }

    #[test]
    fn lava_with_water_above_becomes_obsidian_source() {
        // POSSIBLE_FLOW_DIRS includes UP - water on top of a
        // lava source also triggers the conversion.
        let mut w = world_with_chunk();
        w.set([0, 0, 0], BlockId(LAVA_ID));
        w.set([0, 1, 0], BlockId(WATER_ID));
        let mut lm = HashMap::new();
        lm.insert([0, 0, 0], 0u8);
        lm.insert([0, 1, 0], 0u8);
        let res = lava_boundary_solid_id(&w, &lm, [0, 0, 0], lava());
        assert_eq!(res, Some(OBSIDIAN_ID));
    }

    #[test]
    fn lava_with_water_below_returns_none() {
        // DOWN is NOT in POSSIBLE_FLOW_DIRS - lava over water
        // pours through the normal spread path (or fizzes via
        // the flow's own tick); MC handles that case separately.
        let mut w = world_with_chunk();
        w.set([0, 0, 0], BlockId(LAVA_ID));
        w.set([0, -1, 0], BlockId(WATER_ID));
        let mut lm = HashMap::new();
        lm.insert([0, 0, 0], 0u8);
        lm.insert([0, -1, 0], 0u8);
        let res = lava_boundary_solid_id(&w, &lm, [0, 0, 0], lava());
        assert_eq!(res, None);
    }

    #[test]
    fn non_lava_cell_returns_none() {
        let mut w = world_with_chunk();
        w.set([0, 0, 0], BlockId(WATER_ID));
        w.set([1, 0, 0], BlockId(WATER_ID));
        let lm = HashMap::new();
        let res = lava_boundary_solid_id(&w, &lm, [0, 0, 0], lava());
        assert_eq!(res, None);
    }

    #[test]
    fn lava_flow_water_flow_both_become_cobble() {
        // Both cells flow (non-zero level) - the lava flow
        // converts to cobble; the water flow is unaffected
        // by THIS check.
        let mut w = world_with_chunk();
        w.set([0, 0, 0], BlockId(LAVA_ID));
        w.set([1, 0, 0], BlockId(WATER_ID));
        let mut lm = HashMap::new();
        lm.insert([0, 0, 0], 4u8);
        lm.insert([1, 0, 0], 3u8);
        let res = lava_boundary_solid_id(&w, &lm, [0, 0, 0], lava());
        assert_eq!(res, Some(COBBLE_ID));
    }
}
