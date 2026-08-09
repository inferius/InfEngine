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

use std::collections::{HashMap, HashSet};

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
/// W441: headroom between the eye and the collision-box top. MC's
/// standing pose is a 1.8 m box with the eye at 1.62 m, so the
/// camera stays well below the ceiling plane. Our eye used to BE
/// the box top - flying up against a ceiling put the camera's near
/// plane inside the block above and the R32 screen saw straight
/// through walls.
pub(crate) const EYE_HEADROOM: f32 = 0.18;
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
        Vec3::new(
            pos.x + PLAYER_RADIUS,
            pos.y + EYE_HEADROOM,
            pos.z + PLAYER_RADIUS,
        ),
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

/// W138: lift a player pose out of solid terrain. A stale save (or
/// a worldgen change underneath an old save) can hand back an eye
/// position INSIDE blocks - the camera spawned "in the ground" and
/// the player was stuck. Scans the body AABB against the collision
/// predicate; when buried, climbs straight up and returns the first
/// free eye position (`None` = already free / no free spot found).
pub(crate) fn unstuck_after_load(
    world: &World,
    snow: &HashMap<[i32; 3], u8>,
    door_open: &HashSet<[i32; 3]>,
    eye: Vec3,
) -> Option<Vec3> {
    let stuck = |eye: Vec3| -> bool {
        let (mn, mx) = body_aabb(eye);
        let x0 = mn.x.floor() as i32;
        let x1 = (mx.x - 1e-4).floor() as i32;
        let y0 = mn.y.floor() as i32;
        let y1 = (mx.y - 1e-4).floor() as i32;
        let z0 = mn.z.floor() as i32;
        let z1 = (mx.z - 1e-4).floor() as i32;
        for y in y0..=y1 {
            for z in z0..=z1 {
                for x in x0..=x1 {
                    if voxel_is_solid_with_doors(world, snow, door_open, [x, y, z]) {
                        return true;
                    }
                }
            }
        }
        false
    };
    if !stuck(eye) {
        return None;
    }
    let mut lifted = eye;
    // 256 = the full world height; plenty to clear any burial.
    for _ in 0..256 {
        lifted.y += 1.0;
        if !stuck(lifted) {
            return Some(lifted);
        }
    }
    None
}

/// Combined solid lookup: world solid AND not in the passable
/// decoration set AND, when the block is snow, has fewer than
/// 8 stacked layers (so a fully-buried voxel becomes a solid
/// block the player can stand on).
#[allow(dead_code)]
pub(crate) fn voxel_is_solid(
    world: &World,
    snow: &HashMap<[i32; 3], u8>,
    v: [i32; 3],
) -> bool {
    voxel_is_solid_with_doors(world, snow, &HashSet::new(), v)
}

