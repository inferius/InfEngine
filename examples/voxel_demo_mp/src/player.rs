//! Player physics helpers + damage application.
//!
//! Owns the small, well-isolated player utilities: AABB
//! construction, collision-test helpers against the voxel
//! world + the snow layer map, the "what liquid is my body /
//! eye in" probes, and the death + respawn path that fires
//! when HP crosses zero.
//!
//! The wider movement loop (gravity, jump, walk, sprint, head
//! bob, sneak clamp, swim float) stays inline in
//! `Voxel::update_play` - it interleaves heavily with input
//! capture, network broadcasts, and the mining/attack
//! arbitration, so pulling just the kinematic body of it would
//! either drag half of `update_play` along or fragment the
//! per-tick state into a maze of out-params.
//!
//! Constants the player kinematics tune (`PLAYER_HEIGHT`,
//! `PLAYER_RADIUS`, `PLAYER_SPEED`, `JUMP_VELOCITY`, `GRAVITY`,
//! `COLLISION_EPSILON`) live here too and are re-exported by
//! main.rs so existing call sites don't have to change. The
//! cross-cutting `PLAYER_MAX_HEALTH` stays in main.rs - it's
//! referenced from chat, mob, drops, UI etc and isn't
//! player-physics-only.

use std::collections::HashMap;

use glam::Vec3;
use infengine_voxel::{BlockId, World};

use crate::block::{block_is_aabb_passable, block_is_swim_medium};
use crate::liquid::{Liquid, liquid_for};
use crate::net::NetRole;
use crate::{DamageKind, HOTBAR_SLOTS, PLAYER_MAX_HEALTH, Voxel};

/// Horizontal player walk + sprint base speed (m/s). The
/// sprint multiplier and food/sneak modifiers scale this.
pub(crate) const PLAYER_SPEED: f32 = 6.0;
/// Initial upward velocity on jump. Peak height =
/// `v^2 / (2 * |g|)`, so 8.0 ^ 2 / 36 ~= 1.78 voxels - enough
/// to clear a one-block step with margin, similar to
/// Minecraft's 1.25-block jump.
pub(crate) const JUMP_VELOCITY: f32 = 8.0;
/// Downward gravity in m/s^2. Tuned in tandem with
/// `JUMP_VELOCITY` so the jump arc lands around 0.6 s.
pub(crate) const GRAVITY: f32 = -18.0;
/// Player body height (eye -> feet) in metres.
pub(crate) const PLAYER_HEIGHT: f32 = 1.7;
/// Half-width of the player AABB on the X/Z axes.
pub(crate) const PLAYER_RADIUS: f32 = 0.3;
/// Tiny inset applied when sampling AABB faces against voxel
/// boundaries. Stops a player flush against a wall from
/// reporting a false collision with the empty cell next door.
pub(crate) const COLLISION_EPSILON: f32 = 1e-4;

/// Player AABB at `pos`, where `pos.y` is the eye (head)
/// height.
pub(crate) fn body_aabb(pos: Vec3) -> (Vec3, Vec3) {
    (
        Vec3::new(
            pos.x - PLAYER_RADIUS,
            pos.y - PLAYER_HEIGHT,
            pos.z - PLAYER_RADIUS,
        ),
        Vec3::new(pos.x + PLAYER_RADIUS, pos.y, pos.z + PLAYER_RADIUS),
    )
}

/// True for blocks the player walks through (collision-wise).
/// Delegates to the generic `F_AABB_PASS` flag so decorations
/// (torch, flowers) and liquids (water) are handled by their
/// `BlockType` row rather than a hard-coded id list. Snow goes
/// through `voxel_is_solid` because its passable / solid state
/// depends on the runtime layer count.
pub(crate) fn is_passable(b: BlockId) -> bool {
    block_is_aabb_passable(b)
}

