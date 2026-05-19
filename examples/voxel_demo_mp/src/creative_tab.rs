//! W10.6: Creative-mode inventory tab.
//!
//! Mirrors MC's `CreativeModeInventoryScreen.java` at a coarse
//! grain - a single searchable grid of every registered item +
//! the 9-slot hotbar at the bottom. Multiple categorised tabs
//! (Blocks / Items / Tools / Food) are a follow-up; for Wave 10
//! we ship a single combined view with a text-search filter as
//! the headline filter.
//!
//! The module is INPUT + STATE only. The egui rendering itself
//! lives inline in `main.rs::render_ui` so the rest of the HUD
//! plumbing (egui frame, atlas texture id, drag&drop helpers)
//! is on hand. This file owns:
//!
//! - `CreativeTabState`: the open / closed flag, the live search
//!   query, the cached filter result, and the cursor stack.
//! - `available_items()`: the catalog. Built by walking
//!   `item::ITEM_TYPES` once at panel open time + filtered on
//!   every search-string change.
//! - `filter_items()`: case-insensitive substring match against
//!   the display name. Pure function - easy to unit test.

use infengine_voxel::BlockId;

use crate::item::{ITEM_TYPES, ItemType, item_display_name};

/// One entry in the creative tab grid. Carries everything the
/// UI needs to render + react to clicks without hopping back to
/// the item registry.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CreativeEntry {
    /// Item id - written into the inventory slot on click.
    pub id: u16,
    /// `max_stack` for the picked-up stack. Clicking on a tile
    /// fills the cursor with a stack of this size.
    pub max_stack: u8,
}

impl CreativeEntry {
    /// Build a fresh entry from an `ItemType` row.
    pub(crate) fn from_item(it: &'static ItemType) -> Self {
        Self {
            id: it.id,
            max_stack: it.max_stack,
        }
    }
}

/// Open / closed state + live search filter + result cache.
/// `Default` opens the panel closed with an empty filter.
#[derive(Debug, Default, Clone)]
pub(crate) struct CreativeTabState {
    /// True when the player has the tab open (E pressed in
    /// Creative). False otherwise.
    pub open: bool,
    /// Live search query. Lowercased substring match against the
    /// item display name. Empty string = show everything.
    pub query: String,
    /// Currently held stack (cursor). Populated by left-clicking
    /// a grid tile; deposited by clicking a hotbar slot. None
    /// means the cursor is empty.
    pub cursor: Option<(BlockId, u8)>,
}

impl CreativeTabState {
    /// Open the tab with a fresh empty query.
    pub(crate) fn open_now(&mut self) {
        self.open = true;
        self.query.clear();
        self.cursor = None;
    }

    /// Close the tab. Drops any held cursor stack (matches MC:
    /// closing the creative inventory abandons the cursor without
    /// returning items to the world).
    pub(crate) fn close(&mut self) {
        self.open = false;
        self.query.clear();
        self.cursor = None;
    }
}

/// Build the full list of creative-tab entries. Walks
/// `ITEM_TYPES` and emits one entry per registered item. The
/// list is stable; callers cache it and re-filter via
/// `filter_items` on every search-string keystroke.
pub(crate) fn available_items() -> Vec<CreativeEntry> {
    ITEM_TYPES
        .iter()
        .map(CreativeEntry::from_item)
        .collect()
}

/// Case-insensitive substring filter over the entry display
/// names. An empty query returns every entry unchanged.
pub(crate) fn filter_items(
    entries: &[CreativeEntry],
    query: &str,
) -> Vec<CreativeEntry> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return entries.to_vec();
    }
    entries
        .iter()
        .copied()
        .filter(|e| {
            let name = item_display_name(e.id).to_lowercase();
            name.contains(&q)
        })
        .collect()
}

