//! Per-dim block-state save / apply (chests, furnaces, fires,
//! doors, brewing stands, levers). Builds `DimensionContainers`
//! from raw refs (so the per-dim save path can hand in either the
//! active dim's live maps or a parked-dim snapshot) and writes the
//! bundle back onto `voxel` on load.

use std::collections::{HashMap, HashSet};

use crate::Voxel;

use crate::world_save::containers::{
    BrewingSave, CONTAINERS_FORMAT_VERSION, ChestSave, DimensionContainers, DoorSave,
    FacingSave, FurnaceSave,
};

/// W138: the per-position block-state maps beyond chests/furnaces/
/// fires. Bundled so the build fn keeps a readable signature (the
/// CLAUDE.md ctx-struct rule) - the save path fills it from either
/// the live `Voxel` fields or a parked `DimWorld`.
pub(in crate::world_save::runtime) struct BlockStateRefs<'a> {
    pub door_facing: &'a HashMap<[i32; 3], u8>,
    pub door_open: &'a HashSet<[i32; 3]>,
    pub chest_facing: &'a HashMap<[i32; 3], u8>,
    pub brewing_stands: &'a HashMap<[i32; 3], crate::container::Container>,
    pub brewing_runtimes:
        &'a HashMap<[i32; 3], crate::block::brewing::BrewingRuntime>,
    pub lever_on: &'a HashSet<[i32; 3]>,
    /// W143: stored rail shapes (stateful MC RailState port).
    pub rail_shapes: &'a HashMap<[i32; 3], u8>,
    /// W387: per-pos orientation bytes (see containers.rs v6).
    pub stair_states: &'a HashMap<[i32; 3], u8>,
    pub bed_states: &'a HashMap<[i32; 3], u8>,
    pub torch_orient: &'a HashMap<[i32; 3], u8>,
    pub spawner_kinds: &'a HashMap<[i32; 3], u8>,
}

/// W387: map -> save-vec helper for the per-pos state bytes.
fn pos_state_vec(
    map: &HashMap<[i32; 3], u8>,
) -> Vec<crate::world_save::containers::PosStateSave> {
    map.iter()
        .map(|(pos, state)| crate::world_save::containers::PosStateSave {
            pos: *pos,
            state: *state,
        })
        .collect()
}

/// Build a `DimensionContainers` snapshot from the chest +
/// furnace + runtime maps. The per-dim save path hands in either
/// `voxel.chests` etc. (active dim) or the parked DimWorld maps.
pub(in crate::world_save::runtime) fn build_dimension_containers_from(
    chests: &std::collections::HashMap<[i32; 3], crate::container::Container>,
    furnaces: &std::collections::HashMap<[i32; 3], crate::container::Container>,
    furnace_runtimes: &std::collections::HashMap<
        [i32; 3],
        crate::block::furnace::FurnaceRuntime,
    >,
    fires: &std::collections::HashMap<[i32; 3], u8>,
    block_state: BlockStateRefs<'_>,
) -> DimensionContainers {
    let chest_saves: Vec<ChestSave> = chests
        .iter()
        .map(|(pos, container)| ChestSave {
            pos: *pos,
            slots: container.slots().to_vec(),
        })
        .collect();
    let furnace_saves: Vec<FurnaceSave> = furnaces
        .iter()
        .map(|(pos, container)| {
            let rt = furnace_runtimes.get(pos).copied().unwrap_or_default();
            FurnaceSave {
                pos: *pos,
                slots: container.slots().to_vec(),
                fuel_left_ms: rt.fuel_left_ms,
                fuel_full_ms: rt.fuel_full_ms,
                cook_progress_ms: rt.cook_progress_ms,
            }
        })
        .collect();
    let fire_saves: Vec<crate::world_save::containers::FireSave> = fires
        .iter()
        .map(|(pos, age)| crate::world_save::containers::FireSave {
            pos: *pos,
            age: *age,
        })
        .collect();
    // W138: doors (+trapdoors) persist their facing byte + open
    // flag; chests their latch facing; brewing stands slots +
    // progress; levers the on-set.
    let door_saves: Vec<DoorSave> = block_state
        .door_facing
        .iter()
        .map(|(pos, facing)| DoorSave {
            pos: *pos,
            facing: *facing,
            open: block_state.door_open.contains(pos),
        })
        .collect();
    let facing_saves: Vec<FacingSave> = block_state
        .chest_facing
        .iter()
        .map(|(pos, facing)| FacingSave {
            pos: *pos,
            facing: *facing,
        })
        .collect();
    let brewing_saves: Vec<BrewingSave> = block_state
        .brewing_stands
        .iter()
        .map(|(pos, container)| BrewingSave {
            pos: *pos,
            slots: container.slots().to_vec(),
            progress_ms: block_state
                .brewing_runtimes
                .get(pos)
                .copied()
                .unwrap_or_default()
                .progress_ms,
        })
        .collect();
    let levers_on: Vec<[i32; 3]> = block_state.lever_on.iter().copied().collect();
    // W143: stored rail shapes (power bit is derived, never saved).
    let rail_saves: Vec<crate::world_save::containers::RailShapeSave> = block_state
        .rail_shapes
        .iter()
        .map(|(pos, shape)| crate::world_save::containers::RailShapeSave {
            pos: *pos,
            shape: *shape & 0x7F,
        })
        .collect();
    DimensionContainers {
        format_version: CONTAINERS_FORMAT_VERSION,
        chests: chest_saves,
        furnaces: furnace_saves,
        fires: fire_saves,
        doors: door_saves,
        chest_facings: facing_saves,
        brewing: brewing_saves,
        levers_on,
        rails: rail_saves,
        // W387: orientation bytes - the maps disk-loaded chunks
        // can never re-derive (their worldgen is skipped).
        stairs: pos_state_vec(block_state.stair_states),
        beds: pos_state_vec(block_state.bed_states),
        torch_orients: pos_state_vec(block_state.torch_orient),
        spawners: pos_state_vec(block_state.spawner_kinds),
    }
}

