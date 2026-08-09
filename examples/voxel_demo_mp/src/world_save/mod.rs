//! Save / load orchestration for the host's authoritative world.
//!
//! Phase B.7a wires the multi-world `world_save` layer (`meta.json`,
//! `world.json`, per-chunk files, per-player files, per-dimension
//! entities, scheduled timers) into the runtime through
//! `Voxel::save_server_state` / `Voxel::load_server_state` (still the
//! thin shims in `main.rs`, now calling `save_world` / `load_world`
//! here). The legacy single-file `voxel_server_state.bin` flow is
//! gone; the very first run that still has that file lying around
//! gets it migrated into a `worlds/legacy/` slot via
//! `migrate_legacy_if_needed`.
//!
//! Layout (per the design doc):
//!
//! ```text
//! <data_dir>/InfEngine/voxel_demo_mp/worlds/<slug>/
//!   meta.json                        # human-readable header
//!   world.json                       # worldgen + seed + spawn
//!   chunks/overworld/cx_cy_cz.chunk.bin
//!   players/<name>.bin               # per-player save
//!   entities/overworld.bin           # mobs + dropped items
//!   timers/scheduled.bin             # furnace / crop / liquid ticks
//! ```
//!
//! The heavy lifting (build PlayerSave / DimensionEntities from
//! `Voxel`, iterate chunks, decode timers + dispatch catch-up) lives
//! in `runtime.rs` to keep this file under the 500-LOC soft cap.
//! Public surface: `save_world`, `load_world`, `LoadedWorld`,
//! `migrate_legacy_if_needed`.

use std::io;

use tracing::{info, warn};

use crate::Voxel;

pub(crate) mod atomic;
pub(crate) mod chunks;
pub(crate) mod containers;
pub(crate) mod entities;
pub(crate) mod entities_v9;
pub(crate) mod fixups;
pub(crate) mod layout;
pub(crate) mod meta;
pub(crate) mod migrate;
pub(crate) mod players;
pub(crate) mod players_v7;
pub(crate) mod preflight;
pub(crate) mod runtime;
pub(crate) mod timers;
pub(crate) mod world_json;

#[cfg(test)]
mod atomic_tests;
#[cfg(test)]
mod chunks_tests;
#[cfg(test)]
mod containers_tests;
#[cfg(test)]
mod containers_v2_tests;
#[cfg(test)]
mod containers_v3_tests;
#[cfg(test)]
mod entities_tests;
#[cfg(test)]
mod migrate_tests;
#[cfg(test)]
mod players_tests;
#[cfg(test)]
mod multi_dim_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod timers_tests;

/// Path of the pre-A4 legacy server-side world delta file. Lives next
/// to the launch directory; `migrate_legacy_if_needed` relocates it
/// into `worlds/legacy/` once the multi-world layer takes over.
pub(crate) const STATE_FILE: &str = "voxel_server_state.bin";

/// Default slug used when the runtime has no `current_world_slug`
/// (e.g. legacy-migrated saves and the very first launch on a fresh
/// install).
pub(crate) const DEFAULT_SLUG: &str = "default";

/// Result bundle returned from `load_world`. Carries the parsed
/// `meta.json` + `world.json` so the caller can adjust UI state
/// (Worlds picker selection, last-played sort) and surface the
/// "playing slot" badge without re-reading the files.
///
/// `#[allow(dead_code)]` because the runtime caller currently
/// discards the bundle - the Worlds picker hook (Phase G follow-up)
/// is what reads these fields. Keeping the struct populated now
/// means we don't have to plumb a second loader return path later.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct LoadedWorld {
    pub(crate) meta: meta::WorldMeta,
    pub(crate) world: world_json::WorldJson,
}

