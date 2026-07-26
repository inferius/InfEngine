//! W47.13 - place / break decision helpers extracted from `on_update`.
//!
//! The block-edit path inside `Voxel::on_update` is a ~1100 LOC
//! sequence with deep `&mut self` borrows + side effects (chunk
//! rebuilds, edit broadcasts, particle spawns, inventory writes).
//! Extracting the entire flow at once is high-risk; this module
//! instead grew piece-by-piece with pure free functions that the
//! main loop calls. Each helper takes only the fields it needs so
//! the caller can keep other borrows alive.
//!
//! W47.14: the single 1261-LOC file split into per-action submodules
//! (`break_pipeline`, `place_pipeline`, `rmb_use`, `containers`,
//! `mining_tick`, `combat`). `mod.rs` keeps the pure-data decision
//! layer (`AttackTarget` + `resolve_attack_target`, `RmbAction` +
//! `classify_rmb_action`) plus re-exports so existing callers don't
//! have to know about the split.
//!
//! Helper overview:
//! - W47.13a: `resolve_attack_target` - pick closest of pvp / mob /
//!   voxel. Pure data; no engine dependencies.
//! - W47.13c: `classify_rmb_action` - decide which RMB branch fires.
//!   Pure decision tree.
//! - W47.13d: `apply_block_break` - break pipeline (world set +
//!   chunk rebuild + drops + broadcast + SFX). See `break_pipeline`.
//! - W47.13e: `apply_block_place` - place pipeline (target resolve +
//!   AABB / support / occupied gates + torch wall swap + broadcast +
//!   container mint + held-stack decrement + SFX). See
//!   `place_pipeline`.
//! - W47.13f: `apply_shovel_grass_path` - grass -> dirt on
//!   shovel-RMB. See `break_pipeline`.
//! - W47.13g: `apply_flint_and_steel_ignite` - portal frame ignite
//!   or fire drop fallback. See `rmb_use`.
//! - W47.13h: `apply_bucket_use` - scoop a source or pour onto an
//!   air / replaceable cell (extinguishes fire). See `rmb_use`.
//! - W47.13i: `open_furnace`, `open_chest`, `try_sleep_bed` - the
//!   small UI-opener branches. See `containers`.
//! - W47.13j: `tick_mining` - LMB-held damage tick + session
//!   bookkeeping. See `mining_tick`.
//! - W47.13k: `apply_combat` - 3-stamina drain + PvP / mob attack
//!   dispatch. See `combat`.

mod break_pipeline;
mod combat;
mod containers;
mod mining_tick;
mod place_pipeline;
pub(crate) mod rail_place;
mod rmb_use;

pub(crate) use break_pipeline::{
    apply_block_break, apply_bonemeal, apply_hoe_till, apply_shovel_grass_path,
};
pub(crate) use combat::apply_combat;
pub(crate) use containers::{
    open_brewing_stand, open_chest, open_crafting_table, open_furnace, open_smithing_table,
    open_stonecutter,
    play_note_block, try_sleep_bed,
};
pub(crate) use mining_tick::tick_mining;
pub(crate) use place_pipeline::apply_block_place;
pub(crate) use rmb_use::{apply_bucket_use, apply_flint_and_steel_ignite};

/// Outcome of the per-frame combat-vs-block-edit arbitration.
///
/// At most one of `player` / `mob` is set; if `voxel_dist` was the
/// closest hit (or no candidate beat infinity) both are `None` and
/// the caller falls through to the block-break / place path.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AttackTarget {
    pub player: Option<u32>,
    pub mob: Option<u32>,
}

