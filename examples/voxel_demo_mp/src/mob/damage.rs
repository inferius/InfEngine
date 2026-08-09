//! Authoritative mob damage application + death + loot drop.
//!
//! Owns the path from "player swing connects" through "HP -= n" to
//! "broadcast hurt / death + drop loot".
//!
//! MC mirror:
//! - `net/minecraft/world/entity/LivingEntity.java::actuallyHurt` -
//!   the canonical damage application path: clamp by armour
//!   (W474: `CombatRules.getDamageAfterAbsorb` over the mob's ARMOR
//!   attribute), fire the death event when HP <= 0, drop loot.
//! - `net/minecraft/world/entity/Mob.java::dropFromLootTable` -
//!   per-kind drop tables. W474: `mob_loot` rolls the generated
//!   `mob::loot_table` ports of `loot_table/entities/<kind>.json`.

use infengine_voxel::BlockId;

use crate::Voxel;
use crate::inventory::inv_pickup;
use crate::net::{MobEventMsg, NetRole};

use super::kinds::{
    KIND_SKELETON, KIND_SPIDER, KIND_ZOMBIE, KIND_ZOMBIFIED_PIGLIN,
};
use super::state::{MOB_DEATH_FADE, MOB_HIT_DAMAGE};

/// Authoritative damage application at the 1.0 baseline (=
/// `MOB_HIT_DAMAGE` raw). Caller must own the world simulation
/// (offline or host server). Broadcasts a hurt or death event and
/// on death drops loot. `attacker_client_id` is `None` for "no
/// player credit" (blast / mob-vs-mob / command kills - those also
/// skip the killed_by_player loot pools).
pub(crate) fn damage_mob(voxel: &mut Voxel, mob_id: u32, attacker_client_id: Option<u32>) {
    damage_mob_scaled(voxel, mob_id, attacker_client_id, 1.0);
}

