//! W132 - thin-shape-aware block targeting.
//!
//! The voxel raycast treats every blocked cell as a FULL cube, so a
//! flat rail "filled" its whole cell for aiming: looking at the
//! grass beyond a rail still targeted the rail, and placing blocks
//! treated the rail like sturdy ground ("jako by to byl blok"). MC
//! ray-tests the block's real outline shape instead.
//!
//! `raycast_target` wraps the voxel raycast: when the hit cell holds
//! a THIN block (rails today), the ray is intersected with the
//! block's actual flat box; a miss continues the ray behind the
//! cell, exactly like MC's outline-shape picking. Bounded to a few
//! continuations so a rail yard can't make the ray scan forever.

use glam::Vec3;
use infengine_voxel::{RayHit, World, raycast_with};

/// Rail outline height (MC's flat-rail pick box is 2 px; a little
/// extra makes clicking the rail itself forgiving). Ascending rails
/// reuse it - their lower half remains clickable, the empty air
/// above clicks through.
const RAIL_PICK_HEIGHT: f32 = 0.25;

/// Ray / AABB intersection via the slab method. Returns the
/// `(t_enter, t_exit)` pair along `dir` (unnormalised ok) or `None`
/// when the ray misses the box.
fn ray_aabb(origin: Vec3, dir: Vec3, bmin: Vec3, bmax: Vec3) -> Option<(f32, f32)> {
    let mut t0 = f32::NEG_INFINITY;
    let mut t1 = f32::INFINITY;
    for axis in 0..3 {
        let (o, d) = (origin[axis], dir[axis]);
        let (mn, mx) = (bmin[axis], bmax[axis]);
        if d.abs() < 1e-8 {
            if o < mn || o > mx {
                return None;
            }
            continue;
        }
        let inv = 1.0 / d;
        let (mut ta, mut tb) = ((mn - o) * inv, (mx - o) * inv);
        if ta > tb {
            std::mem::swap(&mut ta, &mut tb);
        }
        t0 = t0.max(ta);
        t1 = t1.min(tb);
        if t0 > t1 {
            return None;
        }
    }
    Some((t0, t1))
}

/// Targeting raycast with thin-shape awareness (see module docs).
/// `is_blocked` matches the predicate handed to `raycast_with`.
pub(crate) fn raycast_target(
    world: &World,
    origin: Vec3,
    dir: Vec3,
    max_dist: f32,
    is_blocked: impl Fn(infengine_voxel::BlockId) -> bool,
) -> Option<RayHit> {
    let dir = dir.normalize_or_zero();
    if dir == Vec3::ZERO {
        return None;
    }
    let mut o = origin;
    let mut remaining = max_dist;
    // A handful of continuations covers any realistic rail run the
    // crosshair sweeps across.
    for _ in 0..8 {
        let hit = raycast_with(world, o, dir, remaining, &is_blocked)?;
        let b = world.get(hit.voxel);
        if !crate::block::rail::is_rail_block(b.0) {
            return Some(hit);
        }
        // Intersect with the rail's REAL flat box.
        let bmin = Vec3::new(
            hit.voxel[0] as f32,
            hit.voxel[1] as f32,
            hit.voxel[2] as f32,
        );
        let bmax = bmin + Vec3::new(1.0, RAIL_PICK_HEIGHT, 1.0);
        if let Some((t_in, _)) = ray_aabb(o, dir, bmin, bmax)
            && t_in >= 0.0
            && t_in <= remaining
        {
            return Some(hit);
        }
        // Missed the flat shape: continue behind this cell.
        let cell_max = bmin + Vec3::ONE;
        let Some((_, t_out)) = ray_aabb(o, dir, bmin, cell_max) else {
            return Some(hit);
        };
        let advance = t_out + 1e-3;
        if advance >= remaining {
            return None;
        }
        o += dir * advance;
        remaining -= advance;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use infengine_voxel::{BlockId, Chunk};

    fn world_with_rail_on_grass() -> World {
        let mut w = World::default();
        w.insert_chunk([0, 0, 0], Chunk::empty());
        w.insert_chunk([1, 0, 0], Chunk::empty());
        // Floor at y=3 spanning x 0..32.
        for x in 0..32 {
            w.set([x, 3, 4], BlockId(3));
        }
        // A rail at x=8 on top of the floor.
        w.set([8, 4, 4], BlockId(crate::block::RAIL_ID));
        w
    }

    fn blocked(b: BlockId) -> bool {
        !b.is_air()
    }

    #[test]
    fn grazing_ray_passes_over_the_rail() {
        let w = world_with_rail_on_grass();
        // Eye above the floor aiming shallowly over the rail toward
        // far floor: must NOT pick the rail cell.
        let origin = Vec3::new(4.5, 5.6, 4.5);
        let dir = Vec3::new(1.0, -0.18, 0.0);
        let hit = raycast_target(&w, origin, dir, 16.0, blocked).expect("hit");
        assert_ne!(hit.voxel, [8, 4, 4], "rail must not block the graze");
        assert_eq!(hit.voxel[1], 3, "should land on the floor beyond");
        assert!(hit.voxel[0] > 8, "beyond the rail, got {:?}", hit.voxel);
    }

    #[test]
    fn steep_ray_still_picks_the_rail() {
        let w = world_with_rail_on_grass();
        // Aim almost straight down at the rail.
        let origin = Vec3::new(8.5, 7.0, 4.5);
        let dir = Vec3::new(0.05, -1.0, 0.0);
        let hit = raycast_target(&w, origin, dir, 8.0, blocked).expect("hit");
        assert_eq!(hit.voxel, [8, 4, 4], "steep aim hits the rail itself");
    }

    #[test]
    fn solid_blocks_unaffected() {
        let mut w = world_with_rail_on_grass();
        // A solid wall column ahead of the eye.
        w.set([12, 4, 4], infengine_voxel::BlockId(1));
        w.set([12, 5, 4], infengine_voxel::BlockId(1));
        let origin = Vec3::new(4.5, 5.5, 4.5);
        let dir = Vec3::new(1.0, 0.0, 0.0);
        let hit = raycast_target(&w, origin, dir, 16.0, blocked).expect("hit");
        assert_eq!(hit.voxel, [12, 5, 4], "the wall blocks like always");
    }
}
