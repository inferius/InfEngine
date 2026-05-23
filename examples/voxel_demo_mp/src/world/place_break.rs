//! W47.13 - place / break decision helpers extracted from `on_update`.
//!
//! The block-edit path inside `Voxel::on_update` is a ~1100 LOC
//! sequence with deep `&mut self` borrows + side effects (chunk
//! rebuilds, edit broadcasts, particle spawns, inventory writes).
//! Extracting the entire flow at once is high-risk; this module
//! instead grows piece-by-piece with pure free functions that the
//! main loop calls. Each helper takes only the fields it needs so
//! the caller can keep other borrows alive.
//!
//! W47.13a: `resolve_attack_target` - given the three candidate hits
//! (remote player, mob, voxel), return whichever wins by closest
//! distance. Pure data; no engine dependencies.
//!
//! W47.13c: `classify_rmb_action` - given the held block, target
//! block id, and ability flags, return which RMB branch should fire.
//! Replaces the inline `else if rmb && self.world.get(...) == X`
//! conditions in the main on_update cascade. Branch bodies (side
//! effects) stay in the caller.
//!
//! W47.13d: `apply_block_break` - the full break-edit pipeline
//!     (world set + chunk rebuild + particles + edit broadcast + drops
//!     + container drain + dislodge cascade + break SFX). Takes `&mut
//!     Voxel` because the body touches ~15 fields; the win is that the
//!     130-line block stops sitting inline in `on_update`.

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
    /// Right-click on a furnace voxel - opens the furnace UI.
    OpenFurnace,
    /// Right-click on a chest voxel - opens the chest UI.
    OpenChest,
    /// Right-click on a bed voxel (id 35) - tries to skip the
    /// night.
    SleepBed,
    /// Right-click with flint+steel - tries to ignite a portal
    /// frame or drop a fire on the clicked surface.
    IgnitePortal,
    /// Right-click with an empty/water/lava bucket - scoops or
    /// pours a liquid voxel.
    BucketUse,
    /// Right-click with a placeable block selected - places a new
    /// voxel at the adjacent (or replaceable) cell.
    PlaceBlock,
}

/// Bed block id. There is no const in `block::ids` yet; magic
/// number documented here so the classifier stays self-contained.
const BED_ID: u16 = 35;

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
        FLINT_AND_STEEL_ID, FURNACE_ID,
    };
    if !rmb {
        return RmbAction::None;
    }
    // 1. Shovel + grass -> path. Held must be a shovel (tier > 0)
    //    AND target must be the grass block.
    if target_id == 3 && held_shovel_tier > 0 {
        return RmbAction::PathGrass;
    }
    // 2. Containers + bed are target-only - the held block does
    //    not matter for these. A shovel on a furnace still opens
    //    the furnace.
    if target_id == FURNACE_ID {
        return RmbAction::OpenFurnace;
    }
    if target_id == CHEST_ID {
        return RmbAction::OpenChest;
    }
    if target_id == BED_ID {
        return RmbAction::SleepBed;
    }
    // 3. Held-item-driven branches: flint+steel and buckets.
    if let Some(h) = held_id {
        if h == FLINT_AND_STEEL_ID {
            return RmbAction::IgnitePortal;
        }
        if matches!(h, BUCKET_EMPTY_ID | BUCKET_WATER_ID | BUCKET_LAVA_ID) {
            return RmbAction::BucketUse;
        }
    }
    // 4. Generic block place. Suppressed in Adventure mode, with
    //    an empty hand, or while holding a tool/weapon.
    if may_build && held_id.is_some() && !held_is_tool_block {
        return RmbAction::PlaceBlock;
    }
    RmbAction::None
}

