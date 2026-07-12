//! W245.4 - the jigsaw solver: `JigsawPlacement.java` (26.1.2) port.
//!
//! Solves a village layout: starting from the biome start pool at a
//! chunk anchor, recursively attaches template pieces socket-to-
//! socket (`minecraft:jigsaw` blocks: front faces must oppose,
//! `target` must equal the neighbour's `name`, aligned vertical
//! joints must share their top). Pieces draw from weight-expanded
//! shuffled pools with the pool fallback appended; an EMPTY element
//! terminates the socket, depth caps at the structure `size`, and a
//! free-space ledger (the VoxelShape `ONLY_FIRST` dance) keeps boxes
//! from overlapping - MC's quarter-block deflate is exactly "integer
//! boxes may touch faces but not share cells", which `Bb::intersects`
//! expresses directly. Interior sockets (attach position inside the
//! source box) collide against the SOURCE box ledger instead of the
//! world ledger, like the Java `sourceFree` switch.
//!
//! The expansion hack (villages set `use_expansion_hack`) reserves
//! vertical headroom over low pieces for the tallest child their
//! sockets could pull, so streets don't wall off house pools.
//!
//! Honest gaps vs MC:
//! - `feature_pool_element` sockets (street trees / pile decor) are
//!   consumed without placing anything - the socket outcome matches
//!   (nothing else spawns there), the tree visual lands with a later
//!   wave.
//! - Junctions are not recorded: the `beard_thin` terrain adaptation
//!   is approximated at carve time (W245.5), not via the noise beard.
//! - `getFirstFreeHeight(WORLD_SURFACE_WG)` maps to the post-carve
//!   solid floor + 1; over water MC would measure the surface, we
//!   measure the seabed (villages don't start in oceans - biome gate).

use std::cell::RefCell;
use std::rc::Rc;

use super::mineshaft::Bb;
use super::noise::SplitMix64;
use super::template::{JigsawInfo, StructureTemplate};
use super::terrain::mc_floor_at;
use super::village_data::{self, ElementKind, ListOverlay, TemplatePool};
use crate::worldgen::WorldgenCtx;

/// `village_*.json` shared config: recursion depth 6, +-80 blocks
/// horizontal, unbounded vertical (the single-value MaxDistance form
/// defaults vertical to the world height). The outpost runs the same
/// solver at depth 7 (`pillager_outpost.json` size).
pub(crate) const VILLAGE_MAX_DEPTH: i32 = 6;
pub(crate) const VILLAGE_MAX_DISTANCE: i32 = 80;

fn next_int(r: &mut SplitMix64, n: i32) -> i32 {
    (r.next_u64() % n.max(1) as u64) as i32
}

fn shuffle<T>(v: &mut [T], rng: &mut SplitMix64) {
    for i in (1..v.len()).rev() {
        let j = (rng.next_u64() % (i as u64 + 1)) as usize;
        v.swap(i, j);
    }
}

// ----------------------------------------------------------------
// Directions + rotations.
// ----------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Dir6 {
    North,
    South,
    West,
    East,
    Up,
    Down,
}

impl Dir6 {
    fn parse(s: &str) -> Option<Dir6> {
        Some(match s {
            "north" => Dir6::North,
            "south" => Dir6::South,
            "west" => Dir6::West,
            "east" => Dir6::East,
            "up" => Dir6::Up,
            "down" => Dir6::Down,
            _ => return None,
        })
    }

    fn opposite(self) -> Dir6 {
        match self {
            Dir6::North => Dir6::South,
            Dir6::South => Dir6::North,
            Dir6::West => Dir6::East,
            Dir6::East => Dir6::West,
            Dir6::Up => Dir6::Down,
            Dir6::Down => Dir6::Up,
        }
    }

    fn step(self) -> [i32; 3] {
        match self {
            Dir6::North => [0, 0, -1],
            Dir6::South => [0, 0, 1],
            Dir6::West => [-1, 0, 0],
            Dir6::East => [1, 0, 0],
            Dir6::Up => [0, 1, 0],
            Dir6::Down => [0, -1, 0],
        }
    }

