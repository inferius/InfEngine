//! Static `BLOCK_TYPES` table - one row per block kind.
//!
//! Split out of `block/mod.rs` in Wave 12. Adding a block:
//!   1. Add a `BlockType` row here.
//!   2. Add a `*_ID` constant in `ids.rs`.
//!   3. (Optional) Add a special model in `special_models.rs`.
//!   4. (Optional) Add a light emission row in `light_emission.rs`.
//!
//! The table is consumed by `block_type(id)` in `mod.rs` + the
//! atlas loader + the chunk mesher.

use super::types::{
    BlockType, TileSpec, F_AABB_PASS, F_DEDICATED_RENDER, F_NO_PLACE, F_RAY_PASS,
    F_REPLACEABLE, F_SWIM, F_TRANSPARENT_RENDER, F_WASHABLE,
};

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
    // | F_TRANSPARENT_RENDER) plus F_WASHABLE so water spreading
    // INTO a fire cell extinguishes it (Q.4e) - the caller's
    // washed-tape handler clears `voxel.fires[pos]` without
    // spawning a drop.
    BlockType {
        id: 49, name: "fire", color: [1.00, 0.55, 0.10], transparent: true,
        flags: F_RAY_PASS | F_AABB_PASS | F_REPLACEABLE | F_TRANSPARENT_RENDER | F_DEDICATED_RENDER | F_WASHABLE,
        top: TileSpec { col: 0, row: 0, files: &["fire_layer_0.png", "torch_on.png"], keep_alpha: true },
        bottom: TileSpec { col: 0, row: 0, files: &["fire_layer_0.png", "torch_on.png"], keep_alpha: true },
        sides: TileSpec { col: 0, row: 0, files: &["fire_layer_0.png", "torch_on.png"], keep_alpha: true },
    },
    // ---------------------------------------------------------------
    // W11.3 - MC emitter + cosmetic blocks.
    //
    // Every entry here mirrors a `.lightLevel(...)` row from
    // `block::light_emission::MC_EMITTERS` so the new ids show up
    // as light sources in the BFS automatically. Texture paths
    // follow the W9 name_map pattern (the asset pack picks the
    // right tile when MC is installed; the swatch colour is the
    // fallback). Most carry the same flag set as TORCH so the
    // render pipeline + ray-cast + collision code don't need any
    // emitter-specific branches.

    // Redstone torch (id 50). MC emits 7 when lit. Same pillar
    // render shape as a regular torch; tinted red.
    BlockType {
        id: 50, name: "redstone_torch", color: [0.85, 0.15, 0.10], transparent: true,
        flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_DEDICATED_RENDER | F_WASHABLE,
        top: TileSpec { col: 3, row: 2, files: &["redstone_torch_on.png", "redstone_torch.png", "torch_on.png"], keep_alpha: true },
        bottom: TileSpec { col: 3, row: 2, files: &["redstone_torch_on.png", "redstone_torch.png", "torch_on.png"], keep_alpha: true },
        sides: TileSpec { col: 3, row: 2, files: &["redstone_torch_on.png", "redstone_torch.png", "torch_on.png"], keep_alpha: true },
    },
    // Redstone wall torch (id 51). Wall-mounted variant of
    // REDSTONE_TORCH. Same flags + emission.
    BlockType {
        id: 51, name: "redstone_wall_torch", color: [0.85, 0.15, 0.10], transparent: true,
        flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_DEDICATED_RENDER | F_WASHABLE,
        top: TileSpec { col: 3, row: 2, files: &["redstone_torch_on.png", "redstone_torch.png", "torch_on.png"], keep_alpha: true },
        bottom: TileSpec { col: 3, row: 2, files: &["redstone_torch_on.png", "redstone_torch.png", "torch_on.png"], keep_alpha: true },
        sides: TileSpec { col: 3, row: 2, files: &["redstone_torch_on.png", "redstone_torch.png", "torch_on.png"], keep_alpha: true },
    },
    // Soul torch (id 52). MC emits 10. Cooler-coloured variant of
    // TORCH crafted from a torch + soul soil/sand.
    BlockType {
        id: 52, name: "soul_torch", color: [0.40, 0.85, 1.00], transparent: true,
        flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_DEDICATED_RENDER | F_WASHABLE,
        top: TileSpec { col: 3, row: 2, files: &["soul_torch.png", "torch_on.png"], keep_alpha: true },
        bottom: TileSpec { col: 3, row: 2, files: &["soul_torch.png", "torch_on.png"], keep_alpha: true },
        sides: TileSpec { col: 3, row: 2, files: &["soul_torch.png", "torch_on.png"], keep_alpha: true },
    },
    // Soul wall torch (id 53). Wall variant of SOUL_TORCH.
    BlockType {
        id: 53, name: "soul_wall_torch", color: [0.40, 0.85, 1.00], transparent: true,
        flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_DEDICATED_RENDER | F_WASHABLE,
        top: TileSpec { col: 3, row: 2, files: &["soul_torch.png", "torch_on.png"], keep_alpha: true },
        bottom: TileSpec { col: 3, row: 2, files: &["soul_torch.png", "torch_on.png"], keep_alpha: true },
        sides: TileSpec { col: 3, row: 2, files: &["soul_torch.png", "torch_on.png"], keep_alpha: true },
    },
    // Sea lantern (id 54). MC emits 15. Underwater light source;
    // we ship it as a full cube without water-only restrictions
    // (placement works anywhere).
    BlockType {
        id: 54, name: "sea_lantern", color: [0.85, 0.95, 1.00], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["sea_lantern.png", "glowstone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["sea_lantern.png", "glowstone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["sea_lantern.png", "glowstone.png"], keep_alpha: false },
    },
    // Jack o'lantern (id 55). MC emits 15. Carved pumpkin with
    // a lit face; full-cube opaque block.
    BlockType {
        id: 55, name: "jack_o_lantern", color: [1.00, 0.78, 0.30], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["pumpkin_top.png", "jack_o_lantern.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["pumpkin_top.png", "jack_o_lantern.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["jack_o_lantern.png", "pumpkin_face_on.png"], keep_alpha: false },
    },
    // Soul fire (id 56). MC emits 10. Same flag set as FIRE but
    // with a cooler colour. No spread logic in W11.3 - that's a
    // future enhancement to fire.rs gated on the block id.
    BlockType {
        id: 56, name: "soul_fire", color: [0.40, 0.85, 1.00], transparent: true,
        flags: F_RAY_PASS | F_AABB_PASS | F_REPLACEABLE | F_TRANSPARENT_RENDER | F_DEDICATED_RENDER | F_WASHABLE,
        top: TileSpec { col: 0, row: 0, files: &["soul_fire_0.png", "fire_layer_0.png"], keep_alpha: true },
        bottom: TileSpec { col: 0, row: 0, files: &["soul_fire_0.png", "fire_layer_0.png"], keep_alpha: true },
        sides: TileSpec { col: 0, row: 0, files: &["soul_fire_0.png", "fire_layer_0.png"], keep_alpha: true },
    },
    // End rod (id 57). MC emits 14. Thin white-violet rod; render
    // shape promoted to F_DEDICATED_RENDER like torches so the
    // chunk mesher doesn't emit a full cube.
    BlockType {
        id: 57, name: "end_rod", color: [0.95, 0.90, 0.95], transparent: true,
        flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_DEDICATED_RENDER | F_WASHABLE,
        top: TileSpec { col: 0, row: 0, files: &["end_rod.png", "torch_on.png"], keep_alpha: true },
        bottom: TileSpec { col: 0, row: 0, files: &["end_rod.png", "torch_on.png"], keep_alpha: true },
        sides: TileSpec { col: 0, row: 0, files: &["end_rod.png", "torch_on.png"], keep_alpha: true },
    },
    // Magma block (id 58). MC emits 3. Solid orange-glow block.
    // No fire-damage step in W11.3.
    BlockType {
        id: 58, name: "magma_block", color: [0.85, 0.30, 0.15], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["magma.png", "netherrack.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["magma.png", "netherrack.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["magma.png", "netherrack.png"], keep_alpha: false },
    },
    // Shroomlight (id 59). MC emits 15. Bright orange fungal
    // light from huge fungi.
    BlockType {
        id: 59, name: "shroomlight", color: [1.00, 0.60, 0.20], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["shroomlight.png", "glowstone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["shroomlight.png", "glowstone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["shroomlight.png", "glowstone.png"], keep_alpha: false },
    },
    // Lantern (id 60). MC emits 15. Hanging metal lantern; full
    // cube here (no hangs-down rendering).
    BlockType {
        id: 60, name: "lantern", color: [0.55, 0.45, 0.30], transparent: true,
        flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_DEDICATED_RENDER | F_WASHABLE,
        top: TileSpec { col: 0, row: 0, files: &["lantern.png", "torch_on.png"], keep_alpha: true },
        bottom: TileSpec { col: 0, row: 0, files: &["lantern.png", "torch_on.png"], keep_alpha: true },
        sides: TileSpec { col: 0, row: 0, files: &["lantern.png", "torch_on.png"], keep_alpha: true },
    },
    // Soul lantern (id 61). MC emits 10.
    BlockType {
        id: 61, name: "soul_lantern", color: [0.35, 0.75, 0.95], transparent: true,
        flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_DEDICATED_RENDER | F_WASHABLE,
        top: TileSpec { col: 0, row: 0, files: &["soul_lantern.png", "torch_on.png"], keep_alpha: true },
        bottom: TileSpec { col: 0, row: 0, files: &["soul_lantern.png", "torch_on.png"], keep_alpha: true },
        sides: TileSpec { col: 0, row: 0, files: &["soul_lantern.png", "torch_on.png"], keep_alpha: true },
    },
    // Crying obsidian (id 62). MC emits 10. Decorative variant of
    // obsidian that drips purple light particles in MC; we ship
    // the lit cube, particles are a future enhancement.
    BlockType {
        id: 62, name: "crying_obsidian", color: [0.18, 0.05, 0.30], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["crying_obsidian.png", "obsidian.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["crying_obsidian.png", "obsidian.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["crying_obsidian.png", "obsidian.png"], keep_alpha: false },
    },
    // Respawn anchor (id 63). MC emits 0/3/7/11/15 by charge
    // state (0..4 glowstone charges). We ship the table value
    // (15 = fully charged) since the engine has no per-state
    // charge yet; when charge state lands the lookup will route
    // through a state-aware fn that returns 0 for uncharged.
    BlockType {
        id: 63, name: "respawn_anchor", color: [0.18, 0.05, 0.30], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["respawn_anchor_top_off.png", "obsidian.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["respawn_anchor_bottom.png", "obsidian.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["respawn_anchor_side0.png", "obsidian.png"], keep_alpha: false },
    },
    // Ender chest (id 64). MC emits 7. Cosmetic only in W11.3 -
    // no per-player Ender inventory yet. Looks like obsidian
    // with cyan accents in MC.
    BlockType {
        id: 64, name: "ender_chest", color: [0.18, 0.18, 0.28], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["ender_chest_top.png", "obsidian.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["ender_chest_bottom.png", "obsidian.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["ender_chest_side.png", "obsidian.png"], keep_alpha: false },
    },
    // Lit redstone lamp (id 65). MC emits 15 when lit. The unlit
    // form is a separate id - we only ship the lit variant
    // because the engine has no redstone state yet.
    BlockType {
        id: 65, name: "lit_redstone_lamp", color: [1.00, 0.85, 0.60], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["redstone_lamp_on.png", "glowstone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["redstone_lamp_on.png", "glowstone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["redstone_lamp_on.png", "glowstone.png"], keep_alpha: false },
    },
    // Lit redstone ore (id 66). MC emits 9 when activated.
    BlockType {
        id: 66, name: "lit_redstone_ore", color: [0.80, 0.30, 0.30], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["redstone_ore.png", "stone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["redstone_ore.png", "stone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["redstone_ore.png", "stone.png"], keep_alpha: false },
    },
    // Light block (id 67). MC's invisible debug-stick light.
    // MC emits 15. Render as F_TRANSPARENT_RENDER + F_RAY_PASS
    // + F_AABB_PASS so it's effectively invisible in-world; the
    // BFS still picks up the emission.
    BlockType {
        id: 67, name: "light_block", color: [1.00, 1.00, 1.00], transparent: true,
        flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_TRANSPARENT_RENDER | F_DEDICATED_RENDER,
        top: TileSpec { col: 0, row: 0, files: &["light_15.png", "glowstone.png"], keep_alpha: true },
        bottom: TileSpec { col: 0, row: 0, files: &["light_15.png", "glowstone.png"], keep_alpha: true },
        sides: TileSpec { col: 0, row: 0, files: &["light_15.png", "glowstone.png"], keep_alpha: true },
    },
    // ---------------------------------------------------------------
    // W12.2 - Slab + Fence + Door + Stairs + Wall.
    //
    // ids 68..=82. Each carries F_DEDICATED_RENDER so the chunk
    // mesher routes them through `special_models::model_for_block`
    // instead of emitting a default cube. Texture file lists fall
    // back to the matching full-cube texture so the BlockType row
    // still renders if the special model can't resolve a mesh.
    //
    // Per W12 design decisions: slabs ship one id per top/bottom
    // half (state-driven dual-mesh is a follow-up). Stairs default
    // east-facing; rotation states are a follow-up. Doors split
    // lower + upper ids; auto-pair logic is a follow-up.

    // Oak slab (bottom half). Cite: `models/block/oak_slab.json`
    // -> `template = block/slab` (lower-half cuboid).
    BlockType {
        id: 68, name: "oak_slab", color: [0.78, 0.60, 0.36], transparent: true,
        flags: F_DEDICATED_RENDER,
        top: TileSpec { col: 0, row: 2, files: &["planks_oak.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 2, files: &["planks_oak.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 2, files: &["planks_oak.png"], keep_alpha: false },
    },
    // Stone slab. Cite: `models/block/stone_slab.json`.
    BlockType {
        id: 69, name: "stone_slab", color: [0.50, 0.50, 0.55], transparent: true,
        flags: F_DEDICATED_RENDER,
        top: TileSpec { col: 0, row: 0, files: &["stone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["stone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["stone.png"], keep_alpha: false },
    },
    // Cobblestone slab. Cite: `models/block/cobblestone_slab.json`.
    BlockType {
        id: 70, name: "cobblestone_slab", color: [0.42, 0.42, 0.45], transparent: true,
        flags: F_DEDICATED_RENDER,
        top: TileSpec { col: 1, row: 2, files: &["cobblestone.png"], keep_alpha: false },
        bottom: TileSpec { col: 1, row: 2, files: &["cobblestone.png"], keep_alpha: false },
        sides: TileSpec { col: 1, row: 2, files: &["cobblestone.png"], keep_alpha: false },
    },
    // Oak slab top-half. Cite: `models/block/oak_slab_top.json`.
    BlockType {
        id: 71, name: "oak_slab_top", color: [0.78, 0.60, 0.36], transparent: true,
        flags: F_DEDICATED_RENDER,
        top: TileSpec { col: 0, row: 2, files: &["planks_oak.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 2, files: &["planks_oak.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 2, files: &["planks_oak.png"], keep_alpha: false },
    },
    // Oak stairs. Cite: `models/block/oak_stairs.json`. W12.2 ships
    // default east-facing only; rotation states are a follow-up.
    BlockType {
        id: 72, name: "oak_stairs", color: [0.78, 0.60, 0.36], transparent: true,
        flags: F_DEDICATED_RENDER,
        top: TileSpec { col: 0, row: 2, files: &["planks_oak.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 2, files: &["planks_oak.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 2, files: &["planks_oak.png"], keep_alpha: false },
    },
    // Stone stairs. Cite: `models/block/stone_stairs.json`.
    BlockType {
        id: 73, name: "stone_stairs", color: [0.50, 0.50, 0.55], transparent: true,
        flags: F_DEDICATED_RENDER,
        top: TileSpec { col: 0, row: 0, files: &["stone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["stone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["stone.png"], keep_alpha: false },
    },
    // Cobblestone stairs. Cite: `models/block/cobblestone_stairs.json`.
    BlockType {
        id: 74, name: "cobblestone_stairs", color: [0.42, 0.42, 0.45], transparent: true,
        flags: F_DEDICATED_RENDER,
        top: TileSpec { col: 1, row: 2, files: &["cobblestone.png"], keep_alpha: false },
        bottom: TileSpec { col: 1, row: 2, files: &["cobblestone.png"], keep_alpha: false },
        sides: TileSpec { col: 1, row: 2, files: &["cobblestone.png"], keep_alpha: false },
    },
    // Oak fence. Cite: `models/block/oak_fence_post.json` plus 4
    // connection arms (deferred to a follow-up that needs
    // neighbour-aware multipart blockstate dispatch).
    BlockType {
        id: 75, name: "oak_fence", color: [0.78, 0.60, 0.36], transparent: true,
        flags: F_RAY_PASS | F_AABB_PASS | F_DEDICATED_RENDER,
        top: TileSpec { col: 0, row: 2, files: &["planks_oak.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 2, files: &["planks_oak.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 2, files: &["planks_oak.png"], keep_alpha: false },
    },
    // Nether brick fence. Cite: `models/block/nether_brick_fence_post.json`.
    BlockType {
        id: 76, name: "nether_brick_fence", color: [0.35, 0.18, 0.20], transparent: true,
        flags: F_RAY_PASS | F_AABB_PASS | F_DEDICATED_RENDER,
        top: TileSpec { col: 0, row: 0, files: &["nether_brick.png", "netherrack.png", "stone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["nether_brick.png", "netherrack.png", "stone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["nether_brick.png", "netherrack.png", "stone.png"], keep_alpha: false },
    },
    // Oak door (lower half). Cite: `models/block/oak_door_bottom.json`
    // -> `template = block/door_bottom_left`.
    BlockType {
        id: 77, name: "oak_door", color: [0.78, 0.60, 0.36], transparent: true,
        flags: F_RAY_PASS | F_AABB_PASS | F_DEDICATED_RENDER,
        top: TileSpec { col: 0, row: 2, files: &["door_oak_lower.png", "planks_oak.png"], keep_alpha: true },
        bottom: TileSpec { col: 0, row: 2, files: &["door_oak_lower.png", "planks_oak.png"], keep_alpha: true },
        sides: TileSpec { col: 0, row: 2, files: &["door_oak_lower.png", "planks_oak.png"], keep_alpha: true },
    },
    // Oak door (upper half). Cite: `models/block/oak_door_top.json`.
    // Manual placement only in W12.2 - the chat /setblock places one
    // half at a time; auto-pair-on-place is a follow-up.
    BlockType {
        id: 78, name: "oak_door_upper", color: [0.78, 0.60, 0.36], transparent: true,
        flags: F_RAY_PASS | F_AABB_PASS | F_DEDICATED_RENDER,
        top: TileSpec { col: 0, row: 2, files: &["door_oak_upper.png", "planks_oak.png"], keep_alpha: true },
        bottom: TileSpec { col: 0, row: 2, files: &["door_oak_upper.png", "planks_oak.png"], keep_alpha: true },
        sides: TileSpec { col: 0, row: 2, files: &["door_oak_upper.png", "planks_oak.png"], keep_alpha: true },
    },
    // Iron door (lower half). Cite: `models/block/iron_door_bottom.json`.
    BlockType {
        id: 79, name: "iron_door", color: [0.80, 0.80, 0.85], transparent: true,
        flags: F_RAY_PASS | F_AABB_PASS | F_DEDICATED_RENDER,
        top: TileSpec { col: 0, row: 0, files: &["door_iron_lower.png", "iron_ore.png", "stone.png"], keep_alpha: true },
        bottom: TileSpec { col: 0, row: 0, files: &["door_iron_lower.png", "iron_ore.png", "stone.png"], keep_alpha: true },
        sides: TileSpec { col: 0, row: 0, files: &["door_iron_lower.png", "iron_ore.png", "stone.png"], keep_alpha: true },
    },
    // Glass pane. Cite: `models/block/glass_pane_post.json` - thin
    // 2x2 centre post + 4 connection arms.
    BlockType {
        id: 80, name: "glass_pane", color: [0.85, 0.92, 0.98], transparent: true,
        flags: F_RAY_PASS | F_AABB_PASS | F_DEDICATED_RENDER,
        top: TileSpec { col: 2, row: 2, files: &["glass.png"], keep_alpha: true },
        bottom: TileSpec { col: 2, row: 2, files: &["glass.png"], keep_alpha: true },
        sides: TileSpec { col: 2, row: 2, files: &["glass.png"], keep_alpha: true },
    },
    // Iron bars. Cite: `models/block/iron_bars_post.json`.
    BlockType {
        id: 81, name: "iron_bars", color: [0.55, 0.55, 0.60], transparent: true,
        flags: F_RAY_PASS | F_AABB_PASS | F_DEDICATED_RENDER,
        top: TileSpec { col: 0, row: 0, files: &["iron_bars.png", "iron_ore.png", "stone.png"], keep_alpha: true },
        bottom: TileSpec { col: 0, row: 0, files: &["iron_bars.png", "iron_ore.png", "stone.png"], keep_alpha: true },
        sides: TileSpec { col: 0, row: 0, files: &["iron_bars.png", "iron_ore.png", "stone.png"], keep_alpha: true },
    },
    // Cobblestone wall. Cite: `models/block/cobblestone_wall_post.json`.
    BlockType {
        id: 82, name: "cobblestone_wall", color: [0.42, 0.42, 0.45], transparent: true,
        flags: F_DEDICATED_RENDER,
        top: TileSpec { col: 1, row: 2, files: &["cobblestone.png"], keep_alpha: false },
        bottom: TileSpec { col: 1, row: 2, files: &["cobblestone.png"], keep_alpha: false },
        sides: TileSpec { col: 1, row: 2, files: &["cobblestone.png"], keep_alpha: false },
    },
    // ---------------------------------------------------------------
    // W35.3 - Stone variants + wood variants + wool colours +
    //         building blocks. IDs 83..=97.
    // Colors sampled from the average of the matching MC texture.
    // ---------------------------------------------------------------
    // Granite - warm pinkish igneous stone.
    BlockType {
        id: 83, name: "granite", color: [0.68, 0.48, 0.40], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["granite.png", "stone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["granite.png", "stone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["granite.png", "stone.png"], keep_alpha: false },
    },
    // Diorite - light grey flecked igneous stone.
    BlockType {
        id: 84, name: "diorite", color: [0.82, 0.82, 0.82], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["diorite.png", "stone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["diorite.png", "stone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["diorite.png", "stone.png"], keep_alpha: false },
    },
    // Andesite - dark grey igneous stone.
    BlockType {
        id: 85, name: "andesite", color: [0.58, 0.58, 0.58], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["andesite.png", "stone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["andesite.png", "stone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["andesite.png", "stone.png"], keep_alpha: false },
    },
    // Bricks - red clay bricks.
    BlockType {
        id: 86, name: "bricks", color: [0.68, 0.38, 0.30], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["bricks.png", "stone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["bricks.png", "stone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["bricks.png", "stone.png"], keep_alpha: false },
    },
    // Smooth stone - polished stone slab top texture on all faces.
    BlockType {
        id: 87, name: "smooth_stone", color: [0.55, 0.55, 0.58], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["smooth_stone.png", "stone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["smooth_stone.png", "stone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["smooth_stone.png", "stone.png"], keep_alpha: false },
    },
    // Birch log - pale white-black bark.
    BlockType {
        id: 88, name: "birch_log", color: [0.88, 0.86, 0.72], transparent: false, flags: 0,
        top: TileSpec { col: 3, row: 1, files: &["birch_log_top.png", "log_oak_top.png"], keep_alpha: false },
        bottom: TileSpec { col: 3, row: 1, files: &["birch_log_top.png", "log_oak_top.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 1, files: &["birch_log.png", "log_oak.png"], keep_alpha: false },
    },
    // Birch planks - cream coloured planks.
    BlockType {
        id: 89, name: "birch_planks", color: [0.90, 0.84, 0.60], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 2, files: &["birch_planks.png", "planks_oak.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 2, files: &["birch_planks.png", "planks_oak.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 2, files: &["birch_planks.png", "planks_oak.png"], keep_alpha: false },
    },
    // Birch leaves - bright yellow-green.
    BlockType {
        id: 90, name: "birch_leaves", color: [0.70, 0.85, 0.32], transparent: true, flags: 0,
        top: TileSpec { col: 1, row: 1, files: &["birch_leaves.png", "leaves_oak.tga"], keep_alpha: true },
        bottom: TileSpec { col: 1, row: 1, files: &["birch_leaves.png", "leaves_oak.tga"], keep_alpha: true },
        sides: TileSpec { col: 1, row: 1, files: &["birch_leaves.png", "leaves_oak.tga"], keep_alpha: true },
    },
    // Blue wool.
    BlockType {
        id: 91, name: "blue_wool", color: [0.22, 0.32, 0.72], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["blue_wool.png", "white_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["blue_wool.png", "white_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["blue_wool.png", "white_wool.png"], keep_alpha: false },
    },
    // Red wool.
    BlockType {
        id: 92, name: "red_wool", color: [0.74, 0.18, 0.18], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["red_wool.png", "white_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["red_wool.png", "white_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["red_wool.png", "white_wool.png"], keep_alpha: false },
    },
    // Yellow wool.
    BlockType {
        id: 93, name: "yellow_wool", color: [0.92, 0.82, 0.16], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["yellow_wool.png", "white_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["yellow_wool.png", "white_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["yellow_wool.png", "white_wool.png"], keep_alpha: false },
    },
    // Green wool.
    BlockType {
        id: 94, name: "green_wool", color: [0.28, 0.52, 0.20], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["green_wool.png", "white_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["green_wool.png", "white_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["green_wool.png", "white_wool.png"], keep_alpha: false },
    },
    // Tuff - dark mottled volcanic rock. Added in 1.17.
    BlockType {
        id: 95, name: "tuff", color: [0.46, 0.46, 0.42], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["tuff.png", "stone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["tuff.png", "stone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["tuff.png", "stone.png"], keep_alpha: false },
    },
    // Deepslate - very dark grey, heavier than stone. Added in 1.17.
    BlockType {
        id: 96, name: "deepslate", color: [0.30, 0.30, 0.32], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["deepslate_top.png", "stone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["deepslate_top.png", "stone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["deepslate.png", "stone.png"], keep_alpha: false },
    },
    // Calcite - bright white mineral, part of the dripstone cave biome.
    BlockType {
        id: 97, name: "calcite", color: [0.92, 0.92, 0.90], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["calcite.png", "stone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["calcite.png", "stone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["calcite.png", "stone.png"], keep_alpha: false },
    },
];
