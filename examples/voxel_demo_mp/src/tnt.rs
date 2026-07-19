//! W338 - primed TNT (MC `PrimedTnt` entity over the W337
//! explosion).
//!
//! Flint-and-steel on a TNT block swaps it for the primed entity:
//! 80-tick fuse, the vanilla ignite pop-up (0.2 b/t vertical, a
//! whisper of random horizontal drift), entity gravity 0.04 b/t^2
//! with 0.98/tick drag and the 0.7 ground friction, then a power-4
//! block explosion (no fire, 1/4 drop yield - both fall out of the
//! shared `explosion::explode` math). TNT caught in another blast
//! primes with the vanilla short fuse (10 + rand 20 ticks) instead
//! of dropping - chain reactions included.
//!
//! Host-authoritative like arrows/fireballs: `vx.tnt` snapshots +
//! `vx.tnt_remove`; clients render `remote_tnt`. The visual is the
//! real textured TNT block cube (held-item mesh cache) with the
//! vanilla white flash cadence + the final swell.

use glam::Vec3;

use crate::Voxel;
use crate::block::TNT_ID;
use crate::net::{NetRole, TntMsg};
use infengine_voxel::BlockId;

/// MC fuse: 80 ticks.
const TNT_FUSE_S: f32 = 4.0;
/// Entity gravity 0.04 b/t^2 -> 16 b/s^2.
const TNT_GRAVITY: f32 = 16.0;
/// Per-tick air drag.
const TNT_DRAG_PER_TICK: f32 = 0.98;
/// `TntBlock.explode` radius.
const TNT_POWER: f32 = 4.0;

/// One live primed TNT (host side).
#[derive(Debug, Clone)]
pub(crate) struct PrimedTnt {
    pub(crate) id: u32,
    /// Feet-corner position of the block cube (like a block pos).
    pub(crate) pos: Vec3,
    pub(crate) vel: Vec3,
    pub(crate) fuse: f32,
}

/// Swap the clicked TNT block for the primed entity (flint+steel
/// RMB - `TntBlock.onCaughtFire`).
pub(crate) fn ignite_block(vx: &mut Voxel, voxel: [i32; 3]) {
    if vx.world.get(voxel).0 != TNT_ID {
        return;
    }
    vx.world_set_tracked(voxel, BlockId::AIR);
    let center = Vec3::new(
        voxel[0] as f32 + 0.5,
        voxel[1] as f32,
        voxel[2] as f32 + 0.5,
    );
    spawn_primed(vx, center, TNT_FUSE_S);
    vx.consume_flint_and_steel_use();
    vx.lmb_pending = false;
    vx.rmb_pending = false;
}

/// Mint a primed TNT with the vanilla ignite pop: 0.2 b/t up plus
/// a random whisper of horizontal drift.
pub(crate) fn spawn_primed(vx: &mut Voxel, center: Vec3, fuse: f32) {
    let id = vx.next_tnt_id;
    vx.next_tnt_id = vx.next_tnt_id.wrapping_add(1).max(1);
    let angle = (id as f32 * 2.399_963) % std::f32::consts::TAU;
    vx.primed_tnt.push(PrimedTnt {
        id,
        pos: center,
        vel: Vec3::new(-angle.sin() * 0.4, 4.0, -angle.cos() * 0.4),
        fuse,
    });
    vx.sfx_at("tnt/primed", [center.x, center.y, center.z], |_, _| {});
}

/// Fuse + physics + detonation (host only).
pub(crate) fn tick(vx: &mut Voxel, dt: f32) {
    if matches!(vx.net, NetRole::Client { .. }) {
        return;
    }
    if vx.primed_tnt.is_empty() {
        return;
    }
    let mut blasts: Vec<Vec3> = Vec::new();
    let mut removed: Vec<u32> = Vec::new();
    {
        let world = &vx.world;
        for t in vx.primed_tnt.iter_mut() {
            t.fuse -= dt;
            if t.fuse <= 0.0 {
                // MC explodes at getY(0.0625) - a touch above the feet.
                blasts.push(t.pos + Vec3::new(0.0, 0.06, 0.0));
                removed.push(t.id);
                continue;
            }
            t.vel.y -= TNT_GRAVITY * dt;
            t.vel *= TNT_DRAG_PER_TICK.powf(dt * 20.0);
            let mut next = t.pos + t.vel * dt;
            // Ground rest: the cube sits on the first solid top under
            // its centre (full collision is overkill for a 4 s prop).
            let below = [
                next.x.floor() as i32,
                (next.y - 0.01).floor() as i32,
                next.z.floor() as i32,
            ];
            let b = world.get(below);
            if t.vel.y <= 0.0
                && !b.is_air()
                && crate::block::block_fully_occludes(b)
            {
                next.y = below[1] as f32 + 1.0;
                t.vel.y = 0.0;
                // MC ground friction (0.7, -0.5, 0.7) - the pop-up
                // drift dies on landing.
                t.vel.x *= 0.7;
                t.vel.z *= 0.7;
            }
            t.pos = next;
        }
    }
    if !removed.is_empty() {
        vx.primed_tnt.retain(|t| !removed.contains(&t.id));
        if let NetRole::Server { server, .. } = &vx.net {
            let cfg = bincode::config::standard();
            for id in &removed {
                if let Ok(buf) = bincode::serde::encode_to_vec(*id, cfg) {
                    server.broadcast_bytes("vx.tnt_remove", buf);
                }
            }
        }
    }
    for c in blasts {
        crate::explosion::explode(vx, c, TNT_POWER, false);
    }
    if let NetRole::Server { server, .. } = &vx.net {
        let cfg = bincode::config::standard();
        for t in &vx.primed_tnt {
            let msg = TntMsg {
                id: t.id,
                pos: [t.pos.x, t.pos.y, t.pos.z],
                fuse: t.fuse,
            };
            if let Ok(buf) = bincode::serde::encode_to_vec(msg, cfg) {
                server.broadcast_bytes("vx.tnt", buf);
            }
        }
    }
}

