//! World-generation module: biome lookup, height curves, ore +
//! tree + flower + cactus decorations, and the four named
//! generator profiles (Classic / Flat / Islands / Amplified).
//!
//! Determinism: every noise function mixes `world_seed()` into
//! its hash. Two peers running the same generator with the
//! same seed produce byte-identical chunks, so the workers +
//! the host + every client all agree on the world without
//! shipping the bulk terrain over the wire.
//!
//! The shared seed + generator profile sit in atomic statics
//! (`WORLD_SEED`, `WORLDGEN`) so background worker threads can
//! read them lock-free. They're set once on host start (or
//! after `/seed` / `/worldgen`) and replicated to clients via
//! `vx.seed`.

use infengine_voxel::{BlockId, CHUNK_SIZE, Chunk, ChunkCoord, World};

/// Three large-scale biomes drive both the terrain height curve and
/// the surface block. Biome selection rides on a slow 2D noise so a
/// peer running the same generator picks the same biome at every
/// (wx, wz) - no extra net traffic needed.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum Biome {
    Desert,
    Plains,
    Mountain,
}

/// Global world-generation seed. Mixed into every noise function so
/// distinct hosts (or /seed-cycled sessions) produce different
/// terrain + ore distributions while staying deterministic across
/// workers and clients running the same value. Set on host start
/// + replicated to clients via `vx.seed`.
static WORLD_SEED: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(0);

pub(crate) fn world_seed() -> u32 {
    WORLD_SEED.load(std::sync::atomic::Ordering::Relaxed)
}

pub(crate) fn set_world_seed(value: u32) {
    WORLD_SEED.store(value, std::sync::atomic::Ordering::Relaxed);
}

fn seed_offset() -> f32 {
    // Mix the seed into a deterministic float used as a phase
    // offset for the sin/cos noise stack. Picking a small range
    // keeps the curves visually similar across seeds while still
    // producing distinct terrain.
    (world_seed().wrapping_mul(0x9E37_79B1) as f32 / u32::MAX as f32) * 1000.0
}

pub(crate) fn biome_at(wx: f32, wz: f32) -> Biome {
    // Two layered sin/cos waves keep the biome map smooth and
    // deterministic. Range maps roughly to `[-1, 1]` so the cutoffs
    // are easy to reason about. A seed offset shifts the wave so
    // distinct hosts get different biome maps. Cutoffs are tuned
    // so Plains dominates (~70%) with Mountain + Desert as
    // accents - looks more like a starter world than a wall of
    // stone.
    let s = seed_offset();
    let n = ((wx + s) * 0.012).sin() * 0.6
        + ((wz + s) * 0.009).cos() * 0.5
        + (((wx + wz) + s) * 0.004).sin() * 0.3;
    if n < -0.95 {
        Biome::Desert
    } else if n > 0.95 {
        Biome::Mountain
    } else {
        Biome::Plains
    }
}

/// Cheap, deterministic 3D hash used by the ore generator. Mixes
/// integer voxel coordinates + a per-ore-kind salt into a [0, 1]
/// float. Two octaves of bit-rotation give a noticeably "veiny"
/// distribution rather than salt-and-pepper.
pub(crate) fn ore_hash(x: i32, y: i32, z: i32, salt: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x27D4_EB2F);
    h ^= (y as u32).wrapping_mul(0x9E37_79B1);
    h ^= (z as u32).wrapping_mul(0x1656_67B1);
    h ^= salt ^ world_seed();
    h = h.wrapping_mul(0x85EB_CA6B);
    h ^= h >> 13;
    h = h.wrapping_mul(0xC2B2_AE35);
    h ^= h >> 16;
    // Blob softening: average with a neighbour cell so the result
    // forms small pockets instead of isolated voxels.
    let mut g = (x as u32 ^ y as u32 ^ z as u32).wrapping_mul(salt | 1);
    g ^= g >> 17;
    g = g.wrapping_mul(0xC2B2_AE35);
    let a = h as f32 / u32::MAX as f32;
    let b = g as f32 / u32::MAX as f32;
    (a * 0.7 + b * 0.3).clamp(0.0, 1.0)
}

/// Cheap, deterministic 2D hash. Used for tree placement so two
/// peers running the same generator drop trees in the same spots.
pub(crate) fn hash2(wx: i32, wz: i32) -> u32 {
    let mut h = (wx as u32).wrapping_mul(0x27D4_EB2F);
    h ^= (wz as u32).wrapping_mul(0x1656_67B1);
    h ^= world_seed();
    h ^= h >> 16;
    h = h.wrapping_mul(0x85EB_CA6B);
    h ^= h >> 13;
    h.wrapping_mul(0xC2B2_AE35)
}

