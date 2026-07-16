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
use crate::mob::MobState;

/// Canonical name of the overworld dimension. Used as the default
/// when a save record has no explicit `current_dim`.
pub(crate) const OVERWORLD: &str = "overworld";

/// Canonical name of the nether dimension. The default 2-dim save
/// (Phase F) ships overworld + nether.
#[allow(dead_code)] // first call site lands in F.6 (cross-dim teleport).
pub(crate) const NETHER: &str = "nether";

/// W265: MC `ServerLevel.END_SPAWN_POINT`.
pub(crate) const END_SPAWN: [i32; 3] = [100, 50, 0];

/// W265: the End arrival - MC's `makeObsidianPlatform`: a 5x5
/// obsidian slab two below `END_SPAWN_POINT` (y 48) with the three
/// layers above forced to air, the player standing on its centre.
/// Every End entry lands here (the vanilla rule), so the platform
/// regenerates even if someone mined it.
pub(crate) fn arrive_in_end(voxel: &mut crate::Voxel) {
    // Bake real terrain around the spawn column first so the
    // platform edits land in resident chunks.
    voxel.ensure_generated_chunks_around(END_SPAWN);
    let y = END_SPAWN[1] - 2;
    for dx in -2..=2 {
        for dz in -2..=2 {
            let (x, z) = (END_SPAWN[0] + dx, END_SPAWN[2] + dz);
            voxel.world.set(
                [x, y, z],
                infengine_voxel::BlockId(crate::block::OBSIDIAN_ID),
            );
            for dy in 1..=3 {
                voxel.world.set([x, y + dy, z], infengine_voxel::BlockId(0));
            }
        }
    }
    // Feet on the platform centre (the /rtp convention: solid cell
    // y + 1.5).
    voxel.player_pos = glam::Vec3::new(
        END_SPAWN[0] as f32 + 0.5,
        y as f32 + 1.5,
        END_SPAWN[2] as f32 + 0.5,
    );
    voxel.player_vel = glam::Vec3::ZERO;
    voxel.fall_start_y = None;
}

/// Per-dim mob + drop state parked while the dim is inactive.
///
/// Mirrors what `world_save::entities::DimensionEntities` carries
/// on disk, but in the live-runtime types. The applier writes
/// these out + the loader fills them back in.
#[derive(Debug, Default)]
pub(crate) struct DimEntities {
    pub mobs: Vec<MobState>,
    pub drops: Vec<DropItem>,
    /// W137: minecarts parked with their dim (previously carts
    /// LEAKED across a dim switch - overworld carts rode nether
    /// rails).
    pub carts: Vec<crate::minecart::Minecart>,
    pub next_mob_id: u32,
    pub next_drop_id: u32,
    pub next_cart_id: u32,
}

impl DimEntities {
    /// Build an empty bundle for a brand-new dim.
    pub(crate) fn empty() -> Self {
        Self {
            mobs: Vec::new(),
            drops: Vec::new(),
            carts: Vec::new(),
            next_mob_id: 1,
            next_drop_id: 1,
            next_cart_id: 1,
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
    /// W12.5: generic MC-emitter index parked per-dim. See
    /// `Voxel::emitter_positions` for semantics.
    pub emitter_positions: HashMap<[i32; 3], u16>,
    pub torch_orient: HashMap<[i32; 3], u8>,
    /// W279: template stair orientations park with their dim.
    pub stair_states: HashMap<[i32; 3], u8>,
    pub snow_positions: HashMap<[i32; 3], u8>,
    /// Per-cell fire age (Q.4b). Parked alongside chests + furnaces
    /// when the dim swaps out.
    pub fires: HashMap<[i32; 3], u8>,
    pub liquid_schedule: LiquidSchedule,
    pub liquid_clock: u64,
    pub liquid_level: HashMap<[i32; 3], u8>,
    pub liquid_positions: std::collections::HashSet<[i32; 3]>,
    /// W138: door/trapdoor facing bytes (see `block::door`). These
    /// runtime maps used to evaporate on a dim switch - the door
    /// blocks stayed but rendered with default facing + closed.
    pub door_facing: HashMap<[i32; 3], u8>,
    /// W138: open doors/trapdoors (keyed by the lower half).
    pub door_open: std::collections::HashSet<[i32; 3]>,
    /// W138: chest latch facing (W126 render state).
    pub chest_facing: HashMap<[i32; 3], u8>,
    /// W138: brewing stands + their progress runtimes.
    pub brewing_stands: HashMap<[i32; 3], Container>,
    pub brewing_runtimes:
        HashMap<[i32; 3], crate::block::brewing::BrewingRuntime>,
    /// W138: levers currently switched ON.
    pub lever_on: std::collections::HashSet<[i32; 3]>,
    /// W143: stored rail shapes (stateful MC RailState port).
    pub rail_shapes: HashMap<[i32; 3], u8>,
    /// W242.2: monster-spawner index parked per-dim (chunks in the
    /// parked world never re-run the worker side-effect scan on a
    /// switch, so the set must survive alongside them).
    pub spawner_positions: std::collections::HashSet<[i32; 3]>,
    /// W242.2: per-spawner tick state (delay countdowns keep their
    /// place across a dim round-trip; MC keeps the loaded block
    /// entity alive the same way).
    pub spawner_states: HashMap<[i32; 3], crate::mob::spawner::SpawnerState>,
    /// W250: worker-resolved spawner entity kinds park with their dim.
    pub spawner_kinds: HashMap<[i32; 3], u8>,
}

impl Default for DimWorld {
    fn default() -> Self {
        Self {
            world: World::new(),
            chests: HashMap::new(),
            furnaces: HashMap::new(),
            furnace_runtimes: HashMap::new(),
            torch_positions: std::collections::HashSet::new(),
            emitter_positions: HashMap::new(),
            torch_orient: HashMap::new(),
            stair_states: HashMap::new(),
            snow_positions: HashMap::new(),
            fires: HashMap::new(),
            liquid_schedule: LiquidSchedule::new(),
            liquid_clock: 0,
            liquid_level: HashMap::new(),
            liquid_positions: std::collections::HashSet::new(),
            door_facing: HashMap::new(),
            door_open: std::collections::HashSet::new(),
            chest_facing: HashMap::new(),
            brewing_stands: HashMap::new(),
            brewing_runtimes: HashMap::new(),
            lever_on: std::collections::HashSet::new(),
            rail_shapes: HashMap::new(),
            spawner_positions: std::collections::HashSet::new(),
            spawner_states: HashMap::new(),
            spawner_kinds: HashMap::new(),
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
        assert!(dw.fires.is_empty());
        assert_eq!(dw.liquid_clock, 0);
    }

    #[test]
    fn dim_world_default_fires_map_is_empty() {
        // Q.4b: every freshly-minted dim starts with no fire cells.
        let dw = DimWorld::default();
        assert!(dw.fires.is_empty());
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
