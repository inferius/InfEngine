//! W245.5 - villages in the worldgen pipeline.
//!
//! Placement (`structure_set/villages.json`): random_spread with
//! spacing 34 / separation 8 / salt 10387312 - every 34x34-chunk grid
//! cell rolls one candidate start chunk (`origin + rand(26)` on both
//! axes, the LINEAR spread), and the variant follows the biome at the
//! start anchor (`has_structure/village_*`: plains+meadow, desert,
//! savanna, snowy_plains, taiga - the five weight-1 set entries carry
//! disjoint biome tags, so the weighted shuffle collapses to a direct
//! biome match).
//!
//! The layout comes from the W245.4 jigsaw solver, cached per start
//! chunk (pure in the effective seed). Carving mirrors MC placement:
//! pieces in generation order; every template cell in NBT order maps
//! through the W245.3 blockmap; jigsaw cells resolve to their
//! `final_state`. RIGID pieces sit at their solved Y; terrain_matching
//! pieces drape per column like the GravityProcessor(WORLD_SURFACE_WG,
//! -1): world y = floor(x, z) + local y, so a street's y=0 layer
//! replaces the turf block of each column it crosses.
//!
//! `beard_thin` terrain adaptation is approximated: rigid pieces get
//! a dirt foundation from their box floor down to the terrain (capped
//! at 12 - taller stilts read as cliff overhangs and stay open, like
//! the fading MC beard). Honest gaps: the drape reads the NATURAL
//! heightmap (sibling pieces are invisible to it - MC's live
//! heightmap sees earlier writes); feature sockets place no trees
//! yet; village chests roll the simple_dungeon table until per-
//! building tables arrive (better stocked than empty).

use infengine_voxel::{BlockId, CHUNK_SIZE, Chunk, ChunkCoord};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use super::biome::McBiome;
use super::blockmap::{Mapped, map_state, map_state_string};
use super::features::stream_seed;
use super::jigsaw::{self, PlacedPiece, template_bb};
use super::noise::SplitMix64;
use super::terrain::{mc_floor_at, mc_full_biome_at};
use super::village_data;
use crate::worldgen::WorldgenCtx;

/// `structure_set/villages.json`.
pub(crate) const SPACING: i32 = 34;
const SEPARATION: i32 = 8;
const SALT_VILLAGE_SET: u64 = 10_387_312;
/// Layout stream per start chunk.
const SALT_VILLAGE_LAYOUT: u64 = 0xD4E6_0020;

/// Dirt for the beard-thin foundation approximation.
const FOUNDATION_ID: u16 = 2;
/// Foundations taller than this stay open (the MC beard fades out).
const FOUNDATION_MAX: i32 = 12;

// ---------------------------------------------------------------
// W279: template blockstate ORIENTATION. The palette carries the MC
// properties (facing / half / open) but the carve only wrote block
// ids - every stair rendered its default facing (the user's
// unclimbable village stairs), doors ignored their hinge, trapdoors
// their half. Carve now records oriented placements in a
// thread-local; the worker gen job drains it right after
// `generate_chunk_at_ctx`, feeds the mesher AND ships the entries as
// ChunkSideEffects for the main-thread state maps.
// ---------------------------------------------------------------

/// One oriented template placement (world pos + kind + state byte +
/// open flag). Kinds: 0 = stairs, 1 = door (lower half), 2 =
/// trapdoor, 3 = fence gate.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CarvedOriented {
    pub(crate) pos: [i32; 3],
    pub(crate) kind: u8,
    /// facing (bits 0-1) | TRAPDOOR_TOP_BIT (bit 2, stairs/trapdoor
    /// upside-down half).
    pub(crate) state: u8,
    pub(crate) open: bool,
}

thread_local! {
    static CARVE_ORIENTED: RefCell<Vec<CarvedOriented>> = const { RefCell::new(Vec::new()) };
}

/// Drain the oriented placements recorded since the last take (the
/// worker calls this right after generating one chunk).
pub(crate) fn take_carved_oriented() -> Vec<CarvedOriented> {
    CARVE_ORIENTED.with(|c| std::mem::take(&mut *c.borrow_mut()))
}

