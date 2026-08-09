//! W477 - which loot table a worldgen chest rolls.
//!
//! `ChestLootKind` + the membership chain moved here from
//! `workers.rs` (W249 origin) and grew the village tables, the
//! trial-chamber reward/barrel split, the monster-room / realistic
//! -dungeon positive probes and the honest `Unrouted` terminal.
//!
//! The old chain ended `else { SimpleDungeon }`, so every chest of
//! every structure without an explicit arm - all village houses
//! first among them - filled with monster-room dungeon loot. The
//! chain now ends in `Unrouted`: the chest mints EMPTY and logs
//! once, because an empty chest is honest while dungeon loot in a
//! village is not.
//!
//! Village identity comes from the chest cell's own block-entity
//! `LootTable` nbt (parsed into `StructureTemplate::loot_cells`), so
//! each house profession maps 1:1 onto its MC table - no biome
//! defaulting needed.

use super::LootTable;
use crate::worldgen::{WorldGen, WorldgenCtx, mcgen};

/// W249/W477: which loot table a worldgen chest rolls at delivery.
/// Decided on the worker via the deterministic structure-membership
/// probes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChestLootKind {
    SimpleDungeon,
    AbandonedMineshaft,
    DesertPyramid,
    IglooChest,
    PillagerOutpost,
    ShipwreckMap,
    ShipwreckTreasure,
    ShipwreckSupply,
    BuriedTreasure,
    UnderwaterRuinSmall,
    UnderwaterRuinBig,
    JungleTemple,
    StrongholdCorridor,
    StrongholdCrossing,
    StrongholdLibrary,
    RuinedPortal,
    WoodlandMansion,
    AncientCity,
    AncientCityIceBox,
    TrialChambersCorridor,
    TrialChambersEntrance,
    TrialChambersIntersection,
    TrialChambersIntersectionBarrel,
    TrialChambersReward,
    TrialChambersSupply,
    BastionTreasure,
    BastionOther,
    BastionBridge,
    BastionHoglinStable,
    NetherBridge,
    // W477: the 16 village tables, keyed by the chest cell's own
    // LootTable nbt.
    VillageArmorer,
    VillageButcher,
    VillageCartographer,
    VillageDesertHouse,
    VillageFisher,
    VillageFletcher,
    VillageMason,
    VillagePlainsHouse,
    VillageSavannaHouse,
    VillageShepherd,
    VillageSnowyHouse,
    VillageTaigaHouse,
    VillageTannery,
    VillageTemple,
    VillageToolsmith,
    VillageWeaponsmith,
    /// W477: no structure claimed the chest. Fills NOTHING (once
    /// -logged at delivery) so future waves wire the owner instead
    /// of players finding dungeon loot in it.
    Unrouted,
}

