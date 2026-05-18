//! Inventory-icon lookup. Items render their slot thumbnail from the
//! shared block atlas, but the chooser doesn't care which CUBE FACE
//! the texture belongs to - one canonical tile per item is enough.
//!
//! For block-form items we default to the matching `BlockType.top`
//! slot, which is what the existing inventory UI already does
//! visually. Tool items can override with an explicit `IconSpec` on
//! their `ItemType` row when they ship a dedicated icon tile (out of
//! scope until C.2 ships a separate item-atlas page).
//!
//! `icon_uv_rect` is the per-tile UV math, identical to the block
//! atlas equation in `block::atlas_uv_rect` but indexed off an
//! `IconSpec` so the inventory render path can stay block-agnostic.

use super::{block_top_tile_for_item, item_type};
use crate::block::{ATLAS_COLS, ATLAS_ROWS};

/// Atlas-tile address for an inventory icon. Stable across faces -
/// inventory thumbnails don't care about top/side/bottom variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct IconSpec {
    pub(crate) col: u32,
    pub(crate) row: u32,
}

/// Resolve the inventory icon for an item id.
///
/// Resolution order:
/// 1. Explicit `ItemType.icon` - tool items with a dedicated atlas
///    tile (none today; future work).
/// 2. The `BlockType.top` tile for the matching block id - covers
///    every block-form item.
/// 3. `None` - inventory render falls back to the tinted swatch.
pub(crate) fn icon_for(item_id: u16) -> Option<IconSpec> {
    let it = item_type(item_id)?;
    if let Some(icon) = it.icon {
        return Some(icon);
    }
    let tile = block_top_tile_for_item(item_id)?;
    Some(IconSpec { col: tile.col, row: tile.row })
}

/// UV rect (u0, v0, u1, v1) for an icon tile in the shared atlas.
/// Same math as `block::atlas_uv_rect` but indexed off `IconSpec`
/// instead of a face normal.
pub(crate) fn icon_uv_rect(icon: IconSpec) -> [f32; 4] {
    let u_step = 1.0 / ATLAS_COLS as f32;
    let v_step = 1.0 / ATLAS_ROWS as f32;
    let u0 = icon.col as f32 * u_step;
    let v0 = icon.row as f32 * v_step;
    [u0, v0, u0 + u_step, v0 + v_step]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_for_unknown_id_is_none() {
        assert!(icon_for(0).is_none());
        assert!(icon_for(9999).is_none());
    }

    #[test]
    fn icon_uv_rect_is_one_tile_wide() {
        let r = icon_uv_rect(IconSpec { col: 0, row: 0 });
        let u_step = 1.0 / ATLAS_COLS as f32;
        let v_step = 1.0 / ATLAS_ROWS as f32;
        assert!((r[2] - r[0] - u_step).abs() < 1e-6);
        assert!((r[3] - r[1] - v_step).abs() < 1e-6);
    }
}
