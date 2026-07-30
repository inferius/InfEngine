//! W47.17 - remote + local player cube render, relocated from
//! on_update.
//!
//! Each visible peer renders as a two-cube silhouette: a wider
//! body cube + a slightly cooler-tinted head cube on top. Colours
//! come from a stable hash of the peer's id so the same player
//! always reads as the same colour. The interpolation buffer
//! (`remote_interp`) is populated by every received `vx.player`
//! regardless of role; the helper samples it at `now`. In
//! third-person camera mode the local player is appended to the
//! render list so the pulled-back camera has something to look at.

use std::collections::HashMap;

use glam::{Mat4, Vec3};
use infengine_render::{MeshHandle, Renderer};

use crate::{PlayerMsg, RemoteInterp};

/// Optional local-player override appended after the remote list
/// (used in third-person camera mode).
pub(crate) struct LocalPlayerRender {
    pub id: u32,
    pub pos: Vec3,
    pub yaw: f32,
    pub pitch: f32,
}

/// Draw every remote peer in `remote_interp` (skipping `own_id`) +
/// the optional `local` override.
///
/// `cube` is the cached cube mesh handle owned by the App.
pub(crate) fn draw(
    renderer: &mut Renderer,
    cube: MeshHandle,
    remote_interp: &HashMap<u32, RemoteInterp>,
    own_id: u32,
    include_remote: bool,
    local: Option<LocalPlayerRender>,
) {
    let now = std::time::Instant::now();
    let mut render_list: Vec<PlayerMsg> = if include_remote {
        remote_interp
            .iter()
            .filter(|(id, _)| **id != own_id)
            .map(|(_, r)| r.sample(now))
            .collect()
    } else {
        Vec::new()
    };
    if let Some(l) = local {
        render_list.push(PlayerMsg {
            id: l.id,
            pos: [l.pos.x, l.pos.y, l.pos.z],
            yaw: l.yaw,
            pitch: l.pitch,
            // W471: pose-only render entry; the wear-routing slot
            // field is irrelevant here.
            sel: crate::net::SEL_NONE,
        });
    }
    if render_list.is_empty() {
        return;
    }
    for p in render_list {
        // Hash id to a stable colour + a cooler head tone so the
        // two cubes read as separate body segments.
        let h = (p.id.wrapping_mul(2654435761)) as f32 / u32::MAX as f32;
        let tau = std::f32::consts::TAU;
        let body_color = [
            (0.4 + 0.5 * (h * tau).sin().abs()).min(1.0),
            (0.4 + 0.5 * ((h + 0.33) * tau).sin().abs()).min(1.0),
            (0.4 + 0.5 * ((h + 0.66) * tau).sin().abs()).min(1.0),
            1.0,
        ];
        let head_color = [
            (body_color[0] * 0.85).min(1.0),
            (body_color[1] * 0.85).min(1.0),
            (body_color[2] * 0.85).min(1.0),
            1.0,
        ];
        let rot = glam::Quat::from_rotation_y(p.yaw);
        let body = Mat4::from_scale_rotation_translation(
            Vec3::new(0.65, 1.0, 0.4),
            rot,
            Vec3::new(p.pos[0], p.pos[1] - 1.2, p.pos[2]),
        );
        renderer.draw_mesh_pbr(cube, body.to_cols_array(), body_color, 0.0, 0.7);
        let head = Mat4::from_scale_rotation_translation(
            Vec3::new(0.55, 0.55, 0.55),
            rot,
            Vec3::new(p.pos[0], p.pos[1] - 0.45, p.pos[2]),
        );
        renderer.draw_mesh_pbr(cube, head.to_cols_array(), head_color, 0.0, 0.7);
    }
}