/// Persist the full world state for `slug`. Idempotent: a follow-up
/// call writes the same set of files (with updated timestamps + any
/// dirty chunks since the previous call). Atomic per-file via
/// `atomic::atomic_save`.
///
/// Wiring summary:
/// - Updates `meta.json` (`last_played` + `play_time_seconds`).
/// - Creates `world.json` on first save (worldgen + seed pinned at
///   creation; subsequent saves leave it alone).
/// - Iterates every loaded chunk + writes its `ChunkPayload`.
/// - Builds a `PlayerSave` for the local host + writes it under
///   `players/host.bin`.
/// - Builds a `DimensionEntities` from mobs + drops + writes
///   `entities/overworld.bin`.
/// - Drains the in-memory `LiquidSchedule` into a
///   `ScheduledTimers` bundle + writes `timers/scheduled.bin`.
pub(crate) fn save_world(voxel: &mut Voxel, slug: &str) -> io::Result<()> {
    runtime::save_world(voxel, slug)
}

/// W46.24: write a single chunk to disk WITHOUT touching the rest
/// of the world. Used by the chunk-unload path so edits made past
/// the stream radius (e.g. a torch the player placed then walked
/// away from) persist instead of evaporating when the chunk gets
/// evicted from memory.
#[allow(dead_code)] // W46.24 sync entry point; W46.28 main path uses async build/write split
pub(crate) fn save_one_chunk(
    voxel: &Voxel,
    slug: &str,
    dim: &str,
    coord: infengine_voxel::ChunkCoord,
) -> io::Result<()> {
    runtime::save_one_chunk(voxel, slug, dim, coord)
}

// W233: the per-chunk `build_chunk_payload` wrapper is gone - the
// unload path uses `runtime::build_chunk_payloads` (batch form, ONE
// pass per side map for the whole unload set). The per-chunk fn
// survives inside `runtime` for the single-chunk save path.

/// W46.28: write a pre-built `ChunkPayload` to disk. Pure I/O,
/// safe to call from a background thread.
pub(crate) fn write_chunk_payload(
    slug: &str,
    dim: &str,
    payload: &chunks::ChunkPayload,
) -> io::Result<()> {
    runtime::write_chunk_payload(slug, dim, payload)
}

/// Load `slug` into `voxel`. On a fresh slot (no `meta.json` yet)
/// the call returns `io::ErrorKind::NotFound` so the caller can
/// decide whether to mint a new world (default behaviour: yes).
///
/// Wiring summary (mirrors `save_world` in reverse):
/// - Parses `meta.json` + `world.json`. Refuses unsupported
///   `format_version` values unless a migration is registered.
/// - Pins the global `WorldGen` + `world_seed` from `world.json`.
/// - Inserts every chunk file's voxels into `voxel.world`, plus
///   per-cell liquid level / torch orient / snow layer overrides.
/// - Loads the host player file into `voxel`.
/// - Re-hydrates mobs + drops from `entities/overworld.bin`.
/// - Reads `timers/scheduled.bin`, runs the catch-up dispatch
///   against wall-now, applies the resulting events, and re-arms
///   the `LiquidSchedule` from the LiquidScheduled events.
pub(crate) fn load_world(voxel: &mut Voxel, slug: &str) -> io::Result<LoadedWorld> {
    runtime::load_world(voxel, slug)
}

/// W249: apply a saved world's meta (active dim, seed, per-dim
/// worldgen profiles, day time) + return the host player's saved
/// position, WITHOUT touching chunks. Run by `enter_world` before
/// the initial async build so the build targets the right chunks
/// with the right profile.
pub(crate) fn apply_world_meta(voxel: &mut Voxel, slug: &str) -> io::Result<[f32; 3]> {
    runtime::apply_world_meta(voxel, slug)
}

