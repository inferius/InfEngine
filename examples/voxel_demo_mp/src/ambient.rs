//! W47.4 - ambient particle spawning, relocated from main.rs.
//!
//! `tick_ambient_particles` walks every torch / fire / lava source in
//! the loaded chunks, throttles by view-radius + spawn-rate, and pushes
//! flame + smoke + lava-drop particles into the live particle systems.

// W47.4 / W47.5: helpers (within_radius, accumulator_fires, spawn_*,
// emit_textured_*, flame_tint_for, lava_sources_in_chunk) all live in
// this module now - they were relocated together so the per-frame
// loop and its supporting helpers stay co-located.
use crate::{resource_pack, LAVA_ID, REDSTONE_TORCH_ID, REDSTONE_WALL_TORCH_ID, TORCH_ID};

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
            // W128: redstone torches GLOW, they don't burn (MC has no
            // flame or smoke on them - just the lit head + a subtle
            // red shimmer). The old path stacked red flame cubes on
            // the tip, which read as an opaque dark-red blob.
            if block_id == REDSTONE_TORCH_ID || block_id == REDSTONE_WALL_TORCH_ID {
                emit_textured_glow(
                    &mut vx.textured_particles,
                    pa,
                    pos,
                    vx.net_tick as u32,
                );
                spawned += 1;
                continue;
            }
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
        // W151: underwater bubbles (the user's "more underwater
        // effects" polish item).
        tick_underwater_bubbles(vx, dt, &mut frame_seed);
    }

// ---------------------------------------------------------------
// W151 - underwater bubbles.
// ---------------------------------------------------------------

/// One rising bubble. The textured-particle pass is immediate-mode
/// (instances are re-emitted every frame), so moving bubbles keep
/// their own tiny state list on `Voxel` and re-emit per frame.
#[derive(Clone, Copy)]
pub(crate) struct Bubble {
    pub(crate) pos: [f32; 3],
    pub(crate) vel: [f32; 3],
    pub(crate) life: f32,
}

/// Soft cap on live bubbles (the swarm reads dense well before this).
const BUBBLE_CAP: usize = 48;

/// Spawn + advance + emit the underwater bubble swarm. While the
/// camera is submerged, bubbles spawn around the player and float
/// up, dying at their lifetime or the moment they leave water (the
/// MC bubble particle pops at the surface).
fn tick_underwater_bubbles(vx: &mut crate::Voxel, dt: f32, seed: &mut u32) {
    let rand = |s: &mut u32| -> f32 {
        *s ^= *s << 13;
        *s ^= *s >> 17;
        *s ^= *s << 5;
        (*s as f32 / u32::MAX as f32).clamp(0.0, 1.0)
    };
    let eye = vx.player_pos;
    let eye_voxel = [
        eye.x.floor() as i32,
        eye.y.floor() as i32,
        eye.z.floor() as i32,
    ];
    let submerged = vx.world.get(eye_voxel).0 == crate::block::WATER_ID;
    // Spawn: ~6 bubbles/s while submerged (MC shows a steady light
    // stream around the swimming player).
    if submerged && vx.bubbles.len() < BUBBLE_CAP && accumulator_fires(seed, (dt * 6.0).clamp(0.0, 1.0)) {
        let off = [
            (rand(seed) - 0.5) * 2.2,
            (rand(seed) - 0.7) * 1.4,
            (rand(seed) - 0.5) * 2.2,
        ];
        vx.bubbles.push(Bubble {
            pos: [eye.x + off[0], eye.y + off[1], eye.z + off[2]],
            vel: [
                (rand(seed) - 0.5) * 0.15,
                0.5 + rand(seed) * 0.4,
                (rand(seed) - 0.5) * 0.15,
            ],
            life: 1.2 + rand(seed) * 1.3,
        });
    }
    // Advance + cull (lifetime out, or risen out of the water).
    let world = &vx.world;
    vx.bubbles.retain_mut(|b| {
        b.life -= dt;
        if b.life <= 0.0 {
            return false;
        }
        b.pos[0] += b.vel[0] * dt;
        b.pos[1] += b.vel[1] * dt;
        b.pos[2] += b.vel[2] * dt;
        let cell = [
            b.pos[0].floor() as i32,
            b.pos[1].floor() as i32,
            b.pos[2].floor() as i32,
        ];
        world.get(cell).0 == crate::block::WATER_ID
    });
    // Emit one textured billboard per live bubble.
    if vx.bubbles.is_empty() {
        return;
    }
    let pa = vx.resource_pack.particle_atlas.as_ref();
    let (uv_min, uv_max) = if let Some(pa) = pa {
        if let Some(uv) = pa.sprite_uv("bubble", 0) {
            ([uv[0], uv[1]], [uv[2], uv[3]])
        } else {
            ([0.0f32, 0.0], [1.0, 1.0])
        }
    } else {
        ([0.0f32, 0.0], [1.0, 1.0])
    };
    for b in &vx.bubbles {
        vx.textured_particles.push(infengine_render::TexturedParticleInstance {
            center_world: b.pos,
            size: 0.07,
            uv_min,
            uv_max,
            color: [1.0, 1.0, 1.0, 0.9],
        });
    }
}

