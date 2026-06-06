//! Runtime glue between the typed save-layer modules and the live
//! `Voxel` runtime.
//!
//! `world_save::mod` is the public surface (`save_world`, `load_world`,
//! `migrate_legacy_if_needed`). This sub-module owns the actual
//! builders + appliers: turn the in-memory chunks + player + mobs +
//! drops + liquid schedule into `ChunkPayload` / `PlayerSave` /
//! `DimensionEntities` / `ScheduledTimers`, and apply each back on
//! load.
//!
//! Layout:
//! - `mod.rs` (this file): the two entry points `save_world` /
//!   `load_world` + the constants they share.
//! - `helpers.rs`: per-section builders + appliers + coord helpers.
//!
//! ## Iteration policy
//!
//! `save_world` iterates ALL loaded chunks, not just dirty ones. A
//! follow-up optimisation (dirty-chunk tracking on
//! `world_set_tracked` + the chunk streamer) lands in a later
//! commit; for B.7a the conservative "rewrite every loaded chunk"
//! matches the design doc's Phase B baseline.

use std::io;

use tracing::{info, warn};

use crate::worldgen::WorldGen;
use crate::Voxel;

use super::layout;
use super::meta::{self, create_meta};
use super::players::{self, fresh_player};
use super::timers::{self, MAX_OFFLINE_MS, ScheduledTimers, catch_up::catch_up};
use super::world_json::{self, create_world};
use super::{LoadedWorld, current_wall_ms, worldgen_str};

pub(crate) mod dims;
pub(crate) mod helpers;

use helpers::{
    apply_catch_up_events, apply_player_save, build_player_save, build_timers,
    hydrate_liquid_schedule, session_seconds_since_save,
};

/// Dimension tag used for the host's overworld. Phase F extends this
/// to a per-dim sweep over `world.json.dimensions`.
pub(super) const OVERWORLD: &str = "overworld";

/// Player file name used for the host. On a dedicated server every
/// remote client gets a file too (`net.player_name` -> sanitized);
/// for now we serialise only the host pose.
pub(super) const HOST_PLAYER: &str = "host";

/// Persist the full world state for `slug`. See `super::save_world`
/// for the public contract.
///
/// Phase F: iterates every dim in `world.json::dimensions`. The
/// active dim's state lives in the top-level Voxel fields; parked
/// dims read from `voxel.worlds` + `voxel.entities_by_dim`. The
/// timers bundle is single-file - each `Timer` carries its own
/// `dim` tag, so the catch-up dispatcher routes events correctly
/// on load.
pub(crate) fn save_world(voxel: &mut Voxel, slug: &str) -> io::Result<()> {
    let world_dir = layout::world_dir(slug);
    std::fs::create_dir_all(&world_dir)?;

    // 1. Meta. Read-modify-write so `play_time_seconds` accumulates
    //    across saves. A brand-new slot mints a fresh meta.
    let mut meta = match meta::read_meta(slug) {
        Ok(m) => m,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            create_meta(slug, slug)
        }
        Err(err) => return Err(err),
    };
    meta.last_played = chrono::Utc::now();
    meta.play_time_seconds = meta
        .play_time_seconds
        .saturating_add(session_seconds_since_save(voxel));
    meta::write_meta(slug, &meta)?;

    // 2. world.json: write only if missing (worldgen + seed are
    //    pinned at creation).
    // W46.31: also rewrite when day_time drifted from disk so the
    //   sun position survives across restarts. Cheap (a few hundred
    //   bytes of JSON).
    let world_json_value = match world_json::read_world(slug) {
        Ok(mut wj) => {
            if (wj.day_time - voxel.day_time).abs() > 0.5 {
                wj.day_time = voxel.day_time;
                world_json::write_world(slug, &wj)?;
            }
            wj
        }
        Err(_) => {
            // Missing OR corrupt - mint a fresh one. The base seed
            // and active dim's profile come straight from voxel
            // state (W5.1: no global atomics in the call graph).
            let active_wg = voxel
                .dim_worldgens
                .get(&voxel.current_dim)
                .copied()
                .unwrap_or(WorldGen::Classic);
            let mut wj = create_world(
                worldgen_str(active_wg),
                voxel.base_seed,
            );
            wj.day_time = voxel.day_time; // W46.31
            world_json::write_world(slug, &wj)?;
            wj
        }
    };

    // 3. Per-dim chunks + entities + containers.
    let chunk_count = dims::save_all_dims(voxel, slug, &world_json_value)?;

    // 4. Host player.
    let player = build_player_save(voxel);
    players::write_player(slug, HOST_PLAYER, &player)?;

    // 5. Timers (single file; each Timer carries its own `dim`).
    let timers_bundle = build_timers(voxel);
    timers::write_timers(slug, &timers_bundle)?;

    voxel.last_state_save = Some(std::time::Instant::now());
    voxel.note_save();
    info!(
        slug = slug,
        chunks = chunk_count,
        dims = world_json_value.dimensions.len(),
        active_dim = %voxel.current_dim,
        liquid_timers = timers_bundle.items.len(),
        "world_save: save_world ok"
    );
    Ok(())
}

