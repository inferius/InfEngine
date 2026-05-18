//! Per-dim save/load orchestration.
//!
//! Phase F: a save can hold multiple dimensions resident at once.
//! Each dimension owns its own subdirectory under the slot
//! (`chunks/<dim>/`, `entities/<dim>.bin`, `containers/<dim>.bin`).
//! Timers stay in a single file - every `Timer` carries its own
//! `dim: String`.
//!
//! `save_all_dims` iterates `world_json.dimensions`, writing the
//! active dim from `voxel.{world,chests,furnaces,...}` directly
//! and the parked dims from `voxel.worlds` + `voxel.entities_by_dim`.
//!
//! `load_all_dims` reverses the iteration. For every declared dim
//! it reads chunks + entities + containers from disk; the entry
//! that matches `voxel.current_dim` lands in the top-level live
//! fields, the rest park in the `_by_dim` maps.

use std::collections::HashMap;
use std::io;

use infengine_voxel::ChunkCoord;
use tracing::warn;

use crate::Voxel;
use crate::block::furnace::FurnaceRuntime;
use crate::container::Container;
use crate::dim::DimWorld;
use crate::drops::DropItem;
use crate::mob_ai::MobState;

use super::super::chunks::{self, CHUNK_PAYLOAD_VERSION, ChunkPayload, ChunkVoxels};
use super::super::containers::{self, DimensionContainers};
use super::super::entities::{self, DimensionEntities};
use super::super::layout;
use super::super::world_json::WorldJson;

use super::helpers::{
    apply_chunk_payload, apply_dimension_containers,
    apply_dimension_entities, build_dimension_containers_from,
    build_dimension_entities_from, collect_for_chunk,
    parse_chunk_filename,
};

/// Save every dimension declared in `world_json`. Returns the
/// total chunk count written across all dims.
pub(super) fn save_all_dims(
    voxel: &mut Voxel,
    slug: &str,
    world_json: &WorldJson,
) -> io::Result<usize> {
    let mut total_chunks: usize = 0;
    for dim_name in &world_json.dimensions {
        let n = if dim_name == &voxel.current_dim {
            save_active_dim(voxel, slug, dim_name)?
        } else {
            save_parked_dim(voxel, slug, dim_name)?
        };
        total_chunks += n;
    }
    Ok(total_chunks)
}

/// Save the dim whose state is currently in the active
/// (top-level) `Voxel` fields. Mirrors the legacy single-dim
/// flow but parameterised on the dim tag.
fn save_active_dim(voxel: &Voxel, slug: &str, dim: &str) -> io::Result<usize> {
    let mut chunk_count: usize = 0;
    let coords: Vec<ChunkCoord> = voxel.world.iter_chunks().map(|(c, _)| *c).collect();
    for coord in coords {
        let Some(chunk) = voxel.world.chunk(coord) else {
            continue;
        };
        let payload = ChunkPayload {
            format_version: CHUNK_PAYLOAD_VERSION,
            coord,
            voxels: ChunkVoxels::from_chunk(chunk),
            liquid_levels: collect_for_chunk(&voxel.liquid_level, coord),
            torch_orient: collect_for_chunk(&voxel.torch_orient, coord),
            snow_layers: collect_for_chunk(&voxel.snow_positions, coord),
        };
        chunks::write_chunk(slug, dim, &payload)?;
        chunk_count += 1;
    }
    let dim_entities = build_dimension_entities_from(
        &voxel.mobs,
        &voxel.dropped_items,
        voxel.next_mob_id,
        voxel.next_drop_id,
    );
    entities::write_entities(slug, dim, &dim_entities)?;
    let dim_containers = build_dimension_containers_from(
        &voxel.chests,
        &voxel.furnaces,
        &voxel.furnace_runtimes,
    );
    containers::write_containers(slug, dim, &dim_containers)?;
    Ok(chunk_count)
}

