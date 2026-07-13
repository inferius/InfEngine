//! Test suite for `block::portal`.
//!
//! W5.2c: split out of `portal.rs` (810 LOC -> 496 prod + this
//! sibling) so each file lands well under the 500-LOC soft cap.
//! Mounted via `#[cfg(test)] #[path = "portal_tests.rs"] mod
//! tests;` at the bottom of `portal.rs`.
//!
//! Two test groups live here:
//! - `portal_pair_tests`: cross-dim pair_coord / pair_dim +
//!   find_portal_near + build_minimal_portal +
//!   ensure_return_portal.
//! - `tests` (kept the original name): try_ignite flood-fill
//!   ignition, rejection paths.

// Mounted from `portal.rs` via `#[path = "portal_tests.rs"] mod
// tests;`, so `super::*` resolves to the `portal` module itself.
use super::*;
use infengine_voxel::{BlockId, Chunk, World};

mod portal_pair_tests {
    use super::*;

    fn insert_neighbour_chunks(w: &mut World, range: i32) {
        for cx in -range..=range {
            for cy in -range..=range {
                for cz in -range..=range {
                    w.insert_chunk([cx, cy, cz], Chunk::empty());
                }
            }
        }
    }

    #[test]
    fn pair_coord_overworld_to_nether_scales_by_8() {
        assert_eq!(pair_coord("overworld", [80, 64, -16]), [10, 64, -2]);
        assert_eq!(pair_coord("overworld", [0, 64, 0]), [0, 64, 0]);
        // Euclidean div on negatives: -1 / 8 = -1 (NOT 0).
        assert_eq!(pair_coord("overworld", [-1, 64, -1]), [-1, 64, -1]);
    }

    #[test]
    fn pair_coord_nether_to_overworld_scales_by_8() {
        assert_eq!(pair_coord("nether", [10, 64, -2]), [80, 64, -16]);
        assert_eq!(pair_coord("nether", [0, 64, 0]), [0, 64, 0]);
        assert_eq!(pair_coord("nether", [-1, 64, -1]), [-8, 64, -8]);
    }

    #[test]
    fn pair_dim_inverts_when_called_twice() {
        let a = "overworld";
        let b = pair_dim(a);
        let c = pair_dim(b);
        assert_eq!(c, a);
    }

    #[test]
    fn find_portal_near_returns_closest() {
        let mut w = World::new();
        // Use y near 8 so a small chunk range still covers it.
        insert_neighbour_chunks(&mut w, 2);
        w.set([5, 8, 5], BlockId(PORTAL_ID));
        w.set([10, 8, 5], BlockId(PORTAL_ID));
        // Distance from [0, 8, 0]: first = 10 (5+0+5),
        // second = 15 (10+0+5). Closest = [5, 8, 5].
        let found = find_portal_near(&w, [0, 8, 0]);
        assert_eq!(found, Some([5, 8, 5]));
    }

    #[test]
    fn find_portal_near_returns_none_outside_radius() {
        let mut w = World::new();
        insert_neighbour_chunks(&mut w, 3);
        // Place a portal outside the search radius (>16
        // Manhattan distance from origin).
        w.set([20, 8, 20], BlockId(PORTAL_ID));
        let found = find_portal_near(&w, [0, 8, 0]);
        // 20 + 0 + 20 = 40 > 16 (radius); but the cube checks
        // each axis <= radius, so [20, 8, 20] is outside the
        // search cube too. Expect None.
        assert!(found.is_none(), "portal at 20,8,20 should be outside the search cube");
    }

    #[test]
    fn build_minimal_portal_creates_lit_frame() {
        let mut w = World::new();
        insert_neighbour_chunks(&mut w, 2);
        let edits = build_minimal_portal(&mut w, [0, 5, 5]);
        // Frame walls (10 obsidian) + interior (6 portal) = 16
        // cells total (corners are written too in this impl).
        assert!(edits.len() >= 16, "expected at least 16 edits, got {}", edits.len());
        // Interior is portal:
        for x in 0..=1 {
            for y in 5..=7 {
                assert_eq!(w.get([x, y, 5]).0, PORTAL_ID);
            }
        }
        // Walls are obsidian:
        assert_eq!(w.get([-1, 5, 5]).0, OBSIDIAN_ID);
        assert_eq!(w.get([2, 5, 5]).0, OBSIDIAN_ID);
        assert_eq!(w.get([0, 4, 5]).0, OBSIDIAN_ID);
        assert_eq!(w.get([0, 8, 5]).0, OBSIDIAN_ID);
    }

    #[test]
    fn ensure_return_portal_reuses_existing() {
        let mut w = World::new();
        insert_neighbour_chunks(&mut w, 2);
        w.set([3, 8, 3], BlockId(PORTAL_ID));
        // Target near the existing portal: no new frame should
        // be built; W155: the spawn coord is the portal cell
        // itself (arrive INSIDE like MC; the cool-down stops an
        // instant re-trigger).
        let mut edits: Vec<[i32; 3]> = Vec::new();
        let spawn = ensure_return_portal(&mut w, [0, 8, 0], Some(&mut edits));
        assert!(edits.is_empty(), "no edits expected when reusing existing portal");
        assert_eq!(spawn, [3, 8, 3]); // in-portal arrival
    }

