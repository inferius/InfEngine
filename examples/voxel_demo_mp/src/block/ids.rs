//! Block + item id constants.
//!
//! Split out of `block/mod.rs` in Wave 12 so the registry module
//! stays under the 500 LOC cap. Every constant matches a row in
//! `BLOCK_TYPES` (in `registry.rs`); call sites import either the
//! constant or use `block_type(id)` to resolve the row.

/// Block id of the torch (light source). Kept as a const so the
/// renderer-side light scan doesn't have to do a string lookup.
pub(crate) const TORCH_ID: u16 = 10;
pub(crate) const GLOWSTONE_ID: u16 = 28;
/// Cactus block id. Renders via the MC inset model (sides pulled in
/// 1 px). W109: opaque material but NOT a full cube, so it must not
/// cull a neighbouring full block's adjacent face - otherwise the
/// sand top under the cactus is culled and the 1 px inset gap shows
/// through to the void/sky. See `block_fully_occludes`.
pub(crate) const CACTUS_ID: u16 = 27;
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
#[allow(dead_code)]
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
#[allow(dead_code)]
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
/// RMB. Phase F. W5.2b adds 64-use durability tracked in
/// `Voxel::item_durability` (parallel map keyed by inventory slot
/// index; chosen over extending `Slot` so the save format stays
/// unchanged - migrate-into-Slot is a deferred follow-up).
pub(crate) const FLINT_AND_STEEL_ID: u16 = 48;
/// W5.2b: max uses of a fresh flint+steel. Matches MC's 64-use
/// budget; every ignition (portal frame OR fire block) decrements.
pub(crate) const FLINT_AND_STEEL_MAX_USES: u16 = 64;
/// Fire block id (Q.4b). Per-cell age (0..=15) lives in the
/// parallel `Voxel::fires` HashMap (same pattern as chests +
/// furnaces). Spreads to flammable neighbours, dies on
/// extinguish / support failure.
#[allow(dead_code)] // wired up by Q.4c fire tick + Q.4d/e ignite
pub(crate) const FIRE_ID: u16 = 49;

// ---------------------------------------------------------------
// W11.3 - Emitter + cosmetic block ids.
// ---------------------------------------------------------------
//
// Reserved id range 50..=70 for emitter + cosmetic blocks added
// alongside the MC `Blocks.java` lightLevel transcribe (W11.2).
// Each constant has a matching `BlockType` row in `BLOCK_TYPES`
// + a matching MC name in `light_emission::engine_id_to_mc_name`
// so the lighting BFS picks up the value on the next re-flood.

/// Redstone torch (lit). Emits 7 in MC (FireBlock.bootStrap unlit
/// emits 0; we ship the lit variant only since the engine has no
/// redstone state yet).
#[allow(dead_code)] // wired by W11.2 light_emission table
pub(crate) const REDSTONE_TORCH_ID: u16 = 50;
/// Redstone wall torch (lit). MC mirror of REDSTONE_TORCH for the
/// wall-mounted orientation; same emission.
#[allow(dead_code)]
pub(crate) const REDSTONE_WALL_TORCH_ID: u16 = 51;
/// Soul torch. MC emits 10.
#[allow(dead_code)]
pub(crate) const SOUL_TORCH_ID: u16 = 52;
/// Soul wall torch. Same emission as SOUL_TORCH (10).
#[allow(dead_code)]
pub(crate) const SOUL_WALL_TORCH_ID: u16 = 53;
/// Sea lantern. MC emits 15.
#[allow(dead_code)]
pub(crate) const SEA_LANTERN_ID: u16 = 54;
/// Jack o'lantern. MC emits 15.
#[allow(dead_code)]
pub(crate) const JACK_O_LANTERN_ID: u16 = 55;
/// Soul fire. MC emits 10.
#[allow(dead_code)]
pub(crate) const SOUL_FIRE_ID: u16 = 56;
/// End rod. MC emits 14.
#[allow(dead_code)]
pub(crate) const END_ROD_ID: u16 = 57;
/// Magma block. MC emits 3 - a low-grade light source that the
/// player can stand on (no fire damage in our engine yet).
#[allow(dead_code)]
pub(crate) const MAGMA_BLOCK_ID: u16 = 58;
/// Shroomlight. MC emits 15.
#[allow(dead_code)]
pub(crate) const SHROOMLIGHT_ID: u16 = 59;
/// Lantern. MC emits 15.
#[allow(dead_code)]
pub(crate) const LANTERN_ID: u16 = 60;
/// Soul lantern. MC emits 10.
#[allow(dead_code)]
pub(crate) const SOUL_LANTERN_ID: u16 = 61;
/// Crying obsidian. MC emits 10.
#[allow(dead_code)]
pub(crate) const CRYING_OBSIDIAN_ID: u16 = 62;
/// Respawn anchor. MC emits 0/3/7/11/15 based on charge state;
/// the engine has no charge state yet so the block ships with
/// emission 0 (state defaults to "uncharged"). The MC name
/// `respawn_anchor` is still in the emitter table at value 15 so
/// adding charge state later picks up the right max.
#[allow(dead_code)]
pub(crate) const RESPAWN_ANCHOR_ID: u16 = 63;
/// Ender chest. MC emits 7. Cosmetic-only in W11.3 (no per-player
/// inventory yet); the block lights up but has no inventory.
#[allow(dead_code)]
pub(crate) const ENDER_CHEST_ID: u16 = 64;
/// Lit redstone lamp. MC emits 15 when lit; the unlit form is a
/// separate id (66). Since the engine has no redstone state, the
/// lit form is the only variant we ship.
#[allow(dead_code)]
pub(crate) const LIT_REDSTONE_LAMP_ID: u16 = 65;
/// Lit redstone ore. MC emits 9 when lit (player walks over /
/// punches it). Like LIT_REDSTONE_LAMP we ship the lit form only.
#[allow(dead_code)]
pub(crate) const LIT_REDSTONE_ORE_ID: u16 = 66;
/// Light block (MC's invisible debug-stick light). MC emits 15.
/// In MC the player only gets this via the /setblock command with
/// the debug stick; our engine exposes it the same way.
#[allow(dead_code)]
pub(crate) const LIGHT_BLOCK_ID: u16 = 67;

