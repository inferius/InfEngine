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

use std::io;

use infengine_voxel::ChunkCoord;
use tracing::warn;

use crate::Voxel;
use crate::block::furnace::FurnaceRuntime;
use crate::container::Container;
use crate::dim::DimWorld;
use crate::drops::DropItem;
use crate::mob::MobState;

use super::super::chunks::{self, CHUNK_PAYLOAD_VERSION, ChunkPayload, ChunkVoxels};
use super::super::containers::{self, DimensionContainers};
use super::super::entities::{self, DimensionEntities};
use super::super::layout;
use super::super::world_json::WorldJson;

use super::helpers::{
    BlockStateRefs, apply_chunk_payload, apply_dimension_containers,
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
            let n = save_active_dim(voxel, slug, dim_name)?;
            // W250: everything dirty is on disk now.
            voxel.world.clear_dirty();
            n
        } else {
            let n = save_parked_dim(voxel, slug, dim_name)?;
            if let Some(dw) = voxel.worlds.get_mut(dim_name) {
                dw.world.clear_dirty();
            }
            n
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
    // W250: persist only chunks mutated since the last save. Fresh
    // worldgen re-derives deterministically; loaded-but-untouched
    // chunks already have their file. Writing every loaded chunk
    // made each exit/autosave a 67k-file flush at view distance 32
    // (the "quitting takes forever" report).
    let coords: Vec<ChunkCoord> = voxel.world.dirty().iter().copied().collect();
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
        &voxel.carts,
        voxel.next_mob_id,
        voxel.next_drop_id,
        voxel.next_cart_id,
        &voxel.structure_mob_anchors,
    );
    entities::write_entities(slug, dim, &dim_entities)?;
    let dim_containers = build_dimension_containers_from(
        &voxel.chests,
        &voxel.furnaces,
        &voxel.furnace_runtimes,
        &voxel.fires,
        BlockStateRefs {
            door_facing: &voxel.door_facing,
            door_open: &voxel.door_open,
            chest_facing: &voxel.chest_facing,
            brewing_stands: &voxel.brewing_stands,
            brewing_runtimes: &voxel.brewing_runtimes,
            lever_on: &voxel.lever_on,
            rail_shapes: &voxel.rail_shapes,
            stair_states: &voxel.stair_states,
            bed_states: &voxel.bed_states,
            torch_orient: &voxel.torch_orient,
            spawner_kinds: &voxel.spawner_kinds,
        },
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
    // W250: dirty-only, same as the active dim.
    let coords: Vec<ChunkCoord> = dw.world.dirty().iter().copied().collect();
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
    // Entities: missing entry = no mobs/drops/carts parked yet.
    let (mobs, drops, carts, nm, nd, nc) = match voxel.entities_by_dim.get(dim) {
        Some(e) => (
            e.mobs.as_slice(),
            e.drops.as_slice(),
            e.carts.as_slice(),
            e.next_mob_id,
            e.next_drop_id,
            e.next_cart_id,
        ),
        None => (
            &[][..] as &[MobState],
            &[][..] as &[DropItem],
            &[][..] as &[crate::minecart::Minecart],
            1u32,
            1u32,
            1u32,
        ),
    };
    let dim_entities = build_dimension_entities_from(
        mobs,
        drops,
        carts,
        nm,
        nd,
        nc,
        &dw.structure_mob_anchors,
    );
    entities::write_entities(slug, dim, &dim_entities)?;
    let dim_containers = build_dimension_containers_from(
        &dw.chests,
        &dw.furnaces,
        &dw.furnace_runtimes,
        &dw.fires,
        BlockStateRefs {
            door_facing: &dw.door_facing,
            door_open: &dw.door_open,
            chest_facing: &dw.chest_facing,
            brewing_stands: &dw.brewing_stands,
            brewing_runtimes: &dw.brewing_runtimes,
            lever_on: &dw.lever_on,
            rail_shapes: &dw.rail_shapes,
            stair_states: &dw.stair_states,
            bed_states: &dw.bed_states,
            torch_orient: &dw.torch_orient,
            spawner_kinds: &dw.spawner_kinds,
        },
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
/// W447: run `work` over per-chunk payloads on a scoped worker
/// pool. Disk read + zstd + bincode dominated the world-load stall
/// (the R31 33 s / 17k-chunk single-threaded load); each file is
/// independent, so workers drain an atomic index over the coord
/// list and the caller applies results serially. Divergence from
/// the old serial loop: a hard read error now skips that one chunk
/// with a warn instead of aborting the whole load.
fn parallel_chunk_map<T: Send>(
    slug: &str,
    dim: &str,
    coords: &[[i32; 3]],
    work: impl Fn([i32; 3], ChunkPayload) -> Option<T> + Sync,
) -> Vec<T> {
    let n = coords.len();
    if n == 0 {
        return Vec::new();
    }
    let workers = std::thread::available_parallelism()
        .map(|p| p.get())
        .unwrap_or(4)
        .saturating_sub(1)
        .clamp(1, 12)
        .min(n);
    let next = std::sync::atomic::AtomicUsize::new(0);
    let sink: std::sync::Mutex<Vec<T>> =
        std::sync::Mutex::new(Vec::with_capacity(n));
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| {
                let mut local: Vec<T> = Vec::new();
                loop {
                    let i = next
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if i >= n {
                        break;
                    }
                    let coord = coords[i];
                    match chunks::read_chunk(slug, dim, coord) {
                        Ok(Some(p)) => {
                            if let Some(t) = work(coord, p) {
                                local.push(t);
                            }
                        }
                        Ok(None) => {}
                        Err(err) => {
                            warn!(
                                ?coord,
                                dim,
                                %err,
                                "world_save: chunk read failed during load (skipped)",
                            );
                        }
                    }
                }
                if !local.is_empty() {
                    sink.lock().unwrap().extend(local);
                }
            });
        }
    });
    sink.into_inner().unwrap()
}

