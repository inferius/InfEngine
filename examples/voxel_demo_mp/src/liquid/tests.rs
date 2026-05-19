//! Behaviour tests for the liquid simulation. The pure helpers
//! (`liquid_surface_height_at`, `distance_to_drop_at`,
//! `liquid_corner_height_at`) are exercised directly; the tick loop
//! is driven through `LiquidSim`, a minimal wrapper around the same
//! `liquid_plan_step` / `liquid_apply_plan` / `liquid_merge_step`
//! free fns the live `Voxel::tick_liquid_cell` calls. Side effects
//! (renderer / network / drops / edit history) are skipped - we only
//! care about world state transitions here.

use super::*;
use super::mesh::{bottom_open, liquid_surface_height_at, side_open};
use super::plan::liquid_plan_cell;
use super::schedule::LiquidSchedule;
use crate::block::WATER_ID;
use infengine_voxel::{BlockId, Chunk, World};
use std::collections::{HashMap, HashSet};

/// Side-effect-free liquid simulator for tests. Holds the minimal
/// state set the pure planner/applier need, no renderer / network /
/// mesh hookups.
pub(crate) struct LiquidSim {
    pub(crate) world: World,
    pub(crate) level: HashMap<[i32; 3], u8>,
    pub(crate) active: HashSet<[i32; 3]>,
    pub(crate) positions: HashSet<[i32; 3]>,
    pub(crate) bias_drops: bool,
}

impl LiquidSim {
    pub(crate) fn new() -> Self {
        // 3x3x3 chunk grid around origin -> writable coords
        // [-16 .. 32) on each axis. Plenty of room for a 7-radius
        // pool + a 10-tall pillar.
        let mut w = World::new();
        for cx in -1..=1 {
            for cy in -1..=1 {
                for cz in -1..=1 {
                    w.insert_chunk([cx, cy, cz], Chunk::empty());
                }
            }
        }
        Self {
            world: w,
            level: HashMap::new(),
            active: HashSet::new(),
            positions: HashSet::new(),
            bias_drops: true,
        }
    }
    pub(crate) fn solid(&mut self, p: [i32; 3]) {
        self.world.set(p, BlockId(1));
    }
    pub(crate) fn fill_floor(
        &mut self,
        y: i32,
        x: std::ops::RangeInclusive<i32>,
        z: std::ops::RangeInclusive<i32>,
    ) {
        for xx in x {
            for zz in z.clone() {
                self.solid([xx, y, zz]);
            }
        }
    }
    pub(crate) fn place_source(&mut self, p: [i32; 3], liquid: &Liquid) {
        self.world.set(p, BlockId(liquid.block_id));
        self.level.insert(p, 0);
        self.active.insert(p);
        self.positions.insert(p);
    }
    pub(crate) fn remove_voxel(&mut self, p: [i32; 3]) {
        self.world.set(p, BlockId::AIR);
        self.level.remove(&p);
        self.active.remove(&p);
        self.positions.remove(&p);
        // Mirror `world_set_tracked` neighbour wake-up so adjacent
        // liquid is re-queued for the next tick.
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if dx == 0 && dy == 0 && dz == 0 {
                        continue;
                    }
                    let n = [p[0] + dx, p[1] + dy, p[2] + dz];
                    if self.positions.contains(&n) {
                        self.active.insert(n);
                    }
                }
            }
        }
    }
    pub(crate) fn tick(&mut self, liquid: &Liquid) {
        let plan = liquid_plan_step(
            &self.world,
            &self.level,
            &self.active,
            liquid,
            self.bias_drops,
        );
        let _ = liquid_apply_plan(
            &mut self.world,
            &mut self.level,
            &mut self.active,
            &mut self.positions,
            liquid,
            plan,
        );
        if liquid.infinite_source {
            let promoted = liquid_merge_step(&self.world, &self.level, liquid);
            for p in promoted {
                self.level.insert(p, 0);
            }
        }
    }
    pub(crate) fn run(&mut self, liquid: &Liquid, ticks: u32) {
        for _ in 0..ticks {
            self.tick(liquid);
        }
    }
    pub(crate) fn is(&self, p: [i32; 3], liquid: &Liquid) -> bool {
        self.world.get(p).0 == liquid.block_id
    }
    pub(crate) fn level_at(&self, p: [i32; 3]) -> Option<u8> {
        self.level.get(&p).copied()
    }
}

pub(crate) fn water() -> &'static Liquid {
    &LIQUIDS[0]
}
pub(crate) fn lava() -> &'static Liquid {
    &LIQUIDS[1]
}
fn mk_world() -> World {
    // Single chunk at the origin so coords [0..16) on each axis are
    // writable.
    let mut w = World::new();
    w.insert_chunk([0, 0, 0], Chunk::empty());
    w
}

// -----------------------------------------------------------------
// Pure helper tests: mesh height + drop BFS.
// -----------------------------------------------------------------

#[test]
fn surface_source_is_full_block() {
    let mut w = mk_world();
    w.set([5, 5, 5], BlockId(WATER_ID));
    let mut map = HashMap::new();
    map.insert([5, 5, 5], 0u8);
    assert!(
        (liquid_surface_height_at(&w, &map, [5, 5, 5], WATER_ID) - 1.0).abs()
            < 1e-4
    );
}

#[test]
fn surface_flow_levels_ramp_down() {
    let mut w = mk_world();
    w.set([5, 5, 5], BlockId(WATER_ID));
    let mut map = HashMap::new();
    for lvl in 1u8..=7 {
        map.insert([5, 5, 5], lvl);
        let h = liquid_surface_height_at(&w, &map, [5, 5, 5], WATER_ID);
        let expected = (8 - lvl as i32).max(1) as f32 / 9.0;
        assert!(
            (h - expected).abs() < 1e-4,
            "level {} -> {:.3}, want {:.3}",
            lvl,
            h,
            expected
        );
    }
}

#[test]
fn surface_falling_or_column_full_block() {
    let mut w = mk_world();
    w.set([5, 5, 5], BlockId(WATER_ID));
    w.set([5, 6, 5], BlockId(WATER_ID));
    let mut map = HashMap::new();
    map.insert([5, 5, 5], 7u8);
    let h = liquid_surface_height_at(&w, &map, [5, 5, 5], WATER_ID);
    assert!((h - 1.0).abs() < 1e-4);
    let mut w2 = mk_world();
    w2.set([5, 5, 5], BlockId(WATER_ID));
    let mut map2 = HashMap::new();
    map2.insert([5, 5, 5], 8u8);
    let h2 = liquid_surface_height_at(&w2, &map2, [5, 5, 5], WATER_ID);
    assert!((h2 - 1.0).abs() < 1e-4);
}