// ---------------------------------------------------------------
// W12.2 - Slab / fence / door / stairs / wall ids (68..=82).
// ---------------------------------------------------------------
//
// Each id has a `BlockType` row in `registry.rs` with the
// `F_DEDICATED_RENDER` flag set + a matching `name_for_block`
// routing in `resource_pack::special_models`. Per the W12 design
// decision the slabs split into top + bottom ids, stairs default
// east-facing, doors split into lower + upper ids.

// ---------------------------------------------------------------
// W16 - Plant / flower ids (20..=22 already existed as BlockType
// rows; exported here so special_models + the mesher can reference
// them without bare literals).
// ---------------------------------------------------------------
pub(crate) const FLOWER_RED_ID: u16 = 20;
pub(crate) const FLOWER_YELLOW_ID: u16 = 21;
pub(crate) const FLOWER_BLUE_ID: u16 = 22;

#[allow(dead_code)]
pub(crate) const OAK_SLAB_ID: u16 = 68;
#[allow(dead_code)]
pub(crate) const STONE_SLAB_ID: u16 = 69;
#[allow(dead_code)]
pub(crate) const COBBLESTONE_SLAB_ID: u16 = 70;
#[allow(dead_code)]
pub(crate) const OAK_SLAB_TOP_ID: u16 = 71;
#[allow(dead_code)]
pub(crate) const OAK_STAIRS_ID: u16 = 72;
#[allow(dead_code)]
pub(crate) const STONE_STAIRS_ID: u16 = 73;
#[allow(dead_code)]
pub(crate) const COBBLESTONE_STAIRS_ID: u16 = 74;
#[allow(dead_code)]
pub(crate) const OAK_FENCE_ID: u16 = 75;
#[allow(dead_code)]
pub(crate) const NETHER_BRICK_FENCE_ID: u16 = 76;
#[allow(dead_code)]
pub(crate) const OAK_DOOR_ID: u16 = 77;
#[allow(dead_code)]
pub(crate) const OAK_DOOR_UPPER_ID: u16 = 78;
#[allow(dead_code)]
pub(crate) const IRON_DOOR_ID: u16 = 79;
#[allow(dead_code)]
pub(crate) const GLASS_PANE_ID: u16 = 80;
#[allow(dead_code)]
pub(crate) const IRON_BARS_ID: u16 = 81;
#[allow(dead_code)]
pub(crate) const COBBLESTONE_WALL_ID: u16 = 82;

// ---------------------------------------------------------------
// W35.3 - Stone variants + wood variants + wool + building blocks.
// IDs 83..=97.
// ---------------------------------------------------------------
#[allow(dead_code)]
pub(crate) const GRANITE_ID: u16 = 83;
#[allow(dead_code)]
pub(crate) const DIORITE_ID: u16 = 84;
#[allow(dead_code)]
pub(crate) const ANDESITE_ID: u16 = 85;
#[allow(dead_code)]
pub(crate) const BRICKS_ID: u16 = 86;
#[allow(dead_code)]
pub(crate) const SMOOTH_STONE_ID: u16 = 87;
#[allow(dead_code)]
pub(crate) const BIRCH_LOG_ID: u16 = 88;
#[allow(dead_code)]
pub(crate) const BIRCH_PLANKS_ID: u16 = 89;
#[allow(dead_code)]
pub(crate) const BIRCH_LEAVES_ID: u16 = 90;
#[allow(dead_code)]
pub(crate) const BLUE_WOOL_ID: u16 = 91;
#[allow(dead_code)]
pub(crate) const RED_WOOL_ID: u16 = 92;
#[allow(dead_code)]
pub(crate) const YELLOW_WOOL_ID: u16 = 93;
#[allow(dead_code)]
pub(crate) const GREEN_WOOL_ID: u16 = 94;
#[allow(dead_code)]
pub(crate) const TUFF_ID: u16 = 95;
#[allow(dead_code)]
pub(crate) const DEEPSLATE_ID: u16 = 96;
#[allow(dead_code)]
pub(crate) const CALCITE_ID: u16 = 97;

/// W46.21: Copper torch. MC 1.21+ adds copper-tinted variant. Emits 14.
#[allow(dead_code)]
pub(crate) const COPPER_TORCH_ID: u16 = 351;
/// W46.21: Copper wall torch. Wall variant of COPPER_TORCH. Emits 14.
#[allow(dead_code)]
pub(crate) const COPPER_WALL_TORCH_ID: u16 = 352;

