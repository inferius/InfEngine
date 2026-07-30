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
/// - mob: host applies damage_mob_scaled directly with the held
///   weapon's multiplier; client sends vx.mob_hit + paints an
///   optimistic hurt flash + plays the hit SFX
pub(crate) fn apply_combat(
    vx: &mut crate::Voxel,
    attack_player: Option<u32>,
    attack_mob: Option<u32>,
    is_client: bool,
) {
    use crate::net::NetRole;
    use crate::{
        BlockId, DamageMsg, MobHitMsg, PVP_DAMAGE, PvPHitMsg,
        weapon_damage_mult,
    };
    if (attack_player.is_some() || attack_mob.is_some())
        && !vx.abilities.invulnerable
    {
        vx.stamina = (vx.stamina - 3.0).max(0.0);
    }
    if let Some(target_id) = attack_player {
        let held = vx
            .inventory
            .get(vx.selected_slot)
            .and_then(|s| s.as_ref())
            .map(|st| BlockId(st.id))
            .unwrap_or(BlockId::AIR);
        let mult = weapon_damage_mult(held);
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
                    amount: PVP_DAMAGE * mult,
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
                .map(|st| BlockId(st.id))
                .unwrap_or(BlockId::AIR);
            let mult = weapon_damage_mult(held);
            vx.damage_mob_scaled(mob_id, Some(0), mult);
        }
    }
}
