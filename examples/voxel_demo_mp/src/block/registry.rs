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
        top: TileSpec { col: 0, row: 2, files: &["oak_planks.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 2, files: &["oak_planks.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 2, files: &["oak_planks.png"], keep_alpha: false },
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
        // W46.15: F_DEDICATED_RENDER dropped - chunk mesher now emits
        // the JSON template_torch.json geometry via emit_block_elements.
        // The legacy custom render path was removed in W46.13.
        // W46.16: color forced to white so the per-block tint in
        // queue_chunk_draw doesn't darken/recolour the already-baked
        // torch.png pixels (shader does albedo = color.rgb * tex.rgb).
        id: 10, name: "torch", color: [1.00, 1.00, 1.00], transparent: true,
        flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE,
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
        id: 41, name: "chest", color: [0.62, 0.42, 0.20], transparent: false,
        // W55.2: F_DEDICATED_RENDER so the special-model path
        // (build_chest) drives geometry; chunk mesher skips cube
        // emission. Falls back to planks_oak texture for both
        // pack-missing and entity-textures-not-atlased cases.
        flags: F_DEDICATED_RENDER,
        top: TileSpec { col: 0, row: 0, files: &["oak_planks.png", "oak_planks.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["oak_planks.png", "oak_planks.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["oak_planks.png", "oak_planks.png"], keep_alpha: false },
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
        // W159: sides were furnace_FRONT (the opening) on all four
        // faces - every furnace looked like 4 stacked oven doors.
        // Until per-facing front render lands (a chest-style W160),
        // use the plain furnace_side everywhere so it reads as a
        // stone box; top stays distinct.
        id: 42, name: "furnace", color: [0.55, 0.55, 0.58], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["furnace_top.png", "stone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["furnace_top.png", "stone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["furnace_side.png", "stone.png"], keep_alpha: false },
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
        id: 50, name: "redstone_torch", color: [1.00, 1.00, 1.00], transparent: true, // W46.16
        flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, // W46.15
        top: TileSpec { col: 3, row: 2, files: &["redstone_torch_on.png", "redstone_torch.png", "torch_on.png"], keep_alpha: true },
        bottom: TileSpec { col: 3, row: 2, files: &["redstone_torch_on.png", "redstone_torch.png", "torch_on.png"], keep_alpha: true },
        sides: TileSpec { col: 3, row: 2, files: &["redstone_torch_on.png", "redstone_torch.png", "torch_on.png"], keep_alpha: true },
    },
    // Redstone wall torch (id 51). Wall-mounted variant of
    // REDSTONE_TORCH. Same flags + emission.
    BlockType {
        id: 51, name: "redstone_wall_torch", color: [1.00, 1.00, 1.00], transparent: true, // W46.16
        flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, // W46.15
        top: TileSpec { col: 3, row: 2, files: &["redstone_torch_on.png", "redstone_torch.png", "torch_on.png"], keep_alpha: true },
        bottom: TileSpec { col: 3, row: 2, files: &["redstone_torch_on.png", "redstone_torch.png", "torch_on.png"], keep_alpha: true },
        sides: TileSpec { col: 3, row: 2, files: &["redstone_torch_on.png", "redstone_torch.png", "torch_on.png"], keep_alpha: true },
    },
    // Soul torch (id 52). MC emits 10. Cooler-coloured variant of
    // TORCH crafted from a torch + soul soil/sand.
    BlockType {
        id: 52, name: "soul_torch", color: [1.00, 1.00, 1.00], transparent: true, // W46.16
        flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, // W46.15
        top: TileSpec { col: 3, row: 2, files: &["soul_torch.png", "torch_on.png"], keep_alpha: true },
        bottom: TileSpec { col: 3, row: 2, files: &["soul_torch.png", "torch_on.png"], keep_alpha: true },
        sides: TileSpec { col: 3, row: 2, files: &["soul_torch.png", "torch_on.png"], keep_alpha: true },
    },
    // Soul wall torch (id 53). Wall variant of SOUL_TORCH.
    BlockType {
        id: 53, name: "soul_wall_torch", color: [1.00, 1.00, 1.00], transparent: true, // W46.16
        flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, // W46.15
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
        top: TileSpec { col: 0, row: 2, files: &["oak_planks.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 2, files: &["oak_planks.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 2, files: &["oak_planks.png"], keep_alpha: false },
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
        top: TileSpec { col: 0, row: 2, files: &["oak_planks.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 2, files: &["oak_planks.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 2, files: &["oak_planks.png"], keep_alpha: false },
    },
    // Oak stairs. Cite: `models/block/oak_stairs.json`. W12.2 ships
    // default east-facing only; rotation states are a follow-up.
    BlockType {
        id: 72, name: "oak_stairs", color: [0.78, 0.60, 0.36], transparent: true,
        flags: F_DEDICATED_RENDER,
        top: TileSpec { col: 0, row: 2, files: &["oak_planks.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 2, files: &["oak_planks.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 2, files: &["oak_planks.png"], keep_alpha: false },
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
        // W131: fences are SOLID + targetable in MC (no walking
        // through, breakable); the multipart blockstate grows arms.
        flags: F_DEDICATED_RENDER,
        top: TileSpec { col: 0, row: 2, files: &["oak_planks.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 2, files: &["oak_planks.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 2, files: &["oak_planks.png"], keep_alpha: false },
    },
    // Nether brick fence. Cite: `models/block/nether_brick_fence_post.json`.
    BlockType {
        id: 76, name: "nether_brick_fence", color: [0.35, 0.18, 0.20], transparent: true,
        flags: F_DEDICATED_RENDER,
        top: TileSpec { col: 0, row: 0, files: &["nether_brick.png", "netherrack.png", "stone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["nether_brick.png", "netherrack.png", "stone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["nether_brick.png", "netherrack.png", "stone.png"], keep_alpha: false },
    },
    // Oak door (lower half). Cite: `models/block/oak_door_bottom.json`
    // -> `template = block/door_bottom_left`.
    BlockType {
        id: 77, name: "oak_door", color: [0.78, 0.60, 0.36], transparent: true,
        flags: F_DEDICATED_RENDER,
        top: TileSpec { col: 0, row: 2, files: &["door_oak_lower.png", "oak_planks.png"], keep_alpha: true },
        bottom: TileSpec { col: 0, row: 2, files: &["door_oak_lower.png", "oak_planks.png"], keep_alpha: true },
        sides: TileSpec { col: 0, row: 2, files: &["door_oak_lower.png", "oak_planks.png"], keep_alpha: true },
    },
    // Oak door (upper half). Cite: `models/block/oak_door_top.json`.
    // Manual placement only in W12.2 - the chat /setblock places one
    // half at a time; auto-pair-on-place is a follow-up.
    BlockType {
        id: 78, name: "oak_door_upper", color: [0.78, 0.60, 0.36], transparent: true,
        flags: F_DEDICATED_RENDER,
        top: TileSpec { col: 0, row: 2, files: &["door_oak_upper.png", "oak_planks.png"], keep_alpha: true },
        bottom: TileSpec { col: 0, row: 2, files: &["door_oak_upper.png", "oak_planks.png"], keep_alpha: true },
        sides: TileSpec { col: 0, row: 2, files: &["door_oak_upper.png", "oak_planks.png"], keep_alpha: true },
    },
    // Iron door (lower half). Cite: `models/block/iron_door_bottom.json`.
    BlockType {
        id: 79, name: "iron_door", color: [0.80, 0.80, 0.85], transparent: true,
        flags: F_DEDICATED_RENDER,
        top: TileSpec { col: 0, row: 0, files: &["door_iron_lower.png", "iron_ore.png", "stone.png"], keep_alpha: true },
        bottom: TileSpec { col: 0, row: 0, files: &["door_iron_lower.png", "iron_ore.png", "stone.png"], keep_alpha: true },
        sides: TileSpec { col: 0, row: 0, files: &["door_iron_lower.png", "iron_ore.png", "stone.png"], keep_alpha: true },
    },
    // Glass pane. Cite: `models/block/glass_pane_post.json` - thin
    // 2x2 centre post + 4 connection arms.
    BlockType {
        id: 80, name: "glass_pane", color: [0.85, 0.92, 0.98], transparent: true,
        flags: F_DEDICATED_RENDER,
        top: TileSpec { col: 2, row: 2, files: &["glass.png"], keep_alpha: true },
        bottom: TileSpec { col: 2, row: 2, files: &["glass.png"], keep_alpha: true },
        sides: TileSpec { col: 2, row: 2, files: &["glass.png"], keep_alpha: true },
    },
    // Iron bars. Cite: `models/block/iron_bars_post.json`.
    BlockType {
        id: 81, name: "iron_bars", color: [0.55, 0.55, 0.60], transparent: true,
        flags: F_DEDICATED_RENDER,
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
        top: TileSpec { col: 0, row: 2, files: &["birch_planks.png", "oak_planks.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 2, files: &["birch_planks.png", "oak_planks.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 2, files: &["birch_planks.png", "oak_planks.png"], keep_alpha: false },
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
    // ---------------------------------------------------------------
    // W37.2 - Bulk MC item/block registry import.
    //
    // IDs 98..=173. All entries follow the same atlas-fallback
    // pattern as W35.3: primary file is the canonical MC texture
    // name; fallback is a representative vanilla stone/planks tile
    // so the block always renders even without a resource pack.
    //
    // Wool colors (ids 98..=108). White wool (id 11) + blue/red/
    // yellow/green (ids 91..=94) already existed; we add the
    // remaining 11 dye variants here.
    // ---------------------------------------------------------------
    BlockType {
        id: 98, name: "orange_wool", color: [0.88, 0.52, 0.15], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["orange_wool.png", "white_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["orange_wool.png", "white_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["orange_wool.png", "white_wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 99, name: "magenta_wool", color: [0.78, 0.28, 0.72], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["magenta_wool.png", "white_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["magenta_wool.png", "white_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["magenta_wool.png", "white_wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 100, name: "light_blue_wool", color: [0.42, 0.68, 0.90], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["light_blue_wool.png", "white_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["light_blue_wool.png", "white_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["light_blue_wool.png", "white_wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 101, name: "lime_wool", color: [0.52, 0.82, 0.22], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["lime_wool.png", "white_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["lime_wool.png", "white_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["lime_wool.png", "white_wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 102, name: "pink_wool", color: [0.92, 0.58, 0.65], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["pink_wool.png", "white_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["pink_wool.png", "white_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["pink_wool.png", "white_wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 103, name: "gray_wool", color: [0.40, 0.40, 0.40], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["gray_wool.png", "white_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["gray_wool.png", "white_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["gray_wool.png", "white_wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 104, name: "light_gray_wool", color: [0.68, 0.68, 0.68], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["light_gray_wool.png", "white_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["light_gray_wool.png", "white_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["light_gray_wool.png", "white_wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 105, name: "cyan_wool", color: [0.18, 0.52, 0.58], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["cyan_wool.png", "white_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["cyan_wool.png", "white_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["cyan_wool.png", "white_wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 106, name: "purple_wool", color: [0.50, 0.18, 0.68], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["purple_wool.png", "white_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["purple_wool.png", "white_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["purple_wool.png", "white_wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 107, name: "brown_wool", color: [0.52, 0.32, 0.18], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["brown_wool.png", "white_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["brown_wool.png", "white_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["brown_wool.png", "white_wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 108, name: "black_wool", color: [0.12, 0.12, 0.12], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["black_wool.png", "white_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["black_wool.png", "white_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["black_wool.png", "white_wool.png"], keep_alpha: false },
    },
    // Stone family extras (ids 109..=116).
    BlockType {
        id: 109, name: "mossy_cobblestone", color: [0.38, 0.48, 0.36], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["mossy_cobblestone.png", "cobblestone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["mossy_cobblestone.png", "cobblestone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["mossy_cobblestone.png", "cobblestone.png"], keep_alpha: false },
    },
    BlockType {
        id: 110, name: "stone_bricks", color: [0.50, 0.50, 0.55], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["stone_bricks.png", "stone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["stone_bricks.png", "stone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["stone_bricks.png", "stone.png"], keep_alpha: false },
    },
    BlockType {
        id: 111, name: "mossy_stone_bricks", color: [0.40, 0.52, 0.40], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["mossy_stone_bricks.png", "stone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["mossy_stone_bricks.png", "stone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["mossy_stone_bricks.png", "stone.png"], keep_alpha: false },
    },
    BlockType {
        id: 112, name: "cracked_stone_bricks", color: [0.48, 0.48, 0.52], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["cracked_stone_bricks.png", "stone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["cracked_stone_bricks.png", "stone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["cracked_stone_bricks.png", "stone.png"], keep_alpha: false },
    },
    BlockType {
        id: 113, name: "chiseled_stone_bricks", color: [0.52, 0.52, 0.56], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["chiseled_stone_bricks.png", "stone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["chiseled_stone_bricks.png", "stone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["chiseled_stone_bricks.png", "stone.png"], keep_alpha: false },
    },
    BlockType {
        id: 114, name: "polished_granite", color: [0.72, 0.52, 0.44], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["polished_granite.png", "granite.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["polished_granite.png", "granite.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["polished_granite.png", "granite.png"], keep_alpha: false },
    },
    BlockType {
        id: 115, name: "polished_diorite", color: [0.86, 0.86, 0.86], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["polished_diorite.png", "diorite.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["polished_diorite.png", "diorite.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["polished_diorite.png", "diorite.png"], keep_alpha: false },
    },
    BlockType {
        id: 116, name: "polished_andesite", color: [0.62, 0.62, 0.62], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["polished_andesite.png", "andesite.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["polished_andesite.png", "andesite.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["polished_andesite.png", "andesite.png"], keep_alpha: false },
    },
    // Ores (ids 117..=124). Coal (13) + iron (14) already exist.
    BlockType {
        id: 117, name: "gold_ore", color: [0.88, 0.78, 0.28], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["gold_ore.png", "stone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["gold_ore.png", "stone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["gold_ore.png", "stone.png"], keep_alpha: false },
    },
    BlockType {
        id: 118, name: "diamond_ore", color: [0.45, 0.85, 0.85], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["diamond_ore.png", "stone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["diamond_ore.png", "stone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["diamond_ore.png", "stone.png"], keep_alpha: false },
    },
    BlockType {
        id: 119, name: "redstone_ore", color: [0.72, 0.22, 0.22], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["redstone_ore.png", "stone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["redstone_ore.png", "stone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["redstone_ore.png", "stone.png"], keep_alpha: false },
    },
    BlockType {
        id: 120, name: "lapis_ore", color: [0.28, 0.42, 0.72], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["lapis_ore.png", "stone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["lapis_ore.png", "stone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["lapis_ore.png", "stone.png"], keep_alpha: false },
    },
    BlockType {
        id: 121, name: "emerald_ore", color: [0.28, 0.75, 0.38], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["emerald_ore.png", "stone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["emerald_ore.png", "stone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["emerald_ore.png", "stone.png"], keep_alpha: false },
    },
    BlockType {
        id: 122, name: "copper_ore", color: [0.72, 0.45, 0.32], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["copper_ore.png", "stone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["copper_ore.png", "stone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["copper_ore.png", "stone.png"], keep_alpha: false },
    },
    // Deepslate ore variants (ids 123..=126).
    BlockType {
        id: 123, name: "deepslate_coal_ore", color: [0.28, 0.28, 0.30], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["deepslate_coal_ore.png", "deepslate.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["deepslate_coal_ore.png", "deepslate.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["deepslate_coal_ore.png", "deepslate.png"], keep_alpha: false },
    },
    BlockType {
        id: 124, name: "deepslate_iron_ore", color: [0.52, 0.48, 0.45], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["deepslate_iron_ore.png", "deepslate.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["deepslate_iron_ore.png", "deepslate.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["deepslate_iron_ore.png", "deepslate.png"], keep_alpha: false },
    },
    BlockType {
        id: 125, name: "deepslate_diamond_ore", color: [0.34, 0.56, 0.56], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["deepslate_diamond_ore.png", "deepslate.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["deepslate_diamond_ore.png", "deepslate.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["deepslate_diamond_ore.png", "deepslate.png"], keep_alpha: false },
    },
    BlockType {
        id: 126, name: "deepslate_gold_ore", color: [0.60, 0.56, 0.28], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["deepslate_gold_ore.png", "deepslate.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["deepslate_gold_ore.png", "deepslate.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["deepslate_gold_ore.png", "deepslate.png"], keep_alpha: false },
    },
    // Wood variants: acacia (ids 127..=129).
    BlockType {
        id: 127, name: "acacia_log", color: [0.60, 0.42, 0.32], transparent: false, flags: 0,
        top: TileSpec { col: 3, row: 1, files: &["acacia_log_top.png", "oak_log_top.png"], keep_alpha: false },
        bottom: TileSpec { col: 3, row: 1, files: &["acacia_log_top.png", "oak_log_top.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 1, files: &["acacia_log.png", "oak_log.png"], keep_alpha: false },
    },
    BlockType {
        id: 128, name: "acacia_planks", color: [0.82, 0.52, 0.32], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 2, files: &["acacia_planks.png", "oak_planks.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 2, files: &["acacia_planks.png", "oak_planks.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 2, files: &["acacia_planks.png", "oak_planks.png"], keep_alpha: false },
    },
    BlockType {
        id: 129, name: "acacia_leaves", color: [0.30, 0.52, 0.22], transparent: true, flags: 0,
        top: TileSpec { col: 1, row: 1, files: &["acacia_leaves.png", "oak_leaves.png"], keep_alpha: true },
        bottom: TileSpec { col: 1, row: 1, files: &["acacia_leaves.png", "oak_leaves.png"], keep_alpha: true },
        sides: TileSpec { col: 1, row: 1, files: &["acacia_leaves.png", "oak_leaves.png"], keep_alpha: true },
    },
    // Spruce (ids 130..=132).
    BlockType {
        id: 130, name: "spruce_log", color: [0.40, 0.30, 0.20], transparent: false, flags: 0,
        top: TileSpec { col: 3, row: 1, files: &["spruce_log_top.png", "oak_log_top.png"], keep_alpha: false },
        bottom: TileSpec { col: 3, row: 1, files: &["spruce_log_top.png", "oak_log_top.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 1, files: &["spruce_log.png", "oak_log.png"], keep_alpha: false },
    },
    BlockType {
        id: 131, name: "spruce_planks", color: [0.58, 0.40, 0.25], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 2, files: &["spruce_planks.png", "oak_planks.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 2, files: &["spruce_planks.png", "oak_planks.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 2, files: &["spruce_planks.png", "oak_planks.png"], keep_alpha: false },
    },
    BlockType {
        id: 132, name: "spruce_leaves", color: [0.22, 0.40, 0.18], transparent: true, flags: 0,
        top: TileSpec { col: 1, row: 1, files: &["spruce_leaves.png", "oak_leaves.png"], keep_alpha: true },
        bottom: TileSpec { col: 1, row: 1, files: &["spruce_leaves.png", "oak_leaves.png"], keep_alpha: true },
        sides: TileSpec { col: 1, row: 1, files: &["spruce_leaves.png", "oak_leaves.png"], keep_alpha: true },
    },
    // Jungle (ids 133..=135).
    BlockType {
        id: 133, name: "jungle_log", color: [0.50, 0.38, 0.24], transparent: false, flags: 0,
        top: TileSpec { col: 3, row: 1, files: &["jungle_log_top.png", "oak_log_top.png"], keep_alpha: false },
        bottom: TileSpec { col: 3, row: 1, files: &["jungle_log_top.png", "oak_log_top.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 1, files: &["jungle_log.png", "oak_log.png"], keep_alpha: false },
    },
    BlockType {
        id: 134, name: "jungle_planks", color: [0.68, 0.50, 0.30], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 2, files: &["jungle_planks.png", "oak_planks.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 2, files: &["jungle_planks.png", "oak_planks.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 2, files: &["jungle_planks.png", "oak_planks.png"], keep_alpha: false },
    },
    BlockType {
        id: 135, name: "jungle_leaves", color: [0.28, 0.55, 0.20], transparent: true, flags: 0,
        top: TileSpec { col: 1, row: 1, files: &["jungle_leaves.png", "oak_leaves.png"], keep_alpha: true },
        bottom: TileSpec { col: 1, row: 1, files: &["jungle_leaves.png", "oak_leaves.png"], keep_alpha: true },
        sides: TileSpec { col: 1, row: 1, files: &["jungle_leaves.png", "oak_leaves.png"], keep_alpha: true },
    },
    // Dark oak (ids 136..=138).
    BlockType {
        id: 136, name: "dark_oak_log", color: [0.28, 0.20, 0.12], transparent: false, flags: 0,
        top: TileSpec { col: 3, row: 1, files: &["dark_oak_log_top.png", "oak_log_top.png"], keep_alpha: false },
        bottom: TileSpec { col: 3, row: 1, files: &["dark_oak_log_top.png", "oak_log_top.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 1, files: &["dark_oak_log.png", "oak_log.png"], keep_alpha: false },
    },
    BlockType {
        id: 137, name: "dark_oak_planks", color: [0.38, 0.25, 0.15], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 2, files: &["dark_oak_planks.png", "oak_planks.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 2, files: &["dark_oak_planks.png", "oak_planks.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 2, files: &["dark_oak_planks.png", "oak_planks.png"], keep_alpha: false },
    },
    BlockType {
        id: 138, name: "dark_oak_leaves", color: [0.20, 0.40, 0.14], transparent: true, flags: 0,
        top: TileSpec { col: 1, row: 1, files: &["dark_oak_leaves.png", "oak_leaves.png"], keep_alpha: true },
        bottom: TileSpec { col: 1, row: 1, files: &["dark_oak_leaves.png", "oak_leaves.png"], keep_alpha: true },
        sides: TileSpec { col: 1, row: 1, files: &["dark_oak_leaves.png", "oak_leaves.png"], keep_alpha: true },
    },
    // Flowers (ids 139..=147). Poppy (20), dandelion (21), blue_orchid
    // (22) already exist; adding remaining common flowers.
    BlockType {
        id: 139, name: "allium", color: [0.65, 0.45, 0.90], transparent: true,
        flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE,
        top: TileSpec { col: 0, row: 0, files: &["allium.png", "poppy.png"], keep_alpha: true },
        bottom: TileSpec { col: 0, row: 0, files: &["allium.png", "poppy.png"], keep_alpha: true },
        sides: TileSpec { col: 0, row: 0, files: &["allium.png", "poppy.png"], keep_alpha: true },
    },
    BlockType {
        id: 140, name: "azure_bluet", color: [0.78, 0.85, 0.90], transparent: true,
        flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE,
        top: TileSpec { col: 0, row: 0, files: &["azure_bluet.png", "poppy.png"], keep_alpha: true },
        bottom: TileSpec { col: 0, row: 0, files: &["azure_bluet.png", "poppy.png"], keep_alpha: true },
        sides: TileSpec { col: 0, row: 0, files: &["azure_bluet.png", "poppy.png"], keep_alpha: true },
    },
    BlockType {
        id: 141, name: "red_tulip", color: [0.85, 0.28, 0.22], transparent: true,
        flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE,
        top: TileSpec { col: 0, row: 0, files: &["red_tulip.png", "poppy.png"], keep_alpha: true },
        bottom: TileSpec { col: 0, row: 0, files: &["red_tulip.png", "poppy.png"], keep_alpha: true },
        sides: TileSpec { col: 0, row: 0, files: &["red_tulip.png", "poppy.png"], keep_alpha: true },
    },
    BlockType {
        id: 142, name: "orange_tulip", color: [0.92, 0.58, 0.22], transparent: true,
        flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE,
        top: TileSpec { col: 0, row: 0, files: &["orange_tulip.png", "poppy.png"], keep_alpha: true },
        bottom: TileSpec { col: 0, row: 0, files: &["orange_tulip.png", "poppy.png"], keep_alpha: true },
        sides: TileSpec { col: 0, row: 0, files: &["orange_tulip.png", "poppy.png"], keep_alpha: true },
    },
    BlockType {
        id: 143, name: "white_tulip", color: [0.92, 0.92, 0.90], transparent: true,
        flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE,
        top: TileSpec { col: 0, row: 0, files: &["white_tulip.png", "poppy.png"], keep_alpha: true },
        bottom: TileSpec { col: 0, row: 0, files: &["white_tulip.png", "poppy.png"], keep_alpha: true },
        sides: TileSpec { col: 0, row: 0, files: &["white_tulip.png", "poppy.png"], keep_alpha: true },
    },
    BlockType {
        id: 144, name: "pink_tulip", color: [0.95, 0.72, 0.78], transparent: true,
        flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE,
        top: TileSpec { col: 0, row: 0, files: &["pink_tulip.png", "poppy.png"], keep_alpha: true },
        bottom: TileSpec { col: 0, row: 0, files: &["pink_tulip.png", "poppy.png"], keep_alpha: true },
        sides: TileSpec { col: 0, row: 0, files: &["pink_tulip.png", "poppy.png"], keep_alpha: true },
    },
    BlockType {
        id: 145, name: "oxeye_daisy", color: [0.92, 0.92, 0.82], transparent: true,
        flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE,
        top: TileSpec { col: 0, row: 0, files: &["oxeye_daisy.png", "poppy.png"], keep_alpha: true },
        bottom: TileSpec { col: 0, row: 0, files: &["oxeye_daisy.png", "poppy.png"], keep_alpha: true },
        sides: TileSpec { col: 0, row: 0, files: &["oxeye_daisy.png", "poppy.png"], keep_alpha: true },
    },
    BlockType {
        id: 146, name: "cornflower", color: [0.38, 0.55, 0.95], transparent: true,
        flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE,
        top: TileSpec { col: 0, row: 0, files: &["cornflower.png", "poppy.png"], keep_alpha: true },
        bottom: TileSpec { col: 0, row: 0, files: &["cornflower.png", "poppy.png"], keep_alpha: true },
        sides: TileSpec { col: 0, row: 0, files: &["cornflower.png", "poppy.png"], keep_alpha: true },
    },
    BlockType {
        id: 147, name: "lily_of_the_valley", color: [0.95, 0.98, 0.92], transparent: true,
        flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE,
        top: TileSpec { col: 0, row: 0, files: &["lily_of_the_valley.png", "poppy.png"], keep_alpha: true },
        bottom: TileSpec { col: 0, row: 0, files: &["lily_of_the_valley.png", "poppy.png"], keep_alpha: true },
        sides: TileSpec { col: 0, row: 0, files: &["lily_of_the_valley.png", "poppy.png"], keep_alpha: true },
    },
    // Misc terrain (ids 148..=153).
    BlockType {
        id: 148, name: "end_stone", color: [0.90, 0.90, 0.72], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["end_stone.png", "stone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["end_stone.png", "stone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["end_stone.png", "stone.png"], keep_alpha: false },
    },
    BlockType {
        id: 149, name: "soul_sand", color: [0.38, 0.28, 0.22], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["soul_sand.png", "sand.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["soul_sand.png", "sand.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["soul_sand.png", "sand.png"], keep_alpha: false },
    },
    BlockType {
        id: 150, name: "soul_soil", color: [0.32, 0.24, 0.18], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["soul_soil.png", "sand.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["soul_soil.png", "sand.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["soul_soil.png", "sand.png"], keep_alpha: false },
    },
    BlockType {
        id: 151, name: "bone_block", color: [0.88, 0.88, 0.80], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["bone_block_top.png", "bone_block_side.png", "stone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["bone_block_top.png", "stone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["bone_block_side.png", "stone.png"], keep_alpha: false },
    },
    BlockType {
        id: 152, name: "mycelium", color: [0.55, 0.42, 0.55], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["mycelium_top.png", "dirt.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["dirt.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["mycelium_side.png", "dirt.png"], keep_alpha: false },
    },
    BlockType {
        id: 153, name: "podzol", color: [0.40, 0.28, 0.18], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["podzol_top.png", "dirt.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["dirt.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["podzol_side.png", "dirt.png"], keep_alpha: false },
    },
    // Gold / diamond tools (ids 154..=161).
    // Tools have transparent=false + no flags (same as pickaxe_wood).
    BlockType {
        id: 154, name: "pickaxe_gold", color: [0.92, 0.80, 0.28], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["golden_pickaxe.png", "gold_ore.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["golden_pickaxe.png", "gold_ore.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["golden_pickaxe.png", "gold_ore.png"], keep_alpha: false },
    },
    BlockType {
        id: 155, name: "pickaxe_diamond", color: [0.45, 0.85, 0.85], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["diamond_pickaxe.png", "diamond_ore.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["diamond_pickaxe.png", "diamond_ore.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["diamond_pickaxe.png", "diamond_ore.png"], keep_alpha: false },
    },
    BlockType {
        id: 156, name: "sword_gold", color: [0.92, 0.80, 0.28], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["golden_sword.png", "gold_ore.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["golden_sword.png", "gold_ore.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["golden_sword.png", "gold_ore.png"], keep_alpha: false },
    },
    BlockType {
        id: 157, name: "sword_diamond", color: [0.45, 0.85, 0.85], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["diamond_sword.png", "diamond_ore.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["diamond_sword.png", "diamond_ore.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["diamond_sword.png", "diamond_ore.png"], keep_alpha: false },
    },
    BlockType {
        id: 158, name: "shovel_gold", color: [0.92, 0.80, 0.28], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["golden_shovel.png", "gold_ore.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["golden_shovel.png", "gold_ore.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["golden_shovel.png", "gold_ore.png"], keep_alpha: false },
    },
    BlockType {
        id: 159, name: "shovel_diamond", color: [0.45, 0.85, 0.85], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["diamond_shovel.png", "diamond_ore.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["diamond_shovel.png", "diamond_ore.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["diamond_shovel.png", "diamond_ore.png"], keep_alpha: false },
    },
    BlockType {
        id: 160, name: "axe_gold", color: [0.92, 0.80, 0.28], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["golden_axe.png", "gold_ore.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["golden_axe.png", "gold_ore.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["golden_axe.png", "gold_ore.png"], keep_alpha: false },
    },
    BlockType {
        id: 161, name: "axe_diamond", color: [0.45, 0.85, 0.85], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["diamond_axe.png", "diamond_ore.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["diamond_axe.png", "diamond_ore.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["diamond_axe.png", "diamond_ore.png"], keep_alpha: false },
    },
    // Food items - item-only, no block form. Stored here so the
    // 1:1 BLOCK_TYPES / ITEM_TYPES mirror stays intact (ids
    // 162..=168). Colors are indicative; these resolve via the
    // ITEM_NAME_MAP to MC item sprites rather than block textures.
    BlockType {
        id: 162, name: "apple", color: [0.85, 0.22, 0.18], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["apple.png", "red_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["apple.png", "red_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["apple.png", "red_wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 163, name: "bread", color: [0.75, 0.55, 0.30], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["bread.png", "sandstone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["bread.png", "sandstone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["bread.png", "sandstone.png"], keep_alpha: false },
    },
    BlockType {
        id: 164, name: "cooked_beef", color: [0.58, 0.30, 0.18], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["cooked_beef.png", "netherrack.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["cooked_beef.png", "netherrack.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["cooked_beef.png", "netherrack.png"], keep_alpha: false },
    },
    BlockType {
        id: 165, name: "raw_chicken", color: [0.92, 0.72, 0.65], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["chicken_raw.png", "pink_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["chicken_raw.png", "pink_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["chicken_raw.png", "pink_wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 166, name: "cooked_chicken", color: [0.72, 0.55, 0.35], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["chicken_cooked.png", "sandstone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["chicken_cooked.png", "sandstone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["chicken_cooked.png", "sandstone.png"], keep_alpha: false },
    },
    BlockType {
        id: 167, name: "cookie", color: [0.78, 0.58, 0.35], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["cookie.png", "sandstone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["cookie.png", "sandstone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["cookie.png", "sandstone.png"], keep_alpha: false },
    },
    BlockType {
        id: 168, name: "melon_slice", color: [0.72, 0.88, 0.40], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["melon_slice.png", "melon_side.png", "lime_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["melon_slice.png", "melon_side.png", "lime_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["melon_slice.png", "melon_side.png", "lime_wool.png"], keep_alpha: false },
    },
    // ---------------------------------------------------------------
    // W40.2 - Bulk MC item/block registry import.
    //
    // IDs 169..=370. Follows the same atlas-fallback pattern as
    // W37.2. For item-only entries (food, mob drops, ingredients)
    // transparent=false + flags=0 (they never render as world blocks).
    // ---------------------------------------------------------------

    // ---- Stained glass (16 colors, ids 169..=184) ----
    BlockType {
        id: 169, name: "white_stained_glass", color: [0.95, 0.95, 0.95], transparent: true, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["white_stained_glass.png", "glass.png"], keep_alpha: true },
        bottom: TileSpec { col: 0, row: 0, files: &["white_stained_glass.png", "glass.png"], keep_alpha: true },
        sides: TileSpec { col: 0, row: 0, files: &["white_stained_glass.png", "glass.png"], keep_alpha: true },
    },
    BlockType {
        id: 170, name: "orange_stained_glass", color: [0.88, 0.52, 0.15], transparent: true, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["orange_stained_glass.png", "glass.png"], keep_alpha: true },
        bottom: TileSpec { col: 0, row: 0, files: &["orange_stained_glass.png", "glass.png"], keep_alpha: true },
        sides: TileSpec { col: 0, row: 0, files: &["orange_stained_glass.png", "glass.png"], keep_alpha: true },
    },
    BlockType {
        id: 171, name: "magenta_stained_glass", color: [0.78, 0.28, 0.72], transparent: true, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["magenta_stained_glass.png", "glass.png"], keep_alpha: true },
        bottom: TileSpec { col: 0, row: 0, files: &["magenta_stained_glass.png", "glass.png"], keep_alpha: true },
        sides: TileSpec { col: 0, row: 0, files: &["magenta_stained_glass.png", "glass.png"], keep_alpha: true },
    },
    BlockType {
        id: 172, name: "light_blue_stained_glass", color: [0.42, 0.68, 0.90], transparent: true, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["light_blue_stained_glass.png", "glass.png"], keep_alpha: true },
        bottom: TileSpec { col: 0, row: 0, files: &["light_blue_stained_glass.png", "glass.png"], keep_alpha: true },
        sides: TileSpec { col: 0, row: 0, files: &["light_blue_stained_glass.png", "glass.png"], keep_alpha: true },
    },
    BlockType {
        id: 173, name: "yellow_stained_glass", color: [0.92, 0.82, 0.16], transparent: true, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["yellow_stained_glass.png", "glass.png"], keep_alpha: true },
        bottom: TileSpec { col: 0, row: 0, files: &["yellow_stained_glass.png", "glass.png"], keep_alpha: true },
        sides: TileSpec { col: 0, row: 0, files: &["yellow_stained_glass.png", "glass.png"], keep_alpha: true },
    },
    BlockType {
        id: 174, name: "lime_stained_glass", color: [0.52, 0.82, 0.22], transparent: true, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["lime_stained_glass.png", "glass.png"], keep_alpha: true },
        bottom: TileSpec { col: 0, row: 0, files: &["lime_stained_glass.png", "glass.png"], keep_alpha: true },
        sides: TileSpec { col: 0, row: 0, files: &["lime_stained_glass.png", "glass.png"], keep_alpha: true },
    },
    BlockType {
        id: 175, name: "pink_stained_glass", color: [0.92, 0.58, 0.65], transparent: true, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["pink_stained_glass.png", "glass.png"], keep_alpha: true },
        bottom: TileSpec { col: 0, row: 0, files: &["pink_stained_glass.png", "glass.png"], keep_alpha: true },
        sides: TileSpec { col: 0, row: 0, files: &["pink_stained_glass.png", "glass.png"], keep_alpha: true },
    },
    BlockType {
        id: 176, name: "gray_stained_glass", color: [0.40, 0.40, 0.40], transparent: true, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["gray_stained_glass.png", "glass.png"], keep_alpha: true },
        bottom: TileSpec { col: 0, row: 0, files: &["gray_stained_glass.png", "glass.png"], keep_alpha: true },
        sides: TileSpec { col: 0, row: 0, files: &["gray_stained_glass.png", "glass.png"], keep_alpha: true },
    },
    BlockType {
        id: 177, name: "light_gray_stained_glass", color: [0.68, 0.68, 0.68], transparent: true, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["light_gray_stained_glass.png", "glass.png"], keep_alpha: true },
        bottom: TileSpec { col: 0, row: 0, files: &["light_gray_stained_glass.png", "glass.png"], keep_alpha: true },
        sides: TileSpec { col: 0, row: 0, files: &["light_gray_stained_glass.png", "glass.png"], keep_alpha: true },
    },
    BlockType {
        id: 178, name: "cyan_stained_glass", color: [0.18, 0.52, 0.58], transparent: true, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["cyan_stained_glass.png", "glass.png"], keep_alpha: true },
        bottom: TileSpec { col: 0, row: 0, files: &["cyan_stained_glass.png", "glass.png"], keep_alpha: true },
        sides: TileSpec { col: 0, row: 0, files: &["cyan_stained_glass.png", "glass.png"], keep_alpha: true },
    },
    BlockType {
        id: 179, name: "purple_stained_glass", color: [0.50, 0.18, 0.68], transparent: true, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["purple_stained_glass.png", "glass.png"], keep_alpha: true },
        bottom: TileSpec { col: 0, row: 0, files: &["purple_stained_glass.png", "glass.png"], keep_alpha: true },
        sides: TileSpec { col: 0, row: 0, files: &["purple_stained_glass.png", "glass.png"], keep_alpha: true },
    },
    BlockType {
        id: 180, name: "blue_stained_glass", color: [0.22, 0.32, 0.72], transparent: true, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["blue_stained_glass.png", "glass.png"], keep_alpha: true },
        bottom: TileSpec { col: 0, row: 0, files: &["blue_stained_glass.png", "glass.png"], keep_alpha: true },
        sides: TileSpec { col: 0, row: 0, files: &["blue_stained_glass.png", "glass.png"], keep_alpha: true },
    },
    BlockType {
        id: 181, name: "brown_stained_glass", color: [0.52, 0.32, 0.18], transparent: true, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["brown_stained_glass.png", "glass.png"], keep_alpha: true },
        bottom: TileSpec { col: 0, row: 0, files: &["brown_stained_glass.png", "glass.png"], keep_alpha: true },
        sides: TileSpec { col: 0, row: 0, files: &["brown_stained_glass.png", "glass.png"], keep_alpha: true },
    },
    BlockType {
        id: 182, name: "green_stained_glass", color: [0.28, 0.52, 0.20], transparent: true, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["green_stained_glass.png", "glass.png"], keep_alpha: true },
        bottom: TileSpec { col: 0, row: 0, files: &["green_stained_glass.png", "glass.png"], keep_alpha: true },
        sides: TileSpec { col: 0, row: 0, files: &["green_stained_glass.png", "glass.png"], keep_alpha: true },
    },
    BlockType {
        id: 183, name: "red_stained_glass", color: [0.74, 0.18, 0.18], transparent: true, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["red_stained_glass.png", "glass.png"], keep_alpha: true },
        bottom: TileSpec { col: 0, row: 0, files: &["red_stained_glass.png", "glass.png"], keep_alpha: true },
        sides: TileSpec { col: 0, row: 0, files: &["red_stained_glass.png", "glass.png"], keep_alpha: true },
    },
    BlockType {
        id: 184, name: "black_stained_glass", color: [0.12, 0.12, 0.12], transparent: true, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["black_stained_glass.png", "glass.png"], keep_alpha: true },
        bottom: TileSpec { col: 0, row: 0, files: &["black_stained_glass.png", "glass.png"], keep_alpha: true },
        sides: TileSpec { col: 0, row: 0, files: &["black_stained_glass.png", "glass.png"], keep_alpha: true },
    },
    // ---- Concrete (16 colors, ids 185..=200) ----
    BlockType {
        id: 185, name: "white_concrete", color: [0.92, 0.92, 0.92], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["white_concrete.png", "white_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["white_concrete.png", "white_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["white_concrete.png", "white_wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 186, name: "orange_concrete", color: [0.82, 0.46, 0.12], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["orange_concrete.png", "orange_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["orange_concrete.png", "orange_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["orange_concrete.png", "orange_wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 187, name: "magenta_concrete", color: [0.70, 0.24, 0.68], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["magenta_concrete.png", "magenta_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["magenta_concrete.png", "magenta_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["magenta_concrete.png", "magenta_wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 188, name: "light_blue_concrete", color: [0.36, 0.62, 0.88], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["light_blue_concrete.png", "light_blue_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["light_blue_concrete.png", "light_blue_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["light_blue_concrete.png", "light_blue_wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 189, name: "yellow_concrete", color: [0.92, 0.80, 0.12], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["yellow_concrete.png", "yellow_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["yellow_concrete.png", "yellow_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["yellow_concrete.png", "yellow_wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 190, name: "lime_concrete", color: [0.46, 0.76, 0.18], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["lime_concrete.png", "lime_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["lime_concrete.png", "lime_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["lime_concrete.png", "lime_wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 191, name: "pink_concrete", color: [0.90, 0.52, 0.60], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["pink_concrete.png", "pink_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["pink_concrete.png", "pink_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["pink_concrete.png", "pink_wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 192, name: "gray_concrete", color: [0.36, 0.36, 0.36], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["gray_concrete.png", "gray_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["gray_concrete.png", "gray_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["gray_concrete.png", "gray_wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 193, name: "light_gray_concrete", color: [0.62, 0.62, 0.62], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["light_gray_concrete.png", "light_gray_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["light_gray_concrete.png", "light_gray_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["light_gray_concrete.png", "light_gray_wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 194, name: "cyan_concrete", color: [0.14, 0.48, 0.54], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["cyan_concrete.png", "cyan_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["cyan_concrete.png", "cyan_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["cyan_concrete.png", "cyan_wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 195, name: "purple_concrete", color: [0.44, 0.14, 0.64], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["purple_concrete.png", "purple_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["purple_concrete.png", "purple_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["purple_concrete.png", "purple_wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 196, name: "blue_concrete", color: [0.18, 0.28, 0.68], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["blue_concrete.png", "blue_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["blue_concrete.png", "blue_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["blue_concrete.png", "blue_wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 197, name: "brown_concrete", color: [0.48, 0.28, 0.14], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["brown_concrete.png", "brown_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["brown_concrete.png", "brown_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["brown_concrete.png", "brown_wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 198, name: "green_concrete", color: [0.24, 0.48, 0.16], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["green_concrete.png", "green_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["green_concrete.png", "green_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["green_concrete.png", "green_wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 199, name: "red_concrete", color: [0.68, 0.14, 0.14], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["red_concrete.png", "red_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["red_concrete.png", "red_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["red_concrete.png", "red_wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 200, name: "black_concrete", color: [0.08, 0.08, 0.08], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["black_concrete.png", "black_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["black_concrete.png", "black_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["black_concrete.png", "black_wool.png"], keep_alpha: false },
    },
    // ---- Concrete powder (16 colors, ids 201..=216) ----
    BlockType {
        id: 201, name: "white_concrete_powder", color: [0.90, 0.88, 0.84], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["white_concrete_powder.png", "sand.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["white_concrete_powder.png", "sand.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["white_concrete_powder.png", "sand.png"], keep_alpha: false },
    },
    BlockType {
        id: 202, name: "orange_concrete_powder", color: [0.88, 0.54, 0.18], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["orange_concrete_powder.png", "sand.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["orange_concrete_powder.png", "sand.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["orange_concrete_powder.png", "sand.png"], keep_alpha: false },
    },
    BlockType {
        id: 203, name: "magenta_concrete_powder", color: [0.82, 0.38, 0.80], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["magenta_concrete_powder.png", "sand.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["magenta_concrete_powder.png", "sand.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["magenta_concrete_powder.png", "sand.png"], keep_alpha: false },
    },
    BlockType {
        id: 204, name: "light_blue_concrete_powder", color: [0.50, 0.72, 0.92], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["light_blue_concrete_powder.png", "sand.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["light_blue_concrete_powder.png", "sand.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["light_blue_concrete_powder.png", "sand.png"], keep_alpha: false },
    },
    BlockType {
        id: 205, name: "yellow_concrete_powder", color: [0.96, 0.88, 0.26], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["yellow_concrete_powder.png", "sand.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["yellow_concrete_powder.png", "sand.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["yellow_concrete_powder.png", "sand.png"], keep_alpha: false },
    },
    BlockType {
        id: 206, name: "lime_concrete_powder", color: [0.58, 0.86, 0.26], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["lime_concrete_powder.png", "sand.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["lime_concrete_powder.png", "sand.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["lime_concrete_powder.png", "sand.png"], keep_alpha: false },
    },
    BlockType {
        id: 207, name: "pink_concrete_powder", color: [0.94, 0.62, 0.72], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["pink_concrete_powder.png", "sand.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["pink_concrete_powder.png", "sand.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["pink_concrete_powder.png", "sand.png"], keep_alpha: false },
    },
    BlockType {
        id: 208, name: "gray_concrete_powder", color: [0.46, 0.46, 0.46], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["gray_concrete_powder.png", "sand.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["gray_concrete_powder.png", "sand.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["gray_concrete_powder.png", "sand.png"], keep_alpha: false },
    },
    BlockType {
        id: 209, name: "light_gray_concrete_powder", color: [0.72, 0.72, 0.68], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["light_gray_concrete_powder.png", "sand.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["light_gray_concrete_powder.png", "sand.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["light_gray_concrete_powder.png", "sand.png"], keep_alpha: false },
    },
    BlockType {
        id: 210, name: "cyan_concrete_powder", color: [0.20, 0.58, 0.64], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["cyan_concrete_powder.png", "sand.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["cyan_concrete_powder.png", "sand.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["cyan_concrete_powder.png", "sand.png"], keep_alpha: false },
    },
    BlockType {
        id: 211, name: "purple_concrete_powder", color: [0.56, 0.22, 0.76], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["purple_concrete_powder.png", "sand.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["purple_concrete_powder.png", "sand.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["purple_concrete_powder.png", "sand.png"], keep_alpha: false },
    },
    BlockType {
        id: 212, name: "blue_concrete_powder", color: [0.26, 0.36, 0.80], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["blue_concrete_powder.png", "sand.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["blue_concrete_powder.png", "sand.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["blue_concrete_powder.png", "sand.png"], keep_alpha: false },
    },
    BlockType {
        id: 213, name: "brown_concrete_powder", color: [0.58, 0.36, 0.22], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["brown_concrete_powder.png", "sand.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["brown_concrete_powder.png", "sand.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["brown_concrete_powder.png", "sand.png"], keep_alpha: false },
    },
    BlockType {
        id: 214, name: "green_concrete_powder", color: [0.32, 0.56, 0.22], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["green_concrete_powder.png", "sand.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["green_concrete_powder.png", "sand.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["green_concrete_powder.png", "sand.png"], keep_alpha: false },
    },
    BlockType {
        id: 215, name: "red_concrete_powder", color: [0.78, 0.22, 0.22], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["red_concrete_powder.png", "sand.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["red_concrete_powder.png", "sand.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["red_concrete_powder.png", "sand.png"], keep_alpha: false },
    },
    BlockType {
        id: 216, name: "black_concrete_powder", color: [0.16, 0.16, 0.16], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["black_concrete_powder.png", "sand.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["black_concrete_powder.png", "sand.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["black_concrete_powder.png", "sand.png"], keep_alpha: false },
    },
    // ---- Carpet (16 colors, ids 217..=232) ----
    BlockType {
        id: 217, name: "white_carpet", color: [0.95, 0.95, 0.95], transparent: true,
        flags: F_DEDICATED_RENDER,
        top: TileSpec { col: 0, row: 0, files: &["white_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["white_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["white_wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 218, name: "orange_carpet", color: [0.88, 0.52, 0.15], transparent: true,
        flags: F_DEDICATED_RENDER,
        top: TileSpec { col: 0, row: 0, files: &["orange_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["orange_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["orange_wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 219, name: "magenta_carpet", color: [0.78, 0.28, 0.72], transparent: true,
        flags: F_DEDICATED_RENDER,
        top: TileSpec { col: 0, row: 0, files: &["magenta_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["magenta_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["magenta_wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 220, name: "light_blue_carpet", color: [0.42, 0.68, 0.90], transparent: true,
        flags: F_DEDICATED_RENDER,
        top: TileSpec { col: 0, row: 0, files: &["light_blue_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["light_blue_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["light_blue_wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 221, name: "yellow_carpet", color: [0.92, 0.82, 0.16], transparent: true,
        flags: F_DEDICATED_RENDER,
        top: TileSpec { col: 0, row: 0, files: &["yellow_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["yellow_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["yellow_wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 222, name: "lime_carpet", color: [0.52, 0.82, 0.22], transparent: true,
        flags: F_DEDICATED_RENDER,
        top: TileSpec { col: 0, row: 0, files: &["lime_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["lime_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["lime_wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 223, name: "pink_carpet", color: [0.92, 0.58, 0.65], transparent: true,
        flags: F_DEDICATED_RENDER,
        top: TileSpec { col: 0, row: 0, files: &["pink_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["pink_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["pink_wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 224, name: "gray_carpet", color: [0.40, 0.40, 0.40], transparent: true,
        flags: F_DEDICATED_RENDER,
        top: TileSpec { col: 0, row: 0, files: &["gray_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["gray_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["gray_wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 225, name: "light_gray_carpet", color: [0.68, 0.68, 0.68], transparent: true,
        flags: F_DEDICATED_RENDER,
        top: TileSpec { col: 0, row: 0, files: &["light_gray_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["light_gray_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["light_gray_wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 226, name: "cyan_carpet", color: [0.18, 0.52, 0.58], transparent: true,
        flags: F_DEDICATED_RENDER,
        top: TileSpec { col: 0, row: 0, files: &["cyan_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["cyan_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["cyan_wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 227, name: "purple_carpet", color: [0.50, 0.18, 0.68], transparent: true,
        flags: F_DEDICATED_RENDER,
        top: TileSpec { col: 0, row: 0, files: &["purple_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["purple_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["purple_wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 228, name: "blue_carpet", color: [0.22, 0.32, 0.72], transparent: true,
        flags: F_DEDICATED_RENDER,
        top: TileSpec { col: 0, row: 0, files: &["blue_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["blue_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["blue_wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 229, name: "brown_carpet", color: [0.52, 0.32, 0.18], transparent: true,
        flags: F_DEDICATED_RENDER,
        top: TileSpec { col: 0, row: 0, files: &["brown_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["brown_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["brown_wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 230, name: "green_carpet", color: [0.28, 0.52, 0.20], transparent: true,
        flags: F_DEDICATED_RENDER,
        top: TileSpec { col: 0, row: 0, files: &["green_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["green_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["green_wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 231, name: "red_carpet", color: [0.74, 0.18, 0.18], transparent: true,
        flags: F_DEDICATED_RENDER,
        top: TileSpec { col: 0, row: 0, files: &["red_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["red_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["red_wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 232, name: "black_carpet", color: [0.12, 0.12, 0.12], transparent: true,
        flags: F_DEDICATED_RENDER,
        top: TileSpec { col: 0, row: 0, files: &["black_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["black_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["black_wool.png"], keep_alpha: false },
    },
    // ---- New wood variants: mangrove (ids 233..=236) ----
    BlockType {
        id: 233, name: "mangrove_log", color: [0.48, 0.26, 0.16], transparent: false, flags: 0,
        top: TileSpec { col: 3, row: 1, files: &["mangrove_log_top.png", "oak_log_top.png"], keep_alpha: false },
        bottom: TileSpec { col: 3, row: 1, files: &["mangrove_log_top.png", "oak_log_top.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 1, files: &["mangrove_log.png", "oak_log.png"], keep_alpha: false },
    },
    BlockType {
        id: 234, name: "mangrove_planks", color: [0.72, 0.36, 0.24], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 2, files: &["mangrove_planks.png", "oak_planks.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 2, files: &["mangrove_planks.png", "oak_planks.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 2, files: &["mangrove_planks.png", "oak_planks.png"], keep_alpha: false },
    },
    BlockType {
        id: 235, name: "mangrove_leaves", color: [0.28, 0.52, 0.20], transparent: true, flags: 0,
        top: TileSpec { col: 1, row: 1, files: &["mangrove_leaves.png", "oak_leaves.png"], keep_alpha: true },
        bottom: TileSpec { col: 1, row: 1, files: &["mangrove_leaves.png", "oak_leaves.png"], keep_alpha: true },
        sides: TileSpec { col: 1, row: 1, files: &["mangrove_leaves.png", "oak_leaves.png"], keep_alpha: true },
    },
    BlockType {
        id: 236, name: "mangrove_roots", color: [0.42, 0.32, 0.20], transparent: true, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["mangrove_roots_top.png", "oak_log_top.png"], keep_alpha: true },
        bottom: TileSpec { col: 0, row: 0, files: &["mangrove_roots_top.png", "oak_log_top.png"], keep_alpha: true },
        sides: TileSpec { col: 0, row: 0, files: &["mangrove_roots_side.png", "oak_log.png"], keep_alpha: true },
    },
    // ---- Cherry wood (ids 237..=240) ----
    BlockType {
        id: 237, name: "cherry_log", color: [0.62, 0.32, 0.28], transparent: false, flags: 0,
        top: TileSpec { col: 3, row: 1, files: &["cherry_log_top.png", "oak_log_top.png"], keep_alpha: false },
        bottom: TileSpec { col: 3, row: 1, files: &["cherry_log_top.png", "oak_log_top.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 1, files: &["cherry_log.png", "oak_log.png"], keep_alpha: false },
    },
    BlockType {
        id: 238, name: "cherry_planks", color: [0.82, 0.62, 0.58], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 2, files: &["cherry_planks.png", "oak_planks.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 2, files: &["cherry_planks.png", "oak_planks.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 2, files: &["cherry_planks.png", "oak_planks.png"], keep_alpha: false },
    },
    BlockType {
        id: 239, name: "cherry_leaves", color: [0.92, 0.60, 0.72], transparent: true, flags: 0,
        top: TileSpec { col: 1, row: 1, files: &["cherry_leaves.png", "oak_leaves.png"], keep_alpha: true },
        bottom: TileSpec { col: 1, row: 1, files: &["cherry_leaves.png", "oak_leaves.png"], keep_alpha: true },
        sides: TileSpec { col: 1, row: 1, files: &["cherry_leaves.png", "oak_leaves.png"], keep_alpha: true },
    },
    // ---- Crimson wood (ids 240..=243) ----
    BlockType {
        id: 240, name: "crimson_stem", color: [0.50, 0.14, 0.24], transparent: false, flags: 0,
        top: TileSpec { col: 3, row: 1, files: &["crimson_stem_top.png", "oak_log_top.png"], keep_alpha: false },
        bottom: TileSpec { col: 3, row: 1, files: &["crimson_stem_top.png", "oak_log_top.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 1, files: &["crimson_stem.png", "netherrack.png"], keep_alpha: false },
    },
    BlockType {
        id: 241, name: "crimson_planks", color: [0.58, 0.22, 0.28], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 2, files: &["crimson_planks.png", "oak_planks.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 2, files: &["crimson_planks.png", "oak_planks.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 2, files: &["crimson_planks.png", "oak_planks.png"], keep_alpha: false },
    },
    BlockType {
        id: 242, name: "crimson_hyphae", color: [0.48, 0.14, 0.22], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["crimson_hyphae.png", "netherrack.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["crimson_hyphae.png", "netherrack.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["crimson_hyphae.png", "netherrack.png"], keep_alpha: false },
    },
    // ---- Warped wood (ids 243..=245) ----
    BlockType {
        id: 243, name: "warped_stem", color: [0.14, 0.44, 0.42], transparent: false, flags: 0,
        top: TileSpec { col: 3, row: 1, files: &["warped_stem_top.png", "oak_log_top.png"], keep_alpha: false },
        bottom: TileSpec { col: 3, row: 1, files: &["warped_stem_top.png", "oak_log_top.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 1, files: &["warped_stem.png", "oak_log.png"], keep_alpha: false },
    },
    BlockType {
        id: 244, name: "warped_planks", color: [0.22, 0.58, 0.56], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 2, files: &["warped_planks.png", "oak_planks.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 2, files: &["warped_planks.png", "oak_planks.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 2, files: &["warped_planks.png", "oak_planks.png"], keep_alpha: false },
    },
    BlockType {
        id: 245, name: "warped_hyphae", color: [0.12, 0.40, 0.40], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["warped_hyphae.png", "oak_log.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["warped_hyphae.png", "oak_log.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["warped_hyphae.png", "oak_log.png"], keep_alpha: false },
    },
    // ---- Remaining deepslate ores (ids 246..=249) ----
    BlockType {
        id: 246, name: "deepslate_redstone_ore", color: [0.54, 0.22, 0.24], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["deepslate_redstone_ore.png", "deepslate.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["deepslate_redstone_ore.png", "deepslate.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["deepslate_redstone_ore.png", "deepslate.png"], keep_alpha: false },
    },
    BlockType {
        id: 247, name: "deepslate_lapis_ore", color: [0.26, 0.38, 0.60], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["deepslate_lapis_ore.png", "deepslate.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["deepslate_lapis_ore.png", "deepslate.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["deepslate_lapis_ore.png", "deepslate.png"], keep_alpha: false },
    },
    BlockType {
        id: 248, name: "deepslate_emerald_ore", color: [0.24, 0.60, 0.34], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["deepslate_emerald_ore.png", "deepslate.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["deepslate_emerald_ore.png", "deepslate.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["deepslate_emerald_ore.png", "deepslate.png"], keep_alpha: false },
    },
    BlockType {
        id: 249, name: "deepslate_copper_ore", color: [0.60, 0.40, 0.28], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["deepslate_copper_ore.png", "deepslate.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["deepslate_copper_ore.png", "deepslate.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["deepslate_copper_ore.png", "deepslate.png"], keep_alpha: false },
    },
    // ---- Nether ores (ids 250..=251) ----
    BlockType {
        id: 250, name: "nether_quartz_ore", color: [0.65, 0.45, 0.40], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["nether_quartz_ore.png", "netherrack.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["nether_quartz_ore.png", "netherrack.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["nether_quartz_ore.png", "netherrack.png"], keep_alpha: false },
    },
    BlockType {
        id: 251, name: "nether_gold_ore", color: [0.78, 0.60, 0.22], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["nether_gold_ore.png", "netherrack.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["nether_gold_ore.png", "netherrack.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["nether_gold_ore.png", "netherrack.png"], keep_alpha: false },
    },
    // ---- Ancient debris (id 252) ----
    BlockType {
        id: 252, name: "ancient_debris", color: [0.42, 0.34, 0.28], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["ancient_debris_top.png", "netherrack.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["ancient_debris_top.png", "netherrack.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["ancient_debris_side.png", "netherrack.png"], keep_alpha: false },
    },
    // ---- Nether building blocks (ids 253..=258) ----
    BlockType {
        id: 253, name: "nether_bricks", color: [0.28, 0.12, 0.12], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["nether_bricks.png", "netherrack.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["nether_bricks.png", "netherrack.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["nether_bricks.png", "netherrack.png"], keep_alpha: false },
    },
    BlockType {
        id: 254, name: "red_nether_bricks", color: [0.42, 0.08, 0.08], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["red_nether_bricks.png", "netherrack.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["red_nether_bricks.png", "netherrack.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["red_nether_bricks.png", "netherrack.png"], keep_alpha: false },
    },
    BlockType {
        id: 255, name: "blackstone", color: [0.18, 0.14, 0.16], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["blackstone_top.png", "stone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["blackstone_top.png", "stone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["blackstone.png", "stone.png"], keep_alpha: false },
    },
    BlockType {
        id: 256, name: "polished_blackstone", color: [0.20, 0.16, 0.18], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["polished_blackstone.png", "stone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["polished_blackstone.png", "stone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["polished_blackstone.png", "stone.png"], keep_alpha: false },
    },
    BlockType {
        id: 257, name: "basalt", color: [0.32, 0.32, 0.34], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["basalt_top.png", "stone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["basalt_top.png", "stone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["basalt_side.png", "stone.png"], keep_alpha: false },
    },
    BlockType {
        id: 258, name: "smooth_basalt", color: [0.28, 0.28, 0.30], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["smooth_basalt.png", "stone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["smooth_basalt.png", "stone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["smooth_basalt.png", "stone.png"], keep_alpha: false },
    },
    // ---- End blocks (ids 259..=263) ----
    BlockType {
        id: 259, name: "end_stone_bricks", color: [0.92, 0.92, 0.74], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["end_stone_bricks.png", "end_stone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["end_stone_bricks.png", "end_stone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["end_stone_bricks.png", "end_stone.png"], keep_alpha: false },
    },
    BlockType {
        id: 260, name: "purpur_block", color: [0.65, 0.46, 0.65], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["purpur_block.png", "obsidian.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["purpur_block.png", "obsidian.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["purpur_block.png", "obsidian.png"], keep_alpha: false },
    },
    BlockType {
        id: 261, name: "purpur_pillar", color: [0.70, 0.50, 0.70], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["purpur_pillar_top.png", "purpur_block.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["purpur_pillar_top.png", "purpur_block.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["purpur_pillar.png", "purpur_block.png"], keep_alpha: false },
    },
    BlockType {
        id: 262, name: "dragon_egg", color: [0.10, 0.06, 0.12], transparent: true,
        flags: F_DEDICATED_RENDER,
        top: TileSpec { col: 0, row: 0, files: &["dragon_egg.png", "obsidian.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["dragon_egg.png", "obsidian.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["dragon_egg.png", "obsidian.png"], keep_alpha: false },
    },
    // ---- Prismarine family (ids 263..=269) ----
    BlockType {
        id: 263, name: "prismarine", color: [0.40, 0.72, 0.64], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["prismarine.png", "stone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["prismarine.png", "stone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["prismarine.png", "stone.png"], keep_alpha: false },
    },
    BlockType {
        id: 264, name: "prismarine_bricks", color: [0.38, 0.72, 0.62], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["prismarine_bricks.png", "stone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["prismarine_bricks.png", "stone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["prismarine_bricks.png", "stone.png"], keep_alpha: false },
    },
    BlockType {
        id: 265, name: "dark_prismarine", color: [0.22, 0.46, 0.40], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["dark_prismarine.png", "stone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["dark_prismarine.png", "stone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["dark_prismarine.png", "stone.png"], keep_alpha: false },
    },
    // ---- Copper blocks (ids 266..=271) ----
    BlockType {
        id: 266, name: "copper_block", color: [0.78, 0.52, 0.38], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["copper_block.png", "copper_ore.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["copper_block.png", "copper_ore.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["copper_block.png", "copper_ore.png"], keep_alpha: false },
    },
    BlockType {
        id: 267, name: "exposed_copper", color: [0.72, 0.58, 0.44], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["exposed_copper.png", "copper_block.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["exposed_copper.png", "copper_block.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["exposed_copper.png", "copper_block.png"], keep_alpha: false },
    },
    BlockType {
        id: 268, name: "weathered_copper", color: [0.50, 0.70, 0.50], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["weathered_copper.png", "copper_block.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["weathered_copper.png", "copper_block.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["weathered_copper.png", "copper_block.png"], keep_alpha: false },
    },
    BlockType {
        id: 269, name: "oxidized_copper", color: [0.34, 0.62, 0.54], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["oxidized_copper.png", "copper_block.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["oxidized_copper.png", "copper_block.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["oxidized_copper.png", "copper_block.png"], keep_alpha: false },
    },
    BlockType {
        id: 270, name: "cut_copper", color: [0.78, 0.52, 0.38], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["cut_copper.png", "copper_block.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["cut_copper.png", "copper_block.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["cut_copper.png", "copper_block.png"], keep_alpha: false },
    },
    // ---- Amethyst (ids 271..=274) ----
    BlockType {
        id: 271, name: "amethyst_block", color: [0.62, 0.42, 0.82], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["amethyst_block.png", "obsidian.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["amethyst_block.png", "obsidian.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["amethyst_block.png", "obsidian.png"], keep_alpha: false },
    },
    BlockType {
        id: 272, name: "budding_amethyst", color: [0.58, 0.38, 0.78], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["budding_amethyst.png", "amethyst_block.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["budding_amethyst.png", "amethyst_block.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["budding_amethyst.png", "amethyst_block.png"], keep_alpha: false },
    },
    BlockType {
        id: 273, name: "amethyst_cluster", color: [0.70, 0.50, 0.90], transparent: true,
        flags: F_RAY_PASS | F_AABB_PASS | F_DEDICATED_RENDER,
        top: TileSpec { col: 0, row: 0, files: &["amethyst_cluster.png", "amethyst_block.png"], keep_alpha: true },
        bottom: TileSpec { col: 0, row: 0, files: &["amethyst_cluster.png", "amethyst_block.png"], keep_alpha: true },
        sides: TileSpec { col: 0, row: 0, files: &["amethyst_cluster.png", "amethyst_block.png"], keep_alpha: true },
    },
    // ---- Netherite tools (ids 274..=278) ----
    BlockType {
        id: 274, name: "pickaxe_netherite", color: [0.30, 0.28, 0.30], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["netherite_pickaxe.png", "ancient_debris_top.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["netherite_pickaxe.png", "ancient_debris_top.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["netherite_pickaxe.png", "ancient_debris_top.png"], keep_alpha: false },
    },
    BlockType {
        id: 275, name: "sword_netherite", color: [0.30, 0.28, 0.30], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["netherite_sword.png", "ancient_debris_top.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["netherite_sword.png", "ancient_debris_top.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["netherite_sword.png", "ancient_debris_top.png"], keep_alpha: false },
    },
    BlockType {
        id: 276, name: "shovel_netherite", color: [0.30, 0.28, 0.30], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["netherite_shovel.png", "ancient_debris_top.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["netherite_shovel.png", "ancient_debris_top.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["netherite_shovel.png", "ancient_debris_top.png"], keep_alpha: false },
    },
    BlockType {
        id: 277, name: "axe_netherite", color: [0.30, 0.28, 0.30], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["netherite_axe.png", "ancient_debris_top.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["netherite_axe.png", "ancient_debris_top.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["netherite_axe.png", "ancient_debris_top.png"], keep_alpha: false },
    },
    BlockType {
        id: 278, name: "hoe_netherite", color: [0.30, 0.28, 0.30], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["netherite_hoe.png", "ancient_debris_top.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["netherite_hoe.png", "ancient_debris_top.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["netherite_hoe.png", "ancient_debris_top.png"], keep_alpha: false },
    },
    // ---- Netherite ingot + scrap (ids 279..=280) ----
    BlockType {
        id: 279, name: "netherite_ingot", color: [0.28, 0.26, 0.28], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["netherite_ingot.png", "ancient_debris_top.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["netherite_ingot.png", "ancient_debris_top.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["netherite_ingot.png", "ancient_debris_top.png"], keep_alpha: false },
    },
    BlockType {
        id: 280, name: "netherite_scrap", color: [0.32, 0.28, 0.24], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["netherite_scrap.png", "ancient_debris_top.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["netherite_scrap.png", "ancient_debris_top.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["netherite_scrap.png", "ancient_debris_top.png"], keep_alpha: false },
    },
    // ---- More food items (ids 281..=298) ----
    BlockType {
        id: 281, name: "pumpkin_pie", color: [0.85, 0.62, 0.32], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["pumpkin_pie.png", "sandstone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["pumpkin_pie.png", "sandstone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["pumpkin_pie.png", "sandstone.png"], keep_alpha: false },
    },
    BlockType {
        id: 282, name: "golden_apple", color: [0.92, 0.72, 0.12], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["golden_apple.png", "gold_ore.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["golden_apple.png", "gold_ore.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["golden_apple.png", "gold_ore.png"], keep_alpha: false },
    },
    BlockType {
        id: 283, name: "baked_potato", color: [0.78, 0.60, 0.32], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["baked_potato.png", "sandstone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["baked_potato.png", "sandstone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["baked_potato.png", "sandstone.png"], keep_alpha: false },
    },
    BlockType {
        id: 284, name: "raw_porkchop", color: [0.90, 0.62, 0.60], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["porkchop_raw.png", "pink_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["porkchop_raw.png", "pink_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["porkchop_raw.png", "pink_wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 285, name: "cooked_porkchop", color: [0.72, 0.44, 0.28], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["porkchop_cooked.png", "sandstone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["porkchop_cooked.png", "sandstone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["porkchop_cooked.png", "sandstone.png"], keep_alpha: false },
    },
    BlockType {
        id: 286, name: "raw_mutton", color: [0.88, 0.58, 0.52], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["mutton_raw.png", "pink_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["mutton_raw.png", "pink_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["mutton_raw.png", "pink_wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 287, name: "cooked_mutton", color: [0.68, 0.42, 0.26], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["mutton_cooked.png", "sandstone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["mutton_cooked.png", "sandstone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["mutton_cooked.png", "sandstone.png"], keep_alpha: false },
    },
    BlockType {
        id: 288, name: "raw_cod", color: [0.70, 0.60, 0.42], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["cod_raw.png", "sand.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["cod_raw.png", "sand.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["cod_raw.png", "sand.png"], keep_alpha: false },
    },
    BlockType {
        id: 289, name: "cooked_cod", color: [0.88, 0.76, 0.50], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["cod_cooked.png", "sandstone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["cod_cooked.png", "sandstone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["cod_cooked.png", "sandstone.png"], keep_alpha: false },
    },
    BlockType {
        id: 290, name: "raw_salmon", color: [0.82, 0.46, 0.38], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["salmon_raw.png", "pink_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["salmon_raw.png", "pink_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["salmon_raw.png", "pink_wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 291, name: "cooked_salmon", color: [0.72, 0.40, 0.28], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["salmon_cooked.png", "sandstone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["salmon_cooked.png", "sandstone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["salmon_cooked.png", "sandstone.png"], keep_alpha: false },
    },
    BlockType {
        id: 292, name: "dried_kelp", color: [0.30, 0.36, 0.22], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["dried_kelp.png", "green_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["dried_kelp.png", "green_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["dried_kelp.png", "green_wool.png"], keep_alpha: false },
    },
    // ---- Mob drops (ids 293..=307) ----
    BlockType {
        id: 293, name: "rotten_flesh", color: [0.52, 0.30, 0.22], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["rotten_flesh.png", "red_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["rotten_flesh.png", "red_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["rotten_flesh.png", "red_wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 294, name: "bone", color: [0.92, 0.90, 0.80], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["bone.png", "bone_block_side.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["bone.png", "bone_block_side.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["bone.png", "bone_block_side.png"], keep_alpha: false },
    },
    BlockType {
        id: 295, name: "ender_pearl", color: [0.30, 0.68, 0.60], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["ender_pearl.png", "obsidian.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["ender_pearl.png", "obsidian.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["ender_pearl.png", "obsidian.png"], keep_alpha: false },
    },
    BlockType {
        id: 296, name: "blaze_rod", color: [0.92, 0.70, 0.22], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["blaze_rod.png", "gold_ore.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["blaze_rod.png", "gold_ore.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["blaze_rod.png", "gold_ore.png"], keep_alpha: false },
    },
    BlockType {
        id: 297, name: "ghast_tear", color: [0.90, 0.90, 0.95], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["ghast_tear.png", "glass.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["ghast_tear.png", "glass.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["ghast_tear.png", "glass.png"], keep_alpha: false },
    },
    BlockType {
        id: 298, name: "slime_ball", color: [0.50, 0.78, 0.38], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["slime_ball.png", "lime_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["slime_ball.png", "lime_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["slime_ball.png", "lime_wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 299, name: "gunpowder", color: [0.55, 0.55, 0.52], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["gunpowder.png", "gray_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["gunpowder.png", "gray_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["gunpowder.png", "gray_wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 300, name: "spider_eye", color: [0.72, 0.18, 0.18], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["spider_eye.png", "red_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["spider_eye.png", "red_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["spider_eye.png", "red_wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 301, name: "string", color: [0.90, 0.88, 0.85], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["string.png", "white_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["string.png", "white_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["string.png", "white_wool.png"], keep_alpha: false },
    },
    // ---- Misc ingredients (ids 302..=320) ----
    BlockType {
        id: 302, name: "stick", color: [0.68, 0.46, 0.26], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["stick.png", "oak_planks.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["stick.png", "oak_planks.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["stick.png", "oak_planks.png"], keep_alpha: false },
    },
    BlockType {
        id: 303, name: "leather", color: [0.72, 0.48, 0.26], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["leather.png", "sandstone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["leather.png", "sandstone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["leather.png", "sandstone.png"], keep_alpha: false },
    },
    BlockType {
        id: 304, name: "paper", color: [0.96, 0.94, 0.86], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["paper.png", "sandstone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["paper.png", "sandstone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["paper.png", "sandstone.png"], keep_alpha: false },
    },
    BlockType {
        id: 305, name: "book", color: [0.72, 0.56, 0.38], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["book_normal.png", "sandstone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["book_normal.png", "sandstone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["book_normal.png", "sandstone.png"], keep_alpha: false },
    },
    BlockType {
        id: 306, name: "map", color: [0.88, 0.84, 0.70], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["empty_map.png", "sandstone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["empty_map.png", "sandstone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["empty_map.png", "sandstone.png"], keep_alpha: false },
    },
    BlockType {
        id: 307, name: "compass", color: [0.78, 0.72, 0.62], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["compass_16.png", "compass.png", "iron_ingot.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["compass_16.png", "compass.png", "iron_ingot.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["compass_16.png", "compass.png", "iron_ingot.png"], keep_alpha: false },
    },
    BlockType {
        id: 308, name: "clock", color: [0.90, 0.80, 0.30], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["clock_16.png", "clock.png", "gold_ore.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["clock_16.png", "clock.png", "gold_ore.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["clock_16.png", "clock.png", "gold_ore.png"], keep_alpha: false },
    },
    BlockType {
        id: 309, name: "redstone_dust", color: [0.72, 0.12, 0.08], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["redstone_dust_dot.png", "redstone_ore.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["redstone_dust_dot.png", "redstone_ore.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["redstone_dust_dot.png", "redstone_ore.png"], keep_alpha: false },
    },
    BlockType {
        id: 310, name: "glowstone_dust", color: [0.92, 0.84, 0.62], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["glowstone_dust.png", "glowstone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["glowstone_dust.png", "glowstone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["glowstone_dust.png", "glowstone.png"], keep_alpha: false },
    },
    BlockType {
        id: 311, name: "prismarine_shard", color: [0.40, 0.72, 0.64], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["prismarine_shard.png", "prismarine.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["prismarine_shard.png", "prismarine.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["prismarine_shard.png", "prismarine.png"], keep_alpha: false },
    },
    BlockType {
        id: 312, name: "prismarine_crystals", color: [0.60, 0.90, 0.82], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["prismarine_crystals.png", "prismarine.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["prismarine_crystals.png", "prismarine.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["prismarine_crystals.png", "prismarine.png"], keep_alpha: false },
    },
    BlockType {
        id: 313, name: "nautilus_shell", color: [0.90, 0.80, 0.68], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["nautilus_shell.png", "sand.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["nautilus_shell.png", "sand.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["nautilus_shell.png", "sand.png"], keep_alpha: false },
    },
    BlockType {
        id: 314, name: "heart_of_the_sea", color: [0.22, 0.60, 0.80], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["heart_of_the_sea.png", "prismarine.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["heart_of_the_sea.png", "prismarine.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["heart_of_the_sea.png", "prismarine.png"], keep_alpha: false },
    },
    BlockType {
        id: 315, name: "lapis_lazuli", color: [0.22, 0.36, 0.72], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["lapis_lazuli.png", "lapis_ore.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["lapis_lazuli.png", "lapis_ore.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["lapis_lazuli.png", "lapis_ore.png"], keep_alpha: false },
    },
    BlockType {
        id: 316, name: "diamond", color: [0.45, 0.85, 0.85], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["diamond.png", "diamond_ore.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["diamond.png", "diamond_ore.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["diamond.png", "diamond_ore.png"], keep_alpha: false },
    },
    BlockType {
        id: 317, name: "emerald", color: [0.28, 0.75, 0.38], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["emerald.png", "emerald_ore.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["emerald.png", "emerald_ore.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["emerald.png", "emerald_ore.png"], keep_alpha: false },
    },
    BlockType {
        id: 318, name: "gold_ingot", color: [0.92, 0.80, 0.28], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["gold_ingot.png", "gold_ore.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["gold_ingot.png", "gold_ore.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["gold_ingot.png", "gold_ore.png"], keep_alpha: false },
    },
    BlockType {
        id: 319, name: "sugar", color: [0.95, 0.95, 0.95], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["sugar.png", "white_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["sugar.png", "white_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["sugar.png", "white_wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 320, name: "blaze_powder", color: [0.90, 0.62, 0.12], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["blaze_powder.png", "gold_ore.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["blaze_powder.png", "gold_ore.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["blaze_powder.png", "gold_ore.png"], keep_alpha: false },
    },
    BlockType {
        id: 321, name: "magma_cream", color: [0.86, 0.40, 0.12], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["magma_cream.png", "magma.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["magma_cream.png", "magma.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["magma_cream.png", "magma.png"], keep_alpha: false },
    },
    BlockType {
        id: 322, name: "fermented_spider_eye", color: [0.52, 0.22, 0.22], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["fermented_spider_eye.png", "red_wool.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["fermented_spider_eye.png", "red_wool.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["fermented_spider_eye.png", "red_wool.png"], keep_alpha: false },
    },
    BlockType {
        id: 323, name: "amethyst_shard", color: [0.70, 0.50, 0.90], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["amethyst_shard.png", "amethyst_block.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["amethyst_shard.png", "amethyst_block.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["amethyst_shard.png", "amethyst_block.png"], keep_alpha: false },
    },
    // W46.21: copper torch family. MC 1.21+ added copper torches with
    // emission 14 (same as regular torch). JSON pipeline picks up the
    // template_torch.json geometry via the name_map "copper_torch"
    // mapping; def.color forced to white so the texture renders
    // unmodulated (matches W46.16 torch family).
    BlockType {
        id: 351, name: "copper_torch", color: [1.00, 1.00, 1.00], transparent: true,
        flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE,
        top: TileSpec { col: 0, row: 0, files: &["copper_torch.png", "torch.png"], keep_alpha: true },
        bottom: TileSpec { col: 0, row: 0, files: &["copper_torch.png", "torch.png"], keep_alpha: true },
        sides: TileSpec { col: 0, row: 0, files: &["copper_torch.png", "torch.png"], keep_alpha: true },
    },
    BlockType {
        id: 352, name: "copper_wall_torch", color: [1.00, 1.00, 1.00], transparent: true,
        flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE,
        top: TileSpec { col: 0, row: 0, files: &["copper_torch.png", "torch.png"], keep_alpha: true },
        bottom: TileSpec { col: 0, row: 0, files: &["copper_torch.png", "torch.png"], keep_alpha: true },
        sides: TileSpec { col: 0, row: 0, files: &["copper_torch.png", "torch.png"], keep_alpha: true },
    },
    // ============================================================
    // W48.1 batch 1: 50 new high-value blocks from MC reference pack.
    // IDs 353-402. Colors mirror MC's average texture tone; real
    // per-block textures come from the pack via the `files` fallback
    // chain (atlas col/row stays at slot 0 - unused when the pack
    // resolves the name).
    // ============================================================
    // Stone variants - polished + sandstone + deepslate + bricks.
    BlockType {
        id: 353, name: "polished_granite", color: [0.71, 0.50, 0.42], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["polished_granite.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["polished_granite.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["polished_granite.png"], keep_alpha: false },
    },
    BlockType {
        id: 354, name: "polished_diorite", color: [0.85, 0.85, 0.86], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["polished_diorite.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["polished_diorite.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["polished_diorite.png"], keep_alpha: false },
    },
    BlockType {
        id: 355, name: "polished_andesite", color: [0.60, 0.60, 0.61], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["polished_andesite.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["polished_andesite.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["polished_andesite.png"], keep_alpha: false },
    },
    BlockType {
        id: 356, name: "smooth_sandstone", color: [0.94, 0.88, 0.65], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["smooth_sandstone.png", "sandstone_top.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["smooth_sandstone.png", "sandstone_top.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["smooth_sandstone.png", "sandstone_top.png"], keep_alpha: false },
    },
    BlockType {
        id: 357, name: "cut_sandstone", color: [0.93, 0.87, 0.63], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["cut_sandstone.png", "sandstone_top.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["cut_sandstone.png", "sandstone_top.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["cut_sandstone.png"], keep_alpha: false },
    },
    BlockType {
        id: 358, name: "red_sandstone", color: [0.78, 0.42, 0.18], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["red_sandstone_top.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["red_sandstone_bottom.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["red_sandstone.png"], keep_alpha: false },
    },
    BlockType {
        id: 359, name: "smooth_red_sandstone", color: [0.78, 0.42, 0.18], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["smooth_red_sandstone.png", "red_sandstone_top.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["smooth_red_sandstone.png", "red_sandstone_top.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["smooth_red_sandstone.png", "red_sandstone_top.png"], keep_alpha: false },
    },
    BlockType {
        id: 360, name: "cut_red_sandstone", color: [0.78, 0.42, 0.18], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["cut_red_sandstone.png", "red_sandstone_top.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["cut_red_sandstone.png", "red_sandstone_top.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["cut_red_sandstone.png"], keep_alpha: false },
    },
    BlockType {
        id: 361, name: "cobbled_deepslate", color: [0.32, 0.32, 0.34], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["cobbled_deepslate.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["cobbled_deepslate.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["cobbled_deepslate.png"], keep_alpha: false },
    },
    BlockType {
        id: 362, name: "polished_deepslate", color: [0.30, 0.30, 0.32], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["polished_deepslate.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["polished_deepslate.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["polished_deepslate.png"], keep_alpha: false },
    },
    BlockType {
        id: 363, name: "deepslate_bricks", color: [0.32, 0.32, 0.34], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["deepslate_bricks.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["deepslate_bricks.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["deepslate_bricks.png"], keep_alpha: false },
    },
    BlockType {
        id: 364, name: "deepslate_tiles", color: [0.28, 0.28, 0.30], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["deepslate_tiles.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["deepslate_tiles.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["deepslate_tiles.png"], keep_alpha: false },
    },
    BlockType {
        id: 365, name: "chiseled_stone_bricks", color: [0.48, 0.48, 0.50], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["chiseled_stone_bricks.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["chiseled_stone_bricks.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["chiseled_stone_bricks.png"], keep_alpha: false },
    },
    BlockType {
        id: 366, name: "mossy_cobblestone", color: [0.40, 0.46, 0.38], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["mossy_cobblestone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["mossy_cobblestone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["mossy_cobblestone.png"], keep_alpha: false },
    },
    BlockType {
        id: 367, name: "mossy_stone_bricks", color: [0.45, 0.52, 0.42], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["mossy_stone_bricks.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["mossy_stone_bricks.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["mossy_stone_bricks.png"], keep_alpha: false },
    },
    // Nether content.
    BlockType {
        id: 368, name: "blackstone", color: [0.18, 0.16, 0.20], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["blackstone_top.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["blackstone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["blackstone.png"], keep_alpha: false },
    },
    BlockType {
        id: 369, name: "polished_blackstone", color: [0.16, 0.14, 0.18], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["polished_blackstone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["polished_blackstone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["polished_blackstone.png"], keep_alpha: false },
    },
    BlockType {
        id: 370, name: "polished_blackstone_bricks", color: [0.18, 0.16, 0.20], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["polished_blackstone_bricks.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["polished_blackstone_bricks.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["polished_blackstone_bricks.png"], keep_alpha: false },
    },
    BlockType {
        id: 371, name: "gilded_blackstone", color: [0.30, 0.24, 0.20], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["gilded_blackstone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["gilded_blackstone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["gilded_blackstone.png"], keep_alpha: false },
    },
    BlockType {
        id: 372, name: "basalt", color: [0.30, 0.28, 0.30], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["basalt_top.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["basalt_top.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["basalt_side.png"], keep_alpha: false },
    },
    BlockType {
        id: 373, name: "polished_basalt", color: [0.32, 0.30, 0.32], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["polished_basalt_top.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["polished_basalt_top.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["polished_basalt_side.png"], keep_alpha: false },
    },
    BlockType {
        id: 374, name: "soul_sand", color: [0.35, 0.26, 0.20], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["soul_sand.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["soul_sand.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["soul_sand.png"], keep_alpha: false },
    },
    BlockType {
        id: 375, name: "soul_soil", color: [0.32, 0.24, 0.18], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["soul_soil.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["soul_soil.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["soul_soil.png"], keep_alpha: false },
    },
    BlockType {
        id: 376, name: "nether_bricks", color: [0.30, 0.16, 0.18], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["nether_bricks.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["nether_bricks.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["nether_bricks.png"], keep_alpha: false },
    },
    BlockType {
        id: 377, name: "nether_wart_block", color: [0.66, 0.05, 0.05], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["nether_wart_block.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["nether_wart_block.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["nether_wart_block.png"], keep_alpha: false },
    },
    // End content.
    BlockType {
        id: 378, name: "end_stone", color: [0.86, 0.86, 0.65], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["end_stone.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["end_stone.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["end_stone.png"], keep_alpha: false },
    },
    BlockType {
        id: 379, name: "end_stone_bricks", color: [0.86, 0.86, 0.65], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["end_stone_bricks.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["end_stone_bricks.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["end_stone_bricks.png"], keep_alpha: false },
    },
    BlockType {
        id: 380, name: "purpur_block", color: [0.65, 0.42, 0.65], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["purpur_block.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["purpur_block.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["purpur_block.png"], keep_alpha: false },
    },
    BlockType {
        id: 381, name: "purpur_pillar", color: [0.65, 0.42, 0.65], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["purpur_pillar_top.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["purpur_pillar_top.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["purpur_pillar.png"], keep_alpha: false },
    },
    BlockType {
        id: 382, name: "chorus_flower", color: [0.55, 0.32, 0.58], transparent: true,
        flags: F_TRANSPARENT_RENDER,
        top: TileSpec { col: 0, row: 0, files: &["chorus_flower.png"], keep_alpha: true },
        bottom: TileSpec { col: 0, row: 0, files: &["chorus_flower.png"], keep_alpha: true },
        sides: TileSpec { col: 0, row: 0, files: &["chorus_flower.png"], keep_alpha: true },
    },
    // Copper full family + ore blocks.
    BlockType {
        id: 383, name: "copper_block", color: [0.78, 0.42, 0.25], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["copper_block.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["copper_block.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["copper_block.png"], keep_alpha: false },
    },
    BlockType {
        id: 384, name: "exposed_copper", color: [0.65, 0.48, 0.35], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["exposed_copper.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["exposed_copper.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["exposed_copper.png"], keep_alpha: false },
    },
    BlockType {
        id: 385, name: "weathered_copper", color: [0.45, 0.55, 0.40], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["weathered_copper.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["weathered_copper.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["weathered_copper.png"], keep_alpha: false },
    },
    BlockType {
        id: 386, name: "oxidized_copper", color: [0.32, 0.62, 0.52], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["oxidized_copper.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["oxidized_copper.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["oxidized_copper.png"], keep_alpha: false },
    },
    BlockType {
        id: 387, name: "cut_copper", color: [0.76, 0.41, 0.24], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["cut_copper.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["cut_copper.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["cut_copper.png"], keep_alpha: false },
    },
    BlockType {
        id: 388, name: "raw_copper_block", color: [0.65, 0.40, 0.28], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["raw_copper_block.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["raw_copper_block.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["raw_copper_block.png"], keep_alpha: false },
    },
    BlockType {
        id: 389, name: "raw_iron_block", color: [0.72, 0.55, 0.42], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["raw_iron_block.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["raw_iron_block.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["raw_iron_block.png"], keep_alpha: false },
    },
    BlockType {
        id: 390, name: "raw_gold_block", color: [0.85, 0.65, 0.20], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["raw_gold_block.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["raw_gold_block.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["raw_gold_block.png"], keep_alpha: false },
    },
    BlockType {
        id: 391, name: "gold_block", color: [0.95, 0.82, 0.25], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["gold_block.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["gold_block.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["gold_block.png"], keep_alpha: false },
    },
    BlockType {
        id: 392, name: "diamond_block", color: [0.50, 0.92, 0.92], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["diamond_block.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["diamond_block.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["diamond_block.png"], keep_alpha: false },
    },
    // Misc - earth + ice variants.
    BlockType {
        id: 393, name: "dirt_path", color: [0.55, 0.45, 0.28], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["dirt_path_top.png", "grass_path_top.png"], keep_alpha: false },
        bottom: TileSpec { col: 1, row: 0, files: &["dirt.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["dirt_path_side.png", "grass_path_side.png"], keep_alpha: false },
    },
    BlockType {
        id: 394, name: "podzol", color: [0.38, 0.25, 0.12], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["podzol_top.png"], keep_alpha: false },
        bottom: TileSpec { col: 1, row: 0, files: &["dirt.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["podzol_side.png"], keep_alpha: false },
    },
    BlockType {
        id: 395, name: "mycelium", color: [0.50, 0.40, 0.50], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["mycelium_top.png"], keep_alpha: false },
        bottom: TileSpec { col: 1, row: 0, files: &["dirt.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["mycelium_side.png"], keep_alpha: false },
    },
    BlockType {
        id: 396, name: "coarse_dirt", color: [0.40, 0.28, 0.18], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["coarse_dirt.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["coarse_dirt.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["coarse_dirt.png"], keep_alpha: false },
    },
    BlockType {
        id: 397, name: "mud", color: [0.30, 0.25, 0.25], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["mud.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["mud.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["mud.png"], keep_alpha: false },
    },
    BlockType {
        id: 398, name: "packed_mud", color: [0.55, 0.42, 0.32], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["packed_mud.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["packed_mud.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["packed_mud.png"], keep_alpha: false },
    },
    BlockType {
        id: 399, name: "mud_bricks", color: [0.60, 0.48, 0.36], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["mud_bricks.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["mud_bricks.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["mud_bricks.png"], keep_alpha: false },
    },
    BlockType {
        id: 400, name: "ice", color: [0.70, 0.85, 0.95], transparent: true,
        flags: F_TRANSPARENT_RENDER,
        top: TileSpec { col: 0, row: 0, files: &["ice.png"], keep_alpha: true },
        bottom: TileSpec { col: 0, row: 0, files: &["ice.png"], keep_alpha: true },
        sides: TileSpec { col: 0, row: 0, files: &["ice.png"], keep_alpha: true },
    },
    BlockType {
        id: 401, name: "packed_ice", color: [0.60, 0.78, 0.92], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["packed_ice.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["packed_ice.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["packed_ice.png"], keep_alpha: false },
    },
    BlockType {
        id: 402, name: "blue_ice", color: [0.42, 0.65, 0.92], transparent: false, flags: 0,
        top: TileSpec { col: 0, row: 0, files: &["blue_ice.png"], keep_alpha: false },
        bottom: TileSpec { col: 0, row: 0, files: &["blue_ice.png"], keep_alpha: false },
        sides: TileSpec { col: 0, row: 0, files: &["blue_ice.png"], keep_alpha: false },
    },
    // ============================================================
    // W48.2 batch 2: 50 new blocks (IDs 403-452).
    // Stained glass + terracotta + utility decor.
    // ============================================================
    // 16 stained glass colors.
    BlockType { id: 403, name: "white_stained_glass", color: [1.00, 1.00, 1.00], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["white_stained_glass.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["white_stained_glass.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["white_stained_glass.png"], keep_alpha: true } },
    BlockType { id: 404, name: "orange_stained_glass", color: [0.95, 0.55, 0.18], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["orange_stained_glass.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["orange_stained_glass.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["orange_stained_glass.png"], keep_alpha: true } },
    BlockType { id: 405, name: "magenta_stained_glass", color: [0.78, 0.30, 0.74], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["magenta_stained_glass.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["magenta_stained_glass.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["magenta_stained_glass.png"], keep_alpha: true } },
    BlockType { id: 406, name: "light_blue_stained_glass", color: [0.45, 0.70, 0.92], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["light_blue_stained_glass.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["light_blue_stained_glass.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["light_blue_stained_glass.png"], keep_alpha: true } },
    BlockType { id: 407, name: "yellow_stained_glass", color: [0.95, 0.82, 0.18], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["yellow_stained_glass.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["yellow_stained_glass.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["yellow_stained_glass.png"], keep_alpha: true } },
    BlockType { id: 408, name: "lime_stained_glass", color: [0.55, 0.83, 0.22], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["lime_stained_glass.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["lime_stained_glass.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["lime_stained_glass.png"], keep_alpha: true } },
    BlockType { id: 409, name: "pink_stained_glass", color: [0.95, 0.62, 0.72], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["pink_stained_glass.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["pink_stained_glass.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["pink_stained_glass.png"], keep_alpha: true } },
    BlockType { id: 410, name: "gray_stained_glass", color: [0.32, 0.32, 0.32], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["gray_stained_glass.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["gray_stained_glass.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["gray_stained_glass.png"], keep_alpha: true } },
    BlockType { id: 411, name: "light_gray_stained_glass", color: [0.62, 0.62, 0.62], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["light_gray_stained_glass.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["light_gray_stained_glass.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["light_gray_stained_glass.png"], keep_alpha: true } },
    BlockType { id: 412, name: "cyan_stained_glass", color: [0.18, 0.62, 0.65], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["cyan_stained_glass.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["cyan_stained_glass.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["cyan_stained_glass.png"], keep_alpha: true } },
    BlockType { id: 413, name: "purple_stained_glass", color: [0.55, 0.22, 0.78], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["purple_stained_glass.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["purple_stained_glass.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["purple_stained_glass.png"], keep_alpha: true } },
    BlockType { id: 414, name: "blue_stained_glass", color: [0.22, 0.32, 0.72], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["blue_stained_glass.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["blue_stained_glass.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["blue_stained_glass.png"], keep_alpha: true } },
    BlockType { id: 415, name: "brown_stained_glass", color: [0.45, 0.30, 0.18], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["brown_stained_glass.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["brown_stained_glass.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["brown_stained_glass.png"], keep_alpha: true } },
    BlockType { id: 416, name: "green_stained_glass", color: [0.28, 0.52, 0.20], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["green_stained_glass.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["green_stained_glass.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["green_stained_glass.png"], keep_alpha: true } },
    BlockType { id: 417, name: "red_stained_glass", color: [0.74, 0.18, 0.18], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["red_stained_glass.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["red_stained_glass.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["red_stained_glass.png"], keep_alpha: true } },
    BlockType { id: 418, name: "black_stained_glass", color: [0.10, 0.10, 0.12], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["black_stained_glass.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["black_stained_glass.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["black_stained_glass.png"], keep_alpha: true } },
    // 16 terracotta plain.
    BlockType { id: 419, name: "terracotta", color: [0.62, 0.40, 0.30], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["terracotta.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["terracotta.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["terracotta.png"], keep_alpha: false } },
    BlockType { id: 420, name: "white_terracotta", color: [0.82, 0.70, 0.62], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["white_terracotta.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["white_terracotta.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["white_terracotta.png"], keep_alpha: false } },
    BlockType { id: 421, name: "orange_terracotta", color: [0.65, 0.35, 0.18], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["orange_terracotta.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["orange_terracotta.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["orange_terracotta.png"], keep_alpha: false } },
    BlockType { id: 422, name: "magenta_terracotta", color: [0.60, 0.30, 0.45], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["magenta_terracotta.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["magenta_terracotta.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["magenta_terracotta.png"], keep_alpha: false } },
    BlockType { id: 423, name: "light_blue_terracotta", color: [0.42, 0.45, 0.55], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["light_blue_terracotta.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["light_blue_terracotta.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["light_blue_terracotta.png"], keep_alpha: false } },
    BlockType { id: 424, name: "yellow_terracotta", color: [0.72, 0.55, 0.20], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["yellow_terracotta.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["yellow_terracotta.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["yellow_terracotta.png"], keep_alpha: false } },
    BlockType { id: 425, name: "lime_terracotta", color: [0.42, 0.50, 0.23], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["lime_terracotta.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["lime_terracotta.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["lime_terracotta.png"], keep_alpha: false } },
    BlockType { id: 426, name: "pink_terracotta", color: [0.66, 0.38, 0.38], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["pink_terracotta.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["pink_terracotta.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["pink_terracotta.png"], keep_alpha: false } },
    BlockType { id: 427, name: "gray_terracotta", color: [0.30, 0.22, 0.18], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["gray_terracotta.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["gray_terracotta.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["gray_terracotta.png"], keep_alpha: false } },
    BlockType { id: 428, name: "light_gray_terracotta", color: [0.58, 0.45, 0.38], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["light_gray_terracotta.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["light_gray_terracotta.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["light_gray_terracotta.png"], keep_alpha: false } },
    BlockType { id: 429, name: "cyan_terracotta", color: [0.34, 0.40, 0.42], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["cyan_terracotta.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["cyan_terracotta.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["cyan_terracotta.png"], keep_alpha: false } },
    BlockType { id: 430, name: "purple_terracotta", color: [0.45, 0.28, 0.35], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["purple_terracotta.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["purple_terracotta.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["purple_terracotta.png"], keep_alpha: false } },
    BlockType { id: 431, name: "blue_terracotta", color: [0.30, 0.32, 0.42], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["blue_terracotta.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["blue_terracotta.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["blue_terracotta.png"], keep_alpha: false } },
    BlockType { id: 432, name: "brown_terracotta", color: [0.36, 0.24, 0.18], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["brown_terracotta.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["brown_terracotta.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["brown_terracotta.png"], keep_alpha: false } },
    BlockType { id: 433, name: "green_terracotta", color: [0.30, 0.32, 0.20], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["green_terracotta.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["green_terracotta.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["green_terracotta.png"], keep_alpha: false } },
    BlockType { id: 434, name: "red_terracotta", color: [0.56, 0.24, 0.18], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["red_terracotta.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["red_terracotta.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["red_terracotta.png"], keep_alpha: false } },
    // W150: the missing 16th plain colour + the full glazed set.
    // Glazed terracotta places unrotated for now (MC rotates the
    // pattern by facing - follow-up when block rotation state lands).
    BlockType { id: 684, name: "black_terracotta", color: [0.18, 0.12, 0.10], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["black_terracotta.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["black_terracotta.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["black_terracotta.png"], keep_alpha: false } },
    BlockType { id: 685, name: "white_glazed_terracotta", color: [0.85, 0.78, 0.72], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["white_glazed_terracotta.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["white_glazed_terracotta.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["white_glazed_terracotta.png"], keep_alpha: false } },
    BlockType { id: 686, name: "orange_glazed_terracotta", color: [0.72, 0.42, 0.16], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["orange_glazed_terracotta.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["orange_glazed_terracotta.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["orange_glazed_terracotta.png"], keep_alpha: false } },
    BlockType { id: 687, name: "magenta_glazed_terracotta", color: [0.72, 0.36, 0.60], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["magenta_glazed_terracotta.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["magenta_glazed_terracotta.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["magenta_glazed_terracotta.png"], keep_alpha: false } },
    BlockType { id: 688, name: "light_blue_glazed_terracotta", color: [0.38, 0.62, 0.78], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["light_blue_glazed_terracotta.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["light_blue_glazed_terracotta.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["light_blue_glazed_terracotta.png"], keep_alpha: false } },
    BlockType { id: 689, name: "yellow_glazed_terracotta", color: [0.80, 0.65, 0.22], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["yellow_glazed_terracotta.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["yellow_glazed_terracotta.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["yellow_glazed_terracotta.png"], keep_alpha: false } },
    BlockType { id: 690, name: "lime_glazed_terracotta", color: [0.50, 0.70, 0.22], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["lime_glazed_terracotta.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["lime_glazed_terracotta.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["lime_glazed_terracotta.png"], keep_alpha: false } },
    BlockType { id: 691, name: "pink_glazed_terracotta", color: [0.82, 0.52, 0.60], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["pink_glazed_terracotta.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["pink_glazed_terracotta.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["pink_glazed_terracotta.png"], keep_alpha: false } },
    BlockType { id: 692, name: "gray_glazed_terracotta", color: [0.32, 0.36, 0.40], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["gray_glazed_terracotta.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["gray_glazed_terracotta.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["gray_glazed_terracotta.png"], keep_alpha: false } },
    BlockType { id: 693, name: "light_gray_glazed_terracotta", color: [0.60, 0.62, 0.62], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["light_gray_glazed_terracotta.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["light_gray_glazed_terracotta.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["light_gray_glazed_terracotta.png"], keep_alpha: false } },
    BlockType { id: 694, name: "cyan_glazed_terracotta", color: [0.28, 0.52, 0.55], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["cyan_glazed_terracotta.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["cyan_glazed_terracotta.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["cyan_glazed_terracotta.png"], keep_alpha: false } },
    BlockType { id: 695, name: "purple_glazed_terracotta", color: [0.45, 0.28, 0.62], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["purple_glazed_terracotta.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["purple_glazed_terracotta.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["purple_glazed_terracotta.png"], keep_alpha: false } },
    BlockType { id: 696, name: "blue_glazed_terracotta", color: [0.25, 0.32, 0.58], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["blue_glazed_terracotta.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["blue_glazed_terracotta.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["blue_glazed_terracotta.png"], keep_alpha: false } },
    BlockType { id: 697, name: "brown_glazed_terracotta", color: [0.45, 0.32, 0.22], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["brown_glazed_terracotta.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["brown_glazed_terracotta.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["brown_glazed_terracotta.png"], keep_alpha: false } },
    BlockType { id: 698, name: "green_glazed_terracotta", color: [0.42, 0.50, 0.22], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["green_glazed_terracotta.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["green_glazed_terracotta.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["green_glazed_terracotta.png"], keep_alpha: false } },
    BlockType { id: 699, name: "red_glazed_terracotta", color: [0.68, 0.26, 0.20], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["red_glazed_terracotta.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["red_glazed_terracotta.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["red_glazed_terracotta.png"], keep_alpha: false } },
    BlockType { id: 700, name: "black_glazed_terracotta", color: [0.16, 0.12, 0.14], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["black_glazed_terracotta.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["black_glazed_terracotta.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["black_glazed_terracotta.png"], keep_alpha: false } },
    // W152: the 16 stained glass panes - same thin-pane dedicated
    // render as glass_pane (build_thin_pane samples the TileSpec).
    BlockType { id: 701, name: "white_stained_glass_pane", color: [0.92, 0.92, 0.94], transparent: true, flags: F_DEDICATED_RENDER, top: TileSpec { col: 2, row: 2, files: &["white_stained_glass.png"], keep_alpha: true }, bottom: TileSpec { col: 2, row: 2, files: &["white_stained_glass.png"], keep_alpha: true }, sides: TileSpec { col: 2, row: 2, files: &["white_stained_glass.png"], keep_alpha: true } },
    BlockType { id: 702, name: "orange_stained_glass_pane", color: [0.85, 0.50, 0.20], transparent: true, flags: F_DEDICATED_RENDER, top: TileSpec { col: 2, row: 2, files: &["orange_stained_glass.png"], keep_alpha: true }, bottom: TileSpec { col: 2, row: 2, files: &["orange_stained_glass.png"], keep_alpha: true }, sides: TileSpec { col: 2, row: 2, files: &["orange_stained_glass.png"], keep_alpha: true } },
    BlockType { id: 703, name: "magenta_stained_glass_pane", color: [0.70, 0.30, 0.85], transparent: true, flags: F_DEDICATED_RENDER, top: TileSpec { col: 2, row: 2, files: &["magenta_stained_glass.png"], keep_alpha: true }, bottom: TileSpec { col: 2, row: 2, files: &["magenta_stained_glass.png"], keep_alpha: true }, sides: TileSpec { col: 2, row: 2, files: &["magenta_stained_glass.png"], keep_alpha: true } },
    BlockType { id: 704, name: "light_blue_stained_glass_pane", color: [0.40, 0.60, 0.85], transparent: true, flags: F_DEDICATED_RENDER, top: TileSpec { col: 2, row: 2, files: &["light_blue_stained_glass.png"], keep_alpha: true }, bottom: TileSpec { col: 2, row: 2, files: &["light_blue_stained_glass.png"], keep_alpha: true }, sides: TileSpec { col: 2, row: 2, files: &["light_blue_stained_glass.png"], keep_alpha: true } },
    BlockType { id: 705, name: "yellow_stained_glass_pane", color: [0.88, 0.85, 0.25], transparent: true, flags: F_DEDICATED_RENDER, top: TileSpec { col: 2, row: 2, files: &["yellow_stained_glass.png"], keep_alpha: true }, bottom: TileSpec { col: 2, row: 2, files: &["yellow_stained_glass.png"], keep_alpha: true }, sides: TileSpec { col: 2, row: 2, files: &["yellow_stained_glass.png"], keep_alpha: true } },
    BlockType { id: 706, name: "lime_stained_glass_pane", color: [0.50, 0.78, 0.12], transparent: true, flags: F_DEDICATED_RENDER, top: TileSpec { col: 2, row: 2, files: &["lime_stained_glass.png"], keep_alpha: true }, bottom: TileSpec { col: 2, row: 2, files: &["lime_stained_glass.png"], keep_alpha: true }, sides: TileSpec { col: 2, row: 2, files: &["lime_stained_glass.png"], keep_alpha: true } },
    BlockType { id: 707, name: "pink_stained_glass_pane", color: [0.94, 0.50, 0.65], transparent: true, flags: F_DEDICATED_RENDER, top: TileSpec { col: 2, row: 2, files: &["pink_stained_glass.png"], keep_alpha: true }, bottom: TileSpec { col: 2, row: 2, files: &["pink_stained_glass.png"], keep_alpha: true }, sides: TileSpec { col: 2, row: 2, files: &["pink_stained_glass.png"], keep_alpha: true } },
    BlockType { id: 708, name: "gray_stained_glass_pane", color: [0.28, 0.31, 0.32], transparent: true, flags: F_DEDICATED_RENDER, top: TileSpec { col: 2, row: 2, files: &["gray_stained_glass.png"], keep_alpha: true }, bottom: TileSpec { col: 2, row: 2, files: &["gray_stained_glass.png"], keep_alpha: true }, sides: TileSpec { col: 2, row: 2, files: &["gray_stained_glass.png"], keep_alpha: true } },
    BlockType { id: 709, name: "light_gray_stained_glass_pane", color: [0.60, 0.60, 0.58], transparent: true, flags: F_DEDICATED_RENDER, top: TileSpec { col: 2, row: 2, files: &["light_gray_stained_glass.png"], keep_alpha: true }, bottom: TileSpec { col: 2, row: 2, files: &["light_gray_stained_glass.png"], keep_alpha: true }, sides: TileSpec { col: 2, row: 2, files: &["light_gray_stained_glass.png"], keep_alpha: true } },
    BlockType { id: 710, name: "cyan_stained_glass_pane", color: [0.30, 0.56, 0.60], transparent: true, flags: F_DEDICATED_RENDER, top: TileSpec { col: 2, row: 2, files: &["cyan_stained_glass.png"], keep_alpha: true }, bottom: TileSpec { col: 2, row: 2, files: &["cyan_stained_glass.png"], keep_alpha: true }, sides: TileSpec { col: 2, row: 2, files: &["cyan_stained_glass.png"], keep_alpha: true } },
    BlockType { id: 711, name: "purple_stained_glass_pane", color: [0.50, 0.25, 0.70], transparent: true, flags: F_DEDICATED_RENDER, top: TileSpec { col: 2, row: 2, files: &["purple_stained_glass.png"], keep_alpha: true }, bottom: TileSpec { col: 2, row: 2, files: &["purple_stained_glass.png"], keep_alpha: true }, sides: TileSpec { col: 2, row: 2, files: &["purple_stained_glass.png"], keep_alpha: true } },
    BlockType { id: 712, name: "blue_stained_glass_pane", color: [0.28, 0.30, 0.62], transparent: true, flags: F_DEDICATED_RENDER, top: TileSpec { col: 2, row: 2, files: &["blue_stained_glass.png"], keep_alpha: true }, bottom: TileSpec { col: 2, row: 2, files: &["blue_stained_glass.png"], keep_alpha: true }, sides: TileSpec { col: 2, row: 2, files: &["blue_stained_glass.png"], keep_alpha: true } },
    BlockType { id: 713, name: "brown_stained_glass_pane", color: [0.40, 0.30, 0.18], transparent: true, flags: F_DEDICATED_RENDER, top: TileSpec { col: 2, row: 2, files: &["brown_stained_glass.png"], keep_alpha: true }, bottom: TileSpec { col: 2, row: 2, files: &["brown_stained_glass.png"], keep_alpha: true }, sides: TileSpec { col: 2, row: 2, files: &["brown_stained_glass.png"], keep_alpha: true } },
    BlockType { id: 714, name: "green_stained_glass_pane", color: [0.34, 0.46, 0.15], transparent: true, flags: F_DEDICATED_RENDER, top: TileSpec { col: 2, row: 2, files: &["green_stained_glass.png"], keep_alpha: true }, bottom: TileSpec { col: 2, row: 2, files: &["green_stained_glass.png"], keep_alpha: true }, sides: TileSpec { col: 2, row: 2, files: &["green_stained_glass.png"], keep_alpha: true } },
    BlockType { id: 715, name: "red_stained_glass_pane", color: [0.74, 0.18, 0.18], transparent: true, flags: F_DEDICATED_RENDER, top: TileSpec { col: 2, row: 2, files: &["red_stained_glass.png"], keep_alpha: true }, bottom: TileSpec { col: 2, row: 2, files: &["red_stained_glass.png"], keep_alpha: true }, sides: TileSpec { col: 2, row: 2, files: &["red_stained_glass.png"], keep_alpha: true } },
    BlockType { id: 716, name: "black_stained_glass_pane", color: [0.10, 0.10, 0.12], transparent: true, flags: F_DEDICATED_RENDER, top: TileSpec { col: 2, row: 2, files: &["black_stained_glass.png"], keep_alpha: true }, bottom: TileSpec { col: 2, row: 2, files: &["black_stained_glass.png"], keep_alpha: true }, sides: TileSpec { col: 2, row: 2, files: &["black_stained_glass.png"], keep_alpha: true } },
    // W159: bookshelf was MISSING entirely (user report). Plank
    // top/bottom + the bookshelf texture on all four sides, like MC.
    BlockType { id: 718, name: "bookshelf", color: [0.62, 0.48, 0.30], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["oak_planks.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["oak_planks.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["bookshelf.png"], keep_alpha: false } },
    // W160: enchanting table was MISSING (user meant this, not the
    // crafting table). Full-cube for now with the correct MC faces;
    // the 0.75-tall slab shape + floating bobbing book entity are a
    // follow-up render (chest-style block entity).
    BlockType { id: 719, name: "enchanting_table", color: [0.30, 0.12, 0.18], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["enchanting_table_top.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["enchanting_table_bottom.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["enchanting_table_side.png"], keep_alpha: false } },
    // 18 misc utility decor.
    BlockType { id: 435, name: "hay_block", color: [0.78, 0.62, 0.18], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["hay_block_top.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["hay_block_top.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["hay_block_side.png"], keep_alpha: false } },
    BlockType { id: 436, name: "dried_kelp_block", color: [0.30, 0.40, 0.30], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["dried_kelp_top.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["dried_kelp_bottom.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["dried_kelp_side.png"], keep_alpha: false } },
    BlockType { id: 437, name: "bone_block", color: [0.92, 0.88, 0.78], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["bone_block_top.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["bone_block_top.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["bone_block_side.png"], keep_alpha: false } },
    BlockType { id: 438, name: "lapis_block", color: [0.18, 0.32, 0.62], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["lapis_block.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["lapis_block.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["lapis_block.png"], keep_alpha: false } },
    BlockType { id: 439, name: "emerald_block", color: [0.20, 0.85, 0.42], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["emerald_block.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["emerald_block.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["emerald_block.png"], keep_alpha: false } },
    BlockType { id: 440, name: "iron_block", color: [0.86, 0.86, 0.88], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["iron_block.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["iron_block.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["iron_block.png"], keep_alpha: false } },
    BlockType { id: 441, name: "redstone_block", color: [0.78, 0.10, 0.10], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["redstone_block.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["redstone_block.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["redstone_block.png"], keep_alpha: false } },
    BlockType { id: 442, name: "slime_block", color: [0.45, 0.78, 0.40], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["slime_block.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["slime_block.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["slime_block.png"], keep_alpha: true } },
    BlockType { id: 443, name: "honey_block", color: [0.95, 0.65, 0.18], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["honey_block_top.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["honey_block_bottom.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["honey_block_side.png"], keep_alpha: true } },
    BlockType { id: 444, name: "honeycomb_block", color: [0.92, 0.60, 0.18], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["honeycomb_block.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["honeycomb_block.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["honeycomb_block.png"], keep_alpha: false } },
    BlockType { id: 445, name: "sponge", color: [0.95, 0.92, 0.32], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["sponge.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["sponge.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["sponge.png"], keep_alpha: false } },
    BlockType { id: 446, name: "wet_sponge", color: [0.72, 0.78, 0.30], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["wet_sponge.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["wet_sponge.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["wet_sponge.png"], keep_alpha: false } },
    BlockType { id: 447, name: "melon", color: [0.55, 0.78, 0.22], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["melon_top.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["melon_top.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["melon_side.png"], keep_alpha: false } },
    BlockType { id: 448, name: "pumpkin", color: [0.85, 0.55, 0.18], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["pumpkin_top.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["pumpkin_top.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["pumpkin_side.png"], keep_alpha: false } },
    BlockType { id: 449, name: "carved_pumpkin", color: [0.85, 0.55, 0.18], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["carved_pumpkin_top.png", "pumpkin_top.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["pumpkin_top.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["carved_pumpkin.png"], keep_alpha: false } },
    BlockType { id: 450, name: "obsidian_pillar", color: [0.12, 0.05, 0.18], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["obsidian.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["obsidian.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["obsidian.png"], keep_alpha: false } },
    BlockType { id: 451, name: "amethyst_block", color: [0.55, 0.42, 0.78], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["amethyst_block.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["amethyst_block.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["amethyst_block.png"], keep_alpha: false } },
    BlockType { id: 452, name: "budding_amethyst", color: [0.55, 0.42, 0.78], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["budding_amethyst.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["budding_amethyst.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["budding_amethyst.png"], keep_alpha: false } },
    // ============================================================
    // W48.3 batch 3: 50 new blocks (IDs 453-502).
    // Redstone hardware + plants/saplings + flowers + crops.
    // ============================================================
    // Redstone hardware (15).
    BlockType { id: 453, name: "lever", color: [0.78, 0.60, 0.36], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["lever.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["lever.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["lever.png"], keep_alpha: true } },
    BlockType { id: 454, name: "oak_button", color: [0.78, 0.60, 0.36], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["oak_planks.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["oak_planks.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["oak_planks.png"], keep_alpha: true } },
    BlockType { id: 455, name: "stone_button", color: [0.50, 0.50, 0.55], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["stone.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["stone.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["stone.png"], keep_alpha: true } },
    BlockType { id: 456, name: "oak_pressure_plate", color: [0.78, 0.60, 0.36], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["oak_planks.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["oak_planks.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["oak_planks.png"], keep_alpha: true } },
    BlockType { id: 457, name: "stone_pressure_plate", color: [0.50, 0.50, 0.55], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["stone.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["stone.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["stone.png"], keep_alpha: true } },
    BlockType { id: 458, name: "light_weighted_pressure_plate", color: [0.95, 0.82, 0.25], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["gold_block.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["gold_block.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["gold_block.png"], keep_alpha: true } },
    BlockType { id: 459, name: "heavy_weighted_pressure_plate", color: [0.86, 0.86, 0.88], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["iron_block.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["iron_block.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["iron_block.png"], keep_alpha: true } },
    BlockType { id: 460, name: "repeater", color: [0.55, 0.42, 0.32], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["repeater.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["repeater.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["repeater.png"], keep_alpha: true } },
    BlockType { id: 461, name: "comparator", color: [0.55, 0.42, 0.32], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["comparator.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["comparator.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["comparator.png"], keep_alpha: true } },
    BlockType { id: 462, name: "observer", color: [0.32, 0.32, 0.34], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["observer_top.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["observer_top.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["observer_side.png"], keep_alpha: false } },
    BlockType { id: 463, name: "piston", color: [0.55, 0.42, 0.32], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["piston_top.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["piston_bottom.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["piston_side.png"], keep_alpha: false } },
    BlockType { id: 464, name: "sticky_piston", color: [0.55, 0.65, 0.32], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["piston_top_sticky.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["piston_bottom.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["piston_side.png"], keep_alpha: false } },
    BlockType { id: 465, name: "dispenser", color: [0.45, 0.45, 0.48], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["dispenser_top.png", "furnace_top.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["furnace_top.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["dispenser_front.png"], keep_alpha: false } },
    BlockType { id: 466, name: "dropper", color: [0.45, 0.45, 0.48], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["dropper_top.png", "furnace_top.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["furnace_top.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["dropper_front.png"], keep_alpha: false } },
    BlockType { id: 467, name: "hopper", color: [0.30, 0.30, 0.32], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["hopper_inside.png", "hopper_top.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["hopper_outside.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["hopper_outside.png"], keep_alpha: true } },
    // Saplings + plants (10).
    BlockType { id: 468, name: "oak_sapling", color: [0.45, 0.65, 0.20], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["oak_sapling.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["oak_sapling.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["oak_sapling.png"], keep_alpha: true } },
    BlockType { id: 469, name: "birch_sapling", color: [0.70, 0.85, 0.32], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["birch_sapling.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["birch_sapling.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["birch_sapling.png"], keep_alpha: true } },
    BlockType { id: 470, name: "spruce_sapling", color: [0.30, 0.45, 0.20], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["spruce_sapling.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["spruce_sapling.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["spruce_sapling.png"], keep_alpha: true } },
    BlockType { id: 471, name: "jungle_sapling", color: [0.42, 0.62, 0.18], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["jungle_sapling.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["jungle_sapling.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["jungle_sapling.png"], keep_alpha: true } },
    BlockType { id: 472, name: "acacia_sapling", color: [0.62, 0.45, 0.18], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["acacia_sapling.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["acacia_sapling.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["acacia_sapling.png"], keep_alpha: true } },
    BlockType { id: 473, name: "dark_oak_sapling", color: [0.35, 0.25, 0.12], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["dark_oak_sapling.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["dark_oak_sapling.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["dark_oak_sapling.png"], keep_alpha: true } },
    BlockType { id: 474, name: "mangrove_propagule", color: [0.55, 0.30, 0.25], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["mangrove_propagule.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["mangrove_propagule.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["mangrove_propagule.png"], keep_alpha: true } },
    BlockType { id: 475, name: "cherry_sapling", color: [0.92, 0.65, 0.78], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["cherry_sapling.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["cherry_sapling.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["cherry_sapling.png"], keep_alpha: true } },
    BlockType { id: 476, name: "azalea", color: [0.45, 0.65, 0.30], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["azalea_top.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["azalea_top.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["azalea_side.png"], keep_alpha: true } },
    BlockType { id: 477, name: "flowering_azalea", color: [0.82, 0.45, 0.65], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["flowering_azalea_top.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["flowering_azalea_top.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["flowering_azalea_side.png"], keep_alpha: true } },
    // Flowers (15).
    BlockType { id: 478, name: "allium", color: [0.78, 0.45, 0.92], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["allium.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["allium.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["allium.png"], keep_alpha: true } },
    BlockType { id: 479, name: "blue_orchid", color: [0.18, 0.62, 0.92], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["blue_orchid.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["blue_orchid.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["blue_orchid.png"], keep_alpha: true } },
    BlockType { id: 480, name: "dandelion", color: [0.95, 0.92, 0.32], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["dandelion.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["dandelion.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["dandelion.png"], keep_alpha: true } },
    BlockType { id: 481, name: "poppy", color: [0.92, 0.18, 0.18], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["poppy.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["poppy.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["poppy.png"], keep_alpha: true } },
    BlockType { id: 482, name: "oxeye_daisy", color: [0.95, 0.95, 0.95], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["oxeye_daisy.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["oxeye_daisy.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["oxeye_daisy.png"], keep_alpha: true } },
    BlockType { id: 483, name: "cornflower", color: [0.32, 0.55, 0.95], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["cornflower.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["cornflower.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["cornflower.png"], keep_alpha: true } },
    BlockType { id: 484, name: "lily_of_the_valley", color: [0.95, 0.95, 0.92], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["lily_of_the_valley.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["lily_of_the_valley.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["lily_of_the_valley.png"], keep_alpha: true } },
    BlockType { id: 485, name: "sunflower", color: [0.95, 0.85, 0.18], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["sunflower_top.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["sunflower_bottom.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["sunflower_front.png"], keep_alpha: true } },
    BlockType { id: 486, name: "peony", color: [0.85, 0.55, 0.78], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["peony_top.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["peony_bottom.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["peony_top.png"], keep_alpha: true } },
    BlockType { id: 487, name: "rose_bush", color: [0.92, 0.18, 0.18], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["rose_bush_top.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["rose_bush_bottom.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["rose_bush_top.png"], keep_alpha: true } },
    BlockType { id: 488, name: "lilac", color: [0.78, 0.55, 0.85], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["lilac_top.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["lilac_bottom.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["lilac_top.png"], keep_alpha: true } },
    BlockType { id: 489, name: "wither_rose", color: [0.18, 0.18, 0.18], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["wither_rose.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["wither_rose.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["wither_rose.png"], keep_alpha: true } },
    BlockType { id: 490, name: "torchflower", color: [0.95, 0.55, 0.18], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["torchflower.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["torchflower.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["torchflower.png"], keep_alpha: true } },
    // W153: the pack never shipped pitcher_plant_top/_bottom - the
    // real BLOCK textures are the pitcher_crop_* stages (the full
    // bloom = stage 4); pitcher_plant.png is the ITEM icon and rides
    // an ITEM_NAME_MAP row. The phantom names left the icon AND the
    // world faces unresolved.
    BlockType { id: 491, name: "pitcher_plant", color: [0.55, 0.32, 0.78], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["pitcher_crop_top_stage_4.png", "pitcher_crop_top.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["pitcher_crop_bottom_stage_4.png", "pitcher_crop_bottom.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["pitcher_crop_top_stage_4.png", "pitcher_crop_top.png"], keep_alpha: true } },
    // W144: dead_bush is REPLACEABLE in MC - placing a block into
    // its cell overwrites it (same as short_grass / fern).
    BlockType { id: 492, name: "dead_bush", color: [0.55, 0.42, 0.20], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE | F_REPLACEABLE, top: TileSpec { col: 0, row: 0, files: &["dead_bush.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["dead_bush.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["dead_bush.png"], keep_alpha: true } },
    // Crops (10).
    BlockType { id: 493, name: "wheat", color: [0.85, 0.78, 0.32], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["wheat_stage7.png", "wheat.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["wheat_stage7.png", "wheat.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["wheat_stage7.png", "wheat.png"], keep_alpha: true } },
    BlockType { id: 494, name: "carrots", color: [0.95, 0.55, 0.18], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["carrots_stage3.png", "carrots.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["carrots_stage3.png", "carrots.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["carrots_stage3.png", "carrots.png"], keep_alpha: true } },
    BlockType { id: 495, name: "potatoes", color: [0.78, 0.62, 0.42], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["potatoes_stage3.png", "potatoes.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["potatoes_stage3.png", "potatoes.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["potatoes_stage3.png", "potatoes.png"], keep_alpha: true } },
    BlockType { id: 496, name: "beetroots", color: [0.62, 0.18, 0.32], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["beetroots_stage3.png", "beetroots.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["beetroots_stage3.png", "beetroots.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["beetroots_stage3.png", "beetroots.png"], keep_alpha: true } },
    BlockType { id: 497, name: "melon_stem", color: [0.45, 0.65, 0.20], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["melon_stem.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["melon_stem.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["melon_stem.png"], keep_alpha: true } },
    BlockType { id: 498, name: "pumpkin_stem", color: [0.45, 0.65, 0.20], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["pumpkin_stem.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["pumpkin_stem.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["pumpkin_stem.png"], keep_alpha: true } },
    BlockType { id: 499, name: "sugar_cane", color: [0.55, 0.85, 0.42], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["sugar_cane.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["sugar_cane.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["sugar_cane.png"], keep_alpha: true } },
    BlockType { id: 500, name: "bamboo", color: [0.62, 0.82, 0.32], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["bamboo_stalk.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["bamboo_stalk.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["bamboo_stalk.png"], keep_alpha: true } },
    BlockType { id: 501, name: "cocoa", color: [0.65, 0.32, 0.18], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["cocoa_stage2.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["cocoa_stage2.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["cocoa_stage2.png"], keep_alpha: true } },
    BlockType { id: 502, name: "nether_wart", color: [0.65, 0.18, 0.18], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["nether_wart_stage2.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["nether_wart_stage2.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["nether_wart_stage2.png"], keep_alpha: true } },
    // ============================================================
    // W48.4 batch 4: 50 new blocks (IDs 503-552).
    // Slabs + stairs + walls + fences + trapdoors variants.
    // ============================================================
    // Slabs (15).
    BlockType { id: 503, name: "birch_slab", color: [0.90, 0.84, 0.60], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["birch_planks.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["birch_planks.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["birch_planks.png"], keep_alpha: true } },
    BlockType { id: 504, name: "spruce_slab", color: [0.50, 0.32, 0.18], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["spruce_planks.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["spruce_planks.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["spruce_planks.png"], keep_alpha: true } },
    BlockType { id: 505, name: "jungle_slab", color: [0.78, 0.55, 0.32], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["jungle_planks.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["jungle_planks.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["jungle_planks.png"], keep_alpha: true } },
    BlockType { id: 506, name: "acacia_slab", color: [0.78, 0.42, 0.18], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["acacia_planks.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["acacia_planks.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["acacia_planks.png"], keep_alpha: true } },
    BlockType { id: 507, name: "dark_oak_slab", color: [0.32, 0.22, 0.12], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["dark_oak_planks.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["dark_oak_planks.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["dark_oak_planks.png"], keep_alpha: true } },
    BlockType { id: 508, name: "mangrove_slab", color: [0.55, 0.30, 0.25], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["mangrove_planks.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["mangrove_planks.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["mangrove_planks.png"], keep_alpha: true } },
    BlockType { id: 509, name: "cherry_slab", color: [0.92, 0.65, 0.78], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["cherry_planks.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["cherry_planks.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["cherry_planks.png"], keep_alpha: true } },
    BlockType { id: 510, name: "smooth_stone_slab", color: [0.58, 0.58, 0.62], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["smooth_stone.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["smooth_stone.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["smooth_stone_slab_side.png"], keep_alpha: true } },
    BlockType { id: 511, name: "sandstone_slab", color: [0.93, 0.87, 0.63], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["sandstone_top.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["sandstone_bottom.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["sandstone.png"], keep_alpha: true } },
    BlockType { id: 512, name: "red_sandstone_slab", color: [0.78, 0.42, 0.18], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["red_sandstone_top.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["red_sandstone_bottom.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["red_sandstone.png"], keep_alpha: true } },
    BlockType { id: 513, name: "brick_slab", color: [0.68, 0.38, 0.30], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["bricks.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["bricks.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["bricks.png"], keep_alpha: true } },
    BlockType { id: 514, name: "stone_brick_slab", color: [0.50, 0.50, 0.55], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["stone_bricks.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["stone_bricks.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["stone_bricks.png"], keep_alpha: true } },
    BlockType { id: 515, name: "mossy_stone_brick_slab", color: [0.45, 0.52, 0.42], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["mossy_stone_bricks.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["mossy_stone_bricks.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["mossy_stone_bricks.png"], keep_alpha: true } },
    BlockType { id: 516, name: "nether_brick_slab", color: [0.30, 0.16, 0.18], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["nether_bricks.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["nether_bricks.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["nether_bricks.png"], keep_alpha: true } },
    BlockType { id: 517, name: "end_stone_brick_slab", color: [0.86, 0.86, 0.65], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["end_stone_bricks.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["end_stone_bricks.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["end_stone_bricks.png"], keep_alpha: true } },
    // Stairs (15).
    BlockType { id: 518, name: "birch_stairs", color: [0.90, 0.84, 0.60], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["birch_planks.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["birch_planks.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["birch_planks.png"], keep_alpha: true } },
    BlockType { id: 519, name: "spruce_stairs", color: [0.50, 0.32, 0.18], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["spruce_planks.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["spruce_planks.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["spruce_planks.png"], keep_alpha: true } },
    BlockType { id: 520, name: "jungle_stairs", color: [0.78, 0.55, 0.32], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["jungle_planks.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["jungle_planks.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["jungle_planks.png"], keep_alpha: true } },
    BlockType { id: 521, name: "acacia_stairs", color: [0.78, 0.42, 0.18], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["acacia_planks.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["acacia_planks.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["acacia_planks.png"], keep_alpha: true } },
    BlockType { id: 522, name: "dark_oak_stairs", color: [0.32, 0.22, 0.12], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["dark_oak_planks.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["dark_oak_planks.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["dark_oak_planks.png"], keep_alpha: true } },
    BlockType { id: 523, name: "mangrove_stairs", color: [0.55, 0.30, 0.25], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["mangrove_planks.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["mangrove_planks.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["mangrove_planks.png"], keep_alpha: true } },
    BlockType { id: 524, name: "cherry_stairs", color: [0.92, 0.65, 0.78], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["cherry_planks.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["cherry_planks.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["cherry_planks.png"], keep_alpha: true } },
    BlockType { id: 525, name: "sandstone_stairs", color: [0.93, 0.87, 0.63], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["sandstone_top.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["sandstone_bottom.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["sandstone.png"], keep_alpha: true } },
    BlockType { id: 526, name: "red_sandstone_stairs", color: [0.78, 0.42, 0.18], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["red_sandstone_top.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["red_sandstone_bottom.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["red_sandstone.png"], keep_alpha: true } },
    BlockType { id: 527, name: "brick_stairs", color: [0.68, 0.38, 0.30], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["bricks.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["bricks.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["bricks.png"], keep_alpha: true } },
    BlockType { id: 528, name: "stone_brick_stairs", color: [0.50, 0.50, 0.55], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["stone_bricks.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["stone_bricks.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["stone_bricks.png"], keep_alpha: true } },
    BlockType { id: 529, name: "mossy_stone_brick_stairs", color: [0.45, 0.52, 0.42], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["mossy_stone_bricks.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["mossy_stone_bricks.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["mossy_stone_bricks.png"], keep_alpha: true } },
    BlockType { id: 530, name: "nether_brick_stairs", color: [0.30, 0.16, 0.18], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["nether_bricks.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["nether_bricks.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["nether_bricks.png"], keep_alpha: true } },
    BlockType { id: 531, name: "granite_stairs", color: [0.68, 0.48, 0.40], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["granite.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["granite.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["granite.png"], keep_alpha: true } },
    BlockType { id: 532, name: "polished_granite_stairs", color: [0.71, 0.50, 0.42], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["polished_granite.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["polished_granite.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["polished_granite.png"], keep_alpha: true } },
    // Walls (10).
    BlockType { id: 533, name: "granite_wall", color: [0.68, 0.48, 0.40], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["granite.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["granite.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["granite.png"], keep_alpha: true } },
    BlockType { id: 534, name: "diorite_wall", color: [0.82, 0.82, 0.82], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["diorite.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["diorite.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["diorite.png"], keep_alpha: true } },
    BlockType { id: 535, name: "andesite_wall", color: [0.58, 0.58, 0.58], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["andesite.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["andesite.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["andesite.png"], keep_alpha: true } },
    BlockType { id: 536, name: "sandstone_wall", color: [0.93, 0.87, 0.63], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["sandstone_top.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["sandstone_bottom.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["sandstone.png"], keep_alpha: true } },
    BlockType { id: 537, name: "red_sandstone_wall", color: [0.78, 0.42, 0.18], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["red_sandstone_top.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["red_sandstone_bottom.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["red_sandstone.png"], keep_alpha: true } },
    BlockType { id: 538, name: "brick_wall", color: [0.68, 0.38, 0.30], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["bricks.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["bricks.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["bricks.png"], keep_alpha: true } },
    BlockType { id: 539, name: "stone_brick_wall", color: [0.50, 0.50, 0.55], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["stone_bricks.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["stone_bricks.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["stone_bricks.png"], keep_alpha: true } },
    BlockType { id: 540, name: "mossy_stone_brick_wall", color: [0.45, 0.52, 0.42], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["mossy_stone_bricks.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["mossy_stone_bricks.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["mossy_stone_bricks.png"], keep_alpha: true } },
    BlockType { id: 541, name: "nether_brick_wall", color: [0.30, 0.16, 0.18], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["nether_bricks.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["nether_bricks.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["nether_bricks.png"], keep_alpha: true } },
    BlockType { id: 542, name: "end_stone_brick_wall", color: [0.86, 0.86, 0.65], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["end_stone_bricks.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["end_stone_bricks.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["end_stone_bricks.png"], keep_alpha: true } },
    // Fences (5).
    BlockType { id: 543, name: "birch_fence", color: [0.90, 0.84, 0.60], transparent: true, flags: F_TRANSPARENT_RENDER | F_DEDICATED_RENDER, top: TileSpec { col: 0, row: 0, files: &["birch_planks.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["birch_planks.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["birch_planks.png"], keep_alpha: true } },
    BlockType { id: 544, name: "spruce_fence", color: [0.50, 0.32, 0.18], transparent: true, flags: F_TRANSPARENT_RENDER | F_DEDICATED_RENDER, top: TileSpec { col: 0, row: 0, files: &["spruce_planks.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["spruce_planks.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["spruce_planks.png"], keep_alpha: true } },
    BlockType { id: 545, name: "jungle_fence", color: [0.78, 0.55, 0.32], transparent: true, flags: F_TRANSPARENT_RENDER | F_DEDICATED_RENDER, top: TileSpec { col: 0, row: 0, files: &["jungle_planks.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["jungle_planks.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["jungle_planks.png"], keep_alpha: true } },
    BlockType { id: 546, name: "acacia_fence", color: [0.78, 0.42, 0.18], transparent: true, flags: F_TRANSPARENT_RENDER | F_DEDICATED_RENDER, top: TileSpec { col: 0, row: 0, files: &["acacia_planks.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["acacia_planks.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["acacia_planks.png"], keep_alpha: true } },
    BlockType { id: 547, name: "dark_oak_fence", color: [0.32, 0.22, 0.12], transparent: true, flags: F_TRANSPARENT_RENDER | F_DEDICATED_RENDER, top: TileSpec { col: 0, row: 0, files: &["dark_oak_planks.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["dark_oak_planks.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["dark_oak_planks.png"], keep_alpha: true } },
    // W167: fence gates (were missing). Plank-textured; stateful open
    // /closed + facing via the door open-state set + emit_fence_gate.
    BlockType { id: 726, name: "oak_fence_gate", color: [0.78, 0.60, 0.36], transparent: true, flags: F_TRANSPARENT_RENDER | F_DEDICATED_RENDER, top: TileSpec { col: 0, row: 0, files: &["oak_planks.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["oak_planks.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["oak_planks.png"], keep_alpha: true } },
    BlockType { id: 727, name: "birch_fence_gate", color: [0.90, 0.84, 0.60], transparent: true, flags: F_TRANSPARENT_RENDER | F_DEDICATED_RENDER, top: TileSpec { col: 0, row: 0, files: &["birch_planks.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["birch_planks.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["birch_planks.png"], keep_alpha: true } },
    BlockType { id: 728, name: "spruce_fence_gate", color: [0.50, 0.32, 0.18], transparent: true, flags: F_TRANSPARENT_RENDER | F_DEDICATED_RENDER, top: TileSpec { col: 0, row: 0, files: &["spruce_planks.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["spruce_planks.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["spruce_planks.png"], keep_alpha: true } },
    BlockType { id: 729, name: "jungle_fence_gate", color: [0.78, 0.55, 0.32], transparent: true, flags: F_TRANSPARENT_RENDER | F_DEDICATED_RENDER, top: TileSpec { col: 0, row: 0, files: &["jungle_planks.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["jungle_planks.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["jungle_planks.png"], keep_alpha: true } },
    BlockType { id: 730, name: "acacia_fence_gate", color: [0.78, 0.42, 0.18], transparent: true, flags: F_TRANSPARENT_RENDER | F_DEDICATED_RENDER, top: TileSpec { col: 0, row: 0, files: &["acacia_planks.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["acacia_planks.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["acacia_planks.png"], keep_alpha: true } },
    BlockType { id: 731, name: "dark_oak_fence_gate", color: [0.32, 0.22, 0.12], transparent: true, flags: F_TRANSPARENT_RENDER | F_DEDICATED_RENDER, top: TileSpec { col: 0, row: 0, files: &["dark_oak_planks.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["dark_oak_planks.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["dark_oak_planks.png"], keep_alpha: true } },
    // W168: candles (were missing). Thin pillar model, passable, lit
    // (light 3 via the emission table). 1-4 count + flame geometry are
    // follow-ups. `candle` = the plain wax candle, + 16 dyed colours.
    BlockType { id: 732, name: "candle", color: [0.90, 0.85, 0.70], transparent: true, flags: F_TRANSPARENT_RENDER | F_DEDICATED_RENDER | F_RAY_PASS | F_AABB_PASS, top: TileSpec { col: 0, row: 0, files: &["candle.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["candle.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["candle.png"], keep_alpha: true } },
    BlockType { id: 733, name: "white_candle", color: [0.95, 0.95, 0.95], transparent: true, flags: F_TRANSPARENT_RENDER | F_DEDICATED_RENDER | F_RAY_PASS | F_AABB_PASS, top: TileSpec { col: 0, row: 0, files: &["white_candle.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["white_candle.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["white_candle.png"], keep_alpha: true } },
    BlockType { id: 734, name: "orange_candle", color: [0.85, 0.50, 0.20], transparent: true, flags: F_TRANSPARENT_RENDER | F_DEDICATED_RENDER | F_RAY_PASS | F_AABB_PASS, top: TileSpec { col: 0, row: 0, files: &["orange_candle.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["orange_candle.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["orange_candle.png"], keep_alpha: true } },
    BlockType { id: 735, name: "magenta_candle", color: [0.75, 0.30, 0.70], transparent: true, flags: F_TRANSPARENT_RENDER | F_DEDICATED_RENDER | F_RAY_PASS | F_AABB_PASS, top: TileSpec { col: 0, row: 0, files: &["magenta_candle.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["magenta_candle.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["magenta_candle.png"], keep_alpha: true } },
    BlockType { id: 736, name: "light_blue_candle", color: [0.40, 0.65, 0.85], transparent: true, flags: F_TRANSPARENT_RENDER | F_DEDICATED_RENDER | F_RAY_PASS | F_AABB_PASS, top: TileSpec { col: 0, row: 0, files: &["light_blue_candle.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["light_blue_candle.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["light_blue_candle.png"], keep_alpha: true } },
    BlockType { id: 737, name: "yellow_candle", color: [0.90, 0.85, 0.25], transparent: true, flags: F_TRANSPARENT_RENDER | F_DEDICATED_RENDER | F_RAY_PASS | F_AABB_PASS, top: TileSpec { col: 0, row: 0, files: &["yellow_candle.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["yellow_candle.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["yellow_candle.png"], keep_alpha: true } },
    BlockType { id: 738, name: "lime_candle", color: [0.50, 0.80, 0.20], transparent: true, flags: F_TRANSPARENT_RENDER | F_DEDICATED_RENDER | F_RAY_PASS | F_AABB_PASS, top: TileSpec { col: 0, row: 0, files: &["lime_candle.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["lime_candle.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["lime_candle.png"], keep_alpha: true } },
    BlockType { id: 739, name: "pink_candle", color: [0.90, 0.55, 0.70], transparent: true, flags: F_TRANSPARENT_RENDER | F_DEDICATED_RENDER | F_RAY_PASS | F_AABB_PASS, top: TileSpec { col: 0, row: 0, files: &["pink_candle.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["pink_candle.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["pink_candle.png"], keep_alpha: true } },
    BlockType { id: 740, name: "gray_candle", color: [0.35, 0.35, 0.35], transparent: true, flags: F_TRANSPARENT_RENDER | F_DEDICATED_RENDER | F_RAY_PASS | F_AABB_PASS, top: TileSpec { col: 0, row: 0, files: &["gray_candle.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["gray_candle.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["gray_candle.png"], keep_alpha: true } },
    BlockType { id: 741, name: "light_gray_candle", color: [0.62, 0.62, 0.60], transparent: true, flags: F_TRANSPARENT_RENDER | F_DEDICATED_RENDER | F_RAY_PASS | F_AABB_PASS, top: TileSpec { col: 0, row: 0, files: &["light_gray_candle.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["light_gray_candle.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["light_gray_candle.png"], keep_alpha: true } },
    BlockType { id: 742, name: "cyan_candle", color: [0.20, 0.55, 0.60], transparent: true, flags: F_TRANSPARENT_RENDER | F_DEDICATED_RENDER | F_RAY_PASS | F_AABB_PASS, top: TileSpec { col: 0, row: 0, files: &["cyan_candle.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["cyan_candle.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["cyan_candle.png"], keep_alpha: true } },
    BlockType { id: 743, name: "purple_candle", color: [0.50, 0.25, 0.70], transparent: true, flags: F_TRANSPARENT_RENDER | F_DEDICATED_RENDER | F_RAY_PASS | F_AABB_PASS, top: TileSpec { col: 0, row: 0, files: &["purple_candle.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["purple_candle.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["purple_candle.png"], keep_alpha: true } },
    BlockType { id: 744, name: "blue_candle", color: [0.20, 0.30, 0.70], transparent: true, flags: F_TRANSPARENT_RENDER | F_DEDICATED_RENDER | F_RAY_PASS | F_AABB_PASS, top: TileSpec { col: 0, row: 0, files: &["blue_candle.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["blue_candle.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["blue_candle.png"], keep_alpha: true } },
    BlockType { id: 745, name: "brown_candle", color: [0.45, 0.30, 0.18], transparent: true, flags: F_TRANSPARENT_RENDER | F_DEDICATED_RENDER | F_RAY_PASS | F_AABB_PASS, top: TileSpec { col: 0, row: 0, files: &["brown_candle.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["brown_candle.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["brown_candle.png"], keep_alpha: true } },
    BlockType { id: 746, name: "green_candle", color: [0.35, 0.50, 0.18], transparent: true, flags: F_TRANSPARENT_RENDER | F_DEDICATED_RENDER | F_RAY_PASS | F_AABB_PASS, top: TileSpec { col: 0, row: 0, files: &["green_candle.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["green_candle.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["green_candle.png"], keep_alpha: true } },
    BlockType { id: 747, name: "red_candle", color: [0.70, 0.20, 0.18], transparent: true, flags: F_TRANSPARENT_RENDER | F_DEDICATED_RENDER | F_RAY_PASS | F_AABB_PASS, top: TileSpec { col: 0, row: 0, files: &["red_candle.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["red_candle.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["red_candle.png"], keep_alpha: true } },
    BlockType { id: 748, name: "black_candle", color: [0.12, 0.12, 0.14], transparent: true, flags: F_TRANSPARENT_RENDER | F_DEDICATED_RENDER | F_RAY_PASS | F_AABB_PASS, top: TileSpec { col: 0, row: 0, files: &["black_candle.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["black_candle.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["black_candle.png"], keep_alpha: true } },
    // Trapdoors (5).
    BlockType { id: 548, name: "oak_trapdoor", color: [0.78, 0.60, 0.36], transparent: true, flags: F_TRANSPARENT_RENDER | F_DEDICATED_RENDER | F_AABB_PASS, top: TileSpec { col: 0, row: 0, files: &["oak_trapdoor.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["oak_trapdoor.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["oak_trapdoor.png"], keep_alpha: true } },
    BlockType { id: 549, name: "iron_trapdoor", color: [0.80, 0.80, 0.85], transparent: true, flags: F_TRANSPARENT_RENDER | F_DEDICATED_RENDER | F_AABB_PASS, top: TileSpec { col: 0, row: 0, files: &["iron_trapdoor.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["iron_trapdoor.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["iron_trapdoor.png"], keep_alpha: true } },
    BlockType { id: 550, name: "birch_trapdoor", color: [0.90, 0.84, 0.60], transparent: true, flags: F_TRANSPARENT_RENDER | F_DEDICATED_RENDER | F_AABB_PASS, top: TileSpec { col: 0, row: 0, files: &["birch_trapdoor.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["birch_trapdoor.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["birch_trapdoor.png"], keep_alpha: true } },
    BlockType { id: 551, name: "spruce_trapdoor", color: [0.50, 0.32, 0.18], transparent: true, flags: F_TRANSPARENT_RENDER | F_DEDICATED_RENDER | F_AABB_PASS, top: TileSpec { col: 0, row: 0, files: &["spruce_trapdoor.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["spruce_trapdoor.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["spruce_trapdoor.png"], keep_alpha: true } },
    BlockType { id: 552, name: "jungle_trapdoor", color: [0.78, 0.55, 0.32], transparent: true, flags: F_TRANSPARENT_RENDER | F_DEDICATED_RENDER | F_AABB_PASS, top: TileSpec { col: 0, row: 0, files: &["jungle_trapdoor.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["jungle_trapdoor.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["jungle_trapdoor.png"], keep_alpha: true } },
    // W166: the remaining wood trapdoors (were missing - acacia /
    // dark_oak / mangrove / cherry). Share the W129 emit_trapdoor
    // render + the door open-state toggle via is_trapdoor_block.
    BlockType { id: 722, name: "acacia_trapdoor", color: [0.66, 0.35, 0.18], transparent: true, flags: F_TRANSPARENT_RENDER | F_DEDICATED_RENDER | F_AABB_PASS, top: TileSpec { col: 0, row: 0, files: &["acacia_trapdoor.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["acacia_trapdoor.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["acacia_trapdoor.png"], keep_alpha: true } },
    BlockType { id: 723, name: "dark_oak_trapdoor", color: [0.30, 0.20, 0.10], transparent: true, flags: F_TRANSPARENT_RENDER | F_DEDICATED_RENDER | F_AABB_PASS, top: TileSpec { col: 0, row: 0, files: &["dark_oak_trapdoor.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["dark_oak_trapdoor.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["dark_oak_trapdoor.png"], keep_alpha: true } },
    BlockType { id: 724, name: "mangrove_trapdoor", color: [0.45, 0.16, 0.16], transparent: true, flags: F_TRANSPARENT_RENDER | F_DEDICATED_RENDER | F_AABB_PASS, top: TileSpec { col: 0, row: 0, files: &["mangrove_trapdoor.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["mangrove_trapdoor.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["mangrove_trapdoor.png"], keep_alpha: true } },
    BlockType { id: 725, name: "cherry_trapdoor", color: [0.84, 0.55, 0.58], transparent: true, flags: F_TRANSPARENT_RENDER | F_DEDICATED_RENDER | F_AABB_PASS, top: TileSpec { col: 0, row: 0, files: &["cherry_trapdoor.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["cherry_trapdoor.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["cherry_trapdoor.png"], keep_alpha: true } },
    // ============================================================
    // W48.5 batch 5: 50 new blocks (IDs 553-602).
    // Utility (crafting/anvil/smithing/etc.) + doors + beds + misc.
    // ============================================================
    // Utility blocks (15).
    BlockType { id: 553, name: "crafting_table", color: [0.55, 0.42, 0.28], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["crafting_table_top.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["oak_planks.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["crafting_table_side.png", "crafting_table_front.png"], keep_alpha: false } },
    BlockType { id: 554, name: "anvil", color: [0.30, 0.30, 0.32], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["anvil_top.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["anvil.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["anvil.png"], keep_alpha: false } },
    BlockType { id: 555, name: "chipped_anvil", color: [0.30, 0.30, 0.32], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["chipped_anvil_top.png", "anvil_top.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["anvil.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["anvil.png"], keep_alpha: false } },
    BlockType { id: 556, name: "damaged_anvil", color: [0.30, 0.30, 0.32], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["damaged_anvil_top.png", "anvil_top.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["anvil.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["anvil.png"], keep_alpha: false } },
    BlockType { id: 557, name: "smithing_table", color: [0.30, 0.30, 0.42], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["smithing_table_top.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["smithing_table_bottom.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["smithing_table_side.png", "smithing_table_front.png"], keep_alpha: false } },
    BlockType { id: 558, name: "fletching_table", color: [0.85, 0.78, 0.62], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["fletching_table_top.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["oak_planks.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["fletching_table_side.png"], keep_alpha: false } },
    BlockType { id: 559, name: "cartography_table", color: [0.55, 0.42, 0.28], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["cartography_table_top.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["dark_oak_planks.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["cartography_table_side1.png"], keep_alpha: false } },
    BlockType { id: 560, name: "lectern", color: [0.55, 0.42, 0.28], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["lectern_top.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["lectern_base.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["lectern_sides.png"], keep_alpha: false } },
    BlockType { id: 561, name: "smoker", color: [0.30, 0.30, 0.30], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["smoker_top.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["smoker_bottom.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["smoker_front.png"], keep_alpha: false } },
    BlockType { id: 562, name: "blast_furnace", color: [0.30, 0.30, 0.32], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["blast_furnace_top.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["blast_furnace_top.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["blast_furnace_front.png", "blast_furnace_side.png"], keep_alpha: false } },
    BlockType { id: 563, name: "grindstone", color: [0.55, 0.55, 0.55], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["grindstone_round.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["grindstone_round.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["grindstone_side.png"], keep_alpha: false } },
    BlockType { id: 564, name: "stonecutter", color: [0.50, 0.50, 0.55], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["stonecutter_top.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["stonecutter_bottom.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["stonecutter_side.png"], keep_alpha: false } },
    BlockType { id: 565, name: "loom", color: [0.78, 0.62, 0.28], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["loom_top.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["loom_bottom.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["loom_side.png", "loom_front.png"], keep_alpha: false } },
    BlockType { id: 566, name: "brewing_stand", color: [0.55, 0.45, 0.42], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["brewing_stand.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["brewing_stand_base.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["brewing_stand.png"], keep_alpha: true } },
    BlockType { id: 567, name: "beacon", color: [0.85, 1.00, 1.00], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["beacon.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["obsidian.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["beacon.png"], keep_alpha: true } },
    // Wooden doors (8).
    BlockType { id: 568, name: "birch_door", color: [0.90, 0.84, 0.60], transparent: true, flags: F_TRANSPARENT_RENDER | F_DEDICATED_RENDER, top: TileSpec { col: 0, row: 0, files: &["birch_door_top.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["birch_door_bottom.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["birch_door_bottom.png"], keep_alpha: true } },
    BlockType { id: 569, name: "spruce_door", color: [0.50, 0.32, 0.18], transparent: true, flags: F_TRANSPARENT_RENDER | F_DEDICATED_RENDER, top: TileSpec { col: 0, row: 0, files: &["spruce_door_top.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["spruce_door_bottom.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["spruce_door_bottom.png"], keep_alpha: true } },
    BlockType { id: 570, name: "jungle_door", color: [0.78, 0.55, 0.32], transparent: true, flags: F_TRANSPARENT_RENDER | F_DEDICATED_RENDER, top: TileSpec { col: 0, row: 0, files: &["jungle_door_top.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["jungle_door_bottom.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["jungle_door_bottom.png"], keep_alpha: true } },
    BlockType { id: 571, name: "acacia_door", color: [0.78, 0.42, 0.18], transparent: true, flags: F_TRANSPARENT_RENDER | F_DEDICATED_RENDER, top: TileSpec { col: 0, row: 0, files: &["acacia_door_top.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["acacia_door_bottom.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["acacia_door_bottom.png"], keep_alpha: true } },
    BlockType { id: 572, name: "dark_oak_door", color: [0.32, 0.22, 0.12], transparent: true, flags: F_TRANSPARENT_RENDER | F_DEDICATED_RENDER, top: TileSpec { col: 0, row: 0, files: &["dark_oak_door_top.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["dark_oak_door_bottom.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["dark_oak_door_bottom.png"], keep_alpha: true } },
    BlockType { id: 573, name: "mangrove_door", color: [0.55, 0.30, 0.25], transparent: true, flags: F_TRANSPARENT_RENDER | F_DEDICATED_RENDER, top: TileSpec { col: 0, row: 0, files: &["mangrove_door_top.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["mangrove_door_bottom.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["mangrove_door_bottom.png"], keep_alpha: true } },
    BlockType { id: 574, name: "cherry_door", color: [0.92, 0.65, 0.78], transparent: true, flags: F_TRANSPARENT_RENDER | F_DEDICATED_RENDER, top: TileSpec { col: 0, row: 0, files: &["cherry_door_top.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["cherry_door_bottom.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["cherry_door_bottom.png"], keep_alpha: true } },
    BlockType { id: 575, name: "warped_door", color: [0.18, 0.55, 0.55], transparent: true, flags: F_TRANSPARENT_RENDER | F_DEDICATED_RENDER, top: TileSpec { col: 0, row: 0, files: &["warped_door_top.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["warped_door_bottom.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["warped_door_bottom.png"], keep_alpha: true } },
    // Beds (16).
    BlockType { id: 576, name: "white_bed", color: [0.95, 0.95, 0.95], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["white_wool.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["white_wool.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["white_wool.png"], keep_alpha: false } },
    BlockType { id: 577, name: "orange_bed", color: [0.88, 0.52, 0.15], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["orange_wool.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["orange_wool.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["orange_wool.png"], keep_alpha: false } },
    BlockType { id: 578, name: "magenta_bed", color: [0.78, 0.28, 0.72], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["magenta_wool.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["magenta_wool.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["magenta_wool.png"], keep_alpha: false } },
    BlockType { id: 579, name: "light_blue_bed", color: [0.42, 0.68, 0.90], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["light_blue_wool.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["light_blue_wool.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["light_blue_wool.png"], keep_alpha: false } },
    BlockType { id: 580, name: "yellow_bed", color: [0.92, 0.82, 0.16], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["yellow_wool.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["yellow_wool.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["yellow_wool.png"], keep_alpha: false } },
    BlockType { id: 581, name: "lime_bed", color: [0.52, 0.82, 0.22], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["lime_wool.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["lime_wool.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["lime_wool.png"], keep_alpha: false } },
    BlockType { id: 582, name: "pink_bed", color: [0.95, 0.62, 0.72], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["pink_wool.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["pink_wool.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["pink_wool.png"], keep_alpha: false } },
    BlockType { id: 583, name: "gray_bed", color: [0.32, 0.32, 0.32], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["gray_wool.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["gray_wool.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["gray_wool.png"], keep_alpha: false } },
    BlockType { id: 584, name: "light_gray_bed", color: [0.62, 0.62, 0.62], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["light_gray_wool.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["light_gray_wool.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["light_gray_wool.png"], keep_alpha: false } },
    BlockType { id: 585, name: "cyan_bed", color: [0.18, 0.62, 0.65], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["cyan_wool.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["cyan_wool.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["cyan_wool.png"], keep_alpha: false } },
    BlockType { id: 586, name: "purple_bed", color: [0.55, 0.22, 0.78], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["purple_wool.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["purple_wool.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["purple_wool.png"], keep_alpha: false } },
    BlockType { id: 587, name: "blue_bed", color: [0.22, 0.32, 0.72], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["blue_wool.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["blue_wool.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["blue_wool.png"], keep_alpha: false } },
    BlockType { id: 588, name: "brown_bed", color: [0.45, 0.30, 0.18], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["brown_wool.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["brown_wool.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["brown_wool.png"], keep_alpha: false } },
    BlockType { id: 589, name: "green_bed", color: [0.28, 0.52, 0.20], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["green_wool.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["green_wool.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["green_wool.png"], keep_alpha: false } },
    BlockType { id: 590, name: "red_bed", color: [0.74, 0.18, 0.18], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["red_wool.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["red_wool.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["red_wool.png"], keep_alpha: false } },
    BlockType { id: 591, name: "black_bed", color: [0.10, 0.10, 0.12], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["black_wool.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["black_wool.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["black_wool.png"], keep_alpha: false } },
    // Misc (11).
    BlockType { id: 592, name: "jukebox", color: [0.55, 0.42, 0.28], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["jukebox_top.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["jukebox_side.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["jukebox_side.png"], keep_alpha: false } },
    BlockType { id: 593, name: "note_block", color: [0.55, 0.42, 0.28], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["note_block.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["note_block.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["note_block.png"], keep_alpha: false } },
    BlockType { id: 594, name: "scaffolding", color: [0.85, 0.65, 0.32], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["scaffolding_top.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["scaffolding_bottom.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["scaffolding_side.png"], keep_alpha: true } },
    BlockType { id: 595, name: "ladder", color: [0.55, 0.42, 0.28], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["ladder.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["ladder.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["ladder.png"], keep_alpha: true } },
    BlockType { id: 596, name: "end_portal_frame", color: [0.18, 0.32, 0.30], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["end_portal_frame_top.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["end_stone.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["end_portal_frame_side.png"], keep_alpha: false } },
    BlockType { id: 597, name: "dragon_egg", color: [0.18, 0.05, 0.18], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["dragon_egg.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["dragon_egg.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["dragon_egg.png"], keep_alpha: true } },
    BlockType { id: 598, name: "conduit", color: [0.78, 0.78, 0.62], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["conduit.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["conduit.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["conduit.png"], keep_alpha: true } },
    BlockType { id: 599, name: "target", color: [0.85, 0.78, 0.55], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["target_top.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["target_top.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["target_side.png"], keep_alpha: false } },
    BlockType { id: 600, name: "bee_nest", color: [0.78, 0.62, 0.18], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["bee_nest_top.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["bee_nest_bottom.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["bee_nest_side.png", "bee_nest_front.png"], keep_alpha: false } },
    BlockType { id: 601, name: "beehive", color: [0.78, 0.62, 0.32], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["beehive_end.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["beehive_end.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["beehive_side.png", "beehive_front.png"], keep_alpha: false } },
    BlockType { id: 602, name: "lodestone", color: [0.45, 0.45, 0.48], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["lodestone_top.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["lodestone_top.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["lodestone_side.png"], keep_alpha: false } },
    // ============================================================
    // W48.6 batch 6: 30 more blocks (IDs 603-632) - hits 600+ target.
    // Waxed copper + nether nature + lush cave + coral + sculk.
    // ============================================================
    // Waxed copper (4).
    BlockType { id: 603, name: "waxed_copper_block", color: [0.78, 0.42, 0.25], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["waxed_copper_block.png", "copper_block.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["waxed_copper_block.png", "copper_block.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["waxed_copper_block.png", "copper_block.png"], keep_alpha: false } },
    BlockType { id: 604, name: "waxed_exposed_copper", color: [0.65, 0.48, 0.35], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["waxed_exposed_copper.png", "exposed_copper.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["waxed_exposed_copper.png", "exposed_copper.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["waxed_exposed_copper.png", "exposed_copper.png"], keep_alpha: false } },
    BlockType { id: 605, name: "waxed_weathered_copper", color: [0.45, 0.55, 0.40], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["waxed_weathered_copper.png", "weathered_copper.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["waxed_weathered_copper.png", "weathered_copper.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["waxed_weathered_copper.png", "weathered_copper.png"], keep_alpha: false } },
    BlockType { id: 606, name: "waxed_oxidized_copper", color: [0.32, 0.62, 0.52], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["waxed_oxidized_copper.png", "oxidized_copper.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["waxed_oxidized_copper.png", "oxidized_copper.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["waxed_oxidized_copper.png", "oxidized_copper.png"], keep_alpha: false } },
    // Nether nature (8).
    BlockType { id: 607, name: "crimson_nylium", color: [0.55, 0.18, 0.18], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["crimson_nylium.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["netherrack.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["crimson_nylium_side.png"], keep_alpha: false } },
    BlockType { id: 608, name: "warped_nylium", color: [0.18, 0.55, 0.45], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["warped_nylium.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["netherrack.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["warped_nylium_side.png"], keep_alpha: false } },
    BlockType { id: 609, name: "crimson_fungus", color: [0.78, 0.18, 0.18], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["crimson_fungus.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["crimson_fungus.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["crimson_fungus.png"], keep_alpha: true } },
    BlockType { id: 610, name: "warped_fungus", color: [0.55, 0.75, 0.30], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["warped_fungus.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["warped_fungus.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["warped_fungus.png"], keep_alpha: true } },
    BlockType { id: 611, name: "weeping_vines", color: [0.55, 0.10, 0.10], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["weeping_vines_plant.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["weeping_vines_plant.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["weeping_vines_plant.png"], keep_alpha: true } },
    BlockType { id: 612, name: "twisting_vines", color: [0.32, 0.78, 0.65], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["twisting_vines_plant.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["twisting_vines_plant.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["twisting_vines_plant.png"], keep_alpha: true } },
    BlockType { id: 613, name: "nether_sprouts", color: [0.32, 0.78, 0.65], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["nether_sprouts.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["nether_sprouts.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["nether_sprouts.png"], keep_alpha: true } },
    BlockType { id: 614, name: "warped_wart_block", color: [0.18, 0.55, 0.55], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["warped_wart_block.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["warped_wart_block.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["warped_wart_block.png"], keep_alpha: false } },
    // Lush cave (6).
    BlockType { id: 615, name: "moss_block", color: [0.45, 0.62, 0.30], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["moss_block.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["moss_block.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["moss_block.png"], keep_alpha: false } },
    BlockType { id: 616, name: "moss_carpet", color: [0.45, 0.62, 0.30], transparent: true, flags: F_TRANSPARENT_RENDER, top: TileSpec { col: 0, row: 0, files: &["moss_block.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["moss_block.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["moss_block.png"], keep_alpha: true } },
    BlockType { id: 617, name: "big_dripleaf", color: [0.55, 0.78, 0.30], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["big_dripleaf_top.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["big_dripleaf_top.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["big_dripleaf_side.png"], keep_alpha: true } },
    BlockType { id: 618, name: "small_dripleaf", color: [0.55, 0.78, 0.30], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["small_dripleaf_top.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["small_dripleaf_top.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["small_dripleaf_side.png"], keep_alpha: true } },
    BlockType { id: 619, name: "hanging_roots", color: [0.55, 0.32, 0.18], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["hanging_roots.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["hanging_roots.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["hanging_roots.png"], keep_alpha: true } },
    BlockType { id: 620, name: "spore_blossom", color: [0.78, 0.32, 0.55], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["spore_blossom.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["spore_blossom_base.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["spore_blossom.png"], keep_alpha: true } },
    // Coral (6).
    BlockType { id: 621, name: "tube_coral_block", color: [0.18, 0.32, 0.85], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["tube_coral_block.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["tube_coral_block.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["tube_coral_block.png"], keep_alpha: false } },
    BlockType { id: 622, name: "brain_coral_block", color: [0.85, 0.42, 0.78], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["brain_coral_block.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["brain_coral_block.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["brain_coral_block.png"], keep_alpha: false } },
    BlockType { id: 623, name: "bubble_coral_block", color: [0.62, 0.22, 0.78], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["bubble_coral_block.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["bubble_coral_block.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["bubble_coral_block.png"], keep_alpha: false } },
    BlockType { id: 624, name: "fire_coral_block", color: [0.85, 0.18, 0.18], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["fire_coral_block.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["fire_coral_block.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["fire_coral_block.png"], keep_alpha: false } },
    BlockType { id: 625, name: "horn_coral_block", color: [0.95, 0.85, 0.18], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["horn_coral_block.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["horn_coral_block.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["horn_coral_block.png"], keep_alpha: false } },
    BlockType { id: 626, name: "dead_brain_coral_block", color: [0.55, 0.55, 0.55], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["dead_brain_coral_block.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["dead_brain_coral_block.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["dead_brain_coral_block.png"], keep_alpha: false } },
    // Sculk (6).
    BlockType { id: 627, name: "glow_lichen", color: [0.42, 0.78, 0.55], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["glow_lichen.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["glow_lichen.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["glow_lichen.png"], keep_alpha: true } },
    BlockType { id: 628, name: "sculk", color: [0.10, 0.12, 0.18], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["sculk.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["sculk.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["sculk.png"], keep_alpha: false } },
    BlockType { id: 629, name: "sculk_catalyst", color: [0.10, 0.12, 0.18], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["sculk_catalyst_top.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["sculk_catalyst_bottom.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["sculk_catalyst_side.png"], keep_alpha: false } },
    BlockType { id: 630, name: "sculk_shrieker", color: [0.10, 0.12, 0.18], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["sculk_shrieker_top.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["sculk_shrieker_bottom.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["sculk_shrieker_side.png"], keep_alpha: false } },
    BlockType { id: 631, name: "sculk_sensor", color: [0.10, 0.12, 0.18], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["sculk_sensor_top.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["sculk_sensor_bottom.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["sculk_sensor_side.png"], keep_alpha: false } },
    // W145: the side texture is calibrated_sculk_sensor_INPUT_side in
    // the MC pack ("_side.png" never existed - the icon + world sides
    // rendered missing).
    BlockType { id: 632, name: "calibrated_sculk_sensor", color: [0.10, 0.12, 0.18], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["calibrated_sculk_sensor_top.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["sculk_sensor_bottom.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["calibrated_sculk_sensor_input_side.png", "sculk_sensor_side.png"], keep_alpha: false } },
    // W64.2 - gravity blocks: gravel + red_sand fall like sand
    // (folded into world::falling::is_falling_block). Ids jump from
    // 632 to 644 because 633..=643 are already claimed by non-block
    // tool / weapon items (hoes, bow, arrow, ...) in the shared
    // block/item id space - block-form items use `item.id ==
    // block.id`, so a new placeable block must avoid those. The
    // HashMap-keyed BlockRegistry + find-based block_type() tolerate
    // the gap.
    BlockType { id: 644, name: "gravel", color: [0.55, 0.52, 0.50], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["gravel.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["gravel.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["gravel.png"], keep_alpha: false } },
    BlockType { id: 645, name: "red_sand", color: [0.78, 0.45, 0.20], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["red_sand.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["red_sand.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["red_sand.png"], keep_alpha: false } },
    // W65.4 - immature crop stages. Each mirrors its mature crop's
    // flags + TileSpec (same render path) but shows an early-stage
    // sprite; a random tick advances immature -> mature. World-only
    // (no item form): planting the mature crop item substitutes the
    // immature block, and harvesting an immature crop drops nothing.
    BlockType { id: 646, name: "wheat_immature", color: [0.55, 0.62, 0.28], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["wheat_stage3.png", "wheat.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["wheat_stage3.png", "wheat.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["wheat_stage3.png", "wheat.png"], keep_alpha: true } },
    BlockType { id: 647, name: "carrots_immature", color: [0.40, 0.60, 0.25], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["carrots_stage1.png", "carrots.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["carrots_stage1.png", "carrots.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["carrots_stage1.png", "carrots.png"], keep_alpha: true } },
    BlockType { id: 648, name: "potatoes_immature", color: [0.42, 0.58, 0.28], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["potatoes_stage1.png", "potatoes.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["potatoes_stage1.png", "potatoes.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["potatoes_stage1.png", "potatoes.png"], keep_alpha: true } },
    BlockType { id: 649, name: "beetroots_immature", color: [0.45, 0.55, 0.25], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE, top: TileSpec { col: 0, row: 0, files: &["beetroots_stage1.png", "beetroots.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["beetroots_stage1.png", "beetroots.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["beetroots_stage1.png", "beetroots.png"], keep_alpha: true } },
    // W67.1 - unlit redstone lamp (the placeable / powered-off state).
    // The redstone solver swaps it to lit_redstone_lamp (65) when a
    // neighbouring wire / source carries power, and back when it goes
    // dark.
    BlockType { id: 650, name: "redstone_lamp", color: [0.42, 0.24, 0.14], transparent: false, flags: 0, top: TileSpec { col: 0, row: 0, files: &["redstone_lamp.png", "glowstone.png"], keep_alpha: false }, bottom: TileSpec { col: 0, row: 0, files: &["redstone_lamp.png", "glowstone.png"], keep_alpha: false }, sides: TileSpec { col: 0, row: 0, files: &["redstone_lamp.png", "glowstone.png"], keep_alpha: false } },
    // W80 - MC ground cover. Cross-pattern plants; the grayscale pack
    // textures carry a tintindex so the green `color` below tints them
    // (same path as the grass block top). F_WASHABLE so water clears them.
    // W144: REPLACEABLE like MC - building into a grass/fern cell
    // simply overwrites the plant (no drop; breaking it by hand
    // rolls the wheat-seeds loot instead).
    BlockType { id: 651, name: "short_grass", color: [0.36, 0.62, 0.28], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE | F_REPLACEABLE, top: TileSpec { col: 0, row: 0, files: &["short_grass.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["short_grass.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["short_grass.png"], keep_alpha: true } },
    BlockType { id: 652, name: "fern", color: [0.33, 0.55, 0.26], transparent: true, flags: F_RAY_PASS | F_AABB_PASS | F_NO_PLACE | F_WASHABLE | F_REPLACEABLE, top: TileSpec { col: 0, row: 0, files: &["fern.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["fern.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["fern.png"], keep_alpha: true } },
    // W127 - rails. Flat dedicated-render quads (emit_rail_quad);
    // AABB-passable (walk through) but ray-TARGETABLE (breakable).
    // Shapes (straight/curve/slope) derive from the neighbourhood
    // at remesh time. Curves are regular-rail only, like MC.
    BlockType { id: 653, name: "rail", color: [0.55, 0.47, 0.38], transparent: true, flags: F_DEDICATED_RENDER | F_AABB_PASS, top: TileSpec { col: 0, row: 0, files: &["rail.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["rail.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["rail.png"], keep_alpha: true } },
    BlockType { id: 654, name: "powered_rail", color: [0.60, 0.45, 0.30], transparent: true, flags: F_DEDICATED_RENDER | F_AABB_PASS, top: TileSpec { col: 0, row: 0, files: &["powered_rail.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["powered_rail.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["powered_rail.png"], keep_alpha: true } },
    BlockType { id: 655, name: "detector_rail", color: [0.55, 0.45, 0.35], transparent: true, flags: F_DEDICATED_RENDER | F_AABB_PASS, top: TileSpec { col: 0, row: 0, files: &["detector_rail.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["detector_rail.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["detector_rail.png"], keep_alpha: true } },
    BlockType { id: 656, name: "activator_rail", color: [0.55, 0.42, 0.35], transparent: true, flags: F_DEDICATED_RENDER | F_AABB_PASS, top: TileSpec { col: 0, row: 0, files: &["activator_rail.png"], keep_alpha: true }, bottom: TileSpec { col: 0, row: 0, files: &["activator_rail.png"], keep_alpha: true }, sides: TileSpec { col: 0, row: 0, files: &["activator_rail.png"], keep_alpha: true } },
];