// W48.1: 50 new high-value blocks from MC reference pack (IDs 353-402).
#[allow(dead_code)] pub(crate) const POLISHED_GRANITE_ID: u16 = 353;
#[allow(dead_code)] pub(crate) const POLISHED_DIORITE_ID: u16 = 354;
#[allow(dead_code)] pub(crate) const POLISHED_ANDESITE_ID: u16 = 355;
#[allow(dead_code)] pub(crate) const SMOOTH_SANDSTONE_ID: u16 = 356;
#[allow(dead_code)] pub(crate) const CUT_SANDSTONE_ID: u16 = 357;
#[allow(dead_code)] pub(crate) const RED_SANDSTONE_ID: u16 = 358;
#[allow(dead_code)] pub(crate) const SMOOTH_RED_SANDSTONE_ID: u16 = 359;
#[allow(dead_code)] pub(crate) const CUT_RED_SANDSTONE_ID: u16 = 360;
#[allow(dead_code)] pub(crate) const COBBLED_DEEPSLATE_ID: u16 = 361;
#[allow(dead_code)] pub(crate) const POLISHED_DEEPSLATE_ID: u16 = 362;
#[allow(dead_code)] pub(crate) const DEEPSLATE_BRICKS_ID: u16 = 363;
#[allow(dead_code)] pub(crate) const DEEPSLATE_TILES_ID: u16 = 364;
#[allow(dead_code)] pub(crate) const CHISELED_STONE_BRICKS_ID: u16 = 365;
#[allow(dead_code)] pub(crate) const MOSSY_COBBLESTONE_ID: u16 = 366;
#[allow(dead_code)] pub(crate) const MOSSY_STONE_BRICKS_ID: u16 = 367;
#[allow(dead_code)] pub(crate) const BLACKSTONE_ID: u16 = 368;
#[allow(dead_code)] pub(crate) const POLISHED_BLACKSTONE_ID: u16 = 369;
#[allow(dead_code)] pub(crate) const POLISHED_BLACKSTONE_BRICKS_ID: u16 = 370;
#[allow(dead_code)] pub(crate) const GILDED_BLACKSTONE_ID: u16 = 371;
#[allow(dead_code)] pub(crate) const BASALT_ID: u16 = 372;
#[allow(dead_code)] pub(crate) const POLISHED_BASALT_ID: u16 = 373;
#[allow(dead_code)] pub(crate) const SOUL_SAND_ID: u16 = 374;
#[allow(dead_code)] pub(crate) const SOUL_SOIL_ID: u16 = 375;
#[allow(dead_code)] pub(crate) const NETHER_BRICKS_ID: u16 = 376;
#[allow(dead_code)] pub(crate) const NETHER_WART_BLOCK_ID: u16 = 377;
#[allow(dead_code)] pub(crate) const END_STONE_ID: u16 = 378;
#[allow(dead_code)] pub(crate) const END_STONE_BRICKS_ID: u16 = 379;
#[allow(dead_code)] pub(crate) const PURPUR_BLOCK_ID: u16 = 380;
#[allow(dead_code)] pub(crate) const PURPUR_PILLAR_ID: u16 = 381;
#[allow(dead_code)] pub(crate) const CHORUS_FLOWER_ID: u16 = 382;
#[allow(dead_code)] pub(crate) const COPPER_BLOCK_ID: u16 = 383;
#[allow(dead_code)] pub(crate) const EXPOSED_COPPER_ID: u16 = 384;
#[allow(dead_code)] pub(crate) const WEATHERED_COPPER_ID: u16 = 385;
#[allow(dead_code)] pub(crate) const OXIDIZED_COPPER_ID: u16 = 386;
#[allow(dead_code)] pub(crate) const CUT_COPPER_ID: u16 = 387;
#[allow(dead_code)] pub(crate) const RAW_COPPER_BLOCK_ID: u16 = 388;
#[allow(dead_code)] pub(crate) const RAW_IRON_BLOCK_ID: u16 = 389;
#[allow(dead_code)] pub(crate) const RAW_GOLD_BLOCK_ID: u16 = 390;
#[allow(dead_code)] pub(crate) const GOLD_BLOCK_ID: u16 = 391;
#[allow(dead_code)] pub(crate) const DIAMOND_BLOCK_ID: u16 = 392;
#[allow(dead_code)] pub(crate) const DIRT_PATH_ID: u16 = 393;
#[allow(dead_code)] pub(crate) const PODZOL_ID: u16 = 394;
#[allow(dead_code)] pub(crate) const MYCELIUM_ID: u16 = 395;
#[allow(dead_code)] pub(crate) const COARSE_DIRT_ID: u16 = 396;
#[allow(dead_code)] pub(crate) const MUD_ID: u16 = 397;
#[allow(dead_code)] pub(crate) const PACKED_MUD_ID: u16 = 398;
#[allow(dead_code)] pub(crate) const MUD_BRICKS_ID: u16 = 399;
#[allow(dead_code)] pub(crate) const ICE_ID: u16 = 400;
#[allow(dead_code)] pub(crate) const PACKED_ICE_ID: u16 = 401;
#[allow(dead_code)] pub(crate) const BLUE_ICE_ID: u16 = 402;

