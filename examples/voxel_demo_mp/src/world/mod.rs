//! W47.13 - world-mutation helpers (place / break / target resolution),
//! relocated out of `main.rs` to keep the bin shell small. Each
//! submodule is pure / borrow-friendly free functions; the App
//! (`Voxel`) calls them with the explicit field references it needs.

pub mod place_break;