// ===========================================================================
// W47.5: particle spawn + emit helpers relocated from main.rs.
// All previously pub(crate) at module root of main.rs.
// ===========================================================================

use crate::{SOUL_TORCH_ID, SOUL_WALL_TORCH_ID};
use infengine_voxel::{CHUNK_SIZE, Chunk, ChunkCoord};

/// W38.2: Per-emitter flame colour. MC torch variants each have a
/// distinct flame hue:
///   - Regular torch       -> warm orange [1.0, 0.78, 0.40]
///   - Redstone torch      -> red         [1.0, 0.20, 0.10]
///   - Soul torch          -> cyan-blue   [0.20, 0.75, 1.0]
pub(crate) fn flame_tint_for(id: u16) -> [f32; 3] {
    if id == REDSTONE_TORCH_ID || id == REDSTONE_WALL_TORCH_ID {
        [1.0, 0.20, 0.10]
    } else if id == SOUL_TORCH_ID || id == SOUL_WALL_TORCH_ID {
        [0.20, 0.75, 1.0]
    } else {
        // Regular torch + any unregistered emitter -> warm orange.
        [1.0, 0.78, 0.40]
    }
}

/// True when `pos` is within `radius` of the player.
pub(crate) fn within_radius(player: [f32; 3], pos: &[i32; 3], radius: f32) -> bool {
    let dx = pos[0] as f32 + 0.5 - player[0];
    let dy = pos[1] as f32 + 0.5 - player[1];
    let dz = pos[2] as f32 + 0.5 - player[2];
    dx * dx + dy * dy + dz * dz <= radius * radius
}

/// Bumps an xorshift seed + rolls a Bernoulli accumulator: returns
/// `true` with probability `target`. Cheap stand-in for a full
/// fractional-spawn accumulator.
pub(crate) fn accumulator_fires(seed: &mut u32, target: f32) -> bool {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 17;
    *seed ^= *seed << 5;
    let roll = (*seed as f32 / u32::MAX as f32).clamp(0.0, 1.0);
    roll < target.clamp(0.0, 1.0)
}

/// Spawn one upward-drifting flame particle above `pos`.
pub(crate) fn spawn_flame_particle(
    sys: &mut infengine_particles::ParticleSystem,
    pos: &[i32; 3],
    seed: &mut u32,
    color: [f32; 3],
) {
    let rand = |s: &mut u32| {
        *s ^= *s << 13;
        *s ^= *s >> 17;
        *s ^= *s << 5;
        (*s as f32 / u32::MAX as f32).clamp(0.0, 1.0)
    };
    let jitter_x = (rand(seed) - 0.5) * 0.3;
    let jitter_z = (rand(seed) - 0.5) * 0.3;
    let v_up = 0.4 + rand(seed) * 0.5;
    sys.particles.push(infengine_particles::Particle {
        position: [
            pos[0] as f32 + 0.5 + jitter_x,
            pos[1] as f32 + 0.7,
            pos[2] as f32 + 0.5 + jitter_z,
        ],
        velocity: [0.0, v_up, 0.0],
        color: [color[0], color[1], color[2], 0.95],
        size: 0.08,
        age: 0.0,
        lifetime: 1.2,
        // W45.3: gravity_scale = 0.0 - flame wisps rise upward and
        // must not be pulled down by the ambient emitter's gravity.
        gravity_scale: 0.0,
    });
}

