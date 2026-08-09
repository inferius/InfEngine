//! W477 - block-break drop overrides (the first slice of MC block
//! loot tables).
//!
//! MC blocks whose loot table drops an ITEM other than the block
//! itself (`VanillaBlockLoot.java`). Full block-loot as a system
//! (silk touch, fortune, shears) stays queued; this table carries
//! the rows the W476/W477 economy needs so the id-13 ore-coal
//! hybrid could die: coal ores pay the real coal item, clay pays
//! its four balls.

/// `Some((item_id, count))` when breaking `block_id` drops something
/// other than the block itself; `None` = the block drops itself
/// (the engine default).
pub(crate) fn break_drop_override(block_id: u16) -> Option<(u16, u8)> {
    match block_id {
        // coal_ore / deepslate_coal_ore (123) -> coal item
        // (`createOreDrop(COAL_ORE, Items.COAL)`; fortune queued).
        crate::block::COAL_ORE_ID | 123 => Some((325, 1)),
        // clay -> 4x clay_ball (`createSingleItemTableWithSilkTouch
        // (CLAY, Items.CLAY_BALL, 4)`; no silk touch exists yet).
        765 => Some((994, 4)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coal_ores_drop_the_coal_item() {
        assert_eq!(break_drop_override(13), Some((325, 1)));
        assert_eq!(break_drop_override(123), Some((325, 1)));
    }

    #[test]
    fn clay_drops_four_clay_balls() {
        assert_eq!(break_drop_override(765), Some((994, 4)));
    }

    #[test]
    fn ordinary_blocks_drop_themselves() {
        assert_eq!(break_drop_override(1), None); // stone
        assert_eq!(break_drop_override(14), None); // iron ore (queued)
        assert_eq!(break_drop_override(0), None);
    }
}