/// World-generation profile. Selected once at startup (via the
/// `--worldgen=<name>` CLI flag) and read by every chunk-generator
/// call so background workers stay in sync without plumbing a
/// shared param through every call. Loaded into `WORLDGEN`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorldGen {
    /// Default rolling biomes (Plains / Mountain / Desert) with
    /// small oak trees in plains.
    Classic = 0,
    /// Pancake-flat dirt + grass at y=8. Builders welcome.
    Flat = 1,
    /// Sparse islands rising out of empty space. Walk-the-plank
    /// vibes; great test for fall-out penalty.
    Islands = 2,
    /// Like Classic but with the height map amplified ~2.5x.
    /// Tall mountains, deep valleys, more dramatic skyline.
    Amplified = 3,
}

impl WorldGen {
    pub(crate) fn from_str(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "classic" | "default" => Some(Self::Classic),
            "flat" => Some(Self::Flat),
            "islands" | "island" => Some(Self::Islands),
            "amplified" | "amp" => Some(Self::Amplified),
            _ => None,
        }
    }
    pub(crate) fn current() -> Self {
        match WORLDGEN.load(std::sync::atomic::Ordering::Relaxed) {
            1 => Self::Flat,
            2 => Self::Islands,
            3 => Self::Amplified,
            _ => Self::Classic,
        }
    }
}

/// Shared world-gen mode. Set once on startup, read from every
/// worker thread that generates chunks. `Relaxed` is fine because
/// the value never changes mid-run.
pub(crate) static WORLDGEN: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Top-level chunk generator. Dispatches to the per-profile
/// implementation based on the currently-loaded `WORLDGEN`.
pub(crate) fn generate_chunk_at(coord: ChunkCoord) -> Chunk {
    match WorldGen::current() {
        WorldGen::Classic => gen_classic(coord, 1.0),
        WorldGen::Amplified => gen_classic(coord, 2.5),
        WorldGen::Flat => gen_flat(coord),
        WorldGen::Islands => gen_islands(coord),
    }
}

/// Pancake-flat terrain at y=8: one row of grass, three rows of
/// dirt, stone below. Empty above. Trees disabled.
fn gen_flat(coord: ChunkCoord) -> Chunk {
    let origin = World::chunk_world_origin(coord);
    let mut chunk = Chunk::empty();
    chunk.fill_with(|_x, y, _z| {
        let wy = origin.y as i32 + y as i32;
        match wy {
            n if n > 8 => BlockId::AIR,
            8 => BlockId(3),     // grass
            5..=7 => BlockId(2), // dirt
            _ => BlockId(1),     // stone
        }
    });
    chunk
}

/// Sparse-island terrain: only where 2D noise exceeds a threshold
/// does land rise; everywhere else is bottomless air. Trees grow
/// on the island tops.
fn gen_islands(coord: ChunkCoord) -> Chunk {
    let origin = World::chunk_world_origin(coord);
    let mut chunk = Chunk::empty();
    let mask = |wx: f32, wz: f32| -> f32 {
        // Two octaves of sin/cos; treat the result as height
        // potential. Anything < threshold = ocean (air).
        let a = (wx * 0.04).sin() + (wz * 0.05).cos();
        let b = (wx * 0.11 + 17.0).sin() * 0.5 + (wz * 0.13 + 9.0).cos() * 0.5;
        a + b * 0.7
    };
    chunk.fill_with(|x, y, z| {
        let wx = origin.x + x as f32;
        let wz = origin.z + z as f32;
        let m = mask(wx, wz);
        if m <= 0.55 {
            return BlockId::AIR;
        }
        let h = 10 + ((m - 0.55) * 18.0).round() as i32;
        let wy = origin.y as i32 + y as i32;
        if wy > h {
            return BlockId::AIR;
        }
        if wy == h {
            BlockId(3) // grass crown
        } else if wy >= h - 2 {
            BlockId(2) // dirt
        } else {
            BlockId(1) // stone core
        }
    });
    chunk
}