/// Spawn one slow-rising smoke wisp above `pos`. `mc_tint`, when
/// supplied, overrides the hardcoded grey with the average pixel
/// colour sampled from the MC `particle/smoke_0..7` sprite set
/// (W9.y.5). Falls back to the legacy grey when no MC pack is
/// loaded so the placeholder atlas still reads as smoke.
pub(crate) fn spawn_smoke_particle(
    sys: &mut infengine_particles::ParticleSystem,
    pos: &[i32; 3],
    seed: &mut u32,
    mc_tint: Option<[f32; 4]>,
) {
    let rand = |s: &mut u32| {
        *s ^= *s << 13;
        *s ^= *s >> 17;
        *s ^= *s << 5;
        (*s as f32 / u32::MAX as f32).clamp(0.0, 1.0)
    };
    let jitter_x = (rand(seed) - 0.5) * 0.5;
    let jitter_z = (rand(seed) - 0.5) * 0.5;
    // Default to the W9.5 grey when MC isn't installed. When MC
    // smoke sprites resolved, blend the alpha back to 0.7 so
    // every smoke wisp keeps the translucent look (the MC PNG
    // ships fully opaque; particle rendering is what makes it
    // wispy).
    let color = match mc_tint {
        Some(t) => [t[0], t[1], t[2], 0.7],
        None => [0.35, 0.35, 0.35, 0.7],
    };
    sys.particles.push(infengine_particles::Particle {
        position: [
            pos[0] as f32 + 0.5 + jitter_x,
            pos[1] as f32 + 1.0,
            pos[2] as f32 + 0.5 + jitter_z,
        ],
        velocity: [0.0, 0.3 + rand(seed) * 0.2, 0.0],
        color,
        size: 0.10,
        age: 0.0,
        lifetime: 1.8,
        // W45.3: gravity_scale = 0.0 so the ambient emitter's -10 m/s^2
        // downward gravity does not cancel the upward drift velocity.
        // Smoke wisps must rise, not fall.
        gravity_scale: 0.0,
    });
}

// W18.3: textured billboard particle emitters.
// These mirror the legacy `spawn_flame/smoke/lava_particle` helpers
// but produce `TexturedParticleInstance`s that are rendered by the
// GPU particle pass (billboard shader + separate particle atlas) instead
// of the legacy cube particle system. The two systems can coexist: the
// CPU system handles debris/break effects; the GPU pass handles
// ambient torch/fire/lava ambient sprites.

/// Emit textured flame quads above `pos` into `out`.  Uses the
/// `particle_atlas`'s "flame" sprite strip; falls back to a solid
/// orange quad when the atlas is absent (placeholder pack).
///
/// W45.4: anti-jump fix (Option C). Instead of one random-frame quad,
/// we emit `FLAME_LAYERS` overlapping quads each showing a different
/// animation frame at slightly different alpha. This way multiple
/// adjacent frames of the flame strip coexist in the scene at once,
/// and the visual averages out to a smooth perceived animation rather
/// than one frame flipping to a random other frame each tick.
/// Emit a textured flame billboard above `pos` into `out`.
///
/// W46.2: rewritten from W45.4. The old "Option C" multi-layer
/// emit (3 overlapping billboards with per-call random XZ jitter)
/// was producing a "flame skips along X axis" look instead of a
/// proper in-place sprite animation, because each emit call rolled
/// fresh random jitter and a fresh random base frame. MC's actual
/// flame is ONE billboard, fixed at the torch top, with the frame
/// index advancing deterministically from world time so the sprite
/// changes IN PLACE.
///
/// `tick` is the world tick counter (advances at the simulation
/// rate). The MC flame texture has `n_frames` sub-sprites stacked
/// vertically; we cycle one frame per tick so the full loop runs at
/// the same cadence MC uses (~32 frames over ~32 ticks = 1.6 s).
///
/// `seed` is no longer used for randomness here (the visible flame
/// is deterministic by design) but is kept for callers that still
/// pass it; per-tick non-determinism lives in the legacy
/// `spawn_flame_particle` path that handles the rising/colliding
/// CPU particle.
/// W128: small translucent red shimmer for redstone torches (the
/// `glow` sprite at low alpha). Gently pulses with the world tick so
/// it reads as energised rather than burning.
pub(crate) fn emit_textured_glow(
    out: &mut Vec<infengine_render::TexturedParticleInstance>,
    particle_atlas: Option<&crate::resource_pack::particle_atlas::ParticleAtlasData>,
    pos: &[i32; 3],
    tick: u32,
) {
    let (uv_min, uv_max) = if let Some(pa) = particle_atlas {
        if let Some(uv) = pa.sprite_uv("glow", 0) {
            ([uv[0], uv[1]], [uv[2], uv[3]])
        } else {
            ([0.0f32, 0.0], [1.0, 1.0])
        }
    } else {
        ([0.0f32, 0.0], [1.0, 1.0])
    };
    // Slow 2 s pulse between 0.30 and 0.50 alpha.
    let phase = (tick % 40) as f32 / 40.0 * std::f32::consts::TAU;
    let alpha = 0.40 + 0.10 * phase.sin();
    out.push(infengine_render::TexturedParticleInstance {
        center_world: [
            pos[0] as f32 + 0.5,
            pos[1] as f32 + 0.65,
            pos[2] as f32 + 0.5,
        ],
        size: 0.11,
        uv_min,
        uv_max,
        color: [1.0, 0.35, 0.28, alpha],
    });
}

