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

use infengine_voxel::ChunkCoord;
use tracing::{info, warn};

use crate::worldgen::{WorldGen, set_world_seed, world_seed};
use crate::Voxel;

use super::chunks::{self, CHUNK_PAYLOAD_VERSION, ChunkPayload, ChunkVoxels};
use super::entities::{self, DimensionEntities};
use super::layout;
use super::meta::{self, create_meta};
use super::players::{self, fresh_player};
use super::timers::{self, MAX_OFFLINE_MS, ScheduledTimers, catch_up::catch_up};
use super::world_json::{self, create_world};
use super::{LoadedWorld, current_wall_ms, set_worldgen, worldgen_str};

pub(crate) mod helpers;

use helpers::{
    apply_catch_up_events, apply_chunk_payload, apply_dimension_entities,
    apply_player_save, build_dimension_entities, build_player_save, build_timers,
    collect_for_chunk, hydrate_liquid_schedule, parse_chunk_filename,
    session_seconds_since_save,
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
pub(crate) fn save_world(voxel: &mut Voxel, slug: &str) -> io::Result<()> {
    let world_dir = layout::world_dir(slug);
    std::fs::create_dir_all(&world_dir)?;

    // 1. Meta. Read-modify-write so `play_time_seconds` accumulates
    //    across saves. A brand-new slot mints a fresh meta.
    let mut meta = match meta::read_meta(slug) {
        Ok(m) => m,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            // Borrow the slug as the display name. The Worlds picker
            // shows both, so the user can rename later via meta.json.
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
    if world_json::read_world(slug).is_err() {
        // read_world returns Err(NotFound) when the file is missing;
        // any other error already logged a corruption warning via the
        // recovery layer, but we still re-mint here so a corrupt
        // world.json doesn't block the save loop.
        let wj = create_world(worldgen_str(WorldGen::current()), world_seed());
        world_json::write_world(slug, &wj)?;
    }

    // 3. Chunks. Iterate every loaded chunk + collect per-chunk
    //    decoration overrides.
    let coords: Vec<ChunkCoord> = voxel.world.iter_chunks().map(|(c, _)| *c).collect();
    let mut chunk_count: usize = 0;
    for coord in coords {
        // Borrow split: iter_chunks() holds an immutable borrow; we
        // need to drop it before we can call write_chunk. Re-fetch
        // the chunk reference here so each iteration is self-contained.
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
        chunks::write_chunk(slug, OVERWORLD, &payload)?;
        chunk_count += 1;
    }

    // 4. Host player.
    let player = build_player_save(voxel);
    players::write_player(slug, HOST_PLAYER, &player)?;

    // 5. Entities (mobs + drops).
    let dim_entities = build_dimension_entities(voxel);
    entities::write_entities(slug, OVERWORLD, &dim_entities)?;

    // 6. Timers. Drain the live `LiquidSchedule` into TimerKind::
    //    LiquidScheduled entries. Furnace + crop timers will join here
    //    once Phase C lands; the current world has none.
    let timers_bundle = build_timers(voxel);
    timers::write_timers(slug, &timers_bundle)?;

    voxel.last_state_save = Some(std::time::Instant::now());
    voxel.note_save();
    info!(
        slug = slug,
        chunks = chunk_count,
        mobs = dim_entities.mobs.len(),
        drops = dim_entities.drops.len(),
        liquid_timers = timers_bundle.items.len(),
        "world_save: save_world ok"
    );
    Ok(())
}

/// Load `slug` into `voxel`. See `super::load_world` for the public
/// contract.
pub(crate) fn load_world(voxel: &mut Voxel, slug: &str) -> io::Result<LoadedWorld> {
    let meta = meta::read_meta(slug)?;
    let world_meta = world_json::read_world(slug)?;

    // Pin worldgen + seed from the on-disk record.
    let wg = WorldGen::from_str(&world_meta.worldgen).unwrap_or(WorldGen::Classic);
    set_worldgen(wg);
    if world_meta.seed != 0 {
        set_world_seed(world_meta.seed);
    }

    // Drop any chunks the runtime had already streamed in (e.g. from
    // a previous slot in the same session) so we don't end up with a
    // mix of worldgens. `reset_world` clears renderer handles + the
    // per-chunk auxiliary maps as well.
    voxel.reset_world();

    // Load chunks.
    let mut chunks_loaded: usize = 0;
    let chunks_dir = layout::world_dir(slug)
        .join("chunks")
        .join(OVERWORLD);
    if chunks_dir.is_dir() {
        let read = std::fs::read_dir(&chunks_dir)?;
        for entry in read.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("bin") {
                continue;
            }
            let Some(coord) = parse_chunk_filename(&path) else {
                warn!(path = %path.display(), "world_save: skipping unrecognised chunk filename");
                continue;
            };
            let payload = match chunks::read_chunk(slug, OVERWORLD, coord)? {
                Some(p) => p,
                None => continue,
            };
            apply_chunk_payload(voxel, payload)?;
            chunks_loaded += 1;
        }
    }

    // Load the host player file (or mint a fresh one).
    let player = players::read_player(slug, HOST_PLAYER)?
        .unwrap_or_else(|| fresh_player(world_meta.spawn));
    apply_player_save(voxel, &player);

    // Load entities (mobs + drops).
    let dim_entities = entities::read_entities(slug, OVERWORLD)?
        .unwrap_or_else(DimensionEntities::empty);
    apply_dimension_entities(voxel, dim_entities);

    // Timers + catch-up.
    let now_ms = current_wall_ms();
    let mut bundle = timers::read_timers(slug)?
        .unwrap_or_else(|| ScheduledTimers::empty(now_ms));
    // Cap the elapsed window the same way `timers::catch_up` does so
    // a year-old save doesn't burn minutes simulating. The catch-up
    // itself enforces the cap; mirroring it here just keeps the
    // logged elapsed-ms honest.
    let elapsed = now_ms
        .saturating_sub(bundle.last_saved_wall_ms)
        .min(MAX_OFFLINE_MS);
    let events = catch_up(&mut bundle, now_ms);
    apply_catch_up_events(voxel, &events);
    hydrate_liquid_schedule(voxel, &events);
    // Persist the bumped `last_saved_wall_ms` so the next boot
    // measures elapsed from `now`, not from the previous save.
    if let Err(err) = timers::write_timers(slug, &bundle) {
        warn!(error = %err, "world_save: write_timers post-catch-up failed");
    }

    voxel.current_world_slug = Some(slug.to_string());
    voxel.last_state_save = Some(std::time::Instant::now());
    voxel.rebuild_all_meshes();
    info!(
        slug = slug,
        chunks = chunks_loaded,
        elapsed_ms = elapsed,
        catch_up_events = events.len(),
        "world_save: load_world ok"
    );
    Ok(LoadedWorld {
        meta,
        world: world_meta,
    })
}