/// Save a dim whose state is parked in `voxel.worlds` +
/// `voxel.entities_by_dim`. Skips silently if the dim isn't
/// parked - that just means the player never visited it this
/// session, no data to write.
fn save_parked_dim(voxel: &Voxel, slug: &str, dim: &str) -> io::Result<usize> {
    let Some(dw) = voxel.worlds.get(dim) else {
        return Ok(0);
    };
    let mut chunk_count: usize = 0;
    let coords: Vec<ChunkCoord> = dw.world.iter_chunks().map(|(c, _)| *c).collect();
    for coord in coords {
        let Some(chunk) = dw.world.chunk(coord) else {
            continue;
        };
        let payload = ChunkPayload {
            format_version: CHUNK_PAYLOAD_VERSION,
            coord,
            voxels: ChunkVoxels::from_chunk(chunk),
            liquid_levels: collect_for_chunk(&dw.liquid_level, coord),
            torch_orient: collect_for_chunk(&dw.torch_orient, coord),
            snow_layers: collect_for_chunk(&dw.snow_positions, coord),
        };
        chunks::write_chunk(slug, dim, &payload)?;
        chunk_count += 1;
    }
    // Entities: missing entry = no mobs/drops parked yet.
    let (mobs, drops, nm, nd) = match voxel.entities_by_dim.get(dim) {
        Some(e) => (e.mobs.as_slice(), e.drops.as_slice(), e.next_mob_id, e.next_drop_id),
        None => (&[][..] as &[MobState], &[][..] as &[DropItem], 1u32, 1u32),
    };
    let dim_entities = build_dimension_entities_from(mobs, drops, nm, nd);
    entities::write_entities(slug, dim, &dim_entities)?;
    let dim_containers = build_dimension_containers_from(
        &dw.chests,
        &dw.furnaces,
        &dw.furnace_runtimes,
    );
    containers::write_containers(slug, dim, &dim_containers)?;
    Ok(chunk_count)
}

/// Load every dim declared in `world_json`. Returns the total
/// chunks loaded across all dims. The dim matching
/// `voxel.current_dim` is left in the active (top-level) Voxel
/// slots; the rest land parked in `voxel.worlds` +
/// `voxel.entities_by_dim`.
///
/// `current_dim` is set by the caller (typically from the
/// player's `last_dimension`) BEFORE this fn runs.
pub(super) fn load_all_dims(
    voxel: &mut Voxel,
    slug: &str,
    world_json: &WorldJson,
) -> io::Result<usize> {
    let mut total_chunks: usize = 0;
    for dim_name in &world_json.dimensions {
        let n = if dim_name == &voxel.current_dim {
            load_active_dim(voxel, slug, dim_name)?
        } else {
            load_parked_dim(voxel, slug, dim_name)?
        };
        total_chunks += n;
    }
    Ok(total_chunks)
}

/// Load a dim into the active (top-level) Voxel slots. Same
/// shape as the legacy single-dim load.
fn load_active_dim(voxel: &mut Voxel, slug: &str, dim: &str) -> io::Result<usize> {
    let mut chunks_loaded: usize = 0;
    let dir = layout::world_dir(slug).join("chunks").join(dim);
    if dir.is_dir() {
        let read = std::fs::read_dir(&dir)?;
        for entry in read.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("bin") {
                continue;
            }
            let Some(coord) = parse_chunk_filename(&path) else {
                warn!(path = %path.display(), "world_save: skipping unrecognised chunk filename");
                continue;
            };
            let payload = match chunks::read_chunk(slug, dim, coord)? {
                Some(p) => p,
                None => continue,
            };
            apply_chunk_payload(voxel, payload)?;
            chunks_loaded += 1;
        }
    }
    let ents = entities::read_entities(slug, dim)?.unwrap_or_else(DimensionEntities::empty);
    apply_dimension_entities(voxel, ents);
    let conts = containers::read_containers(slug, dim)?
        .unwrap_or_else(DimensionContainers::empty);
    apply_dimension_containers(voxel, conts);
    Ok(chunks_loaded)
}