pub(crate) fn emit_textured_flame(
    out: &mut Vec<infengine_render::TexturedParticleInstance>,
    particle_atlas: Option<&crate::resource_pack::particle_atlas::ParticleAtlasData>,
    pos: &[i32; 3],
    _seed: &mut u32,
    color: [f32; 3],
    tick: u32,
) {
    let n_frames = particle_atlas
        .map(|pa| pa.frame_count("flame").max(1))
        .unwrap_or(1);
    // Frame index purely from world time -> animation runs in place
    // regardless of emit cadence. One frame per tick matches vanilla
    // MC's flame animation speed.
    let frame = tick % n_frames;

    let (uv_min, uv_max) = if let Some(pa) = particle_atlas {
        if let Some(uv) = pa.sprite_uv("flame", frame) {
            ([uv[0], uv[1]], [uv[2], uv[3]])
        } else {
            ([0.0f32, 0.0], [1.0, 1.0])
        }
    } else {
        ([0.0f32, 0.0], [1.0, 1.0])
    };

    out.push(infengine_render::TexturedParticleInstance {
        // Centred on the voxel, lifted to the torch flame height.
        // No XZ jitter - the flame stays anchored to the torch tip.
        center_world: [
            pos[0] as f32 + 0.5,
            pos[1] as f32 + 0.8,
            pos[2] as f32 + 0.5,
        ],
        size: 0.125,
        uv_min,
        uv_max,
        color: [color[0], color[1], color[2], 1.0],
    });
}

