//! W46.20 - rebuild emitter / torch / fire indices from chunk contents.
//!
//! The `world_set_tracked` edit path keeps `Voxel.emitter_positions`,
//! `Voxel.torch_positions`, and `Voxel.fires` in sync with the world
//! whenever the player or a script PLACES / BREAKS a block. After a
//! disk load, however, the chunks come back populated by raw block ids
//! without going through that path - so the indices stay empty and
//! existing torches / lava emit no point lights, no flame particles,
//! and no smoke.
//!
//! This module re-scans the chunk grid (or one specific chunk) and
//! folds emitter cells into the three maps.
//!
//! Cases covered:
//! - After `world_save::load_world`: walk every loaded chunk.
//! - After `world.insert_chunk(...)` when the chunk streamer brings
//!   a previously-unloaded chunk back: scan that single chunk so its
//!   torches relight without waiting for the player to re-place.

use infengine_voxel::{CHUNK_SIZE, Chunk, ChunkCoord};

use crate::block::light_emission::light_emission_for_id;
use crate::flame_emitter;

/// Re-scan one chunk and merge its emitter cells into the global
/// indices. Cells that no longer hold emitter blocks are NOT removed
/// here - the per-cell `world_set_tracked` path handles delete on
/// edits, and re-loading a chunk from disk only ever adds cells.
pub(crate) fn scan_chunk(
    coord: ChunkCoord,
    chunk: &Chunk,
    emitter_positions: &mut std::collections::HashMap<[i32; 3], u16>,
    torch_positions: &mut std::collections::HashSet<[i32; 3]>,
    fires: &mut std::collections::HashMap<[i32; 3], u8>,
    portal_positions: &mut std::collections::HashSet<[i32; 3]>,
) {
    let cs = CHUNK_SIZE as i32;
    let origin = [coord[0] * cs, coord[1] * cs, coord[2] * cs];
    for lx in 0..CHUNK_SIZE {
        for ly in 0..CHUNK_SIZE {
            for lz in 0..CHUNK_SIZE {
                let block = chunk.get(lx, ly, lz);
                if block.is_air() {
                    continue;
                }
                let id = block.0;
                let world = [
                    origin[0] + lx as i32,
                    origin[1] + ly as i32,
                    origin[2] + lz as i32,
                ];
                // Any emission level > 0 -> point-light source.
                if light_emission_for_id(id) > 0 {
                    // W297: lava is excluded - its light is the
                    // block-light BFS (W281), point_lights filters it
                    // out anyway, and a nether lava ocean pushed the
                    // map to 1.78M entries = a 12 ms/frame scan.
                    if id != crate::block::LAVA_ID {
                        emitter_positions.insert(world, id);
                    }
                }
                // Torch family -> flame + smoke spawn list.
                if flame_emitter(id) {
                    torch_positions.insert(world);
                }
                // Fire id -> fires map. (Fire blocks have their own
                // separate tick + spread logic; the lifetime byte is
                // 0 on initial scan = "freshly lit".)
                if id == crate::block::FIRE_ID || id == crate::block::SOUL_FIRE_ID {
                    fires.entry(world).or_insert(0);
                }
                // W282: portal cells feed the swirl render list. It
                // was only populated by `world_set_tracked` at ignite
                // time, so a save/load came back with the PORTAL_ID
                // blocks present (teleport worked) but the visual
                // dead - and re-lighting was a no-op because the
                // cells were already portal.
                if id == crate::block::PORTAL_ID {
                    portal_positions.insert(world);
                }
            }
        }
    }
}

/// Walk every loaded chunk and rebuild all three emitter maps from
/// scratch. Reserved for diagnostics / dev tools - the normal load
/// path (W46.22) now populates the maps incrementally inside
/// `world_save::apply_chunk_payload`, so calling `rebuild_all`
/// during load_world is no longer needed.
///
/// W46.22: the initial implementation cloned every chunk before
/// scanning ("snapshot coords + chunks first") which was O(N) extra
/// memory + O(N) extra memcpy on top of the scan itself. For a save
/// with 11k+ chunks this added ~80 MB allocation churn and pushed
/// load time past 90 s. The split-borrow form below scans
/// in-place against `&World` while still allowing mutable access to
/// the three target maps.
#[allow(dead_code)] // kept for diagnostics + future dev tools
pub(crate) fn rebuild_all(vx: &mut crate::Voxel) {
    let start = std::time::Instant::now();
    // Split-borrow: take an immutable iterator over chunks while
    // separately holding mutable refs to the three target maps.
    let world = &vx.world;
    let emitter_positions = &mut vx.emitter_positions;
    let torch_positions = &mut vx.torch_positions;
    let fires = &mut vx.fires;
    let portal_positions = &mut vx.portal_positions;
    let mut scanned = 0usize;
    for (coord, chunk) in world.iter_chunks() {
        scan_chunk(
            *coord,
            chunk,
            emitter_positions,
            torch_positions,
            fires,
            portal_positions,
        );
        scanned += 1;
    }
    tracing::info!(
        scanned,
        emitters = emitter_positions.len(),
        torches = torch_positions.len(),
        fires = fires.len(),
        elapsed_ms = start.elapsed().as_millis() as u64,
        "W46.20: emitter maps rebuilt from loaded chunks",
    );
}
