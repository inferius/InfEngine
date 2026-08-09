//! W479 - structure spawn overrides from MC data.
//!
//! MC structures may REPLACE a spawn category's weighted list inside
//! their bounds (`Structure.spawnOverrides` -> `StructureSpawnOverride
//! { boundingBox: FULL | PIECE, spawns }`; consumed by
//! `ChunkGenerator.getMobsAt`). W378-W380 hand-minted single mobs for
//! the outpost / monument / mansion instead - this module replaces
//! those arms with the real per-category tables read from the
//! structure jsons, pack-spawned through the W475 pack loop.
//!
//! Source of truth (data-driven mandate),
//! `F:\Data\mc_assets\mc_src\26.1.2\data\minecraft\worldgen\structure\`:
//! - `pillager_outpost.json`: monster -> pillager w1 (1-1), FULL box.
//! - `monument.json`: monster -> guardian w1 (2-4); axolotls +
//!   underground_water_creature -> EMPTY lists; FULL box. (The elder
//!   guardians are generation-time placements in MC, not natural
//!   spawns - the old 1-in-20 elder mint was a W380 improvisation.)
//! - `mansion.json`: `spawn_overrides: {}` - NO override at all; the
//!   biome pool applies inside. (MC mansions get their vindicators /
//!   evokers at generation time; the engine's mansion gen does not
//!   place them yet - noted for a follow-up wave.)
//! - `fortress.json`: monster -> blaze w10 (2-3) / zombified_piglin
//!   w5 (4-4) / wither_skeleton w8 (5-5) / skeleton w2 (5-5) /
//!   magma_cube w3 (4-4), PIECE boxes.
//!
//! Bounds granularity: the engine probes are `pos_in_outpost` /
//! `pos_in_monument` (structure-box membership = MC `bounding_box:
//! full`) and the fortress piece-list walk (each piece's Bb = MC
//! `bounding_box: piece`) - both match their json's declared box
//! kind. `pos_in_mansion` is no longer consulted for spawns (empty
//! overrides).

use crate::Voxel;

use super::kinds::{
    KIND_BLAZE, KIND_GUARDIAN, KIND_MAGMA_CUBE, KIND_PILLAGER,
    KIND_SKELETON, KIND_WITHER_SKELETON, KIND_ZOMBIFIED_PIGLIN,
};
use super::pool::{SpawnCategory, SpawnRow};

/// One structure's category replacements. `Some(rows)` REPLACES the
/// biome pool list for that category inside the structure's bounds
/// (an empty list = category silenced there); `None` = the biome
/// pool applies unchanged. Only the categories that appear in the
/// 26.1.2 jsons are modelled (`axolotls` has no engine category).
pub(crate) struct StructureSpawnOverrides {
    pub(crate) monster: Option<&'static [SpawnRow]>,
    pub(crate) underground_water_creature: Option<&'static [SpawnRow]>,
}

impl StructureSpawnOverrides {
    /// The replacement list for `cat`, if this structure declares one.
    pub(crate) fn rows_for(
        &self,
        cat: SpawnCategory,
    ) -> Option<&'static [SpawnRow]> {
        match cat {
            SpawnCategory::Monster => self.monster,
            SpawnCategory::UndergroundWaterCreature => {
                self.underground_water_creature
            }
            _ => None,
        }
    }
}

/// `pillager_outpost.json` spawn_overrides.
pub(crate) const OUTPOST: StructureSpawnOverrides = StructureSpawnOverrides {
    monster: Some(&[
        SpawnRow { kind: KIND_PILLAGER, weight: 1, min_count: 1, max_count: 1 },
    ]),
    underground_water_creature: None,
};

/// `monument.json` spawn_overrides (the empty underground-water list
/// silences glow squids inside the monument, exactly like MC).
pub(crate) const MONUMENT: StructureSpawnOverrides = StructureSpawnOverrides {
    monster: Some(&[
        SpawnRow { kind: KIND_GUARDIAN, weight: 1, min_count: 2, max_count: 4 },
    ]),
    underground_water_creature: Some(&[]),
};