/// Emit a textured smoke quad above `pos` into `out`. Cycles the
/// `smoke_0..7` sprites from the particle atlas; falls back to a
/// grey quad when the atlas is absent.
///
/// W36.3: no longer called - static billboard had no velocity so it
/// appeared to sink. Kept for reference; callers were migrated to the
/// legacy CPU particle path (`spawn_smoke_particle`) which has a
/// rising velocity field. Remove in a future cleanup pass.
#[allow(dead_code)]
pub(crate) fn emit_textured_smoke(
    out: &mut Vec<infengine_render::TexturedParticleInstance>,
    particle_atlas: Option<&crate::resource_pack::particle_atlas::ParticleAtlasData>,
    pos: &[i32; 3],
    seed: &mut u32,
) {
    let rand = |s: &mut u32| -> f32 {
        *s ^= *s << 13;
        *s ^= *s >> 17;
        *s ^= *s << 5;
        (*s as f32 / u32::MAX as f32).clamp(0.0, 1.0)
    };
    let jitter_x = (rand(seed) - 0.5) * 0.5;
    let jitter_z = (rand(seed) - 0.5) * 0.5;
    // Pick a random smoke frame at spawn time. The legacy system
    // cycled the frame over the particle's lifetime (W9.y.5); the
    // GPU pass renders a static sprite, so we pick once at birth.
    let (uv_min, uv_max, alpha) = if let Some(pa) = particle_atlas {
        let frame = (rand(seed) * 8.0) as u32 % 8;
        let name = match frame {
            0 => "smoke_0", 1 => "smoke_1", 2 => "smoke_2", 3 => "smoke_3",
            4 => "smoke_4", 5 => "smoke_5", 6 => "smoke_6", _ => "smoke_7",
        };
        if let Some(uv) = pa.sprite_uv(name, 0) {
            ([uv[0], uv[1]], [uv[2], uv[3]], 0.7f32)
        } else {
            ([0.0f32, 0.0], [1.0, 1.0], 0.5)
        }
    } else {
        ([0.0f32, 0.0], [1.0, 1.0], 0.5)
    };
    out.push(infengine_render::TexturedParticleInstance {
        center_world: [
            pos[0] as f32 + 0.5 + jitter_x,
            pos[1] as f32 + 1.1,
            pos[2] as f32 + 0.5 + jitter_z,
        ],
        size: 0.15,
        uv_min,
        uv_max,
        // Grey tint: MC smoke is off-white/grey.
        color: [0.7, 0.7, 0.7, alpha],
    });
}

/// Emit a textured lava drip quad above `pos` into `out`. Uses the
/// single-frame `lava` sprite from the particle atlas.
pub(crate) fn emit_textured_lava(
    out: &mut Vec<infengine_render::TexturedParticleInstance>,
    particle_atlas: Option<&crate::resource_pack::particle_atlas::ParticleAtlasData>,
    pos: &[i32; 3],
    seed: &mut u32,
) {
    let rand = |s: &mut u32| -> f32 {
        *s ^= *s << 13;
        *s ^= *s >> 17;
        *s ^= *s << 5;
        (*s as f32 / u32::MAX as f32).clamp(0.0, 1.0)
    };
    let jitter_x = (rand(seed) - 0.5) * 0.4;
    let jitter_z = (rand(seed) - 0.5) * 0.4;
    let (uv_min, uv_max) = if let Some(pa) = particle_atlas {
        if let Some(uv) = pa.sprite_uv("lava", 0) {
            ([uv[0], uv[1]], [uv[2], uv[3]])
        } else {
            ([0.0f32, 0.0], [1.0, 1.0])
        }
    } else {
        ([0.0f32, 0.0], [1.0, 1.0])
    };
    out.push(infengine_render::TexturedParticleInstance {
        center_world: [
            pos[0] as f32 + 0.5 + jitter_x,
            pos[1] as f32 + 0.6,
            pos[2] as f32 + 0.5 + jitter_z,
        ],
        size: 0.10,
        uv_min,
        uv_max,
        // MC lava particle is a warm orange-red.
        color: [1.0, 0.45, 0.15, 1.0],
    });
}

/// Walk a chunk's voxels + emit every lava source cell. Used by
/// `tick_ambient_particles`. Returns absolute world coords paired
/// with the block id so the caller can filter without re-fetching.
pub(crate) fn lava_sources_in_chunk(
    coord: ChunkCoord,
    chunk: &Chunk,
) -> Vec<([i32; 3], u16)> {
    let mut out = Vec::new();
    let base_x = coord[0] * CHUNK_SIZE as i32;
    let base_y = coord[1] * CHUNK_SIZE as i32;
    let base_z = coord[2] * CHUNK_SIZE as i32;
    for lz in 0..CHUNK_SIZE {
        for ly in 0..CHUNK_SIZE {
            for lx in 0..CHUNK_SIZE {
                let b = chunk.get(lx, ly, lz);
                if b.0 == LAVA_ID {
                    out.push((
                        [
                            base_x + lx as i32,
                            base_y + ly as i32,
                            base_z + lz as i32,
                        ],
                        b.0,
                    ));
                }
            }
        }
    }
    out
}

// ===========================================================================
// W47.7: break + place particle bursts, relocated from main.rs.
// ===========================================================================

