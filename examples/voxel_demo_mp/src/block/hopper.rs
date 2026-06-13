//! W172 - hopper item transport.
//!
//! A hopper is a small container that, on a fixed cadence, pulls one
//! item from the container directly ABOVE it and pushes one item to
//! the container directly BELOW it. This is the classic
//! redstone-adjacent transport mechanic, and it needs NO new data
//! model: it just moves `(item_id, count)` stacks between the
//! `Container`s the engine already stores in `Voxel::chests` (chests,
//! barrels and hoppers all live there).
//!
//! v1 scope: vertical transport between chest/barrel/hopper. Furnace
//! input/output coupling, sideways-facing hoppers and the manual
//! 5-slot UI are documented follow-ups.

use std::collections::HashMap;

use crate::container::Container;

/// Hopper buffer size (MC = 5 slots).
pub(crate) const HOPPER_SLOTS: usize = 5;

/// MC moves one item every 8 game ticks (~0.4 s). We accumulate wall
/// clock so the cadence is frame-rate independent.
pub(crate) const HOPPER_INTERVAL_MS: u32 = 400;

/// Remove one item from the first non-empty slot. Returns `(id, 1)`
/// or `None` when every slot is empty.
fn take_one(c: &mut Container) -> Option<(u16, u8)> {
    for i in 0..c.len() {
        if let Some((id, n)) = c.slot(i) {
            let left = n - 1;
            c.set_slot(i, if left == 0 { None } else { Some((id, left)) });
            return Some((id, 1));
        }
    }
    None
}

/// Insert one item; returns true when it found a home (0 leftover).
fn insert_one(c: &mut Container, stack: (u16, u8)) -> bool {
    c.insert(stack) == 0
}

/// Run one transport round for every hopper in `containers` whose
/// world block id is `HOPPER_ID`. `is_container` answers whether a
/// world position holds a pullable/pushable container block (the
/// caller wires it to the chest/barrel/hopper test). The container
/// store is the single `chests` map; sources / dests must already
/// have an entry (minted on place) to take part.
pub(crate) fn transport_round(
    containers: &mut HashMap<[i32; 3], Container>,
    hopper_positions: &[[i32; 3]],
) {
    for &hp in hopper_positions {
        let above = [hp[0], hp[1] + 1, hp[2]];
        let below = [hp[0], hp[1] - 1, hp[2]];
        // Pull one from above into the hopper.
        if containers.contains_key(&above)
            && let Some(item) = containers.get_mut(&above).and_then(take_one)
        {
            let placed =
                containers.get_mut(&hp).map(|h| insert_one(h, item)).unwrap_or(false);
            if !placed
                && let Some(a) = containers.get_mut(&above)
            {
                a.insert(item); // hopper full - put it back
            }
        }
        // Push one from the hopper into the container below.
        if containers.contains_key(&below)
            && let Some(item) = containers.get_mut(&hp).and_then(take_one)
        {
            let placed =
                containers.get_mut(&below).map(|b| insert_one(b, item)).unwrap_or(false);
            if !placed
                && let Some(h) = containers.get_mut(&hp)
            {
                h.insert(item); // dest full - put it back
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chest_with(items: &[(u16, u8)]) -> Container {
        let mut c = Container::with_size(27);
        for (i, &it) in items.iter().enumerate() {
            c.set_slot(i, Some(it));
        }
        c
    }

    /// A hopper pulls from the chest above and pushes to the chest
    /// below, one item per round.
    #[test]
    fn moves_one_item_top_to_bottom() {
        let mut map: HashMap<[i32; 3], Container> = HashMap::new();
        map.insert([0, 1, 0], chest_with(&[(5, 10)])); // above: 10 logs
        map.insert([0, 0, 0], Container::with_size(HOPPER_SLOTS)); // hopper
        map.insert([0, -1, 0], chest_with(&[])); // below: empty

        transport_round(&mut map, &[[0, 0, 0]]);

        // One pulled from above (10 -> 9); one pushed to below (0 -> 1).
        assert_eq!(map[&[0, 1, 0]].slot(0), Some((5, 9)));
        // The hopper either holds it briefly or it already moved down;
        // total conserved across the three containers = 10.
        let total: u32 = [[0, 1, 0], [0, 0, 0], [0, -1, 0]]
            .iter()
            .map(|p| {
                (0..map[p].len())
                    .filter_map(|i| map[p].slot(i))
                    .map(|(_, n)| n as u32)
                    .sum::<u32>()
            })
            .sum();
        assert_eq!(total, 10, "no items created or destroyed");
    }

    /// No container below -> the hopper keeps what it pulled (no loss).
    #[test]
    fn no_dest_keeps_item() {
        let mut map: HashMap<[i32; 3], Container> = HashMap::new();
        map.insert([0, 1, 0], chest_with(&[(5, 1)]));
        map.insert([0, 0, 0], Container::with_size(HOPPER_SLOTS));
        // no [0,-1,0]
        transport_round(&mut map, &[[0, 0, 0]]);
        let above: u32 = (0..map[&[0, 1, 0]].len())
            .filter_map(|i| map[&[0, 1, 0]].slot(i))
            .map(|(_, n)| n as u32)
            .sum();
        let hop: u32 = (0..map[&[0, 0, 0]].len())
            .filter_map(|i| map[&[0, 0, 0]].slot(i))
            .map(|(_, n)| n as u32)
            .sum();
        assert_eq!(above + hop, 1, "item conserved, just relocated");
    }
}