#[test]
fn distance_to_drop_immediate() {
    let w = mk_world();
    let lm: HashMap<[i32; 3], u8> = HashMap::new();
    assert_eq!(distance_to_drop_at(&w, &lm, WATER_ID, [5, 5, 5], 4), 0);
}

#[test]
fn distance_to_drop_solid_floor_unreachable() {
    let mut w = mk_world();
    for x in 0..16 {
        for z in 0..16 {
            w.set([x, 4, z], BlockId(1));
        }
    }
    let lm: HashMap<[i32; 3], u8> = HashMap::new();
    assert_eq!(distance_to_drop_at(&w, &lm, WATER_ID, [5, 5, 5], 4), u32::MAX);
}

#[test]
fn distance_to_drop_steps_to_cliff() {
    let mut w = mk_world();
    for x in 0..=8 {
        for z in 0..16 {
            w.set([x, 4, z], BlockId(1));
        }
    }
    let lm: HashMap<[i32; 3], u8> = HashMap::new();
    let d = distance_to_drop_at(&w, &lm, WATER_ID, [5, 5, 5], 6);
    assert!(d <= 4, "got {d}");
    assert!(d > 0);
}

#[test]
fn corner_height_all_water_flat() {
    let mut w = mk_world();
    for x in 4..=5 {
        for z in 4..=5 {
            w.set([x, 5, z], BlockId(WATER_ID));
        }
    }
    let mut map = HashMap::new();
    for x in 4..=5 {
        for z in 4..=5 {
            map.insert([x, 5, z], 0u8);
        }
    }
    let h = liquid_corner_height_at(&w, &map, 4, 5, 4, 1, 1, WATER_ID);
    assert!((h - 1.0).abs() < 1e-4, "got {h}");
}

#[test]
fn corner_height_isolated_cell_tapers_slightly() {
    let mut w = mk_world();
    w.set([5, 5, 5], BlockId(WATER_ID));
    let mut map = HashMap::new();
    map.insert([5, 5, 5], 0u8);
    let h = liquid_corner_height_at(&w, &map, 5, 5, 5, 0, 0, WATER_ID);
    assert!(
        (0.70..0.85).contains(&h),
        "isolated cell tip corner: got {h}, want ~0.77"
    );
    let h_far = liquid_corner_height_at(&w, &map, 5, 5, 5, 2, 2, WATER_ID);
    assert!(h_far.abs() < 1e-4, "non-touching corner: got {h_far}");
}

#[test]
fn corner_height_pool_joint_stays_high() {
    let mut w = mk_world();
    w.set([5, 5, 5], BlockId(WATER_ID));
    w.set([6, 5, 5], BlockId(WATER_ID));
    let mut map = HashMap::new();
    map.insert([5, 5, 5], 0u8);
    map.insert([6, 5, 5], 0u8);
    let joint = liquid_corner_height_at(&w, &map, 5, 5, 5, 1, 0, WATER_ID);
    let tip = liquid_corner_height_at(&w, &map, 5, 5, 5, 0, 0, WATER_ID);
    assert!(
        joint > tip,
        "joint corner ({joint}) should be higher than tip ({tip})"
    );
    assert!(
        (0.85..0.95).contains(&joint),
        "joint expected ~0.91, got {joint}"
    );
}

#[test]
fn corner_height_column_above_locks_full() {
    let mut w = mk_world();
    w.set([5, 5, 5], BlockId(WATER_ID));
    w.set([5, 6, 5], BlockId(WATER_ID));
    let mut map = HashMap::new();
    map.insert([5, 5, 5], 7u8);
    map.insert([5, 6, 5], 0u8);
    let h = liquid_corner_height_at(&w, &map, 5, 5, 5, 0, 0, WATER_ID);
    assert!((h - 1.0).abs() < 1e-4, "got {h}");
}

// -----------------------------------------------------------------
// Behaviour tests: full sim through LiquidSim.
// -----------------------------------------------------------------

#[test]
fn pour_on_flat_spreads_seven_blocks() {
    let mut sim = LiquidSim::new();
    sim.fill_floor(0, -12..=12, -12..=12);
    sim.place_source([0, 1, 0], water());
    sim.run(water(), 30);
    assert!(sim.is([7, 1, 0], water()), "+x distance 7 missing");
    assert!(sim.is([-7, 1, 0], water()), "-x distance 7 missing");
    assert!(sim.is([0, 1, 7], water()), "+z distance 7 missing");
    assert!(sim.is([0, 1, -7], water()), "-z distance 7 missing");
    assert!(!sim.is([8, 1, 0], water()), "+x distance 8 should stay air");
    assert!(!sim.is([0, 1, 8], water()), "+z distance 8 should stay air");
    assert_eq!(sim.level_at([0, 1, 0]), Some(0), "source corrupted");
}

/// Regression for Bug 1: `distance_to_drop_at` used to treat
/// same-liquid flow cells as walls, so once the source had
/// surrounded itself with flow water the BFS returned u32::MAX
/// in every direction. With every distance tied at MAX the bias
/// collapsed and the source spread radially - eastwards across
/// flat stone even when an air hole sat 4 cells west.
#[test]
fn pour_biases_toward_hole_through_existing_water() {
    let mut sim = LiquidSim::new();
    // Flat floor x in [-12..=12], z in [-12..=12], EXCEPT the
    // tile at (-4, 0, 0) is missing - that's the cliff drop.
    for x in -12..=12 {
        for z in -12..=12 {
            if x == -4 && z == 0 {
                continue;
            }
            sim.solid([x, 0, z]);
        }
    }
    sim.place_source([0, 1, 0], water());
    sim.run(water(), 30);
    // Sanity: water reached the hole and fell.
    assert!(
        sim.is([-4, 1, 0], water()),
        "water didn't reach the hole on the west side"
    );
    // East side stays dry - the source's east BFS sees no drop
    // and the west BFS (through existing water) wins.
    for x in 1..=7 {
        let p = [x, 1, 0];
        assert!(
            !sim.is(p, water()),
            "east cell {p:?} should stay air with hole-bias active"
        );
    }
}

