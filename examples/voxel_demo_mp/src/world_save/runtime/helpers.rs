//! Per-section builders + appliers + small utilities used by
//! `runtime::save_world` / `runtime::load_world`.
//!
//! Pulled out so the orchestration in `mod.rs` stays a 200-LOC flat
//! sequence of "read meta -> write chunks -> ...". Each helper here
//! owns one of the typed conversions between in-memory `Voxel` state
//! and the typed save schema.

use std::io;

use infengine_voxel::{BlockId, CHUNK_SIZE, ChunkCoord};
use tracing::debug;

use crate::drops::DropItem;
use crate::liquid::liquid_for;
use crate::mob_ai::MobState;
use crate::{DAY_LENGTH_S, PLAYER_MAX_HEALTH, Voxel};

use super::super::chunks::ChunkPayload;
use super::super::entities::{DimensionEntities, DropSave, MobSave};
use super::super::players::{
    DEFAULT_HOST_PERMISSION, PLAYER_FORMAT_VERSION, PlayerSave,
};
use super::super::timers::{
    ScheduledTimers, TIMERS_FORMAT_VERSION, Timer, TimerKind,
    catch_up::CatchUpEvent,
};
use super::super::current_wall_ms;
use super::super::entities::ENTITIES_FORMAT_VERSION;
use super::OVERWORLD;

/// Build a `PlayerSave` from the live host pose + inventory.
pub(super) fn build_player_save(voxel: &Voxel) -> PlayerSave {
    let inventory: Vec<Option<(u16, u8)>> = voxel
        .inventory
        .iter()
        .map(|s| s.map(|(b, c)| (b.0, c)))
        .collect();
    // The runtime craft grid is 2x2 (4 slots) but the save schema
    // budgets a 3x3 grid (9 slots) so Phase D's larger grid lands
    // without bumping the format. Pad the unused slots with `None`.
    let mut craft: Vec<Option<(u16, u8)>> = vec![None; 9];
    for (i, slot) in voxel.craft_grid.iter().enumerate() {
        if i >= craft.len() {
            break;
        }
        if let Some((b, c)) = slot {
            craft[i] = Some((b.0, *c));
        }
    }
    PlayerSave {
        format_version: PLAYER_FORMAT_VERSION,
        pos: [voxel.player_pos.x, voxel.player_pos.y, voxel.player_pos.z],
        yaw: voxel.player_yaw,
        pitch: voxel.player_pitch,
        inventory,
        craft_grid: craft,
        selected_slot: voxel.selected_slot.min(u8::MAX as usize) as u8,
        air: voxel.player_air,
        health: voxel.player_health,
        home_pos: voxel.home_pos,
        spawn_pos: voxel.spawn_pos,
        permission_level: DEFAULT_HOST_PERMISSION,
        play_time_seconds: 0,
        last_dimension: OVERWORLD.to_string(),
    }
}

/// Apply a `PlayerSave` back onto `voxel`. Resets pose, health,
/// inventory, etc. The day-clock + kill counter live in the world
/// payload, not the player file - those stay untouched here.
pub(super) fn apply_player_save(voxel: &mut Voxel, save: &PlayerSave) {
    voxel.player_pos = glam::Vec3::new(save.pos[0], save.pos[1], save.pos[2]);
    voxel.player_yaw = save.yaw;
    voxel.player_pitch = save.pitch;
    for (i, slot) in save.inventory.iter().enumerate() {
        if i >= voxel.inventory.len() {
            break;
        }
        voxel.inventory[i] = slot.map(|(b, c)| (BlockId(b), c));
    }
    // Craft grid is 2x2 in the runtime, 3x3 in the save - only the
    // first 4 slots round-trip.
    for (i, slot) in save.craft_grid.iter().enumerate() {
        if i >= voxel.craft_grid.len() {
            break;
        }
        voxel.craft_grid[i] = slot.map(|(b, c)| (BlockId(b), c));
    }
    voxel.selected_slot = (save.selected_slot as usize).min(voxel.inventory.len() - 1);
    voxel.player_air = save.air;
    voxel.player_health = save.health.clamp(0.0, PLAYER_MAX_HEALTH);
    voxel.home_pos = save.home_pos;
    voxel.spawn_pos = save.spawn_pos;
    // Re-anchor the day clock so a load right after a save doesn't
    // jump the sun. `day_time` itself lives in `meta.json` once Phase
    // C lands; for now the runtime keeps its value across reloads.
    voxel.day_time = voxel.day_time.rem_euclid(DAY_LENGTH_S);
}

