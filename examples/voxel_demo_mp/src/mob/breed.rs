//! W66.2 - animal breeding.
//!
//! Right-clicking a farm animal with breeding food puts it into love
//! mode; two in-love animals of the same kind within range produce
//! offspring and enter a breed cooldown. This is a scoped version of
//! MC's `Animal` breeding: v1 spawns an adult (no baby growth stage)
//! and uses wheat as the universal food. Baby rendering + per-species
//! food (seeds / carrots) are a follow-up.
//!
//! MC mirror:
//! - `net/minecraft/world/entity/animal/Animal.java::setInLove` -
//!   love timer set on feeding.
//! - `Animal::spawnChildFromBreeding` - pairs two in-love parents,
//!   spawns the child, applies the 6000-tick `ageBoundaryReached`
//!   cooldown.

use crate::net::{MobMsg, NetRole};
use crate::Voxel;

use super::kinds::{KIND_CHICKEN, KIND_COW, KIND_PIG, KIND_RABBIT, KIND_SHEEP};
use super::spawn::initial_health_for;
use super::state::MobState;

/// Seconds a fed animal stays in love, looking for a mate (MC 600
/// ticks).
const LOVE_TIME: f32 = 30.0;
/// W290: seconds before a bred animal can be fed again - MC's 6000
/// tick `setAge(6000)` parent cooldown (was 60 s).
const BREED_COOLDOWN: f32 = 300.0;
/// Horizontal distance (blocks) two in-love animals must be within
/// to pair off.
const BREED_RANGE: f32 = 3.0;

/// W290: MC per-species food (`Animal.isFood`): cow + sheep eat
/// wheat, pigs carrot/potato/beetroot, chickens seeds, rabbits
/// carrot/dandelion (no golden carrot item yet). The same table
/// drives BOTH feeding (love mode) and the TemptGoal follow.
pub(crate) fn is_food_for(kind: u8, item_id: u16) -> bool {
    // W293: harvesting a crop drops the BLOCK-FORM item (wheat 493,
    // carrots 494, potatoes 495, beetroots 496), not the plain item
    // row - the user held 493 and no cow followed. Both id families
    // count as the species food.
    let wheat = item_id == 337 || item_id == 493;
    let carrot = item_id == 344 || item_id == 494;
    let potato = item_id == 343 || item_id == 495;
    let beetroot = item_id == 345 || item_id == 496;
    match kind {
        k if k == KIND_COW || k == KIND_SHEEP => wheat,
        k if k == KIND_PIG => carrot || potato || beetroot,
        k if k == KIND_CHICKEN => item_id == 338,
        k if k == KIND_RABBIT => carrot || item_id == 480,
        _ => false,
    }
}

/// Any item at least one species eats - the cheap pre-filter the
/// RMB path runs before the per-kind check.
pub(crate) fn is_breeding_food(item_id: u16) -> bool {
    matches!(item_id, 337 | 338 | 343 | 344 | 345 | 480 | 493 | 494 | 495 | 496)
}

/// W290: MC TemptGoal speed modifier per kind (Cow 1.25, Sheep 1.1,
/// Pig 1.2, Chicken 1.0, Rabbit 1.0).
pub(crate) fn tempt_speed(kind: u8) -> f32 {
    match kind {
        k if k == KIND_COW => 1.25,
        k if k == KIND_SHEEP => 1.1,
        k if k == KIND_PIG => 1.2,
        _ => 1.0,
    }
}

/// Kinds that can breed (passive farm animals).
fn can_breed(kind: u8) -> bool {
    matches!(
        kind,
        KIND_SHEEP | KIND_COW | KIND_PIG | KIND_CHICKEN | KIND_RABBIT
    )
}