// W48.2: 50 more blocks (IDs 403-452) - stained glass + terracotta + utility.
#[allow(dead_code)] pub(crate) const WHITE_STAINED_GLASS_ID: u16 = 403;
#[allow(dead_code)] pub(crate) const ORANGE_STAINED_GLASS_ID: u16 = 404;
#[allow(dead_code)] pub(crate) const MAGENTA_STAINED_GLASS_ID: u16 = 405;
#[allow(dead_code)] pub(crate) const LIGHT_BLUE_STAINED_GLASS_ID: u16 = 406;
#[allow(dead_code)] pub(crate) const YELLOW_STAINED_GLASS_ID: u16 = 407;
#[allow(dead_code)] pub(crate) const LIME_STAINED_GLASS_ID: u16 = 408;
#[allow(dead_code)] pub(crate) const PINK_STAINED_GLASS_ID: u16 = 409;
#[allow(dead_code)] pub(crate) const GRAY_STAINED_GLASS_ID: u16 = 410;
#[allow(dead_code)] pub(crate) const LIGHT_GRAY_STAINED_GLASS_ID: u16 = 411;
#[allow(dead_code)] pub(crate) const CYAN_STAINED_GLASS_ID: u16 = 412;
#[allow(dead_code)] pub(crate) const PURPLE_STAINED_GLASS_ID: u16 = 413;
#[allow(dead_code)] pub(crate) const BLUE_STAINED_GLASS_ID: u16 = 414;
#[allow(dead_code)] pub(crate) const BROWN_STAINED_GLASS_ID: u16 = 415;
#[allow(dead_code)] pub(crate) const GREEN_STAINED_GLASS_ID: u16 = 416;
#[allow(dead_code)] pub(crate) const RED_STAINED_GLASS_ID: u16 = 417;
#[allow(dead_code)] pub(crate) const BLACK_STAINED_GLASS_ID: u16 = 418;
#[allow(dead_code)] pub(crate) const TERRACOTTA_ID: u16 = 419;
#[allow(dead_code)] pub(crate) const WHITE_TERRACOTTA_ID: u16 = 420;
#[allow(dead_code)] pub(crate) const ORANGE_TERRACOTTA_ID: u16 = 421;
#[allow(dead_code)] pub(crate) const MAGENTA_TERRACOTTA_ID: u16 = 422;
#[allow(dead_code)] pub(crate) const LIGHT_BLUE_TERRACOTTA_ID: u16 = 423;
#[allow(dead_code)] pub(crate) const YELLOW_TERRACOTTA_ID: u16 = 424;
#[allow(dead_code)] pub(crate) const LIME_TERRACOTTA_ID: u16 = 425;
#[allow(dead_code)] pub(crate) const PINK_TERRACOTTA_ID: u16 = 426;
#[allow(dead_code)] pub(crate) const GRAY_TERRACOTTA_ID: u16 = 427;
#[allow(dead_code)] pub(crate) const LIGHT_GRAY_TERRACOTTA_ID: u16 = 428;
#[allow(dead_code)] pub(crate) const CYAN_TERRACOTTA_ID: u16 = 429;
#[allow(dead_code)] pub(crate) const PURPLE_TERRACOTTA_ID: u16 = 430;
#[allow(dead_code)] pub(crate) const BLUE_TERRACOTTA_ID: u16 = 431;
#[allow(dead_code)] pub(crate) const BROWN_TERRACOTTA_ID: u16 = 432;
#[allow(dead_code)] pub(crate) const GREEN_TERRACOTTA_ID: u16 = 433;
#[allow(dead_code)] pub(crate) const RED_TERRACOTTA_ID: u16 = 434;
#[allow(dead_code)] pub(crate) const HAY_BLOCK_ID: u16 = 435;
#[allow(dead_code)] pub(crate) const DRIED_KELP_BLOCK_ID: u16 = 436;
#[allow(dead_code)] pub(crate) const BONE_BLOCK_ID: u16 = 437;
#[allow(dead_code)] pub(crate) const LAPIS_BLOCK_ID: u16 = 438;
#[allow(dead_code)] pub(crate) const EMERALD_BLOCK_ID: u16 = 439;
#[allow(dead_code)] pub(crate) const IRON_BLOCK_ID: u16 = 440;
#[allow(dead_code)] pub(crate) const REDSTONE_BLOCK_ID: u16 = 441;
#[allow(dead_code)] pub(crate) const SLIME_BLOCK_ID: u16 = 442;
#[allow(dead_code)] pub(crate) const HONEY_BLOCK_ID: u16 = 443;
#[allow(dead_code)] pub(crate) const HONEYCOMB_BLOCK_ID: u16 = 444;
#[allow(dead_code)] pub(crate) const SPONGE_ID: u16 = 445;
#[allow(dead_code)] pub(crate) const WET_SPONGE_ID: u16 = 446;
#[allow(dead_code)] pub(crate) const MELON_ID: u16 = 447;
#[allow(dead_code)] pub(crate) const PUMPKIN_ID: u16 = 448;
#[allow(dead_code)] pub(crate) const CARVED_PUMPKIN_ID: u16 = 449;
#[allow(dead_code)] pub(crate) const OBSIDIAN_PILLAR_ID: u16 = 450;
#[allow(dead_code)] pub(crate) const AMETHYST_BLOCK_ID: u16 = 451;
#[allow(dead_code)] pub(crate) const BUDDING_AMETHYST_ID: u16 = 452;

