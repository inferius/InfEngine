//! W319 - villager trades, generated 1:1 from the MC 26.1.2 data
//! JSONs (`data/minecraft/villager_trade/<profession>/<tier>/*.json`).
//! Tiers 1-2 (novice + apprentice); entries with
//! `given_item_modifiers` (enchanted books, maps, dyed armor) or
//! items missing from the registry are skipped for now. Each row
//! cites its source path.
//!
//! Runtime: a villager's profession derives from a stable hash of
//! its mob id (no save-schema change); `item_matches` accepts the
//! crop block-form ids the harvest drops next to the canonical item
//! ids (the W293 family aliases), so wheat from a farm pays a
//! farmer's wheat trade.

/// One trade offer: `want` (+ optional `want2`) buys `give`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TradeDef {
    pub(crate) profession: &'static str,
    /// Reserved for the leveling UI (tier gating is a follow-up).
    #[allow(dead_code)]
    pub(crate) tier: u8,
    pub(crate) want: u16,
    pub(crate) want_n: u8,
    pub(crate) want2: Option<(u16, u8)>,
    pub(crate) give: u16,
    pub(crate) give_n: u8,
    /// W458: MC max_uses (offer exhaustion + workstation restock
    /// are a follow-up wave; carried so the data port is complete).
    #[allow(dead_code)]
    pub(crate) max_uses: u8,
    /// W458: villager XP granted per completed trade.
    pub(crate) xp: u8,
}

// W458: the FULL tier 1-5 table lives in `trades_table.rs`
// (generated from the MC data JSONs); re-exported so call sites
// keep their import path.
pub(crate) use crate::trades_table::TRADES;

/// W458: MC VillagerData level thresholds (Novice..Master):
/// 0 / 10 / 70 / 150 / 250 accumulated trade XP.
const LEVEL_XP: [u32; 5] = [0, 10, 70, 150, 250];

/// Villager trade level (1..=5) for accumulated XP.
pub(crate) fn level_for_xp(xp: u32) -> u8 {
    let mut level = 1u8;
    for (i, min) in LEVEL_XP.iter().enumerate().skip(1) {
        if xp >= *min {
            level = i as u8 + 1;
        }
    }
    level
}

/// MC level names for the trading UI title.
pub(crate) fn level_name(level: u8) -> &'static str {
    match level {
        1 => "Novice",
        2 => "Apprentice",
        3 => "Journeyman",
        4 => "Expert",
        _ => "Master",
    }
}

/// Professions with at least one usable trade row.
pub(crate) const PROFESSIONS: &[&str] = &[
    "armorer",
    "butcher",
    "cartographer",
    "cleric",
    "farmer",
    "fisherman",
    "fletcher",
    "leatherworker",
    "librarian",
    "mason",
    "shepherd",
    "smith",
    "toolsmith",
    "weaponsmith",
];

/// Stable profession pick per villager (hash of the mob id - no
/// save-schema change; the same villager keeps its trade page).
pub(crate) fn profession_for(mob_id: u32) -> &'static str {
    let h = (mob_id as u64)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .rotate_left(17);
    PROFESSIONS[(h % PROFESSIONS.len() as u64) as usize]
}

/// W341: MC PoiTypes -> VillagerProfession. A villager claims the
/// job of the nearest workstation block; the table is the vanilla
/// acquirable-job-site list.
const WORKSTATIONS: &[(u16, &str)] = &[
    (562, "armorer"),       // blast_furnace
    (561, "butcher"),       // smoker
    (559, "cartographer"),  // cartography_table
    (566, "cleric"),        // brewing_stand
    (965, "farmer"),        // composter
    (749, "fisherman"),     // barrel
    (558, "fletcher"),      // fletching_table
    (967, "leatherworker"), // cauldron
    (560, "librarian"),     // lectern
    (564, "mason"),         // stonecutter
    (565, "shepherd"),      // loom
    (557, "toolsmith"),     // smithing_table
    (563, "weaponsmith"),   // grindstone
];

/// W341: resolve a villager's profession from the NEAREST
/// workstation block within the MC job-search reach (+-8 xz, +-4 y
/// of the villager - vanilla's POI scan is villager-centred), with
/// the stable hash as the no-workstation fallback. One-shot scan at
/// trade-open; the caller caches per mob id.
pub(crate) fn profession_near(
    world: &infengine_voxel::World,
    pos: [f32; 3],
    mob_id: u32,
) -> &'static str {
    let (cx, cy, cz) = (
        pos[0].floor() as i32,
        pos[1].floor() as i32,
        pos[2].floor() as i32,
    );
    let mut best: Option<(&'static str, i32)> = None;
    for y in (cy - 4)..=(cy + 4) {
        for x in (cx - 8)..=(cx + 8) {
            for z in (cz - 8)..=(cz + 8) {
                let id = world.get([x, y, z]).0;
                let Some((_, prof)) =
                    WORKSTATIONS.iter().find(|(w, _)| *w == id)
                else {
                    continue;
                };
                let d2 = (x - cx).pow(2) + (y - cy).pow(2) + (z - cz).pow(2);
                if best.is_none_or(|(_, bd)| d2 < bd) {
                    best = Some((prof, d2));
                }
            }
        }
    }
    best.map(|(p, _)| p).unwrap_or_else(|| profession_for(mob_id))
}