/// W447: worker-side product for one parked-dim chunk - decoded
/// blocks plus everything the old serial loop derived from them.
struct ParkedDecoded {
    coord: [i32; 3],
    chunk: infengine_voxel::Chunk,
    liquids: Vec<[i32; 3]>,
    spawners: Vec<[i32; 3]>,
    liquid_levels: Vec<([u8; 3], u8)>,
    torch_orient: Vec<([u8; 3], u8)>,
    snow_layers: Vec<([u8; 3], u8)>,
}

/// W447: decode + scan one parked chunk (pure; runs on the pool).
fn scan_parked(
    coord: [i32; 3],
    payload: ChunkPayload,
) -> io::Result<ParkedDecoded> {
    use crate::liquid::liquid_for;
    use infengine_voxel::CHUNK_SIZE;
    let chunk = payload.voxels.into_chunk()?;
    let cs = CHUNK_SIZE as i32;
    let origin = [coord[0] * cs, coord[1] * cs, coord[2] * cs];
    let mut liquids: Vec<[i32; 3]> = Vec::new();
    let mut spawners: Vec<[i32; 3]> = Vec::new();
    for ly in 0..cs {
        for lz in 0..cs {
            for lx in 0..cs {
                let b = chunk.get(lx as usize, ly as usize, lz as usize);
                if liquid_for(b).is_some() {
                    liquids.push([
                        origin[0] + lx,
                        origin[1] + ly,
                        origin[2] + lz,
                    ]);
                }
                // W242.2: parked dims never re-run the worker scan on
                // a switch - index their spawners during the load.
                if b.0 == crate::block::SPAWNER_ID {
                    spawners.push([
                        origin[0] + lx,
                        origin[1] + ly,
                        origin[2] + lz,
                    ]);
                }
            }
        }
    }
    Ok(ParkedDecoded {
        coord,
        chunk,
        liquids,
        spawners,
        liquid_levels: payload.liquid_levels,
        torch_orient: payload.torch_orient,
        snow_layers: payload.snow_layers,
    })
}

pub(super) fn load_all_dims(
    voxel: &mut Voxel,
    slug: &str,
    world_json: &WorldJson,
    keep_center: [i32; 2],
    keep_radius: i32,
) -> io::Result<usize> {
    let mut total_chunks: usize = 0;
    for dim_name in &world_json.dimensions {
        let n = if dim_name == &voxel.current_dim {
            load_active_dim(voxel, slug, dim_name, keep_center, keep_radius)?
        } else {
            load_parked_dim(voxel, slug, dim_name)?
        };
        total_chunks += n;
    }
    Ok(total_chunks)
}

