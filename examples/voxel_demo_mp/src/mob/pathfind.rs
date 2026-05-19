//! A* pathfinding on the voxel grid.
//!
//! H.1 lays the module skeleton; H.4 fills in the real A* implementation
//! along with cost-function tuning + the "fall back to straight walk"
//! escape hatch.
//!
//! MC mirror: `net/minecraft/world/level/pathfinder/PathFinder.java`
//! - A* on a 1-block grid keyed by `BlockPathTypes` cost weights. We
//!   simplify to a fixed cost table per direction (orth = 1.0, diag =
//!   1.4, step-up = +0.5, fall = +0.3 per block, fall > 3 = +inf).