/// MC facing prop -> engine facing code (0=+X east, 1=+Z south,
/// 2=-X west, 3=-Z north - the door/torch convention).
fn facing_code(name: &str) -> Option<u8> {
    Some(match name {
        "east" => 0,
        "south" => 1,
        "west" => 2,
        "north" => 3,
        _ => return None,
    })
}

/// Record the orientation of an oriented-family block placed from a
/// template palette state, composing the piece rotation (CW quarter
/// turns rotate +X -> +Z, so `facing' = facing + r`).
fn record_oriented(
    id: u16,
    state: &super::template::PaletteState,
    rot: u8,
    pos: [i32; 3],
) {
    let name = crate::block::block_type(id).map(|b| b.name).unwrap_or("");
    let is_stairs = name.ends_with("_stairs");
    let is_door = crate::block::door::is_door_block(id);
    let is_trap = crate::block::door::is_trapdoor_block(id);
    let is_gate = crate::block::door::is_fence_gate_block(id);
    // W288: wall torches keep their template facing too - the MC
    // name (from the palette, BEFORE the blockmap folds wall_torch
    // into the plain torch id) is the only trace of wall-ness.
    let is_wall_torch = state.name.ends_with("wall_torch");
    if !(is_stairs || is_door || is_trap || is_gate || is_wall_torch) {
        return;
    }
    // Doors: only the LOWER half keys the state maps.
    if is_door && state.prop("half") == Some("upper") {
        return;
    }
    let Some(f) = state.prop("facing").and_then(facing_code) else {
        return;
    };
    let facing = (f + rot) % 4;
    let top = state.prop("half") == Some("top");
    let open = state.prop("open") == Some("true");
    let kind = if is_stairs {
        0
    } else if is_door {
        1
    } else if is_trap {
        2
    } else if is_gate {
        3
    } else {
        4
    };
    let stt = facing | if top { crate::block::door::TRAPDOOR_TOP_BIT } else { 0 };
    CARVE_ORIENTED.with(|c| {
        c.borrow_mut().push(CarvedOriented { pos, kind, state: stt, open })
    });
}

/// The candidate start chunk of a 34x34 grid cell. `pub(crate)`: the
/// outpost's exclusion zone (10 chunks from any village placement)
/// probes this grid.
pub(crate) fn potential_start(seed: u32, grid_x: i32, grid_z: i32) -> [i32; 2] {
    super::scattered::random_spread_start(
        seed,
        grid_x,
        grid_z,
        SPACING,
        SEPARATION,
        SALT_VILLAGE_SET,
    )
}

/// The start pool for the biome at the start anchor, per the five
/// disjoint `has_structure/village_*` tags.
fn start_pool_for(biome: McBiome) -> Option<&'static str> {
    Some(match biome {
        McBiome::Plains | McBiome::Meadow => "minecraft:village/plains/town_centers",
        McBiome::Desert => "minecraft:village/desert/town_centers",
        McBiome::Savanna => "minecraft:village/savanna/town_centers",
        McBiome::SnowyPlains => "minecraft:village/snowy/town_centers",
        McBiome::Taiga => "minecraft:village/taiga/town_centers",
        _ => return None,
    })
}

/// Solve (and cache) the village layout for a start chunk. `None`
/// when the cell's biome hosts no village or the start pool rolls
/// empty.
pub(crate) fn layout_for_start(
    ctx: &WorldgenCtx,
    scx: i32,
    scz: i32,
) -> Option<Rc<Vec<PlacedPiece>>> {
    type Cache = HashMap<(u32, i32, i32), Option<Rc<Vec<PlacedPiece>>>>;
    thread_local! {
        static CACHE: RefCell<Cache> = RefCell::new(HashMap::new());
    }
    let seed = ctx.effective_seed();
    if let Some(hit) = CACHE.with(|c| c.borrow().get(&(seed, scx, scz)).cloned()) {
        return hit;
    }
    let built = build_layout(ctx, scx, scz).map(Rc::new);
    CACHE.with(|c| {
        let mut map = c.borrow_mut();
        if map.len() > 256 {
            map.clear();
        }
        map.insert((seed, scx, scz), built.clone());
    });
    built
}

