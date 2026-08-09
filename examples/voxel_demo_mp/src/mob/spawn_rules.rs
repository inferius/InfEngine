//! W475 - per-kind spawn placement rules (`SpawnPlacements.java`
//! ports), split out of `spawn.rs` so the pack/category machinery
//! and the per-species checks stay under the file-size budget.
//!
//! MC mirror:
//! - `net/minecraft/world/entity/SpawnPlacements.java` - the
//!   per-type predicate registry `validate_member` dispatches over
//!   (W479 adds the pillager block-light rule + the guardian water
//!   rule for the structure-override tables).
//! - `net/minecraft/world/entity/monster/Slime.java::checkSlimeSpawnRules`
//!   - swamp surface band + moon factor, slime-chunk y<40 branch.
//! - `net/minecraft/world/entity/ambient/Bat.java::checkBatSpawnRules`
//!   - below world surface + coin flip + light <= nextInt(4) +
//!     stone floor.
//! - `net/minecraft/world/entity/animal/squid/GlowSquid.java::checkGlowSquidSpawnRules`
//!   - y <= sea_level - 33 + raw brightness 0 + water.
//! - `data/minecraft/tags/block/*_spawnable_on.json` - the per-kind
//!   floor sets behind `creature_floor_ok`.

use infengine_voxel::CHUNK_SIZE;

use crate::{DAY_LENGTH_S, Voxel};

use super::kinds::{
    KIND_BAT, KIND_CAMEL, KIND_DROWNED, KIND_ELDER_GUARDIAN, KIND_FOX,
    KIND_GLOW_SQUID, KIND_GOAT, KIND_GUARDIAN, KIND_MOOSHROOM, KIND_OCELOT,
    KIND_PARROT, KIND_PILLAGER, KIND_POLAR_BEAR, KIND_RABBIT, KIND_SLIME,
    KIND_SQUID, KIND_TURTLE, KIND_WOLF,
};
use super::light::{
    block_light_at, effective_sky_light, has_solid_floor, has_standing_room,
    is_bright_enough_for_passive, is_dark_enough_for_hostile, sky_light_at,
};
use super::pool::SpawnCategory;
use super::spawn::{SpawnRng, is_slime_chunk};

/// Block id of the cell UNDER the spawn target.
fn floor_block(voxel: &Voxel, wpos: [i32; 3]) -> u16 {
    voxel.world.get([wpos[0], wpos[1] - 1, wpos[2]]).0
}

pub(crate) fn is_water_at(voxel: &Voxel, wpos: [i32; 3]) -> bool {
    voxel.world.get(wpos).0 == crate::block::WATER_ID
}

/// MC `getMaxLocalRawBrightness` approximation: the brighter of the
/// block-light layer and the day-factor-scaled sky layer.
fn local_brightness(voxel: &Voxel, wpos: [i32; 3]) -> u8 {
    let sky = effective_sky_light(voxel, wpos).round().clamp(0.0, 15.0) as u8;
    block_light_at(voxel, wpos).max(sky)
}

/// `DimensionType.MOON_BRIGHTNESS_PER_PHASE` - full moon (phase 0)
/// down to new moon (phase 4) and back.
const MOON_BRIGHTNESS_PER_PHASE: [f32; 8] =
    [1.0, 0.75, 0.5, 0.25, 0.0, 0.25, 0.5, 0.75];

/// Current moon brightness, derived from the same session-clock day
/// index the sky renderer uses for the moon disc
/// (`render/celestial.rs`), so spawn behaviour matches what the
/// player sees overhead.
fn moon_brightness(voxel: &Voxel) -> f32 {
    let secs = voxel
        .session_started
        .map(|s| s.elapsed().as_secs_f32())
        .unwrap_or(0.0);
    let day_idx = (secs / DAY_LENGTH_S) as u32;
    let phase = crate::sky::moon_phase_for_day(day_idx) as usize;
    MOON_BRIGHTNESS_PER_PHASE[phase % 8]
}

