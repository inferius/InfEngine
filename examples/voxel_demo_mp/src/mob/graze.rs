//! W474 - sheep grass eating (MC `EatBlockGoal` port).
//!
//! Host-side pass over the mob list: an adult sheep occasionally
//! (1-in-1000 per 20 Hz tick, the MC `canUse` gate) starts the
//! 40-tick eat when it stands IN an edible plant (the
//! `EDIBLE_FOR_SHEEP` tag - our short grass) or ON a grass block.
//! Four ticks before the animation ends the bite lands: the plant
//! breaks (no drops) or the grass block turns to dirt, and
//! `Sheep.ate()` clears the sheared flag so the wool regrows.
//!
//! MC mirror:
//! - `net/minecraft/world/entity/ai/goal/EatBlockGoal.java` - the
//!   gate, the 40-tick timer, the `adjustedTickDelay(4)` bite
//!   moment, `destroyBlock` / `setBlock(DIRT)`.
//! - `net/minecraft/world/entity/animal/sheep/Sheep.java::ate` -
//!   `setSheared(false)` on a successful bite.
//!
//! Scope notes: the baby 1-in-50 gate + `ageUp(60)` growth boost are
//! queued (babies have no wool to regrow); the head-down eat pose is
//! a render follow-up.

use infengine_voxel::BlockId;

use crate::Voxel;
use crate::net::NetRole;

use super::kinds::KIND_SHEEP;
use super::spawn::SpawnRng;

/// MC `EAT_ANIMATION_TICKS` = 40 game ticks at 20 TPS.
pub(crate) const EAT_TIME_S: f32 = 2.0;
/// The bite lands `adjustedTickDelay(4)` ticks before the timer
/// ends (EatBlockGoal.tick's `eatAnimationTick == 4` moment).
pub(crate) const EAT_BITE_MARK_S: f32 = 4.0 / 20.0;

/// Grass block id (the passive-spawn gate block, `BlockId(3)`).
const GRASS_BLOCK: u16 = 3;
/// Dirt block id - what a grazed grass block decays to.
const DIRT_BLOCK: u16 = 2;

/// True for blocks in the MC `EDIBLE_FOR_SHEEP` tag that exist in
/// our registry (short_grass; short_dry_grass / tall_dry_grass /
/// fern have no rows yet).
fn is_edible_plant(id: u16) -> bool {
    id == crate::block::SHORT_GRASS_ID
}

/// Host-side per-tick graze pass (caller = `tick_mobs_once`, so the
/// client role never gets here and `dt` is the fixed 20 Hz step).
pub(crate) fn graze_pass(voxel: &mut Voxel, dt: f32, rand_seed: &mut u32) {
    let mut rng = SpawnRng::new(rand_seed);
    for i in 0..voxel.mobs.len() {
        let m = voxel.mobs[i];
        if m.kind != KIND_SHEEP || m.dying > 0.0 {
            continue;
        }
        // PanicGoal preempts the eat (MC goal priority 1 vs 5).
        if m.panic_left > 0.0 {
            voxel.mobs[i].eat_left = 0.0;
            continue;
        }
        let feet = [
            m.pos[0].floor() as i32,
            m.pos[1].floor() as i32,
            m.pos[2].floor() as i32,
        ];
        if m.eat_left > 0.0 {
            let before = m.eat_left;
            let after = (before - dt).max(0.0);
            voxel.mobs[i].eat_left = after;
            // The bite: crossing the 4-ticks-left mark.
            if before > EAT_BITE_MARK_S && after <= EAT_BITE_MARK_S {
                let standing = voxel.world.get(feet).0;
                let below = [feet[0], feet[1] - 1, feet[2]];
                if is_edible_plant(standing) {
                    // destroyBlock(pos, false) - no drops.
                    eat_block_edit(voxel, feet, BlockId::AIR);
                    voxel.mobs[i].sheared = false;
                } else if voxel.world.get(below).0 == GRASS_BLOCK {
                    // levelEvent 2001 (break particles) + dirt.
                    eat_block_edit(voxel, below, BlockId(DIRT_BLOCK));
                    voxel.mobs[i].sheared = false;
                }
                // Neither block still present = no `ate()` - the MC
                // goal just runs out without regrowing the wool.
            }
            continue;
        }
        // canUse: rng 1/1000 per tick for adults (the baby 1/50
        // growth path is queued - no wool on a lamb), and the block
        // check that MC runs in the same call.
        if m.baby_left > 0.0 {
            continue;
        }
        if !rng.next_u32().is_multiple_of(1000) {
            continue;
        }
        let standing = voxel.world.get(feet).0;
        let below = [feet[0], feet[1] - 1, feet[2]];
        if is_edible_plant(standing) || voxel.world.get(below).0 == GRASS_BLOCK
        {
            let mm = &mut voxel.mobs[i];
            mm.eat_left = EAT_TIME_S;
            // EatBlockGoal.start: navigation stops for the bite.
            mm.strolling = false;
            mm.target = mm.pos;
        }
    }
}

