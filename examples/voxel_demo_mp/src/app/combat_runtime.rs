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
        // W260: clamp to the full world span (-64..=319), not 0..=255.
        let top = max_y.clamp(crate::WORLD_MIN_Y, crate::WORLD_MAX_Y);
        // W98: ground = first SOLID block, skipping air AND passable
        // decorations (F_AABB_PASS: grass, ferns, flowers, dead bush, ...).
        // Mobs were snapping their feet onto tall grass and bobbing over it;
        // now they stand on the real ground and walk through plants, like
        // the player physics (`voxel_is_solid_with_doors`).
        (crate::WORLD_MIN_Y..=top).rev().find(|&y| {
            let b = self.world.get([wx, y, wz]);
            !b.is_air() && !crate::block::block_is_aabb_passable(b)
        })
    }

    /// True if the player capsule for eye-height `pos` overlaps a solid
    /// (non-`F_AABB_PASS`) block - i.e. the player is embedded in terrain
    /// rather than standing in air / passable plants. Used on world load to
    /// detect a saved position that a worldgen change (W112 raised the sea
    /// level 12 -> 63, lifting the terrain band) or a later edit has buried.
    pub(crate) fn capsule_embedded(&self, pos: Vec3) -> bool {
        let bx = pos.x.floor() as i32;
        let bz = pos.z.floor() as i32;
        let feet_y = (pos.y - PLAYER_HEIGHT).floor() as i32;
        let head_y = pos.y.floor() as i32;
        (feet_y..=head_y).any(|y| {
            let b = self.world.get([bx, y, bz]);
            !b.is_air() && !crate::block::block_is_aabb_passable(b)
        })
    }

    /// Resolve a safe player spawn for the current world. Uses
    /// `spawn_pos` when configured, otherwise scans an outward
    /// spiral around the origin for a column whose top voxel is
    /// solid + has 3 clear air blocks above (dry, roofless ground).
    /// If the whole search area is flooded / capped it lands on the
    /// highest surface found instead of free-falling from altitude.
    /// Caller should clear `player_vel` + `fall_start_y` after
    /// assigning the returned position.
    pub(crate) fn safe_spawn(&self) -> Vec3 {
        if let Some(p) = self.spawn_pos {
            // Trust a stored spawn only if the player capsule actually
            // fits there. An old save (or a world whose terrain changed
            // under it) can leave the recorded spawn buried in solid
            // blocks; in that case fall through to the spiral search
            // rather than dropping the player inside the ground.
            let bx = p[0].floor() as i32;
            let bz = p[2].floor() as i32;
            let feet_y = (p[1] - PLAYER_HEIGHT).floor() as i32;
            let head_y = p[1].floor() as i32;
            let clear = (feet_y..=head_y).all(|y| self.world.get([bx, y, bz]).is_air());
            if clear {
                return Vec3::new(p[0], p[1], p[2]);
            }
        }
        let ceiling = crate::WORLD_MAX_Y;
        // Square-spiral search out to 48 blocks. A column with a solid
        // top + 3 clear air blocks above (dry, roofless ground) wins.
        // The wider radius matters for the Natural profile, where the
        // origin can land in a low / ocean patch. `best` keeps the
        // highest surface seen so a no-dry-column result still lands on
        // solid ground rather than free-falling from a fixed altitude
        // and wedging the player inside a hillside ("spawned stuck in
        // the terrain").
        let mut best: Option<(i32, i32, i32)> = None; // (surface_y, dx, dz)
        let mut dry: Option<(i32, i32, i32)> = None;
        // Generator height per column lets us reject the floor of a cave
        // / ravine carved into a column - it also has clear air above so
        // it otherwise reads as a valid surface, which is exactly how the
        // player kept spawning deep inside the terrain. A real ground
        // surface sits within a couple of blocks of the generator's
        // terrain top; a carve floor sits well below it, a tree canopy
        // well above it.
        let ctx = self.current_worldgen_ctx();
        'search: for r in 0i32..=48 {
            for dz in -r..=r {
                for dx in -r..=r {
                    // Cheap ring filter: only inspect cells on the
                    // current ring perimeter.
                    if r > 0 && dx.abs() != r && dz.abs() != r {
                        continue;
                    }
                    let sy = match self.surface_y_below(dx, dz, ceiling) {
                        Some(y) => y,
                        None => continue,
                    };
                    if best.is_none_or(|(by, _, _)| sy > by) {
                        best = Some((sy, dx, dz));
                    }
                    // Need 3 clear air blocks above the surface voxel so
                    // the player capsule (PLAYER_HEIGHT = 1.7 m) has
                    // headroom + body room, and so a flooded column
                    // (water directly above the seabed) is rejected.
                    let above1 = self.world.get([dx, sy + 1, dz]);
                    let above2 = self.world.get([dx, sy + 2, dz]);
                    let above3 = self.world.get([dx, sy + 3, dz]);
                    if above1.is_air() && above2.is_air() && above3.is_air() {
                        // Only accept a surface at (near) the generator's
                        // terrain height - not a ravine / cave floor below
                        // it, nor a tree canopy above it.
                        let gh = crate::worldgen::chunk::column_height(&ctx, dx, dz);
                        if (sy - gh).abs() <= 2 {
                            dry = Some((sy, dx, dz));
                            break 'search;
                        }
                    }
                }
            }
        }
        // W186: no dry column in the streamed region - under MC worldgen
        // the origin can sit in open ocean whose nearest shore is past
        // both the spiral and the streamed chunks. Ask the generator for
        // the nearest land (it reads no chunk data, so it sees terrain
        // the in-world scan can't); continuous streaming fills those
        // chunks in once the player is positioned. The in-world spiral
        // stays primary so a normal land spawn keeps its cave/canopy
        // rejection.
        if dry.is_none() {
            // Intermediate binding (not a collapsed let-chain) so the
            // expensive search only runs on the ocean-origin path.
            let land = crate::worldgen::chunk::find_land_spawn(&ctx);
            if let Some((lx, lz, lh)) = land {
                return Vec3::new(
                    lx as f32 + 0.5,
                    (lh + 1) as f32 + PLAYER_HEIGHT + 0.05,
                    lz as f32 + 0.5,
                );
            }
        }
        // Prefer the dry column; else the highest surface seen. Voxel
        // `sy` is solid with top at world y = sy + 1; the foot sits
        // there and `player_pos.y` is the eye/cap height, so it lands
        // at sy + 1 + PLAYER_HEIGHT.
        let Some((sy, dx, dz)) = dry.or(best) else {
            // Pathological: nothing solid in range. The world always
            // has a y=0 floor now, so a high drop still lands safely.
            return Vec3::new(0.5, 90.0, 0.5);
        };
        // W84 safety lift: raise the foot cell until two clear air blocks
        // (body + head room) sit above the ground, so even the fallback
        // path (no dry column found, e.g. a surface under a canopy) can
        // never leave the player embedded. For a normal dry column this
        // is a no-op (foot = sy + 1 is already clear).
        let mut foot = sy + 1;
        while foot + 1 < ceiling
            && (!self.world.get([dx, foot, dz]).is_air()
                || !self.world.get([dx, foot + 1, dz]).is_air())
        {
            foot += 1;
        }
        // `foot` is the air cell the feet rest in (ground top at `foot`);
        // player_pos.y is the eye, PLAYER_HEIGHT above the feet.
        let pos = Vec3::new(
            dx as f32 + 0.5,
            foot as f32 + PLAYER_HEIGHT + 0.05,
            dz as f32 + 0.5,
        );
        // Log the actual blocks at the spawn so an "I spawned in the
        // ground" report is decidable from the log: ground should be
        // solid, at_feet / at_head should be 0 (air).
        tracing::info!(
            dx,
            dz,
            sy,
            foot,
            dry = dry.is_some(),
            y = pos.y,
            ground = self.world.get([dx, foot - 1, dz]).0,
            at_feet = self.world.get([dx, foot, dz]).0,
            at_head = self.world.get([dx, foot + 1, dz]).0,
            "voxel: safe_spawn resolved"
        );
        pos
    }
}
