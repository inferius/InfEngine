//! W47.28 - combat_runtime impl block (relocated from app/mod.rs).
#![allow(unused_imports)]
use crate::*;
use crate::lighting::HaloLighting;

impl Voxel {
    /// Thin wrapper - see `mob::damage::damage_mob`.
    pub(crate) fn damage_mob(&mut self, mob_id: u32, attacker_client_id: Option<u32>) {
        mob::damage_mob(self, mob_id, attacker_client_id);
    }

    /// Thin wrapper - see `mob::damage::damage_mob_scaled`.
    pub(crate) fn damage_mob_scaled(
        &mut self,
        mob_id: u32,
        attacker_client_id: Option<u32>,
        mult: f32,
    ) {
        mob::damage_mob_scaled(self, mob_id, attacker_client_id, mult);
    }

    /// Thin wrapper - see `mob::raycast::raycast_mob`.
    pub(crate) fn raycast_mob(
        &self,
        origin: Vec3,
        dir: Vec3,
        max_dist: f32,
    ) -> Option<(u32, f32)> {
        mob::raycast_mob(self, origin, dir, max_dist)
    }

    /// Find the world-space Y of the topmost solid voxel at column
    /// `(wx, wz)` at or below `max_y`. `None` if the column is air
    /// all the way down.
    ///
    /// We scan from `max_y` downward (instead of from the world
    /// ceiling) so a sheep walking under a tree's leaf canopy
    /// doesn't get its surface snap teleported up to the canopy
    /// top - the relevant ground is the first solid block below
    /// the mob's current head, not the highest block in the
    /// column.
    pub(crate) fn surface_y_below(&self, wx: i32, wz: i32, max_y: i32) -> Option<i32> {
        let top = max_y.min((CHUNK_SIZE as i32) * VERTICAL_CHUNKS - 1).max(0);
        (0..=top).rev().find(|&y| !self.world.get([wx, y, wz]).is_air())
    }

    /// Resolve a safe player spawn for the current world. Uses
    /// `spawn_pos` when configured, otherwise scans an outward
    /// spiral around the origin for a column whose top voxel is
    /// solid + has clear air above. Falls back to high-altitude
    /// Y=90 so gravity can drop the player onto whatever terrain
    /// exists below. Caller should clear `player_vel` +
    /// `fall_start_y` after assigning the returned position.
    pub(crate) fn safe_spawn(&self) -> Vec3 {
        if let Some(p) = self.spawn_pos {
            return Vec3::new(p[0], p[1], p[2]);
        }
        let ceiling = (CHUNK_SIZE as i32) * VERTICAL_CHUNKS - 1;
        // Square-spiral search out to 24 blocks. The first column
        // with a confirmed surface + 2 blocks of air directly
        // above wins so the player doesn't ever spawn buried.
        for r in 0i32..=24 {
            for dz in -r..=r {
                for dx in -r..=r {
                    // Cheap ring filter: only inspect cells on
                    // the current ring perimeter.
                    if r > 0 && dx.abs() != r && dz.abs() != r {
                        continue;
                    }
                    let sy = match self.surface_y_below(dx, dz, ceiling) {
                        Some(y) => y,
                        None => continue,
                    };
                    // Need 3 clear air blocks above the surface
                    // voxel so the player capsule (PLAYER_HEIGHT
                    // = 1.7 m) has headroom + body room.
                    let above1 = self.world.get([dx, sy + 1, dz]);
                    let above2 = self.world.get([dx, sy + 2, dz]);
                    let above3 = self.world.get([dx, sy + 3, dz]);
                    if above1.is_air() && above2.is_air() && above3.is_air() {
                        // Voxel `sy` is solid with top at world
                        // y = sy + 1. Foot sits on that surface;
                        // `player_pos.y` is the eye/cap height so
                        // it lands at sy + 1 + PLAYER_HEIGHT.
                        return Vec3::new(
                            dx as f32 + 0.5,
                            sy as f32 + 1.0 + PLAYER_HEIGHT + 0.05,
                            dz as f32 + 0.5,
                        );
                    }
                }
            }
        }
        Vec3::new(0.0, 90.0, 0.0)
    }
}