/// Apply one graze block edit with the host-side funnel every other
/// mob-driven world edit uses: tracked set (light + emitter
/// bookkeeping) + the DEFERRED remesh queue (the W326 liquid-tick
/// pattern - headless-safe and off the 20 Hz budget) + a `vx.edit`
/// broadcast in the server role, plus the break particle puff
/// standing in for MC's levelEvent 2001.
fn eat_block_edit(voxel: &mut Voxel, cell: [i32; 3], to: BlockId) {
    voxel.world_set_tracked(cell, to);
    for coord in Voxel::affected_chunks(cell) {
        if voxel.world.chunk(coord).is_some()
            && voxel.pending_remesh_set.insert(coord)
        {
            voxel.pending_remesh.push_back(coord);
        }
    }
    voxel.spawn_break_particles(cell);
    let edit = crate::EditMsg {
        x: cell[0],
        y: cell[1],
        z: cell[2],
        block: to.0,
    };
    if let NetRole::Server { server, .. } = &voxel.net {
        let cfg = bincode::config::standard();
        if let Ok(buf) = bincode::serde::encode_to_vec(edit, cfg) {
            server.broadcast_bytes("vx.edit", buf);
        }
        voxel.edit_history.push(edit);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mob::state::MobState;
    use infengine_voxel::Chunk;

    /// The floor / feet cells sit at low Y so they land inside the
    /// origin chunk whatever CHUNK_SIZE is.
    const FLOOR: [i32; 3] = [4, 7, 4];
    const FEET: [i32; 3] = [4, 8, 4];

    fn sheep_on_grass() -> (Voxel, u32) {
        let mut v = Voxel::new();
        v.world.insert_chunk([0, 0, 0], Chunk::empty());
        v.world.set(FLOOR, BlockId(GRASS_BLOCK));
        let mut m = MobState::new_natural(
            1,
            KIND_SHEEP,
            [4.5, 8.0, 4.5],
            [4.5, 8.0, 4.5],
            0.0,
            0.0,
            8.0,
        );
        m.sheared = true;
        v.mobs.push(m);
        (v, 1)
    }

    /// W474: the full regrow cycle - eat timer runs 2 s, the bite
    /// turns the grass block below to dirt and clears `sheared`.
    #[test]
    fn graze_bite_eats_grass_and_clears_sheared() {
        let (mut v, _) = sheep_on_grass();
        v.mobs[0].eat_left = EAT_TIME_S;
        let mut seed = 7u32;
        // 2 s of 20 Hz ticks walks the timer through the bite mark.
        for _ in 0..41 {
            graze_pass(&mut v, 1.0 / 20.0, &mut seed);
        }
        assert_eq!(
            v.world.get(FLOOR),
            BlockId(DIRT_BLOCK),
            "grass block below the sheep decays to dirt"
        );
        assert!(!v.mobs[0].sheared, "ate() regrows the wool");
        assert!(v.mobs[0].eat_left <= 0.0);
    }

    /// The 1/1000 canUse gate eventually starts an eat on grass, and
    /// never starts one for a sheep floating over bare air.
    #[test]
    fn graze_gate_needs_grass_underfoot() {
        let (mut v, _) = sheep_on_grass();
        let mut seed = 0xBEEFu32;
        for _ in 0..20_000 {
            graze_pass(&mut v, 1.0 / 20.0, &mut seed);
            if v.mobs[0].eat_left > 0.0 {
                break;
            }
        }
        assert!(v.mobs[0].eat_left > 0.0, "eat starts within 20k ticks");

        let mut v2 = Voxel::new();
        v2.world.insert_chunk([0, 0, 0], Chunk::empty());
        let m = MobState::new_natural(
            2,
            KIND_SHEEP,
            [4.5, 8.0, 4.5],
            [4.5, 8.0, 4.5],
            0.0,
            0.0,
            8.0,
        );
        v2.mobs.push(m);
        let mut seed2 = 0xBEEFu32;
        for _ in 0..20_000 {
            graze_pass(&mut v2, 1.0 / 20.0, &mut seed2);
        }
        assert!(
            v2.mobs[0].eat_left <= 0.0,
            "no eat without grass or an edible plant"
        );
    }

    /// Standing IN short grass eats the plant itself (destroyBlock,
    /// -> air), not the block below.
    #[test]
    fn graze_bite_prefers_the_plant_cell() {
        let (mut v, _) = sheep_on_grass();
        v.world.set(FEET, BlockId(crate::block::SHORT_GRASS_ID));
        v.mobs[0].eat_left = EAT_TIME_S;
        let mut seed = 3u32;
        for _ in 0..41 {
            graze_pass(&mut v, 1.0 / 20.0, &mut seed);
        }
        assert!(v.world.get(FEET).is_air(), "short grass eaten");
        assert_eq!(
            v.world.get(FLOOR),
            BlockId(GRASS_BLOCK),
            "grass block below stays when the plant fed the bite"
        );
        assert!(!v.mobs[0].sheared);
    }
}