/// `Slime.checkSlimeSpawnRules` port, both branches:
/// - swamp surface: biome allows surface slimes AND y in (50, 70)
///   AND rand < moon_brightness * 0.5 (the `Timelines` keyframe
///   `SURFACE_SLIME_SPAWN_CHANCE`) AND local brightness <= rng(8);
/// - slime chunk: 1-in-10 AND the W464 deterministic chunk lattice
///   AND y < 40 (any light level).
pub(crate) fn slime_spawn_rules_ok(
    voxel: &Voxel,
    wpos: [i32; 3],
    in_swamp: bool,
    rng: &mut SpawnRng<'_>,
) -> bool {
    if in_swamp && wpos[1] > 50 && wpos[1] < 70 {
        let chance = moon_brightness(voxel) * 0.5;
        if rng.next_f32() < chance
            && local_brightness(voxel, wpos) <= (rng.next_u32() % 8) as u8
        {
            return true;
        }
    }
    if rng.next_u32().is_multiple_of(10) && wpos[1] < 40 {
        let seed = voxel.current_worldgen_ctx().effective_seed();
        let cs = CHUNK_SIZE as i32;
        if is_slime_chunk(
            seed,
            wpos[0].div_euclid(cs),
            wpos[2].div_euclid(cs),
        ) {
            return true;
        }
    }
    false
}

/// `Bat.checkBatSpawnRules` port: strictly below the column's world
/// surface (bats are cave dwellers - they spawn underground even at
/// noon), a coin flip, local brightness <= rng(4) and a
/// `#bats_spawnable_on` floor (`#base_stone_overworld`: stone /
/// deepslate / granite / diorite / andesite / tuff).
pub(crate) fn bat_spawn_rules_ok(
    voxel: &Voxel,
    wpos: [i32; 3],
    rng: &mut SpawnRng<'_>,
) -> bool {
    let Some(surface) =
        voxel.surface_y_below(wpos[0], wpos[2], crate::WORLD_MAX_Y)
    else {
        return false;
    };
    if wpos[1] >= surface {
        return false;
    }
    if rng.next_u32().is_multiple_of(2) {
        return false;
    }
    if local_brightness(voxel, wpos) > (rng.next_u32() % 4) as u8 {
        return false;
    }
    matches!(floor_block(voxel, wpos), 1 | 96 | 83 | 84 | 85 | 95)
}

/// `GlowSquid.checkGlowSquidSpawnRules` port: at or below
/// `sea_level - 33`, raw brightness 0 (both light layers dark) and
/// inside water.
pub(crate) fn glow_squid_rules_ok(voxel: &Voxel, wpos: [i32; 3]) -> bool {
    wpos[1] <= crate::worldgen::climate::SEA_LEVEL - 33
        && block_light_at(voxel, wpos) == 0
        && sky_light_at(voxel, wpos) == 0
        && is_water_at(voxel, wpos)
}

/// Per-kind spawnable-floor sets, from the MC block tag jsons
/// (`data/minecraft/tags/block/*_spawnable_on.json`). The default
/// creature rule is `#animals_spawnable_on` = grass only; the arms
/// list the species with wider tags. Engine block ids per
/// `block/registry.rs` (podzol + mycelium exist twice - 153/394 and
/// 152/395 - both accepted).
pub(crate) fn creature_floor_ok(kind: u8, floor: u16) -> bool {
    const GRASS: u16 = 3;
    match kind {
        // rabbits_spawnable_on: grass / snow / snow_block / sand.
        k if k == KIND_RABBIT => matches!(floor, GRASS | 15 | 752 | 4),
        // camels_spawnable_on: #sand (sand, red_sand).
        k if k == KIND_CAMEL => matches!(floor, 4 | 645),
        // Turtle: sand per `TurtleEggBlock.onSand` (y band checked
        // separately in `validate_member`).
        k if k == KIND_TURTLE => matches!(floor, 4 | 645),
        // foxes_spawnable_on: grass / snow / snow_block / podzol /
        // coarse_dirt.
        k if k == KIND_FOX => {
            matches!(floor, GRASS | 15 | 752 | 153 | 394 | 396)
        }
        // wolves_spawnable_on: grass / snow / snow_block /
        // coarse_dirt / podzol.
        k if k == KIND_WOLF => {
            matches!(floor, GRASS | 15 | 752 | 396 | 153 | 394)
        }
        // goats_spawnable_on: grass / stone / snow / snow_block /
        // packed_ice / gravel.
        k if k == KIND_GOAT => {
            matches!(floor, GRASS | 1 | 15 | 752 | 401 | 644)
        }
        // Polar bear: grass via the animal rule + the
        // polar_bears_spawnable_on_alternate ice tag + snow cover.
        k if k == KIND_POLAR_BEAR => matches!(floor, GRASS | 15 | 752 | 400),
        // mooshrooms_spawnable_on: mycelium.
        k if k == KIND_MOOSHROOM => matches!(floor, 152 | 395),
        // parrots_spawnable_on: grass / leaves / logs.
        k if k == KIND_PARROT => matches!(floor, GRASS | 6 | 90 | 5 | 88),
        // animals_spawnable_on: grass_block only (sheep, pig,
        // chicken, cow, horse, donkey, panda, trader, ...).
        _ => floor == GRASS,
    }
}

