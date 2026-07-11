//! Per-section builders + appliers + small utilities used by
//! `runtime::save_world` / `runtime::load_world`.
//!
//! Pulled out so the orchestration in `mod.rs` stays a 200-LOC flat
//! sequence of "read meta -> write chunks -> ...". Each helper here
//! owns one of the typed conversions between in-memory `Voxel` state
//! and the typed save schema.
//!
//! W5.2d folder split: the original `helpers.rs` (547 LOC) crossed
//! the 500-LOC soft cap. Split per-section into:
//! - `player`  - player pose + inventory save / apply.
//! - `entities` - per-dim mob + drop save / apply.
//! - `containers` - per-dim chest + furnace + fire save / apply.
//! - `timers` - schedule + catch-up + furnace-timer plumbing.
//! - `mod.rs` - small shared utilities + the chunk-payload
//!   applier that don't belong in any single section.
//!
//! Every sub-file is `pub(super) mod ...;` here and re-exported so
//! callers (`mod.rs` + `dims.rs`) keep their existing import lines.

use std::io;

use infengine_voxel::{CHUNK_SIZE, ChunkCoord};

use crate::Voxel;
use crate::liquid::liquid_for;

use super::super::chunks::ChunkPayload;

pub(super) mod containers;
pub(super) mod entities;
pub(super) mod player;
pub(super) mod timers;

// Section re-exports so the existing callers don't have to track
// the new folder structure.
pub(super) use containers::{
    BlockStateRefs, apply_dimension_containers, build_dimension_containers_from,
};
pub(super) use entities::{apply_dimension_entities, build_dimension_entities_from};
pub(super) use player::{apply_player_save, build_player_save};
pub(super) use timers::{apply_catch_up_events, build_timers, hydrate_liquid_schedule};

/// Apply one decoded `ChunkPayload` onto `voxel`. Inserts the chunk,
/// then walks the per-chunk decoration overrides (liquid level, torch
/// orientation, snow layers) into the global maps using the
/// `chunk_origin + local` -> world-space conversion.
pub(super) fn apply_chunk_payload(voxel: &mut Voxel, payload: ChunkPayload) -> io::Result<()> {
    let coord = payload.coord;
    let chunk = payload.voxels.into_chunk()?;
    let origin = [
        coord[0] * CHUNK_SIZE as i32,
        coord[1] * CHUNK_SIZE as i32,
        coord[2] * CHUNK_SIZE as i32,
    ];
    voxel.world.insert_chunk(coord, chunk);

    // Scan the chunk for blocks the runtime tracks via side maps
    // (torch positions, snow positions, liquid positions). The
    // per-chunk override lists below then carry the variable data
    // (level / orientation / layer count) that the worldgen pass
    // wouldn't infer.
    //
    // W46.22: fold emitter + flame-emitter + fire indices into this
    // same scan so the on-load post-pass (the W46.20 emitter_scan
    // rebuild_all that walked every chunk a second time) is no
    // longer needed. For an 11k-chunk save that second pass added
    // ~80 MB of chunk-clone churn and pushed load time past 90 s.
    if let Some(chunk_ref) = voxel.world.chunk(coord) {
        let cs = CHUNK_SIZE as i32;
        for ly in 0..cs {
            for lz in 0..cs {
                for lx in 0..cs {
                    let b = chunk_ref.get(lx as usize, ly as usize, lz as usize);
                    if b.is_air() {
                        continue;
                    }
                    let world_pos = [origin[0] + lx, origin[1] + ly, origin[2] + lz];
                    if liquid_for(b).is_some() {
                        voxel.liquid_positions.insert(world_pos);
                    }
                    if crate::block::light_emission::light_emission_for_id(b.0) > 0 {
                        voxel.emitter_positions.insert(world_pos, b.0);
                    }
                    if crate::flame_emitter(b.0) {
                        voxel.torch_positions.insert(world_pos);
                    }
                    if b.0 == crate::block::FIRE_ID || b.0 == crate::block::SOUL_FIRE_ID {
                        voxel.fires.entry(world_pos).or_insert(0);
                    }
                    // W242.2: saved chunks re-index their spawners
                    // here (streamed chunks go through the worker
                    // side-effect scan instead).
                    if b.0 == crate::block::SPAWNER_ID {
                        voxel.spawner_positions.insert(world_pos);
                    }
                }
            }
        }
    }

    for (local, level) in payload.liquid_levels {
        let world_pos = local_to_world(origin, local);
        voxel.liquid_level.insert(world_pos, level.min(7));
    }
    for (local, orient) in payload.torch_orient {
        let world_pos = local_to_world(origin, local);
        // W46.23: guard against corrupt-save phantoms. Only re-attach
        // orient + torch_positions when the chunk voxel ACTUALLY holds
        // a torch-family block. A previous session may have saved a
        // torch_orient entry whose backing voxel was cleared without
        // the matching torch_orient cleanup; without this guard the
        // load step keeps spawning smoke from a non-existent torch.
        if let Some(chunk_ref) = voxel.world.chunk(coord) {
            let b = chunk_ref.get(local[0] as usize, local[1] as usize, local[2] as usize);
            if !crate::flame_emitter(b.0) {
                continue;
            }
        }
        voxel.torch_orient.insert(world_pos, orient);
        voxel.torch_positions.insert(world_pos);
    }
    for (local, layers) in payload.snow_layers {
        let world_pos = local_to_world(origin, local);
        voxel.snow_positions.insert(world_pos, layers.clamp(1, 8));
    }

    voxel.liquid_mesh_dirty = true;
    Ok(())
}