/// Try to feed the mob `mob_id`. Returns `true` (the caller then
/// consumes one food) when it's a breedable, alive animal that's off
/// cooldown and not already in love; sets its love timer + a heart
/// particle puff. Returns `false` otherwise (no food consumed).
pub(crate) fn try_feed(voxel: &mut Voxel, mob_id: u32, item_id: u16) -> bool {
    let pos;
    {
        let Some(m) = voxel.mobs.iter_mut().find(|m| m.id == mob_id) else {
            return false;
        };
        // W290: the species must actually EAT this item (MC isFood).
        if !can_breed(m.kind)
            || !is_food_for(m.kind, item_id)
            || m.baby_left > 0.0
            || m.dying > 0.0
            || m.breed_cooldown > 0.0
            || m.love > 0.0
        {
            return false;
        }
        m.love = LOVE_TIME;
        pos = m.pos;
    }
    // Heart-ish feedback at the animal's body height.
    let cell = [
        pos[0].floor() as i32,
        (pos[1] + 0.8).floor() as i32,
        pos[2].floor() as i32,
    ];
    voxel.spawn_place_particles(cell, [0.95, 0.35, 0.5]);
    true
}

/// Per-tick breeding pass (host-only; `tick_mobs` already returns
/// early on clients before calling this). Decays the love / cooldown
/// timers, then pairs up in-love same-kind animals within range to
/// spawn offspring.
pub(crate) fn breed_pass(voxel: &mut Voxel, dt: f32) {
    if matches!(voxel.net, NetRole::Client { .. }) {
        return;
    }
    for m in voxel.mobs.iter_mut() {
        m.love = (m.love - dt).max(0.0);
        m.breed_cooldown = (m.breed_cooldown - dt).max(0.0);
        // W291: babies grow toward adulthood (MC -24000 age ticks).
        m.baby_left = (m.baby_left - dt).max(0.0);
    }
    // W295: MC in-love HEART particles - vanilla animals shed hearts
    // for the whole love window, not only at feed time (the user
    // bred cows and saw nothing above them). ~2 puffs/s per animal,
    // staggered by id off the shared 20 Hz tick counter.
    let hearts: Vec<[f32; 3]> = voxel
        .mobs
        .iter()
        .filter(|m| {
            m.love > 0.0
                && m.dying <= 0.0
                && voxel.net_tick % 10 == (m.id as u64) % 10
        })
        .map(|m| m.pos)
        .collect();
    for p in hearts {
        let cell = [
            p[0].floor() as i32,
            (p[1] + 1.2).floor() as i32,
            p[2].floor() as i32,
        ];
        voxel.spawn_place_particles(cell, [0.95, 0.35, 0.5]);
    }
    // Snapshot the in-love subset (index, kind, pos) so the pairing
    // search doesn't hold a borrow across the spawn.
    let in_love: Vec<(usize, u8, [f32; 3])> = voxel
        .mobs
        .iter()
        .enumerate()
        .filter(|(_, m)| m.love > 0.0 && m.dying <= 0.0)
        .map(|(i, m)| (i, m.kind, m.pos))
        .collect();
    let pairs = pair_breeders(&in_love);
    for (ia, ib, mid, kind) in pairs {
        voxel.mobs[ia].love = 0.0;
        voxel.mobs[ia].breed_cooldown = BREED_COOLDOWN;
        voxel.mobs[ib].love = 0.0;
        voxel.mobs[ib].breed_cooldown = BREED_COOLDOWN;
        spawn_offspring(voxel, kind, mid);
    }
}

/// Greedily pair in-love animals: each animal joins at most one pair,
/// matched to the first same-kind partner within `BREED_RANGE`.
/// Returns `(index_a, index_b, midpoint, kind)` tuples. Pure over the
/// snapshot so it's unit-testable.
fn pair_breeders(
    in_love: &[(usize, u8, [f32; 3])],
) -> Vec<(usize, usize, [f32; 3], u8)> {
    let mut used = vec![false; in_love.len()];
    let mut out = Vec::new();
    for a in 0..in_love.len() {
        if used[a] {
            continue;
        }
        for b in (a + 1)..in_love.len() {
            if used[b] || in_love[a].1 != in_love[b].1 {
                continue;
            }
            let pa = in_love[a].2;
            let pb = in_love[b].2;
            let d2 = (pa[0] - pb[0]).powi(2) + (pa[2] - pb[2]).powi(2);
            if d2 <= BREED_RANGE * BREED_RANGE {
                let mid = [(pa[0] + pb[0]) * 0.5, pa[1], (pa[2] + pb[2]) * 0.5];
                out.push((in_love[a].0, in_love[b].0, mid, in_love[a].1));
                used[a] = true;
                used[b] = true;
                break;
            }
        }
    }
    out
}