    fn step_y(self) -> i32 {
        self.step()[1]
    }

    /// Rotate a direction by `r` clockwise quarter turns (vertical
    /// axes are rotation-invariant).
    fn rotated(self, r: u8) -> Dir6 {
        let mut d = self;
        for _ in 0..(r % 4) {
            d = match d {
                Dir6::North => Dir6::East,
                Dir6::East => Dir6::South,
                Dir6::South => Dir6::West,
                Dir6::West => Dir6::North,
                v => v,
            };
        }
        d
    }
}

/// Rotate a local template position `r` clockwise quarter turns
/// around the origin (`StructureTemplate.transform`, pivot ZERO:
/// CW90 maps (x, z) -> (-z, x)).
pub(crate) fn rot_pos(r: u8, p: [i32; 3]) -> [i32; 3] {
    match r % 4 {
        0 => p,
        1 => [-p[2], p[1], p[0]],
        2 => [-p[0], p[1], -p[2]],
        _ => [p[2], p[1], -p[0]],
    }
}

/// World bounding box of a template anchored at `pos` with rotation
/// `r` (min/max over the rotated corner pair).
pub(crate) fn template_bb(size: [i32; 3], pos: [i32; 3], r: u8) -> Bb {
    let a = rot_pos(r, [0, 0, 0]);
    let b = rot_pos(r, [size[0] - 1, size[1] - 1, size[2] - 1]);
    Bb {
        x0: pos[0] + a[0].min(b[0]),
        y0: pos[1] + a[1].min(b[1]),
        z0: pos[2] + a[2].min(b[2]),
        x1: pos[0] + a[0].max(b[0]),
        y1: pos[1] + a[1].max(b[1]),
        z1: pos[2] + a[2].max(b[2]),
    }
}

// ----------------------------------------------------------------
// Jigsaw sockets.
// ----------------------------------------------------------------

/// One jigsaw socket of a placed (positioned + rotated) template.
#[derive(Debug, Clone)]
struct Socket {
    /// World position of the jigsaw cell.
    pos: [i32; 3],
    front: Dir6,
    top: Dir6,
    info: JigsawInfo,
}

/// The template's jigsaw blocks transformed to world space, shuffled,
/// then stable-sorted by selection priority (descending) - the
/// `getShuffledJigsawBlocks` contract.
fn shuffled_sockets(
    t: &StructureTemplate,
    pos: [i32; 3],
    rot: u8,
    rng: &mut SplitMix64,
) -> Vec<Socket> {
    let mut sockets: Vec<Socket> = t
        .jigsaws()
        .filter_map(|(cell, info)| {
            let orientation = t.palette[cell.state].prop("orientation")?;
            let (front_s, top_s) = orientation.split_once('_')?;
            let front = Dir6::parse(front_s)?.rotated(rot);
            let top = Dir6::parse(top_s)?.rotated(rot);
            let rp = rot_pos(rot, cell.pos);
            Some(Socket {
                pos: [pos[0] + rp[0], pos[1] + rp[1], pos[2] + rp[2]],
                front,
                top,
                info: info.clone(),
            })
        })
        .collect();
    shuffle(&mut sockets, rng);
    sockets.sort_by_key(|s| std::cmp::Reverse(s.info.selection_priority));
    sockets
}

/// `JigsawBlock.canAttach`.
fn can_attach(source: &Socket, target: &Socket) -> bool {
    source.front == target.front.opposite()
        && (source.info.joint_rollable || source.top == target.top)
        && source.info.target == target.info.name
}

// ----------------------------------------------------------------
// Pools (weight-expanded, shuffled) + pool max size.
// ----------------------------------------------------------------