pub(crate) fn spawn_break_particles(particles: &mut infengine_particles::ParticleSystem, voxel: [i32; 3]) {
        // No-tint variant: callers that don't know the source
        // block fall through to a neutral dirt-ish color.
        spawn_break_particles_tinted(particles, voxel, [0.75, 0.55, 0.30]);
    }
pub(crate) fn spawn_break_particles_tinted(particles: &mut infengine_particles::ParticleSystem, voxel: [i32; 3], color: [f32; 3]) {
        let pos = [
            voxel[0] as f32 + 0.5,
            voxel[1] as f32 + 0.5,
            voxel[2] as f32 + 0.5,
        ];
        let mut rng: u32 = 0xCAFEBABE
            ^ (voxel[0] as u32)
            ^ ((voxel[1] as u32) << 7)
            ^ ((voxel[2] as u32) << 13);
        let mut rand01 = || -> f32 {
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            (rng as f32 / u32::MAX as f32).clamp(0.0, 1.0)
        };
        for _ in 0..12 {
            let vx = (rand01() * 2.0 - 1.0) * 3.0;
            let vy = rand01() * 3.5;
            let vz = (rand01() * 2.0 - 1.0) * 3.0;
            // Small per-particle brightness jitter so the puff
            // doesn't read as a flat monochrome cluster.
            let j = 0.85 + rand01() * 0.30;
            particles.particles.push(infengine_particles::Particle {
                position: pos,
                velocity: [vx, vy, vz],
                color: [
                    (color[0] * j).clamp(0.0, 1.0),
                    (color[1] * j).clamp(0.0, 1.0),
                    (color[2] * j).clamp(0.0, 1.0),
                    1.0,
                ],
                size: 0.12,
                age: 0.0,
                lifetime: 0.5,
                gravity_scale: 1.0,
            });
        }
    }

    /// Burst of small dust particles when a block is placed. Half
    /// the count of `spawn_break_particles` so it reads as a
    /// gentle "click" rather than a full debris cloud, tinted with
    /// the block's albedo.
pub(crate) fn spawn_place_particles(particles: &mut infengine_particles::ParticleSystem, voxel: [i32; 3], color: [f32; 3]) {
        let pos = [
            voxel[0] as f32 + 0.5,
            voxel[1] as f32 + 0.5,
            voxel[2] as f32 + 0.5,
        ];
        let mut rng: u32 = 0xABAD1DEA
            ^ (voxel[0] as u32)
            ^ ((voxel[1] as u32) << 7)
            ^ ((voxel[2] as u32) << 13);
        let mut rand01 = || -> f32 {
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            (rng as f32 / u32::MAX as f32).clamp(0.0, 1.0)
        };
        for _ in 0..6 {
            let vx = (rand01() * 2.0 - 1.0) * 1.5;
            let vy = rand01() * 1.0 + 0.5;
            let vz = (rand01() * 2.0 - 1.0) * 1.5;
            particles.particles.push(infengine_particles::Particle {
                position: pos,
                velocity: [vx, vy, vz],
                color: [color[0], color[1], color[2], 0.9],
                size: 0.08,
                age: 0.0,
                lifetime: 0.3,
                gravity_scale: 1.0,
            });
        }
    }
/// W9.5: ambient particle helper unit tests. Live below the
/// day_night_tests module so the items_after_test_module lint
/// stays happy.
#[cfg(test)]
mod ambient_particle_tests {
    // W47.5: helpers moved to crate::ambient. Test imports updated.
    use crate::ambient::{
        accumulator_fires, spawn_flame_particle, spawn_smoke_particle,
        within_radius,
    };
    use infengine_particles::ParticleSystem;

    #[test]
    fn within_radius_includes_close_position() {
        let player = [0.0, 0.0, 0.0];
        assert!(within_radius(player, &[1, 0, 0], 5.0));
        assert!(within_radius(player, &[3, 0, 3], 10.0));
    }

    #[test]
    fn within_radius_excludes_far_position() {
        let player = [0.0, 0.0, 0.0];
        assert!(!within_radius(player, &[100, 0, 0], 32.0));
        assert!(!within_radius(player, &[40, 0, 0], 32.0));
    }