// W48.3: 50 more (IDs 453-502) - redstone hardware + saplings + flowers + crops.
#[allow(dead_code)] pub(crate) const LEVER_ID: u16 = 453;
#[allow(dead_code)] pub(crate) const OAK_BUTTON_ID: u16 = 454;
#[allow(dead_code)] pub(crate) const STONE_BUTTON_ID: u16 = 455;
#[allow(dead_code)] pub(crate) const OAK_PRESSURE_PLATE_ID: u16 = 456;
#[allow(dead_code)] pub(crate) const STONE_PRESSURE_PLATE_ID: u16 = 457;
#[allow(dead_code)] pub(crate) const LIGHT_WEIGHTED_PRESSURE_PLATE_ID: u16 = 458;
#[allow(dead_code)] pub(crate) const HEAVY_WEIGHTED_PRESSURE_PLATE_ID: u16 = 459;
#[allow(dead_code)] pub(crate) const REPEATER_ID: u16 = 460;
#[allow(dead_code)] pub(crate) const COMPARATOR_ID: u16 = 461;
#[allow(dead_code)] pub(crate) const OBSERVER_ID: u16 = 462;
#[allow(dead_code)] pub(crate) const PISTON_ID: u16 = 463;
#[allow(dead_code)] pub(crate) const STICKY_PISTON_ID: u16 = 464;
#[allow(dead_code)] pub(crate) const DISPENSER_ID: u16 = 465;
#[allow(dead_code)] pub(crate) const DROPPER_ID: u16 = 466;
#[allow(dead_code)] pub(crate) const HOPPER_ID: u16 = 467;
#[allow(dead_code)] pub(crate) const OAK_SAPLING_ID: u16 = 468;
#[allow(dead_code)] pub(crate) const BIRCH_SAPLING_ID: u16 = 469;
#[allow(dead_code)] pub(crate) const SPRUCE_SAPLING_ID: u16 = 470;
#[allow(dead_code)] pub(crate) const JUNGLE_SAPLING_ID: u16 = 471;
#[allow(dead_code)] pub(crate) const ACACIA_SAPLING_ID: u16 = 472;
#[allow(dead_code)] pub(crate) const DARK_OAK_SAPLING_ID: u16 = 473;
#[allow(dead_code)] pub(crate) const MANGROVE_PROPAGULE_ID: u16 = 474;
#[allow(dead_code)] pub(crate) const CHERRY_SAPLING_ID: u16 = 475;
#[allow(dead_code)] pub(crate) const AZALEA_ID: u16 = 476;
#[allow(dead_code)] pub(crate) const FLOWERING_AZALEA_ID: u16 = 477;
#[allow(dead_code)] pub(crate) const ALLIUM_ID: u16 = 478;
#[allow(dead_code)] pub(crate) const BLUE_ORCHID_ID: u16 = 479;
#[allow(dead_code)] pub(crate) const DANDELION_ID: u16 = 480;
#[allow(dead_code)] pub(crate) const POPPY_ID: u16 = 481;
#[allow(dead_code)] pub(crate) const OXEYE_DAISY_ID: u16 = 482;
#[allow(dead_code)] pub(crate) const CORNFLOWER_ID: u16 = 483;
#[allow(dead_code)] pub(crate) const LILY_OF_THE_VALLEY_ID: u16 = 484;
#[allow(dead_code)] pub(crate) const SUNFLOWER_ID: u16 = 485;
#[allow(dead_code)] pub(crate) const PEONY_ID: u16 = 486;
#[allow(dead_code)] pub(crate) const ROSE_BUSH_ID: u16 = 487;
#[allow(dead_code)] pub(crate) const LILAC_ID: u16 = 488;
#[allow(dead_code)] pub(crate) const WITHER_ROSE_ID: u16 = 489;
#[allow(dead_code)] pub(crate) const TORCHFLOWER_ID: u16 = 490;
#[allow(dead_code)] pub(crate) const PITCHER_PLANT_ID: u16 = 491;
#[allow(dead_code)] pub(crate) const DEAD_BUSH_ID: u16 = 492;
#[allow(dead_code)] pub(crate) const WHEAT_ID: u16 = 493;
#[allow(dead_code)] pub(crate) const CARROTS_ID: u16 = 494;
#[allow(dead_code)] pub(crate) const POTATOES_ID: u16 = 495;
#[allow(dead_code)] pub(crate) const BEETROOTS_ID: u16 = 496;
#[allow(dead_code)] pub(crate) const MELON_STEM_ID: u16 = 497;
#[allow(dead_code)] pub(crate) const PUMPKIN_STEM_ID: u16 = 498;
#[allow(dead_code)] pub(crate) const SUGAR_CANE_ID: u16 = 499;
#[allow(dead_code)] pub(crate) const BAMBOO_ID: u16 = 500;
#[allow(dead_code)] pub(crate) const COCOA_ID: u16 = 501;
#[allow(dead_code)] pub(crate) const NETHER_WART_ID: u16 = 502;