/// Load `slug` into `voxel`. See `super::load_world` for the public
/// contract.
///
/// Phase F: every dimension in `world.json::dimensions` lands either
/// in the active Voxel slots (the dim matching the player's
/// `last_dimension`) or in the per-dim parking maps. The base seed
/// and per-dim worldgen profiles come from `world.json` and land in
/// `voxel.base_seed` + `voxel.dim_worldgens`. W5.1: no global
/// atomics are touched - every gen call rebuilds a `WorldgenCtx`
/// from these fields.
pub(crate) fn load_world(voxel: &mut Voxel, slug: &str) -> io::Result<LoadedWorld> {
    let meta = meta::read_meta(slug)?;
    let world_meta = world_json::read_world(slug)?;

    // Drop any state the runtime had streamed in for a previous
    // slot.  Also clears the per-dim parking maps so we don't mix
    // dims from two saves.
    voxel.reset_world();
    voxel.worlds.clear();
    voxel.entities_by_dim.clear();
    voxel.dim_worldgens.clear();

    // Resolve the active dim. The player file is the source of
    // truth for `last_dimension`; fall back to the world's default
    // dimension when no player file exists.
    let player = players::read_player(slug, HOST_PLAYER)?;
    let last_dim = player
        .as_ref()
        .map(|p| p.last_dimension.clone())
        .unwrap_or_else(|| world_meta.default_dimension.clone());
    // Reject an active dim that isn't in the declared list - fall
    // back to the world default so a corrupt player file doesn't
    // strand the loader.
    let active_dim = if world_meta.dimensions.contains(&last_dim) {
        last_dim
    } else {
        world_meta.default_dimension.clone()
    };
    voxel.current_dim = active_dim.clone();
    voxel.base_seed = world_meta.seed;
    // W46.31: restore the saved time-of-day so the sun resumes where
    // the player left it instead of jumping to the engine's default.
    voxel.day_time = world_meta.day_time;
    // Per-dim worldgens table; populate before per-dim load so
    // each dim's chunk-gen can see its assigned variant.
    dims::apply_dim_worldgens(voxel, &world_meta);

    // W5.1: the dim_worldgens map (above) is the single source of
    // truth for the active dim's profile; no atomics to pin.
    // Apply the world.json fallback into the map when the map
    // lacks an entry so future ctx reads pick up a sensible
    // default rather than landing on Classic by accident.
    if !voxel.dim_worldgens.contains_key(&active_dim) {
        let fallback = WorldGen::from_str(&world_meta.worldgen)
            .unwrap_or(WorldGen::Classic);
        voxel.dim_worldgens.insert(active_dim.clone(), fallback);
    }

    // Per-dim chunks + entities + containers.
    let chunks_loaded = dims::load_all_dims(voxel, slug, &world_meta)?;

    // Player pose + inventory + last_dimension.
    // W69: a freshly created world (no player file yet) seeds its
    // player from the game mode chosen in the New World form, stored in
    // world.json's `default_game_mode`. Without this the fresh player's
    // Survival default overwrote a Creative pick via apply_player_save
    // below, so "I selected Creative on create" had no effect.
    let player_save = match player {
        Some(p) => p,
        None => {
            // W86: a freshly created world has no player file. The bare
            // world-spawn is [0,0,0] -> the player would load at the
            // bedrock floor, deep underground. The procedural chunks are
            // already in `voxel.world` by now, so resolve a real surface
            // spawn directly here rather than relying on a later lift.
            let mut fp = fresh_player(world_meta.spawn);
            if world_meta.default_game_mode.eq_ignore_ascii_case("creative") {
                fp.game_mode = 1; // PlayerSave game_mode: 1 = Creative
            }
            let s = voxel.safe_spawn();
            fp.pos = [s.x, s.y, s.z];
            fp
        }
    };
    apply_player_save(voxel, &player_save);

    // Timers + catch-up.
    let now_ms = current_wall_ms();
    let mut bundle = timers::read_timers(slug)?
        .unwrap_or_else(|| ScheduledTimers::empty(now_ms));
    let elapsed = now_ms
        .saturating_sub(bundle.last_saved_wall_ms)
        .min(MAX_OFFLINE_MS);
    let events = catch_up(&mut bundle, now_ms);
    apply_catch_up_events(voxel, &events);
    hydrate_liquid_schedule(voxel, &events);
    if let Err(err) = timers::write_timers(slug, &bundle) {
        warn!(error = %err, "world_save: write_timers post-catch-up failed");
    }

    voxel.current_world_slug = Some(slug.to_string());
    voxel.last_state_save = Some(std::time::Instant::now());
    voxel.rebuild_all_meshes();
    info!(
        slug = slug,
        chunks = chunks_loaded,
        dims = world_meta.dimensions.len(),
        active_dim = %voxel.current_dim,
        elapsed_ms = elapsed,
        catch_up_events = events.len(),
        "world_save: load_world ok"
    );
    Ok(LoadedWorld {
        meta,
        world: world_meta,
    })
}

