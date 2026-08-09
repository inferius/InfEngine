//! W474 - sheep shearing (MC `Sheep.mobInteract` + `shear` port).
//!
//! RMB with shears on an adult, alive, unsheared sheep:
//! - rolls `loot_table/shearing/sheep/<color>` (uniform 1-3 rolls of
//!   one wool; colors collapse to our single wool id) and spawns
//!   each wool as its own item entity at the sheep, exactly like the
//!   `Sheep.shear` per-count `spawnAtLocation` loop;
//! - plays `entity.sheep.shear` (synth opt-out - silence beats a
//!   beep when the pack misses the OGG);
//! - sets the `sheared` flag (persisted in entities v7, shipped on
//!   `MobMsg` so every peer hides the wool layer);
//! - wears the held shears 1 point via the W471 `hurt_and_break`
//!   path (`itemStack.hurtAndBreak(1, ...)` in the source).
//!
//! `readyForShearing` = alive && !sheared && !baby; a click on
//! anything else returns false so the RMB cascade keeps going.

use infengine_voxel::BlockId;

use crate::Voxel;

use super::kinds::KIND_SHEEP;

/// Try to shear the mob `mob_id`. Returns true when a shear
/// happened (the caller then suppresses the rest of the RMB
/// cascade). Host / offline only - the caller already gated the
/// client role out.
pub(crate) fn try_shear_sheep(voxel: &mut Voxel, mob_id: u32) -> bool {
    let pos;
    {
        let Some(m) = voxel.mobs.iter_mut().find(|m| m.id == mob_id) else {
            return false;
        };
        // Sheep.readyForShearing: alive, unsheared, adult.
        if m.kind != KIND_SHEEP
            || m.dying > 0.0
            || m.sheared
            || m.baby_left > 0.0
        {
            return false;
        }
        m.sheared = true;
        pos = m.pos;
    }
    // SHEAR_SHEEP loot: uniform 1-3 rolls of 1 wool, one item
    // entity per wool. Seeded off the mob id + shear count-ish salt
    // (day clock bits) so repeat shears vary like MC's RNG.
    let seed = (mob_id as u64)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (voxel.day_time.to_bits() as u64);
    let mut rng = crate::worldgen::mcgen::noise::SplitMix64::new(seed);
    let stacks =
        crate::loot::roll(&super::loot_table::SHEEP_SHEARING, &mut rng);
    let cell = [
        pos[0].floor() as i32,
        pos[1].floor() as i32,
        pos[2].floor() as i32,
    ];
    for (id, count) in stacks {
        for _ in 0..count {
            voxel.spawn_drop(BlockId(id), cell);
        }
    }
    // entity.sheep.shear at the sheep; no synth fallback (W56.1).
    voxel.sfx_at("mob/sheep/shear", pos, |_, _| {});
    // MC: itemStack.hurtAndBreak(1, player, hand) - one wear point
    // on the held shears through the shared W471 port.
    voxel.wear_selected_stack(1);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::ItemStack;
    use crate::mob::state::MobState;

    /// Wool block/item id (`item/mod.rs` row 11).
    const WOOL_ID: u16 = 11;

    fn voxel_with_sheep(baby: bool, sheared: bool) -> Voxel {
        let mut v = Voxel::new();
        let mut m = MobState::new_natural(
            9,
            KIND_SHEEP,
            [2.5, 64.0, 2.5],
            [2.5, 64.0, 2.5],
            0.0,
            0.0,
            8.0,
        );
        m.sheared = sheared;
        if baby {
            m.baby_left = 100.0;
        }
        v.mobs.push(m);
        // Held shears in slot 0 so the wear path has a target.
        v.selected_slot = 0;
        v.inventory[0] =
            Some(ItemStack::new(crate::block::SHEARS_ITEM_ID, 1));
        v
    }

    /// W474: a shear sets the flag, drops 1-3 wool item entities at
    /// the sheep and wears the shears by 1.
    #[test]
    fn shear_sets_flag_drops_wool_and_wears_shears() {
        let mut v = voxel_with_sheep(false, false);
        assert!(try_shear_sheep(&mut v, 9));
        assert!(v.mobs[0].sheared, "setSheared(true)");
        let wool = v
            .dropped_items
            .iter()
            .filter(|d| d.stack.id == WOOL_ID)
            .count();
        assert!(
            (1..=3).contains(&wool),
            "1-3 wool item entities, got {wool}"
        );
        let shears = v.inventory[0].as_ref().expect("shears survive 1 wear");
        assert_eq!(shears.components.damage, 1, "hurtAndBreak(1)");
    }

    /// readyForShearing gates: already-sheared and baby sheep both
    /// refuse (and consume nothing).
    #[test]
    fn shear_refuses_sheared_and_baby() {
        let mut v = voxel_with_sheep(false, true);
        assert!(!try_shear_sheep(&mut v, 9));
        assert!(v.dropped_items.is_empty());

        let mut v2 = voxel_with_sheep(true, false);
        assert!(!try_shear_sheep(&mut v2, 9));
        assert!(!v2.mobs[0].sheared);
        assert_eq!(
            v2.inventory[0].as_ref().unwrap().components.damage,
            0,
            "no wear on a refused shear"
        );
    }

    /// Repeat shears across many sheep hit every count in 1..=3.
    #[test]
    fn shear_wool_count_spans_one_to_three() {
        let mut seen = [false; 4];
        for id in 0..64u32 {
            let mut v = Voxel::new();
            let m = MobState::new_natural(
                id + 1,
                KIND_SHEEP,
                [2.5, 64.0, 2.5],
                [2.5, 64.0, 2.5],
                0.0,
                0.0,
                8.0,
            );
            v.mobs.push(m);
            v.selected_slot = 0;
            v.inventory[0] =
                Some(ItemStack::new(crate::block::SHEARS_ITEM_ID, 1));
            assert!(try_shear_sheep(&mut v, id + 1));
            let wool = v
                .dropped_items
                .iter()
                .filter(|d| d.stack.id == WOOL_ID)
                .count();
            assert!((1..=3).contains(&wool));
            seen[wool] = true;
        }
        assert!(seen[1] && seen[2] && seen[3], "uniform 1-3 rolls: {seen:?}");
    }
}
