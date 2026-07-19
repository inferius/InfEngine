//! W47.9 - fire tick runtime, relocated from main.rs.
//!
//! Server-only tick logic for the fire block:
//! - `fire_tick`: per-tick spread / age / extinguish pass
//! - `apply_fire_step`: fold a planned FireStep into world + side
//!   maps (ignite, consume, broadcast vx.edit)
//! - `tick_lava_fire_emit`: random-tick lava ignites flammable
//!   neighbours
//!
//! The pure-data fire planner stays in `block::fire`; this module
//! is the bin-only glue that mutates the live Voxel runtime + the
//! NetRole::Server broadcast surface.

use crate::*;
use crate::net::NetRole;
use infengine_voxel::BlockId;
pub(crate) fn fire_tick(vx: &mut crate::Voxel) {
        if matches!(vx.net, NetRole::Client { .. }) {
            return;
        }
        if vx.fires.is_empty() {
            return;
        }
        vx.fire_clock = vx.fire_clock.wrapping_add(1);
        let clock = vx.fire_clock;
        // Snapshot the keys so we can mutate `vx.fires` inside
        // the loop without invalidating an iterator borrow.
        let positions: Vec<[i32; 3]> = vx.fires.keys().copied().collect();
        let mut combined = block::fire::FireStep::default();
        for pos in positions {
            // Per-cell 1/RATE_DENOM gate. Salt 0x7000 separates
            // the gate roll from the in-tick decision salts.
            let h_gate = block::fire::hash3_for(pos, clock, 0x7000);
            if !h_gate.is_multiple_of(block::fire::RATE_DENOM) {
                continue;
            }
            block::fire::plan_fire_cell(
                &vx.world,
                &vx.fires,
                pos,
                clock,
                &mut combined,
            );
        }
        if combined.extinguish.is_empty()
            && combined.age_bump.is_empty()
            && combined.ignite.is_empty()
            && combined.consume.is_empty()
        {
            return;
        }
        apply_fire_step(vx, combined);
    }

    /// Commit a `FireStep` to the world + fires map + chunk mesh +
    /// network. Split from `fire_tick` so flint+steel and the
    /// lava-ignite path (Q.4d/e) can reuse the same commit shape.
pub(crate) fn apply_fire_step(vx: &mut crate::Voxel, step: crate::block::fire::FireStep) {
        let cfg = bincode::config::standard();
        let mut chunk_set: HashSet<ChunkCoord> = HashSet::new();
        // Extinguish first - clears the world block + fires entry.
        for pos in &step.extinguish {
            if !block::fire::is_fire_id(vx.world.get(*pos).0) {
                continue;
            }
            // W312: tracked write - the untracked set skipped the
            // lighting-dirty path, so a dying fire left its baked
            // glow behind (and a lit one never glowed at all).
            vx.world_set_tracked(*pos, BlockId::AIR);
            vx.fires.remove(pos);
            for c in crate::Voxel::affected_chunks(*pos) {
                chunk_set.insert(c);
            }
        }
        // Consume (flammable -> air). Block was non-fire; just clear.
        for pos in &step.consume {
            let here = vx.world.get(*pos);
            if here.is_air() || here.0 == block::FIRE_ID {
                continue;
            }
            // Don't drop the consumed block (MC: burned blocks
            // don't drop items).
            vx.world_set_tracked(*pos, BlockId::AIR);
            for c in crate::Voxel::affected_chunks(*pos) {
                chunk_set.insert(c);
            }
        }
        // Age bumps - update fires map only; world stays FIRE_ID.
        for (pos, age) in &step.age_bump {
            if !block::fire::is_fire_id(vx.world.get(*pos).0) {
                continue;
            }
            vx.fires.insert(*pos, *age);
        }
        // Ignite (replace flammable / air with fire). MUST run
        // after consume so a check_burn_out catch overrides the
        // implicit air write.
        for (pos, age) in &step.ignite {
            // Defensive: don't overwrite an already-set fire with
            // a stale ignite entry.
            let here = vx.world.get(*pos);
            if block::fire::is_fire_id(here.0) {
                // Already burning - just refresh age.
                vx.fires.insert(*pos, *age);
                continue;
            }
            // The ignite target must currently be air OR a
            // flammable block (the latter when check_burn_out
            // catches). For air, ensure support is present.
            if !here.is_air()
                && block::fire::block_flammability(here.0) == (0, 0)
            {
                continue;
            }
            // W312: MC BaseFireBlock.getState - SOUL fire over soul
            // sand / soul soil (light 10, blue), regular fire
            // elsewhere. Tracked write bakes the light.
            let below = vx.world.get([pos[0], pos[1] - 1, pos[2]]).0;
            let fire_id = if below == crate::block::SOUL_SAND_ID
                || below == crate::block::SOUL_SOIL_ID
            {
                crate::block::SOUL_FIRE_ID
            } else {
                block::FIRE_ID
            };
            vx.world_set_tracked(*pos, BlockId(fire_id));
            vx.fires.insert(*pos, *age);
            for c in crate::Voxel::affected_chunks(*pos) {
                chunk_set.insert(c);
            }
        }
        if !chunk_set.is_empty() {
            let coords: Vec<ChunkCoord> = chunk_set.into_iter().collect();
            vx.rebuild_chunks(&coords);
        }
        // Broadcast edits for every world-block change.
        if let NetRole::Server { server, .. } = &mut vx.net {
            for pos in &step.extinguish {
                let edit = EditMsg {
                    x: pos[0],
                    y: pos[1],
                    z: pos[2],
                    block: 0,
                };
                vx.edit_history.push(edit);
                if let Ok(buf) = bincode::serde::encode_to_vec(edit, cfg) {
                    server.broadcast_bytes("vx.edit", buf);
                }
            }
            for pos in &step.consume {
                let edit = EditMsg {
                    x: pos[0],
                    y: pos[1],
                    z: pos[2],
                    block: 0,
                };
                vx.edit_history.push(edit);
                if let Ok(buf) = bincode::serde::encode_to_vec(edit, cfg) {
                    server.broadcast_bytes("vx.edit", buf);
                }
            }
            for (pos, _) in &step.ignite {
                let edit = EditMsg {
                    x: pos[0],
                    y: pos[1],
                    z: pos[2],
                    block: block::FIRE_ID,
                };
                vx.edit_history.push(edit);
                if let Ok(buf) = bincode::serde::encode_to_vec(edit, cfg) {
                    server.broadcast_bytes("vx.edit", buf);
                }
            }
        }
    }

    /// Q.4d: lava source cells emit fire on flammable neighbours
    /// (MC LavaFluid::randomTick). W275: SAMPLED like MC's random
    /// ticks instead of iterating every source - the W260 deep lava
    /// sea puts ~2.5k lava cells in EVERY in-reach column, and the
    /// old full walk (world.get + level lookup per cell, then a hash
    /// gate) cost 4-23 ms per tick even after the W267 column index.
    /// Now each in-reach column rolls SAMPLES_PER_COLUMN random cells
    /// per tick and a 1-in-4 gate: a small surface pool (few cells)
    /// still ignites within seconds, while a deep-sea column costs
    /// two array picks - MC's 3-per-section sampling economics.