/// Load a dim into the active (top-level) Voxel slots. Same
/// shape as the legacy single-dim load.
///
/// W236: only chunks within `keep_radius` (Chebyshev, chunk XZ) of
/// `keep_center` are read + applied. The save grows every session
/// (unload-save persists every chunk that scrolls out of range), so
/// a travelled world accumulates far more files than the view
/// radius needs - the user's world had 18.5k of which the first
/// rescan pruned 8.3k immediately. Distant chunks stay on disk; the
/// stream workers already read the saved payload before falling
/// back to worldgen (W46.25), so travelling there later restores
/// edits without this upfront pass.
fn load_active_dim(
    voxel: &mut Voxel,
    slug: &str,
    dim: &str,
    keep_center: [i32; 2],
    keep_radius: i32,
) -> io::Result<usize> {
    let mut chunks_loaded: usize = 0;
    let dir = layout::world_dir(slug).join("chunks").join(dim);
    let mut coords: Vec<[i32; 3]> = Vec::new();
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
            // W236: distance gate BEFORE the file read.
            if (coord[0] - keep_center[0]).abs() > keep_radius
                || (coord[2] - keep_center[1]).abs() > keep_radius
            {
                continue;
            }
            coords.push(coord);
        }
    }
    // W447: disk read + decompress + decode on the pool; the Voxel
    // application (side-state maps) stays serial here.
    for payload in parallel_chunk_map(slug, dim, &coords, |_, p| Some(p)) {
        apply_chunk_payload(voxel, payload)?;
        chunks_loaded += 1;
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

    let mut dw = DimWorld::default();
    let mut chunks_loaded: usize = 0;
    let dir = layout::world_dir(slug).join("chunks").join(dim);
    let mut coords: Vec<[i32; 3]> = Vec::new();
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
            coords.push(coord);
        }
    }
    // W447: read + decode + the 16^3 liquid/spawner scan all run on
    // the pool (`scan_parked`); the serial phase just inserts into
    // the DimWorld maps. This walk had no distance gate - a parked
    // nether with thousands of visited chunks was the bulk of the
    // R31 33 s load stall.
    let decoded = parallel_chunk_map(slug, dim, &coords, |coord, payload| {
        match scan_parked(coord, payload) {
            Ok(d) => Some(d),
            Err(err) => {
                warn!(
                    ?coord,
                    dim,
                    %err,
                    "world_save: parked chunk decode failed (skipped)",
                );
                None
            }
        }
    });
    for d in decoded {
        let origin = [
            d.coord[0] * CHUNK_SIZE as i32,
            d.coord[1] * CHUNK_SIZE as i32,
            d.coord[2] * CHUNK_SIZE as i32,
        ];
        dw.world.insert_chunk(d.coord, d.chunk);
        for p in d.liquids {
            dw.liquid_positions.insert(p);
        }
        for p in d.spawners {
            dw.spawner_positions.insert(p);
        }
        for (local, level) in d.liquid_levels {
            dw.liquid_level.insert(
                [origin[0] + local[0] as i32, origin[1] + local[1] as i32, origin[2] + local[2] as i32],
                level.min(7),
            );
        }
        for (local, orient) in d.torch_orient {
            let wp = [origin[0] + local[0] as i32, origin[1] + local[1] as i32, origin[2] + local[2] as i32];
            dw.torch_orient.insert(wp, orient);
            dw.torch_positions.insert(wp);
        }
        for (local, layers) in d.snow_layers {
            dw.snow_positions.insert(
                [origin[0] + local[0] as i32, origin[1] + local[1] as i32, origin[2] + local[2] as i32],
                layers.clamp(1, 8),
            );
        }
        chunks_loaded += 1;
    }
    // Containers -> dw maps.
    let conts = containers::read_containers(slug, dim)?
        .unwrap_or_else(DimensionContainers::empty);
    apply_dim_containers_to_maps(&mut dw, conts);
    voxel.worlds.insert(dim.to_string(), dw);

    // Entities -> parked entity bundle.
    let ents = entities::read_entities(slug, dim)?
        .unwrap_or_else(DimensionEntities::empty);
    let mut parked = crate::dim::DimEntities::empty();
    for m in ents.mobs {
        if m.health <= 0.0 {
            continue;
        }
        let mut mob = MobState::new_natural(
            m.id,
            m.kind,
            m.pos,
            m.pos,
            m.yaw,
            0.0,
            m.health,
        );
        // W302 (v3): persistence + babyhood survive reload.
        mob.persistent = m.persistent;
        mob.baby_left = m.baby_left;
        parked.mobs.push(mob);
    }
    // W302: parked dim's spawned-anchor set.
    if let Some(d) = voxel.worlds.get_mut(dim) {
        d.structure_mob_anchors =
            ents.structure_mob_anchors.iter().copied().collect();
    }
    for d in ents.drops {
        // W468: full stack rehydrates (count + components).
        let age_ticks = (d.age * 20.0).max(0.0) as u32;
        parked.drops.push(DropItem {
            id: d.id,
            stack: d.stack,
            pos: d.pos,
            vel: d.vel,
            age: d.age,
            age_ticks,
            pickup_delay_ticks: 0,
        });
    }
    // W137: parked carts rebuild their pose against the parked
    // dim's world (inserted into `voxel.worlds` above) so a later
    // dim switch unparks them mid-rail, not at the origin.
    let parked_world = voxel
        .worlds
        .get(dim)
        .map(|d| (&d.world, &d.rail_shapes));
    for c in ents.carts {
        let mut cart = crate::minecart::Minecart {
            id: c.id,
            cell: c.cell,
            t: c.t,
            vel: c.vel,
            pos: glam::Vec3::new(
                c.cell[0] as f32 + 0.5,
                c.cell[1] as f32,
                c.cell[2] as f32 + 0.5,
            ),
            yaw: c.yaw,
        };
        if let Some((w, shapes)) = parked_world {
            crate::minecart::snap_to_rail(w, shapes, &mut cart);
        }
        parked.carts.push(cart);
    }
    parked.next_mob_id = ents
        .next_mob_id
        .max(parked.mobs.iter().map(|m| m.id).max().unwrap_or(0) + 1);
    parked.next_drop_id = ents
        .next_drop_id
        .max(parked.drops.iter().map(|d| d.id).max().unwrap_or(0) + 1);
    parked.next_cart_id = ents
        .next_cart_id
        .max(parked.carts.iter().map(|c| c.id).max().unwrap_or(0) + 1);
    voxel.entities_by_dim.insert(dim.to_string(), parked);

    Ok(chunks_loaded)
}