    #[test]
    fn ensure_return_portal_builds_new_when_far() {
        let mut w = World::new();
        insert_neighbour_chunks(&mut w, 5);
        // No portal nearby - target [20, 8, 20] is far from any
        // existing portal. The fn must build a fresh frame there.
        let mut edits: Vec<[i32; 3]> = Vec::new();
        let _ = ensure_return_portal(&mut w, [20, 8, 20], Some(&mut edits));
        assert!(
            edits.len() >= 6,
            "expected portal-frame edits, got {} edits",
            edits.len()
        );
        // At least one portal block should now sit near the target.
        let found = find_portal_near(&w, [20, 8, 20]);
        assert!(found.is_some());
    }
}

mod ignite_tests {
    use super::*;

    /// Pre-insert every chunk that any of our test coords touches.
    /// Coords used in this test are in `[-1, ..]`; a 4-chunk grid
    /// at the 4 corners of the origin column covers them all.
    fn insert_neighbour_chunks(w: &mut World) {
        for cx in -1..=1 {
            for cy in -1..=1 {
                for cz in -1..=1 {
                    w.insert_chunk([cx, cy, cz], Chunk::empty());
                }
            }
        }
    }

    /// Build a small world that already contains an obsidian
    /// portal frame in the XY plane at z=fixed_z. Interior is
    /// 2 wide x 3 tall.
    fn world_with_2x3_frame_xy(fixed_z: i32) -> World {
        let mut w = World::new();
        insert_neighbour_chunks(&mut w);
        // Frame outer rect: x in [-1..=2], y in [-1..=3], at
        // z = fixed_z. Interior: x in 0..=1, y in 0..=2.
        // Walls (lay obsidian):
        //   bottom row: y=-1, x in -1..=2
        //   top row:    y=3,  x in -1..=2
        //   left col:   x=-1, y in -1..=3
        //   right col:  x=2,  y in -1..=3
        // Corners ARE placed for this test - the algorithm
        // doesn't require them but their presence shouldn't
        // affect the outcome.
        for x in -1..=2 {
            w.set([x, -1, fixed_z], BlockId(OBSIDIAN_ID));
            w.set([x, 3, fixed_z], BlockId(OBSIDIAN_ID));
        }
        for y in -1..=3 {
            w.set([-1, y, fixed_z], BlockId(OBSIDIAN_ID));
            w.set([2, y, fixed_z], BlockId(OBSIDIAN_ID));
        }
        w
    }

    /// W273: PortalShape rejects 2-tall interiors (MIN_HEIGHT 3) -
    /// the old engine wrongly accepted "3x2".
    fn world_with_3wide_2tall_frame_zy(fixed_x: i32) -> World {
        let mut w = World::new();
        insert_neighbour_chunks(&mut w);
        // Interior: z in 0..=2 (3 wide), y in 0..=1 (2 tall).
        // Outer: z in -1..=3, y in -1..=2.
        for z in -1..=3 {
            w.set([fixed_x, -1, z], BlockId(OBSIDIAN_ID));
            w.set([fixed_x, 2, z], BlockId(OBSIDIAN_ID));
        }
        for y in -1..=2 {
            w.set([fixed_x, y, -1], BlockId(OBSIDIAN_ID));
            w.set([fixed_x, y, 3], BlockId(OBSIDIAN_ID));
        }
        w
    }

    /// A valid Z-plane frame: 3 wide x 3 tall.
    fn world_with_3x3_frame_zy(fixed_x: i32) -> World {
        let mut w = World::new();
        insert_neighbour_chunks(&mut w);
        for z in -1..=3 {
            w.set([fixed_x, -1, z], BlockId(OBSIDIAN_ID));
            w.set([fixed_x, 3, z], BlockId(OBSIDIAN_ID));
        }
        for y in -1..=3 {
            w.set([fixed_x, y, -1], BlockId(OBSIDIAN_ID));
            w.set([fixed_x, y, 3], BlockId(OBSIDIAN_ID));
        }
        w
    }

    #[test]
    fn ignites_a_valid_2x3_xy_frame() {
        let mut w = world_with_2x3_frame_xy(5);
        let res = try_ignite(&mut w, [0, 0, 5]); // bottom-left interior
        assert!(res.is_ok(), "expected Ok, got {res:?}");
        if let IgniteResult::Ok { plane, cells } = &res {
            assert_eq!(*plane, PortalPlane::Xy);
            assert_eq!(cells.len(), 6);
        }
        // Every interior cell must now be PORTAL_ID.
        for x in 0..=1 {
            for y in 0..=2 {
                assert_eq!(
                    w.get([x, y, 5]).0,
                    PORTAL_ID,
                    "interior ({x},{y},5) should be portal"
                );
            }
        }
    }

