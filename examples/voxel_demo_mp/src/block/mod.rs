//! Block registry, flags, and per-block lookup helpers.
//!
//! Wave 12 split: this module is now a thin facade.
//! - `types.rs` owns `BlockType`, `TileSpec`, F_* flags, atlas dims.
//! - `registry.rs` owns the static `BLOCK_TYPES` table.
//! - `ids.rs` owns the `*_ID` constants.
//!
//! mod.rs glues them back together with re-exports + the runtime
//! lookup helpers (`block_type`, `block_has_flag`, ...). The atlas
//! UV resolver + tool tiers + bucket helpers + material mapper
//! also live here because they are the table's primary consumers.
//!
//! Layout mirrors MC's `net/minecraft/world/level/block` package
//! at a coarse level. Each block kind there has its own class;
//! ours collapse into one `BlockType` row + behaviour flags.
//! As blocks gain richer state (containers, multi-block, ...)
//! we'll promote some rows to dedicated sub-modules under
//! `block/`.

use infengine_voxel::BlockId;

pub(crate) mod brewing;
pub(crate) mod chest;
pub(crate) mod crop_stages;
pub(crate) mod door;
pub(crate) mod rail;
pub(crate) mod rail_state;
pub(crate) mod fire;
pub(crate) mod flammability;
pub(crate) mod furnace;
pub(crate) mod hopper;
mod ids;
pub(crate) mod light_emission;
pub(crate) mod portal;
mod registry;
mod types;
#[cfg(test)]
mod fire_tests;
#[cfg(test)]
mod phase_f_tests;
#[cfg(test)]
mod split_tests;
#[cfg(test)]
mod w11_emitter_tests;
#[cfg(test)]
mod w12_blocks_tests;

// Re-export every type + constant the rest of the engine consumes.
// New call sites should `use crate::block::*` (or the specific
// name); the internal split into sub-modules is an implementation
// detail.
pub(crate) use ids::*;
pub(crate) use registry::BLOCK_TYPES;
pub(crate) use types::{
    BlockType, TileSpec, ATLAS_COLS, ATLAS_ROWS, F_AABB_PASS,
    F_NO_PLACE, F_RAY_PASS, F_REPLACEABLE, F_SWIM, F_TRANSPARENT_RENDER, F_WASHABLE,
    TILE_PX,
};
#[allow(unused_imports)]
pub(crate) use types::F_DEDICATED_RENDER;

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

/// W203: underwater plants occupy their cell INSTEAD of water (the
/// engine has no waterlogged block states), but in MC these blocks are
/// waterlogged - the cell contains the plant AND water. Everything that
/// asks "is this cell part of the water body" (liquid meshing, the
/// depth fade, swim, underwater ambience) must treat them as water, or
/// the water renders an air-pocket box around every plant.
pub(crate) fn block_is_waterlogged(id: u16) -> bool {
    // W223: sea pickles (774) are always-waterlogged like kelp.
    // W228: coral plants (782-786) + fans (787-791); the solid coral
    // BLOCKS (777-781) are not.
    matches!(
        id,
        SEAGRASS_ID | TALL_SEAGRASS_BOTTOM_ID | TALL_SEAGRASS_TOP_ID
            | KELP_ID | KELP_PLANT_ID | 774 | 782..=791
    )
}

/// W203: "the eye/cell is in water" - true water or a waterlogged
/// plant cell (swimming through a kelp stalk is still swimming).
pub(crate) fn block_is_watery(id: u16) -> bool {
    id == WATER_ID || block_is_waterlogged(id)
}

/// Voxel is a liquid the player can swim in.
#[inline]
pub(crate) fn block_is_swim_medium(b: BlockId) -> bool {
    block_has_flag(b, F_SWIM)
}