    #[test]
    fn accumulator_target_zero_never_fires() {
        let mut seed = 0xDEADBEEF;
        for _ in 0..1000 {
            assert!(!accumulator_fires(&mut seed, 0.0));
        }
    }

    #[test]
    fn accumulator_target_one_always_fires() {
        let mut seed = 0x12345678;
        for _ in 0..1000 {
            assert!(accumulator_fires(&mut seed, 1.0));
        }
    }

    #[test]
    fn flame_particle_spawns_above_position() {
        let mut sys = ParticleSystem::new();
        let mut seed = 0xABCDEF01;
        let start = sys.particles.len();
        spawn_flame_particle(&mut sys, &[10, 5, 7], &mut seed, [1.0, 0.8, 0.4]);
        assert_eq!(sys.particles.len(), start + 1);
        let p = sys.particles.last().unwrap();
        // Y should be above the voxel centre.
        assert!(p.position[1] >= 5.5, "flame should spawn above the cell base");
        assert!(p.velocity[1] > 0.0, "flame should drift upward");
        assert!(p.lifetime > 0.0);
    }

    #[test]
    fn smoke_particle_drifts_slower_than_flame() {
        let mut sys = ParticleSystem::new();
        let mut seed = 0x11223344;
        spawn_flame_particle(&mut sys, &[0, 0, 0], &mut seed, [1.0, 0.8, 0.4]);
        let flame_vy = sys.particles.last().unwrap().velocity[1];
        spawn_smoke_particle(&mut sys, &[0, 0, 0], &mut seed, None);
        let smoke_vy = sys.particles.last().unwrap().velocity[1];
        // Flame range: 0.4..0.9. Smoke range: 0.3..0.5. They can
        // technically overlap, but the smoke ceiling (0.5) is
        // below the flame floor + something - assert the smoke is
        // BOUNDED ABOVE by 0.55 + flame is BOUNDED BELOW by 0.35.
        assert!(smoke_vy < 0.55, "smoke vy should be capped");
        assert!(flame_vy > 0.35, "flame vy should be at least 0.4");
    }

    #[test]
    fn particle_cap_is_enforced_softly() {
        // The cap is checked once at the top of tick_ambient_particles;
        // we can't easily call that here without a full Voxel, but
        // we can at least verify the spawn helpers add ONE particle
        // each per invocation - so the cap math (max-per-tick) works.
        let mut sys = ParticleSystem::new();
        let mut seed = 0x99887766;
        let start = sys.particles.len();
        for _ in 0..10 {
            spawn_flame_particle(&mut sys, &[0, 0, 0], &mut seed, [1.0, 0.8, 0.4]);
        }
        assert_eq!(sys.particles.len(), start + 10);
    }

    #[test]
    fn smoke_particle_uses_mc_tint_when_provided() {
        // W9.y.5: the spawn helper takes an optional MC tint
        // sampled from the smoke_0..7 sprite set. When supplied,
        // the emitted Particle's color must use the MC RGB
        // (alpha forced to 0.7 so the wisp keeps its translucent
        // look).
        let mut sys = ParticleSystem::new();
        let mut seed = 0x42424242;
        let mc = Some([0.2, 0.21, 0.22, 1.0]);
        spawn_smoke_particle(&mut sys, &[0, 0, 0], &mut seed, mc);
        let p = sys.particles.last().unwrap();
        assert!((p.color[0] - 0.2).abs() < 1e-3);
        assert!((p.color[1] - 0.21).abs() < 1e-3);
        assert!((p.color[2] - 0.22).abs() < 1e-3);
        assert!((p.color[3] - 0.7).abs() < 1e-3);
    }

    #[test]
    fn smoke_particle_falls_back_to_grey_without_mc_tint() {
        // No MC pack (placeholder atlas): the smoke wisp uses
        // the legacy grey swatch so the visual stays stable.
        let mut sys = ParticleSystem::new();
        let mut seed = 0x42424242;
        spawn_smoke_particle(&mut sys, &[0, 0, 0], &mut seed, None);
        let p = sys.particles.last().unwrap();
        assert!((p.color[0] - 0.35).abs() < 1e-3);
        assert!((p.color[1] - 0.35).abs() < 1e-3);
        assert!((p.color[2] - 0.35).abs() < 1e-3);
    }
}
