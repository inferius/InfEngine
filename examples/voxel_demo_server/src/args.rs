//! Command-line argument parsing for the headless server.
//!
//! `clap` derive does the heavy lifting; this file is just the
//! struct definition + a small adapter (`WorldGenArg`) that maps
//! the user-facing enum into the `voxel_demo_mp::worldgen::WorldGen`
//! variant the runtime actually consumes.

use clap::{Parser, ValueEnum};

use voxel_demo_mp::worldgen::WorldGen;

/// User-facing worldgen variants. Mirrors
/// `voxel_demo_mp::worldgen::WorldGen` so `--worldgen amplified`
/// reads naturally on the command line. Translated to the runtime
/// enum via `into_runtime`.
#[derive(Clone, Copy, Debug, ValueEnum, PartialEq, Eq)]
pub enum WorldGenArg {
    Classic,
    Flat,
    Islands,
    Amplified,
}

impl WorldGenArg {
    /// Map to the runtime `WorldGen` enum the worldgen module reads
    /// from its atomic global. The two enums stay separate so a
    /// future runtime variant doesn't auto-leak onto the CLI surface
    /// without a deliberate `WorldGenArg` addition.
    #[allow(dead_code)]
    pub fn into_runtime(self) -> WorldGen {
        match self {
            Self::Classic => WorldGen::Classic,
            Self::Flat => WorldGen::Flat,
            Self::Islands => WorldGen::Islands,
            Self::Amplified => WorldGen::Amplified,
        }
    }

    /// Lowercase tag used to populate `world.json::worldgen`.
    /// Pairs with `WorldGen::from_str` on the load path.
    pub fn tag(self) -> &'static str {
        match self {
            Self::Classic => "classic",
            Self::Flat => "flat",
            Self::Islands => "islands",
            Self::Amplified => "amplified",
        }
    }
}

/// Command-line surface for the headless server. `clap` derives the
/// parser; every field has a sensible default so the simplest
/// "boot a fresh world" invocation is `voxel_demo_server --world foo`.
#[derive(Parser, Debug, Clone)]
#[command(
    name = "voxel_demo_server",
    about = "Headless dedicated server for the voxel demo.",
    version
)]
pub struct Args {
    /// UDP port to bind for client connections.
    #[arg(long, default_value_t = 7777)]
    pub port: u16,

    /// World slug to host. Creates if it doesn't exist; loads
    /// otherwise. Sluggified case-insensitively via
    /// `voxel_demo_mp::world_save::layout::slug` so two players
    /// don't end up with platform-divergent slot names.
    #[arg(long)]
    pub world: String,

    /// Worldgen variant when creating a new world. Ignored if the
    /// slug already has a `world.json` (the existing variant wins -
    /// editing worldgen mid-save would invalidate every chunk).
    #[arg(long, default_value = "classic", value_enum)]
    pub worldgen: WorldGenArg,

    /// Seed when creating a new world. Generated from the current
    /// wall-clock if omitted; ignored if the slug already exists.
    #[arg(long)]
    pub seed: Option<u32>,

    /// Bind address. Pair with `--port`; e.g. `--bind 127.0.0.1`
    /// for loopback-only.
    #[arg(long, default_value = "0.0.0.0")]
    pub bind: String,

    /// Autosave cadence in wall-clock seconds. The save fires from
    /// the main tick loop, not a background thread - large worlds
    /// will block ticks briefly during the write window.
    #[arg(long, default_value_t = 60)]
    pub autosave_secs: u32,

    /// Override the on-disk worlds root. Tests use this to point
    /// the server at a `tempfile::TempDir` so the integration
    /// suite doesn't touch the user's real save tree. Production
    /// callers leave it unset and the runtime resolves the
    /// per-OS `directories::ProjectDirs` data dir.
    #[arg(long, hide = true)]
    pub data_dir: Option<std::path::PathBuf>,
}