/// One-shot legacy migration. If `worlds/` does not yet exist as a
/// directory tree AND a `voxel_server_state.bin` does live in the
/// current working directory, build a `worlds/legacy/` slot from it.
///
/// Steps:
/// 1. Detect both conditions; bail with `Ok(None)` if either is
///    missing.
/// 2. Create the legacy slot directory + write a minimal
///    `meta.json` + `world.json` so `list_worlds()` surfaces it.
/// 3. Move the legacy file aside to
///    `voxel_server_state.bin.migrated` so the next launch doesn't
///    re-run this path.
///
/// The actual contents of the legacy file are not decoded here -
/// the file format is `INFV`-magic bincode + serde and we treat it
/// as opaque bytes. A follow-up loader (a registered migration arrow)
/// can pull data out of it later. For now the migration creates a
/// fresh slot pointing at a zero-edit baseline so the player sees
/// the slot in the Worlds picker.
pub(crate) fn migrate_legacy_if_needed() -> io::Result<Option<String>> {
    let legacy_file = std::path::PathBuf::from(STATE_FILE);
    let worlds_root = layout::worlds_dir();

    // Only run on the first launch after Phase B.7a: legacy file
    // exists AND the new worlds tree is still empty.
    if !legacy_file.exists() {
        return Ok(None);
    }
    if let Ok(read) = std::fs::read_dir(&worlds_root) {
        // Treat any pre-existing slot as "migration already happened" -
        // don't trample a slot the player just created.
        let has_entry = read.flatten().any(|e| e.path().is_dir());
        if has_entry {
            return Ok(None);
        }
    }

    let slug = "legacy";
    let world_dir = layout::world_dir(slug);
    std::fs::create_dir_all(&world_dir)?;

    // Mint a minimal meta + world.json so the Worlds menu surfaces
    // the slot. The actual edit history sits in the legacy bin file;
    // a future migration arrow can decode it into chunks + players.
    let mut meta = meta::create_meta("Legacy save", slug);
    // W451: a legacy bin save predates the data-version field -
    // keep the fixup gate open.
    meta.data_version = 0;
    meta::write_meta(slug, &meta)?;
    // W5.1: legacy save migration lands with safe defaults
    // (Classic + seed 0). The runtime's load path will overwrite
    // voxel.base_seed + dim_worldgens from the world.json the next
    // time the slot opens.
    let world = world_json::create_world(
        worldgen_str(crate::worldgen::WorldGen::Classic),
        0,
    );
    world_json::write_world(slug, &world)?;

    // Move the old file aside so the next launch doesn't re-trigger
    // the migration. A rename keeps the bytes around for a future
    // migration arrow without leaving them on the load path.
    let aside = legacy_file.with_extension("bin.migrated");
    if let Err(err) = std::fs::rename(&legacy_file, &aside) {
        warn!(
            error = %err,
            from = %legacy_file.display(),
            to = %aside.display(),
            "migrate_legacy: failed to move legacy file aside"
        );
    } else {
        info!(
            from = %legacy_file.display(),
            to = %aside.display(),
            slug = slug,
            "migrate_legacy: moved legacy save into worlds/legacy/"
        );
    }

    Ok(Some(slug.to_string()))
}

/// Stringify a `WorldGen` variant into the tag used inside
/// `world.json`. Mirror of `WorldGen::from_str` so a round trip
/// through the on-disk JSON preserves the variant.
pub(crate) fn worldgen_str(wg: crate::worldgen::WorldGen) -> &'static str {
    match wg {
        crate::worldgen::WorldGen::Classic => "classic",
        crate::worldgen::WorldGen::Flat => "flat",
        crate::worldgen::WorldGen::Islands => "islands",
        crate::worldgen::WorldGen::Amplified => "amplified",
        crate::worldgen::WorldGen::Realistic => "realistic",
        crate::worldgen::WorldGen::Nether => "nether",
        crate::worldgen::WorldGen::Natural => "natural",
        crate::worldgen::WorldGen::Mc => "mc",
        crate::worldgen::WorldGen::End => "end",
    }
}

// W5.1: `set_worldgen` removed alongside the WORLDGEN atomic.
// Worldgen state now lives in `Voxel::dim_worldgens` and is
// passed by reference via `WorldgenCtx`.

/// Best-effort wall-clock now in ms since UNIX epoch. Falls back to
/// 0 on the (essentially impossible) `SystemTime::now() < UNIX_EPOCH`.
pub(crate) fn current_wall_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Probe `worlds/<slug>/` for a `meta.json`. Lighter than
/// `layout::list_worlds` when the caller only needs the "does the
/// slot exist?" answer.
#[allow(dead_code)]
pub(crate) fn slot_exists(slug: &str) -> bool {
    layout::meta_path(slug).exists()
}
