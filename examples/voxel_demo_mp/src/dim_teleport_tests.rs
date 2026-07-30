//! Integration tests for the F.6 cross-dim teleport flow.
//!
//! These build a Voxel directly and call `tick_player_portal` /
//! `do_cross_dim_teleport` so we exercise the same code paths
//! the per-frame tick loop runs. Renderer state stays None;
//! networking stays Offline so no I/O happens.

use crate::Voxel;
use crate::block::{PORTAL_ID, portal};
use crate::dim::{NETHER, OVERWORLD};
use infengine_voxel::{BlockId, Chunk, ChunkCoord};

/// Pre-insert every chunk in a small range so world.set on
/// negative / large coords actually lands.
fn insert_chunks(voxel: &mut Voxel, range: i32) {
    for cx in -range..=range {
        for cy in -range..=range {
            for cz in -range..=range {
                let coord: ChunkCoord = [cx, cy, cz];
                voxel.world.insert_chunk(coord, Chunk::empty());
            }
        }
    }
}

#[test]
fn standing_in_portal_increments_counter() {
    let mut v = Voxel::new();
    insert_chunks(&mut v, 1);
    v.current_dim = OVERWORLD.to_string();
    // Drop the player into a portal cell.
    v.world.set([0, 8, 0], BlockId(PORTAL_ID));
    v.player_pos = glam::Vec3::new(0.5, 9.0, 0.5); // foot at y=8
    v.portal_cooldown_ticks = 0;
    v.portal_in_ticks = 0;
    v.tick_player_portal(0.05);
    assert_eq!(v.portal_in_ticks, 1);
    v.tick_player_portal(0.05);
    v.tick_player_portal(0.05);
    assert_eq!(v.portal_in_ticks, 3);
}

#[test]
fn stepping_out_resets_counter() {
    let mut v = Voxel::new();
    insert_chunks(&mut v, 1);
    v.current_dim = OVERWORLD.to_string();
    v.world.set([0, 8, 0], BlockId(PORTAL_ID));
    v.player_pos = glam::Vec3::new(0.5, 9.0, 0.5);
    v.portal_cooldown_ticks = 0;
    v.tick_player_portal(0.05);
    v.tick_player_portal(0.05);
    assert_eq!(v.portal_in_ticks, 2);
    // Walk out of the portal cell.
    v.player_pos = glam::Vec3::new(5.5, 9.0, 0.5);
    v.tick_player_portal(0.05);
    assert_eq!(v.portal_in_ticks, 0);
}

#[test]
fn cooldown_blocks_re_trigger() {
    let mut v = Voxel::new();
    insert_chunks(&mut v, 1);
    v.current_dim = OVERWORLD.to_string();
    v.world.set([0, 8, 0], BlockId(PORTAL_ID));
    v.player_pos = glam::Vec3::new(0.5, 9.0, 0.5);
    // Player JUST teleported - cool-down should suppress
    // the in-portal counter even though they're standing
    // inside a portal cell. W277: standing inside RE-ARMS the
    // cooldown to full (MC Entity.setAsInsidePortal) instead of
    // letting it tick down under their feet.
    v.portal_cooldown_ticks = 5;
    v.tick_player_portal(0.05);
    assert_eq!(v.portal_in_ticks, 0);
    assert_eq!(v.portal_cooldown_ticks, portal::PORTAL_COOLDOWN_TICKS);
}

