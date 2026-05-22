//! W47.4 - ambient particle spawning, relocated from main.rs.
//!
//! `tick_ambient_particles` walks every torch / fire / lava source in
//! the loaded chunks, throttles by view-radius + spawn-rate, and pushes
//! flame + smoke + lava-drop particles into the live particle systems.

#[allow(unused_imports)]
use crate::Voxel;
use crate::{
    accumulator_fires, emit_textured_flame, emit_textured_lava, flame_tint_for,
    lava_sources_in_chunk, resource_pack, spawn_flame_particle, spawn_smoke_particle,
    within_radius, LAVA_ID, TORCH_ID,
};

pub(crate) fn tick_ambient_particles(vx: &mut crate::Voxel, dt: f32) {
        /// Max ambient particles spawned per tick across ALL
        /// sources. Bounded so a forest of torches can't bypass
        /// the system's max_particles budget.
        const AMBIENT_PARTICLE_CAP_PER_TICK: usize = 32;
        /// Maximum visibility range for ambient sources. Beyond
        /// this the source is skipped entirely so off-screen
        /// torches don't burn cycles.
        const VISIBLE_RADIUS: f32 = 32.0;
        /// Target rate per source: 5 Hz = 0.2 s/particle. Matches
        /// MC's flame emission cadence.
        const SOURCE_RATE_HZ: f32 = 5.0;

        // Coarse early-out: nothing to spawn when the system is
        // already at cap.
        if vx.particles.live_count() >= 200 {
            return;
        }
        // Per-source per-frame accumulator. Simpler than per-cell
        // state - we just multiply by dt + the source count.
        let target_per_source = (dt * SOURCE_RATE_HZ).clamp(0.0, 1.0);
        let mut spawned = 0usize;
        let player = [
            vx.player_pos.x,
            vx.player_pos.y,
            vx.player_pos.z,
        ];
        // Cheap xorshift seeded by frame counter + source pos.
        let mut frame_seed: u32 = (vx.net_tick as u32) ^ 0xDEADBEEF;
        // W18.3: grab a borrow of the particle atlas once so the per-
        // source loops below can resolve sprite UVs without repeated
        // Option-deref. `particle_atlas` is `None` for the placeholder
        // pack; the textured emitter helpers fall back to a white quad.
        let pa = vx.resource_pack.particle_atlas.as_ref();

        // Walk torches first (most numerous), then fires, then
        // lava sources. Each contributes one flame + an optional
        // smoke wisp.
        let torches: Vec<[i32; 3]> = vx.torch_positions.iter().copied().collect();
        for pos in &torches {
            if spawned >= AMBIENT_PARTICLE_CAP_PER_TICK {
                break;
            }
            if !within_radius(player, pos, VISIBLE_RADIUS) {
                continue;
            }
            if !accumulator_fires(&mut frame_seed, target_per_source) {
                continue;
            }
            // W38.2: per-emitter flame tint. Look up the block id via
            // emitter_positions (covers all flame emitters since they all
            // satisfy light_emission_for_id > 0). Falls back to orange.
            let block_id = vx.emitter_positions.get(pos).copied()
                .unwrap_or(TORCH_ID);
            let flame_tint = flame_tint_for(block_id);
            // Legacy CPU particle (tinted cube) for collision/gravity.
            spawn_flame_particle(&mut vx.particles, pos, &mut frame_seed, flame_tint);
            // W18.3: also push a textured billboard for the GPU pass.
            emit_textured_flame(
                &mut vx.textured_particles,
                pa,
                pos,
                &mut frame_seed,
                flame_tint,
                vx.net_tick as u32,
            );
            // W36.3: torch smoke - uses the legacy CPU particle which
            // has per-particle velocity so it visibly rises upward.
            // Spawn at ~1/4 rate (one smoke wisp per four flame bursts).
            if accumulator_fires(&mut frame_seed, 0.25) {
                let mc_tint = resource_pack::particles::smoke_tint_at(
                    &vx.resource_pack,
                    0.0,
                );
                spawn_smoke_particle(
                    &mut vx.particles,
                    pos,
                    &mut frame_seed,
                    mc_tint,
                );
            }
            spawned += 1;
        }
        let fires: Vec<[i32; 3]> = vx.fires.keys().copied().collect();
        for pos in &fires {
            if spawned >= AMBIENT_PARTICLE_CAP_PER_TICK {
                break;
            }
            if !within_radius(player, pos, VISIBLE_RADIUS) {
                continue;
            }
            if !accumulator_fires(&mut frame_seed, target_per_source * 1.5) {
                continue;
            }
            spawn_flame_particle(&mut vx.particles, pos, &mut frame_seed, [1.0, 0.55, 0.20]);
            emit_textured_flame(
                &mut vx.textured_particles,
                pa,
                pos,
                &mut frame_seed,
                [1.0, 0.55, 0.20],
                vx.net_tick as u32,
            );
            // Fire also coughs a smoke wisp at ~1/3 the flame rate.
            if accumulator_fires(&mut frame_seed, 0.33) {
                // W9.y.5: sample the MC smoke_0..7 sprite set
                // for the spawn-time tint so smoke wisps match
                // the MC pack's grey palette. Falls through to
                // the helper's hardcoded swatch when no MC pack
                // is loaded.
                let mc_tint = resource_pack::particles::smoke_tint_at(
                    &vx.resource_pack,
                    0.0,
                );
                spawn_smoke_particle(
                    &mut vx.particles,
                    pos,
                    &mut frame_seed,
                    mc_tint,
                );
                // W36.3: textured smoke disabled - the static billboard
                // had no velocity so it appeared to sink. Legacy CPU
                // smoke (above) rises correctly via velocity field.
            }
            spawned += 1;
        }
        // Lava source cells. Use the liquid level table - source
        // cells are level=0 in lava. This is the same scan the
        // fire-emit tick uses but only at half the cap so torches
        // / fires aren't crowded out.
        for (pos, b) in vx
            .world
            .iter_chunks()
            .flat_map(|(coord, chunk)| {
                lava_sources_in_chunk(*coord, chunk)
            })
        {
            if spawned >= AMBIENT_PARTICLE_CAP_PER_TICK {
                break;
            }
            if b != LAVA_ID {
                continue;
            }
            if !within_radius(player, &pos, VISIBLE_RADIUS) {
                continue;
            }
            // Lava emits less frequently (matches MC's "bubble"
            // particle cadence).
            if !accumulator_fires(&mut frame_seed, target_per_source * 0.5) {
                continue;
            }
            spawn_flame_particle(&mut vx.particles, &pos, &mut frame_seed, [1.0, 0.45, 0.15]);
            // W18.3: lava drip uses the dedicated lava sprite.
            emit_textured_lava(
                &mut vx.textured_particles,
                pa,
                &pos,
                &mut frame_seed,
            );
            spawned += 1;
        }
    }
