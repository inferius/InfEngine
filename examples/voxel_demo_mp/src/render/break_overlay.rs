//! W369 - block "crack" overlay (MC `destroy_stage_0..9`).
//!
//! While a hold-to-mine session runs, a slightly inflated cube
//! around the target voxel shows the progressive crack texture -
//! the visual heartbeat MC players read mining progress from (the
//! parity-matrix "animations" gap). The mesh is world-space and
//! rebuilt only when the `(voxel, stage)` pair changes, so a whole
//! session costs at most 10 tiny mesh builds; releasing LMB (or
//! finishing the break) clears it.
//!
//! Textures load per stage from `<textures>/block/destroy_stage_N.png`
//! (grayscale cracks with alpha), drawn as a transparent overlay so
//! the block's own faces stay visible underneath.

use std::path::Path;

use infengine_render::{MeshHandle, MeshVertex, Renderer};

/// Active overlay state: which voxel + stage the current mesh shows.
#[derive(Default)]
pub(crate) struct BreakOverlay {
    current: Option<([i32; 3], u8, MeshHandle)>,
}

impl BreakOverlay {
    pub(crate) fn clear(&mut self, r: &mut Renderer) {
        if let Some((_, _, h)) = self.current.take() {
            r.remove_mesh(h);
        }
    }
}

/// Sync the overlay with the mining session. `mining` is
/// `(voxel, progress, total)`; `None` clears.
pub(crate) fn update_break_overlay(
    r: &mut Renderer,
    state: &mut BreakOverlay,
    mining: Option<([i32; 3], f32, f32)>,
    tex_root: Option<&Path>,
) {
    let Some((voxel, progress, total)) = mining else {
        state.clear(r);
        return;
    };
    let frac = (progress / total.max(1e-3)).clamp(0.0, 0.999);
    let stage = (frac * 10.0) as u8;
    if let Some((v, s, _)) = state.current
        && v == voxel
        && s == stage
    {
        return;
    }
    state.clear(r);
    let Some(root) = tex_root else { return };
    let path = root.join("block").join(format!("destroy_stage_{stage}.png"));
    let Ok(img) = image::open(&path) else { return };
    let img = img.to_rgba8();

    // Inflated world-space cube: 6 faces, full-tile UV each. The
    // 0.008 inflate keeps it off the block's own faces (no z-fight)
    // without visibly floating.
    const E: f32 = 0.008;
    let (x0, y0, z0) = (
        voxel[0] as f32 - E,
        voxel[1] as f32 - E,
        voxel[2] as f32 - E,
    );
    let (x1, y1, z1) = (
        voxel[0] as f32 + 1.0 + E,
        voxel[1] as f32 + 1.0 + E,
        voxel[2] as f32 + 1.0 + E,
    );
    let mut verts: Vec<MeshVertex> = Vec::with_capacity(24);
    let mut idxs: Vec<u32> = Vec::with_capacity(36);
    let mut add = |corners: [[f32; 3]; 4], normal: [f32; 3]| {
        let base = verts.len() as u32;
        const UVS: [[f32; 2]; 4] = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
        for i in 0..4 {
            verts.push(
                MeshVertex::new(corners[i], normal)
                    .with_uv(UVS[i])
                    .with_light(0.0, 1.0),
            );
        }
        idxs.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
    };
    add([[x1, y0, z0], [x0, y0, z0], [x0, y1, z0], [x1, y1, z0]], [0.0, 0.0, -1.0]);
    add([[x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1]], [0.0, 0.0, 1.0]);
    add([[x1, y0, z1], [x1, y0, z0], [x1, y1, z0], [x1, y1, z1]], [1.0, 0.0, 0.0]);
    add([[x0, y0, z0], [x0, y0, z1], [x0, y1, z1], [x0, y1, z0]], [-1.0, 0.0, 0.0]);
    add([[x0, y1, z0], [x0, y1, z1], [x1, y1, z1], [x1, y1, z0]], [0.0, 1.0, 0.0]);
    add([[x0, y0, z1], [x0, y0, z0], [x1, y0, z0], [x1, y0, z1]], [0.0, -1.0, 0.0]);

    let mesh = r.create_mesh("break_overlay", &verts, &idxs);
    let albedo = r.make_mesh_albedo(img.width(), img.height(), &img);
    r.bind_mesh_albedo_arc(mesh, albedo);
    r.set_mesh_transparent(mesh, true);
    state.current = Some((voxel, stage, mesh));
}
