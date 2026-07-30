//! W463 - evoker fangs (`EvokerFangs.java` + the evoker's
//! `EvokerAttackSpellGoal`).
//!
//! MC: the evoker casts a fang attack on a ~5 s cadence. A target
//! inside 3 blocks gets two RINGS (5 fangs at r=1.5, 8 at r=2.5);
//! a target out to 12 blocks gets a LINE of 16 fangs spaced 1.25
//! blocks toward it, each with one extra tick of warmup. A fang
//! sits for its warmup, erupts, bites once (~6 HP) and sinks.
//!
//! Host-simulated like the witch potion: damage reaches the host
//! player directly and remote players over `vx.damage`. Clients
//! receive one `vx.fang` spawn per fang and tick a local visual
//! copy - lifetimes are deterministic, so no removal traffic.

use glam::Vec3;

use crate::{NetRole, Voxel};

/// MC `EvokerFangs.ATTACK_DAMAGE` flavour (6.0).
const FANG_DAMAGE: f32 = 6.0;
/// Seconds of eruption before the bite lands (MC bites at ~14
/// warmup-free ticks; the visible pop is short).
const FANG_BITE_AT: f32 = 0.25;
/// Total visible lifetime after the warmup runs out.
const FANG_LIFETIME: f32 = 0.55;

/// One live fang. `warmup > 0` = still underground.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Fang {
    pub(crate) pos: Vec3,
    pub(crate) warmup: f32,
    pub(crate) age: f32,
    pub(crate) bit: bool,
}

/// Snap a fang column to the ground: walk down up to 4 cells from
/// `y` to the first solid floor (MC scans block support the same
/// way); returns the surface Y to stand on.
fn floor_at(world: &infengine_voxel::World, x: f32, y: f32, z: f32) -> f32 {
    let cx = x.floor() as i32;
    let cz = z.floor() as i32;
    let mut cy = y.floor() as i32;
    for _ in 0..4 {
        let below = world.get([cx, cy - 1, cz]);
        if !below.is_air() && crate::block::block_fully_occludes(below) {
            return cy as f32;
        }
        cy -= 1;
    }
    y
}

fn push_fang(vx: &mut Voxel, pos: Vec3, warmup: f32) {
    vx.evoker_fangs.push(Fang {
        pos,
        warmup,
        age: 0.0,
        bit: false,
    });
    if let NetRole::Server { server, .. } = &vx.net {
        let cfg = bincode::config::standard();
        let msg: ([f32; 3], f32) = ([pos.x, pos.y, pos.z], warmup);
        if let Ok(buf) = bincode::serde::encode_to_vec(msg, cfg) {
            server.broadcast_bytes("vx.fang", buf);
        }
    }
}

/// Cast the MC fang pattern from `from` (the evoker's feet) toward
/// `target` (a player's eye pos).
pub(crate) fn cast_fangs(vx: &mut Voxel, from: Vec3, target: Vec3) {
    let dx = target.x - from.x;
    let dz = target.z - from.z;
    let dh = (dx * dx + dz * dz).sqrt();
    if dh < 3.0 {
        // Close: two defensive rings around the caster.
        for (count, radius, base_warmup) in
            [(5u32, 1.5f32, 0.0f32), (8, 2.5, 0.15)]
        {
            for i in 0..count {
                let a = i as f32 / count as f32 * std::f32::consts::TAU;
                let x = from.x + a.cos() * radius;
                let z = from.z + a.sin() * radius;
                let y = floor_at(&vx.world, x, from.y + 1.0, z);
                push_fang(vx, Vec3::new(x, y, z), base_warmup);
            }
        }
    } else {
        // Line of 16 toward the target, one warmup step apart.
        let ux = dx / dh;
        let uz = dz / dh;
        for i in 0..16u32 {
            let d = 1.25 * (i as f32 + 1.0);
            let x = from.x + ux * d;
            let z = from.z + uz * d;
            let y = floor_at(&vx.world, x, from.y + 1.0, z);
            push_fang(vx, Vec3::new(x, y, z), 0.05 * i as f32);
        }
    }
}

/// Client-side spawn from a `vx.fang` message.
pub(crate) fn spawn_remote(vx: &mut Voxel, pos: [f32; 3], warmup: f32) {
    vx.evoker_fangs.push(Fang {
        pos: Vec3::from(pos),
        warmup,
        age: 0.0,
        bit: false,
    });
}

/// Tick every fang. Clients run the same countdowns for the visual
/// copy; only the host applies damage.
pub(crate) fn tick_fangs(vx: &mut Voxel, dt: f32) {
    if vx.evoker_fangs.is_empty() {
        return;
    }
    let host = !matches!(vx.net, NetRole::Client { .. });
    let mut host_bites = 0u32;
    let mut remote_bites: Vec<u32> = Vec::new();
    {
        let player = vx.player_pos;
        let creative = vx.game_mode.is_creative();
        let poses: Vec<(u32, Vec3)> = match &vx.net {
            NetRole::Server { poses, .. } => poses
                .values()
                .filter(|p| p.id != 0)
                .map(|p| (p.id, Vec3::from(p.pos)))
                .collect(),
            _ => Vec::new(),
        };
        for f in vx.evoker_fangs.iter_mut() {
            if f.warmup > 0.0 {
                f.warmup -= dt;
                continue;
            }
            f.age += dt;
            if host && !f.bit && f.age >= FANG_BITE_AT {
                f.bit = true;
                // Bite: players standing on the fang's cell.
                let in_bite = |eye: Vec3| -> bool {
                    let feet = eye - Vec3::new(0.0, 1.7, 0.0);
                    let dx = feet.x - f.pos.x;
                    let dz = feet.z - f.pos.z;
                    dx * dx + dz * dz < 0.9 * 0.9
                        && (feet.y - f.pos.y).abs() < 1.2
                };
                if !creative && in_bite(player) {
                    host_bites += 1;
                }
                for (pid, eye) in &poses {
                    if in_bite(*eye) {
                        remote_bites.push(*pid);
                    }
                }
            }
        }
    }
    for _ in 0..host_bites {
        vx.apply_player_damage_kind(FANG_DAMAGE, crate::DamageKind::Magic);
    }
    if let NetRole::Server { server, .. } = &vx.net {
        let cfg = bincode::config::standard();
        for pid in remote_bites {
            if let Ok(buf) = bincode::serde::encode_to_vec(
                crate::net::DamageMsg {
                    amount: FANG_DAMAGE,
                },
                cfg,
            ) {
                server.send_bytes_to_client(pid, "vx.damage", buf);
            }
        }
    }
    vx.evoker_fangs
        .retain(|f| f.warmup > 0.0 || f.age <= FANG_LIFETIME);
}