/// Offers for one profession, table order (tier 1 first).
pub(crate) fn trades_for(profession: &str) -> impl Iterator<Item = &'static TradeDef> {
    TRADES.iter().filter(move |t| t.profession == profession)
}

/// W440/W458: MC villagers OFFER a per-villager sample, not the
/// whole pool - vanilla rolls 2 offers per LEVEL from that level's
/// VillagerTrades listing. `level` (1..=5, the villager's current
/// trade level from `level_for_xp`) gates the tiers; the
/// Fisher-Yates pick per tier is seeded by the villager id, so the
/// list is stable per villager and grows monotonically as levels
/// unlock (exactly MC's grow-only offer list). Returned values are
/// indices into the `trades_for(profession)` enumeration.
pub(crate) fn sampled_offer_indices(
    profession: &str,
    villager_id: u32,
    level: u8,
) -> Vec<usize> {
    let mut out = Vec::new();
    for tier in 1..=level.clamp(1, 5) {
        let mut pool: Vec<usize> = trades_for(profession)
            .enumerate()
            .filter(|(_, t)| t.tier == tier)
            .map(|(i, _)| i)
            .collect();
        if pool.is_empty() {
            continue;
        }
        let mut h = (villager_id as u64 ^ ((tier as u64) << 32))
            .wrapping_mul(0x9E37_79B9_7F4A_7C15);
        for i in (1..pool.len()).rev() {
            h ^= h >> 27;
            h = h.wrapping_mul(0x94D0_49BB_1331_11EB);
            h ^= h >> 31;
            let j = (h % (i as u64 + 1)) as usize;
            pool.swap(i, j);
        }
        let take = pool.len().min(2);
        pool.truncate(take);
        pool.sort_unstable();
        out.extend(pool);
    }
    out
}

/// W293 crop families: the harvest drops BLOCK-FORM ids next to the
/// canonical items - either pays a trade wanting the crop.
pub(crate) fn item_matches(inv_id: u16, want_id: u16) -> bool {
    if inv_id == want_id {
        return true;
    }
    const FAMILIES: &[(u16, u16)] = &[
        (337, 493), // wheat
        (344, 494), // carrot
        (343, 495), // potato
        (345, 496), // beetroot
    ];
    FAMILIES
        .iter()
        .any(|&(a, b)| (inv_id == a || inv_id == b) && (want_id == a || want_id == b))
}

/// Count matching items across the stack-typed inventory view.
/// Id-keyed - trade costs are plain materials, so per-stack
/// components are irrelevant to affordability (W469).
pub(crate) fn count_items(
    inv: &[Option<crate::item::ItemStack>],
    want_id: u16,
) -> u32 {
    inv.iter()
        .flatten()
        .filter(|st| item_matches(st.id, want_id))
        .map(|st| st.count as u32)
        .sum()
}