/// W47.13d: apply the final block-break edit for `voxel`.
///
/// Called once when `mine_break == Some(voxel)` (mining session
/// completed) and any panel guards have already passed in the
/// caller. Side effects:
/// - sets the voxel to AIR (locally) + rebuilds affected chunks
/// - sprays break particles tinted to the broken block's albedo
/// - broadcasts the edit on Server / queues it on Client
/// - on host: spawns the item drop (suppressed in instabuild or
///   when held-tool-tier < required), drains chest / furnace
///   contents as drops, dislodges any decorations losing support
/// - increments `blocks_broken` counter
/// - plays the final break SFX
///
/// `is_client` mirrors the caller's `matches!(self.net,
/// NetRole::Client { .. })`; passed in to keep the helper from
/// re-matching on `self.net` twice.
pub(crate) fn apply_block_break(
    vx: &mut crate::Voxel,
    voxel: [i32; 3],
    is_client: bool,
) {
    use crate::net::NetRole;
    use crate::{
        BlockId, CHEST_ID, EditMsg, FURNACE_ID, REDSTONE_TORCH_ID,
        REDSTONE_WALL_TORCH_ID, SOUL_TORCH_ID, SOUL_WALL_TORCH_ID, Voxel,
        block, block_material, block_required_tier, tool_tier,
    };
    let _ = std::marker::PhantomData::<Voxel>;
    // Capture the block we're about to break so we can
    // pop it into the inventory (greedy collection - air just no-ops).
    let broken = vx.world.get(voxel);
    // Tint the debris with the broken block's albedo.
    let break_color = vx
        .registry
        .get(broken)
        .map(|d| d.color)
        .unwrap_or([0.75, 0.55, 0.30]);
    let edit = EditMsg {
        x: voxel[0],
        y: voxel[1],
        z: voxel[2],
        block: 0,
    };
    if is_client {
        vx.world_set_tracked(voxel, BlockId::AIR);
        let coords = Voxel::affected_chunks(voxel);
        vx.rebuild_chunks(&coords);
        vx.spawn_break_particles_tinted(voxel, break_color);
        if let NetRole::Client { pending_edits, .. } = &mut vx.net {
            pending_edits.push(edit);
        }
    } else {
        vx.world_set_tracked(voxel, BlockId::AIR);
        let coords = Voxel::affected_chunks(voxel);
        vx.rebuild_chunks(&coords);
        vx.spawn_break_particles_tinted(voxel, break_color);
        if let NetRole::Server { server, .. } = &vx.net {
            let cfg = bincode::config::standard();
            if let Ok(buf) = bincode::serde::encode_to_vec(edit, cfg) {
                server.broadcast_bytes("vx.edit", buf);
            }
        }
        vx.edit_history.push(edit);
    }
    if !is_client && broken != BlockId::AIR {
        // W10.2: Creative `instabuild` suppresses block drops.
        // Container contents (chest, furnace) still drop below.
        let suppress_drops = vx.abilities.instabuild;
        // Tool gating: drop only when the selected slot's tool tier
        // meets the block's requirement.
        let held = vx
            .inventory
            .get(vx.selected_slot)
            .copied()
            .unwrap_or(None)
            .map(|(b, _)| b)
            .unwrap_or(BlockId::AIR);
        let tier = tool_tier(held);
        let needed = block_required_tier(broken);
        if !suppress_drops && tier >= needed {
            // W35.2: wall-torch variants drop the floor-torch item.
            let drop_id = match broken.0 {
                x if x == REDSTONE_WALL_TORCH_ID => REDSTONE_TORCH_ID,
                x if x == SOUL_WALL_TORCH_ID => SOUL_TORCH_ID,
                _ => broken.0,
            };
            vx.spawn_drop(BlockId(drop_id), voxel);
        }
        // Chest break: drain contents as drops, drop the container.
        if broken.0 == CHEST_ID
            && let Some(chest_inv) = vx.chests.remove(&voxel)
        {
            for id in block::chest::drain_drops(&chest_inv) {
                vx.spawn_drop(BlockId(id), voxel);
            }
        }
        // Furnace break: drain 3 slots + drop the runtime entry.
        if broken.0 == FURNACE_ID {
            let drops = block::furnace::break_furnace(
                &mut vx.furnaces,
                &mut vx.furnace_runtimes,
                voxel,
            );
            for id in drops {
                vx.spawn_drop(BlockId(id), voxel);
            }
        }
        // Cascade: dislodge any F_NO_PLACE decoration whose support
        // we just removed.
        vx.dislodge_unsupported_decorations(voxel);
    }
    if broken != BlockId::AIR {
        vx.blocks_broken = vx.blocks_broken.saturating_add(1);
    }
    // Final break SFX.
    let voxel_center = [
        voxel[0] as f32 + 0.5,
        voxel[1] as f32 + 0.5,
        voxel[2] as f32 + 0.5,
    ];
    let key = format!("dig:{}", block_material(broken));
    let freq = 220.0 + (broken.0 as f32) * 28.0;
    vx.sfx_at(&key, voxel_center, move |a, g| {
        let _ = a.play_thud(freq, 0.30 * g);
    });
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

    #[test]
    fn chest_target_opens_regardless_of_held() {
        assert_eq!(
            classify_rmb_action(true, CHEST_ID, None, 0, false, true),
            RmbAction::OpenChest,
        );
    }

    #[test]
    fn bed_target_routes_to_sleep() {
        assert_eq!(
            classify_rmb_action(true, BED_ID, None, 0, false, true),
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
