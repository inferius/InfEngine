//! W294 - MC crop stage chains + loot, 1:1 from `CropBlock` /
//! `BeetrootBlock` and the crop loot tables (26.1.2).
//!
//! Stages are BLOCK IDS (the engine's id-family pattern - chunks
//! persist them with no side map): wheat / carrots / potatoes run
//! MC's 8 ages, beetroots 4. Age 0 keeps the legacy `*_immature`
//! ids (646..649) and the final age keeps the legacy mature ids
//! (493..496), so existing saves stay valid; ages in between are the
//! W294 rows 844..863. Textures follow MC's blockstate mapping
//! (carrots/potatoes paint 4 sprites over 8 ages).
//!
//! Lives in the `block` tree (not `world/crop.rs`) so the LIB-side
//! worldgen blockmap can resolve `wheat[age=N]` template states to
//! stage ids - village farms carve half-grown crops like vanilla.

/// Farmland (W294; MC `farmland`, assumed moist - see the tick).
pub(crate) const FARMLAND_ID: u16 = 864;

const WHEAT_AGES: [u16; 8] = [646, 844, 845, 846, 847, 848, 849, 493];
const CARROT_AGES: [u16; 8] = [647, 850, 851, 852, 853, 854, 855, 494];
const POTATO_AGES: [u16; 8] = [648, 856, 857, 858, 859, 860, 861, 495];
const BEETROOT_AGES: [u16; 4] = [649, 862, 863, 496];

/// Item ids the harvest drops (item/mod.rs rows).
const WHEAT_ITEM: u16 = 337;
const WHEAT_SEEDS_ITEM: u16 = 338;
const BEETROOT_SEEDS_ITEM: u16 = 341;
const POTATO_ITEM: u16 = 343;
const CARROT_ITEM: u16 = 344;
const BEETROOT_ITEM: u16 = 345;

/// The stage chain `id` belongs to + its age index, or `None` when
/// `id` isn't a crop stage.
pub(crate) fn crop_ages(id: u16) -> Option<(&'static [u16], usize)> {
    for ages in [
        &WHEAT_AGES[..],
        &CARROT_AGES[..],
        &POTATO_AGES[..],
        &BEETROOT_AGES[..],
    ] {
        if let Some(i) = ages.iter().position(|a| *a == id) {
            return Some((ages, i));
        }
    }
    None
}

/// Any crop stage (age 0 through mature).
pub(crate) fn is_crop(id: u16) -> bool {
    crop_ages(id).is_some()
}

/// The final age of its chain.
pub(crate) fn is_mature_crop(id: u16) -> bool {
    crop_ages(id).is_some_and(|(ages, i)| i == ages.len() - 1)
}

/// A crop stage that still has growing to do (the random-tick
/// subject; every stage below the final age).
pub(crate) fn is_immature_crop(id: u16) -> bool {
    crop_ages(id).is_some_and(|(ages, i)| i < ages.len() - 1)
}

/// The next age's block id (`None` at the final age / non-crop).
pub(crate) fn next_stage(id: u16) -> Option<u16> {
    let (ages, i) = crop_ages(id)?;
    ages.get(i + 1).copied()
}

/// The age-0 stage a placed MATURE crop id swaps to (the legacy
/// block-form wheat/carrot/... items place their mature id; planting
/// always starts young). `None` for anything else.
pub(crate) fn immature_for(mature: u16) -> Option<u16> {
    match crop_ages(mature) {
        Some((ages, i)) if i == ages.len() - 1 => Some(ages[0]),
        _ => None,
    }
}

/// Beetroot stages tick slower (`BeetrootBlock.randomTick` rolls
/// `nextInt(3) != 0 -> skip` before the shared crop growth).
pub(crate) fn is_beetroot_stage(id: u16) -> bool {
    BEETROOT_AGES.contains(&id)
}

/// Same crop family (the `CropBlock.getGrowthSpeed` row penalty
/// compares `blockState.is(this)`).
pub(crate) fn same_family(a: u16, b: u16) -> bool {
    match (crop_ages(a), crop_ages(b)) {
        (Some((fa, _)), Some((fb, _))) => std::ptr::eq(fa, fb),
        _ => false,
    }
}

/// Blockmap entry: MC blockstate name + `age` -> stage id. Ages
/// clamp into the chain so an odd template value can't panic.
pub(crate) fn stage_for_age(name: &str, age: usize) -> Option<u16> {
    let ages: &[u16] = match name {
        "wheat" => &WHEAT_AGES,
        "carrots" => &CARROT_AGES,
        "potatoes" => &POTATO_AGES,
        "beetroots" => &BEETROOT_AGES,
        _ => return None,
    };
    Some(ages[age.min(ages.len() - 1)])
}