/// Regression for Bug 2: source on a 1x1 stone perch surrounded
/// by air must spread sideways onto the perch's NSEW (where below
/// is air = cliff drop). Those side flows then cascade falling all
/// the way down. Catches a regression where horizontal spread over
/// a cliff edge gets dropped because the perch's `below` blocks
/// but every side is a drop.
#[test]
fn source_on_perch_pours_over_every_side() {
    let mut sim = LiquidSim::new();
    // Floor far below so falling water lands somewhere.
    sim.fill_floor(-5, -6..=6, -6..=6);
    // 1x1 stone perch at (0,0,0). Air everywhere else around the
    // perch column above the floor.
    sim.solid([0, 0, 0]);
    sim.place_source([0, 1, 0], water());
    sim.run(water(), 30);
    assert_eq!(sim.level_at([0, 1, 0]), Some(0), "source vanished");
    // Every side spawns a falling column from y=0 down to y=-4
    // (the floor sits at y=-5).
    let dirs: [[i32; 2]; 4] =
        [[1, 0], [-1, 0], [0, 1], [0, -1]];
    for d in dirs {
        for y in -4..=0 {
            let p = [d[0], y, d[1]];
            assert!(
                sim.is(p, water()),
                "expected water at {p:?} (cliff-edge cascade)"
            );
        }
    }
}

#[test]
fn pour_off_pillar_falls_then_pools() {
    let mut sim = LiquidSim::new();
    sim.fill_floor(-2, -8..=8, -8..=8);
    for y in -1..=10 {
        sim.solid([0, y, 0]);
    }
    sim.place_source([0, 11, 0], water());
    sim.run(water(), 60);
    assert_eq!(sim.level_at([0, 11, 0]), Some(0));
    assert!(sim.is([1, 5, 0], water()), "+x column gap at y=5");
    assert!(
        !sim.is([2, 5, 0], water()),
        "column fanned out mid-air at +x"
    );
    assert!(
        !sim.is([1, 5, 1], water()),
        "column fanned out mid-air at +z of +x column"
    );
    assert!(sim.is([1, -1, 0], water()), "+x base pool missing");
    assert!(sim.is([-1, -1, 0], water()), "-x base pool missing");
    assert!(sim.is([0, -1, 1], water()), "+z base pool missing");
}

#[test]
fn two_sources_closed_one_three_make_middle_source() {
    let mut sim = LiquidSim::new();
    for x in 0..=2 {
        sim.solid([x, -1, 0]);
        sim.solid([x, 0, 1]);
        sim.solid([x, 0, -1]);
    }
    sim.solid([-1, 0, 0]);
    sim.solid([3, 0, 0]);
    sim.place_source([0, 0, 0], water());
    sim.place_source([2, 0, 0], water());
    sim.run(water(), 10);
    assert!(sim.is([1, 0, 0], water()), "middle cell didn't fill");
    assert_eq!(
        sim.level_at([1, 0, 0]),
        Some(0),
        "middle cell didn't promote to source"
    );
}

#[test]
fn two_sources_open_drain_no_middle_source() {
    let mut sim = LiquidSim::new();
    for x in 0..=2 {
        sim.solid([x, -1, 0]);
        sim.solid([x, 0, -1]);
    }
    sim.solid([0, 0, 1]);
    sim.solid([2, 0, 1]);
    sim.solid([-1, 0, 0]);
    sim.solid([3, 0, 0]);
    sim.place_source([0, 0, 0], water());
    sim.place_source([2, 0, 0], water());
    sim.run(water(), 20);
    assert!(sim.is([1, 0, 0], water()), "middle cell didn't fill");
    assert_ne!(
        sim.level_at([1, 0, 0]),
        Some(0),
        "middle wrongly promoted despite open drain"
    );
}

#[test]
fn source_removal_drains_gradually() {
    let mut sim = LiquidSim::new();
    sim.fill_floor(0, -10..=10, -10..=10);
    sim.place_source([0, 1, 0], water());
    sim.run(water(), 30);
    assert!(
        sim.is([5, 1, 0], water()),
        "pool didn't fill before source removal"
    );
    sim.remove_voxel([0, 1, 0]);
    sim.run(water(), 80);
    for x in -8..=8 {
        for z in -8..=8 {
            let p = [x, 1, z];
            assert!(!sim.is(p, water()), "{p:?} still water after drain");
        }
    }
}

#[test]
fn source_on_cliff_no_flicker() {
    let mut sim = LiquidSim::new();
    sim.fill_floor(-2, -6..=6, -6..=6);
    sim.solid([0, 9, 0]);
    sim.place_source([0, 10, 0], water());
    sim.run(water(), 30);
    for _ in 0..10 {
        sim.tick(water());
        assert!(sim.is([0, 10, 0], water()), "source itself vanished");
        assert!(
            sim.is([1, 10, 0], water()),
            "+x falling cell flickered"
        );
    }
}

#[test]
fn source_above_air_no_flicker() {
    let mut sim = LiquidSim::new();
    sim.fill_floor(-2, -6..=6, -6..=6);
    sim.place_source([0, 5, 0], water());
    sim.run(water(), 15);
    for _ in 0..10 {
        sim.tick(water());
        assert!(
            sim.is([0, 4, 0], water()),
            "cell directly below source flickered (source-on-cliff bug)"
        );
    }
}

#[test]
fn two_streams_meet_strongest_wins() {
    let mut sim = LiquidSim::new();
    sim.fill_floor(0, -10..=10, -10..=10);
    sim.place_source([-3, 1, 0], water());
    sim.place_source([4, 1, 0], water());
    sim.run(water(), 30);
    let lvl = sim.level_at([0, 1, 0]).expect("meeting cell empty");
    assert_eq!(
        lvl, 3,
        "expected stronger (-3) side to win with level 3, got {lvl}"
    );
}

// -----------------------------------------------------------------
// Regression suite for the source-removal cliff drainage bug. Caught
// the original push-based oscillation; kept here so a future tweak
// to the spread / pull logic can't reintroduce it.
// -----------------------------------------------------------------

