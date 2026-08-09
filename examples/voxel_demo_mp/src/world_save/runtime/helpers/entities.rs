//! Per-dim mob + drop save / apply. Builds `DimensionEntities`
//! from raw refs (so the per-dim save path can hand in either the
//! active dim's live state or a parked-dim snapshot) and writes
//! the bundle back onto `voxel` on load.

use crate::Voxel;
use crate::drops::DropItem;
use crate::minecart::Minecart;
use crate::mob::MobState;

use crate::world_save::entities::{
    CartSave, DimensionEntities, DropSave, ENTITIES_FORMAT_VERSION, MobSave,
};

/// Build a `DimensionEntities` snapshot from raw refs. The
/// per-dim save path (`dims::save_*_dim`) hands in either
/// `voxel.mobs` / `voxel.dropped_items` / `voxel.carts` (active
/// dim) or the parking maps (`voxel.entities_by_dim[dim]`).
pub(in crate::world_save::runtime) fn build_dimension_entities_from(
    mobs: &[MobState],
    drops: &[DropItem],
    carts: &[Minecart],
    next_mob_id: u32,
    next_drop_id: u32,
    next_cart_id: u32,
    structure_mob_anchors: &std::collections::HashSet<[i32; 3]>,
) -> DimensionEntities {
    let mob_saves: Vec<MobSave> = mobs
        .iter()
        .map(|m| MobSave {
            id: m.id,
            kind: m.kind,
            pos: m.pos,
            yaw: m.yaw,
            vel: [0.0, 0.0, 0.0],
            health: m.health,
            age_seconds: 0.0,
            persistent: m.persistent,
            baby_left: m.baby_left,
            tamed: m.tamed,
            temper: m.temper,
            trade_xp: m.trade_xp,
            // W474 (v7): shorn sheep reload shorn.
            sheared: m.sheared,
            // W476.1 (v8): slime / magma-cube size reloads intact.
            size: m.size,
            // W479 (v9): the trader's despawn budget reloads intact.
            despawn_delay: m.despawn_delay,
        })
        .collect();
    // Deterministic order keeps identical saves byte-identical.
    let mut anchors: Vec<[i32; 3]> =
        structure_mob_anchors.iter().copied().collect();
    anchors.sort();
    // W468 (v6): the drop's full stack persists - a merged
    // 30-cobble pile or a worn tool survives reload (pre-v6 saved
    // one bare block id and reloaded everything at count 1).
    let drop_saves: Vec<DropSave> = drops
        .iter()
        .map(|d| DropSave {
            id: d.id,
            stack: d.stack.clone(),
            pos: d.pos,
            vel: d.vel,
            age: d.age,
        })
        .collect();
    // W137: carts persist their rail binding (cell + t), not the
    // derived world position - `snap_to_rail` rebuilds that on load.
    let cart_saves: Vec<CartSave> = carts
        .iter()
        .map(|c| CartSave {
            id: c.id,
            cell: c.cell,
            t: c.t,
            vel: c.vel,
            yaw: c.yaw,
        })
        .collect();
    DimensionEntities {
        format_version: ENTITIES_FORMAT_VERSION,
        mobs: mob_saves,
        drops: drop_saves,
        next_mob_id,
        next_drop_id,
        carts: cart_saves,
        next_cart_id,
        structure_mob_anchors: anchors,
    }
}

/// W476.1: clamp a saved mob size onto the MC slime-size domain
/// (`Slime.setSize` only ever mints 1, 2 or 4 via `1 << rng(3)`).
/// Rounds down so a hand-edited 3 becomes 2, anything >= 4 caps at
/// 4, and 0 lifts to 1.
pub(in crate::world_save::runtime) fn clamp_mob_size(size: u8) -> u8 {
    match size {
        0 | 1 => 1,
        2 | 3 => 2,
        _ => 4,
    }
}