/// Block has its own per-voxel render path (torch pillar, snow
/// slab, water slab, portal swirl, fire cross) so the chunk
/// mesher should drop its submesh. ONLY these blocks get an
/// alternate world-pass render in `render::decor`; every other
/// F_DEDICATED_RENDER block (slabs, doors, stairs, chest, etc.)
/// emits its special-model geometry into the chunk submesh and
/// needs that submesh uploaded.
///
/// W60.1: previously gated on F_DEDICATED_RENDER alone, which
/// dropped every special-model submesh (chest invisible, slab
/// invisible, ...). MC ships these as block models + we render
/// them through `emit_block_elements`; the chunk submesh IS the
/// render path for them.
#[inline]
pub(crate) fn block_skips_chunk_mesh(b: BlockId) -> bool {
    // W62.1 - hard list of blocks drawn by `render::decor`
    // instead of through chunk submeshes. Torches (10, 50, 51,
    // 52, 53) were dropped from this set in W46.15 - the chunk
    // mesher emits torch geometry via emit_block_elements on
    // the template_torch JSON model + the cutout-layer flag on
    // stream upload. Including them here makes torches
    // invisible (regression W60.1).
    //
    // Current dedicated-pass-only ids:
    //   (15 = snow layer LEFT this list in W316 - the slab meshes
    //   into the chunk now; keeping it here made snow invisible)
    //   36 = water (liquid_mesh_handles)
    //   37 = lava (liquid_mesh_handles)
    //   47 = portal (portal_positions HashSet)
    //   49 = fire (fires HashMap)
    //   56 = soul_fire (fires HashMap)
    matches!(b.0, 36 | 37 | 47 | 49 | 56)
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

/// W109: does this block FULLY fill its cell, occluding a neighbour's
/// adjacent face? `block_is_opaque` conflates "opaque material" with
/// "full cube": the cactus is opaque but its model insets the sides
/// 1 px, so when it culled the top face of the sand it stands on, the
/// inset gap showed through to the void/sky instead of the sand. The
/// chunk mesher's face-cull predicate uses THIS; lighting keeps using
/// `block_is_opaque` (cactus still blocks light, MC-style).
/// W365: MC `RotatedPillarBlock` family - blocks whose model is a
/// `cube_column` with an `axis` state (logs, stripped logs, wood,
/// nether stems/hyphae). The axis rides the per-pos orientation
/// byte map (`stair_states`, 1 = X / 2 = Z; Y needs no record).
pub(crate) fn is_pillar_block(id: u16) -> bool {
    block_type(id).is_some_and(|bt| {
        bt.name == "log"
            || bt.name.ends_with("_log")
            || bt.name.ends_with("_wood")
            || bt.name.ends_with("_stem")
            || bt.name.ends_with("_hyphae")
    })
}

pub(crate) fn block_fully_occludes(b: BlockId) -> bool {
    if b.0 == CACTUS_ID {
        return false;
    }
    // W126: the chest is an inset block ENTITY (14x14x14, drawn by
    // render::chests) - its cell holds no chunk-mesh geometry, so
    // culling neighbour faces against it would punch see-through
    // holes behind every chest.
    if b.0 == CHEST_ID {
        return false;
    }
    block_is_opaque(b)
}

/// W133: does a multipart block (fence/wall/pane) grow an arm toward
/// `neighbor`? Ports the MC connection predicates instead of the old
/// strict same-id rule (which left e.g. an oak fence next to a birch
/// fence - or next to a stone wall it should grab - unconnected):
/// - `FenceBlock::connectsTo`: wooden fences interconnect as a tag
///   family; the nether brick fence is in FENCES but not
///   WOODEN_FENCES so it only pairs with itself; both grab
///   side-sturdy solids.
/// - `IronBarsBlock::attachsTo`: panes + bars connect to any
///   IronBarsBlock, to walls, and to solids.
/// - `WallBlock::connectsTo`: walls connect to walls, panes/bars,
///   and solids.
///
/// Everything else keeps same-id. Side-sturdiness approximated by
/// `block_fully_occludes`, which also excludes MC's connection
/// exceptions (leaves are transparent, so they never read as solid
/// here).
///
/// Fence gates join the predicate once the engine ships them.
pub(crate) fn multipart_connects(self_id: u16, neighbor: BlockId) -> bool {
    let n = neighbor.0;
    let wood_fence = |id: u16| {
        id == OAK_FENCE_ID || (BIRCH_FENCE_ID..=DARK_OAK_FENCE_ID).contains(&id)
    };
    let wall = |id: u16| {
        id == COBBLESTONE_WALL_ID
            || (GRANITE_WALL_ID..=END_STONE_BRICK_WALL_ID).contains(&id)
    };
    let pane_or_bars = |id: u16| id == GLASS_PANE_ID || id == IRON_BARS_ID;
    let solid = !neighbor.is_air() && block_fully_occludes(neighbor);
    // W287: MC FenceBlock.connectsTo includes fence GATES - the
    // village fence used to stop one block short of every gate
    // (docx img9). Honest divergence: MC also checks the gate's
    // facing axis (connectsToDirection); the neighbour's facing
    // lives in the runtime door map which this pure id-level rule
    // can't see, so an axis-misaligned gate still grabs an arm.
    let gate = |id: u16| crate::block::door::is_fence_gate_block(id);
    if wood_fence(self_id) {
        return wood_fence(n) || gate(n) || solid;
    }
    if self_id == NETHER_BRICK_FENCE_ID {
        return n == NETHER_BRICK_FENCE_ID || gate(n) || solid;
    }
    if pane_or_bars(self_id) {
        return pane_or_bars(n) || wall(n) || solid;
    }
    if wall(self_id) {
        return wall(n) || pane_or_bars(n) || solid;
    }
    n == self_id
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
        // W64.2: gravel + red_sand are granular - share the soft sand
        // bank (closest match; MC's distinct gravel bank is a TODO).
        x if x == GRAVEL_ID || x == RED_SAND_ID => "sand",
        // W123: everything else classifies by registry NAME so new
        // blocks pick the right MC sound family without a fresh
        // magic-number row here (the registry name IS the MC name).
        _ => block_material_by_name(b.0),
    }
}