#[test]
fn source_removal_drains_with_cliff_edge_nearby() {
    // Pool on a plateau with a cliff edge nearby. Player removes
    // the source; the whole pool must evaporate even though some
    // cells have an open `below` (cliff edge) and would otherwise
    // get push-recreated by their non-cliff neighbours.
    let mut sim = LiquidSim::new();
    // Plateau floor at y=0, x in [-10..=2], z in [-10..=10]. The +x
    // edge falls off after x=2 - cliff at x=3, basin floor at y=-5.
    for x in -10..=2 {
        for z in -10..=10 {
            sim.solid([x, 0, z]);
        }
    }
    // Basin floor below the cliff so falling water lands. Side
    // walls block water from creating a parasitic falling column
    // off the basin's z edges.
    for x in 3..=10 {
        for z in -10..=10 {
            sim.solid([x, -5, z]);
        }
    }
    for x in 3..=10 {
        for y in -5..=0 {
            sim.solid([x, y, -11]);
            sim.solid([x, y, 11]);
        }
    }
    sim.place_source([0, 1, 0], water());
    sim.run(water(), 40);
    // Sanity: pool reached the plateau edge.
    assert!(sim.is([2, 1, 0], water()), "pool didn't reach plateau edge");
    sim.remove_voxel([0, 1, 0]);
    sim.run(water(), 120);
    // Plateau drained.
    for x in -2..=2 {
        for z in -2..=2 {
            let p = [x, 1, z];
            assert!(
                !sim.is(p, water()),
                "plateau cell {p:?} still water after drain"
            );
        }
    }
    // Falling column gone.
    for y in -4..=0 {
        let p = [3, y, 0];
        assert!(
            !sim.is(p, water()),
            "cliff falling cell {p:?} still water after drain"
        );
    }
}

/// Regression for Bug 3: source-removal drain on a plateau with
/// a cliff drop adjacent. After the source is gone every flow cell
/// must evaporate, including the cells whose path back to the
/// origin runs through other flow cells (so the BFS used by spread
/// must traverse same-liquid, fixed in Bug 1). Catches a stall
/// where the recompute-pull halted because spread couldn't
/// re-evaluate the cliff-side cells without that traversal.
#[test]
fn source_removal_drains_through_multi_dir_cliff() {
    let mut sim = LiquidSim::new();
    // 11x11 plateau at y=0 centred on origin, with a 1-block
    // cliff drop in +X starting at x=4 (y=0 is air for x in 4..).
    // Basin floor far below so falling water lands and doesn't
    // wrap underneath the plateau via a phantom path.
    for x in -5..=3 {
        for z in -5..=5 {
            sim.solid([x, 0, z]);
        }
    }
    for x in 4..=10 {
        for z in -5..=5 {
            sim.solid([x, -5, z]);
        }
    }
    sim.place_source([0, 1, 0], water());
    sim.run(water(), 30);
    // Sanity: pool reached the cliff edge.
    assert!(sim.is([3, 1, 0], water()), "pool didn't reach plateau edge");
    sim.remove_voxel([0, 1, 0]);
    sim.run(water(), 100);
    // Plateau fully drained.
    for x in -5..=3 {
        for z in -5..=5 {
            let p = [x, 1, z];
            assert!(
                !sim.is(p, water()),
                "plateau cell {p:?} still water after drain"
            );
        }
    }
    // Cliff falling column gone.
    for y in -4..=0 {
        let p = [4, y, 0];
        assert!(
            !sim.is(p, water()),
            "cliff falling cell {p:?} still water after drain"
        );
    }
}

#[test]
fn two_pools_remove_one_drains_only_that_one() {
    // Two independent pools separated by a solid wall. Removing
    // the first pool's source must NOT drain the second.
    let mut sim = LiquidSim::new();
    sim.fill_floor(0, -10..=10, -10..=10);
    // Wall between them at x=0, full height.
    for y in 1..=3 {
        for z in -10..=10 {
            sim.solid([0, y, z]);
        }
    }
    sim.place_source([-5, 1, 0], water());
    sim.place_source([5, 1, 0], water());
    sim.run(water(), 40);
    assert!(sim.is([-3, 1, 0], water()), "left pool didn't fill");
    assert!(sim.is([3, 1, 0], water()), "right pool didn't fill");
    sim.remove_voxel([-5, 1, 0]);
    sim.run(water(), 80);
    // Left pool gone.
    for x in -8..=-1 {
        for z in -8..=8 {
            let p = [x, 1, z];
            assert!(!sim.is(p, water()), "left pool {p:?} still water");
        }
    }
    // Right pool intact.
    assert!(sim.is([5, 1, 0], water()), "right source vanished!");
    assert!(sim.is([3, 1, 0], water()), "right pool drained spuriously");
    assert!(sim.is([7, 1, 0], water()), "right pool edge drained spuriously");
}

// -----------------------------------------------------------------
// Mesh emission tests: `side_open` + `bottom_open` predicates that
// gate face generation in `Voxel::maybe_rebuild_liquid_mesh`.
// -----------------------------------------------------------------

/// Count the side faces that would be emitted around a cell at
/// (vx, vy, vz). Matches the four `side_open` checks in the mesh
/// build loop. Used to assert "fully exposed cell -> 4 side faces".
fn count_side_faces(world: &World, p: [i32; 3], liquid_id: u16) -> u32 {
    let [vx, vy, vz] = p;
    let mut n = 0;
    if side_open(world, vx - 1, vy, vz, liquid_id) { n += 1; }
    if side_open(world, vx + 1, vy, vz, liquid_id) { n += 1; }
    if side_open(world, vx, vy, vz - 1, liquid_id) { n += 1; }
    if side_open(world, vx, vy, vz + 1, liquid_id) { n += 1; }
    n
}

#[test]
fn mesh_fully_exposed_cube_emits_four_sides() {
    // Isolated water cube in open air. All 4 NSEW are air -> all
    // 4 side faces must be emitted, and the bottom face too
    // (cliff-edge water below = air -> emit so the underwater
    // view doesn't see plain sky).
    let mut w = mk_world();
    w.set([5, 5, 5], BlockId(WATER_ID));
    assert_eq!(
        count_side_faces(&w, [5, 5, 5], WATER_ID),
        4,
        "exposed cube must emit 4 side faces"
    );
    assert!(
        bottom_open(&w, 5, 4, 5, WATER_ID),
        "cliff-edge water (air below) must emit bottom face"
    );
}

#[test]
fn mesh_buried_cube_emits_no_sides() {
    // Water cube surrounded by water on every face. No side
    // emission - the shared faces sit inside the body.
    let mut w = mk_world();
    for x in 4..=6 {
        for y in 4..=6 {
            for z in 4..=6 {
                w.set([x, y, z], BlockId(WATER_ID));
            }
        }
    }
    assert_eq!(
        count_side_faces(&w, [5, 5, 5], WATER_ID),
        0,
        "buried cube must emit zero side faces"
    );
    assert!(
        !bottom_open(&w, 5, 4, 5, WATER_ID),
        "same-liquid below cancels the bottom face"
    );
}