/// Try to deposit `stack` into the player's hotbar (slots
/// 0..9). First tries to top up a matching stack in any slot,
/// then claims the first empty hotbar slot. Returns `true` when
/// the cursor was deposited (in which case the caller clears
/// `cursor`); `false` means the hotbar was full + the cursor
/// stays as-is.
///
/// Matches the existing `inventory::inv_pickup` flow but only
/// touches the hotbar so creative drops always land in a
/// reachable slot. The full inventory grid stays available for
/// drag&drop.
pub(crate) fn deposit_in_hotbar(
    hotbar: &mut [Option<(BlockId, u8)>],
    stack: (BlockId, u8),
    max_stack: u8,
) -> bool {
    let (block, count) = stack;
    let mut remaining = count;
    // Top-up phase.
    for slot in hotbar.iter_mut() {
        if let Some((b, c)) = slot.as_mut()
            && *b == block
            && *c < max_stack
        {
            let room = max_stack - *c;
            let add = remaining.min(room);
            *c += add;
            remaining = remaining.saturating_sub(add);
            if remaining == 0 {
                return true;
            }
        }
    }
    // First-empty phase.
    for slot in hotbar.iter_mut() {
        if slot.is_none() {
            *slot = Some((block, remaining.min(max_stack)));
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creative_tab_lists_every_block_type() {
        // W10.6 contract: the available list emits one entry per
        // registered item. Loose lower bound so adding new items
        // doesn't break the test, but enough to catch a future
        // refactor that accidentally elides part of the table.
        let items = available_items();
        assert!(items.len() >= 40);
        // Spot-check a known id (stone = 1).
        assert!(items.iter().any(|e| e.id == 1));
    }

    #[test]
    fn creative_tab_search_filters_by_name() {
        // W10.6: substring match against the display name is
        // case-insensitive. "pic" matches all three pickaxes.
        let items = available_items();
        let hits = filter_items(&items, "pic");
        // All three pickaxes show up.
        assert!(hits.iter().any(|e| e.id == 16));
        assert!(hits.iter().any(|e| e.id == 17));
        assert!(hits.iter().any(|e| e.id == 18));
        // Stone (id 1) does NOT match "pic".
        assert!(!hits.iter().any(|e| e.id == 1));
    }

    #[test]
    fn creative_tab_search_is_case_insensitive() {
        let items = available_items();
        let lower = filter_items(&items, "stone");
        let upper = filter_items(&items, "STONE");
        let mixed = filter_items(&items, "StOnE");
        assert_eq!(lower.len(), upper.len());
        assert_eq!(lower.len(), mixed.len());
        assert!(lower.iter().any(|e| e.id == 1));
    }

    #[test]
    fn creative_tab_empty_query_returns_all() {
        // Empty / whitespace query = identity filter.
        let items = available_items();
        let hits = filter_items(&items, "");
        assert_eq!(hits.len(), items.len());
        let hits_ws = filter_items(&items, "   ");
        assert_eq!(hits_ws.len(), items.len());
    }

    #[test]
    fn creative_tab_open_resets_query_and_cursor() {
        // W10.6: opening the panel clears any stale search text +
        // discards the cursor so a fresh session always starts
        // empty.
        let mut state = CreativeTabState {
            open: false,
            query: "stale".into(),
            cursor: Some((BlockId(1), 64)),
        };
        state.open_now();
        assert!(state.open);
        assert!(state.query.is_empty());
        assert!(state.cursor.is_none());
    }

    #[test]
    fn creative_tab_close_drops_cursor() {
        // W10.6: closing the panel discards the held cursor (MC:
        // creative-mode closes drop the cursor without spilling
        // items into the world).
        let mut state = CreativeTabState::default();
        state.open_now();
        state.cursor = Some((BlockId(1), 64));
        state.close();
        assert!(!state.open);
        assert!(state.cursor.is_none());
    }

    #[test]
    fn creative_tab_click_picks_full_stack() {
        // W10.6: clicking a tile fills the cursor with
        // `max_stack` of that item. Verified via the
        // CreativeEntry helper (which is the data the click
        // handler consults).
        let e = CreativeEntry::from_item(&ITEM_TYPES[0]);
        assert_eq!(e.id, ITEM_TYPES[0].id);
        assert_eq!(e.max_stack, ITEM_TYPES[0].max_stack);
    }

    #[test]
    fn deposit_in_hotbar_fills_empty_slot() {
        // Deposit phase #2: empty hotbar receives the cursor in
        // slot 0.
        let mut hotbar: [Option<(BlockId, u8)>; 9] = [None; 9];
        let ok =
            deposit_in_hotbar(&mut hotbar, (BlockId(1), 64), 64);
        assert!(ok);
        assert_eq!(hotbar[0], Some((BlockId(1), 64)));
    }

    #[test]
    fn deposit_in_hotbar_tops_up_existing_stack() {
        // Deposit phase #1: top-up flow. Half-stack already in
        // slot 0; adding 32 more fills it to 64.
        let mut hotbar: [Option<(BlockId, u8)>; 9] = [None; 9];
        hotbar[0] = Some((BlockId(1), 32));
        let ok =
            deposit_in_hotbar(&mut hotbar, (BlockId(1), 32), 64);
        assert!(ok);
        assert_eq!(hotbar[0], Some((BlockId(1), 64)));
    }

    #[test]
    fn deposit_in_hotbar_fails_when_full() {
        // Full hotbar + cursor doesn't match = no deposit.
        let mut hotbar: [Option<(BlockId, u8)>; 9] =
            [Some((BlockId(2), 64)); 9];
        let ok =
            deposit_in_hotbar(&mut hotbar, (BlockId(1), 64), 64);
        assert!(!ok);
    }

    #[test]
    fn filter_items_matches_partial_words() {
        // "wood" matches both "Wood Pickaxe" (16) and
        // "Wood Sword" (23). Multi-hit confidence.
        let items = available_items();
        let hits = filter_items(&items, "wood");
        assert!(hits.iter().any(|e| e.id == 16));
        assert!(hits.iter().any(|e| e.id == 23));
    }

    #[test]
    fn filter_items_returns_empty_for_unknown_text() {
        // Garbage query yields zero hits.
        let items = available_items();
        let hits = filter_items(&items, "xyzzy_not_a_real_item");
        assert!(hits.is_empty());
    }
}
