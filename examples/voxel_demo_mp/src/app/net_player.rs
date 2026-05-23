//! W47.28 - net_player impl block (relocated from app/mod.rs).
#![allow(unused_imports)]
use crate::*;
use crate::lighting::HaloLighting;

impl Voxel {
    /// is fresh (no save yet) - in that case we just carry on with the
    /// procedurally generated world.
    pub(crate) fn load_server_state(&mut self) {
        // First-run legacy migration: if the pre-A4 single-file save
        // is still in CWD + the new worlds tree is empty, create a
        // `worlds/legacy/` slot.
        match world_save::migrate_legacy_if_needed() {
            Ok(Some(legacy_slug)) => {
                self.current_world_slug = Some(legacy_slug);
            }
            Ok(None) => {}
            Err(err) => warn!(error = %err, "world_save: legacy migration failed"),
        }
        let slug = self
            .current_world_slug
            .clone()
            .unwrap_or_else(|| world_save::DEFAULT_SLUG.to_string());
        match world_save::load_world(self, &slug) {
            Ok(_) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                // Fresh slot - no save on disk yet. Carry on with the
                // procedurally generated world; the autosave loop will
                // mint the slot on the next save tick.
                self.current_world_slug = Some(slug);
            }
            Err(err) => warn!(error = %err, "world_save: load_world failed"),
        }
        // Phase L.7: lighting is never persisted - derive it from
        // the freshly-loaded block grid. After this every loaded
        // chunk has its block + sky light populated and the next
        // frame's `rebuild_all_meshes` pass picks the values up
        // into the GPU buffers.
        self.rebuild_all_lighting();
        // W46.20 / W46.22: emitter_positions / torch_positions /
        // fires now get populated INSIDE apply_chunk_payload during
        // load_world (one pass over each chunk's voxels instead of
        // a second full-world scan). The redundant rebuild_all
        // walk is gone - it was adding ~5-10 s of chunk-clone work
        // on top of an already-slow lighting BFS, pushing load time
        // past 90 s on 11k-chunk saves.
    }

    /// Raycast against every remote player's AABB. Returns the
    /// nearest hit `(id, distance)` within `max_dist`, or `None`.
    /// Skips our own id so we never target ourselves. Uses the
    /// interpolated render pose so the hit-test matches what the
    /// player actually sees on screen.
    pub(crate) fn raycast_remote_player(
        &self,
        origin: Vec3,
        dir: Vec3,
        max_dist: f32,
    ) -> Option<(u32, f32)> {
        let own_id: u32 = match &self.net {
            NetRole::Offline => return None,
            NetRole::Server { .. } => 0,
            NetRole::Client { client, .. } => client.client_id.unwrap_or(0),
        };
        let now = std::time::Instant::now();
        let mut best: Option<(u32, f32)> = None;
        for (&id, interp) in self.remote_interp.iter() {
            if id == own_id {
                continue;
            }
            let p = interp.sample(now);
            // Use the same body+head extents as the humanoid
            // render. Box centred at pose - (0, 0.7, 0).
            let cx = p.pos[0];
            let cy = p.pos[1] - 0.7;
            let cz = p.pos[2];
            let hx = 0.35;
            let hy = 1.0;
            let hz = 0.35;
            let min = [cx - hx, cy - hy, cz - hz];
            let max = [cx + hx, cy + hy, cz + hz];
            if let Some(t) = ray_aabb(origin.into(), dir.into(), min, max)
                && t <= max_dist
                && best.is_none_or(|(_, bt)| t < bt)
            {
                best = Some((id, t));
            }
        }
        best
    }

    /// Pick the nearest player position (host + remote pose
    /// table) within `range` of `from`. Returns `None` if no
    /// player is in range or there are no players known. Used by
    /// zombie AI to pick a chase target.
    pub(crate) fn nearest_player_pos(&self, from: [f32; 3], range: f32) -> Option<[f32; 3]> {
        let r2 = range * range;
        let mut best: Option<([f32; 3], f32)> = None;
        let mut consider = |p: [f32; 3]| {
            let dx = p[0] - from[0];
            let dy = p[1] - from[1];
            let dz = p[2] - from[2];
            let d2 = dx * dx + dy * dy + dz * dz;
            if d2 <= r2 && best.is_none_or(|(_, b)| d2 < b) {
                best = Some((p, d2));
            }
        };
        // Host's own pose is always in play.
        consider([self.player_pos.x, self.player_pos.y, self.player_pos.z]);
        // Plus every remote player tracked by the server pose
        // table (clients don't have other peers' poses authoritatively).
        if let NetRole::Server { poses, .. } = &self.net {
            for p in poses.values() {
                if p.id == 0 {
                    continue;
                }
                consider(p.pos);
            }
        }
        best.map(|(p, _)| p)
    }

    /// Probe the voxel column directly under the player's feet
    /// at world XZ `(wx, wz)` and return true if any cell across
    /// the player's collision footprint is solid. Used by the
    /// sneak edge-clamp so a grounded sneaking player can't walk
    /// off the side of a block.
    pub(crate) fn has_floor_under(&self, wx: f32, wz: f32) -> bool {
        let foot_y = (self.player_pos.y - PLAYER_HEIGHT - 0.05).floor() as i32;
        // The player AABB is `(wx - r, wz - r) -> (wx + r, wz + r)`
        // at foot height. Check the 4 corner cells (or fewer when
        // the player straddles fewer voxels) so we don't miss the
        // diagonal case where the supporting block sits on one
        // corner only.
        let r = PLAYER_RADIUS - COLLISION_EPSILON;
        let xs = [(wx - r).floor() as i32, (wx + r).floor() as i32];
        let zs = [(wz - r).floor() as i32, (wz + r).floor() as i32];
        for x in xs {
            for z in zs {
                if !self.world.get([x, foot_y, z]).is_air() {
                    return true;
                }
            }
        }
        false
    }

    /// Apply HP damage to the local player. Thin wrapper around
    /// `player::apply_damage` so existing call sites
    /// (`self.apply_player_damage(...)`) keep working.
    pub(crate) fn apply_player_damage(&mut self, amount: f32) {
        player::apply_damage(self, amount);
    }

    /// Variant that tags the source of the damage. Thin wrapper
    /// around `player::apply_damage_kind`.
    pub(crate) fn apply_player_damage_kind(&mut self, amount: f32, kind: DamageKind) {
        player::apply_damage_kind(self, amount, kind);
    }

    /// Fire-and-forget `vx.leave` to the server (if any). Called
    /// on explicit disconnect / quit so the server doesn't have
    /// to wait its 5 s silence reaper to broadcast a removal.
    pub(crate) fn notify_server_leaving(&self) {
        if let NetRole::Client { client, .. } = &self.net {
            let _ = client.send_bytes("vx.leave", Vec::new());
        }
    }
}