pub(crate) fn tick_lava_fire_emit(vx: &mut crate::Voxel) {
        if matches!(vx.net, NetRole::Client { .. }) {
            return;
        }
        if vx.lava_columns.is_empty() {
            return;
        }
        let clock = vx.fire_clock;
        // W233: same horizontal radius as the random-tick module
        // (columns tick full-height, like MC ticking chunks).
        let cs = infengine_voxel::CHUNK_SIZE as i32;
        let reach_chunks = crate::world::random_tick::RADIUS_CHUNKS + 1;
        let reach = reach_chunks * cs;
        let px = vx.player_pos.x.floor() as i32;
        let pz = vx.player_pos.z.floor() as i32;
        let (pcx, pcz) = (px.div_euclid(cs), pz.div_euclid(cs));
        const SAMPLES_PER_COLUMN: usize = 2;
        let mut step = block::fire::FireStep::default();
        // Counter-hash sampling off the fire clock (a live host-side
        // event - no worldgen replay constraint).
        let mut counter: u32 = clock as u32 ^ 0xA5A5_5A5A;
        let mut next = move || {
            counter ^= counter << 13;
            counter ^= counter >> 17;
            counter ^= counter << 5;
            counter
        };
        for dcx in -reach_chunks..=reach_chunks {
            for dcz in -reach_chunks..=reach_chunks {
                let Some(cells) = vx.lava_columns.get(&[pcx + dcx, pcz + dcz]) else {
                    continue;
                };
                if cells.is_empty() {
                    continue;
                }
                for _ in 0..SAMPLES_PER_COLUMN {
                    let pos = cells[next() as usize % cells.len()];
                    if (pos[0] - px).abs() > reach || (pos[2] - pz).abs() > reach {
                        continue;
                    }
                    // The sampled-attempt gate keeps small pools at a
                    // visible-but-not-instant ignite feel (the W8.4
                    // tuning, re-expressed for sampling).
                    if !block::fire::hash3_for(pos, clock, 0x9000).is_multiple_of(4) {
                        continue;
                    }
                    // Only level-0 lava blocks ignite, per MC
                    // (`fluid_lava_flow_does_not_ignite`).
                    if vx.world.get(pos).0 != block::LAVA_ID {
                        continue;
                    }
                    if vx.liquid_level.get(&pos).copied().unwrap_or(0) != 0 {
                        continue;
                    }
                    block::fire::plan_lava_random_tick(&vx.world, pos, clock, &mut step);
                }
            }
        }
        if step.ignite.is_empty() && step.consume.is_empty() {
            return;
        }
        vx.apply_fire_step(step);
    }