/// The generated table behind a kind; `None` = fill nothing
/// (`Unrouted`).
pub(crate) fn table_for(kind: ChestLootKind) -> Option<&'static LootTable> {
    use super::{tables_gen as t, tables_gen_trial as tc, tables_gen_village as v};
    Some(match kind {
        ChestLootKind::SimpleDungeon => &t::SIMPLE_DUNGEON,
        ChestLootKind::AbandonedMineshaft => &t::ABANDONED_MINESHAFT,
        ChestLootKind::DesertPyramid => &t::DESERT_PYRAMID,
        ChestLootKind::IglooChest => &t::IGLOO_CHEST,
        ChestLootKind::PillagerOutpost => &t::PILLAGER_OUTPOST,
        ChestLootKind::ShipwreckMap => &t::SHIPWRECK_MAP,
        ChestLootKind::ShipwreckTreasure => &t::SHIPWRECK_TREASURE,
        ChestLootKind::ShipwreckSupply => &t::SHIPWRECK_SUPPLY,
        ChestLootKind::BuriedTreasure => &t::BURIED_TREASURE,
        ChestLootKind::UnderwaterRuinSmall => &t::UNDERWATER_RUIN_SMALL,
        ChestLootKind::UnderwaterRuinBig => &t::UNDERWATER_RUIN_BIG,
        ChestLootKind::JungleTemple => &t::JUNGLE_TEMPLE,
        ChestLootKind::StrongholdCorridor => &t::STRONGHOLD_CORRIDOR,
        ChestLootKind::StrongholdCrossing => &t::STRONGHOLD_CROSSING,
        ChestLootKind::StrongholdLibrary => &t::STRONGHOLD_LIBRARY,
        ChestLootKind::RuinedPortal => &t::RUINED_PORTAL,
        ChestLootKind::WoodlandMansion => &t::WOODLAND_MANSION,
        ChestLootKind::AncientCity => &t::ANCIENT_CITY,
        ChestLootKind::AncientCityIceBox => &t::ANCIENT_CITY_ICE_BOX,
        ChestLootKind::TrialChambersCorridor => &tc::TRIAL_CHAMBERS_CORRIDOR,
        ChestLootKind::TrialChambersEntrance => &tc::TRIAL_CHAMBERS_ENTRANCE,
        ChestLootKind::TrialChambersIntersection => {
            &tc::TRIAL_CHAMBERS_INTERSECTION
        }
        ChestLootKind::TrialChambersIntersectionBarrel => {
            &tc::TRIAL_CHAMBERS_INTERSECTION_BARREL
        }
        ChestLootKind::TrialChambersReward => &tc::TRIAL_CHAMBERS_REWARD,
        ChestLootKind::TrialChambersSupply => &tc::TRIAL_CHAMBERS_SUPPLY,
        ChestLootKind::BastionTreasure => &t::BASTION_TREASURE,
        ChestLootKind::BastionOther => &t::BASTION_OTHER,
        ChestLootKind::BastionBridge => &t::BASTION_BRIDGE,
        ChestLootKind::BastionHoglinStable => &t::BASTION_HOGLIN_STABLE,
        ChestLootKind::NetherBridge => &t::NETHER_BRIDGE,
        ChestLootKind::VillageArmorer => &v::VILLAGE_VILLAGE_ARMORER,
        ChestLootKind::VillageButcher => &v::VILLAGE_VILLAGE_BUTCHER,
        ChestLootKind::VillageCartographer => &v::VILLAGE_VILLAGE_CARTOGRAPHER,
        ChestLootKind::VillageDesertHouse => &v::VILLAGE_VILLAGE_DESERT_HOUSE,
        ChestLootKind::VillageFisher => &v::VILLAGE_VILLAGE_FISHER,
        ChestLootKind::VillageFletcher => &v::VILLAGE_VILLAGE_FLETCHER,
        ChestLootKind::VillageMason => &v::VILLAGE_VILLAGE_MASON,
        ChestLootKind::VillagePlainsHouse => &v::VILLAGE_VILLAGE_PLAINS_HOUSE,
        ChestLootKind::VillageSavannaHouse => &v::VILLAGE_VILLAGE_SAVANNA_HOUSE,
        ChestLootKind::VillageShepherd => &v::VILLAGE_VILLAGE_SHEPHERD,
        ChestLootKind::VillageSnowyHouse => &v::VILLAGE_VILLAGE_SNOWY_HOUSE,
        ChestLootKind::VillageTaigaHouse => &v::VILLAGE_VILLAGE_TAIGA_HOUSE,
        ChestLootKind::VillageTannery => &v::VILLAGE_VILLAGE_TANNERY,
        ChestLootKind::VillageTemple => &v::VILLAGE_VILLAGE_TEMPLE,
        ChestLootKind::VillageToolsmith => &v::VILLAGE_VILLAGE_TOOLSMITH,
        ChestLootKind::VillageWeaponsmith => &v::VILLAGE_VILLAGE_WEAPONSMITH,
        ChestLootKind::Unrouted => return None,
    })
}

