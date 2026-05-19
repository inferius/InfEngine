//! Per-dim chest + furnace + fire save / apply. Builds
//! `DimensionContainers` from raw refs (so the per-dim save path can
//! hand in either the active dim's live maps or a parked-dim
//! snapshot) and writes the bundle back onto `voxel` on load.

use crate::Voxel;

use crate::world_save::containers::{
    CONTAINERS_FORMAT_VERSION, ChestSave, DimensionContainers, FurnaceSave,
};

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
    DimensionContainers {
        format_version: CONTAINERS_FORMAT_VERSION,
        chests: chest_saves,
        furnaces: furnace_saves,
        fires: fire_saves,
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
        let mut c = crate::container::Container::with_size(chest.slots.len());
        for (i, slot) in chest.slots.iter().enumerate() {
            c.set_slot(i, *slot);
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
        for (i, slot) in furnace.slots.iter().enumerate() {
            c.set_slot(i, *slot);
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
}