fn build_layout(ctx: &WorldgenCtx, scx: i32, scz: i32) -> Option<Vec<PlacedPiece>> {
    let cs = CHUNK_SIZE as i32;
    let biome = mc_full_biome_at(ctx, scx * cs + 8, scz * cs + 8);
    let pool = start_pool_for(biome)?;
    let mut rng = SplitMix64::new(stream_seed(
        ctx.effective_seed(),
        scx,
        scz,
        SALT_VILLAGE_LAYOUT,
    ));
    jigsaw::solve(ctx, [scx, scz], pool, jigsaw::VILLAGE_MAX_DEPTH, &mut rng)
}

/// The per-chunk village pass: find grid cells whose candidate start
/// is in reach, solve their layouts, carve the intersecting pieces.
/// Runs in SURFACE_STRUCTURES before the desert-well feature.
pub(crate) fn place_villages(
    chunk: &mut Chunk,
    coord: ChunkCoord,
    ctx: &WorldgenCtx,
    origin: [i32; 3],
) {
    let seed = ctx.effective_seed();
    // Pieces reach at most 80 blocks + a piece extent from the start
    // anchor: 7 chunks covers it.
    const REACH: i32 = 7;
    let grid_x0 = (coord[0] - REACH).div_euclid(SPACING);
    let grid_x1 = (coord[0] + REACH).div_euclid(SPACING);
    let grid_z0 = (coord[2] - REACH).div_euclid(SPACING);
    let grid_z1 = (coord[2] + REACH).div_euclid(SPACING);
    for gx in grid_x0..=grid_x1 {
        for gz in grid_z0..=grid_z1 {
            let [scx, scz] = potential_start(seed, gx, gz);
            if (scx - coord[0]).abs() > REACH || (scz - coord[2]).abs() > REACH {
                continue;
            }
            let Some(layout) = layout_for_start(ctx, scx, scz) else {
                continue;
            };
            carve_layout(&layout, chunk, ctx, origin);
        }
    }
}

/// W285: deterministic membership - is `pos` inside the REAL footprint
/// of any village piece in reach? Surface features consult this so
/// they stop growing through houses: a tree whose base sits in a
/// NEIGHBOUR source chunk skips the in-chunk floor check (that
/// chunk's cells aren't available while carving this one), so without
/// a box test its canopy writes straight through whatever piece
/// stands there - while the base's own chunk rejects the same try,
/// leaving an orphaned floating canopy at the border (docx img27/30).
/// Uses the same cached layouts the carve does, so a probe is a few
/// grid lookups + a piece-box scan.
pub(crate) fn pos_in_village(ctx: &WorldgenCtx, pos: [i32; 3]) -> bool {
    let seed = ctx.effective_seed();
    let cs = CHUNK_SIZE as i32;
    let (ccx, ccz) = (pos[0].div_euclid(cs), pos[2].div_euclid(cs));
    const REACH: i32 = 7;
    for gx in (ccx - REACH).div_euclid(SPACING)..=(ccx + REACH).div_euclid(SPACING) {
        for gz in
            (ccz - REACH).div_euclid(SPACING)..=(ccz + REACH).div_euclid(SPACING)
        {
            let [scx, scz] = potential_start(seed, gx, gz);
            if (scx - ccx).abs() > REACH || (scz - ccz).abs() > REACH {
                continue;
            }
            let Some(layout) = layout_for_start(ctx, scx, scz) else {
                continue;
            };
            if layout_contains(&layout, pos) {
                return true;
            }
        }
    }
    false
}

/// Piece-box membership for a solved jigsaw layout, mirroring the
/// carve's clip semantics: real template footprint (not the
/// expansion-hack headroom), full column for terrain_matching drapes
/// (paths follow the heightmap), foundation legs included below.
pub(crate) fn layout_contains(layout: &[PlacedPiece], pos: [i32; 3]) -> bool {
    for piece in layout {
        let Some(t) = village_data::template(&piece.template) else {
            continue;
        };
        let mut bb = template_bb(t.size, piece.position, piece.rotation);
        if piece.terrain_matching {
            bb.y0 = -64;
            bb.y1 = 319;
        } else {
            bb.y0 = (bb.y0 - FOUNDATION_MAX).max(-64);
        }
        if bb.contains(pos) {
            return true;
        }
    }
    false
}

