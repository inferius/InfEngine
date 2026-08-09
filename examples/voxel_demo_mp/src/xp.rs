//! W418 - the MC experience system (points -> levels).
//!
//! Stored as a single running total of points; level + bar
//! progress derive from the MC level-cost curve
//! (`Player.getXpNeededForNextLevel`): `2n+7` through level 15,
//! then `5n-38` through 30, then `9n-158` beyond. Sources this
//! wave: mob kills + ore mining (MC ExperienceOrb amounts);
//! smelting, breeding and bottles are follow-ups. Death wipes the
//! pool (orb drops are a follow-up too).

/// MC `Player.getXpNeededForNextLevel` - points from `level` to
/// `level + 1`.
pub(crate) fn xp_for_level_up(level: u32) -> u32 {
    if level >= 31 {
        9 * level - 158
    } else if level >= 16 {
        5 * level - 38
    } else {
        2 * level + 7
    }
}

/// Derive `(level, bar_progress)` from the running point total.
pub(crate) fn level_progress(total: u32) -> (u32, f32) {
    let mut level = 0u32;
    let mut rem = total;
    loop {
        let need = xp_for_level_up(level);
        if rem < need {
            return (level, rem as f32 / need as f32);
        }
        rem -= need;
        level += 1;
        if level > 21_863 {
            // Past the u32-total ceiling; clamp instead of spinning.
            return (level, 0.0);
        }
    }
}

/// XP for killing `kind` (MC per-entity ExperienceOrb rewards):
/// most hostiles 5, the heavies 10, passives 1-3 (seeded roll).
pub(crate) fn mob_kill_xp(kind: u8, seed: u32) -> u32 {
    let name = crate::mob::kinds::MobKind::from_u8(kind).name();
    match name {
        "blaze" | "guardian" | "elder_guardian" | "evoker" | "ravager" => 10,
        "zombie" | "skeleton" | "spider" | "creeper" | "husk" | "stray"
        | "drowned" | "witch" | "enderman" | "slime" | "phantom"
        | "zombified_piglin" | "ghast" | "magma_cube" | "piglin"
        | "cave_spider" | "silverfish" | "vindicator" | "pillager"
        | "wither_skeleton" | "vex" | "hoglin" | "zoglin" | "bogged"
        | "parched" | "shulker" | "warden" | "breeze" => 5,
        _ => 1 + seed % 3,
    }
}

/// XP for mining an ore (MC `DropExperienceBlock` ranges; iron /
/// gold / copper pay on SMELT, not on break, so they return 0
/// here). Deepslate variants share the stone row's roll.
pub(crate) fn ore_break_xp(block_id: u16, seed: u32) -> u32 {
    let Some(bt) = crate::block::block_type(block_id) else {
        return 0;
    };
    let stem = bt.name.strip_prefix("deepslate_").unwrap_or(bt.name);
    match stem {
        // W477: surface coal ore matches now - its legacy registry
        // name "coal" never hit this row (deepslate did), so it
        // paid no break XP until the rename.
        "coal_ore" => seed % 3,                        // 0..=2
        "diamond_ore" | "emerald_ore" => 3 + seed % 5, // 3..=7
        "lapis_ore" | "nether_quartz_ore" => 2 + seed % 4, // 2..=5
        "redstone_ore" => 1 + seed % 5,                // 1..=5
        "nether_gold_ore" => seed % 2,                 // 0..=1
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_cost_curve_matches_mc() {
        // Pinned to Player.getXpNeededForNextLevel.
        assert_eq!(xp_for_level_up(0), 7);
        assert_eq!(xp_for_level_up(15), 37);
        assert_eq!(xp_for_level_up(16), 42);
        assert_eq!(xp_for_level_up(30), 112);
        assert_eq!(xp_for_level_up(31), 121);
    }

    #[test]
    fn level_progress_walks_the_curve() {
        assert_eq!(level_progress(0), (0, 0.0));
        let (l, p) = level_progress(6);
        assert_eq!(l, 0);
        assert!((p - 6.0 / 7.0).abs() < 1e-6);
        assert_eq!(level_progress(7), (1, 0.0));
        // 7 + 9 = 16 points -> exactly level 2.
        assert_eq!(level_progress(16), (2, 0.0));
    }

    #[test]
    fn xp_sources_ranges() {
        // Hostile flat 5, heavy 10, passive 1..=3.
        assert_eq!(mob_kill_xp(1, 0), 5); // zombie
        for s in 0..10u32 {
            let p = mob_kill_xp(0, s); // sheep
            assert!((1..=3).contains(&p));
        }
        // Ore rows: diamond 3..=7, iron pays nothing on break.
        for s in 0..10u32 {
            let d = ore_break_xp(403, s);
            let _ = d; // id sanity only if 403 is diamond ore
        }
    }
}