/// Build a `DimensionEntities` snapshot from `voxel.mobs` +
/// `voxel.dropped_items`. Both lists are already host-authoritative;
/// we just translate to the save schema.
pub(super) fn build_dimension_entities(voxel: &Voxel) -> DimensionEntities {
    let mobs: Vec<MobSave> = voxel
        .mobs
        .iter()
        .map(|m| MobSave {
            id: m.id,
            kind: m.kind,
            pos: m.pos,
            yaw: m.yaw,
            vel: [0.0, 0.0, 0.0],
            health: m.health,
            age_seconds: 0.0,
        })
        .collect();
    let drops: Vec<DropSave> = voxel
        .dropped_items
        .iter()
        .map(|d| DropSave {
            id: d.id,
            block: d.block,
            pos: d.pos,
            vel: d.vel,
            age: d.age,
        })
        .collect();
    DimensionEntities {
        format_version: ENTITIES_FORMAT_VERSION,
        mobs,
        drops,
        next_mob_id: voxel.next_mob_id,
        next_drop_id: voxel.next_drop_id,
    }
}

/// Apply a loaded `DimensionEntities` back onto `voxel`. Replaces
/// the live mob + drop lists; the runtime broadcast loop will
/// re-emit spawn messages on the next net-tick frame.
pub(super) fn apply_dimension_entities(voxel: &mut Voxel, entities: DimensionEntities) {
    voxel.mobs.clear();
    voxel.remote_mobs.clear();
    voxel.dropped_items.clear();
    for m in entities.mobs {
        // Drop mobs with zero / negative health - they were dead on
        // last save and the runtime would only despawn them again.
        if m.health <= 0.0 {
            continue;
        }
        voxel.mobs.push(MobState {
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
    for d in entities.drops {
        voxel.dropped_items.push(DropItem {
            id: d.id,
            block: d.block,
            pos: d.pos,
            vel: d.vel,
            age: d.age,
        });
    }
    voxel.next_mob_id = entities
        .next_mob_id
        .max(voxel.mobs.iter().map(|m| m.id).max().unwrap_or(0) + 1);
    voxel.next_drop_id = entities
        .next_drop_id
        .max(voxel.dropped_items.iter().map(|d| d.id).max().unwrap_or(0) + 1);
}

/// Build a `ScheduledTimers` bundle from the live `LiquidSchedule`.
/// Furnaces / crops don't exist yet; their builders land in Phase C.
pub(super) fn build_timers(voxel: &Voxel) -> ScheduledTimers {
    let now = current_wall_ms();
    let mut items: Vec<Timer> = Vec::new();
    for pos in voxel.liquid_schedule.iter() {
        // The schedule stores `due_at_tick` privately; for the save
        // we approximate with "fire on the next boot's catch-up
        // cycle". That keeps the spread paused while the world is
        // offline (no time-warp acceleration) but resumes it the
        // moment the player logs back in. Phase B follow-up surfaces
        // the actual due-tick if cross-boot accuracy is needed.
        let liquid_id = voxel.world.get(pos).0;
        items.push(Timer {
            pos,
            dim: OVERWORLD.to_string(),
            due_at_wall_ms: now,
            kind: TimerKind::LiquidScheduled { liquid_id },
        });
    }
    ScheduledTimers {
        format_version: TIMERS_FORMAT_VERSION,
        items,
        last_saved_wall_ms: now,
    }
}

/// Apply the events emitted by `timers::catch_up`. Furnaces + crops
/// land here once Phase C wires them up; for now the only variant we
/// see is `LiquidScheduled`, which `hydrate_liquid_schedule` consumes.
pub(super) fn apply_catch_up_events(_voxel: &mut Voxel, events: &[CatchUpEvent]) {
    for evt in events {
        match evt {
            CatchUpEvent::FurnaceOutput { pos, .. } => {
                debug!(?pos, "world_save: furnace catch-up event (Phase C TODO)");
            }
            CatchUpEvent::CropHarvested { pos, .. } => {
                debug!(?pos, "world_save: crop catch-up event (Phase C TODO)");
            }
            CatchUpEvent::LiquidScheduled { .. } => {
                // Handled by hydrate_liquid_schedule.
            }
        }
    }
}

/// Re-arm the live `LiquidSchedule` from `LiquidScheduled` events.
/// Each event becomes a fresh schedule entry at `liquid_clock +
/// tick_period` so the spread picks up where it left off.
pub(super) fn hydrate_liquid_schedule(voxel: &mut Voxel, events: &[CatchUpEvent]) {
    for evt in events {
        if let CatchUpEvent::LiquidScheduled { pos, .. } = evt {
            let b = voxel.world.get(*pos);
            if let Some(lq) = liquid_for(b) {
                let when = voxel.liquid_clock + (lq.tick_period.max(1)) as u64;
                voxel.liquid_schedule.schedule(*pos, when);
                voxel.liquid_positions.insert(*pos);
            }
        }
    }
}

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
    if let Some(chunk_ref) = voxel.world.chunk(coord) {
        let cs = CHUNK_SIZE as i32;
        for ly in 0..cs {
            for lz in 0..cs {
                for lx in 0..cs {
                    let b = chunk_ref.get(lx as usize, ly as usize, lz as usize);
                    let world_pos = [origin[0] + lx, origin[1] + ly, origin[2] + lz];
                    if liquid_for(b).is_some() {
                        voxel.liquid_positions.insert(world_pos);
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
pub(super) fn collect_for_chunk<T: Copy>(
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