/// Pick whichever attack target is closest: player < mob < voxel.
///
/// The original main.rs sequence walked the three candidates in
/// (pvp, mob, voxel) order, each guarded by `d < best_dist`. The
/// voxel branch only resets the two combat outputs - it never sets
/// them - because a closer voxel means "let the block-edit path
/// handle it" rather than "attack the voxel".
pub fn resolve_attack_target(
    pvp_target: Option<(u32, f32)>,
    mob_target: Option<(u32, f32)>,
    voxel_dist: Option<f32>,
) -> AttackTarget {
    let mut best = f32::INFINITY;
    let mut player: Option<u32> = None;
    let mut mob: Option<u32> = None;
    if let Some((id, d)) = pvp_target
        && d < best
    {
        best = d;
        player = Some(id);
        mob = None;
    }
    if let Some((id, d)) = mob_target
        && d < best
    {
        best = d;
        mob = Some(id);
        player = None;
    }
    if let Some(d) = voxel_dist
        && d < best
    {
        mob = None;
        player = None;
    }
    AttackTarget { player, mob }
}

/// W47.13c: which RMB action wins for this click. The cascade in
/// `Voxel::on_update` walks these in priority order; the first
/// matching branch fires and the rest are suppressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RmbAction {
    /// No RMB this frame, or no condition matched.
    None,
    /// Shovel right-click on a grass voxel (id 3) - degrades to
    /// dirt (the MC "path" shortcut).
    PathGrass,
    /// W294: hoe right-click on dirt / grass - tills farmland.
    TillFarmland,
    /// W414: bone meal on a growable (crop / sapling / grass).
    ApplyBonemeal,
    /// Right-click on a furnace voxel - opens the furnace UI.
    OpenFurnace,
    /// Right-click on a chest voxel - opens the chest UI.
    OpenChest,
    /// W50.3: right-click on a crafting_table voxel - opens the
    /// 3x3 crafting grid UI.
    OpenCraftingTable,
    /// W50.5: right-click on a brewing_stand voxel - opens the
    /// 5-slot brewing UI.
    OpenBrewingStand,
    /// W171: right-click on a stonecutter - opens the cut-select UI.
    OpenStonecutter,
    /// W410: right-click on a smithing table - opens the upgrade UI.
    OpenSmithingTable,
    /// W61.2: right-click on a door / trapdoor - toggles open
    /// state.
    ToggleDoor,
    /// W67.1: right-click on a lever - toggles its redstone power
    /// state + re-solves the network.
    ToggleLever,
    /// W164: right-click on a note block - cycle its pitch one step
    /// (25 steps, F#3..F#5) and play the note.
    PlayNoteBlock,
    /// W338: flint+steel on a TNT block - swaps it for the primed
    /// entity (4 s fuse, then the power-4 blast).
    IgniteTnt,
    /// Right-click on a bed voxel (id 35) - tries to skip the
    /// night.
    SleepBed,
    /// Right-click with flint+steel - tries to ignite a portal
    /// frame or drop a fire on the clicked surface.
    IgnitePortal,
    /// Right-click with an empty/water/lava bucket - scoops or
    /// pours a liquid voxel.
    BucketUse,
    /// W134: right-click with the minecart item on a rail - spawns
    /// a cart entity riding that rail.
    PlaceMinecart,
    /// W145: right-click with a spawn egg - spawns the mob kind on
    /// the clicked surface (consumes one egg outside creative).
    SpawnEgg(u8),
    /// Right-click with a placeable block selected - places a new
    /// voxel at the adjacent (or replaceable) cell.
    PlaceBlock,
}

/// W47.13c: pick the winning RMB action.
///
/// `rmb` is the edge-triggered click; if false the result is
/// always `RmbAction::None`. The remaining inputs match what the
/// inline cascade reads from `self`:
/// - `target_id`: id of the voxel under the crosshair.
/// - `held_id`: id of the block in the selected hotbar slot,
///   `None` if the slot is empty.
/// - `held_shovel_tier`: `shovel_tier(held)` evaluated by the
///   caller. 0 if the held item is not a shovel.
/// - `held_is_tool_block`: `is_tool_block(held)` evaluated by the
///   caller. Tool / weapon blocks suppress the PlaceBlock branch.
/// - `may_build`: `self.abilities.may_build`. Adventure-mode
///   players have this off; PlaceBlock is then suppressed.
///
/// Priority order mirrors the cascade exactly: PathGrass beats
/// PlaceBlock when a shovel hits grass; the container/bed checks
/// only depend on the target voxel so they win whenever the held
/// item is irrelevant to them.
/// W61.3 / W125: trapdoor id check lives in `block::door` now
/// (shared with the collision + render paths).
fn is_trapdoor_block(id: u16) -> bool {
    crate::block::door::is_trapdoor_block(id)
}

