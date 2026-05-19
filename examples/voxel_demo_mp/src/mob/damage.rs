//! Authoritative mob damage application + death + loot drop.
//!
//! Owns the path from "player swing connects" through "HP -= n" to
//! "broadcast hurt / death + drop loot". Pure refactor of the
//! original `mob_ai::damage_mob` + `damage_mob_scaled` - no logic
//! changes here; H.1 just splits the file.
//!
//! MC mirror:
//! - `net/minecraft/world/entity/LivingEntity.java::actuallyHurt` -
//!   the canonical damage application path: clamp by armour, fire
//!   the death event when HP <= 0, drop loot tables.
//! - `net/minecraft/world/entity/Mob.java::dropFromLootTable` -
//!   per-kind drop tables. The InfEngine demo carries a much
//!   simpler hand-rolled drop table (1-3 wool, 1-2 meat, ...).

use infengine_voxel::BlockId;

use crate::Voxel;
use crate::inventory::inv_pickup;
use crate::net::{MobEventMsg, NetRole};

use super::kinds::{KIND_CHICKEN, KIND_COW, KIND_PIG};
use super::state::{MOB_DEATH_FADE, MOB_HIT_DAMAGE};

/// Authoritative damage application. Caller must own the
/// world simulation (offline or host server). Subtracts
/// `MOB_HIT_DAMAGE` from the mob's health, broadcasts a hurt
/// or death event, and on death drops loot. `attacker_client_id`
/// is `None` for "no inventory credit" (e.g. fall damage path
/// later).
pub(crate) fn damage_mob(voxel: &mut Voxel, mob_id: u32, attacker_client_id: Option<u32>) {
    damage_mob_scaled(voxel, mob_id, attacker_client_id, 1.0);
}

/// Same as `damage_mob` but applies a multiplier to
/// `MOB_HIT_DAMAGE` so sword-tier weapons can swing harder.
pub(crate) fn damage_mob_scaled(
    voxel: &mut Voxel,
    mob_id: u32,
    attacker_client_id: Option<u32>,
    mult: f32,
) {
    // Find the mob (server-side).
    let Some(idx) = voxel.mobs.iter().position(|m| m.id == mob_id) else {
        return;
    };
    let mob_pos;
    let killed;
    let already_dying;
    {
        let m = &mut voxel.mobs[idx];
        already_dying = m.dying > 0.0;
        if !already_dying {
            m.health -= MOB_HIT_DAMAGE * mult.max(0.1);
        }
        mob_pos = m.pos;
        killed = m.health <= 0.0 && !already_dying;
    }
    // No re-damage during the death fade.
    if already_dying {
        return;
    }
    // Local visual flash on every peer that has the mob.
    voxel.mob_hurt_flash.insert(mob_id, 0.30);
    // Reset the no-hit timer so the regen path waits anew.
    voxel.mob_no_hit_time.insert(mob_id, 0.0);
    // Broadcast the event so remote clients flash + play
    // sound. Hurt = 0, Death = 1.
    if let NetRole::Server { server, .. } = &voxel.net {
        let cfg = bincode::config::standard();
        let evt = MobEventMsg {
            id: mob_id,
            kind: if killed { 1 } else { 0 },
        };
        if let Ok(buf) = bincode::serde::encode_to_vec(evt, cfg) {
            server.broadcast_bytes("vx.mob_event", buf);
        }
    }
    // Hurt SFX locally too.
    voxel.sfx_at("mob/sheep/say", mob_pos, |a, g| {
        let _ = a.play_beep(280.0, 0.12, 0.20 * g);
    });
    if !killed {
        return;
    }
    // Death: start the fade timer. Mob stays in `voxel.mobs`
    // (skipping wander logic) until the fade completes, then
    // `tick_mobs` evicts it + broadcasts `vx.mob_remove`. Loot
    // drops immediately so the attacker doesn't have to wait
    // for the animation.
    voxel.mobs[idx].dying = MOB_DEATH_FADE;
    voxel.mob_dying.insert(mob_id, MOB_DEATH_FADE);
    // Per-kind loot table. Sheep drops 1-3 wool + 1-2 meat;
    // chicken drops 1-2 feathers + 1-2 meat. Quantities are
    // seeded from the mob id so save/replay matches.
    let kind = voxel.mobs[idx].kind;
    let seed = mob_id.wrapping_mul(2654435761);
    let meat_qty = (1 + ((seed >> 8) & 0x1) as u8).min(2);
    let (loot_id, loot_qty) = if kind == KIND_CHICKEN {
        (BlockId(19), (1 + ((seed >> 4) & 0x1) as u8).min(2))
    } else if kind == KIND_PIG {
        // Extra meat, no fibre drop.
        (BlockId(12), (2 + (seed & 0x1) as u8).min(3))
    } else if kind == KIND_COW {
        // Cow drops leather (placeholder = wool) + extra meat.
        (BlockId(11), (1 + (seed & 0x1) as u8).min(2))
    } else {
        (BlockId(11), (1 + (seed & 0x3) as u8).min(3))
    };
    match attacker_client_id {
        None | Some(0) => {
            for _ in 0..loot_qty {
                voxel.pickup(loot_id);
            }
            for _ in 0..meat_qty {
                voxel.pickup(BlockId(12));
            }
            voxel.kill_count = voxel.kill_count.saturating_add(1);
        }
        Some(cid) => {
            if let NetRole::Server {
                inventories,
                pending_inv_sends,
                ..
            } = &mut voxel.net
                && let Some(inv) = inventories.get_mut(&cid)
            {
                for _ in 0..loot_qty {
                    inv_pickup(&mut inv.inventory, loot_id);
                }
                for _ in 0..meat_qty {
                    inv_pickup(&mut inv.inventory, BlockId(12));
                }
                pending_inv_sends.insert(cid);
            }
        }
    }
    // Particle burst at the death position so the kill reads
    // visually even when the SFX is muted.
    let target_voxel = [
        mob_pos[0].floor() as i32,
        mob_pos[1].floor() as i32,
        mob_pos[2].floor() as i32,
    ];
    voxel.spawn_break_particles(target_voxel);
}
