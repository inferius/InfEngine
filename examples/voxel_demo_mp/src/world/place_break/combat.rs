//! W47.14 - combat submodule (split from W47.13 monolith).
//!
//! Contains `apply_combat` (W47.13k), the PvP + mob attack dispatch
//! that runs once per frame after `resolve_attack_target` picks the
//! winning target. Drains 3 stamina (invulnerable exempt) and routes
//! the hit through the right net path.

/// W47.13k: apply the per-frame combat outcome from
/// `resolve_attack_target`.
///
/// `attack_player` / `attack_mob` are the winning combat targets
/// (at most one is Some). `is_client` mirrors the caller's net role
/// check.
///
/// Side effects:
/// - drains 3 stamina when an attack lands and the player is not
///   `invulnerable`
/// - PvP: host sends DamageMsg straight to the target client;
///   client sends a vx.pvp_hit request the server will validate
/// - mob: host applies damage_mob_scaled with the held weapon's
///   MC attack damage; client sends vx.mob_hit + paints an
///   optimistic hurt flash + plays the hit SFX
///
/// W473: damage comes from `mining_math::attack_damage` - the MC
/// `Attributes.ATTACK_DAMAGE` total (player base 1.0 + the held
/// item's mainhand modifier: wood sword 4 ... netherite sword 8,
/// axes 7-10). MC's attack cooldown / charge meter is an absent
/// mechanic - every landed swing deals the full value.
pub(crate) fn apply_combat(
    vx: &mut crate::Voxel,
    attack_player: Option<u32>,
    attack_mob: Option<u32>,
    is_client: bool,
) {
    use crate::block::mining_math::attack_damage;
    use crate::net::NetRole;
    use crate::{DamageMsg, MobHitMsg, PvPHitMsg};
    if (attack_player.is_some() || attack_mob.is_some())
        && !vx.abilities.invulnerable
    {
        vx.stamina = (vx.stamina - 3.0).max(0.0);
    }
    // W471: melee wear (MC `ItemStack.postHurtEnemy` reading the
    // `Weapon.item_damage_per_attack` component: swords 1, digger
    // tools 2). One wear application per landed swing, on whatever
    // the attacker holds; the helper no-ops for non-weapons and in
    // creative. Clients apply this as PREDICTION on their own
    // mirror - the server wears the authoritative `ClientInv` copy
    // when it validates the hit and the next snapshot reconciles.
    if attack_player.is_some() || attack_mob.is_some() {
        let wear = vx
            .inventory
            .get(vx.selected_slot)
            .and_then(|s| s.as_ref())
            .map(|st| crate::item::durability::attack_wear_amount(st.id))
            .unwrap_or(0);
        vx.wear_selected_stack(wear);
    }
    if let Some(target_id) = attack_player {
        let held = vx
            .inventory
            .get(vx.selected_slot)
            .and_then(|s| s.as_ref())
            .map(|st| st.id)
            .unwrap_or(0);
        match &vx.net {
            NetRole::Client { client, .. } => {
                let cfg = bincode::config::standard();
                let msg = PvPHitMsg { target_id };
                if let Ok(buf) = bincode::serde::encode_to_vec(msg, cfg) {
                    let _ = client.send_bytes("vx.pvp_hit", buf);
                }
            }
            NetRole::Server { server, .. } => {
                let cfg = bincode::config::standard();
                let dmg = DamageMsg {
                    amount: attack_damage(held),
                };
                if let Ok(buf) = bincode::serde::encode_to_vec(dmg, cfg) {
                    server.send_bytes_to_client(target_id, "vx.damage", buf);
                }
            }
            NetRole::Offline => {}
        }
    }
    if let Some(mob_id) = attack_mob {
        if is_client {
            let msg = MobHitMsg { mob_id };
            if let NetRole::Client { client, .. } = &vx.net {
                let cfg = bincode::config::standard();
                if let Ok(buf) = bincode::serde::encode_to_vec(msg, cfg) {
                    let _ = client.send_bytes("vx.mob_hit", buf);
                }
            }
            vx.mob_hurt_flash.insert(mob_id, 0.30);
            // W123: predicted per-kind hurt vocal (the server echoes
            // the authoritative event, which remote peers hear).
            if let Some(mob) = vx.remote_mobs.get(&mob_id).copied() {
                let key = format!(
                    "{}/hurt",
                    crate::mob::audio::mob_sound_stem(mob.kind),
                );
                vx.sfx_at(&key, mob.pos, |_, _| {});
            }
        } else {
            let held = vx
                .inventory
                .get(vx.selected_slot)
                .and_then(|s| s.as_ref())
                .map(|st| st.id)
                .unwrap_or(0);
            // Express the absolute MC damage through the scaled
            // API (same idiom as the projectile / explosion
            // callers: mult = damage / MOB_HIT_DAMAGE).
            vx.damage_mob_scaled(
                mob_id,
                Some(0),
                attack_damage(held) / crate::mob::MOB_HIT_DAMAGE,
            );
        }
    }
}