// W5.1: the world_seed() / WORLDGEN atomics are gone; the
// re-export shim that used to keep the import alive is removed.

/// W46.24: write a single chunk's voxels + per-cell overrides
/// (liquid level, torch orient, snow layers) to disk WITHOUT
/// rewriting any other section. Used by the chunk-unload path so
/// edits in chunks the player walks away from persist before the
/// chunk gets evicted from memory.
///
/// `dim` should be the active dim tag for the chunk's source
/// `voxel.current_dim`. Parked dims should not unload chunks - they
/// stay resident in `voxel.worlds` until the player visits them.
#[allow(dead_code)] // W46.24 sync entry point; W46.28 main path uses async build/write split
pub(crate) fn save_one_chunk(
    voxel: &Voxel,
    slug: &str,
    dim: &str,
    coord: infengine_voxel::ChunkCoord,
) -> io::Result<()> {
    use super::chunks;
    let Some(payload) = build_chunk_payload(voxel, coord) else {
        return Ok(());
    };
    chunks::write_chunk(slug, dim, &payload)
}

/// W46.28: build the on-disk payload struct for `coord` WITHOUT
/// writing it. Used by the chunk-unload path so the main thread
/// can collect a Vec of payloads in microseconds and hand them off
/// to a detached background thread for the file write. Without
/// this split the per-chunk disk write blocked the main thread by
/// ~1 ms each, adding up to a visible hitch when 20 chunks
/// scrolled past UNLOAD_DISTANCE in one frame.
pub(crate) fn build_chunk_payload(
    voxel: &Voxel,
    coord: infengine_voxel::ChunkCoord,
) -> Option<super::chunks::ChunkPayload> {
    use super::chunks::{CHUNK_PAYLOAD_VERSION, ChunkPayload, ChunkVoxels};
    use helpers::collect_for_chunk;
    let chunk = voxel.world.chunk(coord)?;
    Some(ChunkPayload {
        format_version: CHUNK_PAYLOAD_VERSION,
        coord,
        voxels: ChunkVoxels::from_chunk(chunk),
        liquid_levels: collect_for_chunk(&voxel.liquid_level, coord),
        torch_orient: collect_for_chunk(&voxel.torch_orient, coord),
        snow_layers: collect_for_chunk(&voxel.snow_positions, coord),
    })
}

/// W46.28: write a pre-built payload to disk. Pure I/O - safe to
/// call from a background thread because it takes only owned data.
pub(crate) fn write_chunk_payload(
    slug: &str,
    dim: &str,
    payload: &super::chunks::ChunkPayload,
) -> io::Result<()> {
    super::chunks::write_chunk(slug, dim, payload)
}