/// Apply a loaded `DimensionContainers` back onto `voxel.chests`
/// + `voxel.furnaces` (+ runtime triples). Replaces the live maps
/// - the runtime doesn't keep stale entries across loads. Each
///   `ChestSave` / `FurnaceSave` rebuilds a `Container` with the
///   recorded slot count + contents. Furnace containers are minted
///   via `block::furnace::new_furnace` so the accept filters are
///   wired up before the saved slot contents land.
pub(in crate::world_save::runtime) fn apply_dimension_containers(
    voxel: &mut Voxel,
    containers_bundle: DimensionContainers,
) {
    voxel.chests.clear();
    for chest in containers_bundle.chests {
        // W467: slots are ItemStack rows - move them straight in,
        // components included.
        let mut c = crate::container::Container::with_size(chest.slots.len());
        for (i, slot) in chest.slots.into_iter().enumerate() {
            c.set_slot(i, slot);
        }
        voxel.chests.insert(chest.pos, c);
    }
    voxel.furnaces.clear();
    voxel.furnace_runtimes.clear();
    for furnace in containers_bundle.furnaces {
        let mut c = crate::block::furnace::new_furnace();
        // The slot count may differ from FURNACE_SLOTS for a
        // future blast-furnace variant. Resize bridge: the new_
        // furnace mints 3 slots; if the save has more, use raw
        // Container with size + accept filters set ourselves.
        if furnace.slots.len() != c.len() {
            c = crate::container::Container::with_size(furnace.slots.len());
        }
        for (i, slot) in furnace.slots.into_iter().enumerate() {
            c.set_slot(i, slot);
        }
        voxel.furnaces.insert(furnace.pos, c);
        voxel.furnace_runtimes.insert(
            furnace.pos,
            crate::block::furnace::FurnaceRuntime {
                fuel_left_ms: furnace.fuel_left_ms,
                fuel_full_ms: furnace.fuel_full_ms,
                cook_progress_ms: furnace.cook_progress_ms,
            },
        );
    }
    // Q.4f: re-hydrate the fire-cell age map. The world block
    // at each pos was already set by the chunk loader (FIRE_ID);
    // here we just restore the parallel age state.
    voxel.fires.clear();
    for fire in containers_bundle.fires {
        voxel.fires.insert(fire.pos, fire.age);
    }
    // W138: re-hydrate the door/chest/brewing/lever state maps -
    // without these a reloaded door rendered with the default
    // facing (looked open-reversed) and read as closed to the
    // collision check.
    voxel.door_facing.clear();
    voxel.door_open.clear();
    for door in containers_bundle.doors {
        voxel.door_facing.insert(door.pos, door.facing);
        if door.open {
            voxel.door_open.insert(door.pos);
        }
    }
    voxel.chest_facing.clear();
    for f in containers_bundle.chest_facings {
        voxel.chest_facing.insert(f.pos, f.facing);
    }
    voxel.brewing_stands.clear();
    voxel.brewing_runtimes.clear();
    for stand in containers_bundle.brewing {
        let mut c = crate::block::brewing::new_brewing_stand();
        if stand.slots.len() != c.len() {
            c = crate::container::Container::with_size(stand.slots.len());
        }
        for (i, slot) in stand.slots.into_iter().enumerate() {
            c.set_slot(i, slot);
        }
        voxel.brewing_stands.insert(stand.pos, c);
        voxel.brewing_runtimes.insert(
            stand.pos,
            crate::block::brewing::BrewingRuntime {
                progress_ms: stand.progress_ms,
            },
        );
    }
    voxel.lever_on.clear();
    for pos in containers_bundle.levers_on {
        voxel.lever_on.insert(pos);
    }
    // W143: stored rail shapes. Cells whose rail block vanished
    // from the save are pruned lazily by the break path; stale
    // entries are harmless (the mesher only reads shapes for cells
    // that actually hold rails).
    voxel.rail_shapes.clear();
    for r in containers_bundle.rails {
        voxel.rail_shapes.insert(r.pos, r.shape & 0x7F);
    }
    // W387: orientation bytes back into the runtime maps. NOTE:
    // extend, do not clear - the initial async build may have
    // already replayed side effects for freshly-generated chunks
    // before the save applies, and both sources agree wherever
    // they overlap (same deterministic worldgen).
    for e in containers_bundle.stairs {
        voxel.stair_states.insert(e.pos, e.state);
    }
    for e in containers_bundle.beds {
        voxel.bed_states.insert(e.pos, e.state);
    }
    for e in containers_bundle.torch_orients {
        voxel.torch_orient.insert(e.pos, e.state);
    }
    for e in containers_bundle.spawners {
        voxel.spawner_kinds.insert(e.pos, e.state);
    }
}