/// The weight-expanded element list MC pools store (an element with
/// weight w appears w times), shuffled.
fn shuffled_elements(pool: &TemplatePool, rng: &mut SplitMix64) -> Vec<ElementKind> {
    let mut out: Vec<ElementKind> = Vec::new();
    for e in &pool.elements {
        for _ in 0..e.weight {
            out.push(e.kind.clone());
        }
    }
    shuffle(&mut out, rng);
    out
}

/// Weighted random element (`getRandomTemplate`).
fn random_element(pool: &TemplatePool, rng: &mut SplitMix64) -> Option<ElementKind> {
    let total = pool.total_weight();
    if total == 0 {
        return None;
    }
    let mut pick = (rng.next_u64() % total as u64) as u32;
    for e in &pool.elements {
        if pick < e.weight {
            return Some(e.kind.clone());
        }
        pick -= e.weight;
    }
    None
}

/// `StructureTemplatePool.getMaxSize`: the tallest Single template's
/// Y span (the expansion hack's headroom source). Cached per pool.
fn pool_max_size(id: &str) -> i32 {
    thread_local! {
        static CACHE: RefCell<std::collections::HashMap<String, i32>> =
            RefCell::new(std::collections::HashMap::new());
    }
    if let Some(hit) = CACHE.with(|c| c.borrow().get(id).copied()) {
        return hit;
    }
    let mut max = 0;
    if let Some(p) = village_data::pool(id) {
        for e in &p.elements {
            let loc = match &e.kind {
                ElementKind::Single { location, .. } => Some(location),
                ElementKind::List { first, .. } => Some(first),
                _ => None,
            };
            if let Some(loc) = loc
                && let Some(t) = village_data::template(loc)
            {
                max = max.max(t.size[1]);
            }
        }
    }
    CACHE.with(|c| c.borrow_mut().insert(id.to_string(), max));
    max
}

// ----------------------------------------------------------------
// Free-space ledger.
// ----------------------------------------------------------------

/// The port of the VoxelShape free-space context: an allowed region
/// minus the boxes already taken. MC's quarter-block deflate before
/// the overlap test makes face-touching legal; on integer boxes that
/// is exactly `!intersects` (shared faces don't share cells).
#[derive(Debug)]
struct FreeSpace {
    allowed: Bb,
    taken: Vec<Bb>,
}

impl FreeSpace {
    fn fits(&self, bb: &Bb) -> bool {
        bb.x0 >= self.allowed.x0
            && bb.x1 <= self.allowed.x1
            && bb.y0 >= self.allowed.y0
            && bb.y1 <= self.allowed.y1
            && bb.z0 >= self.allowed.z0
            && bb.z1 <= self.allowed.z1
            && !self.taken.iter().any(|t| t.intersects(bb))
    }

    fn take(&mut self, bb: Bb) {
        self.taken.push(bb);
    }
}

// ----------------------------------------------------------------
// The solver.
// ----------------------------------------------------------------

/// One resolved piece of a village layout.
#[derive(Debug, Clone)]
pub(crate) struct PlacedPiece {
    pub(crate) template: String,
    /// World position of the template's local origin cell.
    pub(crate) position: [i32; 3],
    /// Clockwise quarter turns.
    pub(crate) rotation: u8,
    /// The piece box the SOLVER reasons about - includes the
    /// expansion-hack headroom (Java stores the expanded box on the
    /// piece: interior children fit inside it). Carving derives the
    /// real footprint from `template_bb(size, position, rotation)`.
    pub(crate) bb: Bb,
    /// terrain_matching pieces drape onto the heightmap at carve
    /// time; rigid ones sit at their solved Y.
    pub(crate) terrain_matching: bool,
    /// `groundLevelDelta`: world ground = bb.y0 + delta (legacy
    /// elements are built one block below their ground line).
    pub(crate) ground_level_delta: i32,
    /// W251: list-element overlays carved over `template` at the same
    /// anchor (empty for plain single elements).
    pub(crate) overlays: Vec<ListOverlay>,
    /// W261: the element's processor-list reference (the ancient-city
    /// degradation rules) - applied per cell at carve time.
    pub(crate) processors: Option<String>,
}

