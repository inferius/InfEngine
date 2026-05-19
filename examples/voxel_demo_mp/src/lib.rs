//! Headless-safe subset of the voxel_demo_mp crate, exposed as a
//! library so a sibling bin (`voxel_demo_server`) can reuse the
//! save layer + worldgen + block table without dragging in the
//! renderer / window / audio dependencies that the GUI demo bin
//! (`src/main.rs`) leans on.
//!
//! This module tree is INTENTIONALLY a small surface. It re-mounts
//! a curated set of source files via `#[path]` so the bin and lib
//! crates compile from the same source-of-truth files - no
//! duplicated copies, no diverging types.
//!
//! Excluded by design (depend on `Voxel` or renderer types):
//! - The full `world_save::{save_world,load_world,runtime}` glue
//!   (sits on the GUI's `Voxel` struct). Headless callers reach
//!   for the typed leaf modules below directly.
//! - `chat::*`, `mob_ai`, `drops`, `inventory`, `player`,
//!   `mining`, `decorations`, `ui`, `net` (those references rely
//!   on `crate::Voxel` or its constants).
//!
//! What's exposed:
//! - `block`: pure block-table data (atlas indices, categories,
//!   tool tier lookups). Renderer-free.
//! - `worldgen`: deterministic per-chunk terrain generator + the
//!   `WorldGen` variant enum. Headless callers call
//!   `generate_chunk_at` directly to mint chunks for newly-streamed
//!   coords.
//! - `world_save`: the typed save / load primitives the headless
//!   server uses. `chunks`, `meta`, `world_json`, `layout`,
//!   `atomic`, `migrate`, `players`, `entities`, `timers` are all
//!   here; the Voxel-coupled `runtime` orchestrator stays in the
//!   bin tree.
//!
//! The bin (`src/main.rs`) keeps its own `mod block;` /
//! `mod world_save;` declarations - both crates point at the same
//! files. Visibility on the items reachable through the lib uses
//! `pub` directly; `pub(crate)` on the same items inside the bin
//! crate still works because each crate independently re-mounts
//! the source.

#![allow(dead_code)]
// The bin crate consumes the full set of exported items; the lib
// crate sees a subset (no Voxel-coupled callers), so some items
// inside the headless modules look unused from the lib's vantage.
// Silence those warnings at the lib root instead of polluting each
// source file with `#[allow]` attributes.
#![allow(unused_imports)]

// Headless-safe modules: re-mounted from the bin's source tree via
// `#[path]`. Each one already passes the "no `Voxel` references"
// audit (see the file-level docs at the top of each source file).

#[path = "block/mod.rs"]
pub mod block;

#[path = "item/mod.rs"]
pub mod item;

#[path = "container/mod.rs"]
pub mod container;

#[path = "smelting.rs"]
pub mod smelting;

#[path = "worldgen/mod.rs"]
pub mod worldgen;

// Pre-split baseline regression: locked-in chunk-fold hashes for
// every shipped WorldGen profile under seed=0. Compiles into the
// lib's test binary only.
#[cfg(test)]
#[path = "worldgen_baseline_test.rs"]
mod worldgen_baseline_test;

// F.2a regression marker: pure-ctx worldgen stability across global
// atomic writes. Ignored until F.2b adds the `WorldgenCtx` struct +
// `generate_chunk_at_ctx` entry point; un-ignored once the refactor
// makes it pass.
#[cfg(test)]
#[path = "worldgen_ctx_test.rs"]
mod worldgen_ctx_test;

#[path = "liquid/mod.rs"]
pub mod liquid;

// The lib_world_save.rs file wraps the leaf modules under
// world_save::* so the `#[path]` inside resolves against the same
// directory as the wrapper file (i.e. `src/`).
#[path = "lib_world_save.rs"]
pub mod world_save;
