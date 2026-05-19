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
pub(crate) mod biome_table;
pub(crate) mod cactus;
pub(crate) mod chunk;
pub(crate) mod climate;
pub(crate) mod flowers;
pub(crate) mod height;
pub(crate) mod nether;
#[cfg(test)]
mod nether_baseline_test;
pub(crate) mod ore;
pub(crate) mod trees;

// Public re-exports for the surface the rest of the bin + lib
// depends on. Keep this list tight; new internals stay inside the
// module unless someone has a reason to call them.
pub(crate) use biome::{Biome, biome_at};
pub use chunk::generate_chunk_at;
// F.2a: pure-ctx entry point exists but the bin doesn't reach for
// it yet (later sub-phases route the worker pool through it). Mute
// the unused-import lint here so the bin's clippy pass stays
// clean. The `dead_code` allow covers the leaf fn since the bin
// re-mounts the source.
#[allow(unused_imports, dead_code)]
pub use chunk::generate_chunk_at_ctx;

/// Pure-function bundle of every input the worldgen call tree
/// needs. Built at the edge of the generator (chunk-worker enqueue
/// / chat-command / save-load) and threaded by reference through
/// every gen / noise / decoration call. Two calls with the same
/// `WorldgenCtx` + same coord produce byte-identical chunks no
/// matter what the global `WORLD_SEED` / `WORLDGEN` atomics say.
///
/// Phase F.2: introduced to fix a race where background workers
/// (and reload-from-disk chunk regens) would sample the atomics
/// at a different moment than the caller intended, producing
/// terrain that no longer matched the chunk's stored block data
/// when the player walked back to a previously-visited area
/// after a dim swap or `/worldgen` command.
#[allow(dead_code)] // Used by bin-side workers once F.2d lands.
#[derive(Debug, Clone)]
pub struct WorldgenCtx {
    /// Base seed from `world.json::seed`. The same across all
    /// dims of one world; the per-dim split is done by
    /// `effective_seed`.
    pub base_seed: u32,
    /// Name of the dimension this gen call belongs to (e.g.
    /// `"overworld"`, `"nether"`). Folded into the effective seed
    /// so two dims sharing one base seed produce different chunk
    /// lattices.
    pub dim: String,
    /// World-gen profile to run for this dim. Per-dim, not global -
    /// in a multi-dim save the overworld may be `Classic` while
    /// the nether is `Nether`.
    pub profile: WorldGen,
}

impl WorldgenCtx {
    /// `base_seed XOR FNV1a(dim_name)`. Same algorithm as the free
    /// `dim_seed`; kept as a method so callers can avoid importing
    /// the helper when they only need the effective value.
    ///
    /// Special case: if `dim` is empty, returns `base_seed`
    /// unchanged. The legacy-bridge wrapper
    /// (`generate_chunk_at` -> `generate_chunk_at_ctx`) builds a
    /// ctx from the already-effective `WORLD_SEED` atomic + an
    /// empty dim name; skipping the dim mix there keeps the noise
    /// pipeline byte-identical to the pre-refactor output.
    #[allow(dead_code)] // Used by bin-side workers once F.2d lands.
    pub fn effective_seed(&self) -> u32 {
        if self.dim.is_empty() {
            self.base_seed
        } else {
            dim_seed(self.base_seed, &self.dim)
        }
    }
}

impl Default for WorldgenCtx {
    /// The defaults match a fresh `WorldJson::create_world(
    /// "classic", 0)`: base seed 0, overworld dim, Classic
    /// profile. Lets tests that don't care about ctx construct
    /// one trivially.
    fn default() -> Self {
        Self {
            base_seed: 0,
            dim: "overworld".to_string(),
            profile: WorldGen::Classic,
        }
    }
}