/// The template + overlays + processors a pool element resolves to
/// (`None` for feature/empty kinds, which never become pieces).
type ElementParts = (String, bool, Vec<ListOverlay>, Option<String>);

fn element_parts(kind: ElementKind) -> Option<ElementParts> {
    match kind {
        ElementKind::Single { location, terrain_matching, processors } => {
            Some((location, terrain_matching, Vec::new(), processors))
        }
        ElementKind::List { first, overlays, terrain_matching, processors } => {
            Some((first, terrain_matching, overlays, processors))
        }
        _ => None,
    }
}

struct QueueEntry {
    piece_idx: usize,
    free: Rc<RefCell<FreeSpace>>,
    depth: i32,
    priority: i32,
    seq: usize,
}

/// `getFirstFreeHeight(WORLD_SURFACE_WG)` stand-in: one above the
/// post-carve solid floor.
fn first_free_height(ctx: &WorldgenCtx, x: i32, z: i32) -> i32 {
    mc_floor_at(ctx, x, z) + 1
}

/// W261: the per-structure jigsaw configuration (`JigsawStructure`
/// JSON fields the solver honours beyond depth).
pub(crate) struct SolveCfg {
    /// The structure `size` (villages 6, outposts / ancient city 7).
    pub(crate) max_depth: i32,
    /// `max_distance_from_center` horizontal (80 default, city 116).
    pub(crate) max_distance: i32,
    /// `Some(y)` = absolute `start_height`, no heightmap projection
    /// (ancient city -27); `None` = project the start piece onto the
    /// surface (`project_start_to_heightmap`, villages / outposts).
    pub(crate) start_y: Option<i32>,
    /// `start_jigsaw_name`: shift the start template so its jigsaw
    /// with this name lands on the anchor column (city_anchor).
    pub(crate) start_jigsaw: Option<&'static str>,
}

/// Solve one jigsaw layout anchored at `start_chunk` from
/// `start_pool`, recursing `max_depth` deep (the structure JSON
/// `size`: villages 6, outposts 7). Deterministic in (ctx seed,
/// start chunk, rng stream); `None` when the start pool rolls empty.
pub(crate) fn solve(
    ctx: &WorldgenCtx,
    start_chunk: [i32; 2],
    start_pool: &str,
    max_depth: i32,
    rng: &mut SplitMix64,
) -> Option<Vec<PlacedPiece>> {
    solve_cfg(
        ctx,
        start_chunk,
        start_pool,
        &SolveCfg {
            max_depth,
            max_distance: VILLAGE_MAX_DISTANCE,
            start_y: None,
            start_jigsaw: None,
        },
        rng,
    )
}

