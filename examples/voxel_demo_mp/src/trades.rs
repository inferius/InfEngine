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
}

pub(crate) const TRADES: &[TradeDef] = &[
    // armorer/1/emerald_iron_boots
    TradeDef { profession: "armorer", tier: 1, want: 317, want_n: 4, want2: None, give: 890, give_n: 1 },
    // armorer/1/emerald_iron_chestplate
    TradeDef { profession: "armorer", tier: 1, want: 317, want_n: 9, want2: None, give: 888, give_n: 1 },
    // armorer/1/emerald_iron_helmet
    TradeDef { profession: "armorer", tier: 1, want: 317, want_n: 5, want2: None, give: 887, give_n: 1 },
    // armorer/1/emerald_iron_leggings
    TradeDef { profession: "armorer", tier: 1, want: 317, want_n: 7, want2: None, give: 889, give_n: 1 },
    // butcher/1/porkchop_emerald
    TradeDef { profession: "butcher", tier: 1, want: 907, want_n: 7, want2: None, give: 317, give_n: 1 },
    // butcher/2/coal_emerald
    TradeDef { profession: "butcher", tier: 2, want: 325, want_n: 15, want2: None, give: 317, give_n: 1 },
    // butcher/2/emerald_cooked_chicken
    TradeDef { profession: "butcher", tier: 2, want: 317, want_n: 1, want2: None, give: 166, give_n: 8 },
    // butcher/2/emerald_cooked_porkchop
    TradeDef { profession: "butcher", tier: 2, want: 317, want_n: 1, want2: None, give: 285, give_n: 5 },
    // cartographer/1/emerald_map
    TradeDef { profession: "cartographer", tier: 1, want: 317, want_n: 7, want2: None, give: 306, give_n: 1 },
    // cartographer/1/paper_emerald
    TradeDef { profession: "cartographer", tier: 1, want: 304, want_n: 24, want2: None, give: 317, give_n: 1 },
    // cartographer/2/glass_pane_emerald
    TradeDef { profession: "cartographer", tier: 2, want: 80, want_n: 11, want2: None, give: 317, give_n: 1 },
    // cleric/1/rotten_flesh_emerald
    TradeDef { profession: "cleric", tier: 1, want: 293, want_n: 32, want2: None, give: 317, give_n: 1 },
    // cleric/2/emerald_lapis_lazuli
    TradeDef { profession: "cleric", tier: 2, want: 317, want_n: 1, want2: None, give: 315, give_n: 1 },
    // cleric/2/gold_ingot_emerald
    TradeDef { profession: "cleric", tier: 2, want: 318, want_n: 3, want2: None, give: 317, give_n: 1 },
    // farmer/1/beetroot_emerald
    TradeDef { profession: "farmer", tier: 1, want: 345, want_n: 15, want2: None, give: 317, give_n: 1 },
    // farmer/1/carrot_emerald
    TradeDef { profession: "farmer", tier: 1, want: 344, want_n: 22, want2: None, give: 317, give_n: 1 },
    // farmer/1/emerald_bread
    TradeDef { profession: "farmer", tier: 1, want: 317, want_n: 1, want2: None, give: 163, give_n: 6 },
    // farmer/1/potato_emerald
    TradeDef { profession: "farmer", tier: 1, want: 343, want_n: 26, want2: None, give: 317, give_n: 1 },
    // farmer/1/wheat_emerald
    TradeDef { profession: "farmer", tier: 1, want: 337, want_n: 20, want2: None, give: 317, give_n: 1 },
    // farmer/2/emerald_apple
    TradeDef { profession: "farmer", tier: 2, want: 317, want_n: 1, want2: None, give: 162, give_n: 4 },
    // farmer/2/emerald_pumpkin_pie
    TradeDef { profession: "farmer", tier: 2, want: 317, want_n: 1, want2: None, give: 281, give_n: 4 },
    // farmer/2/pumpkin_emerald
    TradeDef { profession: "farmer", tier: 2, want: 448, want_n: 6, want2: None, give: 317, give_n: 1 },
    // fisherman/1/coal_emerald
    TradeDef { profession: "fisherman", tier: 1, want: 325, want_n: 10, want2: None, give: 317, give_n: 1 },
    // fisherman/1/string_emerald
    TradeDef { profession: "fisherman", tier: 1, want: 301, want_n: 20, want2: None, give: 317, give_n: 1 },
    // fletcher/1/emerald_arrow
    TradeDef { profession: "fletcher", tier: 1, want: 317, want_n: 1, want2: None, give: 643, give_n: 16 },
    // fletcher/1/gravel_and_emerald_flint
    TradeDef { profession: "fletcher", tier: 1, want: 644, want_n: 10, want2: Some((317, 1)), give: 327, give_n: 10 },
    // fletcher/1/stick_emerald
    TradeDef { profession: "fletcher", tier: 1, want: 302, want_n: 32, want2: None, give: 317, give_n: 1 },
    // fletcher/2/emerald_bow
    TradeDef { profession: "fletcher", tier: 2, want: 317, want_n: 2, want2: None, give: 638, give_n: 1 },
    // fletcher/2/flint_emerald
    TradeDef { profession: "fletcher", tier: 2, want: 327, want_n: 26, want2: None, give: 317, give_n: 1 },
    // leatherworker/1/leather_emerald
    TradeDef { profession: "leatherworker", tier: 1, want: 303, want_n: 6, want2: None, give: 317, give_n: 1 },
    // leatherworker/2/flint_emerald
    TradeDef { profession: "leatherworker", tier: 2, want: 327, want_n: 26, want2: None, give: 317, give_n: 1 },
    // librarian/1/emerald_bookshelf
    TradeDef { profession: "librarian", tier: 1, want: 317, want_n: 9, want2: None, give: 718, give_n: 1 },
    // librarian/1/paper_emerald
    TradeDef { profession: "librarian", tier: 1, want: 304, want_n: 24, want2: None, give: 317, give_n: 1 },
    // librarian/2/book_emerald
    TradeDef { profession: "librarian", tier: 2, want: 305, want_n: 4, want2: None, give: 317, give_n: 1 },
    // librarian/2/emerald_lantern
    TradeDef { profession: "librarian", tier: 2, want: 317, want_n: 1, want2: None, give: 60, give_n: 1 },
    // mason/1/emerald_brick
    TradeDef { profession: "mason", tier: 1, want: 317, want_n: 1, want2: None, give: 675, give_n: 10 },
    // mason/2/emerald_chiseled_stone_bricks
    TradeDef { profession: "mason", tier: 2, want: 317, want_n: 1, want2: None, give: 365, give_n: 4 },
    // mason/2/stone_emerald
    TradeDef { profession: "mason", tier: 2, want: 1, want_n: 20, want2: None, give: 317, give_n: 1 },
    // shepherd/1/black_wool_emerald
    TradeDef { profession: "shepherd", tier: 1, want: 108, want_n: 18, want2: None, give: 317, give_n: 1 },
    // shepherd/1/brown_wool_emerald
    TradeDef { profession: "shepherd", tier: 1, want: 107, want_n: 18, want2: None, give: 317, give_n: 1 },
    // shepherd/1/emerald_shears
    TradeDef { profession: "shepherd", tier: 1, want: 317, want_n: 2, want2: None, give: 640, give_n: 1 },
    // shepherd/1/gray_wool_emerald
    TradeDef { profession: "shepherd", tier: 1, want: 103, want_n: 18, want2: None, give: 317, give_n: 1 },
    // shepherd/2/black_dye_emerald
    TradeDef { profession: "shepherd", tier: 2, want: 661, want_n: 12, want2: None, give: 317, give_n: 1 },
    // shepherd/2/emerald_black_carpet
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 232, give_n: 4 },
    // shepherd/2/emerald_black_wool
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 108, give_n: 1 },
    // shepherd/2/emerald_blue_carpet
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 228, give_n: 4 },
    // shepherd/2/emerald_blue_wool
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 91, give_n: 1 },
    // shepherd/2/emerald_brown_carpet
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 229, give_n: 4 },
    // shepherd/2/emerald_brown_wool
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 107, give_n: 1 },
    // shepherd/2/emerald_cyan_carpet
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 226, give_n: 4 },
    // shepherd/2/emerald_cyan_wool
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 105, give_n: 1 },
    // shepherd/2/emerald_gray_carpet
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 224, give_n: 4 },
    // shepherd/2/emerald_gray_wool
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 103, give_n: 1 },
    // shepherd/2/emerald_green_carpet
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 230, give_n: 4 },
    // shepherd/2/emerald_green_wool
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 94, give_n: 1 },
    // shepherd/2/emerald_light_blue_carpet
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 220, give_n: 4 },
    // shepherd/2/emerald_light_blue_wool
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 100, give_n: 1 },
    // shepherd/2/emerald_light_gray_carpet
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 225, give_n: 4 },
    // shepherd/2/emerald_light_gray_wool
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 104, give_n: 1 },
    // shepherd/2/emerald_lime_carpet
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 222, give_n: 4 },
    // shepherd/2/emerald_lime_wool
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 101, give_n: 1 },
    // shepherd/2/emerald_magenta_carpet
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 219, give_n: 4 },
    // shepherd/2/emerald_magenta_wool
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 99, give_n: 1 },
    // shepherd/2/emerald_orange_carpet
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 218, give_n: 4 },
    // shepherd/2/emerald_orange_wool
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 98, give_n: 1 },
    // shepherd/2/emerald_pink_carpet
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 223, give_n: 4 },
    // shepherd/2/emerald_pink_wool
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 102, give_n: 1 },
    // shepherd/2/emerald_purple_carpet
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 227, give_n: 4 },
    // shepherd/2/emerald_purple_wool
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 106, give_n: 1 },
    // shepherd/2/emerald_red_carpet
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 231, give_n: 4 },
    // shepherd/2/emerald_red_wool
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 92, give_n: 1 },
    // shepherd/2/emerald_white_carpet
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 217, give_n: 4 },
    // shepherd/2/emerald_yellow_carpet
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 221, give_n: 4 },
    // shepherd/2/emerald_yellow_wool
    TradeDef { profession: "shepherd", tier: 2, want: 317, want_n: 1, want2: None, give: 93, give_n: 1 },
    // shepherd/2/gray_dye_emerald
    TradeDef { profession: "shepherd", tier: 2, want: 660, want_n: 12, want2: None, give: 317, give_n: 1 },
    // shepherd/2/light_blue_dye_emerald
    TradeDef { profession: "shepherd", tier: 2, want: 669, want_n: 12, want2: None, give: 317, give_n: 1 },
    // shepherd/2/lime_dye_emerald
    TradeDef { profession: "shepherd", tier: 2, want: 666, want_n: 12, want2: None, give: 317, give_n: 1 },
    // shepherd/2/white_dye_emerald
    TradeDef { profession: "shepherd", tier: 2, want: 658, want_n: 12, want2: None, give: 317, give_n: 1 },
    // smith/1/coal_emerald
    TradeDef { profession: "smith", tier: 1, want: 325, want_n: 15, want2: None, give: 317, give_n: 1 },
    // smith/2/iron_ingot_emerald
    TradeDef { profession: "smith", tier: 2, want: 324, want_n: 4, want2: None, give: 317, give_n: 1 },
    // toolsmith/1/emerald_stone_axe
    TradeDef { profession: "toolsmith", tier: 1, want: 317, want_n: 1, want2: None, give: 865, give_n: 1 },
    // toolsmith/1/emerald_stone_hoe
    TradeDef { profession: "toolsmith", tier: 1, want: 317, want_n: 1, want2: None, give: 333, give_n: 1 },
    // toolsmith/1/emerald_stone_pickaxe
    TradeDef { profession: "toolsmith", tier: 1, want: 317, want_n: 1, want2: None, give: 866, give_n: 1 },
    // weaponsmith/1/emerald_iron_axe
    TradeDef { profession: "weaponsmith", tier: 1, want: 317, want_n: 3, want2: None, give: 873, give_n: 1 },
];

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

