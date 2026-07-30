//! Creative-flavoured slash commands: inventory edits, item
//! grants, builder shortcuts, time-freeze toggle.
//!
//! Each fn takes `(voxel, arg, lines)` and returns whether the
//! command was handled.

use infengine_voxel::{BlockId, CHUNK_SIZE, ChunkCoord};

use crate::Voxel;
use crate::block::{BLOCK_TYPES, is_tool_block};
use crate::inventory::{HOTBAR_SLOTS, inv_pickup};
use crate::net::{EditMsg, NetRole};

/// `/resetinventory` (alias `/resetinv`). Restore the curated
/// starter bundle.
pub(crate) fn cmd_resetinventory(
    voxel: &mut Voxel,
    _arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // Restore the curated starter bundle
    // (wood tools + basic blocks). Slot
    // 0 + storage stay empty.
    for s in voxel.inventory.iter_mut() {
        *s = None;
    }
    let seed: &[(u16, u8)] = &[
        (16, 1),
        (23, 1),
        (29, 1),
        (32, 1),
        (2, 16),
        (7, 16),
        (8, 16),
        (10, 4),
    ];
    for (i, (bid, count)) in seed.iter().enumerate() {
        let slot = i + 1;
        if slot < HOTBAR_SLOTS {
            voxel.inventory[slot] =
                Some(crate::item::ItemStack::new(*bid, *count));
        }
    }
    for s in voxel.craft_grid.iter_mut() {
        *s = None;
    }
    lines.push("Inventory reset to starter.".to_string());
    true
}

/// `/clearinventory` (alias `/clearinv`). Empty every slot
/// + the crafting grid.
pub(crate) fn cmd_clearinventory(
    voxel: &mut Voxel,
    _arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // Empty every slot including hotbar
    // + craft grid. Useful for testing.
    // In MP this only zeroes the local
    // predicted view; the next vx.inv
    // snapshot from the host wipes it.
    for s in voxel.inventory.iter_mut() {
        *s = None;
    }
    for s in voxel.craft_grid.iter_mut() {
        *s = None;
    }
    lines.push("Inventory cleared.".to_string());
    true
}

/// `/freezetime` (alias `/freeze`). Pause / resume the day
/// clock.
pub(crate) fn cmd_freezetime(
    voxel: &mut Voxel,
    _arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    voxel.freeze_time = !voxel.freeze_time;
    lines.push(format!(
        "Time clock {}",
        if voxel.freeze_time { "PAUSED" } else { "RUNNING" }
    ));
    true
}

