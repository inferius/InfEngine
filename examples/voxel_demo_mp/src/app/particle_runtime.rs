//! W47.28 - particle_runtime impl block (relocated from app/mod.rs).
#![allow(unused_imports)]
use crate::*;
use crate::lighting::HaloLighting;

impl Voxel {
    /// W9.5: emit ambient flame + smoke particles from torches,
    /// fire blocks, and lava source cells near the player. Each
    /// source rolls a per-frame accumulator; whole-particle steps
    /// spawn one flame puff drifting upward. Capped globally by
    /// `AMBIENT_PARTICLE_CAP_PER_TICK` so a torch-heavy build
    /// doesn't push past the system's max budget.
    ///
    /// MC's vanilla rate is ~one flame every 4 ticks (200 ms) per
    /// torch; we use the same target so the look matches.
    pub(crate) fn tick_ambient_particles(&mut self, dt: f32) {
        crate::ambient::tick_ambient_particles(self, dt);
    }

    // W47.7: spawn_break_particles / spawn_break_particles_tinted /
    // spawn_place_particles relocated to crate::ambient. The thin
    // forwards below keep existing call sites working without
    // sprinkling the module path through main.rs.
    pub(crate) fn spawn_break_particles(&mut self, voxel: [i32; 3]) {
        crate::ambient::spawn_break_particles(&mut self.particles, voxel);
    }

    /// Empty-bucket RMB scoop. Casts a ray that stops at the
    /// first liquid voxel or solid block, accepts a liquid
    /// SOURCE (level 0), swaps the bucket to its filled
    /// variant, clears the source voxel, and broadcasts the
    /// edit. Returns `true` when the scoop succeeded.
    ///
    /// Runs OUTSIDE the placement-target chain because
    /// `self.target` skips `F_RAY_PASS` liquids - if the player
    /// aims an empty bucket at a water source with sky behind
    /// it, the placement ray returns `None` and the chain's
    /// bucket arm never fires. This dedicated raycast handles
    /// the scoop independently.
    pub(crate) fn try_scoop_with_bucket(&mut self, forward: Vec3) -> bool {
        let liquid_hit = raycast_with(
            &self.world,
            self.player_pos,
            forward,
            6.0,
            |b| {
                !b.is_air()
                    && (block_is_liquid(b) || !block_is_ray_passable(b))
            },
        );
        let Some(scoop_voxel) = liquid_hit.map(|h| h.voxel) else {
            return false;
        };
        let scoop_block = self.world.get(scoop_voxel);
        if !block_is_liquid(scoop_block) {
            return false;
        }
        // MC: only source voxels (`isSource()`) can be bottled.
        // Flow / falling cells pop the bucket back empty.
        let scoop_level = self
            .liquid_level
            .get(&scoop_voxel)
            .copied()
            .unwrap_or(0);
        if scoop_level != 0 {
            return false;
        }
        let Some(filled) = liquid_to_bucket_id(scoop_block.0) else {
            return false;
        };
        let cfg = bincode::config::standard();
        self.world_set_tracked(scoop_voxel, BlockId::AIR);
        let coords = Self::affected_chunks(scoop_voxel);
        self.rebuild_chunks(&coords);
        self.inventory[self.selected_slot] = Some(crate::item::ItemStack::new(filled, 1));
        let edit = EditMsg {
            x: scoop_voxel[0],
            y: scoop_voxel[1],
            z: scoop_voxel[2],
            block: 0,
        };
        match &mut self.net {
            NetRole::Offline => {}
            NetRole::Server { server, .. } => {
                self.edit_history.push(edit);
                if let Ok(buf) = bincode::serde::encode_to_vec(edit, cfg) {
                    server.broadcast_bytes("vx.edit", buf);
                }
            }
            NetRole::Client { pending_edits, .. } => {
                pending_edits.push(edit);
            }
        }
        let center = [
            scoop_voxel[0] as f32 + 0.5,
            scoop_voxel[1] as f32 + 0.5,
            scoop_voxel[2] as f32 + 0.5,
        ];
        self.sfx_at("splash", center, |a, g| {
            let _ = a.play_thud(140.0, 0.18 * g);
            let _ = a.play_swish(0.16 * g);
        });
        true
    }

    /// Tinted variant. `color` is the linear-RGB albedo of the
    /// broken block; the debris cubes pick up that hue with a
    /// small per-particle brightness jitter so they read as
    /// material-matched chips instead of a generic puff.
    pub(crate) fn spawn_break_particles_tinted(&mut self, voxel: [i32; 3], color: [f32; 3]) {
        crate::ambient::spawn_break_particles_tinted(&mut self.particles, voxel, color);
    }
    pub(crate) fn spawn_place_particles(&mut self, voxel: [i32; 3], color: [f32; 3]) {
        crate::ambient::spawn_place_particles(&mut self.particles, voxel, color);
    }
    pub(crate) fn pickup(&mut self, block: BlockId) {
        if block == BlockId::AIR {
            return;
        }
        let _ = self.pickup_stack(crate::item::ItemStack::new(block.0, 1));
    }

    /// W468: insert a FULL stack (count + components) into the
    /// inventory - `same_stackable` merge first, then empty slots
    /// adopt the stack wholesale (components included). Returns the
    /// leftover count that didn't fit (0 = everything landed); the
    /// caller decides whether to re-spill it as a drop.
    pub(crate) fn pickup_stack(
        &mut self,
        stack: crate::item::ItemStack,
    ) -> u8 {
        if stack.id == 0 || stack.count == 0 {
            return 0;
        }
        let block = BlockId(stack.id);
        let incoming = stack.count;
        let leftover = crate::item::stack::drop_into_first_free_stacks(
            &mut self.inventory,
            stack,
        );
        let landed = incoming - leftover;
        if landed > 0 {
            self.note_pickup(block, landed);
        }
        leftover
    }
}