/// Combined solid lookup: world solid AND not in the passable
/// decoration set AND, when the block is snow, has fewer than
/// 8 stacked layers (so a fully-buried voxel becomes a solid
/// block the player can stand on).
pub(crate) fn voxel_is_solid(
    world: &World,
    snow: &HashMap<[i32; 3], u8>,
    v: [i32; 3],
) -> bool {
    let b = world.get(v);
    if b.is_air() {
        return false;
    }
    if is_passable(b) {
        return false;
    }
    if b.0 == 15 {
        let layers = snow.get(&v).copied().unwrap_or(8);
        return layers >= 8;
    }
    true
}

/// Liquid the player's eye (top of the body AABB) sits in, if
/// any. Used by the screen-tint overlay so the wash only shows
/// when the head is actually under the surface - wading
/// ankle-deep doesn't trigger it. Eye position approximated by
/// `pos.y` (the camera height in the existing rig), pulled
/// inward a few centimetres so a perfectly flush waterline
/// rounds down to "out".
pub(crate) fn submerged_liquid_eye(world: &World, pos: Vec3) -> Option<&'static Liquid> {
    let eye_y = (pos.y - 0.1).floor() as i32;
    let eye_x = pos.x.floor() as i32;
    let eye_z = pos.z.floor() as i32;
    let b = world.get([eye_x, eye_y, eye_z]);
    if !block_is_swim_medium(b) {
        return None;
    }
    liquid_for(b)
}

/// Find the liquid the player's AABB overlaps, if any. Returns
/// the `Liquid` reference so the movement loop can read drag /
/// buoyancy / damage values per fluid kind without a separate
/// lookup. Caller picks whichever liquid the body first hits;
/// mixing two liquids in one body would be unusual and matches
/// MC's "you're in lava, period" semantics.
pub(crate) fn submerged_liquid(world: &World, pos: Vec3) -> Option<&'static Liquid> {
    let (min, max) = body_aabb(pos);
    let x0 = min.x.floor() as i32;
    let x1 = (max.x - COLLISION_EPSILON).floor() as i32;
    let y0 = min.y.floor() as i32;
    let y1 = (max.y - COLLISION_EPSILON).floor() as i32;
    let z0 = min.z.floor() as i32;
    let z1 = (max.z - COLLISION_EPSILON).floor() as i32;
    for y in y0..=y1 {
        for z in z0..=z1 {
            for x in x0..=x1 {
                let b = world.get([x, y, z]);
                if !block_is_swim_medium(b) {
                    continue;
                }
                if let Some(l) = liquid_for(b) {
                    return Some(l);
                }
            }
        }
    }
    None
}

/// Returns true if `pos`'s AABB overlaps any solid voxel.
pub(crate) fn aabb_intersects_solid(
    world: &World,
    snow: &HashMap<[i32; 3], u8>,
    pos: Vec3,
) -> bool {
    let (min, max) = body_aabb(pos);
    let x0 = min.x.floor() as i32;
    let x1 = (max.x - COLLISION_EPSILON).floor() as i32;
    let y0 = min.y.floor() as i32;
    let y1 = (max.y - COLLISION_EPSILON).floor() as i32;
    let z0 = min.z.floor() as i32;
    let z1 = (max.z - COLLISION_EPSILON).floor() as i32;
    for y in y0..=y1 {
        for z in z0..=z1 {
            for x in x0..=x1 {
                if voxel_is_solid(world, snow, [x, y, z]) {
                    return true;
                }
            }
        }
    }
    false
}

/// Apply `amount` HP of damage to the local player. Reaches
/// zero = death = respawn at safe spawn + storage-row wipe +
/// inventory snapshot fired through the `vx.fellout` channel
/// so the server's authoritative copy stays in sync.
pub(crate) fn apply_damage(voxel: &mut Voxel, amount: f32) {
    apply_damage_kind(voxel, amount, DamageKind::Generic);
}

