//! W47.13 - world-mutation helpers (place / break / target resolution),
//! relocated out of `main.rs` to keep the bin shell small. Each
//! submodule is pure / borrow-friendly free functions; the App
//! (`Voxel`) calls them with the explicit field references it needs.

// W65 - shared runtime block-edit funnel (set + remesh + broadcast).
pub mod edit;
// W64 - falling-block physics (sand / concrete_powder / gravel).
pub mod falling;
pub mod place_break;
// W65 - MC-style random block tick (grass spread, growth, decay).
pub mod random_tick;
// W65.4 - crop growth (immature -> mature stage mapping).
pub mod crop;
// W65.2 - runtime sapling -> tree growth (world-space edit builder).
pub mod tree;
// W47.18 - per-frame chunk streamer (queue, unload, dispatch, drain).
pub mod stream;