/// Map a chunk-local `[u8; 3]` coord back into world space.
fn local_to_world(origin: [i32; 3], local: [u8; 3]) -> [i32; 3] {
    [
        origin[0] + local[0] as i32,
        origin[1] + local[1] as i32,
        origin[2] + local[2] as i32,
    ]
}

/// Filter a global `HashMap<[i32; 3], T>` to just the entries inside
/// the chunk at `coord`, converting each world-space key into a
/// chunk-local `[u8; 3]` for the payload schema.
pub(crate) fn collect_for_chunk<T: Copy>(
    map: &std::collections::HashMap<[i32; 3], T>,
    coord: ChunkCoord,
) -> Vec<([u8; 3], T)> {
    let cs = CHUNK_SIZE as i32;
    let origin = [coord[0] * cs, coord[1] * cs, coord[2] * cs];
    let mut out: Vec<([u8; 3], T)> = Vec::new();
    for (k, v) in map {
        let lx = k[0] - origin[0];
        let ly = k[1] - origin[1];
        let lz = k[2] - origin[2];
        if (0..cs).contains(&lx) && (0..cs).contains(&ly) && (0..cs).contains(&lz) {
            out.push(([lx as u8, ly as u8, lz as u8], *v));
        }
    }
    out
}

/// Parse a chunk filename of the shape `cx_cy_cz.chunk.bin` back into
/// a `ChunkCoord`. Returns `None` on any malformed input so the
/// caller can skip unrecognised files without failing the load.
pub(super) fn parse_chunk_filename(path: &std::path::Path) -> Option<ChunkCoord> {
    let name = path.file_name()?.to_str()?;
    // Strip the `.chunk.bin` suffix (note: not just `.bin`).
    let stem = name.strip_suffix(".chunk.bin")?;
    let parts: Vec<&str> = stem.split('_').collect();
    if parts.len() != 3 {
        return None;
    }
    let cx = parts[0].parse::<i32>().ok()?;
    let cy = parts[1].parse::<i32>().ok()?;
    let cz = parts[2].parse::<i32>().ok()?;
    Some([cx, cy, cz])
}

/// Approximate seconds elapsed since the previous `save_world` call.
/// Used to bump `play_time_seconds`. Falls back to 0 when no prior
/// save instant is known.
///
/// Reuses `last_state_save` (the legacy autosave instant) so we don't
/// need a new field on `Voxel` for B.7a. A follow-up commit can split
/// the meta + state save instants if play-time accuracy needs work.
pub(super) fn session_seconds_since_save(voxel: &Voxel) -> u64 {
    match voxel.last_state_save {
        Some(t) => t.elapsed().as_secs(),
        None => voxel
            .session_started
            .map(|t| t.elapsed().as_secs())
            .unwrap_or(0),
    }
}