/// Variant that tags the source of the damage so the SFX
/// layer can pick a fitting synth (gurgle for drown, sizzle
/// for lava, thud for everything else). Sound fires only on
/// integer health crossings so a fractional per-frame drain
/// doesn't spam the player's ears at 60 Hz.
pub(crate) fn apply_damage_kind(voxel: &mut Voxel, amount: f32, kind: DamageKind) {
    if amount <= 0.0 {
        return;
    }
    // Godmode swallows damage entirely. Toggled via /godmode.
    if voxel.god_mode {
        return;
    }
    // W10.4: Creative `invulnerable` swallows every damage source
    // EXCEPT `DamageKind::Void` (out-of-world). Mirrors MC's
    // `Player::isInvulnerableTo` + `DamageSource::bypassInvul`.
    // Void damage stays lethal so /gamemode creative isn't a
    // free way to escape the bottom of the world.
    if voxel.abilities.invulnerable && !matches!(kind, DamageKind::Void) {
        return;
    }
    // Spawn invuln window. Decrements in update_play; while
    // positive every damage call is a no-op so a fresh
    // respawn can't be insta-killed by lingering mobs.
    if voxel.invuln_remaining > 0.0 {
        return;
    }
    let prev_int = voxel.player_health.ceil() as i32;
    voxel.player_health = (voxel.player_health - amount).max(0.0);
    let cur_int = voxel.player_health.ceil() as i32;
    voxel.hurt_flash = 0.5;
    // Damage resets the regen grace timer.
    voxel.regen_accum = 0.0;
    // SFX fires only when an integer heart slot tips over,
    // not every fractional tick. Per-kind synth so drown +
    // burn don't share the loud "hurt" thud.
    if cur_int < prev_int {
        let pos = [voxel.player_pos.x, voxel.player_pos.y, voxel.player_pos.z];
        match kind {
            DamageKind::Generic => {
                voxel.sfx_at("hurt", pos, |a, g| {
                    let _ = a.play_thud(140.0, 0.35 * g);
                });
            }
            DamageKind::Drown => {
                // Soft bubble + airy swish for the breath-out
                // moment. Volume capped so the player can
                // hear themselves think.
                voxel.sfx_at("drown", pos, |a, g| {
                    let _ = a.play_click(95.0, 0.10 * g);
                    let _ = a.play_swish(0.08 * g);
                });
            }
            DamageKind::Burn => {
                voxel.sfx_at("burn", pos, |a, g| {
                    let _ = a.play_swish(0.18 * g);
                });
            }
            DamageKind::Void => {
                // Out-of-world damage: low rumble + bell tone so
                // the player hears the floor disappearing.
                voxel.sfx_at("void", pos, |a, g| {
                    let _ = a.play_thud(60.0, 0.4 * g);
                });
            }
        }
    }
    if voxel.player_health <= 0.0 {
        tracing::warn!("player died (HP = 0) - respawning");
        voxel.player_pos = voxel.safe_spawn();
        voxel.player_vel = Vec3::ZERO;
        voxel.player_health = PLAYER_MAX_HEALTH;
        voxel.invuln_remaining = 3.0;
        let mut dropped = 0usize;
        for slot in voxel.inventory[HOTBAR_SLOTS..].iter_mut() {
            if slot.take().is_some() {
                dropped += 1;
            }
        }
        for slot in voxel.craft_grid.iter_mut() {
            if slot.take().is_some() {
                dropped += 1;
            }
        }
        if dropped > 0 {
            tracing::info!(dropped, "death penalty: cleared storage stacks");
        }
        // Local chat toast so the player gets a visible
        // confirmation matching the audible thud. The line
        // stays local; in MP the kill/death announcement is
        // handled separately by the server when it's
        // implemented.
        voxel.chat_log.push((
            std::time::Instant::now(),
            if dropped > 0 {
                format!("You died. Lost {dropped} storage stacks.")
            } else {
                "You died and respawned at spawn.".to_string()
            },
        ));
        // Broadcast a public death notice in MP so peers see
        // the event. Host tags it with `voxel.player_name`;
        // clients send the raw line and the server's chat
        // path wraps it with the sender's identity.
        let public = "* I just died!".to_string();
        let cfg = bincode::config::standard();
        match &mut voxel.net {
            NetRole::Offline => {}
            NetRole::Server { server, .. } => {
                let line = format!("* {} died.", voxel.player_name);
                voxel.chat_log
                    .push((std::time::Instant::now(), line.clone()));
                if let Ok(buf) = bincode::serde::encode_to_vec(&line, cfg) {
                    server.broadcast_bytes("vx.chat", buf);
                }
            }
            NetRole::Client { client, .. } => {
                if let Ok(buf) = bincode::serde::encode_to_vec(&public, cfg) {
                    let _ = client.send_bytes("vx.chat", buf);
                }
                let _ = client.send_bytes("vx.fellout", Vec::new());
            }
        }
    }
}

