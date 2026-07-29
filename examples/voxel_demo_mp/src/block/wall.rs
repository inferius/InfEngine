//! W455: MC WallBlock visual state (WallBlock.java port).
//!
//! MC walls carry a WallSide per horizontal direction (NONE / LOW /
//! TALL) plus an `up` post flag, recomputed from neighbours on
//! every shape update. The engine stores no per-block wall state,
//! so the mesher recomputes the same functions from the chunk
//! snapshot on every remesh - pure surroundings in, real props out.
//! (R35 follow-up: the earlier low/tall -> connected-bool fold was
//! rejected as a shortcut; this is the actual state machine.)
//!
//! Shape-test approximation (the engine has block classes, not
//! VoxelShapes): a side counts as "covered" when the block above is
//! a fully-occluding cube, carries the WALL_POST_OVERRIDE tag (MC
//! swaps the above shape for a full block), or is a wall whose
//! matching arm runs the same way.

use std::sync::OnceLock;

use infengine_voxel::BlockId;

/// MC `WallSide`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WallArm {
    None,
    Low,
    Tall,
}

impl WallArm {
    pub(crate) fn as_prop(self) -> &'static str {
        match self {
            WallArm::None => "none",
            WallArm::Low => "low",
            WallArm::Tall => "tall",
        }
    }
}

/// Visual state of one wall cell: four arms + the centre post.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WallState {
    pub(crate) north: WallArm,
    pub(crate) east: WallArm,
    pub(crate) south: WallArm,
    pub(crate) west: WallArm,
    pub(crate) post: bool,
}

/// MC block tag `#minecraft:wall_post_override` - blocks that force
/// the post under themselves (torches, signs, banners, pressure
/// plates, tripwire). Name-derived so every registry variant of
/// those families joins without a manual id list.
pub(crate) fn wall_post_override(id: u16) -> bool {
    static SET: OnceLock<std::collections::HashSet<u16>> = OnceLock::new();
    SET.get_or_init(|| {
        crate::block::BLOCK_TYPES
            .iter()
            .filter(|b| {
                b.name.contains("torch")
                    || b.name.contains("sign")
                    || b.name.contains("banner")
                    || b.name.ends_with("_pressure_plate")
                    || b.name == "tripwire"
            })
            .map(|b| b.id)
            .collect()
    })
    .contains(&id)
}

/// Compute a wall's visual state from its surroundings. `sample`
/// returns the block id at a RELATIVE offset from the wall cell
/// (the mesher passes a chunk + boundary-halo sampler; tests pass a
/// closure world).
pub(crate) fn wall_state(
    self_id: u16,
    sample: &dyn Fn([i32; 3]) -> u16,
) -> WallState {
    let above = sample([0, 1, 0]);
    // WALL_POST_OVERRIDE above reads as a full cover in MC
    // (getStateForPlacement swaps the above shape for a full
    // block). Air must short-circuit - block_fully_occludes has no
    // registry row for id 0 and defaults occluding (the same guard
    // multipart_connects carries).
    let above_full = (!BlockId(above).is_air()
        && crate::block::block_fully_occludes(BlockId(above)))
        || wall_post_override(above);
    // WallBlock.makeWallState: connected side -> covered ? TALL :
    // LOW. An above WALL covers this side only where its own
    // matching arm runs (approximated by its connection test).
    let arm = |dx: i32, dz: i32| -> WallArm {
        let n_id = sample([dx, 0, dz]);
        if !crate::block::multipart_connects(self_id, BlockId(n_id)) {
            return WallArm::None;
        }
        let covered = above_full
            || (crate::block::is_wall_block(above)
                && crate::block::multipart_connects(
                    above,
                    BlockId(sample([dx, 1, dz])),
                ));
        if covered { WallArm::Tall } else { WallArm::Low }
    };
    let north = arm(0, -1);
    let east = arm(1, 0);
    let south = arm(0, 1);
    let west = arm(-1, 0);
    // WallBlock.shouldRaisePost: a wall above keeps the post chain;
    // a straight run hides it unless the centre is covered; every
    // other layout (lone post, corner, T, cross, end) shows it.
    let post = if crate::block::is_wall_block(above) {
        true
    } else {
        let ns_run = north != WallArm::None
            && south != WallArm::None
            && east == WallArm::None
            && west == WallArm::None;
        let ew_run = east != WallArm::None
            && west != WallArm::None
            && north == WallArm::None
            && south == WallArm::None;
        if !ns_run && !ew_run { true } else { above_full }
    };
    WallState { north, east, south, west, post }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::COBBLESTONE_WALL_ID;

    fn world(cells: &[([i32; 3], u16)]) -> impl Fn([i32; 3]) -> u16 + '_ {
        move |d| {
            cells
                .iter()
                .find(|(p, _)| *p == d)
                .map(|(_, id)| *id)
                .unwrap_or(0)
        }
    }

    #[test]
    fn lone_wall_is_a_post() {
        let w = world(&[]);
        let st = wall_state(COBBLESTONE_WALL_ID, &w);
        assert_eq!(st.north, WallArm::None);
        assert_eq!(st.east, WallArm::None);
        assert!(st.post);
    }

    #[test]
    fn straight_run_hides_the_post_and_keeps_low_arms() {
        let cells = [
            ([0, 0, -1], COBBLESTONE_WALL_ID),
            ([0, 0, 1], COBBLESTONE_WALL_ID),
        ];
        let w = world(&cells);
        let st = wall_state(COBBLESTONE_WALL_ID, &w);
        assert_eq!(st.north, WallArm::Low);
        assert_eq!(st.south, WallArm::Low);
        assert_eq!(st.east, WallArm::None);
        assert!(!st.post);
    }

    #[test]
    fn torch_above_forces_post_and_tall_arms() {
        let cells = [
            ([0, 0, -1], COBBLESTONE_WALL_ID),
            ([0, 0, 1], COBBLESTONE_WALL_ID),
            ([0, 1, 0], crate::block::TORCH_ID),
        ];
        let w = world(&cells);
        let st = wall_state(COBBLESTONE_WALL_ID, &w);
        assert!(st.post, "wall_post_override raises the post");
        assert_eq!(st.north, WallArm::Tall);
        assert_eq!(st.south, WallArm::Tall);
    }

    #[test]
    fn corner_shows_the_post() {
        let cells = [
            ([0, 0, -1], COBBLESTONE_WALL_ID),
            ([1, 0, 0], COBBLESTONE_WALL_ID),
        ];
        let w = world(&cells);
        let st = wall_state(COBBLESTONE_WALL_ID, &w);
        assert!(st.post);
        assert_eq!(st.north, WallArm::Low);
        assert_eq!(st.east, WallArm::Low);
    }

    #[test]
    fn solid_above_makes_arms_tall_and_raises_the_post() {
        let cells = [
            ([0, 0, -1], COBBLESTONE_WALL_ID),
            ([0, 0, 1], COBBLESTONE_WALL_ID),
            ([0, 1, 0], 1), // stone
        ];
        let w = world(&cells);
        let st = wall_state(COBBLESTONE_WALL_ID, &w);
        assert_eq!(st.north, WallArm::Tall);
        assert_eq!(st.south, WallArm::Tall);
        assert!(st.post, "covered centre raises the post mid-run");
    }

    #[test]
    fn stacked_walls_keep_their_posts() {
        let cells = [([0, 1, 0], COBBLESTONE_WALL_ID)];
        let w = world(&cells);
        let st = wall_state(COBBLESTONE_WALL_ID, &w);
        assert!(st.post);
    }
}