// W48.4: slabs + stairs + walls + fences + trapdoors (IDs 503-552).
#[allow(dead_code)] pub(crate) const BIRCH_SLAB_ID: u16 = 503;
#[allow(dead_code)] pub(crate) const SPRUCE_SLAB_ID: u16 = 504;
#[allow(dead_code)] pub(crate) const JUNGLE_SLAB_ID: u16 = 505;
#[allow(dead_code)] pub(crate) const ACACIA_SLAB_ID: u16 = 506;
#[allow(dead_code)] pub(crate) const DARK_OAK_SLAB_ID: u16 = 507;
#[allow(dead_code)] pub(crate) const MANGROVE_SLAB_ID: u16 = 508;
#[allow(dead_code)] pub(crate) const CHERRY_SLAB_ID: u16 = 509;
#[allow(dead_code)] pub(crate) const SMOOTH_STONE_SLAB_ID: u16 = 510;
#[allow(dead_code)] pub(crate) const SANDSTONE_SLAB_ID: u16 = 511;
#[allow(dead_code)] pub(crate) const RED_SANDSTONE_SLAB_ID: u16 = 512;
#[allow(dead_code)] pub(crate) const BRICK_SLAB_ID: u16 = 513;
#[allow(dead_code)] pub(crate) const STONE_BRICK_SLAB_ID: u16 = 514;
#[allow(dead_code)] pub(crate) const MOSSY_STONE_BRICK_SLAB_ID: u16 = 515;
#[allow(dead_code)] pub(crate) const NETHER_BRICK_SLAB_ID: u16 = 516;
#[allow(dead_code)] pub(crate) const END_STONE_BRICK_SLAB_ID: u16 = 517;
#[allow(dead_code)] pub(crate) const BIRCH_STAIRS_ID: u16 = 518;
#[allow(dead_code)] pub(crate) const SPRUCE_STAIRS_ID: u16 = 519;
#[allow(dead_code)] pub(crate) const JUNGLE_STAIRS_ID: u16 = 520;
#[allow(dead_code)] pub(crate) const ACACIA_STAIRS_ID: u16 = 521;
#[allow(dead_code)] pub(crate) const DARK_OAK_STAIRS_ID: u16 = 522;
#[allow(dead_code)] pub(crate) const MANGROVE_STAIRS_ID: u16 = 523;
#[allow(dead_code)] pub(crate) const CHERRY_STAIRS_ID: u16 = 524;
#[allow(dead_code)] pub(crate) const SANDSTONE_STAIRS_ID: u16 = 525;
#[allow(dead_code)] pub(crate) const RED_SANDSTONE_STAIRS_ID: u16 = 526;
#[allow(dead_code)] pub(crate) const BRICK_STAIRS_ID: u16 = 527;
#[allow(dead_code)] pub(crate) const STONE_BRICK_STAIRS_ID: u16 = 528;
#[allow(dead_code)] pub(crate) const MOSSY_STONE_BRICK_STAIRS_ID: u16 = 529;
#[allow(dead_code)] pub(crate) const NETHER_BRICK_STAIRS_ID: u16 = 530;
#[allow(dead_code)] pub(crate) const GRANITE_STAIRS_ID: u16 = 531;
#[allow(dead_code)] pub(crate) const POLISHED_GRANITE_STAIRS_ID: u16 = 532;
#[allow(dead_code)] pub(crate) const GRANITE_WALL_ID: u16 = 533;
#[allow(dead_code)] pub(crate) const DIORITE_WALL_ID: u16 = 534;
#[allow(dead_code)] pub(crate) const ANDESITE_WALL_ID: u16 = 535;
#[allow(dead_code)] pub(crate) const SANDSTONE_WALL_ID: u16 = 536;
#[allow(dead_code)] pub(crate) const RED_SANDSTONE_WALL_ID: u16 = 537;
#[allow(dead_code)] pub(crate) const BRICK_WALL_ID: u16 = 538;
#[allow(dead_code)] pub(crate) const STONE_BRICK_WALL_ID: u16 = 539;
#[allow(dead_code)] pub(crate) const MOSSY_STONE_BRICK_WALL_ID: u16 = 540;
#[allow(dead_code)] pub(crate) const NETHER_BRICK_WALL_ID: u16 = 541;
#[allow(dead_code)] pub(crate) const END_STONE_BRICK_WALL_ID: u16 = 542;
#[allow(dead_code)] pub(crate) const BIRCH_FENCE_ID: u16 = 543;
#[allow(dead_code)] pub(crate) const SPRUCE_FENCE_ID: u16 = 544;
#[allow(dead_code)] pub(crate) const JUNGLE_FENCE_ID: u16 = 545;
#[allow(dead_code)] pub(crate) const ACACIA_FENCE_ID: u16 = 546;
#[allow(dead_code)] pub(crate) const DARK_OAK_FENCE_ID: u16 = 547;
#[allow(dead_code)] pub(crate) const OAK_TRAPDOOR_ID: u16 = 548;
#[allow(dead_code)] pub(crate) const IRON_TRAPDOOR_ID: u16 = 549;
#[allow(dead_code)] pub(crate) const BIRCH_TRAPDOOR_ID: u16 = 550;
#[allow(dead_code)] pub(crate) const SPRUCE_TRAPDOOR_ID: u16 = 551;
#[allow(dead_code)] pub(crate) const JUNGLE_TRAPDOOR_ID: u16 = 552;