#[test]
fn cooldown_rearms_while_standing_inside_portal() {
    // W277: MC Entity.setAsInsidePortal - the cooldown re-arms to
    // FULL every tick the player stays inside a portal, so the
    // return trip can only start after stepping out of the frame.
    let mut v = Voxel::new();
    insert_chunks(&mut v, 1);
    v.current_dim = OVERWORLD.to_string();
    v.world.set([0, 8, 0], BlockId(PORTAL_ID));
    v.player_pos = glam::Vec3::new(0.5, 9.0, 0.5);
    v.portal_cooldown_ticks = 5;
    for _ in 0..20 {
        v.tick_player_portal(0.05);
    }
    assert_eq!(
        v.portal_cooldown_ticks,
        portal::PORTAL_COOLDOWN_TICKS,
        "standing inside must pin the cooldown at full"
    );
    assert_eq!(v.portal_in_ticks, 0);
    // Step out: now it finally runs down.
    v.player_pos = glam::Vec3::new(5.5, 9.0, 0.5);
    v.tick_player_portal(0.05);
    assert_eq!(v.portal_cooldown_ticks, portal::PORTAL_COOLDOWN_TICKS - 1);
}

#[test]
fn standing_long_enough_teleports_to_paired_dim() {
    let mut v = Voxel::new();
    insert_chunks(&mut v, 4);
    v.current_dim = OVERWORLD.to_string();
    v.base_seed = 7;
    v.world.set([16, 8, 16], BlockId(PORTAL_ID));
    v.player_pos = glam::Vec3::new(16.5, 9.0, 16.5);
    v.portal_cooldown_ticks = 0;
    v.portal_in_ticks = 0;
    // Walk through enough ticks to trigger.
    for _ in 0..portal::PORTAL_TRIGGER_TICKS + 1 {
        v.tick_player_portal(0.05);
    }
    assert_eq!(v.current_dim, NETHER, "should be in the nether now");
    // Cool-down should be set.
    assert!(v.portal_cooldown_ticks > 0);
    // In-portal counter is reset.
    assert_eq!(v.portal_in_ticks, 0);
}

#[test]
fn teleport_overworld_to_nether_scales_coords_by_8() {
    // Direct scale check via the pure helper.
    assert_eq!(
        portal::pair_coord(OVERWORLD, [80, 64, 16]),
        [10, 64, 2]
    );
    assert_eq!(
        portal::pair_coord(NETHER, [10, 64, 2]),
        [80, 64, 16]
    );
}

#[test]
fn teleport_preserves_player_inventory() {
    let mut v = Voxel::new();
    insert_chunks(&mut v, 4);
    v.current_dim = OVERWORLD.to_string();
    v.base_seed = 1;
    v.world.set([0, 8, 0], BlockId(PORTAL_ID));
    v.player_pos = glam::Vec3::new(0.5, 9.0, 0.5);
    // Stash some inventory before teleport.
    v.inventory[0] = Some(crate::item::ItemStack::new(1, 32));
    v.inventory[3] = Some(crate::item::ItemStack::new(7, 64));
    v.player_health = 18.0;
    v.do_cross_dim_teleport([0, 8, 0]);
    assert_eq!(v.current_dim, NETHER);
    // Inventory + health survived the dim switch.
    assert_eq!(v.inventory[0], Some(crate::item::ItemStack::new(1, 32)));
    assert_eq!(v.inventory[3], Some(crate::item::ItemStack::new(7, 64)));
    assert_eq!(v.player_health, 18.0);
}

#[test]
fn teleport_builds_return_portal_when_none_exists() {
    let mut v = Voxel::new();
    insert_chunks(&mut v, 4);
    v.current_dim = OVERWORLD.to_string();
    v.base_seed = 0;
    v.world.set([0, 8, 0], BlockId(PORTAL_ID));
    v.player_pos = glam::Vec3::new(0.5, 9.0, 0.5);
    v.do_cross_dim_teleport([0, 8, 0]);
    // We're now in the nether at coord [0, 8, 0]. There was no
    // portal there -> a fresh one must have been built. The
    // player's spawn voxel should sit adjacent to a portal.
    let found = portal::find_portal_near(
        &v.world,
        [v.player_pos.x as i32, v.player_pos.y as i32, v.player_pos.z as i32],
    );
    assert!(
        found.is_some(),
        "destination should have a fresh portal frame nearby"
    );
}
