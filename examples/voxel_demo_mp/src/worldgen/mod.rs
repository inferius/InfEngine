//! World-generation module: biome lookup, height curves, ore +
//! tree + flower + cactus decorations, and the generator profile
//! enum (Classic / Flat / Islands / Amplified / Realistic).
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
//!
//! Module layout (Phase E split):
//! - `biome`: Biome enum + classic 3-biome map noise.
//! - `height`: per-biome height curve + cave carve.
//! - `ore`: deterministic 3D ore hash + ore pick.
//! - `trees`: tree placement loop + species dispatch (oak only
//!   in Classic; pine/birch/sequoia/jungle/savanna once
//!   `Realistic` lands).
//! - `flowers` / `cactus`: small surface decoration passes.
//! - `chunk`: top-level `generate_chunk_at` orchestrator + the
//!   `Flat` / `Islands` flat-noise profiles.

pub(crate) mod biome;
pub(crate) mod cactus;
pub(crate) mod chunk;
pub(crate) mod climate;
pub(crate) mod flowers;
pub(crate) mod height;
pub(crate) mod ore;
pub(crate) mod trees;

// Public re-exports for the surface the rest of the bin + lib
// depends on. Keep this list tight; new internals stay inside the
// module unless someone has a reason to call them.
pub(crate) use biome::{Biome, biome_at};
pub use chunk::generate_chunk_at;

/// Global world-generation seed. Mixed into every noise function so
/// distinct hosts (or /seed-cycled sessions) produce different
/// terrain + ore distributions while staying deterministic across
/// workers and clients running the same value. Set on host start
/// + replicated to clients via `vx.seed`.
static WORLD_SEED: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(0);

pub fn world_seed() -> u32 {
    WORLD_SEED.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn set_world_seed(value: u32) {
    WORLD_SEED.store(value, std::sync::atomic::Ordering::Relaxed);
}

/// Mix the seed into a deterministic float used as a phase
/// offset for the sin/cos noise stack. Picking a small range
/// keeps the curves visually similar across seeds while still
/// producing distinct terrain.
pub(crate) fn seed_offset() -> f32 {
    (world_seed().wrapping_mul(0x9E37_79B1) as f32 / u32::MAX as f32) * 1000.0
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
pub enum WorldGen {
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
    /// Climate-driven biome map with 10+ biomes + epic trees.
    /// Phase E. Temperature / humidity / elevation noises pick
    /// per-cell biome; each biome has its own height curve,
    /// surface block, and tree species.
    Realistic = 4,
}

impl WorldGen {
    // Named `from_str` for legibility on call sites. Doesn't
    // implement `std::str::FromStr` because the trait expects
    // `Result`, not `Option`, and we'd lose the "unknown variant"
    // signal that the callers rely on.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "classic" | "default" => Some(Self::Classic),
            "flat" => Some(Self::Flat),
            "islands" | "island" => Some(Self::Islands),
            "amplified" | "amp" => Some(Self::Amplified),
            "realistic" | "real" => Some(Self::Realistic),
            _ => None,
        }
    }
    pub fn current() -> Self {
        match WORLDGEN.load(std::sync::atomic::Ordering::Relaxed) {
            1 => Self::Flat,
            2 => Self::Islands,
            3 => Self::Amplified,
            4 => Self::Realistic,
            _ => Self::Classic,
        }
    }
}

/// Shared world-gen mode. Set once on startup, read from every
/// worker thread that generates chunks. `Relaxed` is fine because
/// the value never changes mid-run.
pub static WORLDGEN: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Test-only mutex shared by every test that touches the global
/// `WORLD_SEED` + `WORLDGEN` atomics. Without it, parallel tests
/// in the worldgen tree race over the seed and produce
/// non-deterministic terrain mid-test. Each test should grab this
/// at the top of its body. Note: must be `pub(crate)` so the
/// `worldgen_baseline_test` sibling module + the sub-modules can
/// share the same instance.
#[cfg(test)]
pub(crate) static TEST_GEN_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