/// Apply a loaded `DimensionEntities` back onto `voxel`. Replaces
/// the live mob + drop lists; the runtime broadcast loop will
/// re-emit spawn messages on the next net-tick frame.
pub(in crate::world_save::runtime) fn apply_dimension_entities(
    voxel: &mut Voxel,
    entities: DimensionEntities,
) {
    voxel.mobs.clear();
    voxel.remote_mobs.clear();
    voxel.dropped_items.clear();
    voxel.carts.clear();
    voxel.remote_carts.clear();
    voxel.riding_cart = None;
    voxel.riding_mob = None;
    for m in entities.mobs {
        // Drop mobs with zero / negative health - they were dead on
        // last save and the runtime would only despawn them again.
        if m.health <= 0.0 {
            continue;
        }
        let mut mob = MobState::new_natural(
            m.id,
            m.kind,
            m.pos,
            m.pos,
            m.yaw,
            0.0,
            m.health,
        );
        // W302 (v3): persistence + babyhood survive reload.
        mob.persistent = m.persistent;
        mob.baby_left = m.baby_left;
        // W348 (v4): tame state survives reload.
        mob.tamed = m.tamed;
        mob.temper = m.temper;
        // W459 (v5): villager trade XP survives reload.
        mob.trade_xp = m.trade_xp;
        // W474 (v7): sheep shear state survives reload.
        mob.sheared = m.sheared;
        // W476.1 (v8): slime / magma-cube size survives reload,
        // clamped onto the MC `1 << rng(3)` domain (1|2|4) so an
        // edited save can't mint a size-0 or size-255 blob.
        mob.size = clamp_mob_size(m.size);
        // W479 (v9): the wandering trader's despawn budget keeps
        // counting where it left off.
        mob.despawn_delay = m.despawn_delay;
        voxel.mobs.push(mob);
    }
    // W302: the spawned-anchor set (v2 saves migrate with an empty
    // one; already-live villagers keep their persistent flag OFF for
    // pre-v3 saves - regenerating chunks may re-mint once there).
    voxel.structure_mob_anchors =
        entities.structure_mob_anchors.iter().copied().collect();
    for d in entities.drops {
        // W468: the saved stack rehydrates wholesale (count +
        // components). The tick clocks re-derive from the float
        // age; pickup-delay restarts expired so loading doesn't
        // reset the pickup gate.
        let age_ticks = (d.age * 20.0).max(0.0) as u32;
        voxel.dropped_items.push(DropItem {
            id: d.id,
            stack: d.stack,
            pos: d.pos,
            vel: d.vel,
            age: d.age,
            age_ticks,
            pickup_delay_ticks: 0,
        });
    }
    // W137: rebuild each cart's world pose from its rail binding.
    // The chunks loaded before this call (load_active_dim order), so
    // `snap_to_rail` sees the rails; a cart whose rail vanished
    // (edited save) keeps cell-centre coords and freezes on next tick.
    for c in entities.carts {
        let mut cart = Minecart {
            id: c.id,
            cell: c.cell,
            t: c.t,
            vel: c.vel,
            pos: glam::Vec3::new(
                c.cell[0] as f32 + 0.5,
                c.cell[1] as f32,
                c.cell[2] as f32 + 0.5,
            ),
            yaw: c.yaw,
        };
        crate::minecart::snap_to_rail(&voxel.world, &voxel.rail_shapes, &mut cart);
        voxel.carts.push(cart);
    }
    voxel.next_mob_id = entities
        .next_mob_id
        .max(voxel.mobs.iter().map(|m| m.id).max().unwrap_or(0) + 1);
    voxel.next_drop_id = entities
        .next_drop_id
        .max(voxel.dropped_items.iter().map(|d| d.id).max().unwrap_or(0) + 1);
    voxel.next_cart_id = entities
        .next_cart_id
        .max(voxel.carts.iter().map(|c| c.id).max().unwrap_or(0) + 1);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// W468: the full runtime round trip - a merged, worn drop
    /// (damage 20, count 5) builds into the save bundle, survives
    /// the disk envelope, and applies back with both intact
    /// (pre-v6 the count collapsed to 1 and components did not
    /// exist on drops at all).
    #[test]
    fn drop_stack_survives_save_and_reload() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut worn = crate::item::ItemStack::new(48, 5);
        worn.components.damage = 20;
        let drop = DropItem {
            id: 7,
            stack: worn.clone(),
            pos: [1.5, 65.0, 2.5],
            vel: [0.0; 3],
            age: 3.0,
            age_ticks: 60,
            pickup_delay_ticks: 0,
        };
        let anchors: std::collections::HashSet<[i32; 3]> =
            Default::default();
        let bundle = build_dimension_entities_from(
            &[],
            std::slice::from_ref(&drop),
            &[],
            1,
            8,
            1,
            &anchors,
        );
        assert_eq!(bundle.drops.len(), 1);
        assert_eq!(bundle.drops[0].stack, worn);

        crate::world_save::entities::write_entities_in_dir(
            tmp.path(),
            "slot",
            "overworld",
            &bundle,
        )
        .unwrap();
        let loaded = crate::world_save::entities::read_entities_in_dir(
            tmp.path(),
            "slot",
            "overworld",
        )
        .unwrap()
        .expect("entities file exists");
        assert_eq!(loaded, bundle);

        let mut v = crate::Voxel::new();
        apply_dimension_entities(&mut v, loaded);
        assert_eq!(v.dropped_items.len(), 1);
        assert_eq!(v.dropped_items[0].stack, worn, "count + damage intact");
        assert_eq!(v.dropped_items[0].id, 7);
    }

    /// W476.1: a size-4 slime builds into the save bundle, survives
    /// the disk envelope, and applies back at size 4 (pre-v8 the
    /// field did not exist and every slime reloaded at size 1 with
    /// its big-size HP - the render shrank while health did not).
    #[test]
    fn mob_size_survives_save_and_reload() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut slime = MobState::new_natural(
            5,
            0,
            [3.0, 65.0, 3.0],
            [3.0, 65.0, 3.0],
            0.0,
            0.0,
            16.0,
        );
        slime.size = 4;
        let anchors: std::collections::HashSet<[i32; 3]> = Default::default();
        let bundle = build_dimension_entities_from(
            std::slice::from_ref(&slime),
            &[],
            &[],
            6,
            1,
            1,
            &anchors,
        );
        assert_eq!(bundle.mobs.len(), 1);
        assert_eq!(bundle.mobs[0].size, 4, "size rides the save bundle");

        crate::world_save::entities::write_entities_in_dir(
            tmp.path(),
            "slot",
            "overworld",
            &bundle,
        )
        .unwrap();
        let loaded = crate::world_save::entities::read_entities_in_dir(
            tmp.path(),
            "slot",
            "overworld",
        )
        .unwrap()
        .expect("entities file exists");
        assert_eq!(loaded, bundle);

        let mut v = crate::Voxel::new();
        apply_dimension_entities(&mut v, loaded);
        assert_eq!(v.mobs.len(), 1);
        assert_eq!(v.mobs[0].size, 4, "size survives the round trip");
    }

    /// W476.1: the load side clamps saved sizes onto the MC 1|2|4
    /// domain (edited / corrupted saves).
    #[test]
    fn mob_size_clamps_to_slime_domain() {
        assert_eq!(clamp_mob_size(0), 1);
        assert_eq!(clamp_mob_size(1), 1);
        assert_eq!(clamp_mob_size(2), 2);
        assert_eq!(clamp_mob_size(3), 2);
        assert_eq!(clamp_mob_size(4), 4);
        assert_eq!(clamp_mob_size(200), 4);

        // End-to-end: an out-of-domain size in the bundle applies
        // clamped.
        let mut v = crate::Voxel::new();
        let mut bundle =
            crate::world_save::entities::DimensionEntities::empty();
        bundle.mobs.push(MobSave {
            id: 1,
            kind: 0,
            pos: [0.0, 65.0, 0.0],
            yaw: 0.0,
            vel: [0.0; 3],
            health: 9.0,
            age_seconds: 0.0,
            persistent: false,
            baby_left: 0.0,
            tamed: false,
            temper: 0,
            trade_xp: 0,
            sheared: false,
            size: 3,
            despawn_delay: 0,
        });
        bundle.next_mob_id = 2;
        apply_dimension_entities(&mut v, bundle);
        assert_eq!(v.mobs.len(), 1);
        assert_eq!(v.mobs[0].size, 2, "3 rounds down to 2");
    }
}