/// Classic biome-driven terrain. `amp_scale` lets the Amplified
/// preset reuse this generator with a 2.5x vertical multiplier.
fn gen_classic(coord: ChunkCoord, amp_scale: f32) -> Chunk {
    let origin = World::chunk_world_origin(coord);
    let height_at = |wx: f32, wz: f32, biome: Biome| -> i32 {
        let base = match biome {
            Biome::Desert => 12.0,
            Biome::Plains => 18.0,
            Biome::Mountain => 30.0,
        };
        let amp = (match biome {
            Biome::Desert => 3.0,
            Biome::Plains => 6.0,
            Biome::Mountain => 18.0,
        }) * amp_scale;
        let h = base
            + ((wx * 0.13).sin() * 0.5 + (wz * 0.17).cos() * 0.5) * amp
            + ((wx + wz) * 0.07).sin() * amp * 0.4;
        h.round() as i32
    };
    let mut chunk = Chunk::empty();
    // Cheap procedural cave check: 3D noise made of sin/cos
    // octaves. Returns true when (wx, wy, wz) sits inside a void
    // pocket. Tuned so caves only carve into stone (under the
    // grass / dirt cap) + leave the surface intact, and so two
    // octaves combine into a "blobby tunnel" shape rather than
    // single-block holes.
    let cave_at = |wx: f32, wy: f32, wz: f32| -> bool {
        if wy < 1.0 {
            return false; // keep a bedrock floor at y=0
        }
        let a = (wx * 0.09).sin() * 0.5
            + (wz * 0.08).cos() * 0.5
            + (wy * 0.12).sin() * 0.5;
        let b = (wx * 0.20 + 17.0).sin() * (wy * 0.16 + 9.0).cos()
            + (wz * 0.21 + 31.0).sin() * 0.6;
        let n = a * 0.55 + b * 0.45;
        n > 0.78
    };
    chunk.fill_with(|x, y, z| {
        let wx = origin.x + x as f32;
        let wz = origin.z + z as f32;
        let biome = biome_at(wx, wz);
        let h = height_at(wx, wz, biome);
        let wy = origin.y as i32 + y as i32;
        if wy > h {
            return BlockId::AIR;
        }
        // Cave carve: only below the 3-block dirt / grass cap so
        // we don't punch holes through the surface.
        if wy < h - 3 && cave_at(wx, wy as f32, wz) {
            return BlockId::AIR;
        }
        // Surface block per biome. Mountains above the snow line
        // swap to snow (id 15) for visual identity.
        let surface = match biome {
            Biome::Desert => BlockId(4),
            Biome::Plains => BlockId(3),
            Biome::Mountain => {
                if h >= 26 {
                    BlockId(15)
                } else {
                    BlockId(1)
                }
            }
        };
        let near_surface = match biome {
            Biome::Desert => BlockId(26), // sandstone under sand cap
            Biome::Plains => BlockId(2),
            Biome::Mountain => BlockId(1),
        };
        if wy == h {
            surface
        } else if wy >= h - 3 {
            near_surface
        } else {
            // Stone interior: roll for ore veins. Two deterministic
            // 3D hashes seed coal + iron pockets; each is a noise
            // value that exceeds a threshold rarely. Coal is more
            // common + can live high; iron is rarer + only deep.
            // Mountain biome boosts both ore probabilities (lower
            // thresholds) and raises the iron Y ceiling so peaks
            // become genuinely worth digging into.
            let ox = wx as i32;
            let oy = wy;
            let oz = wz as i32;
            let h_coal = ore_hash(ox, oy, oz, 0x9E37_79B9);
            let h_iron = ore_hash(ox, oy, oz, 0x85EB_CA6B);
            let (iron_thr, iron_max_y, coal_thr) = match biome {
                Biome::Mountain => (0.965, 24, 0.90),
                _ => (0.985, 14, 0.94),
            };
            if h_iron > iron_thr && oy < iron_max_y {
                BlockId(14)
            } else if h_coal > coal_thr {
                BlockId(13)
            } else {
                BlockId(1)
            }
        }
    });
    // Tree pass: in plains biomes drop a small oak with wood pillar
    // + a leafy canopy. We loop over (-2 .. CHUNK_SIZE+2) so trees
    // anchored just outside the chunk still drop their leaves into
    // this chunk - otherwise canopies that overhang the boundary
    // would clip. Each block is only written when its `(lx, lz)`
    // pair sits inside the current chunk.
    let cs = CHUNK_SIZE as i32;
    let set_in_chunk = |chunk: &mut Chunk, lx: i32, ly: i32, lz: i32, block: BlockId| {
        if lx < 0 || lx >= cs || lz < 0 || lz >= cs || ly < 0 || ly >= cs {
            return;
        }
        chunk.set(lx as usize, ly as usize, lz as usize, block);
    };
    for x in -2..(cs + 2) {
        for z in -2..(cs + 2) {
            let wx = origin.x as i32 + x;
            let wz = origin.z as i32 + z;
            let biome = biome_at(wx as f32 + 0.5, wz as f32 + 0.5);
            if biome != Biome::Plains {
                continue;
            }
            // Tree density: roughly 1 tree per 32 cells in plains.
            // Lower modulus = greener world.
            if hash2(wx, wz) % 32 != 7 {
                continue;
            }
            let ground = height_at(wx as f32 + 0.5, wz as f32 + 0.5, biome);
            // Trunk: 4 wood blocks above the surface voxel.
            for ty in 1..=4 {
                let wy = ground + ty;
                let local_y = wy - origin.y as i32;
                set_in_chunk(&mut chunk, x, local_y, z, BlockId(5));
            }
            // Canopy: two 5x5 layers (corners trimmed) at trunk
            // top -1 / 0, a 3x3 cap above, and a single top block.
            for layer in 0i32..2 {
                let wy = ground + 3 + layer;
                let local_y = wy - origin.y as i32;
                for dx in -2i32..=2 {
                    for dz in -2i32..=2 {
                        if dx.abs() == 2 && dz.abs() == 2 {
                            continue;
                        }
                        if dx == 0 && dz == 0 && layer == 0 {
                            continue;
                        }
                        set_in_chunk(&mut chunk, x + dx, local_y, z + dz, BlockId(6));
                    }
                }
            }
            {
                let wy = ground + 5;
                let local_y = wy - origin.y as i32;
                for dx in -1i32..=1 {
                    for dz in -1i32..=1 {
                        set_in_chunk(&mut chunk, x + dx, local_y, z + dz, BlockId(6));
                    }
                }
            }
            // Single block top.
            {
                let wy = ground + 6;
                let local_y = wy - origin.y as i32;
                set_in_chunk(&mut chunk, x, local_y, z, BlockId(6));
            }
        }
    }
    // Flower pass: sparse decorations on plains-biome grass.
    // Uses a different hash salt than trees so the patterns
    // don't overlap. Each candidate cell only blooms when the
    // ground voxel is grass + the air block above is empty.
    for x in 0..cs {
        for z in 0..cs {
            let wx = origin.x as i32 + x;
            let wz = origin.z as i32 + z;
            let biome = biome_at(wx as f32 + 0.5, wz as f32 + 0.5);
            if biome != Biome::Plains {
                continue;
            }
            if hash2(wx ^ 0x9A75, wz ^ 0x5BA9) % 18 != 3 {
                continue;
            }
            let ground = height_at(wx as f32 + 0.5, wz as f32 + 0.5, biome);
            let local_y = (ground + 1) - origin.y as i32;
            // Only place on top of grass + into air.
            if local_y < 0 || local_y >= cs {
                continue;
            }
            let surface_y = local_y - 1;
            if surface_y < 0 || surface_y >= cs {
                continue;
            }
            if chunk.get(x as usize, surface_y as usize, z as usize).0 != 3 {
                continue;
            }
            if chunk.get(x as usize, local_y as usize, z as usize).0 != 0 {
                continue;
            }
            // Pick one of three flower colours from the cell
            // hash so the field reads varied without burning a
            // new noise tap.
            let h = hash2(wx ^ 0x5FAD, wz ^ 0x9117);
            let flower_id = match h % 3 {
                0 => 21u16, // yellow
                1 => 22,    // blue
                _ => 20,    // red
            };
            set_in_chunk(&mut chunk, x, local_y, z, BlockId(flower_id));
        }
    }
    // Pond pass disabled: removed from worldgen while the
    // fluid sim is under debugging. Sources are placed only by
    // bucket pours; the worldgen no longer seeds standing water.
    // Cactus pass: sparse 2-3 block columns on desert sand. Same
    // surface + air checks as flowers; height varies with the
    // hash so dunes don't look uniform.
    for x in 0..cs {
        for z in 0..cs {
            let wx = origin.x as i32 + x;
            let wz = origin.z as i32 + z;
            let biome = biome_at(wx as f32 + 0.5, wz as f32 + 0.5);
            if biome != Biome::Desert {
                continue;
            }
            let h = hash2(wx ^ 0xCAC7, wz ^ 0x1A55);
            if h % 24 != 5 {
                continue;
            }
            let ground = height_at(wx as f32 + 0.5, wz as f32 + 0.5, biome);
            let height_voxels = 2 + ((h >> 4) % 2) as i32; // 2 or 3
            for step in 1..=height_voxels {
                let wy = ground + step;
                let local_y = wy - origin.y as i32;
                if local_y < 0 || local_y >= cs {
                    continue;
                }
                set_in_chunk(&mut chunk, x, local_y, z, BlockId(27));
            }
        }
    }
    chunk
}
