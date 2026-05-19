//! Block table value types + behaviour flag bitmask constants.
//!
//! Split out of `block/mod.rs` in Wave 12 to keep the registry
//! module under the 500 LOC soft cap. The static `BLOCK_TYPES`
//! table lives in `registry.rs`; the per-id constants live in
//! `ids.rs`; everything in this module is the schema those two
//! modules consume.

// ---------------------------------------------------------------
// Texture atlas - tile descriptor + dimensions.
// ---------------------------------------------------------------

/// One tile inside the 4x4 atlas. `files` is the ordered fallback
/// list of source-file names the loader tries; first match wins.
/// `keep_alpha=true` means the source alpha channel survives into
/// the atlas (required for cutout textures like leaves);
/// `false` forces the atlas pixel alpha to 255 so anti-aliased
/// edges in the source don't accidentally discard solid pixels
/// via the global alpha-cutout shader path.
pub(crate) struct TileSpec {
    pub(crate) col: u32,
    pub(crate) row: u32,
    pub(crate) files: &'static [&'static str],
    pub(crate) keep_alpha: bool,
}

pub(crate) const ATLAS_COLS: u32 = 4;
pub(crate) const ATLAS_ROWS: u32 = 4;
pub(crate) const TILE_PX: u32 = 16;

// ---------------------------------------------------------------
// Behaviour flag bitmask.
// ---------------------------------------------------------------

/// Bitmask flags carried on every `BlockType`. Each is a behaviour
/// gate the engine checks instead of hard-coding block ids in the
/// caller. Adding a new flag = one constant + one accessor; adding
/// a new block keeps its flags additive on the table row.
pub(crate) const F_RAY_PASS: u8 = 1 << 0; // raycast traverses through (decorations)
pub(crate) const F_AABB_PASS: u8 = 1 << 1; // player AABB doesn't collide
pub(crate) const F_NO_PLACE: u8 = 1 << 2; // RMB-place refuses to overwrite this voxel
pub(crate) const F_TRANSPARENT_RENDER: u8 = 1 << 3; // alpha-blend pipeline, no shadow
pub(crate) const F_SWIM: u8 = 1 << 4; // liquid: swim drag + buoyancy
/// Block renders via a dedicated per-voxel mesh path (torches as
/// pillars, snow / water as scaled slabs) so the chunk mesher
/// skips it entirely. Saves the cost of generating face geometry
/// only to discard it at registration time.
pub(crate) const F_DEDICATED_RENDER: u8 = 1 << 5;
/// Block can be replaced in-place by RMB-placement. Liquids
/// plus thin overlays (snow layer, future tall grass) opt in so
/// the player can drown a water cell with a stone block or drop
/// a torch into snow without breaking the layer first.
pub(crate) const F_REPLACEABLE: u8 = 1 << 6;
/// Block gets washed away when a liquid spreads or falls into
/// its cell. Decorations (torch, flowers, snow layer, future
/// tall grass) opt in - water sweeping past replaces them with
/// itself. Drops handled separately by the item-entity system.
pub(crate) const F_WASHABLE: u8 = 1 << 7;

// ---------------------------------------------------------------
// BlockType row.
// ---------------------------------------------------------------

/// One block kind's full definition. Adding a new block = one
/// entry here + a hotbar slot below; the registry, atlas loader,
/// UV lookup and opacity test all read from this table so there's
/// no second source of truth to keep in sync.
pub(crate) struct BlockType {
    pub(crate) id: u16,
    pub(crate) name: &'static str,
    pub(crate) color: [f32; 3],
    pub(crate) transparent: bool,
    /// Behaviour bitmask. See `F_*` constants above. `0` = solid
    /// "normal" block. Set bits opt the block into ray pass-through,
    /// passable collision, no-place semantics, transparent render
    /// or swim mechanics.
    pub(crate) flags: u8,
    pub(crate) top: TileSpec,
    pub(crate) bottom: TileSpec,
    pub(crate) sides: TileSpec,
}
