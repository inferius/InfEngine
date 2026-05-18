//! Per-dimension state parking lot + active-dim swap helpers.
//!
//! Phase F: a save can hold multiple dimensions resident at once
//! (overworld + nether). The runtime keeps the ACTIVE dim's state
//! in the existing top-level `Voxel.{world,chests,furnaces,mobs,...}`
//! fields - so the hundreds of `self.world.get(...)` / `self.chests`
//! call sites stay untouched - and parks INACTIVE dim state in the
//! per-dim maps below.
//!
//! Switching dims is therefore a pointer swap:
//! 1. Move `voxel.world` (and the per-dim caches) into the parking
//!    map under `voxel.current_dim`.
//! 2. Take the target dim's state out of the parking map (mint
//!    empty if missing).
//! 3. Swap `voxel.current_dim` to the target name.
//! 4. Rewrite the global `WORLD_SEED` + `WORLDGEN` atomics from the
//!    new dim's `dim_seed(base, dim)` + worldgen profile.
//!
//! The player's pose + inventory are NOT touched here - the player
//! carries their stuff between dims (MC behaviour). Renderer mesh
//! handles ARE invalidated on switch (the active dim's chunk list
//! changes), so the caller is responsible for rebuilding meshes
//! after a swap.

use std::collections::HashMap;

use infengine_voxel::World;

use crate::block::furnace::FurnaceRuntime;
use crate::container::Container;
use crate::drops::DropItem;
use crate::liquid::schedule::LiquidSchedule;
use crate::mob_ai::MobState;

/// Canonical name of the overworld dimension. Used as the default
/// when a save record has no explicit `current_dim`.
pub(crate) const OVERWORLD: &str = "overworld";

/// Canonical name of the nether dimension. The default 2-dim save
/// (Phase F) ships overworld + nether.
#[allow(dead_code)] // first call site lands in F.6 (cross-dim teleport).
pub(crate) const NETHER: &str = "nether";

/// Per-dim mob + drop state parked while the dim is inactive.
///
/// Mirrors what `world_save::entities::DimensionEntities` carries
/// on disk, but in the live-runtime types. The applier writes
/// these out + the loader fills them back in.
#[derive(Debug, Default)]
pub(crate) struct DimEntities {
    pub mobs: Vec<MobState>,
    pub drops: Vec<DropItem>,
    pub next_mob_id: u32,
    pub next_drop_id: u32,
}

impl DimEntities {
    /// Build an empty bundle for a brand-new dim.
    pub(crate) fn empty() -> Self {
        Self {
            mobs: Vec::new(),
            drops: Vec::new(),
            next_mob_id: 1,
            next_drop_id: 1,
        }
    }
}

/// Per-dim voxel + side-state parked while the dim is inactive.
///
/// Holds the bulk world (chunks) plus every side map keyed by a
/// world voxel coord that the active-dim view tracks on `Voxel`:
/// chests, furnaces, liquid level, torch orient, snow layers, ...
///
/// The `Default` impl produces an empty world + empty side maps so
/// a never-visited dim has zero memory footprint on disk-load.
#[derive(Debug)]
pub(crate) struct DimWorld {
    pub world: World,
    pub chests: HashMap<[i32; 3], Container>,
    pub furnaces: HashMap<[i32; 3], Container>,
    pub furnace_runtimes: HashMap<[i32; 3], FurnaceRuntime>,
    pub torch_positions: std::collections::HashSet<[i32; 3]>,
    pub torch_orient: HashMap<[i32; 3], u8>,
    pub snow_positions: HashMap<[i32; 3], u8>,
    pub liquid_schedule: LiquidSchedule,
    pub liquid_clock: u64,
    pub liquid_level: HashMap<[i32; 3], u8>,
    pub liquid_positions: std::collections::HashSet<[i32; 3]>,
}

impl Default for DimWorld {
    fn default() -> Self {
        Self {
            world: World::new(),
            chests: HashMap::new(),
            furnaces: HashMap::new(),
            furnace_runtimes: HashMap::new(),
            torch_positions: std::collections::HashSet::new(),
            torch_orient: HashMap::new(),
            snow_positions: HashMap::new(),
            liquid_schedule: LiquidSchedule::new(),
            liquid_clock: 0,
            liquid_level: HashMap::new(),
            liquid_positions: std::collections::HashSet::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dim_entities_empty_starts_fresh() {
        let e = DimEntities::empty();
        assert!(e.mobs.is_empty());
        assert!(e.drops.is_empty());
        assert_eq!(e.next_mob_id, 1);
        assert_eq!(e.next_drop_id, 1);
    }

    #[test]
    fn dim_world_default_has_no_chunks() {
        let dw = DimWorld::default();
        assert_eq!(dw.world.iter_chunks().count(), 0);
        assert!(dw.chests.is_empty());
        assert!(dw.furnaces.is_empty());
        assert!(dw.torch_positions.is_empty());
        assert_eq!(dw.liquid_clock, 0);
    }

    #[test]
    fn overworld_and_nether_constants_match_world_json() {
        // Sanity-check the canonical dim names against the
        // world_save constants so a typo doesn't desync the
        // parking map keys from the on-disk dim tags.
        assert_eq!(OVERWORLD, "overworld");
        assert_eq!(NETHER, "nether");
    }
}