/// One spawn-pack member's placement validation - MC's
/// `isValidSpawnPostitionForType` + the per-kind
/// `SpawnPlacements.checkSpawnRules` dispatch.
pub(crate) fn validate_member(
    voxel: &Voxel,
    kind: u8,
    cat: SpawnCategory,
    wpos: [i32; 3],
    in_swamp: bool,
    ceiled: bool,
    rng: &mut SpawnRng<'_>,
) -> bool {
    // IN_WATER placements skip the solid-floor / standing-room
    // shape and require water at the spawn cell instead.
    if kind == KIND_SQUID {
        // W388 water gate. (MC's additional sea-level band for
        // surface water animals is deliberately NOT applied - the
        // engine probes the ocean floor, not the surface layer.)
        return is_water_at(voxel, wpos);
    }
    if kind == KIND_GLOW_SQUID {
        return glow_squid_rules_ok(voxel, wpos);
    }
    if kind == KIND_DROWNED {
        // SpawnPlacements: drowned is an IN_WATER monster.
        return is_water_at(voxel, wpos)
            && (ceiled || is_dark_enough_for_hostile(voxel, wpos, rng));
    }
    if kind == KIND_GUARDIAN || kind == KIND_ELDER_GUARDIAN {
        // W479 `Guardian.checkGuardianSpawnRules`: IN_WATER (water at
        // the cell AND below), plus `rand(20) == 0 ||
        // !canSeeSkyFromBelowWater` - NO darkness gate; guardians
        // hunt around the clock. Engine sky probe: raw sky light 15
        // means an unobstructed column (open water surface).
        return is_water_at(voxel, wpos)
            && is_water_at(voxel, [wpos[0], wpos[1] - 1, wpos[2]])
            && (rng.next_u32().is_multiple_of(20)
                || sky_light_at(voxel, wpos) < 15);
    }
    // ON_GROUND placements: solid floor + two air cells
    // (NaturalSpawner.isValidPositionForMob's obstruction check).
    if !has_solid_floor(voxel, wpos) || !has_standing_room(voxel, wpos) {
        return false;
    }
    if kind == KIND_SLIME && !ceiled {
        return slime_spawn_rules_ok(voxel, wpos, in_swamp, rng);
    }
    if kind == KIND_BAT {
        return bat_spawn_rules_ok(voxel, wpos, rng);
    }
    if kind == KIND_OCELOT {
        // Ocelot.checkOcelotSpawnRules: 2-in-3 pass roll + grass or
        // leaves floor, NO light gate (it sits in the jungle's
        // monster spawner list but is no monster).
        return !rng.next_u32().is_multiple_of(3)
            && matches!(floor_block(voxel, wpos), 3 | 6 | 90);
    }
    if kind == KIND_PILLAGER {
        // W479 `PatrollingMonster.checkPatrollingMonsterSpawnRules`:
        // block light <= 8, NO sky-darkness gate - the outpost yard
        // breeds pillagers in broad daylight, exactly like MC.
        return block_light_at(voxel, wpos) <= 8;
    }
    match cat {
        SpawnCategory::Monster => {
            // Nether monsters spawn at any light level in vanilla
            // (zombified piglins under full glowstone light; the MC
            // nether block-light limit is 15 = no check).
            ceiled || is_dark_enough_for_hostile(voxel, wpos, rng)
        }
        SpawnCategory::Creature => {
            if !is_bright_enough_for_passive(voxel, wpos) {
                return false;
            }
            if kind == KIND_TURTLE
                && wpos[1] >= crate::worldgen::climate::SEA_LEVEL + 4
            {
                // Turtle.checkTurtleSpawnRules: beaches only.
                return false;
            }
            creature_floor_ok(kind, floor_block(voxel, wpos))
        }
        // Ambient = bat (handled above); water categories carry
        // their own kind rules above too. Anything new lands here
        // permissive-by-default until it gets a rule.
        _ => true,
    }
}

