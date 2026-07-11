//! Mob-targeting raycast used by the player's melee swing.
//!
//! Tests a forward ray against every known mob's AABB and returns
//! the nearest hit inside `max_dist`. Mirrors the slab-test path
//! the renderer already uses for cube targeting, just over the
//! mob list instead of the voxel grid.
//!
//! MC mirror: `net/minecraft/world/level/ClipContext.java` ->
//! `Level::clip` runs the same flavour of slab raycast against
//! every entity's bounding box.

use glam::Vec3;

use crate::Voxel;
use crate::player::ray_aabb;

use super::kinds::hitbox_for;

/// Raycast a slab-tested ray against every known mob's AABB.
/// Returns `Some((mob_id, distance))` for the nearest hit
/// inside `max_dist`, or `None`. Uses the rendered cube
/// extents (body 1.2x0.8x0.7 + head bump) approximated as one
/// bigger 1.2x1.4x0.8 box so the player doesn't have to pixel-
/// hunt the small head.
pub(crate) fn raycast_mob(
    voxel: &Voxel,
    origin: Vec3,
    dir: Vec3,
    max_dist: f32,
) -> Option<(u32, f32)> {
    let mut best: Option<(u32, f32)> = None;
    for m in voxel.remote_mobs.values() {
        // Per-kind hitbox (W242.2: table shared with the monster
        // spawner via `kinds::hitbox_for`). Sheep is wide + low,
        // zombie + skeleton are tall, chicken is a small cube near
        // the feet, spider is wide + low like a sheep, cow matches
        // the sheep baseline.
        let (hx, hy, hz, cy_off) = hitbox_for(m.kind);
        let cx = m.pos[0];
        let cy = m.pos[1] + cy_off;
        let cz = m.pos[2];
        let min = [cx - hx, cy - hy, cz - hz];
        let max = [cx + hx, cy + hy, cz + hz];
        if let Some(t) = ray_aabb(origin.into(), dir.into(), min, max)
            && t <= max_dist
            && best.is_none_or(|(_, bt)| t < bt)
        {
            best = Some((m.id, t));
        }
    }
    best
}