#[test]
fn mesh_partial_corner_emits_facing_faces() {
    // Water cell at (5,5,5) with another water at (5,5,6). The
    // shared +z face is culled; the other 3 sides emit.
    let mut w = mk_world();
    w.set([5, 5, 5], BlockId(WATER_ID));
    w.set([5, 5, 6], BlockId(WATER_ID));
    let faces = count_side_faces(&w, [5, 5, 5], WATER_ID);
    assert_eq!(
        faces, 3,
        "two-cell pool: outer cell must emit 3 side faces, got {faces}"
    );
    // The +z face specifically is the one culled.
    assert!(
        !side_open(&w, 5, 5, 6, WATER_ID),
        "shared +z face must be culled (same liquid neighbour)"
    );
    assert!(
        side_open(&w, 5, 5, 4, WATER_ID),
        "non-shared -z face must emit"
    );
}

#[test]
fn mesh_solid_neighbour_culls_face() {
    // Water cube against a stone wall on the +x side. +x face is
    // culled (wall covers it); other 3 sides emit.
    let mut w = mk_world();
    w.set([5, 5, 5], BlockId(WATER_ID));
    w.set([6, 5, 5], BlockId(1)); // dirt = solid, no F_AABB_PASS
    let faces = count_side_faces(&w, [5, 5, 5], WATER_ID);
    assert_eq!(
        faces, 3,
        "water-vs-wall: expected 3 faces (no +x face), got {faces}"
    );
    assert!(
        !side_open(&w, 6, 5, 5, WATER_ID),
        "solid neighbour must cull the face"
    );
}

/// Regression for Bug 4: water side face against a snow layer
/// (or any `F_DEDICATED_RENDER` neighbour without `F_AABB_PASS`)
/// must still emit. Otherwise the pool's edge above a snow layer
/// renders as a hard cull where the player expects the water's
/// outer face.
#[test]
fn mesh_side_face_against_snow_and_torch_open() {
    let mut w = mk_world();
    w.set([5, 5, 5], BlockId(WATER_ID));
    // Snow (id 15) has F_DEDICATED_RENDER but not F_AABB_PASS.
    w.set([6, 5, 5], BlockId(15));
    assert!(
        side_open(&w, 6, 5, 5, WATER_ID),
        "side face against snow must emit (dedicated render path)"
    );
    // Torch (id 10) has F_AABB_PASS - was already covered before
    // the fix, but locks in regression coverage so a future tweak
    // can't drop decorations alongside snow.
    w.set([4, 5, 5], BlockId(10));
    assert!(
        side_open(&w, 4, 5, 5, WATER_ID),
        "side face against torch must emit"
    );
    // Flower (id 20) - also passable decoration.
    w.set([5, 5, 6], BlockId(20));
    assert!(
        side_open(&w, 5, 5, 6, WATER_ID),
        "side face against flower must emit"
    );
}

#[test]
fn mesh_bottom_face_emitted_only_when_open() {
    let mut w = mk_world();
    w.set([5, 5, 5], BlockId(WATER_ID));
    // Air below -> emit.
    assert!(
        bottom_open(&w, 5, 4, 5, WATER_ID),
        "air below: emit bottom face"
    );
    // Solid below -> skip.
    w.set([5, 4, 5], BlockId(1));
    assert!(
        !bottom_open(&w, 5, 4, 5, WATER_ID),
        "solid below: no bottom face"
    );
    // Same-liquid below -> skip (would be inside body).
    w.set([5, 4, 5], BlockId(WATER_ID));
    assert!(
        !bottom_open(&w, 5, 4, 5, WATER_ID),
        "same-liquid below: no bottom face"
    );
}

/// Behavioural test for Bug 5 - source on top of a corner leaf
/// in a 3x3 canopy. Leaves are solid (no `F_WASHABLE`, no
/// `F_AABB_PASS`) so the source cannot spread INTO the canopy
/// cells at its own y-level. But its NSEW at y=source.y are AIR
/// (the air column above each adjacent leaf, AND the air past
/// the canopy edge), so spread fires there and cascades down off
/// the canopy.
///
/// This matches MC: water dropped on top of leaves doesn't soak
/// through, but it still pours off the sides. Locking in the
/// expected behaviour so a future tweak to `can_hold_liquid` /
/// the spread gate can't regress to "water trapped in a hole
/// between leaves".
#[test]
fn source_on_canopy_corner_pours_off_the_sides() {
    let mut sim = LiquidSim::new();
    // 3x3 leaf canopy at y=5 centred on origin. Leaves are id 6
    // (flags=0 -> solid, not washable, not aabb-passable).
    for x in -1..=1 {
        for z in -1..=1 {
            sim.world.set([x, 5, z], BlockId(6));
            sim.positions.insert([x, 5, z]);
        }
    }
    // Floor far below so falling water lands somewhere.
    sim.fill_floor(-5, -6..=6, -6..=6);
    // Source on top of the (-1,-1) corner leaf.
    sim.place_source([-1, 6, -1], water());
    sim.run(water(), 20);
    // Source intact.
    assert_eq!(
        sim.level_at([-1, 6, -1]),
        Some(0),
        "source corrupted"
    );
    // Off-canopy NSEW gets water and cascades down.
    assert!(
        sim.is([-2, 6, -1], water()),
        "spread to -x air side missing"
    );
    assert!(
        sim.is([-1, 6, -2], water()),
        "spread to -z air side missing"
    );
    // Cascade reached the cells below the canopy edge.
    assert!(
        sim.is([-2, 5, -1], water()),
        "fall column off -x canopy edge missing"
    );
    assert!(
        sim.is([-1, 5, -2], water()),
        "fall column off -z canopy edge missing"
    );
    // Source CANNOT spread into the canopy itself - leaves are
    // not washable, not air. Cells at the source's own y above
    // the other canopy leaves (e.g. (0,6,-1)) MAY get water via
    // a later spread (their below is leaf so they're not a drop,
    // but the BFS lets them through as air cells). That's OK -
    // what we lock in is that the canopy itself stays solid leaf.
    for x in -1..=1 {
        for z in -1..=1 {
            assert_eq!(
                sim.world.get([x, 5, z]).0,
                6,
                "canopy leaf at ({x},5,{z}) got washed away"
            );
        }
    }
}