/// The scaled damage entry: callers express an absolute MC damage
/// as `mult = damage / MOB_HIT_DAMAGE`. W474: the raw amount then
/// passes through the target's ARMOR attribute (CombatRules) before
/// it lands.
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
    let mob_sheared;
    let mob_size;
    {
        let m = &mut voxel.mobs[idx];
        already_dying = m.dying > 0.0;
        mob_sheared = m.sheared;
        mob_size = m.size;
        if !already_dying {
            // W474: MC `getDamageAfterArmorAbsorb` - the mob's ARMOR
            // attribute (zombie family 2, everything else 0) shaves
            // the incoming damage through the same CombatRules curve
            // the player armor path uses. Mob ARMOR_TOUGHNESS is 0
            // for every ported kind.
            let raw = MOB_HIT_DAMAGE * mult.max(0.1);
            let armor = super::attributes::mob_attributes(m.kind).armor;
            m.health -=
                crate::player::damage_after_armor_absorb(raw, armor, 0.0);
        }
        mob_pos = m.pos;
        kind = m.kind;
        killed = m.health <= 0.0 && !already_dying;
        // W289: MC PanicGoal - a hurt passive animal sprints away
        // (away from the attacker: the demo tracks no attacker pos,
        // so flee AWAY FROM THE PLAYER, which is who hits mobs).
        // Hostiles keep chasing instead.
        if !killed && kind == crate::mob::kinds::KIND_ENDERMAN {
            // W432: a hit angers the enderman AND usually blinks it
            // away (EnderMan.hurt teleports on most damage). The
            // landing spot walks down from a random offset to the
            // first solid floor; no floor inside the probe = stay.
            let m = &mut voxel.mobs[idx];
            let h = (mob_id.wrapping_mul(0x9E37_79B1) >> 7) % 1900;
            m.anger_left = 20.0 + h as f32 / 100.0;
            let r1 = mob_id.wrapping_mul(0x2545_F491);
            if r1 % 10 < 6 {
                let dx = ((r1 >> 8) % 33) as i32 - 16;
                let dz = ((r1 >> 16) % 33) as i32 - 16;
                let bx = m.pos[0].floor() as i32 + dx;
                let bz = m.pos[2].floor() as i32 + dz;
                let mut by = m.pos[1].floor() as i32 + 8;
                let mut landed = None;
                for _ in 0..24 {
                    let below = voxel.world.get([bx, by - 1, bz]);
                    let here = voxel.world.get([bx, by, bz]);
                    let above = voxel.world.get([bx, by + 1, bz]);
                    if here.is_air()
                        && above.is_air()
                        && !below.is_air()
                        && crate::block::block_fully_occludes(below)
                    {
                        landed = Some(by);
                        break;
                    }
                    by -= 1;
                }
                if let Some(y) = landed {
                    let from = m.pos;
                    m.pos = [bx as f32 + 0.5, y as f32, bz as f32 + 0.5];
                    m.target = m.pos;
                    voxel.sfx_at("mob/enderman/teleport", from, |_, _| {});
                    let cell = [
                        from[0].floor() as i32,
                        from[1].floor() as i32,
                        from[2].floor() as i32,
                    ];
                    voxel.spawn_place_particles(cell, [0.65, 0.2, 0.75]);
                }
            }
        } else if !killed && kind == KIND_ZOMBIFIED_PIGLIN {
            // W300: NeutralMob - a hit ANGERS the piglin (MC
            // `PERSISTENT_ANGER_TIME = rangeOfSeconds(20, 39)`)
            // instead of panicking it; the AI chases + melees while
            // the timer runs. Cheap position hash stands in for the
            // uniform roll.
            let m = &mut voxel.mobs[idx];
            let h = (mob_id.wrapping_mul(0x9E37_79B1) >> 7) % 1900;
            m.anger_left = 20.0 + h as f32 / 100.0;
        } else if !killed
            && !matches!(kind, KIND_ZOMBIE | KIND_SKELETON | KIND_SPIDER)
        {
            let day_time = voxel.day_time;
            let px = voxel.player_pos.x;
            let pz = voxel.player_pos.z;
            let m = &mut voxel.mobs[idx];
            let mut fx = m.pos[0] - px;
            let mut fz = m.pos[2] - pz;
            let len = (fx * fx + fz * fz).sqrt();
            if len < 1e-3 {
                fx = 1.0;
                fz = 0.0;
            } else {
                fx /= len;
                fz /= len;
            }
            m.panic_left = 3.0;
            m.strolling = true;
            m.target = [m.pos[0] + fx * 9.0, m.pos[1], m.pos[2] + fz * 9.0];
            m.retarget_at =
                (day_time + 4.0).rem_euclid(crate::DAY_LENGTH_S);
        }
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
    // W429: a size > 1 slime splits into 2-4 halves at death
    // (Slime.remove SPLIT_ON_DEATH). The children spawn in a ring,
    // despawnable, HP = (size/2)^2.
    if kind == crate::mob::kinds::KIND_SLIME {
        let parent_size = voxel.mobs[idx].size;
        if parent_size > 1 {
            let half = parent_size / 2;
            let n = 2 + (mob_id.wrapping_mul(0x2545_F491) >> 7) % 3;
            for ci in 0..n {
                let cid = voxel.next_mob_id;
                voxel.next_mob_id = voxel.next_mob_id.wrapping_add(1).max(1);
                let ang = ci as f32 / n as f32 * std::f32::consts::TAU;
                let cpos = [
                    mob_pos[0] + ang.cos() * 0.6,
                    mob_pos[1] + 0.3,
                    mob_pos[2] + ang.sin() * 0.6,
                ];
                let hp = (half as f32) * (half as f32);
                let mut child = crate::mob::state::MobState::new_natural(
                    cid, kind, cpos, cpos, ang, 0.0, hp,
                );
                child.size = half;
                voxel.mobs.push(child);
                voxel.remote_mobs.insert(
                    cid,
                    crate::net::MobMsg {
                        id: cid,
                        kind,
                        pos: cpos,
                        yaw: ang,
                        health: hp.max(0.0).round() as u8,
                        head_yaw: 0.0,
                        head_pitch: 0.0,
                        baby: false,
                        size: half,
                        sheared: false,
                    },
                );
            }
        }
    }
    // W418: kill XP to the host player (MC ExperienceOrb reward;
    // per-client XP pools are a follow-up with the rest of the
    // client-side survival state).
    if matches!(attacker_client_id, None | Some(0)) {
        let xp_seed = mob_id.wrapping_mul(0x9E37_79B9);
        voxel.player_xp = voxel
            .player_xp
            .saturating_add(crate::xp::mob_kill_xp(kind, xp_seed));
    }
    // W474: real MC entity loot tables (see `mob_loot`), seeded from
    // the mob id so save / replay matches. The kill either
    // auto-collects to the host inventory or credits the attacking
    // client's server-side inventory. `killed_by_player` pools roll
    // only when the kill credits a player (host or client) - fall /
    // blast / mob-vs-mob deaths pass `None` and skip them, like MC.
    let seed = mob_id.wrapping_mul(2654435761);
    let loot = mob_loot(
        kind,
        seed,
        attacker_client_id.is_some(),
        mob_sheared,
        mob_size,
    );
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

/// W474: roll a kind's real MC entity loot tables into `(item_id,
/// qty)` stacks. Deterministic per `seed` (the mob-id hash) so save /
/// replay matches. `player_kill` gates the `killed_by_player` pools;
/// `sheared` gates the sheep wool pool; `size` gates the magma-cream
/// pool (tiny cubes drop none).
///
/// The old wool+meat fallback is dead: kinds whose MC table is empty
/// (wolf) or entirely unresolvable (rabbit, until the items exist)
/// drop NOTHING, exactly like the data says. Unresolvable item names
/// skip with the once-per-name log in `crate::loot`.
fn mob_loot(
    kind: u8,
    seed: u32,
    player_kill: bool,
    sheared: bool,
    size: u8,
) -> Vec<(u16, u8)> {
    use crate::worldgen::mcgen::noise::SplitMix64;
    let mut rng = SplitMix64::new(
        (seed as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0x004D_4F42_4C4F_4F54,
    );
    let mut out: Vec<(u16, u8)> = Vec::new();
    if kind == crate::mob::kinds::KIND_SHEEP {
        out.extend(crate::loot::roll(
            &super::loot_table::SHEEP_BASE,
            &mut rng,
        ));
        // `entities/sheep.json` pool 2 rolls only for an unsheared
        // sheep (the `sheared: false` predicate on every color ref).
        if !sheared {
            out.extend(crate::loot::roll(
                &super::loot_table::SHEEP_WOOL,
                &mut rng,
            ));
        }
        return out;
    }
    if kind == crate::mob::kinds::KIND_MAGMA_CUBE {
        // `entities/magma_cube.json`: magma cream only for size > 1
        // (the inverted size-1 predicate). Engine cubes mint at
        // size 1 today, so no cream until the spawn wave sizes them.
        if size > 1 {
            out.extend(crate::loot::roll(
                &super::loot_table::MAGMA_CUBE_SIZED,
                &mut rng,
            ));
        }
        return out;
    }
    let (base, player_only) = super::loot_table::entity_tables(kind);
    out.extend(crate::loot::roll(base, &mut rng));
    if player_kill {
        out.extend(crate::loot::roll(player_only, &mut rng));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mob::kinds::{
        KIND_CHICKEN, KIND_COW, KIND_PIG, KIND_POLAR_BEAR, KIND_SHEEP,
        KIND_SKELETON, KIND_SPIDER, KIND_WOLF, KIND_ZOMBIE,
    };

    /// Item ids the loot names resolve to (item/mod.rs rows).
    const RAW_MUTTON: u16 = 286;
    const WOOL: u16 = 11;
    const RAW_COD: u16 = 288;
    const RAW_SALMON: u16 = 290;
    const ROTTEN_FLESH: u16 = 293;
    const LEGACY_MEAT: u16 = 12;

    /// W474: the wolf's MC table has NO pools - a dead wolf drops
    /// nothing (the old fallback gave every unlisted kind wool+meat).
    #[test]
    fn wolf_drops_nothing() {
        for seed in 0u32..64 {
            assert!(
                mob_loot(KIND_WOLF, seed, true, false, 1).is_empty(),
                "wolf.json is pool-free"
            );
        }
    }

    /// W474: polar bear rolls the cod-w3 / salmon-w1 pool (0-2 of
    /// the picked fish); nothing outside those two items ever drops.
    #[test]
    fn polar_bear_drops_cod_or_salmon_per_weights() {
        let mut cod = 0u32;
        let mut salmon = 0u32;
        for seed in 0u32..2000 {
            for (id, q) in mob_loot(KIND_POLAR_BEAR, seed, false, false, 1) {
                assert!(
                    id == RAW_COD || id == RAW_SALMON,
                    "unexpected polar bear drop {id}"
                );
                assert!((1..=2).contains(&q));
                if id == RAW_COD {
                    cod += 1;
                } else {
                    salmon += 1;
                }
            }
        }
        assert!(cod > 0 && salmon > 0, "both fish must appear");
        assert!(cod > salmon * 2, "cod weight 3 vs salmon 1: {cod}/{salmon}");
    }

    /// W474: sheep = 1-2 mutton always; wool ONLY while unsheared.
    #[test]
    fn sheep_wool_gates_on_sheared() {
        for seed in 0u32..64 {
            let full = mob_loot(KIND_SHEEP, seed, true, false, 1);
            assert!(
                full.iter().any(|(id, q)| *id == RAW_MUTTON && (1..=2).contains(q)),
                "mutton always drops"
            );
            assert!(
                full.iter().any(|(id, q)| *id == WOOL && *q == 1),
                "unsheared sheep drops 1 wool"
            );
            let shorn = mob_loot(KIND_SHEEP, seed, true, true, 1);
            assert!(
                shorn.iter().any(|(id, _)| *id == RAW_MUTTON),
                "mutton still drops sheared"
            );
            assert!(
                !shorn.iter().any(|(id, _)| *id == WOOL),
                "sheared sheep drops no wool"
            );
        }
    }

    /// W474: killed_by_player pools (spider_eye) roll only on player
    /// kills; the base string pool is unconditional. And no kind
    /// drops the legacy generic meat id anymore.
    #[test]
    fn spider_eye_needs_player_kill() {
        const SPIDER_EYE: u16 = 300;
        let eye_id = crate::item::ITEM_TYPES
            .iter()
            .find(|it| it.name == "spider_eye")
            .map(|it| it.id)
            .expect("spider_eye item exists");
        assert_eq!(eye_id, SPIDER_EYE, "test constant tracks the registry");
        let mut eyes_player = 0u32;
        for seed in 0u32..2000 {
            for (id, _) in mob_loot(KIND_SPIDER, seed, false, false, 1) {
                assert_ne!(id, SPIDER_EYE, "no eye without player credit");
            }
            if mob_loot(KIND_SPIDER, seed, true, false, 1)
                .iter()
                .any(|(id, _)| *id == SPIDER_EYE)
            {
                eyes_player += 1;
            }
        }
        // The eye entry wins 1-in-3 of its pool (weighted blank).
        assert!(
            (400..=1000).contains(&eyes_player),
            "eye rate off: {eyes_player}/2000"
        );
    }

    /// W474: the old wool+meat `_ =>` fallback is gone - hostiles
    /// stay meat-free and drop their table items only.
    #[test]
    fn hostiles_drop_table_items_and_no_meat() {
        for seed in 0u32..64 {
            let z = mob_loot(KIND_ZOMBIE, seed, true, false, 1);
            assert!(z.iter().all(|(id, _)| *id == ROTTEN_FLESH));
            let s = mob_loot(KIND_SKELETON, seed, true, false, 1);
            assert!(
                !s.iter().any(|(id, _)| *id == LEGACY_MEAT),
                "skeleton meat-free"
            );
        }
    }

    /// Passive staples keep their MC produce: chicken feather+meat,
    /// pig 1-3 porkchop, cow leather+beef.
    #[test]
    fn passives_drop_mc_produce() {
        const RAW_BEEF: u16 = 342;
        const RAW_PORKCHOP: u16 = 284;
        const RAW_CHICKEN: u16 = 165;
        let mut beef_seen = false;
        let mut chicken_seen = false;
        let mut pork_seen = false;
        for seed in 0u32..64 {
            beef_seen |= mob_loot(KIND_COW, seed, true, false, 1)
                .iter()
                .any(|(id, _)| *id == RAW_BEEF);
            chicken_seen |= mob_loot(KIND_CHICKEN, seed, true, false, 1)
                .iter()
                .any(|(id, _)| *id == RAW_CHICKEN);
            pork_seen |= mob_loot(KIND_PIG, seed, true, false, 1)
                .iter()
                .any(|(id, q)| *id == RAW_PORKCHOP && (1..=3).contains(q));
        }
        assert!(beef_seen && chicken_seen && pork_seen);
    }
}