/// [`solve`] with the full config (`JigsawPlacement.addPieces`).
pub(crate) fn solve_cfg(
    ctx: &WorldgenCtx,
    start_chunk: [i32; 2],
    start_pool: &str,
    cfg: &SolveCfg,
    rng: &mut SplitMix64,
) -> Option<Vec<PlacedPiece>> {
    let cs = infengine_voxel::CHUNK_SIZE as i32;
    let anchor = [start_chunk[0] * cs, cfg.start_y.unwrap_or(0), start_chunk[1] * cs];
    let center_rotation = next_int(rng, 4) as u8;
    let start = village_data::pool(start_pool)?;
    let (location, terrain_matching, center_overlays, center_processors) =
        element_parts(random_element(&start, rng)?)?;
    let center_template = village_data::template(&location)?;
    // start_jigsaw_name: `position.subtract(localAnchorPosition)` -
    // the named jigsaw (random among matches via the shuffle) lands
    // exactly on the anchor.
    let position = if let Some(name) = cfg.start_jigsaw {
        let a = shuffled_sockets(&center_template, anchor, center_rotation, rng)
            .into_iter()
            .find(|s| s.info.name == name)?;
        [
            anchor[0] - (a.pos[0] - anchor[0]),
            anchor[1] - (a.pos[1] - anchor[1]),
            anchor[2] - (a.pos[2] - anchor[2]),
        ]
    } else {
        anchor
    };
    let raw_bb = template_bb(center_template.size, position, center_rotation);
    let center_x = (raw_bb.x0 + raw_bb.x1) / 2;
    let center_z = (raw_bb.z0 + raw_bb.z1) / 2;
    // groundLevelDelta 1 (every pool element); with a heightmap
    // projection the piece floor line lands on the surface, an
    // absolute start keeps `bottomY = adjustedPosition.getY()`.
    let ground_level_delta = 1;
    let bottom_y = if cfg.start_y.is_some() {
        position[1]
    } else {
        first_free_height(ctx, center_x, center_z)
    };
    let dy = bottom_y - (raw_bb.y0 + ground_level_delta);
    let center_bb = raw_bb.moved(0, dy, 0);
    let center_pos = [position[0], position[1] + dy, position[2]];
    let center_y = bottom_y;

    let mut pieces = vec![PlacedPiece {
        template: location,
        position: center_pos,
        rotation: center_rotation,
        bb: center_bb,
        terrain_matching,
        ground_level_delta,
        overlays: center_overlays,
        processors: center_processors,
    }];

    let world_free = Rc::new(RefCell::new(FreeSpace {
        allowed: Bb {
            x0: center_x - cfg.max_distance,
            // The single-int MaxDistance form leaves the vertical
            // unbounded - the world band clamps it (W260: -64..319).
            y0: -64,
            z0: center_z - cfg.max_distance,
            x1: center_x + cfg.max_distance,
            y1: 319,
            z1: center_z + cfg.max_distance,
        },
        taken: vec![center_bb],
    }));
    let _ = center_y;

    // SequencedPriorityIterator: highest placement priority first,
    // FIFO within a priority band.
    let mut queue: Vec<QueueEntry> = vec![QueueEntry {
        piece_idx: 0,
        free: world_free.clone(),
        depth: 0,
        priority: 0,
        seq: 0,
    }];
    let mut seq = 1usize;
    while !queue.is_empty() {
        let best = queue
            .iter()
            .enumerate()
            .max_by_key(|(_, e)| (e.priority, std::cmp::Reverse(e.seq)))
            .map(|(i, _)| i)
            .unwrap();
        let entry = queue.swap_remove(best);
        try_placing_children(
            ctx,
            &mut pieces,
            entry,
            &mut queue,
            &mut seq,
            cfg.max_depth,
            rng,
        );
    }
    Some(pieces)
}