/// Remove `n` matching items from the view. Returns false (and
/// leaves the view untouched) when there are not enough. Partially
/// consumed stacks keep their components on the remainder (W469).
pub(crate) fn take_items(
    inv: &mut [Option<crate::item::ItemStack>],
    want_id: u16,
    n: u32,
) -> bool {
    if count_items(inv, want_id) < n {
        return false;
    }
    let mut left = n;
    for slot in inv.iter_mut() {
        if left == 0 {
            break;
        }
        if let Some(st) = slot {
            if !item_matches(st.id, want_id) {
                continue;
            }
            let take = (st.count as u32).min(left) as u8;
            left -= take as u32;
            if take >= st.count {
                *slot = None;
            } else {
                st.count -= take;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_nonempty_and_professions_cover_it() {
        assert!(TRADES.len() >= 80, "generated table: {}", TRADES.len());
        for t in TRADES {
            assert!(PROFESSIONS.contains(&t.profession));
            assert!(t.want_n >= 1 && t.give_n >= 1);
        }
    }

    /// W341: a composter next to the villager claims the farmer
    /// job; with no workstation the hash fallback answers.
    #[test]
    fn w341_workstation_claims_profession() {
        let mut w = infengine_voxel::World::new();
        w.insert_chunk([0, 0, 0], infengine_voxel::Chunk::filled(
            infengine_voxel::BlockId(0),
        ));
        assert_eq!(
            profession_near(&w, [4.5, 4.0, 4.5], 7),
            profession_for(7),
            "no workstation -> hash fallback",
        );
        w.set([6, 4, 4], infengine_voxel::BlockId(965)); // composter
        assert_eq!(profession_near(&w, [4.5, 4.0, 4.5], 7), "farmer");
        // A closer lectern outranks the composter.
        w.set([5, 4, 4], infengine_voxel::BlockId(560));
        assert_eq!(profession_near(&w, [4.5, 4.0, 4.5], 7), "librarian");
    }

    #[test]
    fn profession_pick_is_stable() {
        for id in [1u32, 7, 42, 900_001] {
            assert_eq!(profession_for(id), profession_for(id));
        }
    }

    /// W440: the offer sample is deterministic per villager, at
    /// most 2 rows per tier, and every index resolves via the
    /// `trades_for(...).nth(i)` path the apply site uses.
    #[test]
    fn sampled_offers_deterministic_and_bounded() {
        for prof in ["shepherd", "farmer", "mason", "armorer"] {
            for vid in [1u32, 7, 42, 900_001] {
                let a = sampled_offer_indices(prof, vid, 2);
                let b = sampled_offer_indices(prof, vid, 2);
                assert_eq!(a, b, "{prof}/{vid} not deterministic");
                assert!(!a.is_empty(), "{prof}/{vid} empty");
                assert!(a.len() <= 4, "{prof}/{vid} too many rows");
                for &i in &a {
                    assert!(
                        trades_for(prof).nth(i).is_some(),
                        "{prof}/{vid} index {i} out of range",
                    );
                }
                let t1 = a
                    .iter()
                    .filter(|&&i| {
                        trades_for(prof).nth(i).unwrap().tier == 1
                    })
                    .count();
                assert!(t1 <= 2, "{prof}/{vid} more than 2 tier-1 rows");
            }
        }
        // Different villagers eventually see different offers (the
        // shepherd tier-2 pool is huge, so 8 ids colliding on one
        // sample would mean the seed is ignored).
        let distinct: std::collections::HashSet<Vec<usize>> = (0..8u32)
            .map(|vid| sampled_offer_indices("shepherd", vid, 2))
            .collect();
        assert!(distinct.len() > 1, "shepherd samples never vary");
    }

    /// W458: levelling grows the offer list monotonically - the
    /// lower-tier picks stay identical and each unlocked tier adds
    /// at most 2 rows (MC's per-level roll).
    #[test]
    fn levels_grow_offers_monotonically() {
        for prof in ["librarian", "mason", "cleric", "fletcher"] {
            let mut prev: Vec<usize> = Vec::new();
            for level in 1..=5u8 {
                let cur = sampled_offer_indices(prof, 4242, level);
                assert!(cur.len() >= prev.len(), "{prof} level {level}");
                assert!(cur.len() <= 2 * level as usize);
                assert_eq!(
                    &cur[..prev.len()],
                    &prev[..],
                    "{prof} level {level} not prefix-stable",
                );
                prev = cur;
            }
        }
    }

    /// W458: the generated table carries master tiers + real XP.
    #[test]
    fn full_table_covers_master_tiers() {
        for prof in ["cleric", "fletcher", "librarian", "mason"] {
            assert!(
                trades_for(prof).any(|t| t.tier == 5),
                "{prof} has a master tier",
            );
        }
        assert!(TRADES.iter().all(|t| t.xp > 0), "every trade grants XP");
        assert!(TRADES.len() > 150, "full table landed");
    }

    /// W458: MC VillagerData thresholds.
    #[test]
    fn level_thresholds_match_mc() {
        assert_eq!(level_for_xp(0), 1);
        assert_eq!(level_for_xp(9), 1);
        assert_eq!(level_for_xp(10), 2);
        assert_eq!(level_for_xp(69), 2);
        assert_eq!(level_for_xp(70), 3);
        assert_eq!(level_for_xp(149), 3);
        assert_eq!(level_for_xp(150), 4);
        assert_eq!(level_for_xp(250), 5);
        assert_eq!(level_for_xp(100_000), 5);
    }

    #[test]
    fn crop_family_pays_the_trade() {
        // Block-form wheat (493) pays a wheat (337) trade. W469:
        // the view is stack-typed; costs consume by id.
        let mut inv: Vec<Option<crate::item::ItemStack>> =
            vec![Some(crate::item::ItemStack::new(493, 25)), None];
        assert_eq!(count_items(&inv, 337), 25);
        assert!(take_items(&mut inv, 337, 20));
        assert_eq!(inv[0], Some(crate::item::ItemStack::new(493, 5)));
        assert!(!take_items(&mut inv, 337, 6));
    }
}
