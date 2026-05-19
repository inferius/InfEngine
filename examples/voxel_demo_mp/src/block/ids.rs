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
