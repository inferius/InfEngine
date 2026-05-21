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

use crate::item::{ItemCategory, ITEM_TYPES, ItemType, item_display_name};

/// W12.9 - Creative tab category strip. Mirrors MC's
/// `CreativeModeTab` enum but at a coarser grain. Each category
/// maps onto one or more `ItemCategory` rows via
/// `CreativeCategory::accepts`.
#[allow(dead_code)] // tab-strip UI hookup lands in a follow-up commit
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub(crate) enum CreativeCategory {
    /// All (default search across every kind).
    #[default]
    All,
    /// Building blocks (terrain, planks, sandstone, ...).
    Blocks,
    /// Decoration (glass, wool, torches, slabs, doors, ...).
    Decoration,
    /// Tools (pickaxe / shovel / axe + flint and steel).
    Tools,
    /// Combat (swords).
    Combat,
    /// Food (meat + future bread / apple).
    Food,
    /// Misc - buckets + ingredients (feather, ingot, ...).
    Misc,
}

impl CreativeCategory {
    /// Stable display label for the tab strip.
    #[allow(dead_code)]
    pub(crate) fn label(self) -> &'static str {
        match self {
            CreativeCategory::All => "All",
            CreativeCategory::Blocks => "Blocks",
            CreativeCategory::Decoration => "Decoration",
            CreativeCategory::Tools => "Tools",
            CreativeCategory::Combat => "Combat",
            CreativeCategory::Food => "Food",
            CreativeCategory::Misc => "Misc",
        }
    }

    /// Every category in display order. Used by the tab strip
    /// renderer.
    #[allow(dead_code)]
    pub(crate) const fn all() -> [CreativeCategory; 7] {
        [
            CreativeCategory::All,
            CreativeCategory::Blocks,
            CreativeCategory::Decoration,
            CreativeCategory::Tools,
            CreativeCategory::Combat,
            CreativeCategory::Food,
            CreativeCategory::Misc,
        ]
    }

    /// True when an item with `ItemCategory ic` should show up
    /// inside this tab. The `All` tab always returns true so the
    /// search-anything default works as before.
    #[allow(dead_code)]
    pub(crate) fn accepts(self, ic: ItemCategory) -> bool {
        match self {
            CreativeCategory::All => true,
            CreativeCategory::Blocks => matches!(ic, ItemCategory::BuildingBlock),
            CreativeCategory::Decoration => matches!(ic, ItemCategory::Decoration),
            CreativeCategory::Tools => matches!(ic, ItemCategory::Tool),
            CreativeCategory::Combat => matches!(ic, ItemCategory::Weapon),
            CreativeCategory::Food => matches!(ic, ItemCategory::Food),
            CreativeCategory::Misc => matches!(
                ic,
                ItemCategory::Bucket | ItemCategory::Ingredient
            ),
        }
    }
}

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
    /// W12.9 - source ItemCategory the tab-strip filter walks.
    pub category: ItemCategory,
}

