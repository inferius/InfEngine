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
            if vx.world.get(*pos).0 != block::FIRE_ID {
                continue;
            }
            vx.world.set(*pos, BlockId::AIR);
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
            vx.world.set(*pos, BlockId::AIR);
            for c in crate::Voxel::affected_chunks(*pos) {
                chunk_set.insert(c);
            }
        }
        // Age bumps - update fires map only; world stays FIRE_ID.
        for (pos, age) in &step.age_bump {
            if vx.world.get(*pos).0 != block::FIRE_ID {
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
            if here.0 == block::FIRE_ID {
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
            vx.world.set(*pos, BlockId(block::FIRE_ID));
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
    /// (MC LavaFluid::randomTick, lines 78..=114). Walks every
    /// lava source cell once per net-tick frame; each gates
    /// 1/LAVA_IGNITE_RATE_DENOM and rolls the two-branch ignite
    /// algorithm from `block::fire::plan_lava_random_tick`. The
    /// produced `ignite` entries route through `apply_fire_step`
    /// so the broadcast + chunk-rebuild plumbing is shared with
    /// the regular fire spread path.
pub(crate) fn tick_lava_fire_emit(vx: &mut crate::Voxel) {
        if matches!(vx.net, NetRole::Client { .. }) {
            return;
        }
        // W198.10: gate on the lava subset, not all liquids. Standing in an
        // ocean world `liquid_positions` is 300k+ cells; lava is a handful.
        if vx.lava_columns.is_empty() {
            return;
        }
        let clock = vx.fire_clock;
        // Snapshot source cells - only level-0 lava blocks count
        // per MC (`fluid_lava_flow_does_not_ignite` test).
        let mut sources: Vec<[i32; 3]> = Vec::new();
        // W233: MC's lava ignition is randomTick behaviour, i.e.
        // ticking chunks near the player only - same horizontal radius
        // as the random-tick module (columns tick full-height, like MC
        // ticking chunks). W267: the subset is COLUMN-KEYED now - look
        // up only the (2r+1)^2 columns in reach instead of scanning
        // the whole set (the W260 deep lava sea grew it to ~1M cells
        // and the flat scan alone cost 5-13 ms per tick). Entries
        // re-validate against the world, so a momentarily stale cell
        // is skipped.
        let cs = infengine_voxel::CHUNK_SIZE as i32;
        let reach_chunks = crate::world::random_tick::RADIUS_CHUNKS + 1;
        let reach = reach_chunks * cs;
        let px = vx.player_pos.x.floor() as i32;
        let pz = vx.player_pos.z.floor() as i32;
        let (pcx, pcz) = (px.div_euclid(cs), pz.div_euclid(cs));
        for dcx in -reach_chunks..=reach_chunks {
            for dcz in -reach_chunks..=reach_chunks {
                let Some(cells) = vx.lava_columns.get(&[pcx + dcx, pcz + dcz]) else {
                    continue;
                };
                for pos in cells.iter().copied() {
                    if (pos[0] - px).abs() > reach || (pos[2] - pz).abs() > reach {
                        continue;
                    }
                    if vx.world.get(pos).0 != block::LAVA_ID {
                        continue;
                    }
                    if vx.liquid_level.get(&pos).copied().unwrap_or(0) != 0 {
                        continue;
                    }
                    sources.push(pos);
                }
            }
        }
        if sources.is_empty() {
            return;
        }
        let mut step = block::fire::FireStep::default();
        for pos in sources {
            let h_gate = block::fire::hash3_for(pos, clock, 0x9000);
            if !h_gate.is_multiple_of(block::fire::LAVA_IGNITE_RATE_DENOM) {
                continue;
            }
            block::fire::plan_lava_random_tick(&vx.world, pos, clock, &mut step);
        }
        if step.ignite.is_empty() && step.consume.is_empty() {
            return;
        }
        vx.apply_fire_step(step);
    }