/// Global world-generation seed. Mixed into every noise function so
/// distinct hosts (or /seed-cycled sessions) produce different
/// terrain + ore distributions while staying deterministic across
/// workers and clients running the same value. Set on host start
/// + replicated to clients via `vx.seed`.
///
/// Phase F: this atomic stores the ACTIVE dimension's effective
/// seed (`dim_seed(base, current_dim)`). Switching dims rewrites
/// it; the base seed is the one stored in `world.json::seed`.
static WORLD_SEED: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(0);

pub fn world_seed() -> u32 {
    WORLD_SEED.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn set_world_seed(value: u32) {
    WORLD_SEED.store(value, std::sync::atomic::Ordering::Relaxed);
}

/// FNV-1a 32-bit hash of a dimension name. Deterministic across
/// platforms + Rust versions (the algorithm is specified, not
/// implementation-defined). Used by `dim_seed` to fold the dim
/// name into the per-dim effective seed.
fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut hash: u32 = 0x811C_9DC5;
    for &b in bytes {
        hash ^= b as u32;
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

/// Derive a deterministic per-dimension seed from a base seed and
/// a dimension name. `base XOR FNV1a(dim)`.
///
/// The base seed comes from `world.json::seed`; this function
/// folds in the dim name so the overworld and the nether disagree
/// on chunk content even though they share the same world.json
/// seed. Two peers running the same worldgen with the same base
/// seed + same dim name produce byte-identical chunks.
pub fn dim_seed(base: u32, dim: &str) -> u32 {
    base ^ fnv1a32(dim.as_bytes())
}

/// Mix the supplied seed into a deterministic float used as a
/// phase offset for the sin/cos noise stack. Picking a small range
/// keeps the curves visually similar across seeds while still
/// producing distinct terrain.
///
/// F.2c-1: takes an explicit seed instead of reading the
/// `WORLD_SEED` atomic, so the noise pipeline is independent of
/// the global state. Callers pass `ctx.effective_seed()`.
pub(crate) fn seed_offset_for(seed: u32) -> f32 {
    (seed.wrapping_mul(0x9E37_79B1) as f32 / u32::MAX as f32) * 1000.0
}

/// Cheap, deterministic 2D hash. Used for tree placement so two
/// peers running the same generator drop trees in the same spots.
///
/// F.2c-2 plumbs the seed in as a parameter; callers pass
/// `ctx.effective_seed()` directly.
pub(crate) fn hash2_for(wx: i32, wz: i32, seed: u32) -> u32 {
    let mut h = (wx as u32).wrapping_mul(0x27D4_EB2F);
    h ^= (wz as u32).wrapping_mul(0x1656_67B1);
    h ^= seed;
    h ^= h >> 16;
    h = h.wrapping_mul(0x85EB_CA6B);
    h ^= h >> 13;
    h.wrapping_mul(0xC2B2_AE35)
}

// F.2c-3: legacy `hash2(wx, wz)` + `seed_offset()` (atomic
// readers) deleted. Every call site now reads
// `ctx.effective_seed()` and goes through `hash2_for` /
// `seed_offset_for`. The remaining `world_seed()` accessor is
// kept for UI-only consumers (HUD biome label, /biome chat
// command, the worldgen-picker dropdown) and the legacy
// `generate_chunk_at` bridge; nothing inside the gen call tree
// reaches for it.

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
    /// Phase F: hellish dimension. Netherrack bulk with bedrock
    /// floor, glowstone-flecked netherrack ceiling, scattered
    /// lava lakes + caves. Only meaningful when paired with the
    /// `nether` dim in a multi-dim save - the worldgen variant
    /// is per-dim, not a top-level world choice.
    Nether = 5,
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
            "nether" | "hell" => Some(Self::Nether),
            _ => None,
        }
    }
    pub fn current() -> Self {
        match WORLDGEN.load(std::sync::atomic::Ordering::Relaxed) {
            1 => Self::Flat,
            2 => Self::Islands,
            3 => Self::Amplified,
            4 => Self::Realistic,
            5 => Self::Nether,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_str_accepts_realistic() {
        assert_eq!(WorldGen::from_str("realistic"), Some(WorldGen::Realistic));
        assert_eq!(WorldGen::from_str("real"), Some(WorldGen::Realistic));
        assert_eq!(WorldGen::from_str("REALISTIC"), Some(WorldGen::Realistic));
    }

    #[test]
    fn from_str_accepts_legacy_profiles() {
        assert_eq!(WorldGen::from_str("classic"), Some(WorldGen::Classic));
        assert_eq!(WorldGen::from_str("default"), Some(WorldGen::Classic));
        assert_eq!(WorldGen::from_str("flat"), Some(WorldGen::Flat));
        assert_eq!(WorldGen::from_str("islands"), Some(WorldGen::Islands));
        assert_eq!(WorldGen::from_str("amplified"), Some(WorldGen::Amplified));
        assert_eq!(WorldGen::from_str("amp"), Some(WorldGen::Amplified));
    }

    #[test]
    fn from_str_rejects_unknown() {
        assert_eq!(WorldGen::from_str("xyzzy"), None);
        assert_eq!(WorldGen::from_str(""), None);
    }

    #[test]
    fn discriminants_match_atomic_layout() {
        // The atomic stores `as u8`; current() decodes that
        // mapping. Sanity-check the contract.
        assert_eq!(WorldGen::Classic as u8, 0);
        assert_eq!(WorldGen::Flat as u8, 1);
        assert_eq!(WorldGen::Islands as u8, 2);
        assert_eq!(WorldGen::Amplified as u8, 3);
        assert_eq!(WorldGen::Realistic as u8, 4);
        assert_eq!(WorldGen::Nether as u8, 5);
    }

    #[test]
    fn from_str_accepts_nether() {
        assert_eq!(WorldGen::from_str("nether"), Some(WorldGen::Nether));
        assert_eq!(WorldGen::from_str("hell"), Some(WorldGen::Nether));
        assert_eq!(WorldGen::from_str("NETHER"), Some(WorldGen::Nether));
    }

    #[test]
    fn dim_seed_is_deterministic() {
        // Same base + dim must always yield the same effective seed
        // (no time-based or RNG mixing).
        let a = dim_seed(0, "overworld");
        let b = dim_seed(0, "overworld");
        assert_eq!(a, b);
        let c = dim_seed(12345, "nether");
        let d = dim_seed(12345, "nether");
        assert_eq!(c, d);
    }

    #[test]
    fn dim_seed_distinguishes_dims() {
        // The whole point of the per-dim derivation is that two
        // dims with the same base seed produce different effective
        // seeds, so the overworld and nether don't pull from the
        // same noise lattice.
        assert_ne!(dim_seed(0, "overworld"), dim_seed(0, "nether"));
        assert_ne!(dim_seed(42, "overworld"), dim_seed(42, "nether"));
    }

    #[test]
    fn dim_seed_base_zero_still_distinguishes() {
        // Base seed 0 (host hasn't picked one yet) must still split
        // the two dims; the XOR with FNV(dim_name) does the work.
        let ow = dim_seed(0, "overworld");
        let nt = dim_seed(0, "nether");
        assert_ne!(ow, 0);
        assert_ne!(nt, 0);
        assert_ne!(ow, nt);
    }

    #[test]
    fn fnv1a32_known_vectors() {
        // Spot-check the FNV-1a 32-bit hash against a known string
        // so platform-specific surprises (signed/unsigned shifts)
        // would fail loudly. "" -> offset basis; "a" / "foo" /
        // "overworld" -> reference values.
        assert_eq!(fnv1a32(b""), 0x811C_9DC5);
        assert_eq!(fnv1a32(b"a"), 0xE40C_292C);
        assert_eq!(fnv1a32(b"foobar"), 0xBF9C_F968);
    }
}