impl CreativeEntry {
    /// Build a fresh entry from an `ItemType` row.
    pub(crate) fn from_item(it: &'static ItemType) -> Self {
        Self {
            id: it.id,
            max_stack: it.max_stack,
            category: it.category,
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
    /// W12.9 - active tab. Defaults to All.
    pub category: CreativeCategory,
    /// W20.3 - true on the first frame after open_now(). The egui
    /// pass uses this to request keyboard focus on the search box
    /// without polling every frame. Cleared after the first render.
    pub just_opened: bool,
}

impl CreativeTabState {
    /// Open the tab with a fresh empty query.
    ///
    /// Sets `just_opened = true` so the first egui pass can
    /// request focus on the search text box.
    pub(crate) fn open_now(&mut self) {
        self.open = true;
        self.query.clear();
        self.cursor = None;
        self.category = CreativeCategory::All;
        self.just_opened = true;
    }

    /// Close the tab. Drops any held cursor stack (matches MC:
    /// closing the creative inventory abandons the cursor without
    /// returning items to the world).
    pub(crate) fn close(&mut self) {
        self.open = false;
        self.query.clear();
        self.cursor = None;
        self.category = CreativeCategory::All;
        self.just_opened = false;
    }
}

/// W29.2: Item ids that must NOT appear in the creative tab.
///
/// Mirrors MC's `Items.java` logic where technical / non-placeable
/// blocks are excluded from the creative-mode inventory:
///   - 36 (water source)  - not a placeable item; use water bucket.
///   - 37 (lava source)   - not a placeable item; use lava bucket.
///   - 47 (portal)        - spawns via flint+steel ignition only.
///   - 49 (fire)          - spawns via flint+steel / lava-burn only.
///   - 51 (redstone_wall_torch) - W35.2: wall variant auto-selected on
///     placement; only the base torch shows in creative.
///   - 53 (soul_wall_torch)     - W35.2: same rationale as id 51.
///   - 56 (soul_fire)     - same as fire; technical world block.
///   - 78 (oak_door_upper) - internal upper-half block; never held.
pub(crate) const CREATIVE_BLACKLIST: &[u16] = &[36, 37, 47, 49, 51, 53, 56, 78];

/// Build the full list of creative-tab entries. Walks
/// `ITEM_TYPES`, strips technical / non-giftable items via
/// `CREATIVE_BLACKLIST`, and emits one entry per remaining item.
/// The list is stable; callers cache it and re-filter via
/// `filter_items` on every search-string keystroke.
pub(crate) fn available_items() -> Vec<CreativeEntry> {
    ITEM_TYPES
        .iter()
        .filter(|it| !CREATIVE_BLACKLIST.contains(&it.id))
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

/// W12.9 - filter entries by the active creative-tab category.
/// `All` returns every entry; other categories restrict to entries
/// whose source `ItemCategory` is `accept`ed by the tab.
#[allow(dead_code)] // wired up by the future tab-strip UI commit.
pub(crate) fn filter_by_category(
    entries: &[CreativeEntry],
    category: CreativeCategory,
) -> Vec<CreativeEntry> {
    entries
        .iter()
        .copied()
        .filter(|e| category.accepts(e.category))
        .collect()
}

/// W12.9 - combined category + text filter. Equivalent to
/// `filter_items(&filter_by_category(entries, cat), q)`; one
/// helper to keep call sites concise.
#[allow(dead_code)] // wired up by the future tab-strip UI commit.
pub(crate) fn filter_items_in_category(
    entries: &[CreativeEntry],
    category: CreativeCategory,
    query: &str,
) -> Vec<CreativeEntry> {
    let cat_hits = filter_by_category(entries, category);
    filter_items(&cat_hits, query)
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
            category: CreativeCategory::Tools,
            just_opened: false,
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

    // ---------------------------------------------------------------
    // W12.9 - Multi-tab category filter tests.
    // ---------------------------------------------------------------

    #[test]
    fn creative_tab_categories_filter_items() {
        // Tools tab includes pickaxes (id 16/17/18) but NOT stone (1).
        let items = available_items();
        let tools = filter_by_category(&items, CreativeCategory::Tools);
        assert!(tools.iter().any(|e| e.id == 16), "wood pickaxe in Tools");
        assert!(tools.iter().any(|e| e.id == 18), "iron pickaxe in Tools");
        assert!(!tools.iter().any(|e| e.id == 1), "stone NOT in Tools");
        // Combat tab includes swords.
        let combat = filter_by_category(&items, CreativeCategory::Combat);
        assert!(combat.iter().any(|e| e.id == 23), "wood sword in Combat");
        assert!(combat.iter().any(|e| e.id == 25), "iron sword in Combat");
        // Food tab includes meat (id 12).
        let food = filter_by_category(&items, CreativeCategory::Food);
        assert!(food.iter().any(|e| e.id == 12), "meat in Food");
    }

    #[test]
    fn every_item_has_a_category() {
        // The CreativeEntry's `category` field must be populated by
        // `from_item`. None of the rows in ITEM_TYPES leave this as
        // a default fallback - the field is mandatory.
        let items = available_items();
        for e in &items {
            // Sanity check: each entry's category equals the
            // ITEM_TYPES row's category (round-trip).
            let it = ITEM_TYPES.iter().find(|i| i.id == e.id).unwrap();
            assert_eq!(e.category, it.category, "id {} mismatched", e.id);
        }
        // Every entry must be in the All tab.
        let all = filter_by_category(&items, CreativeCategory::All);
        assert_eq!(all.len(), items.len(), "All tab must include everything");
    }

    #[test]
    fn category_strip_lists_all_seven() {
        let strip = CreativeCategory::all();
        assert_eq!(strip.len(), 7);
        // First is All, last is Misc per the display order.
        assert_eq!(strip[0], CreativeCategory::All);
        assert_eq!(strip[strip.len() - 1], CreativeCategory::Misc);
    }

    #[test]
    fn category_labels_are_non_empty() {
        for cat in CreativeCategory::all() {
            assert!(
                !cat.label().is_empty(),
                "category {cat:?} has empty label",
            );
        }
    }

    #[test]
    fn category_default_is_all() {
        let s = CreativeTabState::default();
        assert_eq!(s.category, CreativeCategory::All);
    }

    #[test]
    fn open_resets_category_to_all() {
        // open_now should clear any leftover category selection.
        let mut s = CreativeTabState {
            category: CreativeCategory::Tools,
            ..Default::default()
        };
        s.open_now();
        assert_eq!(s.category, CreativeCategory::All);
    }

    // ---------------------------------------------------------------
    // W20.3: just_opened flag tests.
    // ---------------------------------------------------------------

    #[test]
    fn open_now_sets_just_opened() {
        // W20.3: open_now() must arm the one-shot flag so the next
        // egui frame can request search-box focus.
        let mut s = CreativeTabState::default();
        assert!(!s.just_opened, "default must start with just_opened=false");
        s.open_now();
        assert!(s.just_opened, "open_now must set just_opened=true");
    }

    #[test]
    fn close_clears_just_opened() {
        // W20.3: close() must disarm the flag.
        let mut s = CreativeTabState::default();
        s.open_now();
        assert!(s.just_opened);
        s.close();
        assert!(!s.just_opened, "close must clear just_opened");
    }

    #[test]
    fn filter_items_in_category_combines_both() {
        // Search "wood" inside Tools should match the wood pickaxe
        // (16) + wood shovel (29) + wood axe (32) but NOT the wood
        // sword (23 - that's Combat).
        let items = available_items();
        let tool_woods =
            filter_items_in_category(&items, CreativeCategory::Tools, "wood");
        assert!(tool_woods.iter().any(|e| e.id == 16));
        assert!(tool_woods.iter().any(|e| e.id == 29));
        assert!(tool_woods.iter().any(|e| e.id == 32));
        assert!(!tool_woods.iter().any(|e| e.id == 23));
    }

    #[test]
    fn category_misc_holds_buckets_and_ingredients() {
        let items = available_items();
        let misc = filter_by_category(&items, CreativeCategory::Misc);
        // Empty bucket = id 38.
        assert!(misc.iter().any(|e| e.id == 38));
        // Feather (ingredient) = id 19.
        assert!(misc.iter().any(|e| e.id == 19));
    }

    #[test]
    fn category_accepts_consistent_with_item_category() {
        // Every ItemCategory variant should land in exactly one
        // non-All tab (the partition is total). Check by walking
        // every item + counting how many tabs `accept` it.
        let items = available_items();
        for e in &items {
            let mut hits = 0;
            for cat in CreativeCategory::all() {
                if matches!(cat, CreativeCategory::All) {
                    continue;
                }
                if cat.accepts(e.category) {
                    hits += 1;
                }
            }
            assert_eq!(
                hits, 1,
                "id {} (category {:?}) should land in exactly one tab",
                e.id, e.category,
            );
        }
    }
}