/// Apply a loaded `DimensionContainers` to the per-dim parked
/// block-state maps. Mirror of `helpers::apply_dimension_containers`
/// but writing into the DimWorld parking maps instead of Voxel.
/// W138: takes the whole `DimWorld` (the map list outgrew a sane
/// parameter list once doors/brewing/levers joined).
fn apply_dim_containers_to_maps(
    dw: &mut DimWorld,
    bundle: DimensionContainers,
) {
    dw.chests.clear();
    for c in bundle.chests {
        // W467: ItemStack rows move straight in (components kept).
        let mut row = Container::with_size(c.slots.len());
        for (i, slot) in c.slots.into_iter().enumerate() {
            row.set_slot(i, slot);
        }
        dw.chests.insert(c.pos, row);
    }
    dw.furnaces.clear();
    dw.furnace_runtimes.clear();
    for f in bundle.furnaces {
        let mut row = crate::block::furnace::new_furnace();
        if f.slots.len() != row.len() {
            row = Container::with_size(f.slots.len());
        }
        for (i, slot) in f.slots.into_iter().enumerate() {
            row.set_slot(i, slot);
        }
        dw.furnaces.insert(f.pos, row);
        dw.furnace_runtimes.insert(
            f.pos,
            FurnaceRuntime {
                fuel_left_ms: f.fuel_left_ms,
                fuel_full_ms: f.fuel_full_ms,
                cook_progress_ms: f.cook_progress_ms,
            },
        );
    }
    // Q.4f: per-cell fire age. The world block at each pos was
    // already set by the chunk loader (FIRE_ID); we just restore
    // the parallel age state.
    dw.fires.clear();
    for f in bundle.fires {
        dw.fires.insert(f.pos, f.age);
    }
    // W138: door / chest-facing / brewing / lever state into the
    // parked maps (mirrors helpers::apply_dimension_containers).
    dw.door_facing.clear();
    dw.door_open.clear();
    for d in bundle.doors {
        dw.door_facing.insert(d.pos, d.facing);
        if d.open {
            dw.door_open.insert(d.pos);
        }
    }
    dw.chest_facing.clear();
    for f in bundle.chest_facings {
        dw.chest_facing.insert(f.pos, f.facing);
    }
    dw.brewing_stands.clear();
    dw.brewing_runtimes.clear();
    for stand in bundle.brewing {
        let mut row = crate::block::brewing::new_brewing_stand();
        if stand.slots.len() != row.len() {
            row = Container::with_size(stand.slots.len());
        }
        for (i, slot) in stand.slots.into_iter().enumerate() {
            row.set_slot(i, slot);
        }
        dw.brewing_stands.insert(stand.pos, row);
        dw.brewing_runtimes.insert(
            stand.pos,
            crate::block::brewing::BrewingRuntime {
                progress_ms: stand.progress_ms,
            },
        );
    }
    dw.lever_on.clear();
    for pos in bundle.levers_on {
        dw.lever_on.insert(pos);
    }
    // W143: stored rail shapes ride along into the parked dim.
    dw.rail_shapes.clear();
    for r in bundle.rails {
        dw.rail_shapes.insert(r.pos, r.shape & 0x7F);
    }
    // W387: orientation bytes into the parked maps (extend, not
    // clear - see helpers::apply_dimension_containers).
    for e in bundle.stairs {
        dw.stair_states.insert(e.pos, e.state);
    }
    for e in bundle.beds {
        dw.bed_states.insert(e.pos, e.state);
    }
    for e in bundle.torch_orients {
        dw.torch_orient.insert(e.pos, e.state);
    }
    for e in bundle.spawners {
        dw.spawner_kinds.insert(e.pos, e.state);
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