/// `/give <block> [count]`. Drop stacks of a named block into
/// the local inventory.
pub(crate) fn cmd_give(
    voxel: &mut Voxel,
    arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // Add stacks of a named block to the
    // local inventory. In MP this only
    // affects the host's authoritative
    // copy; a connected client running
    // /give just edits their predicted
    // view (the next vx.inv snapshot
    // wipes it). Mostly useful for
    // testing + creative play.
    if arg.is_empty() {
        lines.push("Usage: /give <block> [count]".to_string());
        return false;
    }
    let mut parts = arg.split_whitespace();
    let block_name = parts.next().unwrap_or("");
    let count: u32 = parts
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(64);
    let block_id = BLOCK_TYPES
        .iter()
        .find(|bt| bt.name.eq_ignore_ascii_case(block_name))
        .map(|bt| bt.id)
        // W136: item-only entries (minecart, ...) have no
        // BLOCK_TYPES row; /give hands those out via ITEM_TYPES
        // (one id space, the inventory stores them the same way).
        .or_else(|| {
            crate::item::ITEM_TYPES
                .iter()
                .find(|it| it.name.eq_ignore_ascii_case(block_name))
                .map(|it| it.id)
        })
        // W346: no exact hit -> try a SUBSTRING match. A unique hit
        // resolves ("/give diamond_p" works); several hits list the
        // candidates instead of a bare "unknown".
        .or_else(|| {
            let needle = block_name.to_ascii_lowercase();
            let mut hits: Vec<(&str, u16)> = BLOCK_TYPES
                .iter()
                .filter(|bt| bt.name.contains(&needle))
                .map(|bt| (bt.name, bt.id))
                .collect();
            for it in crate::item::ITEM_TYPES.iter() {
                if it.name.contains(&needle)
                    && !hits.iter().any(|(n, _)| *n == it.name)
                {
                    hits.push((it.name, it.id));
                }
            }
            match hits.len() {
                1 => Some(hits[0].1),
                0 => None,
                n => {
                    let mut names: Vec<&str> =
                        hits.iter().take(8).map(|(n, _)| *n).collect();
                    names.sort_unstable();
                    lines.push(format!(
                        "{n} matches: {}{}",
                        names.join(", "),
                        if n > 8 { ", ..." } else { "" },
                    ));
                    None
                }
            }
        });
    if let Some(id) = block_id {
        let bid = BlockId(id);
        let mut added = 0u32;
        while added < count {
            let before = voxel
                .inventory
                .iter()
                .filter_map(|s| s.as_ref().map(|st| st.count as u32))
                .sum::<u32>();
            voxel.pickup(bid);
            let after = voxel
                .inventory
                .iter()
                .filter_map(|s| s.as_ref().map(|st| st.count as u32))
                .sum::<u32>();
            if after <= before {
                // Inventory was
                // full; stop early so
                // we don't infinite
                // loop.
                break;
            }
            added += 1;
        }
        lines.push(format!("Gave {added} x {block_name}."));
    } else {
        lines.push(format!("Unknown block/item: {block_name}"));
    }
    true
}

/// `/tool wood|stone|iron|sword_*|shovel_*|axe_*`. Quick item
/// grant shortcut.
pub(crate) fn cmd_tool(
    voxel: &mut Voxel,
    arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // Quick pickaxe grant: /tool wood|stone|iron.
    // Equivalent to /give pickaxe_<tier> 1 but
    // shorter to type.
    let id_opt: Option<u16> = match arg.to_lowercase().as_str()
    {
        "wood" | "pickaxe_wood" => Some(16),
        "stone" | "pickaxe_stone" => Some(17),
        "iron" | "pickaxe_iron" => Some(18),
        "sword_wood" => Some(23),
        "sword_stone" => Some(24),
        "sword_iron" => Some(25),
        "shovel_wood" => Some(29),
        "shovel_stone" => Some(30),
        "shovel_iron" => Some(31),
        "axe_wood" => Some(32),
        "axe_stone" => Some(33),
        "axe_iron" => Some(34),
        _ => None,
    };
    match id_opt {
        None => lines.push(
            "Usage: /tool wood|stone|iron|sword_wood|sword_stone|sword_iron"
                .to_string(),
        ),
        Some(id) => {
            voxel.pickup(BlockId(id));
            lines.push(format!("Granted {arg} pickaxe."));
        }
    }
    true
}

/// `/restock`. Host-only: drop one stack of every non-tool
/// block into every connected client's inventory + the host's.
pub(crate) fn cmd_restock(
    voxel: &mut Voxel,
    _arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // Host-only. Drops a stack of every
    // non-tool block into every connected
    // client's inventory + the host's
    // own. Useful for co-op MP starts.
    if !matches!(voxel.net, NetRole::Server { .. }) {
        lines.push("/restock is host-only.".to_string());
        return true;
    }
    // Self first.
    for bt in BLOCK_TYPES.iter() {
        if is_tool_block(BlockId(bt.id)) {
            continue;
        }
        for _ in 0..16 {
            voxel.pickup(BlockId(bt.id));
        }
    }
    // Each client.
    let mut touched = 0u32;
    if let NetRole::Server {
        inventories,
        pending_inv_sends,
        ..
    } = &mut voxel.net
    {
        for (cid, inv) in inventories.iter_mut() {
            for bt in BLOCK_TYPES.iter() {
                if is_tool_block(BlockId(bt.id)) {
                    continue;
                }
                for _ in 0..16 {
                    inv_pickup(
                        &mut inv.inventory,
                        BlockId(bt.id),
                    );
                }
            }
            pending_inv_sends.insert(*cid);
            touched += 1;
        }
    }
    lines.push(format!(
        "Restocked self + {touched} clients."
    ));
    true
}