// W48.5: utility + doors + beds + misc (IDs 553-602).
#[allow(dead_code)] pub(crate) const CRAFTING_TABLE_ID: u16 = 553;
#[allow(dead_code)] pub(crate) const ANVIL_ID: u16 = 554;
#[allow(dead_code)] pub(crate) const CHIPPED_ANVIL_ID: u16 = 555;
#[allow(dead_code)] pub(crate) const DAMAGED_ANVIL_ID: u16 = 556;
#[allow(dead_code)] pub(crate) const SMITHING_TABLE_ID: u16 = 557;
#[allow(dead_code)] pub(crate) const FLETCHING_TABLE_ID: u16 = 558;
#[allow(dead_code)] pub(crate) const CARTOGRAPHY_TABLE_ID: u16 = 559;
#[allow(dead_code)] pub(crate) const LECTERN_ID: u16 = 560;
#[allow(dead_code)] pub(crate) const SMOKER_ID: u16 = 561;
#[allow(dead_code)] pub(crate) const BLAST_FURNACE_ID: u16 = 562;
/// W169: barrel - a 27-slot container (reuses the chest store + UI).
pub(crate) const BARREL_ID: u16 = 749;
#[allow(dead_code)] pub(crate) const GRINDSTONE_ID: u16 = 563;
#[allow(dead_code)] pub(crate) const STONECUTTER_ID: u16 = 564;
#[allow(dead_code)] pub(crate) const LOOM_ID: u16 = 565;
#[allow(dead_code)] pub(crate) const BREWING_STAND_ID: u16 = 566;
#[allow(dead_code)] pub(crate) const BEACON_ID: u16 = 567;
#[allow(dead_code)] pub(crate) const BIRCH_DOOR_ID: u16 = 568;
#[allow(dead_code)] pub(crate) const SPRUCE_DOOR_ID: u16 = 569;
#[allow(dead_code)] pub(crate) const JUNGLE_DOOR_ID: u16 = 570;
#[allow(dead_code)] pub(crate) const ACACIA_DOOR_ID: u16 = 571;
#[allow(dead_code)] pub(crate) const DARK_OAK_DOOR_ID: u16 = 572;
#[allow(dead_code)] pub(crate) const MANGROVE_DOOR_ID: u16 = 573;
#[allow(dead_code)] pub(crate) const CHERRY_DOOR_ID: u16 = 574;
#[allow(dead_code)] pub(crate) const WARPED_DOOR_ID: u16 = 575;
#[allow(dead_code)] pub(crate) const WHITE_BED_ID: u16 = 576;
#[allow(dead_code)] pub(crate) const ORANGE_BED_ID: u16 = 577;
#[allow(dead_code)] pub(crate) const MAGENTA_BED_ID: u16 = 578;
#[allow(dead_code)] pub(crate) const LIGHT_BLUE_BED_ID: u16 = 579;
#[allow(dead_code)] pub(crate) const YELLOW_BED_ID: u16 = 580;
#[allow(dead_code)] pub(crate) const LIME_BED_ID: u16 = 581;
#[allow(dead_code)] pub(crate) const PINK_BED_ID: u16 = 582;
#[allow(dead_code)] pub(crate) const GRAY_BED_ID: u16 = 583;
#[allow(dead_code)] pub(crate) const LIGHT_GRAY_BED_ID: u16 = 584;
#[allow(dead_code)] pub(crate) const CYAN_BED_ID: u16 = 585;
#[allow(dead_code)] pub(crate) const PURPLE_BED_ID: u16 = 586;
#[allow(dead_code)] pub(crate) const BLUE_BED_ID: u16 = 587;
#[allow(dead_code)] pub(crate) const BROWN_BED_ID: u16 = 588;
#[allow(dead_code)] pub(crate) const GREEN_BED_ID: u16 = 589;
#[allow(dead_code)] pub(crate) const RED_BED_ID: u16 = 590;
#[allow(dead_code)] pub(crate) const BLACK_BED_ID: u16 = 591;
#[allow(dead_code)] pub(crate) const JUKEBOX_ID: u16 = 592;
#[allow(dead_code)] pub(crate) const NOTE_BLOCK_ID: u16 = 593;
#[allow(dead_code)] pub(crate) const SCAFFOLDING_ID: u16 = 594;
#[allow(dead_code)] pub(crate) const LADDER_ID: u16 = 595;
#[allow(dead_code)] pub(crate) const END_PORTAL_FRAME_ID: u16 = 596;
#[allow(dead_code)] pub(crate) const DRAGON_EGG_ID: u16 = 597;
#[allow(dead_code)] pub(crate) const CONDUIT_ID: u16 = 598;
#[allow(dead_code)] pub(crate) const TARGET_ID: u16 = 599;
#[allow(dead_code)] pub(crate) const BEE_NEST_ID: u16 = 600;
#[allow(dead_code)] pub(crate) const BEEHIVE_ID: u16 = 601;
#[allow(dead_code)] pub(crate) const LODESTONE_ID: u16 = 602;