/// Test-world stamping helpers shared by the rule tests below and
/// the pack-machinery tests in `spawn.rs`.
#[cfg(test)]
pub(crate) mod teststamp {
    use crate::Voxel;
    use infengine_voxel::{BlockId, CHUNK_SIZE, Chunk, ChunkCoord};

    pub(crate) fn stamp_light(v: &mut Voxel, w: [i32; 3], block: u8, sky: u8) {
        let cs = CHUNK_SIZE as i32;
        let cc: ChunkCoord = [
            w[0].div_euclid(cs),
            w[1].div_euclid(cs),
            w[2].div_euclid(cs),
        ];
        let lc = [
            w[0].rem_euclid(cs) as u32,
            w[1].rem_euclid(cs) as u32,
            w[2].rem_euclid(cs) as u32,
        ];
        let g = v.lighting.ensure(cc);
        g.set_block(lc, block);
        g.set_sky(lc, sky);
    }

    /// Place `id` at `w`, minting the owning chunk when missing
    /// (`World::set` is a no-op for unloaded chunks).
    pub(crate) fn stamp_block(v: &mut Voxel, w: [i32; 3], id: u16) {
        let cs = CHUNK_SIZE as i32;
        let cc = [
            w[0].div_euclid(cs),
            w[1].div_euclid(cs),
            w[2].div_euclid(cs),
        ];
        if v.world.chunk(cc).is_none() {
            v.world.insert_chunk(cc, Chunk::empty());
        }
        v.world.set(w, BlockId(id));
    }