#[test]
fn waterfall_drains_top_down_after_source_removal() {
    // Source on top, water falls off a tall cliff, pools at the
    // base. Remove the top source -> the entire stream + basin
    // pool must evaporate.
    let mut sim = LiquidSim::new();
    sim.solid([0, 4, 0]);
    sim.fill_floor(-2, -6..=6, -6..=6);
    sim.place_source([0, 5, 0], water());
    sim.run(water(), 30);
    assert!(sim.is([0, 3, 0], water()) || sim.is([1, 3, 0], water()));
    assert!(
        sim.is([1, -1, 0], water()) || sim.is([-1, -1, 0], water()),
        "basin pool didn't form"
    );
    sim.remove_voxel([0, 5, 0]);
    sim.run(water(), 100);
    // Column gone.
    for y in 0..=5 {
        for x in -1..=1 {
            for z in -1..=1 {
                let p = [x, y, z];
                if sim.is(p, water()) {
                    panic!("waterfall cell {p:?} still water after drain");
                }
            }
        }
    }
    // Basin gone.
    for x in -4..=4 {
        for z in -4..=4 {
            let p = [x, -1, z];
            assert!(!sim.is(p, water()), "basin {p:?} still water");
        }
    }
}

// -----------------------------------------------------------------
// Schedule-driven sim regression tests. Mirror the live Voxel
// model: each cell is processed via `liquid_plan_cell` when the
// schedule says it's due, then placed/decayed cells + their 6
// neighbours get rescheduled. The 8192 bulk-cap is gone, so the
// only sane way to verify "no cell is dropped" is to run a fixed
// budget per frame and count how many cells got ticked.
// -----------------------------------------------------------------

/// Side-effect-free scheduled sim. Same shape as the live Voxel's
/// `tick_liquid_cell` loop, minus the network / mesh / drop wiring.
/// Tracks which cells got ticked via a `HashMap<pos, count>` for
/// the no-cell-skipped assertions.
struct ScheduledSim {
    world: World,
    level: HashMap<[i32; 3], u8>,
    positions: HashSet<[i32; 3]>,
    schedule: LiquidSchedule,
    clock: u64,
    /// Per-position tick count - lets a regression test prove every
    /// cell processed at least once across the run.
    tick_counts: HashMap<[i32; 3], u32>,
}

impl ScheduledSim {
    fn new() -> Self {
        let mut w = World::new();
        for cx in -1..=1 {
            for cy in -1..=1 {
                for cz in -1..=1 {
                    w.insert_chunk([cx, cy, cz], Chunk::empty());
                }
            }
        }
        Self {
            world: w,
            level: HashMap::new(),
            positions: HashSet::new(),
            schedule: LiquidSchedule::new(),
            clock: 0,
            tick_counts: HashMap::new(),
        }
    }
    fn solid(&mut self, p: [i32; 3]) {
        self.world.set(p, BlockId(1));
    }
    fn place_source(&mut self, p: [i32; 3], liquid: &Liquid) {
        self.world.set(p, BlockId(liquid.block_id));
        self.level.insert(p, 0);
        self.positions.insert(p);
        self.schedule
            .schedule(p, self.clock + liquid.tick_period.max(1) as u64);
    }
    /// Mirror of `Voxel::world_set_tracked` for tests: when the
    /// edit is liquid, schedule the new cell; either way, wake
    /// adjacent liquid neighbours with delay 1.
    fn set_block(&mut self, p: [i32; 3], block: BlockId, liquid: &Liquid) {
        self.world.set(p, block);
        if block.0 == liquid.block_id {
            self.schedule
                .schedule(p, self.clock + liquid.tick_period.max(1) as u64);
            self.level.entry(p).or_insert(0);
            self.positions.insert(p);
        } else {
            self.level.remove(&p);
            self.positions.remove(&p);
            self.schedule.cancel(p);
        }
        // 3x3x3 neighbour wake.
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if dx == 0 && dy == 0 && dz == 0 {
                        continue;
                    }
                    let n = [p[0] + dx, p[1] + dy, p[2] + dz];
                    if self.positions.contains(&n) {
                        self.schedule.schedule(n, self.clock + 1);
                    }
                }
            }
        }
    }
    /// Process all due cells (no per-frame budget here - tests run
    /// fast enough to drain the schedule every tick).
    fn step(&mut self, liquid: &Liquid) {
        self.clock = self.clock.wrapping_add(1);
        let now = self.clock;
        let delay = liquid.tick_period.max(1) as u64;
        let liquid_id = liquid.block_id;
        let dirs: [[i32; 3]; 6] = [
            [1, 0, 0],
            [-1, 0, 0],
            [0, 0, 1],
            [0, 0, -1],
            [0, 1, 0],
            [0, -1, 0],
        ];
        while let Some(pos) = self.schedule.pop_due(now) {
            if self.world.get(pos).0 != liquid_id {
                continue;
            }
            *self.tick_counts.entry(pos).or_insert(0) += 1;
            let mut plan = super::plan::LiquidPlanStep::default();
            let mut updates: HashMap<[i32; 3], (u8, bool)> = HashMap::new();
            liquid_plan_cell(
                &self.world,
                &self.level,
                pos,
                liquid,
                true,
                &mut plan,
                &mut updates,
            );
            plan.place = updates.into_iter().map(|(p, (l, f))| (p, l, f)).collect();
            if plan.evaporate.is_empty()
                && plan.decay.is_empty()
                && plan.place.is_empty()
            {
                continue;
            }
            let mut scratch = HashSet::new();
            let tape = liquid_apply_plan(
                &mut self.world,
                &mut self.level,
                &mut scratch,
                &mut self.positions,
                liquid,
                plan,
            );
            // Re-schedule placed/decayed/evaporated cells + 6
            // neighbours, mirroring the live tick_liquid_cell.
            for p in &tape.evaporated {
                for d in &dirs {
                    let n = [p[0] + d[0], p[1] + d[1], p[2] + d[2]];
                    if self.world.get(n).0 == liquid_id {
                        self.schedule.schedule(n, now + delay);
                    }
                }
            }
            for (p, _, _) in &tape.placed {
                self.schedule.schedule(*p, now + delay);
                for d in &dirs {
                    let n = [p[0] + d[0], p[1] + d[1], p[2] + d[2]];
                    if self.world.get(n).0 == liquid_id {
                        self.schedule.schedule(n, now + delay);
                    }
                }
            }
            for (p, _) in &tape.decayed {
                self.schedule.schedule(*p, now + delay);
                for d in &dirs {
                    let n = [p[0] + d[0], p[1] + d[1], p[2] + d[2]];
                    if self.world.get(n).0 == liquid_id {
                        self.schedule.schedule(n, now + delay);
                    }
                }
            }
        }
    }
    fn run(&mut self, liquid: &Liquid, frames: u32) {
        for _ in 0..frames {
            self.step(liquid);
        }
    }
}