/// W291: seconds a newborn spends as a baby - MC's -24000 age ticks
/// (20 minutes).
pub(crate) const BABY_GROW_TIME: f32 = 1200.0;

/// Spawn a BABY of `kind` at `pos`, flagged persistent so the bred
/// animal doesn't despawn. Renders at half scale + follows a parent
/// until `baby_left` runs out (MC AgeableMob). The pose broadcasts
/// on the next `tick_mobs` broadcast pass.
fn spawn_offspring(voxel: &mut Voxel, kind: u8, pos: [f32; 3]) {
    let id = voxel.next_mob_id;
    voxel.next_mob_id = voxel.next_mob_id.wrapping_add(1).max(1);
    let hp = initial_health_for(kind);
    let mut mob = MobState::new_natural(id, kind, pos, pos, 0.0, 0.0, hp);
    mob.persistent = true;
    mob.baby_left = BABY_GROW_TIME;
    voxel.mobs.push(mob);
    voxel.remote_mobs.insert(
        id,
        MobMsg {
            id,
            kind,
            pos,
            yaw: 0.0,
            health: hp as u8,
            head_yaw: 0.0,
            head_pitch: 0.0,
            baby: true,
            size: 1,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn per_species_food_matches_mc() {
        // W290 pins: Animal.isFood per species.
        assert!(is_food_for(KIND_COW, 337)); // wheat
        assert!(is_food_for(KIND_SHEEP, 337));
        assert!(!is_food_for(KIND_COW, 344)); // no carrots for cows
        assert!(is_food_for(KIND_PIG, 344)); // carrot
        assert!(is_food_for(KIND_PIG, 343)); // potato
        assert!(is_food_for(KIND_PIG, 345)); // beetroot
        assert!(!is_food_for(KIND_PIG, 337)); // pigs refuse wheat
        assert!(is_food_for(KIND_CHICKEN, 338)); // seeds
        assert!(!is_food_for(KIND_CHICKEN, 337));
        assert!(is_food_for(KIND_RABBIT, 344)); // carrot
        assert!(is_food_for(KIND_RABBIT, 480)); // dandelion
        assert!(is_breeding_food(337));
        assert!(!is_breeding_food(1)); // stone
        assert!((tempt_speed(KIND_COW) - 1.25).abs() < 1e-6);
        assert!((tempt_speed(KIND_CHICKEN) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn only_farm_animals_breed() {
        assert!(can_breed(KIND_SHEEP));
        assert!(can_breed(KIND_COW));
        assert!(can_breed(KIND_PIG));
        assert!(can_breed(KIND_CHICKEN));
        assert!(!can_breed(super::super::kinds::KIND_ZOMBIE));
    }

    #[test]
    fn pairs_two_in_range_same_kind() {
        let in_love = vec![
            (0usize, KIND_COW, [0.0, 64.0, 0.0]),
            (1usize, KIND_COW, [1.0, 64.0, 0.0]),
        ];
        let pairs = pair_breeders(&in_love);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].0, 0);
        assert_eq!(pairs[0].1, 1);
        assert_eq!(pairs[0].3, KIND_COW);
    }

    #[test]
    fn does_not_pair_different_kinds_or_out_of_range() {
        let mixed = vec![
            (0usize, KIND_COW, [0.0, 64.0, 0.0]),
            (1usize, KIND_PIG, [1.0, 64.0, 0.0]),
        ];
        assert!(pair_breeders(&mixed).is_empty());
        let far = vec![
            (0usize, KIND_COW, [0.0, 64.0, 0.0]),
            (1usize, KIND_COW, [50.0, 64.0, 0.0]),
        ];
        assert!(pair_breeders(&far).is_empty());
    }

    #[test]
    fn each_animal_joins_one_pair_only() {
        // Three cows clustered: exactly one pair forms, the third is
        // left unpaired this tick.
        let trio = vec![
            (0usize, KIND_COW, [0.0, 64.0, 0.0]),
            (1usize, KIND_COW, [1.0, 64.0, 0.0]),
            (2usize, KIND_COW, [0.5, 64.0, 1.0]),
        ];
        assert_eq!(pair_breeders(&trio).len(), 1);
    }
}