    /// Solid platform of `id` under a WxW area centred on `(cx, cz)`
    /// at floor height `fy`, with the spawn layer + headroom lit to
    /// `sky`.
    pub(crate) fn stamp_platform(
        v: &mut Voxel,
        cx: i32,
        cz: i32,
        fy: i32,
        half: i32,
        id: u16,
        sky: u8,
    ) {
        for x in (cx - half)..=(cx + half) {
            for z in (cz - half)..=(cz + half) {
                stamp_block(v, [x, fy, z], id);
                stamp_light(v, [x, fy + 1, z], 0, sky);
                stamp_light(v, [x, fy + 2, z], 0, sky);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::kinds::{KIND_SHEEP, KIND_ZOMBIE};
    use super::super::spawn::is_slime_chunk;
    use super::teststamp::{stamp_block, stamp_light};
    use super::*;
    use crate::Voxel;

    #[test]
    fn monster_member_needs_darkness() {
        // Dark cell + solid floor: a zombie member validates at
        // midnight; a torch-lit cell (block_light 14) rejects.
        let mut v = Voxel::new();
        v.day_time = crate::DAY_LENGTH_S * 0.75; // midnight
        stamp_block(&mut v, [10, 59, 10], 1);
        stamp_light(&mut v, [10, 60, 10], 0, 0);
        stamp_light(&mut v, [10, 61, 10], 0, 0);
        let mut seed = 0x12345u32;
        let mut rng = SpawnRng::new(&mut seed);
        assert!(validate_member(
            &v,
            KIND_ZOMBIE,
            SpawnCategory::Monster,
            [10, 60, 10],
            false,
            false,
            &mut rng
        ));
        stamp_light(&mut v, [10, 60, 10], 14, 0);
        assert!(!validate_member(
            &v,
            KIND_ZOMBIE,
            SpawnCategory::Monster,
            [10, 60, 10],
            false,
            false,
            &mut rng
        ));
    }

    #[test]
    fn creature_member_needs_daylight_and_matching_floor() {
        // Sheep: grass floor + sky >= 9 passes; stone floor fails;
        // dim light fails. Desert kinds: rabbit + camel accept sand.
        let mut v = Voxel::new();
        v.day_time = 0.0; // noon
        stamp_block(&mut v, [20, 59, 20], 3);
        stamp_light(&mut v, [20, 60, 20], 0, 15);
        stamp_light(&mut v, [20, 61, 20], 0, 15);
        let mut seed = 0xC0FFEEu32;
        let mut rng = SpawnRng::new(&mut seed);
        let ok = |v: &Voxel, k: u8, rng: &mut SpawnRng<'_>| {
            validate_member(
                v,
                k,
                SpawnCategory::Creature,
                [20, 60, 20],
                false,
                false,
                rng,
            )
        };
        assert!(ok(&v, KIND_SHEEP, &mut rng), "sheep on lit grass");
        stamp_block(&mut v, [20, 59, 20], 1);
        assert!(!ok(&v, KIND_SHEEP, &mut rng), "sheep on stone");
        stamp_block(&mut v, [20, 59, 20], 4);
        assert!(!ok(&v, KIND_SHEEP, &mut rng), "sheep on sand");
        assert!(ok(&v, KIND_RABBIT, &mut rng), "rabbit on sand");
        assert!(ok(&v, KIND_CAMEL, &mut rng), "camel on sand");
        // Dim cell kills the passive gate regardless of floor.
        stamp_block(&mut v, [20, 59, 20], 3);
        stamp_light(&mut v, [20, 60, 20], 0, 8);
        assert!(!ok(&v, KIND_SHEEP, &mut rng), "sheep in the dim");
    }

    #[test]
    fn turtle_needs_sand_near_sea_level() {
        use crate::worldgen::climate::SEA_LEVEL;
        let mut v = Voxel::new();
        v.day_time = 0.0;
        let beach_y = SEA_LEVEL + 1;
        stamp_block(&mut v, [5, beach_y - 1, 5], 4);
        stamp_light(&mut v, [5, beach_y, 5], 0, 15);
        stamp_light(&mut v, [5, beach_y + 1, 5], 0, 15);
        let mut seed = 7u32;
        let mut rng = SpawnRng::new(&mut seed);
        assert!(validate_member(
            &v,
            KIND_TURTLE,
            SpawnCategory::Creature,
            [5, beach_y, 5],
            false,
            false,
            &mut rng
        ));
        // Same sand floor high above the sea: rejected (MC's
        // y < sea_level + 4 band).
        let high_y = SEA_LEVEL + 20;
        stamp_block(&mut v, [7, high_y - 1, 7], 4);
        stamp_light(&mut v, [7, high_y, 7], 0, 15);
        stamp_light(&mut v, [7, high_y + 1, 7], 0, 15);
        assert!(!validate_member(
            &v,
            KIND_TURTLE,
            SpawnCategory::Creature,
            [7, high_y, 7],
            false,
            false,
            &mut rng
        ));
    }

    #[test]
    fn bat_spawns_underground_in_dark_not_on_the_surface_at_noon() {
        let mut v = Voxel::new();
        v.day_time = 0.0; // noon
        // Column: surface slab at y=80, cave floor (stone) at y=40
        // with a dark 2-cell pocket above it.
        stamp_block(&mut v, [30, 80, 30], 3);
        stamp_light(&mut v, [30, 81, 30], 0, 15);
        stamp_block(&mut v, [30, 40, 30], 1);
        stamp_light(&mut v, [30, 41, 30], 0, 0);
        stamp_light(&mut v, [30, 42, 30], 0, 0);
        // Surface cell at noon: never (y >= surface).
        let mut any_surface = false;
        for s in 1..64u32 {
            let mut seed = s;
            let mut rng = SpawnRng::new(&mut seed);
            any_surface |= bat_spawn_rules_ok(&v, [30, 81, 30], &mut rng);
        }
        assert!(!any_surface, "bat took the surface at noon");
        // Dark cave cell: the coin flip + light roll pass for some
        // seeds.
        let mut any_cave = false;
        for s in 1..64u32 {
            let mut seed = s;
            let mut rng = SpawnRng::new(&mut seed);
            any_cave |= bat_spawn_rules_ok(&v, [30, 41, 30], &mut rng);
        }
        assert!(any_cave, "bat never took the dark cave");
        // Grass cave floor: rejected (bats_spawnable_on is the
        // stone family).
        stamp_block(&mut v, [30, 40, 30], 3);
        let mut any_grass = false;
        for s in 1..64u32 {
            let mut seed = s;
            let mut rng = SpawnRng::new(&mut seed);
            any_grass |= bat_spawn_rules_ok(&v, [30, 41, 30], &mut rng);
        }
        assert!(!any_grass, "bat accepted a grass floor");
    }

    #[test]
    fn slime_rules_swamp_band_vs_slime_chunk_vs_neither() {
        use infengine_voxel::CHUNK_SIZE;
        let mut v = Voxel::new();
        v.day_time = crate::DAY_LENGTH_S * 0.75; // midnight
        // Fresh Voxel has no session clock -> day 0 -> full moon
        // (chance 0.5).
        // Swamp surface inside the (50, 70) band, dark.
        stamp_light(&mut v, [3, 60, 3], 0, 0);
        let mut any_swamp = false;
        for s in 1..64u32 {
            let mut seed = s;
            let mut rng = SpawnRng::new(&mut seed);
            any_swamp |= slime_spawn_rules_ok(&v, [3, 60, 3], true, &mut rng);
        }
        assert!(any_swamp, "swamp band + full moon never spawned");
        // Same spot OUTSIDE the swamp bucket: only the (rare)
        // slime-chunk branch could fire, and y=60 >= 40 kills it.
        for s in 1..64u32 {
            let mut seed = s;
            let mut rng = SpawnRng::new(&mut seed);
            assert!(!slime_spawn_rules_ok(&v, [3, 60, 3], false, &mut rng));
        }
        // Above the band: swamp branch dead too.
        stamp_light(&mut v, [3, 75, 3], 0, 0);
        for s in 1..64u32 {
            let mut seed = s;
            let mut rng = SpawnRng::new(&mut seed);
            assert!(!slime_spawn_rules_ok(&v, [3, 75, 3], true, &mut rng));
        }
        // Slime-chunk branch: find a slime chunk for the live seed,
        // probe y=20 inside it (any light).
        let seed_w = v.current_worldgen_ctx().effective_seed();
        let cs = CHUNK_SIZE as i32;
        let slime_c = (0..400)
            .map(|i| (i % 20, i / 20))
            .find(|&(cx, cz)| is_slime_chunk(seed_w, cx, cz))
            .expect("no slime chunk in 20x20");
        let not_slime_c = (0..400)
            .map(|i| (i % 20, i / 20))
            .find(|&(cx, cz)| !is_slime_chunk(seed_w, cx, cz))
            .unwrap();
        let deep = [slime_c.0 * cs + 4, 20, slime_c.1 * cs + 4];
        stamp_light(&mut v, deep, 15, 0); // fully lit - still fine
        let mut any_chunk = false;
        for s in 1..512u32 {
            let mut seed = s;
            let mut rng = SpawnRng::new(&mut seed);
            any_chunk |= slime_spawn_rules_ok(&v, deep, false, &mut rng);
        }
        assert!(any_chunk, "slime chunk at y=20 never spawned");
        let deep_no = [not_slime_c.0 * cs + 4, 20, not_slime_c.1 * cs + 4];
        for s in 1..512u32 {
            let mut seed = s;
            let mut rng = SpawnRng::new(&mut seed);
            assert!(!slime_spawn_rules_ok(&v, deep_no, false, &mut rng));
        }
    }

    #[test]
    fn glow_squid_needs_deep_dark_water() {
        use crate::worldgen::climate::SEA_LEVEL;
        let mut v = Voxel::new();
        let deep = [9, SEA_LEVEL - 40, 9];
        stamp_block(&mut v, deep, crate::block::WATER_ID);
        stamp_light(&mut v, deep, 0, 0);
        assert!(glow_squid_rules_ok(&v, deep));
        // Same water, lit -> rejected.
        stamp_light(&mut v, deep, 3, 0);
        assert!(!glow_squid_rules_ok(&v, deep));
        // Dark water but too shallow -> rejected.
        let shallow = [9, SEA_LEVEL - 5, 9];
        stamp_block(&mut v, shallow, crate::block::WATER_ID);
        stamp_light(&mut v, shallow, 0, 0);
        assert!(!glow_squid_rules_ok(&v, shallow));
    }
}