/// MC crop loot, host-side roll: `(item id, count)` pairs.
///
/// - wheat mature: 1 wheat + binomial(3, 4/7) seeds; immature: 1 seed.
/// - carrots mature: 1 + binomial(3, 4/7) carrots; immature: 1 carrot.
/// - potatoes mature: 1 + binomial(3, 4/7) potatoes (the 2% poisonous
///   roll waits on the poisonous_potato item); immature: 1 potato.
/// - beetroots mature: 1 beetroot + binomial(3, 4/7) seeds; immature:
///   1 seed.
pub(crate) fn harvest_drops(id: u16, mut rng: u32) -> Vec<(u16, u8)> {
    let Some((ages, _)) = crop_ages(id) else {
        return Vec::new();
    };
    let mature = is_mature_crop(id);
    // Three independent 4/7 trials (`ApplyBonusCount.BinomialWithBonusCount`
    // extra=3, probability=0.5714286 at no fortune).
    let mut binom = |tries: u32| -> u8 {
        let mut n = 0u8;
        for _ in 0..tries {
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            if rng % 7 < 4 {
                n += 1;
            }
        }
        n
    };
    let family = ages[ages.len() - 1];
    match family {
        493 => {
            if mature {
                let seeds = binom(3);
                let mut out = vec![(WHEAT_ITEM, 1)];
                if seeds > 0 {
                    out.push((WHEAT_SEEDS_ITEM, seeds));
                }
                out
            } else {
                vec![(WHEAT_SEEDS_ITEM, 1)]
            }
        }
        494 => {
            if mature {
                vec![(CARROT_ITEM, 1 + binom(3))]
            } else {
                vec![(CARROT_ITEM, 1)]
            }
        }
        495 => {
            if mature {
                vec![(POTATO_ITEM, 1 + binom(3))]
            } else {
                vec![(POTATO_ITEM, 1)]
            }
        }
        496 => {
            if mature {
                let seeds = binom(3);
                let mut out = vec![(BEETROOT_ITEM, 1)];
                if seeds > 0 {
                    out.push((BEETROOT_SEEDS_ITEM, seeds));
                }
                out
            } else {
                vec![(BEETROOT_SEEDS_ITEM, 1)]
            }
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_chains_walk_to_mature() {
        // Wheat: 8 ages, 646 -> ... -> 493.
        let mut id = 646;
        let mut hops = 0;
        while let Some(next) = next_stage(id) {
            id = next;
            hops += 1;
        }
        assert_eq!((id, hops), (493, 7));
        // Beetroot: 4 ages.
        let mut id = 649;
        let mut hops = 0;
        while let Some(next) = next_stage(id) {
            id = next;
            hops += 1;
        }
        assert_eq!((id, hops), (496, 3));
        assert!(is_mature_crop(493) && !is_immature_crop(493));
        assert!(is_immature_crop(846) && !is_mature_crop(846));
        assert!(same_family(646, 493) && !same_family(646, 494));
    }

    #[test]
    fn blockmap_age_routing() {
        assert_eq!(stage_for_age("wheat", 0), Some(646));
        assert_eq!(stage_for_age("wheat", 3), Some(846));
        assert_eq!(stage_for_age("wheat", 7), Some(493));
        assert_eq!(stage_for_age("beetroots", 3), Some(496));
        // Odd template age clamps instead of panicking.
        assert_eq!(stage_for_age("wheat", 99), Some(493));
        assert_eq!(stage_for_age("melon_stem", 3), None);
    }

    #[test]
    fn harvest_loot_shapes() {
        // Immature wheat: exactly one seed, never the wheat item.
        let drops = harvest_drops(846, 12345);
        assert_eq!(drops, vec![(338, 1)]);
        // Mature wheat: always 1 wheat first; seeds 0..=3.
        for seed in [1u32, 999, 424242, 7] {
            let drops = harvest_drops(493, seed);
            assert_eq!(drops[0], (337, 1));
            let seeds: u8 =
                drops.iter().filter(|(i, _)| *i == 338).map(|(_, c)| *c).sum();
            assert!(seeds <= 3);
        }
        // Mature carrots: 1..=4 carrots, nothing else.
        for seed in [3u32, 55, 1000] {
            let drops = harvest_drops(494, seed);
            assert_eq!(drops.len(), 1);
            assert_eq!(drops[0].0, 344);
            assert!((1..=4).contains(&drops[0].1));
        }
        // Non-crop: empty.
        assert!(harvest_drops(1, 5).is_empty());
    }
}