/// Offers for one profession, table order (tier 1 first).
pub(crate) fn trades_for(profession: &str) -> impl Iterator<Item = &'static TradeDef> {
    TRADES.iter().filter(move |t| t.profession == profession)
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

/// Count matching items across the inventory view.
pub(crate) fn count_items(inv: &[Option<(u16, u8)>], want_id: u16) -> u32 {
    inv.iter()
        .flatten()
        .filter(|(id, _)| item_matches(*id, want_id))
        .map(|(_, c)| *c as u32)
        .sum()
}

/// Remove `n` matching items from the view. Returns false (and
/// leaves the view untouched) when there are not enough.
pub(crate) fn take_items(inv: &mut [Option<(u16, u8)>], want_id: u16, n: u32) -> bool {
    if count_items(inv, want_id) < n {
        return false;
    }
    let mut left = n;
    for slot in inv.iter_mut() {
        if left == 0 {
            break;
        }
        if let Some((id, count)) = slot {
            if !item_matches(*id, want_id) {
                continue;
            }
            let take = (*count as u32).min(left) as u8;
            left -= take as u32;
            if take >= *count {
                *slot = None;
            } else {
                *count -= take;
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

    #[test]
    fn profession_pick_is_stable() {
        for id in [1u32, 7, 42, 900_001] {
            assert_eq!(profession_for(id), profession_for(id));
        }
    }

    #[test]
    fn crop_family_pays_the_trade() {
        // Block-form wheat (493) pays a wheat (337) trade.
        let mut inv: Vec<Option<(u16, u8)>> = vec![Some((493, 25)), None];
        assert_eq!(count_items(&inv, 337), 25);
        assert!(take_items(&mut inv, 337, 20));
        assert_eq!(inv[0], Some((493, 5)));
        assert!(!take_items(&mut inv, 337, 6));
    }
}