pub fn classify_rmb_action(
    rmb: bool,
    target_id: u16,
    held_id: Option<u16>,
    held_shovel_tier: u8,
    held_is_tool_block: bool,
    may_build: bool,
) -> RmbAction {
    use crate::block::{
        BUCKET_EMPTY_ID, BUCKET_LAVA_ID, BUCKET_WATER_ID, CHEST_ID,
        FLINT_AND_STEEL_ID,
    };
    if !rmb {
        return RmbAction::None;
    }
    // 1. Shovel + grass -> path. Held must be a shovel (tier > 0)
    //    AND target must be the grass block.
    if target_id == 3 && held_shovel_tier > 0 {
        return RmbAction::PathGrass;
    }
    // 1b. W294: hoe + dirt/grass -> farmland (HoeItem TILLABLES).
    if (target_id == 2 || target_id == 3)
        && held_id
            .map(|h| crate::block::hoe_tier(crate::BlockId(h)) > 0)
            .unwrap_or(false)
    {
        return RmbAction::TillFarmland;
    }
    // 1c. W414: bone meal on a growable - immature crop, sapling,
    // or a grass block (MC BoneMealItem.useOn).
    if held_id == Some(crate::item::BONE_MEAL_ID)
        && (crate::world::crop::next_stage(target_id).is_some()
            || crate::block::is_sapling_block(target_id)
            || target_id == 3)
    {
        return RmbAction::ApplyBonemeal;
    }
    // 2. Containers + bed are target-only - the held block does
    //    not matter for these. A shovel on a furnace still opens
    //    the furnace.
    // W163: smoker + blast furnace share the furnace UI + tick.
    if crate::block::furnace::is_furnace_like(target_id) {
        return RmbAction::OpenFurnace;
    }
    // W169: barrel reuses the chest container store + UI.
    // W170: ender chest too (per-position store v1; MC's shared-across-
    // all-ender-chests inventory is a follow-up).
    if target_id == CHEST_ID
        || target_id == crate::block::BARREL_ID
        || target_id == crate::block::ENDER_CHEST_ID
    {
        return RmbAction::OpenChest;
    }
    // W50.3: crafting table opens the 3x3 grid UI (client-side
    // only, no host state).
    if target_id == crate::block::CRAFTING_TABLE_ID {
        return RmbAction::OpenCraftingTable;
    }
    // W50.5: brewing stand opens the 5-slot brewing UI.
    if target_id == crate::block::BREWING_STAND_ID {
        return RmbAction::OpenBrewingStand;
    }
    // W171: stonecutter opens the cut-select UI.
    if target_id == crate::block::STONECUTTER_ID {
        return RmbAction::OpenStonecutter;
    }
    // W410: smithing table opens the upgrade UI.
    if target_id == crate::block::SMITHING_TABLE_ID {
        return RmbAction::OpenSmithingTable;
    }
    // W61.2: door / trapdoor toggles open state.
    if crate::block::door::is_door_block(target_id)
        || is_trapdoor_block(target_id)
        // W167: fence gates toggle open/closed too.
        || crate::block::door::is_fence_gate_block(target_id)
    {
        return RmbAction::ToggleDoor;
    }
    // W366: every bed (legacy + 16 dyed) sleeps - the dyed beds
    // (template-placed) previously fell through to block placement,
    // the R20 "cannot sleep" report.
    if crate::block::is_bed_block(target_id) {
        return RmbAction::SleepBed;
    }
    // W67.1: lever toggles its redstone power state.
    if target_id == crate::block::LEVER_ID {
        return RmbAction::ToggleLever;
    }
    // W164: note block cycles pitch + plays the note.
    if target_id == crate::block::NOTE_BLOCK_ID {
        return RmbAction::PlayNoteBlock;
    }
    // 3. Held-item-driven branches: flint+steel and buckets.
    if let Some(h) = held_id {
        if h == FLINT_AND_STEEL_ID {
            // W338: flint+steel on a TNT block primes it instead of
            // hunting for a portal frame.
            if target_id == crate::block::TNT_ID {
                return RmbAction::IgniteTnt;
            }
            return RmbAction::IgnitePortal;
        }
        if matches!(h, BUCKET_EMPTY_ID | BUCKET_WATER_ID | BUCKET_LAVA_ID) {
            return RmbAction::BucketUse;
        }
        // W134: the minecart item only ever spawns a cart on a
        // rail; it must NEVER fall through to PlaceBlock (its id
        // is item-only - placing it would write a phantom block).
        if h == crate::minecart::MINECART_ITEM_ID {
            return if crate::block::rail::is_rail_block(target_id) {
                RmbAction::PlaceMinecart
            } else {
                RmbAction::None
            };
        }
        // W145: spawn eggs never place a block - they mint their
        // mob on the clicked surface (MC SpawnEggItem::useOn).
        if let Some(kind) = crate::item::spawn_egg_kind(h) {
            return RmbAction::SpawnEgg(kind);
        }
    }
    // 4. Generic block place. Suppressed in Adventure mode, with
    //    an empty hand, or while holding a tool/weapon.
    if may_build && held_id.is_some() && !held_is_tool_block {
        return RmbAction::PlaceBlock;
    }
    RmbAction::None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_candidates_returns_empty() {
        let r = resolve_attack_target(None, None, None);
        assert_eq!(r, AttackTarget::default());
    }

    #[test]
    fn voxel_only_blocks_combat() {
        let r = resolve_attack_target(None, None, Some(3.0));
        assert_eq!(r, AttackTarget::default());
    }

    #[test]
    fn pvp_wins_when_closest() {
        let r = resolve_attack_target(
            Some((7, 1.0)),
            Some((9, 2.0)),
            Some(3.0),
        );
        assert_eq!(r, AttackTarget { player: Some(7), mob: None });
    }

    #[test]
    fn mob_wins_when_closer_than_player() {
        let r = resolve_attack_target(
            Some((7, 4.0)),
            Some((9, 2.0)),
            Some(3.0),
        );
        assert_eq!(r, AttackTarget { player: None, mob: Some(9) });
    }

    #[test]
    fn voxel_closest_suppresses_pvp_and_mob() {
        let r = resolve_attack_target(
            Some((7, 4.0)),
            Some((9, 5.0)),
            Some(1.0),
        );
        assert_eq!(r, AttackTarget::default());
    }

    #[test]
    fn mob_only_when_no_pvp_and_no_voxel() {
        let r = resolve_attack_target(None, Some((9, 2.0)), None);
        assert_eq!(r, AttackTarget { player: None, mob: Some(9) });
    }

    // --- classify_rmb_action ---

    use crate::block::{
        BUCKET_EMPTY_ID, BUCKET_LAVA_ID, BUCKET_WATER_ID, CHEST_ID,
        FLINT_AND_STEEL_ID, FURNACE_ID,
    };

    #[test]
    fn rmb_false_always_none() {
        assert_eq!(
            classify_rmb_action(false, FURNACE_ID, Some(1), 0, false, true),
            RmbAction::None,
        );
    }

    #[test]
    fn shovel_on_grass_wins_path() {
        assert_eq!(
            classify_rmb_action(true, 3, Some(50), 2, false, true),
            RmbAction::PathGrass,
        );
    }

    #[test]
    fn non_shovel_on_grass_falls_through_to_place() {
        // Held is some non-shovel non-tool block (e.g. dirt id 2)
        // - cascade reaches the generic place branch.
        assert_eq!(
            classify_rmb_action(true, 3, Some(2), 0, false, true),
            RmbAction::PlaceBlock,
        );
    }

    #[test]
    fn furnace_target_opens_regardless_of_held() {
        assert_eq!(
            classify_rmb_action(true, FURNACE_ID, Some(50), 3, false, true),
            RmbAction::OpenFurnace,
        );
    }

    /// W163: smoker + blast furnace are furnace-family and route to
    /// the same OpenFurnace action (were dead blocks before).
    #[test]
    fn smoker_and_blast_furnace_open_furnace() {
        use crate::block::{BLAST_FURNACE_ID, SMOKER_ID};
        assert_eq!(
            classify_rmb_action(true, SMOKER_ID, None, 0, false, true),
            RmbAction::OpenFurnace,
        );
        assert_eq!(
            classify_rmb_action(true, BLAST_FURNACE_ID, None, 0, false, true),
            RmbAction::OpenFurnace,
        );
    }

    #[test]
    fn chest_target_opens_regardless_of_held() {
        assert_eq!(
            classify_rmb_action(true, CHEST_ID, None, 0, false, true),
            RmbAction::OpenChest,
        );
    }

    #[test]
    fn bed_target_routes_to_sleep() {
        // Legacy bed (35) and the dyed run (576..=591) all sleep -
        // W366: the dyed beds previously fell through to placement
        // (the R20 "cannot sleep in a village bed" report).
        assert_eq!(
            classify_rmb_action(true, 35, None, 0, false, true),
            RmbAction::SleepBed,
        );
        assert_eq!(
            classify_rmb_action(true, 580, None, 0, false, true),
            RmbAction::SleepBed,
        );
        assert_eq!(
            classify_rmb_action(true, 591, None, 0, false, true),
            RmbAction::SleepBed,
        );
    }

    #[test]
    fn flint_and_steel_routes_to_ignite() {
        assert_eq!(
            classify_rmb_action(true, 1, Some(FLINT_AND_STEEL_ID), 0, false, true),
            RmbAction::IgnitePortal,
        );
    }

    #[test]
    fn empty_bucket_routes_to_bucket_use() {
        assert_eq!(
            classify_rmb_action(true, 1, Some(BUCKET_EMPTY_ID), 0, false, true),
            RmbAction::BucketUse,
        );
    }

    #[test]
    fn water_bucket_routes_to_bucket_use() {
        assert_eq!(
            classify_rmb_action(true, 1, Some(BUCKET_WATER_ID), 0, false, true),
            RmbAction::BucketUse,
        );
    }

    #[test]
    fn lava_bucket_routes_to_bucket_use() {
        assert_eq!(
            classify_rmb_action(true, 1, Some(BUCKET_LAVA_ID), 0, false, true),
            RmbAction::BucketUse,
        );
    }

    #[test]
    fn empty_hand_blocks_place() {
        assert_eq!(
            classify_rmb_action(true, 1, None, 0, false, true),
            RmbAction::None,
        );
    }

    #[test]
    fn tool_block_blocks_place() {
        assert_eq!(
            classify_rmb_action(true, 1, Some(99), 0, true, true),
            RmbAction::None,
        );
    }

    #[test]
    fn may_build_false_blocks_place() {
        assert_eq!(
            classify_rmb_action(true, 1, Some(2), 0, false, false),
            RmbAction::None,
        );
    }

    #[test]
    fn place_block_with_regular_block() {
        assert_eq!(
            classify_rmb_action(true, 1, Some(2), 0, false, true),
            RmbAction::PlaceBlock,
        );
    }
}
