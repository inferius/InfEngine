//! Block registry, flags, and per-block lookup helpers.
//!
//! Owns:
//! - `BlockType` + the static `BLOCK_TYPES` table (every kind of
//!   voxel the demo can place).
//! - `F_*` flag constants that gate behaviour (raycast,
//!   collision, washability, dedicated render path, ...).
//! - Block-id constants (`TORCH_ID`, `WATER_ID`, ...).
//! - Tool tier + mining-effectiveness helpers - they read the
//!   block id table so they live here too.
//! - `TileSpec` + atlas UV resolver - the atlas layout is part
//!   of the block definition.
//!
//! Layout mirrors MC's `net/minecraft/world/level/block` package
//! at a coarse level. Each block kind there has its own class;
//! ours collapse into one `BlockType` row + behaviour flags.
//! As blocks gain richer state (containers, multi-block, ...)
//! we'll promote some rows to dedicated sub-modules under
//! `block/`.

use infengine_voxel::BlockId;

pub(crate) mod chest;
pub(crate) mod fire;
pub(crate) mod furnace;
pub(crate) mod portal;
#[cfg(test)]
mod fire_tests;
#[cfg(test)]
mod phase_f_tests;

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
// Block definitions.
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

pub(crate) const BLOCK_TYPES: &[BlockType] = &[
    BlockType {
        id: 1, name: "stone", color: [0.50, 0.50, 0.55], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["stone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["stone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["stone.png"], keep_alpha: false },
    },
    BlockType {
        id: 2, name: "dirt", color: [0.45, 0.32, 0.20], transparent: false, flags: 0,
        top: TileSpec { col: 1, row: 0, files: &["dirt.png"], keep_alpha: false },
        bottom: TileSpec { col: 1, row: 0, files: &["dirt.png"], keep_alpha: false },
        sides: TileSpec { col: 1, row: 0, files: &["dirt.png"], keep_alpha: false },
    },
    BlockType {
        id: 3, name: "grass", color: [0.35, 0.62, 0.28], transparent: false, flags: 0,
        top: TileSpec {
            col: 2, row: 0,
            files: &["grass_carried.png", "grass_top.png"],
            keep_alpha: false,
        },
        bottom: TileSpec { col: 1, row: 0, files: &["dirt.png"], keep_alpha: false },
        sides: TileSpec {
            col: 2, row: 1,
            files: &["grass_side_carried.png", "grass_side.tga"],
            keep_alpha: false,
        },
    },
    BlockType {
        id: 4, name: "sand", color: [0.92, 0.86, 0.62], transparent: false, flags: 0,
        top: TileSpec { col: 3, row: 0, files: &["sand.png"], keep_alpha: false },
        bottom: TileSpec { col: 3, row: 0, files: &["sand.png"], keep_alpha: false },
        sides: TileSpec { col: 3, row: 0, files: &["sand.png"], keep_alpha: false },
    },
    BlockType {
        id: 5, name: "wood", color: [0.55, 0.38, 0.22], transparent: false, flags: 0,
        top: TileSpec { col: 3, row: 1, files: &["log_oak_top.png"], keep_alpha: false },
        bottom: TileSpec { col: 3, row: 1, files: &["log_oak_top.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 1, files: &["log_oak.png"], keep_alpha: false },
    },
    BlockType {
        id: 6, name: "leaves", color: [0.24, 0.50, 0.20], transparent: true, flags: 0,
        top: TileSpec {
            col: 1, row: 1,
            files: &["leaves_oak.tga", "leaves_oak_carried.png", "leaves_oak_opaque.png"],
            keep_alpha: true,
        },
        bottom: TileSpec {
            col: 1, row: 1,
            files: &["leaves_oak.tga", "leaves_oak_carried.png", "leaves_oak_opaque.png"],
            keep_alpha: true,
        },
        sides: TileSpec {
            col: 1, row: 1,
            files: &["leaves_oak.tga", "leaves_oak_carried.png", "leaves_oak_opaque.png"],
            keep_alpha: true,
        },
    },
    BlockType {
        id: 7, name: "planks", color: [0.78, 0.60, 0.36], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 2, files: &["planks_oak.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 2, files: &["planks_oak.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 2, files: &["planks_oak.png"], keep_alpha: false },
    },
    BlockType {
        id: 8, name: "cobble", color: [0.42, 0.42, 0.45], transparent: false, flags: 0,
        top: TileSpec { col: 1, row: 2, files: &["cobblestone.png"], keep_alpha: false },
        bottom: TileSpec { col: 1, row: 2, files: &["cobblestone.png"], keep_alpha: false },
        sides: TileSpec { col: 1, row: 2, files: &["cobblestone.png"], keep_alpha: false },
    },
    BlockType {
        id: 9, name: "glass", color: [0.85, 0.92, 0.98], transparent: true, flags: 0,
        top: TileSpec { col: 2, row: 2, files: &["glass.png"], keep_alpha: true },
        bottom: TileSpec { col: 2, row: 2, files: &["glass.png"], keep_alpha: true },
        sides: TileSpec { col: 2, row: 2, files: &["glass.png"], keep_alpha: true },
    },
    BlockType {
        id: 10, name: "torch", color: [1.00, 0.78, 0.40], transparent: true,
        flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_DEDICATED_RENDER | F_WASHABLE,
        top: TileSpec { col: 3, row: 2, files: &["torch_on.png", "torch.png"], keep_alpha: true },
        bottom: TileSpec { col: 3, row: 2, files: &["torch_on.png", "torch.png"], keep_alpha: true },
        sides: TileSpec { col: 3, row: 2, files: &["torch_on.png", "torch.png"], keep_alpha: true },
    },
    BlockType {
        id: 11, name: "wool", color: [0.95, 0.95, 0.95], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 3, files: &["wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 3, files: &["wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 3, files: &["wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 12, name: "meat", color: [0.85, 0.45, 0.40], transparent: false, flags: 0,
        top: TileSpec { col: 1, row: 3, files: &["meat.png"], keep_alpha: false },
        bottom: TileSpec { col: 1, row: 3, files: &["meat.png"], keep_alpha: false },
        sides: TileSpec { col: 1, row: 3, files: &["meat.png"], keep_alpha: false },
    },
    BlockType {
        id: 13, name: "coal", color: [0.30, 0.30, 0.30], transparent: false, flags: 0,
        top: TileSpec { col: 2, row: 3, files: &["coal_ore.png"], keep_alpha: false },
        bottom: TileSpec { col: 2, row: 3, files: &["coal_ore.png"], keep_alpha: false },
        sides: TileSpec { col: 2, row: 3, files: &["coal_ore.png"], keep_alpha: false },
    },
    BlockType {
        id: 14, name: "iron", color: [0.75, 0.65, 0.55], transparent: false, flags: 0,
        top: TileSpec { col: 3, row: 3, files: &["iron_ore.png"], keep_alpha: false },
        bottom: TileSpec { col: 3, row: 3, files: &["iron_ore.png"], keep_alpha: false },
        sides: TileSpec { col: 3, row: 3, files: &["iron_ore.png"], keep_alpha: false },
    },
    BlockType {
        // Snow cap. Caps mountain biome heights above the snow
        // line in gen_classic; otherwise behaves like a stone-y
        // surface block. Falls back to a near-white solid colour
        // when no snow.png ships with the assets pack.
        // Snow renders as a thin layer; the chunk mesher skips
        // it and the world pass draws a custom slab using these
        // tile assignments so the texture still gets sourced
        // from the atlas / asset pack.
        id: 15, name: "snow", color: [0.97, 0.98, 1.00], transparent: true,
        flags: F_DEDICATED_RENDER | F_REPLACEABLE | F_WASHABLE,
        top: TileSpec { col: 0, row: 0, files: &["snow.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["snow.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["snow.png"], keep_alpha: false },
    },
    // Pickaxe tools. Stored as block-id slots but `is_tool_block`
    // rejects them from the place path. Atlas tiles fall back to
    // tinted swatches when no `pickaxe_*.png` ships in the pack.
    BlockType {
        id: 16, name: "pickaxe_wood", color: [0.62, 0.42, 0.20], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["pickaxe_wood.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["pickaxe_wood.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["pickaxe_wood.png"], keep_alpha: false },
    },
    BlockType {
        id: 17, name: "pickaxe_stone", color: [0.55, 0.55, 0.55], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["pickaxe_stone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["pickaxe_stone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["pickaxe_stone.png"], keep_alpha: false },
    },
    BlockType {
        id: 18, name: "pickaxe_iron", color: [0.80, 0.80, 0.85], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["pickaxe_iron.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["pickaxe_iron.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["pickaxe_iron.png"], keep_alpha: false },
    },
    // Chicken feathers drop. Cosmetic item used for the
    // chicken-themed recipe slot - not placeable as a world
    // block, so the place path treats it like the pickaxes.
    BlockType {
        id: 19, name: "feather", color: [0.95, 0.95, 0.95], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["feather.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["feather.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["feather.png"], keep_alpha: false },
    },
    // Decorative flower placed sparsely on plains-biome grass.
    // Transparent + alpha-cutout via the standard atlas path; the
    // procedural generator only spawns it on top of grass voxels.
    BlockType {
        id: 20, name: "flower", color: [0.95, 0.30, 0.30], transparent: true,
        flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE,
        top: TileSpec { col: 0, row: 0, files: &["flower.png"], keep_alpha: true },
        bottom: TileSpec { col: 0, row: 0, files: &["flower.png"], keep_alpha: true },
        sides: TileSpec { col: 0, row: 0, files: &["flower.png"], keep_alpha: true },
    },
    BlockType {
        id: 21, name: "flower_yellow", color: [1.00, 0.85, 0.20], transparent: true,
        flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE,
        top: TileSpec { col: 0, row: 0, files: &["flower_yellow.png"], keep_alpha: true },
        bottom: TileSpec { col: 0, row: 0, files: &["flower_yellow.png"], keep_alpha: true },
        sides: TileSpec { col: 0, row: 0, files: &["flower_yellow.png"], keep_alpha: true },
    },
    BlockType {
        id: 22, name: "flower_blue", color: [0.40, 0.55, 0.95], transparent: true,
        flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE,
        top: TileSpec { col: 0, row: 0, files: &["flower_blue.png"], keep_alpha: true },
        bottom: TileSpec { col: 0, row: 0, files: &["flower_blue.png"], keep_alpha: true },
        sides: TileSpec { col: 0, row: 0, files: &["flower_blue.png"], keep_alpha: true },
    },
    // Sword tools. Like pickaxes they live in inventory slots
    // but reject placement; selected slot's `weapon_tier` boosts
    // the player's attack damage when fighting mobs / peers.
    BlockType {
        id: 23, name: "sword_wood", color: [0.62, 0.42, 0.20], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["sword_wood.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["sword_wood.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["sword_wood.png"], keep_alpha: false },
    },
    BlockType {
        id: 24, name: "sword_stone", color: [0.55, 0.55, 0.55], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["sword_stone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["sword_stone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["sword_stone.png"], keep_alpha: false },
    },
    BlockType {
        id: 25, name: "sword_iron", color: [0.80, 0.80, 0.85], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["sword_iron.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["sword_iron.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["sword_iron.png"], keep_alpha: false },
    },
    // Sandstone block. Generates below the sand surface in
    // desert biomes so digging through dunes hits a usable
    // building material instead of bare stone.
    BlockType {
        id: 26, name: "sandstone", color: [0.92, 0.85, 0.62], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["sandstone_top.png", "sandstone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["sandstone_bottom.png", "sandstone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["sandstone.png"], keep_alpha: false },
    },
    // Cactus block. Spawns occasionally in desert biomes. Hurts
    // the player on contact via the world-side touch check.
    BlockType {
        id: 27, name: "cactus", color: [0.30, 0.55, 0.25], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["cactus_top.png", "cactus.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["cactus_bottom.png", "cactus.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["cactus_side.png", "cactus.png"], keep_alpha: false },
    },
    // Glowstone-style light block. Acts as a torch-tier point
    // light: tracked in torch_positions so the shader picks it
    // up alongside placed torches.
    BlockType {
        id: 28, name: "glowstone", color: [1.00, 0.92, 0.60], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["glowstone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["glowstone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["glowstone.png"], keep_alpha: false },
    },
    // Shovel + axe tools. Different tool kinds speed up different
    // block categories: shovels chew dirt / sand / gravel, axes
    // chew wood / planks / leaves. Like pickaxes they are tool-
    // slot items rejected by the place path.
    BlockType {
        id: 29, name: "shovel_wood", color: [0.62, 0.42, 0.20], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["shovel_wood.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["shovel_wood.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["shovel_wood.png"], keep_alpha: false },
    },
    BlockType {
        id: 30, name: "shovel_stone", color: [0.55, 0.55, 0.55], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["shovel_stone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["shovel_stone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["shovel_stone.png"], keep_alpha: false },
    },
    BlockType {
        id: 31, name: "shovel_iron", color: [0.80, 0.80, 0.85], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["shovel_iron.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["shovel_iron.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["shovel_iron.png"], keep_alpha: false },
    },
    BlockType {
        id: 32, name: "axe_wood", color: [0.62, 0.42, 0.20], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["axe_wood.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["axe_wood.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["axe_wood.png"], keep_alpha: false },
    },
    BlockType {
        id: 33, name: "axe_stone", color: [0.55, 0.55, 0.55], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["axe_stone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["axe_stone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["axe_stone.png"], keep_alpha: false },
    },
    BlockType {
        id: 34, name: "axe_iron", color: [0.80, 0.80, 0.85], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["axe_iron.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["axe_iron.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["axe_iron.png"], keep_alpha: false },
    },
    // Bed block. Placeable + decorative. Has no special wakeup
    // behaviour yet - /sleep handles the time skip - but the
    // distinct texture marks safe-rest spots for builders.
    BlockType {
        id: 35, name: "bed", color: [0.85, 0.30, 0.30], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["bed_top.png", "bed.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["bed_bottom.png", "bed.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["bed_side.png", "bed.png"], keep_alpha: false },
    },
    // Water source block. Source-only for now (no flow physics
    // yet); player walks through, swims while submerged. Marked
    // transparent so the chunk mesher emits faces against air
    // neighbours and culls water-water seams. Breaks instantly
    // with a bare hand.
    BlockType {
        id: 36, name: "water", color: [0.20, 0.40, 0.85], transparent: true,
        flags: F_AABB_PASS | F_TRANSPARENT_RENDER | F_SWIM | F_DEDICATED_RENDER | F_REPLACEABLE | F_RAY_PASS,
        top: TileSpec { col: 0, row: 0, files: &["water.png", "stone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["water.png", "stone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["water.png", "stone.png"], keep_alpha: false },
    },
    // Lava: second liquid using the same generic flow physics
    // as water. Slower spread + shorter range follow MC's
    // overworld lava behaviour. Swim mechanics share the
    // generic `F_SWIM` gate today; lava damage lives in a
    // future commit alongside other liquid effects.
    BlockType {
        id: 37, name: "lava", color: [1.00, 0.40, 0.10], transparent: true,
        flags: F_AABB_PASS | F_TRANSPARENT_RENDER | F_SWIM | F_DEDICATED_RENDER | F_REPLACEABLE | F_RAY_PASS,
        top: TileSpec { col: 0, row: 0, files: &["lava.png", "stone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["lava.png", "stone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["lava.png", "stone.png"], keep_alpha: false },
    },
    // Bucket item ids 38..=40. Tool-class slots (no block place);
    // RMB on a liquid scoops, RMB on a placement target empties
    // the bucket into a source block.
    BlockType {
        id: 38, name: "bucket", color: [0.80, 0.80, 0.85], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["bucket_empty.png", "iron_ingot.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["bucket_empty.png", "iron_ingot.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["bucket_empty.png", "iron_ingot.png"], keep_alpha: false },
    },
    BlockType {
        id: 39, name: "water_bucket", color: [0.20, 0.45, 0.85], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["bucket_water.png", "iron_ingot.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["bucket_water.png", "iron_ingot.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["bucket_water.png", "iron_ingot.png"], keep_alpha: false },
    },
    BlockType {
        id: 40, name: "lava_bucket", color: [1.00, 0.45, 0.10], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["bucket_lava.png", "iron_ingot.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["bucket_lava.png", "iron_ingot.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["bucket_lava.png", "iron_ingot.png"], keep_alpha: false },
    },
    // Chest. Opaque cube with per-position 27-slot container state
    // (lives in `Voxel::chests`). RMB opens the chest UI; breaking
    // the chest drops its contents as item entities. Atlas falls
    // back to a brown swatch when no chest.png ships with the
    // resource pack - the (0,0) placeholder makes the texture pick
    // up the wood-planks tile + we tint via `color` so the chest
    // still reads as a distinct block on the world surface.
    BlockType {
        id: 41, name: "chest", color: [0.62, 0.42, 0.20], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["chest_top.png", "chest.png", "planks_oak.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["chest_bottom.png", "chest.png", "planks_oak.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["chest_side.png", "chest.png", "planks_oak.png"], keep_alpha: false },
    },
    // Furnace. Opaque cube with a 3-slot Container (input / fuel /
    // output) stored on `Voxel::furnaces` keyed by absolute world
    // voxel coords. RMB opens the furnace UI; breaking the furnace
    // drops all 3 slots as item entities. The cook step runs in
    // real time via `block::furnace::furnace_tick` and via the
    // offline catch-up dispatcher in `world_save::timers`.
    //
    // Texture: top + front + sides fall back through MC-style file
    // names ("furnace_top", "furnace_front", "furnace_side") then
    // to the stone tile so the block remains visible on resource
    // packs without dedicated furnace art. The (0,0) atlas slot
    // doubles as the placeholder; the `color` tint nudges it grey.
    // Per-face orientation is cosmetic and deferred - C.4 ships a
    // single side texture across all four lateral faces.
    BlockType {
        id: 42, name: "furnace", color: [0.55, 0.55, 0.58], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["furnace_top.png", "furnace_front.png", "stone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["furnace_top.png", "furnace_front.png", "stone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["furnace_front.png", "furnace_side.png", "stone.png"], keep_alpha: false },
    },
    // Netherrack. Bulk material of the nether dimension. Opaque
    // reddish rock. Mineable with any pickaxe (tier 1+). Atlas
    // falls back to the stone tile if no netherrack art ships.
    BlockType {
        id: 44, name: "netherrack", color: [0.55, 0.18, 0.18], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["netherrack.png", "stone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["netherrack.png", "stone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["netherrack.png", "stone.png"], keep_alpha: false },
    },
    // Bedrock. Indestructible floor block. F_NO_PLACE keeps
    // builders from accidentally stacking it on top of a frame;
    // tool-tier-4 + the mining hardness path treat it as
    // unbreakable in practice (no tool tier reaches 4 yet).
    BlockType {
        id: 45, name: "bedrock", color: [0.25, 0.25, 0.27], transparent: false,
        flags: F_NO_PLACE,
        top: TileSpec { col: 0, row: 0, files: &["bedrock.png", "stone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["bedrock.png", "stone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["bedrock.png", "stone.png"], keep_alpha: false },
    },
    // Obsidian. Drops only with a diamond (tier-3-equivalent)
    // pickaxe. The chosen tier is `block_required_tier(46) = 3`,
    // matching the iron pickaxe today since diamond tools don't
    // exist yet. Same purple-black look as MC; renders opaque.
    BlockType {
        id: 46, name: "obsidian", color: [0.10, 0.04, 0.16], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["obsidian.png", "stone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["obsidian.png", "stone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["obsidian.png", "stone.png"], keep_alpha: false },
    },
    // Portal block. Cosmetic-only render today (F_DEDICATED_RENDER
    // tinted purple); pass-through for ray + AABB so the player
    // walks into it to trigger the cross-dim teleport.
    BlockType {
        id: 47, name: "portal", color: [0.55, 0.20, 0.85], transparent: true,
        flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_TRANSPARENT_RENDER | F_DEDICATED_RENDER,
        top: TileSpec { col: 0, row: 0, files: &["portal.png", "water.png"], keep_alpha: true },
        bottom: TileSpec { col: 0, row: 0, files: &["portal.png", "water.png"], keep_alpha: true },
        sides: TileSpec { col: 0, row: 0, files: &["portal.png", "water.png"], keep_alpha: true },
    },
    // Flint and steel. Single-stack tool that ignites a valid
    // obsidian portal frame on RMB. Not placeable; the right-
    // click path consumes nothing (durability is a deferred
    // follow-up - see F.4 module docs).
    BlockType {
        id: 48, name: "flint_and_steel", color: [0.78, 0.68, 0.40], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["flint_and_steel.png", "iron_ingot.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["flint_and_steel.png", "iron_ingot.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["flint_and_steel.png", "iron_ingot.png"], keep_alpha: false },
    },
    // Fire block (Q.4b). Visual-only decoration that the player
    // walks through; carries a per-cell `age: u8` (0..=15) stored
    // on `Voxel::fires`. Spreads to flammable neighbours + dies
    // when its support disappears. Same flag set as the portal
    // (F_RAY_PASS | F_AABB_PASS | F_REPLACEABLE | F_DEDICATED_RENDER
    // | F_TRANSPARENT_RENDER) - chunk mesher skips it; the
    // dedicated render path is a follow-up (placeholder while
    // Wave 2 ships).
    BlockType {
        id: 49, name: "fire", color: [1.00, 0.55, 0.10], transparent: true,
        flags: F_RAY_PASS | F_AABB_PASS | F_REPLACEABLE | F_TRANSPARENT_RENDER | F_DEDICATED_RENDER,
        top: TileSpec { col: 0, row: 0, files: &["fire_layer_0.png", "torch_on.png"], keep_alpha: true },
        bottom: TileSpec { col: 0, row: 0, files: &["fire_layer_0.png", "torch_on.png"], keep_alpha: true },
        sides: TileSpec { col: 0, row: 0, files: &["fire_layer_0.png", "torch_on.png"], keep_alpha: true },
    },
];

/// Block id of the torch (light source). Kept as a const so the
/// renderer-side light scan doesn't have to do a string lookup.
pub(crate) const TORCH_ID: u16 = 10;
pub(crate) const GLOWSTONE_ID: u16 = 28;
/// Block id of water. Passable, instant-break, applies swim drag
/// + buoyancy while the player AABB intersects it.
pub(crate) const WATER_ID: u16 = 36;
/// Block id of lava. Same liquid pipeline as water, different
/// `Liquid` params (shorter range, slower tick, no infinite
/// source).
pub(crate) const LAVA_ID: u16 = 37;

/// Bucket item ids. The empty bucket is the crafted base; the
/// filled variants come from RMB-scooping the matching liquid.
pub(crate) const BUCKET_EMPTY_ID: u16 = 38;
pub(crate) const BUCKET_WATER_ID: u16 = 39;
pub(crate) const BUCKET_LAVA_ID: u16 = 40;
/// Chest block id. Per-position container state (27 slots) lives
/// on `Voxel::chests`; RMB opens the chest UI, break drops the
/// contents as item entities.
#[allow(dead_code)] // wired up in C.3b+
pub(crate) const CHEST_ID: u16 = 41;
/// Furnace block id. Per-position 3-slot container (input / fuel /
/// output) lives on `Voxel::furnaces`. RMB opens the furnace UI;
/// break drops the 3 slots as item entities. The live cook step
/// runs in `block::furnace::furnace_tick`; offline catch-up uses
/// `Timer::Furnace` from the save layer.
pub(crate) const FURNACE_ID: u16 = 42;
/// Coal item id. Re-exported here so the smelting fuel table can
/// reference a single source of truth instead of a bare literal.
pub(crate) const COAL_ID: u16 = 13;
/// Iron-ore block id (raw form, smelts into IRON_INGOT_ID).
pub(crate) const IRON_ORE_ID: u16 = 14;
/// Wood / log id. Doubles as a fuel for the furnace.
pub(crate) const WOOD_ID: u16 = 5;
/// Planks item id. Doubles as a fuel (mirrors MC's planks burn).
pub(crate) const PLANKS_ID: u16 = 7;
/// Cobblestone block id. Drops from `stone` mining + serves as
/// the lava-flow-meets-water conversion product (MC's
/// `LiquidBlock.shouldSpreadLiquid` flow branch).
pub(crate) const COBBLE_ID: u16 = 8;
/// Stone block id. Re-exported alongside `COBBLE_ID` so the
/// lava-boundary path can read either constant without bare
/// literals; also future-proof for a separate "lava flow over
/// water on top" -> stone conversion if we mirror MC's basalt
/// case for our palette.
#[allow(dead_code)] // wired up in a future Q.4 follow-up
pub(crate) const STONE_ID: u16 = 1;
/// Leaves block id. Flammable (catches fire fast per MC).
#[allow(dead_code)] // wired up in Q.4c flammability table
pub(crate) const LEAVES_ID: u16 = 6;
/// Wool block id. Flammable.
#[allow(dead_code)] // wired up in Q.4c flammability table
pub(crate) const WOOL_ID: u16 = 11;
/// Iron ingot item id. Output of smelting iron ore. Pure item -
/// has no block form, so the place path treats it like the
/// pickaxes (not placeable).
pub(crate) const IRON_INGOT_ID: u16 = 43;
/// Netherrack: bulk material of the nether dimension. Phase F.
pub(crate) const NETHERRACK_ID: u16 = 44;
/// Bedrock: indestructible floor block of the nether. Phase F.
pub(crate) const BEDROCK_ID: u16 = 45;
/// Obsidian: portal frame material. Diamond-tier mining (3+).
/// Phase F.
pub(crate) const OBSIDIAN_ID: u16 = 46;
/// Portal block: cosmetic + cross-dim trigger. Phase F.
#[allow(dead_code)] // wired up in F.5+
pub(crate) const PORTAL_ID: u16 = 47;
/// Flint and steel: ignites a valid obsidian portal frame on
/// RMB. Phase F. Durability is a deferred follow-up.
pub(crate) const FLINT_AND_STEEL_ID: u16 = 48;
/// Fire block id (Q.4b). Per-cell age (0..=15) lives in the
/// parallel `Voxel::fires` HashMap (same pattern as chests +
/// furnaces). Spreads to flammable neighbours, dies on
/// extinguish / support failure.
#[allow(dead_code)] // wired up by Q.4c fire tick + Q.4d/e ignite
pub(crate) const FIRE_ID: u16 = 49;

// ---------------------------------------------------------------
// Block lookup helpers.
// ---------------------------------------------------------------

pub(crate) fn block_type(id: u16) -> Option<&'static BlockType> {
    BLOCK_TYPES.iter().find(|b| b.id == id)
}

/// Bit-test helper: do all bits in `mask` light up for `block`?
/// Air is always treated as having zero flags so the caller doesn't
/// have to guard against it. Unknown ids likewise return `false`.
#[inline]
pub(crate) fn block_has_flag(block: BlockId, mask: u8) -> bool {
    if block.is_air() {
        return false;
    }
    block_type(block.0)
        .map(|t| (t.flags & mask) == mask)
        .unwrap_or(false)
}

/// Raycast traverses through this voxel (decorations: torch,
/// flowers). Pairs with the `is_blocked` predicate handed to
/// `raycast_with`.
#[inline]
pub(crate) fn block_is_ray_passable(b: BlockId) -> bool {
    block_has_flag(b, F_RAY_PASS)
}

/// Player AABB skips this voxel for collision. Decorations + the
/// water column.
#[inline]
pub(crate) fn block_is_aabb_passable(b: BlockId) -> bool {
    block_has_flag(b, F_AABB_PASS)
}

/// `RMB place` refuses to overwrite this voxel - the player has
/// to break it first.
#[inline]
pub(crate) fn block_is_placement_blocked(b: BlockId) -> bool {
    block_has_flag(b, F_NO_PLACE)
}

/// Voxel routes through the alpha-blended mesh pipeline + skips
/// the shadow pass.
#[inline]
pub(crate) fn block_is_transparent_render(b: BlockId) -> bool {
    block_has_flag(b, F_TRANSPARENT_RENDER)
}

/// Voxel is a liquid the player can swim in.
#[inline]
pub(crate) fn block_is_swim_medium(b: BlockId) -> bool {
    block_has_flag(b, F_SWIM)
}

/// Block has its own per-voxel render path (torch pillar, snow
/// slab, water slab) so the chunk mesher should skip emitting
/// full-cube geometry for it.
#[inline]
pub(crate) fn block_skips_chunk_mesh(b: BlockId) -> bool {
    block_has_flag(b, F_DEDICATED_RENDER)
}

/// Block can be displaced by RMB-placement (the placed block
/// lands AT the voxel, the old block is overwritten). Liquid
/// cells + the snow layer opt in. Air is implicitly replaceable
/// via the normal "place at adjacent air cell" path.
#[inline]
pub(crate) fn block_is_replaceable(b: BlockId) -> bool {
    block_has_flag(b, F_REPLACEABLE)
}

/// Block gets washed away when a liquid spreads or falls into
/// its cell. Decorations opt in; the liquid simply overwrites
/// them (drops handled by the item-entity system).
#[inline]
pub(crate) fn block_is_washable(b: BlockId) -> bool {
    block_has_flag(b, F_WASHABLE)
}

/// True when the block is one of the registered liquids
/// (regardless of which one). Used as a fast gate in the
/// generic tick logic that has to filter water + lava out of
/// otherwise-solid checks. Equivalent to "registered in the
/// `LIQUIDS` table" since every liquid carries `F_SWIM`.
#[inline]
pub(crate) fn block_is_liquid(b: BlockId) -> bool {
    block_has_flag(b, F_SWIM)
}

pub(crate) fn block_is_opaque(b: BlockId) -> bool {
    block_type(b.0).map(|t| !t.transparent).unwrap_or(true)
}

/// Map a block id to the Minecraft sound-material name. Used to
/// pick the right `dig:*` / `step:*` variant from `sound_assets`.
/// Unknown blocks default to `"stone"` because that's the least
/// surprising thud / step click.
pub(crate) fn block_material(b: BlockId) -> &'static str {
    match b.0 {
        1 | 8 | 13 | 14 => "stone", // stone, cobble, coal ore, iron ore
        2 => "grass",               // dirt - grass-ish sound is fine
        3 => "grass",
        4 => "sand",
        5 | 7 | 10 => "wood",       // wood, planks, torch
        6 => "grass",               // leaves
        9 => "glass",
        36 => "sand", // water - splashy-ish, no dedicated bank yet
        _ => "stone",
    }
}

pub(crate) fn atlas_uv_rect(block: BlockId, normal: [i8; 3]) -> [f32; 4] {
    let Some(bt) = block_type(block.0) else {
        return [0.0, 0.0, 1.0, 1.0];
    };
    let tile = if normal[1] > 0 {
        &bt.top
    } else if normal[1] < 0 {
        &bt.bottom
    } else {
        &bt.sides
    };
    let u_step = 1.0 / ATLAS_COLS as f32;
    let v_step = 1.0 / ATLAS_ROWS as f32;
    let u0 = tile.col as f32 * u_step;
    let v0 = tile.row as f32 * v_step;
    [u0, v0, u0 + u_step, v0 + v_step]
}

// ---------------------------------------------------------------
// Bucket helpers.
// ---------------------------------------------------------------

/// Map a bucket item to the liquid it holds, if any. Used by the
/// RMB place path to spawn a source block when emptying the
/// bucket onto a target voxel. Returns `None` for the empty
/// bucket or for non-bucket items.
pub(crate) fn bucket_liquid_id(b: BlockId) -> Option<u16> {
    match b.0 {
        BUCKET_WATER_ID => Some(WATER_ID),
        BUCKET_LAVA_ID => Some(LAVA_ID),
        _ => None,
    }
}

/// Map a liquid id to the filled-bucket variant produced when the
/// player scoops it with an empty bucket. Returns `None` when
/// the liquid has no bucket variant defined.
pub(crate) fn liquid_to_bucket_id(liquid: u16) -> Option<u16> {
    match liquid {
        WATER_ID => Some(BUCKET_WATER_ID),
        LAVA_ID => Some(BUCKET_LAVA_ID),
        _ => None,
    }
}

// ---------------------------------------------------------------
// Tool tiers + mining effectiveness.
// ---------------------------------------------------------------

/// Tool tier for a slot's contents. 0 = bare hand, 1 = wood
/// pickaxe, 2 = stone pickaxe, 3 = iron pickaxe. Determines what
/// the player can break + how fast.
pub(crate) fn tool_tier(block: BlockId) -> u8 {
    match block.0 {
        16 => 1, // wood pickaxe
        17 => 2, // stone pickaxe
        18 => 3, // iron pickaxe
        _ => 0,
    }
}

/// Minimum tool tier required to drop the block on break. A
/// player can always swing at a block (no hard gate), but the
/// drop only credits if their selected slot meets this tier.
/// Coal lets a bare-hand swing succeed; stone needs wood+; iron
/// needs stone+. Obsidian needs an iron pickaxe (tier 3) as a
/// stand-in for MC's diamond requirement; bedrock requires
/// tier-4 which no shipped tool reaches, making it unbreakable.
pub(crate) fn block_required_tier(block: BlockId) -> u8 {
    match block.0 {
        1 | 8 => 1, // stone, cobble
        13 => 1,    // coal ore
        14 => 2,    // iron ore
        NETHERRACK_ID => 1, // any pickaxe drops netherrack
        OBSIDIAN_ID => 3,   // diamond-tier (iron is our top tier today)
        BEDROCK_ID => 4,    // unbreakable in practice
        _ => 0,
    }
}

/// True if the block id represents a tool / non-placeable item.
pub(crate) fn is_tool_block(block: BlockId) -> bool {
    matches!(
        block.0,
        16..=19 | 23..=25 | 29..=34 | 38..=40 | FLINT_AND_STEEL_ID
    )
}

/// Human label for a tool slot, used by the HUD readout.
pub(crate) fn tool_label(block: BlockId) -> Option<&'static str> {
    match block.0 {
        16 => Some("Wood pickaxe"),
        17 => Some("Stone pickaxe"),
        18 => Some("Iron pickaxe"),
        23 => Some("Wood sword"),
        24 => Some("Stone sword"),
        25 => Some("Iron sword"),
        29 => Some("Wood shovel"),
        30 => Some("Stone shovel"),
        31 => Some("Iron shovel"),
        32 => Some("Wood axe"),
        33 => Some("Stone axe"),
        34 => Some("Iron axe"),
        38 => Some("Bucket"),
        39 => Some("Water bucket"),
        40 => Some("Lava bucket"),
        FLINT_AND_STEEL_ID => Some("Flint and Steel"),
        _ => None,
    }
}

/// Damage multiplier per sword tier. Bare hand = 1.0, wood
/// = 1.5, stone = 2.0, iron = 2.7. Selected slot drives the
/// scale applied to the local PvP / mob hits.
pub(crate) fn weapon_damage_mult(block: BlockId) -> f32 {
    match block.0 {
        23 => 1.5,
        24 => 2.0,
        25 => 2.7,
        _ => 1.0,
    }
}

/// Tier of a shovel slot, 0 = none.
pub(crate) fn shovel_tier(block: BlockId) -> u8 {
    match block.0 {
        29 => 1,
        30 => 2,
        31 => 3,
        _ => 0,
    }
}

/// Tier of an axe slot, 0 = none.
pub(crate) fn axe_tier(block: BlockId) -> u8 {
    match block.0 {
        32 => 1,
        33 => 2,
        34 => 3,
        _ => 0,
    }
}

/// Mining-speed category for a target block.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum BlockCategory {
    Stone,
    Soil,
    Wood,
    Other,
}

pub(crate) fn block_category(block: BlockId) -> BlockCategory {
    match block.0 {
        1 | 8 | 13 | 14 | 26 => BlockCategory::Stone,
        2..=4 | 15 => BlockCategory::Soil, // dirt/grass/sand/snow
        5..=7 => BlockCategory::Wood,
        NETHERRACK_ID | OBSIDIAN_ID | BEDROCK_ID => BlockCategory::Stone,
        _ => BlockCategory::Other,
    }
}

/// Returns a (mining_mult, drop_ok) pair for the held tool vs
/// the block being mined. Lower mult = faster. drop_ok = true
/// when the player gets the broken block back.
pub(crate) fn tool_effectiveness(
    held: BlockId,
    block: BlockId,
) -> (f32, bool) {
    // Liquids break instantly regardless of tool. Skips the
    // category-mismatch 4x penalty so bare-hand scoop is fast
    // for every registered liquid (water, lava, ...).
    if block_is_liquid(block) {
        return (0.25, true);
    }
    let cat = block_category(block);
    let needed = block_required_tier(block);
    let pick = tool_tier(held);
    let shov = shovel_tier(held);
    let axe = axe_tier(held);
    let (matched_tier, matches) = match cat {
        BlockCategory::Stone => (pick, pick > 0),
        BlockCategory::Soil => (shov, shov > 0),
        BlockCategory::Wood => (axe, axe > 0),
        BlockCategory::Other => (0, false),
    };
    let drop_ok = pick >= needed;
    let mult = if matches {
        // Right tool category. Each extra tier shaves ~20% off
        // the time; floor at 0.4 so iron stays distinctly
        // faster than wood.
        let base = 0.55;
        let extra = (matched_tier as i32
            - block_required_tier(block) as i32)
            .max(0) as f32;
        (base - extra * 0.20).max(0.4)
    } else if pick >= needed {
        // Sufficient pickaxe tier but wrong category (e.g.
        // pickaxe on dirt). Normal speed.
        1.0
    } else {
        // Wrong tool entirely; punishing slow.
        4.0
    };
    (mult, drop_ok)
}