/// `Placer.tryPlacingChildren`.
#[allow(clippy::too_many_arguments)]
fn try_placing_children(
    ctx: &WorldgenCtx,
    pieces: &mut Vec<PlacedPiece>,
    entry: QueueEntry,
    queue: &mut Vec<QueueEntry>,
    seq: &mut usize,
    max_depth: i32,
    rng: &mut SplitMix64,
) {
    let source = pieces[entry.piece_idx].clone();
    let Some(source_template) = village_data::template(&source.template) else {
        return;
    };
    let source_rigid = !source.terrain_matching;
    let source_bb = source.bb;
    let source_box_y = source_bb.y0;
    // Lazy per-source interior ledger (Java `sourceFree`).
    let mut interior: Option<Rc<RefCell<FreeSpace>>> = None;

    'sockets: for socket in
        shuffled_sockets(&source_template, source.position, source.rotation, rng)
    {
        let front = socket.front;
        let source_jigsaw_pos = socket.pos;
        let step = front.step();
        let target_jigsaw_pos = [
            source_jigsaw_pos[0] + step[0],
            source_jigsaw_pos[1] + step[1],
            source_jigsaw_pos[2] + step[2],
        ];
        let source_jigsaw_local_y = source_jigsaw_pos[1] - source_box_y;
        let mut source_base_height: Option<i32> = None;
        let Some(target_pool) = village_data::pool(&socket.info.pool) else {
            continue;
        };
        let fallback_pool = village_data::pool(&target_pool.fallback);

        let attach_inside = source_bb.contains(target_jigsaw_pos);
        let children_free: Rc<RefCell<FreeSpace>> = if attach_inside {
            interior
                .get_or_insert_with(|| {
                    Rc::new(RefCell::new(FreeSpace {
                        allowed: source_bb,
                        taken: Vec::new(),
                    }))
                })
                .clone()
        } else {
            entry.free.clone()
        };

        // Depth-capped pool + fallback, each weight-expanded +
        // shuffled, tried in order.
        let mut candidates: Vec<ElementKind> = Vec::new();
        if entry.depth != max_depth {
            candidates.extend(shuffled_elements(&target_pool, rng));
        }
        if let Some(f) = &fallback_pool {
            candidates.extend(shuffled_elements(f, rng));
        }

        for candidate in candidates {
            let (location, target_matching, target_overlays, target_processors) =
                match candidate {
                    // An explicit empty element terminates the socket.
                    ElementKind::Empty => break,
                    // Feature elements (street trees) consume the
                    // socket; the tree visual is a later wave.
                    ElementKind::Feature { .. } => continue 'sockets,
                    ElementKind::Single { location, terrain_matching, processors } => {
                        (location, terrain_matching, Vec::new(), processors)
                    }
                    ElementKind::List { first, overlays, terrain_matching, processors } => {
                        (first, terrain_matching, overlays, processors)
                    }
                };
            let Some(target_template) = village_data::template(&location) else {
                continue;
            };
            let mut rotations = [0u8, 1, 2, 3];
            shuffle(&mut rotations, rng);
            for target_rotation in rotations {
                let target_sockets = shuffled_sockets(
                    &target_template,
                    [0, 0, 0],
                    target_rotation,
                    rng,
                );
                // Expansion hack headroom for low target pieces.
                let hack_bb =
                    template_bb(target_template.size, [0, 0, 0], target_rotation);
                let expand_to = if hack_bb.y_span() <= 16 {
                    target_sockets
                        .iter()
                        .map(|s| {
                            let st = s.front.step();
                            let inward = [
                                s.pos[0] + st[0],
                                s.pos[1] + st[1],
                                s.pos[2] + st[2],
                            ];
                            if !hack_bb.contains(inward) {
                                return 0;
                            }
                            let child = pool_max_size(&s.info.pool);
                            let child_fb = village_data::pool(&s.info.pool)
                                .map(|p| pool_max_size(&p.fallback))
                                .unwrap_or(0);
                            child.max(child_fb)
                        })
                        .max()
                        .unwrap_or(0)
                } else {
                    0
                };

                for target_socket in &target_sockets {
                    if !can_attach(&socket, target_socket) {
                        continue;
                    }
                    let tj_local = target_socket.pos;
                    let raw_box_pos = [
                        target_jigsaw_pos[0] - tj_local[0],
                        target_jigsaw_pos[1] - tj_local[1],
                        target_jigsaw_pos[2] - tj_local[2],
                    ];
                    let raw_bb =
                        template_bb(target_template.size, raw_box_pos, target_rotation);
                    let target_rigid = !target_matching;
                    let delta_y =
                        source_jigsaw_local_y - tj_local[1] + front.step_y();
                    let target_box_y = if source_rigid && target_rigid {
                        source_box_y + delta_y
                    } else {
                        let h = *source_base_height.get_or_insert_with(|| {
                            first_free_height(
                                ctx,
                                source_jigsaw_pos[0],
                                source_jigsaw_pos[2],
                            )
                        });
                        h - tj_local[1]
                    };
                    let y_offset = target_box_y - raw_bb.y0;
                    let mut target_bb = raw_bb.moved(0, y_offset, 0);
                    let target_pos = [
                        raw_box_pos[0],
                        raw_box_pos[1] + y_offset,
                        raw_box_pos[2],
                    ];
                    if expand_to > 0 {
                        let grow = (expand_to + 1).max(target_bb.y1 - target_bb.y0);
                        target_bb.y1 = target_bb.y1.max(target_bb.y0 + grow);
                    }
                    if !children_free.borrow().fits(&target_bb) {
                        continue;
                    }
                    children_free.borrow_mut().take(target_bb);
                    let target_gld = if target_rigid {
                        source.ground_level_delta - delta_y
                    } else {
                        1
                    };
                    let idx = pieces.len();
                    pieces.push(PlacedPiece {
                        template: location.clone(),
                        position: target_pos,
                        rotation: target_rotation,
                        // The EXPANDED box (Java keeps it on the
                        // piece) - street plates must span the
                        // headroom their interior houses occupy.
                        bb: target_bb,
                        terrain_matching: target_matching,
                        ground_level_delta: target_gld,
                        overlays: target_overlays.clone(),
                        processors: target_processors.clone(),
                    });
                    if entry.depth < max_depth {
                        queue.push(QueueEntry {
                            piece_idx: idx,
                            free: children_free.clone(),
                            depth: entry.depth + 1,
                            priority: socket.info.placement_priority,
                            seq: *seq,
                        });
                        *seq += 1;
                    }
                    continue 'sockets;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotations_cycle_positions_and_directions() {
        assert_eq!(rot_pos(1, [3, 5, 2]), [-2, 5, 3]);
        assert_eq!(rot_pos(2, [3, 5, 2]), [-3, 5, -2]);
        assert_eq!(rot_pos(3, [3, 5, 2]), [2, 5, -3]);
        assert_eq!(Dir6::North.rotated(1), Dir6::East);
        assert_eq!(Dir6::West.rotated(2), Dir6::East);
        assert_eq!(Dir6::Up.rotated(3), Dir6::Up);
        // A size-anchored bb under CW90 lands in the -x quadrant.
        let bb = template_bb([4, 3, 6], [10, 20, 30], 1);
        assert_eq!((bb.x0, bb.x1), (10 - 5, 10));
        assert_eq!((bb.z0, bb.z1), (30, 30 + 3));
        assert_eq!((bb.y0, bb.y1), (20, 22));
    }

    #[test]
    fn free_space_face_touch_is_legal_overlap_is_not() {
        let mut f = FreeSpace {
            allowed: Bb { x0: -100, y0: 0, z0: -100, x1: 100, y1: 255, z1: 100 },
            taken: vec![Bb { x0: 0, y0: 60, z0: 0, x1: 9, y1: 70, z1: 9 }],
        };
        // Sharing the x=9|10 face: legal.
        assert!(f.fits(&Bb { x0: 10, y0: 60, z0: 0, x1: 19, y1: 70, z1: 9 }));
        // Overlapping one cell: rejected.
        assert!(!f.fits(&Bb { x0: 9, y0: 60, z0: 0, x1: 18, y1: 70, z1: 9 }));
        // Outside the allowed region: rejected.
        assert!(!f.fits(&Bb { x0: 95, y0: 60, z0: 0, x1: 105, y1: 70, z1: 9 }));
        f.take(Bb { x0: 10, y0: 60, z0: 0, x1: 19, y1: 70, z1: 9 });
        assert!(!f.fits(&Bb { x0: 12, y0: 60, z0: 2, x1: 13, y1: 61, z1: 3 }));
    }

    #[test]
    fn village_layout_solves_deterministically() {
        let ctx = crate::worldgen::WorldgenCtx {
            base_seed: 3592732596,
            dim: "overworld".into(),
            profile: crate::worldgen::WorldGen::Mc,
        };
        let mut rng = SplitMix64::new(0xB00B1E5);
        let a = solve(
            &ctx,
            [300, 300],
            "minecraft:village/plains/town_centers",
            VILLAGE_MAX_DEPTH,
            &mut rng,
        )
        .expect("layout solves");
        let mut rng2 = SplitMix64::new(0xB00B1E5);
        let b = solve(
            &ctx,
            [300, 300],
            "minecraft:village/plains/town_centers",
            VILLAGE_MAX_DEPTH,
            &mut rng2,
        )
        .expect("layout solves twice");
        assert_eq!(a.len(), b.len(), "solver must be deterministic");
        for (pa, pb) in a.iter().zip(&b) {
            assert_eq!(pa.template, pb.template);
            assert_eq!(pa.bb, pb.bb);
            assert_eq!(pa.rotation, pb.rotation);
        }
        // A village is more than a lone fountain...
        assert!(a.len() >= 8, "village came out tiny: {} pieces", a.len());
        // ...respects the max distance envelope...
        let center = a[0].bb;
        let cx = (center.x0 + center.x1) / 2;
        let cz = (center.z0 + center.z1) / 2;
        for p in &a {
            assert!(
                p.bb.x0 >= cx - VILLAGE_MAX_DISTANCE
                    && p.bb.x1 <= cx + VILLAGE_MAX_DISTANCE
                    && p.bb.z0 >= cz - VILLAGE_MAX_DISTANCE
                    && p.bb.z1 <= cz + VILLAGE_MAX_DISTANCE,
                "piece escaped the envelope: {:?}",
                p.bb
            );
        }
        // ...and no two pieces of one free-space context overlap. The
        // solver guarantees non-overlap only per ledger (interior
        // pieces legitimately overlap their host), so check the world
        // ledger members: pieces whose box is NOT inside another's.
        for (i, pa) in a.iter().enumerate() {
            for pb in &a[i + 1..] {
                let inside = |inner: &Bb, outer: &Bb| {
                    inner.x0 >= outer.x0
                        && inner.x1 <= outer.x1
                        && inner.y0 >= outer.y0
                        && inner.y1 <= outer.y1
                        && inner.z0 >= outer.z0
                        && inner.z1 <= outer.z1
                };
                if pa.bb.intersects(&pb.bb) {
                    assert!(
                        inside(&pb.bb, &pa.bb) || inside(&pa.bb, &pb.bb),
                        "non-nested overlap between {:?} and {:?}",
                        pa.template,
                        pb.template
                    );
                }
            }
        }
    }

    /// Probe: solve + summarise a village layout for a seed/chunk.
    /// `INFENGINE_DUMP_SEED=<seed> INFENGINE_VILLAGE_CX/CZ=<chunk>
    /// cargo test -p voxel_demo_mp --release --lib dump_village_layout
    /// -- --ignored --nocapture`
    #[test]
    #[ignore]
    #[allow(clippy::print_stdout)]
    fn dump_village_layout() {
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
        let (cx, cz) = (
            env_i32("INFENGINE_VILLAGE_CX", 300),
            env_i32("INFENGINE_VILLAGE_CZ", 300),
        );
        let ctx = crate::worldgen::WorldgenCtx {
            base_seed: seed,
            dim: std::env::var("INFENGINE_DUMP_DIM")
                .unwrap_or_else(|_| "overworld".into()),
            profile: crate::worldgen::WorldGen::Mc,
        };
        let mut rng = SplitMix64::new(0xB00B1E5);
        let pieces = solve(
            &ctx,
            [cx, cz],
            "minecraft:village/plains/town_centers",
            VILLAGE_MAX_DEPTH,
            &mut rng,
        )
        .expect("layout solves");
        println!("village at chunk ({cx}, {cz}): {} pieces", pieces.len());
        let mut by_kind: std::collections::BTreeMap<&str, usize> =
            std::collections::BTreeMap::new();
        for p in &pieces {
            let kind = p
                .template
                .rsplit('/')
                .nth(1)
                .unwrap_or("?");
            *by_kind.entry(kind).or_insert(0) += 1;
        }
        for (k, n) in &by_kind {
            println!("  {n:>3}x {k}");
        }
        for p in pieces.iter().take(12) {
            println!(
                "  {:?} rot{} {} gld{} {}",
                p.bb,
                p.rotation,
                if p.terrain_matching { "terrain" } else { "rigid" },
                p.ground_level_delta,
                p.template
            );
        }
    }
}