// W48.6: waxed copper + nether nature + lush cave + coral + sculk (IDs 603-632).
#[allow(dead_code)] pub(crate) const WAXED_COPPER_BLOCK_ID: u16 = 603;
#[allow(dead_code)] pub(crate) const WAXED_EXPOSED_COPPER_ID: u16 = 604;
#[allow(dead_code)] pub(crate) const WAXED_WEATHERED_COPPER_ID: u16 = 605;
#[allow(dead_code)] pub(crate) const WAXED_OXIDIZED_COPPER_ID: u16 = 606;
#[allow(dead_code)] pub(crate) const CRIMSON_NYLIUM_ID: u16 = 607;
#[allow(dead_code)] pub(crate) const WARPED_NYLIUM_ID: u16 = 608;
#[allow(dead_code)] pub(crate) const CRIMSON_FUNGUS_ID: u16 = 609;
#[allow(dead_code)] pub(crate) const WARPED_FUNGUS_ID: u16 = 610;
#[allow(dead_code)] pub(crate) const WEEPING_VINES_ID: u16 = 611;
#[allow(dead_code)] pub(crate) const TWISTING_VINES_ID: u16 = 612;
#[allow(dead_code)] pub(crate) const NETHER_SPROUTS_ID: u16 = 613;
#[allow(dead_code)] pub(crate) const WARPED_WART_BLOCK_ID: u16 = 614;
#[allow(dead_code)] pub(crate) const MOSS_BLOCK_ID: u16 = 615;
#[allow(dead_code)] pub(crate) const MOSS_CARPET_ID: u16 = 616;
#[allow(dead_code)] pub(crate) const BIG_DRIPLEAF_ID: u16 = 617;
#[allow(dead_code)] pub(crate) const SMALL_DRIPLEAF_ID: u16 = 618;
#[allow(dead_code)] pub(crate) const HANGING_ROOTS_ID: u16 = 619;
#[allow(dead_code)] pub(crate) const SPORE_BLOSSOM_ID: u16 = 620;
#[allow(dead_code)] pub(crate) const TUBE_CORAL_BLOCK_ID: u16 = 621;
#[allow(dead_code)] pub(crate) const BRAIN_CORAL_BLOCK_ID: u16 = 622;
#[allow(dead_code)] pub(crate) const BUBBLE_CORAL_BLOCK_ID: u16 = 623;
#[allow(dead_code)] pub(crate) const FIRE_CORAL_BLOCK_ID: u16 = 624;
#[allow(dead_code)] pub(crate) const HORN_CORAL_BLOCK_ID: u16 = 625;
#[allow(dead_code)] pub(crate) const DEAD_BRAIN_CORAL_BLOCK_ID: u16 = 626;
#[allow(dead_code)] pub(crate) const GLOW_LICHEN_ID: u16 = 627;
#[allow(dead_code)] pub(crate) const SCULK_ID: u16 = 628;
#[allow(dead_code)] pub(crate) const SCULK_CATALYST_ID: u16 = 629;
#[allow(dead_code)] pub(crate) const SCULK_SHRIEKER_ID: u16 = 630;
#[allow(dead_code)] pub(crate) const SCULK_SENSOR_ID: u16 = 631;
#[allow(dead_code)] pub(crate) const CALIBRATED_SCULK_SENSOR_ID: u16 = 632;
// W64.2 - gravity blocks (fall like sand via world::falling). Ids
// 633..=643 are tool/weapon items in the shared id space, so these
// jump to 644/645 to keep `item.id == block.id` for block-forms.
pub(crate) const GRAVEL_ID: u16 = 644;
pub(crate) const RED_SAND_ID: u16 = 645;
// W65.4 - immature crop stages (grow to the mature crop via random
// tick). World-only intermediate blocks, no item form.
// W294: the *_IMMATURE_ID consts retired - crop stage chains live
// in block::crop_stages (age 0 = 646..649).
// W67.1 - redstone signal blocks. Wire (309) + sources + lamp pair.
pub(crate) const REDSTONE_DUST_ID: u16 = 309;
pub(crate) const REDSTONE_LAMP_ID: u16 = 650;
// W80 - MC ground cover (cross-pattern, biome-tinted plants).
pub(crate) const SHORT_GRASS_ID: u16 = 651;
pub(crate) const FERN_ID: u16 = 652;
// W201 - ocean flora (Phase 6a of the MC worldgen port). Underwater
// cross-plants placed by the data-driven MC placed_feature rules in
// `worldgen::ocean_flora`. tall_seagrass is MC's two-block plant
// (half=lower/upper -> two engine ids); the kelp column is kelp_plant
// stem segments topped by one kelp (the growing tip).
pub(crate) const SEAGRASS_ID: u16 = 753;
pub(crate) const TALL_SEAGRASS_BOTTOM_ID: u16 = 754;
pub(crate) const TALL_SEAGRASS_TOP_ID: u16 = 755;
pub(crate) const KELP_ID: u16 = 756;
pub(crate) const KELP_PLANT_ID: u16 = 757;
// W127 - rails (flat dedicated-render ground blocks; shapes derive
// from the neighbourhood at remesh time, MC RailShape style).
pub(crate) const RAIL_ID: u16 = 653;
pub(crate) const POWERED_RAIL_ID: u16 = 654;
pub(crate) const DETECTOR_RAIL_ID: u16 = 655;
pub(crate) const ACTIVATOR_RAIL_ID: u16 = 656;
// W242 - monster spawner (MonsterRoomFeature places it at the room
// origin; the worker chunk scan reports it as a side effect).
pub(crate) const SPAWNER_ID: u16 = 793;
// W243 - cobweb (mineshaft spider corridors; MC block `cobweb`).
pub(crate) const COBWEB_ID: u16 = 794;