/// 1000 source cells in a 32x32-tile pool (plus a flat floor under
/// them). After 50 schedule sweeps every source must have ticked
/// at least once - the old 8192 bulk cap got replaced by an
/// unbounded heap, so no cell is silently skipped any longer.
#[test]
fn large_pool_no_cells_skipped() {
    let mut sim = ScheduledSim::new();
    // Stone floor under the pool.
    for x in -16..16 {
        for z in -16..16 {
            sim.solid([x, 0, z]);
        }
    }
    let mut placed: Vec<[i32; 3]> = Vec::with_capacity(1024);
    // 32x32 = 1024 source cells > 1000.
    for x in -16..16 {
        for z in -16..16 {
            let p = [x, 1, z];
            sim.place_source(p, water());
            placed.push(p);
        }
    }
    assert!(placed.len() >= 1000, "test setup needs >=1000 cells");
    sim.run(water(), 50);
    // Every placed source must have processed at least one tick
    // across the 50 frames - the schedule has no per-frame cap in
    // this test rig.
    for p in &placed {
        let ticks = sim.tick_counts.get(p).copied().unwrap_or(0);
        assert!(
            ticks >= 1,
            "source cell {p:?} was never ticked across 50 frames \
             (regression: bulk cap reintroduced?)"
        );
    }
}

/// A small pool ticks until it settles, then the schedule must
/// drain to zero. The old bulk model kept every cell in the
/// `liquid_active` set forever and re-processed them every
/// `tick_period` frames; the new per-cell model lets settled
/// cells fall off the schedule once nothing changes.
#[test]
fn settled_pool_schedule_empty() {
    let mut sim = ScheduledSim::new();
    // 9x9 floor + a single source. Source will spread out to
    // distance 7, settle, and stop scheduling itself.
    for x in -8..=8 {
        for z in -8..=8 {
            sim.solid([x, 0, z]);
        }
    }
    sim.place_source([0, 1, 0], water());
    // Plenty of frames for the 7-radius pool to fully settle.
    sim.run(water(), 80);
    // Sanity: the pool reached the +x edge.
    assert!(
        sim.world.get([7, 1, 0]).0 == water().block_id,
        "pool didn't spread before settling check"
    );
    // The schedule must be empty - no cell has a pending tick.
    assert_eq!(
        sim.schedule.len(),
        0,
        "settled pool left {} cells in the schedule",
        sim.schedule.len()
    );
}

/// A source far from any edit is settled. A `set_block` 4 cells
/// away on the floor under the pool must wake the adjacent liquid
/// neighbour cells - matches MC's `scheduleTick(neighbour, fluid,
/// 1)` on `neighborChanged`. Without this wake, edits to the
/// world's topology would freeze for a full `tick_period` before
/// the spread re-evaluates.
#[test]
fn edit_near_liquid_wakes_neighbours() {
    let mut sim = ScheduledSim::new();
    for x in -8..=8 {
        for z in -8..=8 {
            sim.solid([x, 0, z]);
        }
    }
    sim.place_source([0, 1, 0], water());
    sim.run(water(), 80);
    assert_eq!(sim.schedule.len(), 0, "pool never settled before edit");
    // Pick the liquid cell at distance 4 from the source.
    let liq_pos = [4, 1, 0];
    assert!(sim.positions.contains(&liq_pos), "spread didn't reach (4,1,0)");
    // Edit a cell 1 step further away (5, 1, 0 is also liquid;
    // edit a *non-liquid* solid block at (4, 0, 0) -- under the
    // liquid - which is 4 cells away from (0,1,0) in horizontal
    // distance via the floor edit). The 3x3 neighbour wake on
    // the edit should pull liq_pos into the schedule.
    let edit_pos = [4, 0, 0];
    sim.set_block(edit_pos, BlockId::AIR, water());
    // The liquid cell at distance 4 (and its neighbours) must
    // now be scheduled, with a delay of 1..tick_period.
    assert!(
        sim.schedule.contains(liq_pos),
        "edit at {edit_pos:?} didn't wake liquid at {liq_pos:?}"
    );
    // Drain the schedule for the next few frames - the wake
    // schedule was clock+1, so within 1..tick_period (=10)
    // frames the cell must actually fire.
    let saved_tick = sim.tick_counts.get(&liq_pos).copied().unwrap_or(0);
    for _ in 0..5 {
        sim.step(water());
    }
    let new_tick = sim.tick_counts.get(&liq_pos).copied().unwrap_or(0);
    assert!(
        new_tick > saved_tick,
        "liquid at {liq_pos:?} didn't actually tick within 5 frames of \
         the edit (saved {saved_tick}, got {new_tick})"
    );
}

// -----------------------------------------------------------------
// Q.4a: lava-water boundary behaviour through LiquidSim.
// MC reference: LiquidBlock.java::shouldSpreadLiquid (lines 219..=241).
// -----------------------------------------------------------------

use crate::block::{COBBLE_ID, LAVA_ID, OBSIDIAN_ID};

#[test]
fn lava_source_next_to_water_source_becomes_obsidian() {
    // Two adjacent sources. Lava ticks, sees water neighbour,
    // converts to obsidian.
    let mut sim = LiquidSim::new();
    sim.fill_floor(0, -3..=3, -3..=3);
    sim.place_source([0, 1, 0], lava());
    sim.world.set([1, 1, 0], BlockId(crate::block::WATER_ID));
    sim.level.insert([1, 1, 0], 0);
    sim.active.insert([1, 1, 0]);
    sim.positions.insert([1, 1, 0]);
    sim.run(lava(), 2);
    assert_eq!(
        sim.world.get([0, 1, 0]).0,
        OBSIDIAN_ID,
        "lava source did not convert to obsidian"
    );
    // The water cell is untouched.
    assert_eq!(sim.world.get([1, 1, 0]).0, crate::block::WATER_ID);
}

#[test]
fn lava_flow_next_to_water_source_becomes_cobble() {
    // Lava flow (level >= 1) next to water -> cobblestone.
    let mut sim = LiquidSim::new();
    sim.fill_floor(0, -3..=3, -3..=3);
    sim.world.set([0, 1, 0], BlockId(LAVA_ID));
    sim.level.insert([0, 1, 0], 4); // flow
    sim.active.insert([0, 1, 0]);
    sim.positions.insert([0, 1, 0]);
    sim.world.set([1, 1, 0], BlockId(crate::block::WATER_ID));
    sim.level.insert([1, 1, 0], 0);
    sim.active.insert([1, 1, 0]);
    sim.positions.insert([1, 1, 0]);
    sim.run(lava(), 2);
    assert_eq!(
        sim.world.get([0, 1, 0]).0,
        COBBLE_ID,
        "lava flow did not convert to cobble"
    );
}