/// `/blob <radius> <block>`. Host-only: fill a sphere of voxels
/// around the player with the named block.
pub(crate) fn cmd_blob(
    voxel: &mut Voxel,
    arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // Host-only: fill a sphere of voxels
    // around the player with the named
    // block. Usage: /blob <radius> <block>.
    // Hard radius cap at 12 so a typo
    // doesn't lock the host.
    if !matches!(
        voxel.net,
        NetRole::Server { .. } | NetRole::Offline
    ) {
        lines.push("/blob is host-only.".to_string());
        return true;
    }
    let mut parts = arg.split_whitespace();
    let r_str = parts.next().unwrap_or("");
    let block_name = parts.next().unwrap_or("");
    let r_opt: Option<i32> = r_str.parse().ok();
    let bid_opt: Option<u16> =
        if block_name.eq_ignore_ascii_case("air") {
            Some(0)
        } else {
            BLOCK_TYPES
                .iter()
                .find(|bt| {
                    bt.name.eq_ignore_ascii_case(block_name)
                })
                .map(|bt| bt.id)
        };
    match (r_opt, bid_opt) {
        (None, _) | (_, None) => {
            lines.push(
                "Usage: /blob <radius> <block>".to_string(),
            );
        }
        (Some(r), Some(bid)) => {
            let radius = r.clamp(1, 12);
            let block = BlockId(bid);
            let center = [
                voxel.player_pos.x.floor() as i32,
                voxel.player_pos.y.floor() as i32,
                voxel.player_pos.z.floor() as i32,
            ];
            let r2 = radius * radius;
            let mut count = 0u32;
            let cfg = bincode::config::standard();
            for dy in -radius..=radius {
                for dz in -radius..=radius {
                    for dx in -radius..=radius {
                        if dx * dx + dy * dy + dz * dz > r2 {
                            continue;
                        }
                        let v = [
                            center[0] + dx,
                            center[1] + dy,
                            center[2] + dz,
                        ];
                        if v[1] < 1 {
                            continue;
                        }
                        voxel.world_set_tracked(v, block);
                        count += 1;
                        let edit = EditMsg {
                            x: v[0],
                            y: v[1],
                            z: v[2],
                            block: bid,
                        };
                        if let NetRole::Server { server, .. } =
                            &mut voxel.net
                        {
                            voxel.edit_history.push(edit);
                            if let Ok(buf) =
                                bincode::serde::encode_to_vec(
                                    edit, cfg,
                                )
                            {
                                server.broadcast_bytes(
                                    "vx.edit", buf,
                                );
                            }
                        }
                    }
                }
            }
            let cs = CHUNK_SIZE as i32;
            let mn = [
                (center[0] - radius - 1).div_euclid(cs),
                (center[1] - radius - 1).div_euclid(cs),
                (center[2] - radius - 1).div_euclid(cs),
            ];
            let mx = [
                (center[0] + radius + 1).div_euclid(cs),
                (center[1] + radius + 1).div_euclid(cs),
                (center[2] + radius + 1).div_euclid(cs),
            ];
            let mut coords: Vec<ChunkCoord> = Vec::new();
            for cz in mn[2]..=mx[2] {
                for cy in mn[1]..=mx[1] {
                    for cx in mn[0]..=mx[0] {
                        coords.push([cx, cy, cz]);
                    }
                }
            }
            voxel.rebuild_chunks(&coords);
            lines.push(format!(
                "Placed {count} {block_name} (r={radius})."
            ));
        }
    }
    true
}