/// The 16 `chests/village/*` table ids -> kinds. `id` is the raw nbt
/// string (`minecraft:chests/village/village_armorer`).
pub(crate) fn village_kind_for_loot_id(id: &str) -> Option<ChestLootKind> {
    let stem = id
        .strip_prefix("minecraft:")
        .unwrap_or(id)
        .strip_prefix("chests/village/")?;
    Some(match stem {
        "village_armorer" => ChestLootKind::VillageArmorer,
        "village_butcher" => ChestLootKind::VillageButcher,
        "village_cartographer" => ChestLootKind::VillageCartographer,
        "village_desert_house" => ChestLootKind::VillageDesertHouse,
        "village_fisher" => ChestLootKind::VillageFisher,
        "village_fletcher" => ChestLootKind::VillageFletcher,
        "village_mason" => ChestLootKind::VillageMason,
        "village_plains_house" => ChestLootKind::VillagePlainsHouse,
        "village_savanna_house" => ChestLootKind::VillageSavannaHouse,
        "village_shepherd" => ChestLootKind::VillageShepherd,
        "village_snowy_house" => ChestLootKind::VillageSnowyHouse,
        "village_taiga_house" => ChestLootKind::VillageTaigaHouse,
        "village_tannery" => ChestLootKind::VillageTannery,
        "village_temple" => ChestLootKind::VillageTemple,
        "village_toolsmith" => ChestLootKind::VillageToolsmith,
        "village_weaponsmith" => ChestLootKind::VillageWeaponsmith,
        _ => return None,
    })
}

/// The membership chain behind [`ChestLootKind`]. Worker-side only:
/// the probes replay structure layouts through the per-thread
/// worldgen caches, which are warm right after generating the chunk.
pub(crate) fn chest_loot_kind(ctx: &WorldgenCtx, pos: [i32; 3]) -> ChestLootKind {
    if mcgen::mineshaft::pos_in_mineshaft(ctx, pos) {
        ChestLootKind::AbandonedMineshaft
    } else if mcgen::pyramid::pos_in_desert_pyramid(ctx, pos) {
        ChestLootKind::DesertPyramid
    } else if mcgen::igloo::pos_in_igloo(ctx, pos) {
        ChestLootKind::IglooChest
    } else if mcgen::outpost::pos_in_outpost(ctx, pos) {
        ChestLootKind::PillagerOutpost
    } else if let Some(k) = mcgen::shipwreck::chest_kind_at(ctx, pos) {
        match k {
            mcgen::shipwreck::ShipwreckChest::Map => ChestLootKind::ShipwreckMap,
            mcgen::shipwreck::ShipwreckChest::Treasure => {
                ChestLootKind::ShipwreckTreasure
            }
            mcgen::shipwreck::ShipwreckChest::Supply => {
                ChestLootKind::ShipwreckSupply
            }
        }
    } else if mcgen::buried_treasure::is_treasure_chest(ctx, pos) {
        ChestLootKind::BuriedTreasure
    } else if let Some(big) = mcgen::ocean_ruin::chest_is_big_at(ctx, pos) {
        if big {
            ChestLootKind::UnderwaterRuinBig
        } else {
            ChestLootKind::UnderwaterRuinSmall
        }
    } else if mcgen::jungle_temple::pos_in_jungle_temple(ctx, pos) {
        ChestLootKind::JungleTemple
    } else if let Some(k) = mcgen::stronghold::chest_kind_at(ctx, pos) {
        match k {
            mcgen::stronghold::StrongholdChest::Corridor => {
                ChestLootKind::StrongholdCorridor
            }
            mcgen::stronghold::StrongholdChest::Crossing => {
                ChestLootKind::StrongholdCrossing
            }
            mcgen::stronghold::StrongholdChest::Library => {
                ChestLootKind::StrongholdLibrary
            }
        }
    } else if mcgen::ruined_portal::pos_in_ruined_portal(ctx, pos) {
        ChestLootKind::RuinedPortal
    } else if mcgen::mansion::pos_in_mansion(ctx, pos) {
        ChestLootKind::WoodlandMansion
    } else if let Some(k) = mcgen::ancient_city::city_chest_at(ctx, pos) {
        match k {
            mcgen::ancient_city::CityChest::IceBox => {
                ChestLootKind::AncientCityIceBox
            }
            mcgen::ancient_city::CityChest::Regular => ChestLootKind::AncientCity,
        }
    } else if let Some(k) = mcgen::trial_chambers::chamber_chest_at(ctx, pos) {
        match k {
            mcgen::trial_chambers::ChamberChest::Corridor => {
                ChestLootKind::TrialChambersCorridor
            }
            mcgen::trial_chambers::ChamberChest::Entrance => {
                ChestLootKind::TrialChambersEntrance
            }
            mcgen::trial_chambers::ChamberChest::Intersection => {
                ChestLootKind::TrialChambersIntersection
            }
            mcgen::trial_chambers::ChamberChest::IntersectionBarrel => {
                ChestLootKind::TrialChambersIntersectionBarrel
            }
            mcgen::trial_chambers::ChamberChest::Reward => {
                ChestLootKind::TrialChambersReward
            }
            mcgen::trial_chambers::ChamberChest::Supply => {
                ChestLootKind::TrialChambersSupply
            }
        }
    } else if let Some(k) = mcgen::bastion::bastion_chest_at(ctx, pos) {
        match k {
            mcgen::bastion::BastionChest::Treasure => ChestLootKind::BastionTreasure,
            mcgen::bastion::BastionChest::HoglinStable => {
                ChestLootKind::BastionHoglinStable
            }
            mcgen::bastion::BastionChest::Bridge => ChestLootKind::BastionBridge,
            mcgen::bastion::BastionChest::Other => ChestLootKind::BastionOther,
        }
    } else if mcgen::fortress::is_fortress_chest(ctx, pos) {
        ChestLootKind::NetherBridge
    } else if matches!(ctx.profile, WorldGen::Mc)
        && mcgen::structures::pos_is_monster_room_chest(ctx, pos)
    {
        // W477: the monster room is now a POSITIVE probe (it used to
        // be the blanket else-arm that misfed every unwired chest).
        ChestLootKind::SimpleDungeon
    } else if matches!(ctx.profile, WorldGen::Realistic | WorldGen::Natural)
        && crate::worldgen::dungeons::pos_is_dungeon_chest(ctx, pos)
    {
        // The Realistic/Natural-profile dungeon rooms (W17.3) fill
        // with the same monster-room table they always did.
        ChestLootKind::SimpleDungeon
    } else if matches!(ctx.profile, WorldGen::Mc)
        && let Some(id) = mcgen::village::village_chest_loot_at(ctx, pos)
    {
        match village_kind_for_loot_id(&id) {
            Some(k) => k,
            None => {
                // Unknown village table id = template data drift;
                // empty beats a wrong guess.
                log_unrouted(pos, Some(&id));
                ChestLootKind::Unrouted
            }
        }
    } else {
        log_unrouted(pos, None);
        ChestLootKind::Unrouted
    }
}

