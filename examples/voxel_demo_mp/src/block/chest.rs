//! Chest block behaviour: per-position container state + the small
//! glue that ties placement / break / RMB-open into the existing
//! world interact chain.
//!
//! The 27-slot container itself lives in `Voxel::chests`, keyed by
//! absolute world voxel coords. This module owns the helpers that
//! mint a fresh container on place, drop the contents as item
//! entities on break, and translate a frame of `GridAction`s into
//! per-slot mutations during the open-UI loop.
//!
//! W467: the container family and the player inventory are both
//! `Option<ItemStack>`-slotted, so the apply helpers below move
//! full stacks - components (damage / enchants / name) travel with
//! every take / place / transfer, and merging anywhere requires
//! `same_stackable` (a worn tool never count-merges into a fresh
//! one). W468 closed the last edge: the break/spill path emits
//! full stacks too, so a worn tool spilled from a broken chest
//! lands on the floor still worn.

#![allow(dead_code)]

use crate::container::Container;
use crate::container::ui::GridAction;
use crate::item::ItemStack;

/// Number of slots in a single chest. Matches MC's small chest
/// (3 rows of 9). Larger / double-chests can change this per-chest
/// later without touching the save schema (slot count is recorded
/// per-instance).
pub(crate) const CHEST_SLOTS: usize = 27;

/// Build a fresh empty chest container. Used by the world-interact
/// place path when the player drops a chest block at a voxel that
/// doesn't yet have an entry in `Voxel::chests`.
pub(crate) fn new_chest() -> Container {
    Container::with_size(CHEST_SLOTS)
}

/// W128: world direction the EXT_POS double-chest half extends
/// toward, per facing (the axis PERPENDICULAR to the latch face).
/// Shared by the renderer and the pairing rule so the visual merge
/// and the 54-slot UI always agree.
pub(crate) fn ext_pos_dir(facing: u8) -> [i32; 3] {
    match facing % 4 {
        0 => [0, 0, 1],  // facing east -> +Z
        1 => [-1, 0, 0], // facing south -> -X
        2 => [0, 0, -1], // facing west -> -Z
        _ => [1, 0, 0],  // facing north -> +X
    }
}

/// W55.2b / W128 - find the partner chest for a double-chest pair.
/// MC rule: the adjacent chest along the axis PERPENDICULAR to the
/// facing, with the SAME facing, both having live containers. (The
/// old rule paired ANY adjacent chest, so the UI and the renderer
/// could disagree about what is a double chest.)
pub(crate) fn partner_for(
    world: &infengine_voxel::World,
    chests: &std::collections::HashMap<[i32; 3], Container>,
    facing: &std::collections::HashMap<[i32; 3], u8>,
    pos: [i32; 3],
) -> Option<[i32; 3]> {
    let f = facing.get(&pos).copied().unwrap_or(3);
    let ext = ext_pos_dir(f);
    for dir in [ext, [-ext[0], 0, -ext[2]]] {
        let p = [pos[0] + dir[0], pos[1], pos[2] + dir[2]];
        if world.get(p).0 != crate::block::CHEST_ID {
            continue;
        }
        if !chests.contains_key(&p) {
            continue;
        }
        if facing.get(&p).copied().unwrap_or(3) != f {
            continue;
        }
        return Some(p);
    }
    None
}

/// W472: canonical PRIMARY half of a double-chest pair - the half
/// whose partner sits toward `+ext_pos_dir(facing)`. MC's first
/// container is the `ChestType.RIGHT` half, a placement-order
/// blockstate this engine doesn't persist; the geometric rule here
/// is the deterministic stand-in (stable across sessions - facing
/// is persisted, and both halves agree on it). The primary keys
/// the 54-slot menu: slots 0..26 = primary, 27..53 = partner.
pub(crate) fn double_primary(
    pos: [i32; 3],
    partner: [i32; 3],
    facing: u8,
) -> [i32; 3] {
    let ext = ext_pos_dir(facing);
    if partner == [pos[0] + ext[0], pos[1], pos[2] + ext[2]] {
        pos
    } else {
        partner
    }
}

/// W472: canonicalise an arbitrary half of a (possibly) double
/// chest to `(primary, Option<secondary>)`. Single chests return
/// `(pos, None)`.
pub(crate) fn canonical_pair(
    world: &infengine_voxel::World,
    chests: &std::collections::HashMap<[i32; 3], Container>,
    facing: &std::collections::HashMap<[i32; 3], u8>,
    pos: [i32; 3],
) -> ([i32; 3], Option<[i32; 3]>) {
    match partner_for(world, chests, facing, pos) {
        Some(partner) => {
            let f = facing.get(&pos).copied().unwrap_or(3);
            let primary = double_primary(pos, partner, f);
            let secondary = if primary == pos { partner } else { pos };
            (primary, Some(secondary))
        }
        None => (pos, None),
    }
}

