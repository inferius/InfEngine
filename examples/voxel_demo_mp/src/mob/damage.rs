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

use super::kinds::{
    KIND_CHICKEN, KIND_COW, KIND_PIG, KIND_SKELETON, KIND_SPIDER, KIND_ZOMBIE,
};
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
    let kind;
    {
        let m = &mut voxel.mobs[idx];
        already_dying = m.dying > 0.0;
        if !already_dying {
            m.health -= MOB_HIT_DAMAGE * mult.max(0.1);
        }
        mob_pos = m.pos;
        kind = m.kind;
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
    // W123: per-kind hurt / death SFX (real MC OGGs via the bank).
    // Death plays the death vocal, plain hits the hurt one. No synth
    // fallback - silence beats the W56.1 beep when the pack is absent.
    let stem = super::audio::mob_sound_stem(kind);
    let key = if killed {
        format!("{stem}/death")
    } else {
        format!("{stem}/hurt")
    };
    voxel.sfx_at(&key, mob_pos, |_, _| {});
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
    // Per-kind hand-rolled loot table (see `mob_loot`). Quantities
    // are seeded from the mob id so save / replay matches. The kill
    // either auto-collects to the host inventory or credits the
    // attacking client's server-side inventory.
    let seed = mob_id.wrapping_mul(2654435761);
    let loot = mob_loot(kind, seed);
    match attacker_client_id {
        None | Some(0) => {
            for (id, qty) in &loot {
                for _ in 0..*qty {
                    voxel.pickup(BlockId(*id));
                }
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
                for (id, qty) in &loot {
                    for _ in 0..*qty {
                        inv_pickup(&mut inv.inventory, BlockId(*id));
                    }
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

/// Per-kind drop table. Returns `(item_id, qty)` pairs auto-collected
/// on the kill. Ids cover block-form drops (wool 11, meat 12, feather
/// 19) and item-only drops (rotten_flesh 293, bone 294, string 301,
/// leather 303, arrow 643). `seed` is the mob-id hash so quantities
/// replay deterministically.
///
/// MC mirror: each `EntityType` ships a loot-table JSON; we hand-roll
/// the common drops. Hostiles yield their signature item and no meat;
/// passives yield produce + meat.
fn mob_loot(kind: u8, seed: u32) -> Vec<(u16, u8)> {
    let one_or_two = 1 + ((seed >> 4) & 0x1) as u8;
    let zero_to_two = (((seed >> 8) & 0x3) as u8) % 3;
    match kind {
        KIND_CHICKEN => vec![(19, one_or_two), (12, one_or_two)],
        KIND_PIG => vec![(12, 2 + (seed & 0x1) as u8)],
        KIND_COW => vec![(303, one_or_two), (12, one_or_two)],
        KIND_ZOMBIE => vec![(293, one_or_two)],
        KIND_SKELETON => vec![(294, one_or_two), (643, zero_to_two)],
        KIND_SPIDER => vec![(301, zero_to_two)],
        // Sheep (and any other passive default): wool + meat.
        _ => vec![(11, 1 + ((seed & 0x3) as u8) % 3), (12, one_or_two)],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostiles_drop_signature_item_and_no_meat() {
        for seed in 0u32..64 {
            let z = mob_loot(KIND_ZOMBIE, seed);
            assert!(z.iter().any(|(id, q)| *id == 293 && *q >= 1));
            assert!(!z.iter().any(|(id, _)| *id == 12), "zombie meat-free");
            let s = mob_loot(KIND_SKELETON, seed);
            assert!(s.iter().any(|(id, q)| *id == 294 && *q >= 1));
            assert!(!s.iter().any(|(id, _)| *id == 12), "skeleton meat-free");
            let sp = mob_loot(KIND_SPIDER, seed);
            assert!(!sp.iter().any(|(id, _)| *id == 12), "spider meat-free");
        }
    }

    #[test]
    fn passives_drop_meat_and_cow_yields_leather() {
        for kind in [KIND_COW, KIND_CHICKEN, KIND_PIG] {
            assert!(
                mob_loot(kind, 0x1234).iter().any(|(id, _)| *id == 12),
                "kind {kind} drops meat",
            );
        }
        assert!(mob_loot(KIND_COW, 0).iter().any(|(id, _)| *id == 303));
    }
}