/// `fortress.json` spawn_overrides - all five species have engine
/// kinds, so the table ports 1:1 (wither skeletons finally spawn).
pub(crate) const FORTRESS: StructureSpawnOverrides = StructureSpawnOverrides {
    monster: Some(&[
        SpawnRow { kind: KIND_BLAZE, weight: 10, min_count: 2, max_count: 3 },
        SpawnRow { kind: KIND_ZOMBIFIED_PIGLIN, weight: 5, min_count: 4, max_count: 4 },
        SpawnRow { kind: KIND_WITHER_SKELETON, weight: 8, min_count: 5, max_count: 5 },
        SpawnRow { kind: KIND_SKELETON, weight: 2, min_count: 5, max_count: 5 },
        SpawnRow { kind: KIND_MAGMA_CUBE, weight: 3, min_count: 4, max_count: 4 },
    ]),
    underground_water_creature: None,
};

/// Resolve the spawn overrides in effect at `wpos`, if any. Worker
/// probes replay the structure layouts through the per-thread
/// worldgen caches (`probe_budget` guards the cold-build cost on the
/// frame thread, W383).
pub(crate) fn overrides_at(
    voxel: &Voxel,
    wpos: [i32; 3],
) -> Option<&'static StructureSpawnOverrides> {
    let ctx = voxel.current_worldgen_ctx();
    match ctx.profile {
        crate::worldgen::WorldGen::Mc => {
            crate::worldgen::mcgen::probe_budget::mark_frame_thread();
            if crate::worldgen::mcgen::outpost::pos_in_outpost(&ctx, wpos) {
                return Some(&OUTPOST);
            }
            if crate::worldgen::mcgen::monument::pos_in_monument(&ctx, wpos) {
                return Some(&MONUMENT);
            }
            None
        }
        crate::worldgen::WorldGen::Nether => {
            crate::worldgen::mcgen::probe_budget::mark_frame_thread();
            if crate::worldgen::mcgen::fortress::pos_in_fortress(&ctx, wpos) {
                return Some(&FORTRESS);
            }
            None
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pin the override tables against the structure jsons.
    #[test]
    fn override_tables_match_structure_jsons() {
        // pillager_outpost.json: pillager w1 (1-1).
        assert_eq!(OUTPOST.monster.unwrap().len(), 1);
        let p = &OUTPOST.monster.unwrap()[0];
        assert_eq!(
            (p.kind, p.weight, p.min_count, p.max_count),
            (KIND_PILLAGER, 1, 1, 1)
        );
        assert!(OUTPOST.underground_water_creature.is_none());

        // monument.json: guardian w1 (2-4) + silenced glow squids.
        let g = &MONUMENT.monster.unwrap()[0];
        assert_eq!(
            (g.kind, g.weight, g.min_count, g.max_count),
            (KIND_GUARDIAN, 1, 2, 4)
        );
        assert_eq!(MONUMENT.underground_water_creature, Some(&[][..]));

        // fortress.json: the five-row table, weights + packs 1:1.
        let f = FORTRESS.monster.unwrap();
        assert_eq!(f.len(), 5);
        for (kind, w, mn, mx) in [
            (KIND_BLAZE, 10, 2, 3),
            (KIND_ZOMBIFIED_PIGLIN, 5, 4, 4),
            (KIND_WITHER_SKELETON, 8, 5, 5),
            (KIND_SKELETON, 2, 5, 5),
            (KIND_MAGMA_CUBE, 3, 4, 4),
        ] {
            let r = f.iter().find(|r| r.kind == kind).expect("row present");
            assert_eq!((r.weight, r.min_count, r.max_count), (w, mn, mx));
        }
    }

    /// `rows_for` dispatches only the declared categories.
    #[test]
    fn rows_for_dispatches_declared_categories() {
        assert!(OUTPOST.rows_for(SpawnCategory::Monster).is_some());
        assert!(OUTPOST.rows_for(SpawnCategory::Creature).is_none());
        assert!(OUTPOST.rows_for(SpawnCategory::Ambient).is_none());
        // Monument silences the underground water category (empty
        // replacement list), it does not fall back to the biome pool.
        assert_eq!(
            MONUMENT.rows_for(SpawnCategory::UndergroundWaterCreature),
            Some(&[][..])
        );
        assert!(
            FORTRESS
                .rows_for(SpawnCategory::UndergroundWaterCreature)
                .is_none()
        );
    }
}