/// Load a dim into the parking maps. The DimWorld + DimEntities
/// are built from disk, no Voxel side-state touched.
fn load_parked_dim(voxel: &mut Voxel, slug: &str, dim: &str) -> io::Result<usize> {
    use infengine_voxel::CHUNK_SIZE;
    use crate::liquid::liquid_for;

    let mut dw = DimWorld::default();
    let mut chunks_loaded: usize = 0;
    let dir = layout::world_dir(slug).join("chunks").join(dim);
    if dir.is_dir() {
        let read = std::fs::read_dir(&dir)?;
        for entry in read.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("bin") {
                continue;
            }
            let Some(coord) = parse_chunk_filename(&path) else {
                warn!(path = %path.display(), "world_save: skipping unrecognised chunk filename");
                continue;
            };
            let payload = match chunks::read_chunk(slug, dim, coord)? {
                Some(p) => p,
                None => continue,
            };
            // Inline a mini "apply_chunk_payload but to DimWorld"
            // to avoid pulling the live Voxel through here.
            let chunk = payload.voxels.into_chunk()?;
            let origin = [
                coord[0] * CHUNK_SIZE as i32,
                coord[1] * CHUNK_SIZE as i32,
                coord[2] * CHUNK_SIZE as i32,
            ];
            dw.world.insert_chunk(coord, chunk);
            // Liquid-positions scan mirrors the active-dim path
            // so the dim's liquid mesh path picks them up after
            // a future switch.
            if let Some(chunk_ref) = dw.world.chunk(coord) {
                let cs = CHUNK_SIZE as i32;
                for ly in 0..cs {
                    for lz in 0..cs {
                        for lx in 0..cs {
                            let b = chunk_ref.get(lx as usize, ly as usize, lz as usize);
                            if liquid_for(b).is_some() {
                                dw.liquid_positions.insert([
                                    origin[0] + lx,
                                    origin[1] + ly,
                                    origin[2] + lz,
                                ]);
                            }
                        }
                    }
                }
            }
            for (local, level) in payload.liquid_levels {
                dw.liquid_level.insert(
                    [origin[0] + local[0] as i32, origin[1] + local[1] as i32, origin[2] + local[2] as i32],
                    level.min(7),
                );
            }
            for (local, orient) in payload.torch_orient {
                let wp = [origin[0] + local[0] as i32, origin[1] + local[1] as i32, origin[2] + local[2] as i32];
                dw.torch_orient.insert(wp, orient);
                dw.torch_positions.insert(wp);
            }
            for (local, layers) in payload.snow_layers {
                dw.snow_positions.insert(
                    [origin[0] + local[0] as i32, origin[1] + local[1] as i32, origin[2] + local[2] as i32],
                    layers.clamp(1, 8),
                );
            }
            chunks_loaded += 1;
        }
    }
    // Containers -> dw maps.
    let conts = containers::read_containers(slug, dim)?
        .unwrap_or_else(DimensionContainers::empty);
    apply_dim_containers_to_maps(
        &mut dw.chests,
        &mut dw.furnaces,
        &mut dw.furnace_runtimes,
        conts,
    );
    voxel.worlds.insert(dim.to_string(), dw);

    // Entities -> parked entity bundle.
    let ents = entities::read_entities(slug, dim)?
        .unwrap_or_else(DimensionEntities::empty);
    let mut parked = crate::dim::DimEntities::empty();
    for m in ents.mobs {
        if m.health <= 0.0 {
            continue;
        }
        parked.mobs.push(MobState {
            id: m.id,
            kind: m.kind,
            pos: m.pos,
            target: m.pos,
            yaw: m.yaw,
            retarget_at: 0.0,
            health: m.health,
            dying: 0.0,
            attack_cd: 0.0,
        });
    }
    for d in ents.drops {
        parked.drops.push(DropItem {
            id: d.id,
            block: d.block,
            pos: d.pos,
            vel: d.vel,
            age: d.age,
        });
    }
    parked.next_mob_id = ents
        .next_mob_id
        .max(parked.mobs.iter().map(|m| m.id).max().unwrap_or(0) + 1);
    parked.next_drop_id = ents
        .next_drop_id
        .max(parked.drops.iter().map(|d| d.id).max().unwrap_or(0) + 1);
    voxel.entities_by_dim.insert(dim.to_string(), parked);

    Ok(chunks_loaded)
}

/// Apply a loaded `DimensionContainers` to the per-dim parked
/// chest/furnace/runtime maps. Mirror of `helpers::apply_dimension_containers`
/// but writing into the DimWorld parking maps instead of Voxel.
fn apply_dim_containers_to_maps(
    chests: &mut HashMap<[i32; 3], Container>,
    furnaces: &mut HashMap<[i32; 3], Container>,
    furnace_runtimes: &mut HashMap<[i32; 3], FurnaceRuntime>,
    bundle: DimensionContainers,
) {
    chests.clear();
    for c in bundle.chests {
        let mut row = Container::with_size(c.slots.len());
        for (i, slot) in c.slots.iter().enumerate() {
            row.set_slot(i, *slot);
        }
        chests.insert(c.pos, row);
    }
    furnaces.clear();
    furnace_runtimes.clear();
    for f in bundle.furnaces {
        let mut row = crate::block::furnace::new_furnace();
        if f.slots.len() != row.len() {
            row = Container::with_size(f.slots.len());
        }
        for (i, slot) in f.slots.iter().enumerate() {
            row.set_slot(i, *slot);
        }
        furnaces.insert(f.pos, row);
        furnace_runtimes.insert(
            f.pos,
            FurnaceRuntime {
                fuel_left_ms: f.fuel_left_ms,
                fuel_full_ms: f.fuel_full_ms,
                cook_progress_ms: f.cook_progress_ms,
            },
        );
    }
}

/// Apply the `dimension_worldgens` table to the runtime's
/// per-dim worldgen cache + return the active dim's worldgen
/// for the caller to pin onto the global atomic.
pub(super) fn apply_dim_worldgens(voxel: &mut Voxel, world_json: &WorldJson) {
    use crate::worldgen::WorldGen;
    voxel.dim_worldgens.clear();
    for (dim, tag) in &world_json.dimension_worldgens {
        if let Some(wg) = WorldGen::from_str(tag) {
            voxel.dim_worldgens.insert(dim.clone(), wg);
        }
    }
}