/// W123: name-driven sound-material classification. Keyword rules
/// recover MC's SoundType grouping for the whole roster. Returns the
/// engine material key used to build `dig:<mat>` / `step:<mat>`
/// audio-bank keys; each returned value must have ENGINE_TO_MC rows.
fn block_material_by_name(id: u16) -> &'static str {
    let Some(bt) = block_type(id) else {
        return "stone";
    };
    let n = bt.name;
    // Order matters: the specific families match before the broad
    // keyword buckets (deepslate before stone, iron_door before the
    // wood "door" rule, nether_brick before "brick"-ish stone).
    if n.contains("deepslate") {
        "deepslate"
    } else if n == "cobweb" {
        // W243: MC has a dedicated block.cobweb bank; until its OGGs
        // are verified in the cache, the soft grass rustle is the
        // closest loaded family (a stone thud would be worse).
        "grass"
    } else if n.contains("nether_brick") {
        "nether_bricks"
    } else if n.contains("netherrack") || n.contains("nether_quartz_ore") || n.contains("nether_gold_ore") {
        "netherrack"
    } else if n.contains("basalt") {
        "basalt"
    } else if n.contains("nylium") {
        "nylium"
    } else if n.contains("nether_wart") {
        "nether_wart"
    } else if n.contains("sculk") {
        "sculk"
    } else if n.contains("bone") {
        "bone"
    } else if n.contains("wool") || n.contains("carpet") {
        "wool"
    } else if n.contains("snow") {
        "snow"
    } else if n.contains("glass") {
        "glass"
    } else if n.contains("slime") {
        "slime"
    } else if n.contains("honey") {
        "honey"
    } else if n.contains("coral") {
        "coral"
    } else if n.contains("mud") {
        "mud"
    } else if n.contains("kelp") || n.contains("seagrass") {
        // W201: underwater plants use MC's wet_grass family. Must match
        // BEFORE the broad "grass"/"kelp" rules in the grass bucket below.
        "wet_grass"
    } else if n.contains("iron_block")
        || n.contains("gold_block")
        || n.contains("copper")
        || n.contains("anvil")
        || n.contains("chain")
        || n.contains("iron_bars")
        || n.contains("iron_door")
        || n.contains("iron_trapdoor")
        || n.contains("cauldron")
        || n.contains("lantern")
        || n.contains("raw_iron")
        || n.contains("raw_gold")
        || n.ends_with("_rail")
        || n == "rail"
    {
        "metal"
    } else if n.contains("sand") {
        "sand"
    } else if n.contains("gravel")
        || n.contains("dirt")
        || n.contains("podzol")
        || n.contains("mycelium")
        || n.contains("farmland")
        || n.contains("leaves")
        || n.contains("grass")
        || n.contains("moss")
        || n.contains("vine")
        || n.contains("lily")
        || n.contains("sapling")
        || n.contains("fern")
        || n.contains("wheat")
        || n.contains("sugar_cane")
        || n.contains("kelp")
        || n.contains("seagrass")
        || n.contains("bamboo")
        || n.ends_with("_stem")
    {
        "grass"
    } else if n.contains("plank")
        || n.contains("log")
        || n.contains("_wood")
        || n.contains("stripped")
        || n.contains("fence")
        || n.contains("door")
        || n.contains("trapdoor")
        || n.contains("chest")
        || n.contains("barrel")
        || n.contains("table")
        || n.contains("ladder")
        || n.contains("sign")
        || n.contains("bookshelf")
        || n.contains("loom")
        || n.contains("lectern")
        || n.contains("composter")
        || n.contains("scaffolding")
        || n.contains("beehive")
        || n.contains("bee_nest")
        || n.contains("campfire")
        || n.contains("jukebox")
        || n.contains("note_block")
        || n.contains("crafting")
        || n.contains("bed")
        || n.contains("torch")
        || n.contains("pumpkin")
        || n.contains("melon")
    {
        "wood"
    } else {
        "stone"
    }
}

#[cfg(test)]
mod material_tests {
    use super::*;

    fn id_of(name: &str) -> u16 {
        BLOCK_TYPES
            .iter()
            .find(|bt| bt.name == name)
            .unwrap_or_else(|| panic!("registry has no block named {name}"))
            .id
    }