/// Per-cell keep roll for a list-element overlay (the outpost
/// overgrown-watchtower `block_rot`): pure in (seed, world cell) so
/// every replay slice of the column agrees. MC rolls the shared
/// worldgen RNG sequentially - own-stream approximation, same
/// trade-off as every other structure stream.
const SALT_LIST_OVERLAY: u64 = 0xD4E6_0030;

fn overlay_keeps(seed: u32, wx: i32, wy: i32, wz: i32, integrity: f32) -> bool {
    let s = stream_seed(seed, wx, wz, SALT_LIST_OVERLAY)
        ^ (wy as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let roll = (SplitMix64::new(s).next_u64() >> 40) as f32 / (1u64 << 24) as f32;
    roll < integrity
}

/// W261: run a piece's processor list on one mapped cell (the
/// ancient-city degradation): `block_rot` keep roll first (`None` =
/// the cell is dropped, the world block stays - the ruin holes),
/// then the first matching `random_block_match` rule replaces the
/// block (cracked bricks / snuffed soul lanterns). Pure per (seed,
/// world cell), rolls sequential within the cell.
const SALT_CELL_PROC: u64 = 0xD4E6_00E1;

fn apply_cell_processors(
    seed: u32,
    list: &str,
    pos: [i32; 3],
    id: u16,
) -> Option<u16> {
    let procs = village_data::cell_processors(list);
    if procs.rot_integrity >= 1.0 && procs.rules.is_empty() {
        return Some(id);
    }
    let mut r = SplitMix64::new(
        stream_seed(seed, pos[0], pos[2], SALT_CELL_PROC)
            ^ (pos[1] as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15),
    );
    let mut next_f = || (r.next_u64() >> 40) as f32 / (1u64 << 24) as f32;
    if procs.rot_integrity < 1.0
        && (procs.rottable.is_empty() || procs.rottable.contains(&id))
        && next_f() > procs.rot_integrity
    {
        return None;
    }
    for (input, prob, output) in &procs.rules {
        if *input == id && next_f() < *prob {
            return Some(*output);
        }
    }
    Some(id)
}

/// Write every piece cell that lands in this 16^3 cube. Shared by the
/// village and outpost passes (identical carve semantics).
pub(crate) fn carve_layout(
    layout: &[PlacedPiece],
    chunk: &mut Chunk,
    ctx: &WorldgenCtx,
    origin: [i32; 3],
) {
    let cs = CHUNK_SIZE as i32;
    let cube = super::mineshaft::Bb {
        x0: origin[0],
        y0: origin[1],
        z0: origin[2],
        x1: origin[0] + cs - 1,
        y1: origin[1] + cs - 1,
        z1: origin[2] + cs - 1,
    };
    for piece in layout {
        let Some(t) = village_data::template(&piece.template) else {
            continue;
        };
        // The REAL footprint (not the expansion-hack headroom box).
        let mut real_bb = template_bb(t.size, piece.position, piece.rotation);
        if piece.terrain_matching {
            // The drape can move columns anywhere on the terrain:
            // widen the y test to the full column.
            real_bb.y0 = -64;
            real_bb.y1 = 319;
        }
        // Foundations extend below the box - include them in the clip
        // test so a cube under a house still writes the dirt legs.
        // (W261: clamp at the REAL world floor -64 - the old .max(0)
        // degenerated the box for sub-zero pieces and skipped every
        // ancient-city cell.)
        let test_bb = super::mineshaft::Bb {
            y0: (real_bb.y0 - FOUNDATION_MAX).max(-64),
            ..real_bb
        };
        if !test_bb.intersects(&cube) {
            continue;
        }
        for cell in &t.cells {
            let rp = jigsaw::rot_pos(piece.rotation, cell.pos);
            let wx = piece.position[0] + rp[0];
            let wz = piece.position[2] + rp[2];
            let wy = if piece.terrain_matching {
                // GravityProcessor(WORLD_SURFACE_WG, -1): the local
                // y=0 layer replaces each column's turf block.
                mc_floor_at(ctx, wx, wz) + cell.pos[1]
            } else {
                piece.position[1] + rp[1]
            };
            let (lx, ly, lz) =
                (wx - origin[0], wy - origin[1], wz - origin[2]);
            if !(0..cs).contains(&lx)
                || !(0..cs).contains(&ly)
                || !(0..cs).contains(&lz)
            {
                continue;
            }
            let mapped = match &cell.jigsaw {
                Some(j) => map_state_string(&j.final_state),
                None => map_state(&t.palette[cell.state]),
            };
            if let Mapped::Id(mut id) = mapped {
                // W261: the element's processor list (degradation).
                if let Some(list) = &piece.processors {
                    match apply_cell_processors(
                        ctx.effective_seed(),
                        list,
                        [wx, wy, wz],
                        id,
                    ) {
                        Some(out) => id = out,
                        None => continue,
                    }
                }
                chunk.set(lx as usize, ly as usize, lz as usize, BlockId(id));
                // W279: stairs/doors/trapdoors/gates carry their
                // palette orientation to the mesher + state maps.
                if cell.jigsaw.is_none() {
                    record_oriented(
                        id,
                        &t.palette[cell.state],
                        piece.rotation,
                        [wx, wy, wz],
                    );
                }
            }
        }
        // W251: list-element overlays (`ListPoolElement` places every
        // stacked template at the same anchor): each cell of an
        // overlay writes only when its block_rot roll keeps it - the
        // outpost's overgrown watchtower scatters ~5% moss/vine
        // variance over the plain tower.
        for ov in &piece.overlays {
            let Some(ot) = village_data::template(&ov.location) else {
                continue;
            };
            let seed = ctx.effective_seed();
            for cell in &ot.cells {
                let rp = jigsaw::rot_pos(piece.rotation, cell.pos);
                let wx = piece.position[0] + rp[0];
                let wz = piece.position[2] + rp[2];
                let wy = if piece.terrain_matching {
                    mc_floor_at(ctx, wx, wz) + cell.pos[1]
                } else {
                    piece.position[1] + rp[1]
                };
                let (lx, ly, lz) =
                    (wx - origin[0], wy - origin[1], wz - origin[2]);
                if !(0..cs).contains(&lx)
                    || !(0..cs).contains(&ly)
                    || !(0..cs).contains(&lz)
                    || !overlay_keeps(seed, wx, wy, wz, ov.integrity)
                {
                    continue;
                }
                let mapped = match &cell.jigsaw {
                    Some(j) => map_state_string(&j.final_state),
                    None => map_state(&ot.palette[cell.state]),
                };
                if let Mapped::Id(id) = mapped {
                    chunk.set(lx as usize, ly as usize, lz as usize, BlockId(id));
                    if cell.jigsaw.is_none() {
                        record_oriented(
                            id,
                            &ot.palette[cell.state],
                            piece.rotation,
                            [wx, wy, wz],
                        );
                    }
                }
            }
        }
        // beard_thin approximation: rigid pieces stand on a dirt
        // foundation down to the terrain.
        if !piece.terrain_matching {
            let foot = template_bb(t.size, piece.position, piece.rotation);
            for wx in foot.x0..=foot.x1 {
                for wz in foot.z0..=foot.z1 {
                    if wx < cube.x0 || wx > cube.x1 || wz < cube.z0 || wz > cube.z1 {
                        continue;
                    }
                    let ground = mc_floor_at(ctx, wx, wz);
                    if ground >= foot.y0 || foot.y0 - ground > FOUNDATION_MAX {
                        continue;
                    }
                    for wy in (ground + 1)..foot.y0 {
                        let ly = wy - origin[1];
                        if (0..cs).contains(&ly) {
                            chunk.set(
                                (wx - origin[0]) as usize,
                                ly as usize,
                                (wz - origin[2]) as usize,
                                BlockId(FOUNDATION_ID),
                            );
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// W279: template stairs record their rotated facing; the piece
    /// rotation composes as `facing + r` (CW90 maps +X to +Z); door
    /// UPPER halves record nothing (state keys the lower voxel).
    #[test]
    fn oriented_carve_records_rotated_stairs_and_skips_door_uppers() {
        let _ = take_carved_oriented(); // drain any prior entries
        let stairs_id = crate::block::BLOCK_TYPES
            .iter()
            .find(|b| b.name.ends_with("_stairs"))
            .map(|b| b.id)
            .expect("a stairs block exists");
        let st = super::super::template::PaletteState {
            name: "minecraft:oak_stairs".into(),
            props: vec![
                ("facing".into(), "north".into()),
                ("half".into(), "top".into()),
            ],
        };
        record_oriented(stairs_id, &st, 1, [5, 6, 7]);
        let door_upper = super::super::template::PaletteState {
            name: "minecraft:oak_door".into(),
            props: vec![
                ("facing".into(), "east".into()),
                ("half".into(), "upper".into()),
            ],
        };
        record_oriented(77, &door_upper, 0, [1, 2, 3]);
        let got = take_carved_oriented();
        assert_eq!(got.len(), 1, "door upper must not record: {got:?}");
        let o = got[0];
        assert_eq!(o.kind, 0);
        assert_eq!(o.pos, [5, 6, 7]);
        // north(3) rotated CW once -> +X east(0), plus the top bit.
        assert_eq!(o.state & 3, 0);
        assert_ne!(o.state & crate::block::door::TRAPDOOR_TOP_BIT, 0);
    }

    #[test]
    fn potential_starts_stay_in_their_cells() {
        for gx in -3..3 {
            for gz in -3..3 {
                let [sx, sz] = potential_start(42, gx, gz);
                assert!(sx >= gx * SPACING && sx < gx * SPACING + SPACING - SEPARATION);
                assert!(sz >= gz * SPACING && sz < gz * SPACING + SPACING - SEPARATION);
                assert_eq!(potential_start(42, gx, gz), [sx, sz], "deterministic");
            }
        }
    }

    #[test]
    fn start_pools_follow_the_biome_tags() {
        assert!(start_pool_for(McBiome::Plains).unwrap().contains("plains"));
        assert!(start_pool_for(McBiome::Meadow).unwrap().contains("plains"));
        assert!(start_pool_for(McBiome::Desert).unwrap().contains("desert"));
        assert!(start_pool_for(McBiome::SnowyPlains).unwrap().contains("snowy"));
        assert!(start_pool_for(McBiome::Taiga).unwrap().contains("taiga"));
        assert_eq!(start_pool_for(McBiome::SavannaPlateau), None);
        assert_eq!(start_pool_for(McBiome::Badlands), None);
    }

    /// W285: the feature-exclusion probe agrees with the carve - a
    /// point inside a piece's real footprint is a member, a far-away
    /// point is not, and the answer is deterministic per seed.
    #[test]
    fn pos_in_village_matches_piece_footprints() {
        let ctx = crate::worldgen::WorldgenCtx {
            base_seed: 3592732596,
            dim: "overworld".into(),
            profile: crate::worldgen::WorldGen::Mc,
        };
        // Find any village within a modest scan (same seed the dump
        // probe pins; start (296, 295) area hosts one).
        let mut found: Option<(i32, i32)> = None;
        'scan: for gx in 7..12 {
            for gz in 7..12 {
                let [sx, sz] = potential_start(ctx.effective_seed(), gx, gz);
                if layout_for_start(&ctx, sx, sz).is_some() {
                    found = Some((sx, sz));
                    break 'scan;
                }
            }
        }
        let Some((sx, sz)) = found else {
            // Seed-dependent scan came up dry - nothing to assert
            // against (the dump probe covers discovery).
            return;
        };
        let layout = layout_for_start(&ctx, sx, sz).expect("layout");
        // A cell INSIDE the town centre's real footprint is a member.
        let t = village_data::template(&layout[0].template).expect("centre template");
        let bb = template_bb(t.size, layout[0].position, layout[0].rotation);
        let inside = [(bb.x0 + bb.x1) / 2, (bb.y0 + bb.y1) / 2, (bb.z0 + bb.z1) / 2];
        assert!(pos_in_village(&ctx, inside), "centre cell must be a member");
        assert!(
            layout_contains(&layout, inside),
            "layout_contains agrees on the centre cell"
        );
        // A point far outside every piece (villages span < 200 blocks)
        // is not a member.
        let outside = [inside[0] + 4096, inside[1], inside[2] + 4096];
        assert!(!pos_in_village(&ctx, outside), "far point must not be a member");
    }

    /// Probe: scan for villages near a seed/centre + full-pipeline
    /// verify the first one (real chunks must hold its blocks).
    /// `INFENGINE_DUMP_SEED=<seed> INFENGINE_DUMP_CX/CZ=<chunk>
    /// INFENGINE_DUMP_RADIUS=<r> cargo test -p voxel_demo_mp --release
    /// --lib dump_villages -- --ignored --nocapture`
    #[test]
    #[ignore]
    #[allow(clippy::print_stdout)]
    fn dump_villages() {
        let env_i32 = |name: &str, default: i32| -> i32 {
            std::env::var(name)
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(default)
        };
        let seed: u32 = std::env::var("INFENGINE_DUMP_SEED")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(3592732596);
        let radius = env_i32("INFENGINE_DUMP_RADIUS", 96);
        let (ccx, ccz) =
            (env_i32("INFENGINE_DUMP_CX", 296), env_i32("INFENGINE_DUMP_CZ", 295));
        let ctx = crate::worldgen::WorldgenCtx {
            base_seed: seed,
            dim: std::env::var("INFENGINE_DUMP_DIM")
                .unwrap_or_else(|_| "overworld".into()),
            profile: crate::worldgen::WorldGen::Mc,
        };
        let cs = CHUNK_SIZE as i32;
        let mut found: Vec<([i32; 2], usize)> = Vec::new();
        for gx in (ccx - radius).div_euclid(SPACING)..=(ccx + radius).div_euclid(SPACING)
        {
            for gz in
                (ccz - radius).div_euclid(SPACING)..=(ccz + radius).div_euclid(SPACING)
            {
                let [sx, sz] = potential_start(ctx.effective_seed(), gx, gz);
                let biome = mc_full_biome_at(&ctx, sx * cs + 8, sz * cs + 8);
                let Some(layout) = layout_for_start(&ctx, sx, sz) else {
                    println!("cell ({gx}, {gz}): start ({sx}, {sz}) biome {biome:?} - no village");
                    continue;
                };
                let houses = layout
                    .iter()
                    .filter(|p| p.template.contains("/houses/"))
                    .count();
                println!(
                    "village at start chunk ({sx}, {sz}) [{biome:?}]: {} pieces, {houses} houses, centre bb {:?}",
                    layout.len(),
                    layout[0].bb
                );
                found.push(([sx, sz], layout.len()));
            }
        }
        println!("total: {} villages", found.len());
        // Full-pipeline verify on the first: the town-centre cube must
        // contain SOME village-written block after generation.
        if let Some(([sx, sz], _)) = found.first() {
            let layout = layout_for_start(&ctx, *sx, *sz).expect("layout");
            let centre = layout[0].bb;
            let cx = (centre.x0 + centre.x1) / 2;
            let cz = (centre.z0 + centre.z1) / 2;
            let cy = centre.y0.max(1);
            let coord = [cx.div_euclid(cs), cy.div_euclid(cs), cz.div_euclid(cs)];
            let chunk = crate::worldgen::generate_chunk_at_ctx(coord, &ctx);
            let mut village_blocks = 0usize;
            for lx in 0..CHUNK_SIZE {
                for ly in 0..CHUNK_SIZE {
                    for lz in 0..CHUNK_SIZE {
                        let id = chunk.get(lx, ly, lz).0;
                        // Path blocks / planks / cobble / fences are
                        // unmistakable village artefacts on plains.
                        if crate::block::block_type(id).is_some_and(|bt| {
                            matches!(
                                bt.name,
                                "dirt_path" | "planks" | "cobble" | "oak_fence"
                                    | "spruce_log" | "glass_pane" | "hay_block"
                            )
                        }) {
                            village_blocks += 1;
                        }
                    }
                }
            }
            println!(
                "full-pipeline centre cube {coord:?}: {village_blocks} village blocks"
            );
            assert!(
                village_blocks > 0,
                "the town-centre cube generated no village blocks"
            );
        }
    }
}