/// Draw every primed TNT as the textured TNT block cube with MC's
/// white-flash cadence (fuse/5 ticks alternating) and the final
/// swell. Local sim + remote snapshots.
pub(crate) fn draw(
    r: &mut infengine_render::Renderer,
    cache: &mut crate::render::held_item::HeldMeshCache,
    tex_root: Option<&std::path::Path>,
    primed: &[PrimedTnt],
    remote: &std::collections::HashMap<u32, TntMsg>,
) {
    if primed.is_empty() && remote.is_empty() {
        return;
    }
    let Some(held) =
        crate::render::held_item::held_mesh_for(r, cache, tex_root, TNT_ID)
    else {
        return;
    };
    let mut draw_one = |pos: Vec3, fuse: f32| {
        let ticks = (fuse * 20.0).max(0.0);
        // TntRenderer: white overlay while (fuse/5)%2 == 0, plus a
        // swell over the last 10 ticks.
        let flash = ((ticks / 5.0) as i32) % 2 == 0;
        let swell = if ticks < 10.0 {
            1.0 + (1.0 - ticks / 10.0) * 0.3
        } else {
            1.0
        };
        let tint = if flash {
            [2.5f32, 2.5, 2.5, 1.0]
        } else {
            [1.0, 1.0, 1.0, 1.0]
        };
        // The held cube is a unit block about the origin; primed pos
        // is the feet centre.
        let m = glam::Mat4::from_translation(pos + Vec3::new(0.0, 0.5, 0.0))
            * glam::Mat4::from_scale(Vec3::splat(swell));
        r.draw_mesh_pbr(held.mesh, m.to_cols_array(), tint, 0.0, 0.8);
    };
    for t in primed {
        draw_one(t.pos, t.fuse);
    }
    for msg in remote.values() {
        draw_one(Vec3::from(msg.pos), msg.fuse);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use infengine_voxel::Chunk;

    /// Flint+steel on TNT swaps the block for a primed entity with
    /// the vanilla 4 s fuse and the ignite pop-up velocity.
    #[test]
    fn w338_ignite_swaps_block_for_primed() {
        let mut v = crate::Voxel::new();
        v.world.insert_chunk([0, 0, 0], Chunk::filled(BlockId(0)));
        v.world.set([4, 4, 4], BlockId(TNT_ID));
        ignite_block(&mut v, [4, 4, 4]);
        assert!(v.world.get([4, 4, 4]).is_air(), "block consumed");
        assert_eq!(v.primed_tnt.len(), 1);
        let t = &v.primed_tnt[0];
        assert!((t.fuse - 4.0).abs() < 1e-6);
        assert!(t.vel.y > 3.0, "ignite pop-up");
    }

    /// A zero-fuse primed TNT detonates power 4: the dirt floor
    /// craters AND stone breaks (unlike the ghast's power 1).
    #[test]
    fn w338_detonation_craters_stone() {
        let mut v = crate::Voxel::new();
        for cy in -1..=1 {
            for cx in -1..=0 {
                for cz in -1..=0 {
                    v.world.insert_chunk([cx, cy, cz], Chunk::filled(BlockId(0)));
                }
            }
        }
        for x in -8..=8 {
            for z in -8..=8 {
                v.world.set([x, 4, z], BlockId(1)); // stone floor
            }
        }
        v.primed_tnt.push(PrimedTnt {
            id: 1,
            pos: glam::Vec3::new(0.5, 5.0, 0.5),
            vel: glam::Vec3::ZERO,
            fuse: 0.001,
        });
        v.next_tnt_id = 2;
        tick(&mut v, 0.05);
        assert!(v.primed_tnt.is_empty(), "detonated");
        let stone_gone = (-4..=4)
            .flat_map(|x| (-4..=4).map(move |z| (x, z)))
            .filter(|&(x, z)| v.world.get([x, 4, z]).is_air())
            .count();
        assert!(stone_gone > 4, "power 4 must crack stone ({stone_gone})");
    }

    /// TNT caught in a blast primes with the short vanilla fuse
    /// instead of dropping - the chain-reaction path.
    #[test]
    fn w338_explosion_primes_neighbour_tnt() {
        let mut v = crate::Voxel::new();
        for cx in -1..=0 {
            for cz in -1..=0 {
                v.world.insert_chunk([cx, 0, cz], Chunk::filled(BlockId(0)));
            }
        }
        for x in -6..=6 {
            for z in -6..=6 {
                v.world.set([x, 4, z], BlockId(2)); // dirt floor
            }
        }
        v.world.set([1, 5, 0], BlockId(TNT_ID));
        crate::explosion::explode(
            &mut v,
            glam::Vec3::new(0.5, 5.5, 0.5),
            4.0,
            false,
        );
        assert!(v.world.get([1, 5, 0]).is_air(), "TNT consumed by blast");
        assert_eq!(v.primed_tnt.len(), 1, "primed, not dropped");
        let t = &v.primed_tnt[0];
        assert!(
            t.fuse >= 0.5 - 1e-6 && t.fuse <= 1.5 + 1e-6,
            "short chain fuse, got {}",
            t.fuse,
        );
        assert!(
            !v.dropped_items.iter().any(|d| d.block == TNT_ID),
            "no TNT item drop from the blast",
        );
    }
}
