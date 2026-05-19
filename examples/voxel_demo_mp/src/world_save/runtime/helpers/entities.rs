//! Per-dim mob + drop save / apply. Builds `DimensionEntities`
//! from raw refs (so the per-dim save path can hand in either the
//! active dim's live state or a parked-dim snapshot) and writes
//! the bundle back onto `voxel` on load.

use crate::Voxel;
use crate::drops::DropItem;
use crate::mob::MobState;

use crate::world_save::entities::{
    DimensionEntities, DropSave, ENTITIES_FORMAT_VERSION, MobSave,
};

/// Build a `DimensionEntities` snapshot from raw refs. The
/// per-dim save path (`dims::save_*_dim`) hands in either
/// `voxel.mobs` / `voxel.dropped_items` (active dim) or the
/// parking maps (`voxel.entities_by_dim[dim]`).
pub(in crate::world_save::runtime) fn build_dimension_entities_from(
    mobs: &[MobState],
    drops: &[DropItem],
    next_mob_id: u32,
    next_drop_id: u32,
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
        })
        .collect();
    let drop_saves: Vec<DropSave> = drops
        .iter()
        .map(|d| DropSave {
            id: d.id,
            block: d.block,
            pos: d.pos,
            vel: d.vel,
            age: d.age,
        })
        .collect();
    DimensionEntities {
        format_version: ENTITIES_FORMAT_VERSION,
        mobs: mob_saves,
        drops: drop_saves,
        next_mob_id,
        next_drop_id,
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
    for m in entities.mobs {
        // Drop mobs with zero / negative health - they were dead on
        // last save and the runtime would only despawn them again.
        if m.health <= 0.0 {
            continue;
        }
        voxel.mobs.push(MobState::new_natural(
            m.id,
            m.kind,
            m.pos,
            m.pos,
            m.yaw,
            0.0,
            m.health,
        ));
    }
    for d in entities.drops {
        // Older saves don't carry the Q.5 fields; rehydrate as if
        // freshly spawned (count = 1, pickup-delay already
        // expired) so loading doesn't reset the pickup gate.
        let age_ticks = (d.age * 20.0).max(0.0) as u32;
        voxel.dropped_items.push(DropItem {
            id: d.id,
            block: d.block,
            pos: d.pos,
            vel: d.vel,
            age: d.age,
            count: 1,
            age_ticks,
            pickup_delay_ticks: 0,
        });
    }
    voxel.next_mob_id = entities
        .next_mob_id
        .max(voxel.mobs.iter().map(|m| m.id).max().unwrap_or(0) + 1);
    voxel.next_drop_id = entities
        .next_drop_id
        .max(voxel.dropped_items.iter().map(|d| d.id).max().unwrap_or(0) + 1);
}