/// W472: stitch the two per-position 27-slot containers into ONE
/// 54-slot menu view (MC `CompoundContainer(first, second)`).
/// Plain value copy - the per-position storage / save format stays
/// untouched; `unstitch_double` writes the mutated view back.
pub(crate) fn stitch_double(
    first: &Container,
    second: &Container,
) -> Container {
    let mut c = Container::with_size(first.len() + second.len());
    for i in 0..first.len() {
        c.set_slot(i, first.slot(i));
    }
    for i in 0..second.len() {
        c.set_slot(first.len() + i, second.slot(i));
    }
    c
}

/// W472: working MENU view of the chest keyed by `primary`:
/// the plain 27-slot container for a single, the 54-slot stitched
/// `CompoundContainer` for a double. `None` when the primary has
/// no container entry.
pub(crate) fn menu_view(
    chests: &std::collections::HashMap<[i32; 3], Container>,
    primary: [i32; 3],
    secondary: Option<[i32; 3]>,
) -> Option<Container> {
    let first = chests.get(&primary)?;
    match secondary.and_then(|s| chests.get(&s)) {
        Some(second) => Some(stitch_double(first, second)),
        None => Some(first.clone()),
    }
}

/// W472: write a mutated menu view back into per-position storage
/// (both halves for a double). Inverse of `menu_view`.
pub(crate) fn menu_writeback(
    chests: &mut std::collections::HashMap<[i32; 3], Container>,
    primary: [i32; 3],
    secondary: Option<[i32; 3]>,
    view: &Container,
) {
    match secondary {
        Some(sec) => {
            // Both halves must exist (menu_view built from them);
            // guard anyway so a half broken mid-session degrades
            // to a lossy no-op instead of a panic.
            if let Some(first) = chests.get_mut(&primary) {
                for i in 0..first.len().min(view.len()) {
                    first.set_slot(i, view.slot(i));
                }
            }
            let first_len = chests.get(&primary).map(|c| c.len()).unwrap_or(0);
            if let Some(second) = chests.get_mut(&sec) {
                for i in 0..second.len() {
                    second.set_slot(i, view.slot(first_len + i));
                }
            }
        }
        None => {
            if let Some(first) = chests.get_mut(&primary) {
                for i in 0..first.len() {
                    first.set_slot(i, view.slot(i));
                }
            }
        }
    }
}

/// Collect a chest's contents into the drop list emitted on break.
/// W468: one FULL stack per occupied slot - counts + components
/// spill intact (the caller spawns each via `drops::spawn_stack`).
/// Returns an empty vec for an empty chest.
pub(crate) fn drain_drops(chest: &Container) -> Vec<ItemStack> {
    chest.slots().iter().flatten().cloned().collect()
}

/// Recover a cursor-held stack into the chest when the UI closes.
/// Returns the leftover stack the caller should spawn as a floor
/// drop at the chest position - whatever the chest couldn't absorb
/// (e.g. full chest, missing chest). Both the recovered part and
/// the spilled leftover keep the held stack's components (W468).
///
/// Pulled out as a free fn so the close path stays trivially
/// testable; the runtime caller wraps it with the chest lookup +
/// the drop spawn.
pub(crate) fn recover_held_on_close(
    chest: Option<&mut Container>,
    held: Option<ItemStack>,
) -> Option<ItemStack> {
    let stack = held?;
    let leftover = match chest {
        Some(c) => c.insert(stack.clone()),
        None => stack.count,
    };
    (leftover > 0).then(|| stack.with_count(leftover))
}

/// W470: apply one `GridAction` against the chest-side section of
/// the [chest, player inventory] screen. Thin adapter over the
/// SHARED click core (`container::click` via `container::screen`) -
/// the interaction rules live exactly once. Returns any thrown
/// stacks (Q / Ctrl+Q) for the caller to spawn as drops.
pub(crate) fn apply_chest_action(
    chest: &mut Container,
    partner: &mut [Option<ItemStack>],
    held: &mut Option<ItemStack>,
    action: GridAction,
) -> Vec<ItemStack> {
    crate::container::screen::apply_grid_action(
        chest,
        partner,
        held,
        false,
        action,
        &crate::container::screen::CHEST_CFG,
        false,
    )
}

/// W470: player-inventory-side counterpart of `apply_chest_action`
/// (same shared core, `player_side = true`).
pub(crate) fn apply_player_action(
    inv: &mut [Option<ItemStack>],
    chest: &mut Container,
    held: &mut Option<ItemStack>,
    action: GridAction,
) -> Vec<ItemStack> {
    crate::container::screen::apply_grid_action(
        chest,
        inv,
        held,
        true,
        action,
        &crate::container::screen::CHEST_CFG,
        false,
    )
}

#[cfg(test)]
#[path = "chest_tests.rs"]
mod chest_tests;