#[test]
fn lava_flow_spreads_to_water_flow_becomes_cobble() {
    // Two flowing cells touching: the lava flow converts. Mirrors
    // the "lava streams into a water sheet" practical case.
    let mut sim = LiquidSim::new();
    sim.fill_floor(0, -3..=3, -3..=3);
    sim.world.set([0, 1, 0], BlockId(LAVA_ID));
    sim.level.insert([0, 1, 0], 5);
    sim.active.insert([0, 1, 0]);
    sim.positions.insert([0, 1, 0]);
    sim.world.set([1, 1, 0], BlockId(crate::block::WATER_ID));
    sim.level.insert([1, 1, 0], 3); // water flow too
    sim.active.insert([1, 1, 0]);
    sim.positions.insert([1, 1, 0]);
    sim.run(lava(), 2);
    assert_eq!(sim.world.get([0, 1, 0]).0, COBBLE_ID);
}

#[test]
fn lava_without_water_neighbour_does_not_solidify() {
    // Sanity: lava in isolation must continue to flow, not freeze.
    let mut sim = LiquidSim::new();
    sim.fill_floor(0, -3..=3, -3..=3);
    sim.place_source([0, 1, 0], lava());
    sim.run(lava(), 5);
    assert_eq!(sim.world.get([0, 1, 0]).0, LAVA_ID);
}

#[test]
fn water_source_next_to_lava_source_water_unchanged() {
    // Symmetric layout: only the lava cell mutates. Water keeps
    // its source level even after multiple ticks.
    let mut sim = LiquidSim::new();
    sim.fill_floor(0, -3..=3, -3..=3);
    sim.place_source([0, 1, 0], lava());
    sim.place_source([1, 1, 0], water());
    sim.run(lava(), 2);
    sim.run(water(), 2);
    assert_eq!(sim.world.get([1, 1, 0]).0, crate::block::WATER_ID);
    assert_eq!(sim.world.get([0, 1, 0]).0, OBSIDIAN_ID);
}

#[test]
fn water_above_lava_source_becomes_obsidian_below() {
    // Water sitting on top of lava: lava is source -> obsidian
    // (covers the "pour water into lava pit" interaction).
    let mut sim = LiquidSim::new();
    sim.fill_floor(0, -3..=3, -3..=3);
    sim.place_source([0, 1, 0], lava());
    sim.world.set([0, 2, 0], BlockId(crate::block::WATER_ID));
    sim.level.insert([0, 2, 0], 0);
    sim.active.insert([0, 2, 0]);
    sim.positions.insert([0, 2, 0]);
    sim.run(lava(), 2);
    assert_eq!(sim.world.get([0, 1, 0]).0, OBSIDIAN_ID);
}

// -----------------------------------------------------------------
// Q.4e: water extinguishes fire (F_WASHABLE on FIRE_ID drives
// the existing washable-spread path; the tape's `washed` entry
// captures the displaced fire).
// -----------------------------------------------------------------

#[test]
fn water_spreading_into_fire_replaces_it() {
    use crate::block::FIRE_ID;
    // Water source spreads onto a stone floor that extends well
    // past the BFS-drop horizon (>4 cells) so every direction
    // ties at u32::MAX in `distance_to_drop_at` and the spread
    // hits all 4 NSEW neighbours. The fire cell sits on the
    // floor; F_WASHABLE makes it a valid spread target -> water
    // overwrites it.
    let mut sim = LiquidSim::new();
    sim.fill_floor(0, -8..=8, -8..=8);
    sim.place_source([0, 1, 0], water());
    sim.world.set([1, 1, 0], BlockId(FIRE_ID));
    sim.run(water(), 5);
    let after = sim.world.get([1, 1, 0]).0;
    assert_ne!(
        after, FIRE_ID,
        "fire still at [1,1,0] after 5 ticks (water plan must \
         treat F_WASHABLE fire as a valid spread target)"
    );
    assert_eq!(
        after,
        crate::block::WATER_ID,
        "expected water at former fire cell, got block id {after}"
    );
}

#[test]
fn water_spread_emits_washed_entry_for_fire() {
    use crate::block::FIRE_ID;
    // Verify the apply tape surfaces the fire as a washed block
    // - that's what the live caller hooks to drop `voxel.fires`.
    let mut sim = LiquidSim::new();
    sim.fill_floor(0, -8..=8, -8..=8);
    sim.place_source([0, 1, 0], water());
    sim.world.set([1, 1, 0], BlockId(FIRE_ID));
    // Manually run one plan + apply cycle so we can inspect the
    // tape.
    use crate::liquid::plan::liquid_plan_step;
    let mut saw_fire_wash = false;
    for _ in 0..30 {
        let plan = liquid_plan_step(
            &sim.world,
            &sim.level,
            &sim.active,
            water(),
            sim.bias_drops,
        );
        let tape = crate::liquid::liquid_apply_plan(
            &mut sim.world,
            &mut sim.level,
            &mut sim.active,
            &mut sim.positions,
            water(),
            plan,
        );
        if tape.washed.iter().any(|(b, _)| b.0 == FIRE_ID) {
            saw_fire_wash = true;
            break;
        }
    }
    assert!(
        saw_fire_wash,
        "no washed FIRE entry across 30 plan/apply cycles"
    );
}

#[test]
fn solidified_lava_cleared_from_level_map() {
    // Q.4a invariant: after the boundary conversion, the cell is
    // no longer tracked as a liquid - level_map + positions are
    // both cleared so the renderer + future ticks treat it as a
    // regular solid block.
    let mut sim = LiquidSim::new();
    sim.fill_floor(0, -3..=3, -3..=3);
    sim.place_source([0, 1, 0], lava());
    sim.world.set([1, 1, 0], BlockId(crate::block::WATER_ID));
    sim.level.insert([1, 1, 0], 0);
    sim.active.insert([1, 1, 0]);
    sim.positions.insert([1, 1, 0]);
    sim.run(lava(), 2);
    assert!(!sim.level.contains_key(&[0, 1, 0]));
    assert!(!sim.positions.contains(&[0, 1, 0]));
}