    #[test]
    fn ignites_from_any_interior_cell_after_sinking_to_the_floor() {
        // MC's calculateBottomLeft drops from the fire cell to the
        // frame floor - igniting at the TOP of the opening works.
        let mut w = world_with_2x3_frame_xy(5);
        let res = try_ignite(&mut w, [1, 2, 5]);
        assert!(res.is_ok(), "expected Ok, got {res:?}");
    }

    #[test]
    fn rejects_a_2_tall_frame() {
        let mut w = world_with_3wide_2tall_frame_zy(7);
        let res = try_ignite(&mut w, [7, 0, 0]);
        assert!(
            matches!(
                res,
                IgniteResult::NotRectangular
            ),
            "2-tall interiors are below MIN_HEIGHT, got {res:?}"
        );
    }

    #[test]
    fn ignites_a_valid_3x3_zy_frame() {
        let mut w = world_with_3x3_frame_zy(7);
        let res = try_ignite(&mut w, [7, 0, 0]);
        assert!(res.is_ok(), "expected Ok, got {res:?}");
        if let IgniteResult::Ok { plane, cells } = &res {
            assert_eq!(*plane, PortalPlane::Zy);
            assert_eq!(cells.len(), 9);
        }
        for z in 0..=2 {
            for y in 0..=2 {
                assert_eq!(
                    w.get([7, y, z]).0,
                    PORTAL_ID,
                    "interior (7,{y},{z}) should be portal"
                );
            }
        }
    }

    #[test]
    fn rejects_frame_with_gap_in_perimeter() {
        let mut w = world_with_2x3_frame_xy(5);
        // Punch a hole in the top row.
        w.set([0, 3, 5], BlockId::AIR);
        let res = try_ignite(&mut w, [0, 0, 5]);
        assert!(
            matches!(
                res,
                IgniteResult::NotRectangular
            ),
            "expected ignition failure, got {res:?}"
        );
        // No interior cell should have become a portal.
        for x in 0..=1 {
            for y in 0..=2 {
                assert_ne!(w.get([x, y, 5]).0, PORTAL_ID);
            }
        }
    }

    #[test]
    fn rejects_1x1_air_hole() {
        // A single air voxel surrounded by obsidian - too small
        // to be a portal interior.
        let mut w = World::new();
        insert_neighbour_chunks(&mut w);
        let fixed_z = 0;
        // Frame around (0, 0, 0) interior cell.
        for x in -1..=1 {
            w.set([x, -1, fixed_z], BlockId(OBSIDIAN_ID));
            w.set([x, 1, fixed_z], BlockId(OBSIDIAN_ID));
        }
        for y in -1..=1 {
            w.set([-1, y, fixed_z], BlockId(OBSIDIAN_ID));
            w.set([1, y, fixed_z], BlockId(OBSIDIAN_ID));
        }
        let res = try_ignite(&mut w, [0, 0, fixed_z]);
        assert!(
            !res.is_ok(),
            "1x1 hole must not ignite, got {res:?}"
        );
    }

    #[test]
    fn already_portal_cell_is_noop() {
        let mut w = World::new();
        w.insert_chunk([0, 0, 0], Chunk::empty());
        w.set([0, 0, 0], BlockId(PORTAL_ID));
        let res = try_ignite(&mut w, [0, 0, 0]);
        assert_eq!(res, IgniteResult::AlreadyLit);
    }

    #[test]
    fn rejects_hit_cell_without_adjacent_obsidian() {
        // Empty world; the hit cell has no obsidian anywhere.
        let mut w = World::new();
        w.insert_chunk([0, 0, 0], Chunk::empty());
        let res = try_ignite(&mut w, [0, 0, 0]);
        assert_eq!(res, IgniteResult::NoAdjacentObsidian);
    }

    #[test]
    fn ignites_a_5x5_frame_like_mc() {
        // W273: PortalShape accepts up to 21x21 - the old engine's
        // 2x3-only rule wrongly rejected big frames.
        let mut w = World::new();
        insert_neighbour_chunks(&mut w);
        let fixed_z = 0;
        // Frame: x in [-1..=5], y in [-1..=5]. Interior 5 x 5.
        for x in -1..=5 {
            w.set([x, -1, fixed_z], BlockId(OBSIDIAN_ID));
            w.set([x, 5, fixed_z], BlockId(OBSIDIAN_ID));
        }
        for y in -1..=5 {
            w.set([-1, y, fixed_z], BlockId(OBSIDIAN_ID));
            w.set([5, y, fixed_z], BlockId(OBSIDIAN_ID));
        }
        let res = try_ignite(&mut w, [0, 0, fixed_z]);
        assert!(res.is_ok(), "5x5 must ignite, got {res:?}");
        if let IgniteResult::Ok { cells, .. } = &res {
            assert_eq!(cells.len(), 25);
        }
        for x in 0..=4 {
            for y in 0..=4 {
                assert_eq!(w.get([x, y, fixed_z]).0, PORTAL_ID);
            }
        }
    }
}