// ===========================================================================
// W47.2: relocated from main.rs
// ===========================================================================

/// Slab-test ray-AABB intersection. Returns the parametric `t`
/// of the entry point along `dir` (assumed approximately
/// unit-length, but any positive length works) when the ray hits
/// the box; `None` if it misses. Re-used by the mob picker.
pub(crate) fn ray_aabb(origin: [f32; 3], dir: [f32; 3], min: [f32; 3], max: [f32; 3]) -> Option<f32> {
    let mut tmin = 0.0_f32;
    let mut tmax = f32::INFINITY;
    for axis in 0..3 {
        let inv = 1.0 / dir[axis];
        let mut t1 = (min[axis] - origin[axis]) * inv;
        let mut t2 = (max[axis] - origin[axis]) * inv;
        if t1 > t2 {
            std::mem::swap(&mut t1, &mut t2);
        }
        tmin = tmin.max(t1);
        tmax = tmax.min(t2);
        if tmax < tmin {
            return None;
        }
    }
    if tmax < 0.0 {
        None
    } else {
        Some(tmin.max(0.0))
    }
}

pub(crate) fn sweep_aabb(
    world: &World,
    snow: &HashMap<[i32; 3], u8>,
    pos: Vec3,
    delta: Vec3,
) -> (Vec3, bool, bool) {
    let mut new_pos = pos;
    let mut hit_floor = false;
    let mut hit_ceiling = false;

    // X axis.
    let candidate = Vec3::new(new_pos.x + delta.x, new_pos.y, new_pos.z);
    if aabb_intersects_solid(world, snow, candidate) {
        if delta.x > 0.0 {
            let wall = (candidate.x + PLAYER_RADIUS).floor();
            new_pos.x = wall - PLAYER_RADIUS - COLLISION_EPSILON;
        } else if delta.x < 0.0 {
            let wall = (candidate.x - PLAYER_RADIUS).ceil();
            new_pos.x = wall + PLAYER_RADIUS + COLLISION_EPSILON;
        }
    } else {
        new_pos.x = candidate.x;
    }

    // Y axis.
    let candidate = Vec3::new(new_pos.x, new_pos.y + delta.y, new_pos.z);
    if aabb_intersects_solid(world, snow, candidate) {
        if delta.y > 0.0 {
            let wall = candidate.y.floor();
            new_pos.y = wall - COLLISION_EPSILON;
            hit_ceiling = true;
        } else if delta.y < 0.0 {
            let wall = (candidate.y - PLAYER_HEIGHT).ceil();
            new_pos.y = wall + PLAYER_HEIGHT + COLLISION_EPSILON;
            hit_floor = true;
        }
    } else {
        new_pos.y = candidate.y;
    }

    // Z axis.
    let candidate = Vec3::new(new_pos.x, new_pos.y, new_pos.z + delta.z);
    if aabb_intersects_solid(world, snow, candidate) {
        if delta.z > 0.0 {
            let wall = (candidate.z + PLAYER_RADIUS).floor();
            new_pos.z = wall - PLAYER_RADIUS - COLLISION_EPSILON;
        } else if delta.z < 0.0 {
            let wall = (candidate.z - PLAYER_RADIUS).ceil();
            new_pos.z = wall + PLAYER_RADIUS + COLLISION_EPSILON;
        }
    } else {
        new_pos.z = candidate.z;
    }

    (new_pos, hit_floor, hit_ceiling)
}