/// Once-per-process log for unrouted chests (end cities / bonus
/// chests / dispenser-bearing pieces land in future waves; anything
/// else showing up here is a probe gap worth wiring).
fn log_unrouted(pos: [i32; 3], table_hint: Option<&str>) {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        tracing::info!(
            ?pos,
            table_hint,
            "W477: worldgen chest without a routed structure - minted \
             empty (first occurrence; later ones are silent)"
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loot::{tables_gen as t, tables_gen_village as v};

    /// Routing pins: a desert-pyramid kind fills the desert_pyramid
    /// table, village kinds fill their own tables (NEVER
    /// simple_dungeon), and `Unrouted` fills nothing.
    #[test]
    fn kinds_map_to_their_tables() {
        assert!(std::ptr::eq(
            table_for(ChestLootKind::DesertPyramid).unwrap(),
            &t::DESERT_PYRAMID
        ));
        assert!(std::ptr::eq(
            table_for(ChestLootKind::SimpleDungeon).unwrap(),
            &t::SIMPLE_DUNGEON
        ));
        assert!(table_for(ChestLootKind::Unrouted).is_none());
        let village_kinds = [
            ChestLootKind::VillageArmorer,
            ChestLootKind::VillageButcher,
            ChestLootKind::VillageCartographer,
            ChestLootKind::VillageDesertHouse,
            ChestLootKind::VillageFisher,
            ChestLootKind::VillageFletcher,
            ChestLootKind::VillageMason,
            ChestLootKind::VillagePlainsHouse,
            ChestLootKind::VillageSavannaHouse,
            ChestLootKind::VillageShepherd,
            ChestLootKind::VillageSnowyHouse,
            ChestLootKind::VillageTaigaHouse,
            ChestLootKind::VillageTannery,
            ChestLootKind::VillageTemple,
            ChestLootKind::VillageToolsmith,
            ChestLootKind::VillageWeaponsmith,
        ];
        for k in village_kinds {
            let table = table_for(k).expect("village kinds carry a table");
            assert!(
                !std::ptr::eq(table, &t::SIMPLE_DUNGEON),
                "{k:?} must never fill dungeon loot"
            );
        }
        assert!(std::ptr::eq(
            table_for(ChestLootKind::VillagePlainsHouse).unwrap(),
            &v::VILLAGE_VILLAGE_PLAINS_HOUSE
        ));
    }

    /// All 16 MC village table ids resolve to distinct kinds; junk
    /// ids resolve to none.
    #[test]
    fn village_ids_map_one_to_one() {
        let ids = [
            "village_armorer",
            "village_butcher",
            "village_cartographer",
            "village_desert_house",
            "village_fisher",
            "village_fletcher",
            "village_mason",
            "village_plains_house",
            "village_savanna_house",
            "village_shepherd",
            "village_snowy_house",
            "village_taiga_house",
            "village_tannery",
            "village_temple",
            "village_toolsmith",
            "village_weaponsmith",
        ];
        let mut kinds = Vec::new();
        for stem in ids {
            let id = format!("minecraft:chests/village/{stem}");
            let k = village_kind_for_loot_id(&id)
                .unwrap_or_else(|| panic!("{stem} must map"));
            assert!(!kinds.contains(&k), "{stem} collided");
            kinds.push(k);
        }
        assert_eq!(
            village_kind_for_loot_id("minecraft:chests/simple_dungeon"),
            None
        );
        assert_eq!(village_kind_for_loot_id("minecraft:chests/village/nope"), None);
    }

    /// W477: a real village layout's chest cell routes to its OWN
    /// per-building table through the full worker chain - never to
    /// simple_dungeon. (Grid-scans the same seed the other village
    /// tests pin; skips quietly if the scan finds no chest-bearing
    /// house.)
    #[test]
    fn village_chest_routes_to_its_building_table() {
        use crate::worldgen::mcgen::{jigsaw, village, village_data};
        let ctx = WorldgenCtx {
            base_seed: 3592732596,
            dim: "overworld".into(),
            profile: WorldGen::Mc,
        };
        let seed = ctx.effective_seed();
        for gx in 7..12 {
            for gz in 7..12 {
                let [sx, sz] = village::potential_start(seed, gx, gz);
                let Some(layout) = village::layout_for_start(&ctx, sx, sz)
                else {
                    continue;
                };
                for piece in layout.iter() {
                    // Rigid pieces only: their world Y is exact
                    // (drape pieces never carry chests anyway).
                    if piece.terrain_matching {
                        continue;
                    }
                    let Some(t) = village_data::template(&piece.template)
                    else {
                        continue;
                    };
                    let Some((cpos, lt)) = t.loot_cells.first() else {
                        continue;
                    };
                    let rp = jigsaw::rot_pos(piece.rotation, *cpos);
                    let pos = [
                        piece.position[0] + rp[0],
                        piece.position[1] + rp[1],
                        piece.position[2] + rp[2],
                    ];
                    let got = village::village_chest_loot_at(&ctx, pos)
                        .expect("probe finds the chest cell");
                    assert_eq!(&got, lt, "cell nbt id survives the probe");
                    let kind = chest_loot_kind(&ctx, pos);
                    assert_eq!(
                        Some(kind),
                        village_kind_for_loot_id(lt),
                        "full chain routes the {lt} chest"
                    );
                    assert_ne!(kind, ChestLootKind::SimpleDungeon);
                    assert_ne!(kind, ChestLootKind::Unrouted);
                    return;
                }
            }
        }
        // The pinned seed's 7..12 grid deterministically hosts
        // chest-carrying houses (verified in-wave) - reaching this
        // line means the layout solver or the zip data regressed.
        panic!("no chest-carrying village house in the pinned scan");
    }
}