/// W63.1 - solid lookup that also consults the engine's open-
/// door set. Open doors become passable; closed doors stay
/// solid. The shared `voxel_is_solid` keeps the old signature
/// for non-Voxel callers (worldgen / tests); the player
/// collision path threads `door_open` here.
pub(crate) fn voxel_is_solid_with_doors(
    world: &World,
    snow: &HashMap<[i32; 3], u8>,
    door_open: &HashSet<[i32; 3]>,
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
    // W63.1: open door is solid in flag table but passes
    // through here. Check the lower-half cell - upper half
    // shares the state.
    if crate::block::door::is_door_block(b.0) {
        let lower = if crate::block::door::is_door_block(world.get([v[0], v[1] - 1, v[2]]).0) {
            [v[0], v[1] - 1, v[2]]
        } else {
            v
        };
        if door_open.contains(&lower) {
            return false;
        }
    }
    // W125: open trapdoor passes (closed = solid; W125 removed the
    // unconditional F_AABB_PASS that made closed ones walk-through).
    if crate::block::door::is_trapdoor_block(b.0) && door_open.contains(&v) {
        return false;
    }
    // W167: open fence gate passes; closed = solid barrier.
    if crate::block::door::is_fence_gate_block(b.0) && door_open.contains(&v) {
        return false;
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
#[allow(dead_code)]
pub(crate) fn aabb_intersects_solid(
    world: &World,
    snow: &HashMap<[i32; 3], u8>,
    pos: Vec3,
) -> bool {
    aabb_intersects_solid_with_doors(
        world,
        snow,
        &HashSet::new(),
        &HashMap::new(),
        pos,
    )
}

/// W412: does the body AABB overlap a partial-height cell's real
/// MC voxel shape? `state` is the cell's `stair_states` byte.
/// - slab: bottom `[0, 0.5]`, top `[0.5, 1]` (full XZ; DOUBLE is
///   handled as a full cube by the caller).
/// - stairs: base slab (by the half bit) + the riser box covering
///   the facing-side half (MC stairs ascend TOWARD their facing;
///   geometry mirrors `emit_stairs`).
fn partial_cell_overlaps(
    bid: u16,
    state: u8,
    cell: [i32; 3],
    min: Vec3,
    max: Vec3,
) -> bool {
    let cy = cell[1] as f32;
    if crate::block::is_slab_block(bid) {
        let top = state & crate::block::door::TRAPDOOR_TOP_BIT != 0;
        return if top {
            max.y - COLLISION_EPSILON > cy + 0.5
        } else {
            min.y < cy + 0.5
        };
    }
    // Stairs.
    let top = state & crate::block::door::TRAPDOOR_TOP_BIT != 0;
    let (b0, b1) = if top { (0.5, 1.0) } else { (0.0, 0.5) };
    if max.y - COLLISION_EPSILON > cy + b0 && min.y < cy + b1 {
        return true;
    }
    let (r0, r1) = if top { (0.0, 0.5) } else { (0.5, 1.0) };
    if max.y - COLLISION_EPSILON > cy + r0 && min.y < cy + r1 {
        let cx = cell[0] as f32;
        let cz = cell[2] as f32;
        return match state & 3 {
            0 => max.x - COLLISION_EPSILON > cx + 0.5,
            1 => max.z - COLLISION_EPSILON > cz + 0.5,
            2 => min.x < cx + 0.5,
            _ => min.z < cz + 0.5,
        };
    }
    false
}

/// W63.1 - same as `aabb_intersects_solid` but consults the
/// engine's open-door set so the player can walk through doors
/// once they've toggled them open. W412: also takes the per-pos
/// orientation map (`Voxel::stair_states`) so slabs + stairs
/// collide by their real MC shapes - with the full-cube collision
/// the 0.6 auto-step could never climb a staircase (the R23 "can't
/// walk up stairs" report) and the player floated half a block
/// above slabs.
pub(crate) fn aabb_intersects_solid_with_doors(
    world: &World,
    snow: &HashMap<[i32; 3], u8>,
    door_open: &HashSet<[i32; 3]>,
    states: &HashMap<[i32; 3], u8>,
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
                // W412: partial-height cells collide by shape.
                let bid = world.get([x, y, z]).0;
                if bid != 0
                    && (crate::block::is_slab_block(bid)
                        || crate::block::door::is_stairs_block(bid))
                {
                    let st =
                        states.get(&[x, y, z]).copied().unwrap_or(0);
                    let full = crate::block::is_slab_block(bid)
                        && st & crate::block::door::SLAB_DOUBLE_BIT != 0;
                    if full
                        || partial_cell_overlaps(bid, st, [x, y, z], min, max)
                    {
                        return true;
                    }
                    continue;
                }
                if voxel_is_solid_with_doors(world, snow, door_open, [x, y, z]) {
                    return true;
                }
                // W396: fences, walls and CLOSED fence gates collide
                // 1.5 blocks tall in MC - the lower half of the cell
                // above one is solid too, so players cannot simply
                // hop them (the R23 report).
                if min.y < y as f32 + 0.5 {
                    let below = [x, y - 1, z];
                    let bid = world.get(below).0;
                    let fence_tall = crate::block::block_type(bid)
                        .map(|bt| {
                            bt.name.ends_with("fence")
                                || bt.name.ends_with("_wall")
                        })
                        .unwrap_or(false)
                        || (crate::block::door::is_fence_gate_block(bid)
                            && !door_open.contains(&below));
                    if fence_tall {
                        return true;
                    }
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
/// W419: total (defense, toughness) across the worn armor.
pub(crate) fn armor_totals(voxel: &Voxel) -> (f32, f32) {
    let mut def = 0.0;
    let mut tough = 0.0;
    for slot in &voxel.armor_slots {
        if let Some(st) = slot
            && let Some((_, d, t)) = crate::item::armor_props(st.id)
        {
            def += d;
            tough += t;
        }
    }
    (def, tough)
}

/// W474: MC `CombatRules.getDamageAfterAbsorb` - the armor-points
/// mitigation curve shared by the player (W419 wore it inline) and
/// by mobs with a nonzero ARMOR attribute (zombie family armor 2).
/// `def` = armor points, `tough` = armor toughness; def 0 passes the
/// damage through unchanged.
pub(crate) fn damage_after_armor_absorb(amount: f32, def: f32, tough: f32) -> f32 {
    if def <= 0.0 {
        return amount;
    }
    let f = 2.0 + tough / 4.0;
    let g = (def - amount / f).clamp(def * 0.2, 20.0);
    amount * (1.0 - g / 25.0)
}

/// W471: wear every worn damageable armor piece after a hit that
/// armor mitigates - the `LivingEntity.doHurtEquipment` port
/// (`durabilityDamage = (int)max(1.0, damage / 4.0)` to each of
/// the four humanoid slots). `damage` is the PRE-mitigation
/// amount, exactly as `getDamageAfterArmorAbsorb` passes it to
/// `hurtArmor`. A piece that reaches its max damage disappears
/// from its slot with the MC break sound. Caller has already
/// cleared the creative / godmode / invuln gates, so the wear
/// itself is unconditional (creative=false).
pub(crate) fn wear_armor_pieces(voxel: &mut Voxel, damage: f32) {
    let per_piece = crate::item::durability::armor_wear_amount(damage);
    if per_piece == 0 {
        return;
    }
    let mut broke_any = false;
    for slot in voxel.armor_slots.iter_mut() {
        if crate::item::durability::hurt_and_break(slot, per_piece, false) {
            broke_any = true;
        }
    }
    if broke_any {
        voxel.play_item_break_fx();
    }
    // W472: armor is part of the server-known E screen now; assert
    // the post-wear state so the host's authoritative copy (and
    // any later `vx.inv` snapshot) never reverts the damage.
    // Client-asserted, like the `sel` durability model (W471).
    if let crate::net::NetRole::Client { client, .. } = &mut voxel.net {
        let cfg = bincode::config::standard();
        let slots: Vec<Option<crate::item::ItemStack>> =
            voxel.armor_slots.to_vec();
        if let Ok(buf) = bincode::serde::encode_to_vec(&slots, cfg) {
            let _ = client.send_bytes("vx.armor", buf);
        }
    }
}

pub(crate) fn apply_damage_kind(voxel: &mut Voxel, amount: f32, kind: DamageKind) {
    if amount <= 0.0 {
        return;
    }
    // W419: armor absorbs physical + burn damage
    // (CombatRules.getDamageAfterAbsorb); drowning and the void
    // bypass it like MC. (Fall damage rides Generic here -
    // splitting it into its own kind is a follow-up... which W420
    // did: Fall is its own kind and bypasses, like the MC
    // `fall` row in the `bypasses_armor` damage-type tag.)
    // W471: the same kind set drives armor WEAR below - MC calls
    // `hurtArmor` inside `getDamageAfterArmorAbsorb`, i.e. armor
    // wears exactly when it mitigates, with the PRE-mitigation
    // damage amount.
    let armor_mitigates =
        matches!(kind, DamageKind::Generic | DamageKind::Burn);
    let pre_mitigation = amount;
    let amount = if armor_mitigates {
        // W474: the CombatRules curve moved to the shared
        // `damage_after_armor_absorb` (mobs with ARMOR use it too).
        let (def, tough) = armor_totals(voxel);
        damage_after_armor_absorb(amount, def, tough)
    } else {
        amount
    };
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
    // W471: armor wear - now that every no-damage early-out has
    // passed (MC skips `actuallyHurt` entirely for invulnerable
    // players). Each worn damageable piece takes
    // `max(1, floor(pre_mitigation / 4))` (`Player.hurtArmor` ->
    // `doHurtEquipment` over FEET/LEGS/CHEST/HEAD); a piece that
    // reaches max damage breaks off the body.
    if armor_mitigates {
        wear_armor_pieces(voxel, pre_mitigation);
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
            DamageKind::Fall => {
                // W420: same hurt thud as Generic - the split
                // exists for the armor bypass, not the sound.
                voxel.sfx_at("hurt", pos, |a, g| {
                    let _ = a.play_thud(140.0, 0.35 * g);
                });
            }
            DamageKind::Magic => {
                // W438: softer, higher hit - poison/harming.
                voxel.sfx_at("hurt", pos, |a, g| {
                    let _ = a.play_thud(220.0, 0.30 * g);
                });
            }
        }
    }
    if voxel.player_health <= 0.0 {
        tracing::warn!("player died (HP = 0) - respawning");
        // W123: death vocal at the death spot (before the respawn
        // teleport moves the listener away).
        let death_pos = [voxel.player_pos.x, voxel.player_pos.y, voxel.player_pos.z];
        voxel.sfx_at("player/death", death_pos, |a, g| {
            let _ = a.play_thud(90.0, 0.5 * g);
        });
        voxel.player_pos = voxel.safe_spawn();
        voxel.player_vel = Vec3::ZERO;
        voxel.player_health = PLAYER_MAX_HEALTH;
        voxel.invuln_remaining = 3.0;
        // W418: death wipes the XP pool (orb drops at the death
        // spot are a follow-up) and resets hunger to the MC
        // respawn state. W438: effects clear too.
        voxel.player_xp = 0;
        voxel.player_food = crate::hunger::FoodData::default();
        voxel.player_effects.clear();
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

#[allow(dead_code)]
pub(crate) fn sweep_aabb(
    world: &World,
    snow: &HashMap<[i32; 3], u8>,
    pos: Vec3,
    delta: Vec3,
) -> (Vec3, bool, bool) {
    sweep_aabb_with_doors(
        world,
        snow,
        &HashSet::new(),
        &HashMap::new(),
        pos,
        delta,
        false,
    )
}

/// W361: MC `maxUpStep` - a grounded mover walking into a ledge no
/// taller than this climbs it without jumping (stairs, slabs, snow
/// stacks). 0.6 clears a half-block step and never a full block,
/// exactly vanilla.
const STEP_UP: f32 = 0.6;

/// W361: try to resolve a horizontal collision by stepping up.
/// Returns the stepped position (already settled back down onto the
/// obstacle top) or None when the ledge is too tall / headroom is
/// missing. `candidate` is the blocked horizontal move at the
/// ORIGINAL height.
fn try_step_up(
    world: &World,
    snow: &HashMap<[i32; 3], u8>,
    door_open: &HashSet<[i32; 3]>,
    states: &HashMap<[i32; 3], u8>,
    candidate: Vec3,
) -> Option<Vec3> {
    let lifted = Vec3::new(candidate.x, candidate.y + STEP_UP, candidate.z);
    if aabb_intersects_solid_with_doors(world, snow, door_open, states, lifted)
    {
        return None;
    }
    // Settle back down onto the step top so the climb lands exactly
    // on the surface instead of leaving a 0.6 hop for gravity to
    // close (visible as a bounce). 0.05 granularity over the full
    // step height.
    let mut settled = lifted;
    for _ in 0..12 {
        let probe = Vec3::new(settled.x, settled.y - 0.05, settled.z);
        if aabb_intersects_solid_with_doors(
            world, snow, door_open, states, probe,
        ) {
            break;
        }
        settled = probe;
    }
    Some(settled)
}

/// W63.1 - sweep_aabb with the open-door set threaded through so
/// open doors stop blocking the player. Existing `sweep_aabb`
/// keeps the old signature for callers that don't have the
/// engine's door state on hand.
pub(crate) fn sweep_aabb_with_doors(
    world: &World,
    snow: &HashMap<[i32; 3], u8>,
    door_open: &HashSet<[i32; 3]>,
    states: &HashMap<[i32; 3], u8>,
    pos: Vec3,
    delta: Vec3,
    grounded: bool,
) -> (Vec3, bool, bool) {
    let mut new_pos = pos;
    let mut hit_floor = false;
    let mut hit_ceiling = false;
    // W361: step-up only applies to a grounded walker moving flat or
    // falling (a jumper already clears the ledge; MC gates on
    // onGround the same way).
    let can_step = grounded && delta.y <= 0.0;

    // X axis.
    let candidate = Vec3::new(new_pos.x + delta.x, new_pos.y, new_pos.z);
    if aabb_intersects_solid_with_doors(world, snow, door_open, states, candidate)
    {
        if let Some(stepped) = can_step
            .then(|| try_step_up(world, snow, door_open, states, candidate))
            .flatten()
        {
            new_pos = stepped;
        } else if delta.x > 0.0 {
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
    if aabb_intersects_solid_with_doors(world, snow, door_open, states, candidate)
    {
        if delta.y > 0.0 {
            // W412: partial-height undersides (top slab / top-stair
            // base) sit at the half boundary - try it before the
            // full-block snap.
            let wall = candidate.y.floor();
            let half = wall + 0.5;
            let half_pos =
                Vec3::new(candidate.x, half - COLLISION_EPSILON, candidate.z);
            new_pos.y = if half < candidate.y
                && !aabb_intersects_solid_with_doors(
                    world, snow, door_open, states, half_pos,
                ) {
                half - COLLISION_EPSILON
            } else {
                wall - COLLISION_EPSILON
            };
            hit_ceiling = true;
        } else if delta.y < 0.0 {
            // W412: slab / stair-base tops land at the half
            // boundary (feet at .5), not the integer ceil.
            let wall = (candidate.y - PLAYER_HEIGHT).ceil();
            let half = wall - 0.5;
            let half_pos = Vec3::new(
                candidate.x,
                half + PLAYER_HEIGHT + COLLISION_EPSILON,
                candidate.z,
            );
            new_pos.y = if half > candidate.y - PLAYER_HEIGHT
                && !aabb_intersects_solid_with_doors(
                    world, snow, door_open, states, half_pos,
                ) {
                half + PLAYER_HEIGHT + COLLISION_EPSILON
            } else {
                wall + PLAYER_HEIGHT + COLLISION_EPSILON
            };
            hit_floor = true;
        }
    } else {
        new_pos.y = candidate.y;
    }

    // Z axis.
    let candidate = Vec3::new(new_pos.x, new_pos.y, new_pos.z + delta.z);
    if aabb_intersects_solid_with_doors(world, snow, door_open, states, candidate)
    {
        if let Some(stepped) = can_step
            .then(|| try_step_up(world, snow, door_open, states, candidate))
            .flatten()
        {
            new_pos = stepped;
        } else if delta.z > 0.0 {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// W412: slabs + stairs collide by their MC voxel shape, not
    /// the full cube (full-cube stairs were unclimbable - auto-step
    /// is 0.6 and a cube is 1.0).
    #[test]
    fn slab_and_stairs_collide_by_shape() {
        let mut world = World::new();
        let mut ch = infengine_voxel::Chunk::empty();
        ch.set(8, 0, 8, BlockId(68)); // oak_slab (bottom by default)
        ch.set(10, 0, 8, BlockId(73)); // stone stairs
        world.insert_chunk([0, 0, 0], ch);
        let snow = HashMap::new();
        let doors = HashSet::new();
        let mut states = HashMap::new();
        states.insert([10, 0, 8], 0u8); // stairs facing +X, bottom

        let hit = |x: f32, feet: f32, states: &HashMap<[i32; 3], u8>| {
            aabb_intersects_solid_with_doors(
                &world,
                &snow,
                &doors,
                states,
                Vec3::new(x, feet + PLAYER_HEIGHT, 8.5),
            )
        };
        // Standing 0.55 above a bottom slab: clear (slab top = 0.5).
        assert!(!hit(8.5, 0.55, &states));
        // Feet inside the slab's lower half: collide.
        assert!(hit(8.5, 0.3, &states));
        // Stairs, front (low) half at 0.55: clear of both boxes.
        assert!(!hit(10.2, 0.55, &states));
        // Stairs, back (high, +X facing) half at 0.55: the riser.
        assert!(hit(10.7, 0.55, &states));
        // TOP slab: the lower half is walk-through headroom.
        let mut top_states = HashMap::new();
        top_states.insert(
            [8, 0, 8],
            crate::block::door::TRAPDOOR_TOP_BIT,
        );
        // Feet below the cell, box top (eye + EYE_HEADROOM, W441)
        // reaching 0.48 into it: clear (top slab starts at 0.5)...
        assert!(!aabb_intersects_solid_with_doors(
            &world,
            &snow,
            &doors,
            &top_states,
            Vec3::new(8.5, 0.3, 8.5),
        ));
        // ...but reaching past 0.5 hits the slab.
        assert!(aabb_intersects_solid_with_doors(
            &world,
            &snow,
            &doors,
            &top_states,
            Vec3::new(8.5, 0.7, 8.5),
        ));
    }
}