    /// W123: the name classifier recovers MC sound families for the
    /// post-legacy roster (legacy low ids keep the id fast path).
    #[test]
    fn name_classifier_maps_core_families() {
        for (name, want) in [
            ("deepslate", "deepslate"),
            ("deepslate_diamond_ore", "deepslate"),
            ("blue_wool", "wool"),
            ("copper_block", "metal"),
            ("birch_planks", "wood"),
            ("birch_leaves", "grass"),
            ("glass_pane", "glass"),
            ("diamond_ore", "stone"),
        ] {
            let got = block_material(BlockId(id_of(name)));
            assert_eq!(got, want, "{name} should sound like {want}, got {got}");
        }
    }

    // The "every emitted material has ENGINE_TO_MC dig+step rows"
    // cross-check lives in resource_pack::sounds (bin-only module;
    // the lib target doesn't compile resource_pack).
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
/// W53.4: extended to cover gold/diamond/netherite tier variants
/// (added in W48) + hoes / shears / bow / crossbow / fishing_rod /
/// trident. Pattern-driven so new tool items pick up the right
/// behavior without per-id hardcoding.
pub(crate) fn is_tool_block(block: BlockId) -> bool {
    // Legacy fixed-id ranges for the original tool block. Kept for
    // back-compat with save files that store these ids verbatim.
    if matches!(
        block.0,
        16..=19 | 23..=25 | 29..=34 | 38..=40 | FLINT_AND_STEEL_ID
    ) {
        return true;
    }
    // W53.4: name-pattern check for new tool ids. Drives off the
    // ITEM_TYPES table so all gold/diamond/netherite/hoe variants
    // pick up the right behavior without per-id hardcoding.
    let Some(it) = crate::item::item_type(block.0) else {
        return false;
    };
    let name = it.name;
    name.starts_with("pickaxe_")
        || name.starts_with("shovel_")
        || name.starts_with("axe_")
        || name.starts_with("sword_")
        || name.starts_with("hoe_")
        || name == "shears"
        || name == "bow"
        || name == "crossbow"
        || name == "fishing_rod"
        || name == "trident"
        || name == "flint_and_steel"
        || name.ends_with("_bucket")
        || name == "bucket"
}

/// Human label for a tool slot, used by the HUD readout.
/// W53.4: fall through to ITEM_TYPES display name for tool ids
/// the legacy match arms don't cover (gold/diamond/netherite/hoe).
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
        _ => {
            // W53.4: fall through to ITEM_TYPES for the new tool
            // variants (gold/diamond/netherite/hoe/bow/etc.).
            if !is_tool_block(block) {
                return None;
            }
            crate::item::item_type(block.0).and_then(|it| it.display)
        }
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
/// W294: tier of a hoe slot, 0 = none (332 wooden, 333 stone,
/// 334 iron, 335 golden, 278 netherite).
pub(crate) fn hoe_tier(block: BlockId) -> u8 {
    match block.0 {
        332 => 1,
        333 | 335 => 2,
        334 | 278 => 3,
        _ => 0,
    }
}

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

// ---------------------------------------------------------------
// W36.1 - Per-block PBR overrides.
// ---------------------------------------------------------------

/// Return (metallic, roughness) for a block id.
///
/// Most terrain is fully dielectric (metallic=0) and almost matte
/// (roughness=0.95) to prevent the Cook-Torrance specular peak from
/// washing out dark albedo values. Specific materials override:
///
/// - obsidian: roughness=1.0 (fully matte, no specular at all)
/// - glass/glass_pane: roughness=0.05 (smooth, sharp reflection)
/// - iron_ore/iron_bars: metallic=0.4, roughness=0.5
/// - iron block (door/bars grouped): metallic=0.6, roughness=0.4
/// - glowstone/sea_lantern: roughness=1.0 (matte emissive look)
///
/// Default for every other block: metallic=0.0, roughness=0.95.
#[inline]
pub(crate) fn block_pbr(id: u16) -> (f32, f32) {
    match id {
        // Obsidian - fully matte so dark albedo stays vibrant.
        OBSIDIAN_ID => (0.0, 1.0),
        // Crying obsidian - same treatment.
        62 => (0.0, 1.0),
        // Glass + glass pane - very smooth for a crisp specular edge.
        9 | 80 => (0.0, 0.05),
        // Iron ore - slight metallic sheen.
        14 => (0.4, 0.5),
        // Iron door + iron bars - more metallic than raw ore.
        79 | 81 => (0.6, 0.4),
        // Glowstone + sea lantern - matte to let emissive colour pop.
        28 | 54 => (0.0, 1.0),
        // Default: dielectric, almost matte.
        _ => (0.0, 0.95),
    }
}
