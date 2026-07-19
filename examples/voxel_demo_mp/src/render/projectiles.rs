//! W140 - arrow renderer.
//!
//! One crossed-quad mesh baked from MC `entity/projectiles/
//! arrow.png` (the 16x5 shaft strip, both planes), drawn per arrow
//! oriented along its velocity - the same cheap visual MC's
//! `ArrowRenderer` produces at gameplay distances. Local arrows
//! (host sim) + remote snapshots both draw here.

use glam::{Mat4, Vec3};
use infengine_render::{MeshHandle, MeshVertex, Renderer};

/// Baked arrow mesh. `Default` = not loaded (fallback pack).
#[derive(Default)]
pub(crate) struct ArrowMeshes {
    pub(crate) mesh: Option<MeshHandle>,
    /// W337: crossed-quad large fireball (the fire_charge sprite,
    /// MC FireballRenderer's billboard at ~1 block).
    pub(crate) fireball: Option<MeshHandle>,
}

impl ArrowMeshes {
    /// Mirrors the other entity-mesh bundles; the draw pass skips
    /// via the inner Option directly today.
    #[allow(dead_code)]
    pub(crate) fn loaded(&self) -> bool {
        self.mesh.is_some()
    }
}

/// Bake the crossed-quad arrow mesh from the pack's projectile
/// sheet. Local space: the arrow flies along +X, tail at -0.3,
/// tip at +0.3 (MC's rendered arrow is ~0.6 blocks long).
pub(crate) fn load_arrow_meshes(
    r: &mut Renderer,
    entity_dir: Option<&std::path::Path>,
) -> ArrowMeshes {
    let mut out = ArrowMeshes::default();
    let Some(dir) = entity_dir else { return out };
    let p = dir.join("projectiles").join("arrow.png");
    let img = match image::open(&p) {
        Ok(i) => i.to_rgba8(),
        Err(err) => {
            tracing::warn!(error = %err, path = %p.display(), "arrow sheet load failed");
            return out;
        }
    };
    let albedo = r.make_mesh_albedo(img.width(), img.height(), &img);
    let (tw, th) = (img.width() as f32, img.height() as f32);
    // The 16x5 shaft strip at the sheet's top-left.
    let (u0, v0, u1, v1) = (0.0 / tw, 0.0 / th, 16.0 / tw, 5.0 / th);
    const L: f32 = 0.3; // half length
    const H: f32 = 0.09; // half fin height
    let mut verts: Vec<MeshVertex> = Vec::new();
    let mut idxs: Vec<u32> = Vec::new();
    let mut quad = |corners: [[f32; 3]; 4], normal: [f32; 3]| {
        let base = verts.len() as u32;
        let uvs = [[u0, v1], [u1, v1], [u1, v0], [u0, v0]];
        for i in 0..4 {
            verts.push(
                MeshVertex::new(corners[i], normal)
                    .with_uv(uvs[i])
                    .with_light(0.0, 1.0),
            );
        }
        // Both windings so the flat quad reads from either side.
        idxs.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        idxs.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
    };
    // Vertical plane (fins up/down).
    quad(
        [[-L, -H, 0.0], [L, -H, 0.0], [L, H, 0.0], [-L, H, 0.0]],
        [0.0, 0.0, 1.0],
    );
    // Horizontal plane (fins left/right).
    quad(
        [[-L, 0.0, -H], [L, 0.0, -H], [L, 0.0, H], [-L, 0.0, H]],
        [0.0, 1.0, 0.0],
    );
    let mesh = r.create_mesh("voxel_demo_mp::arrow", &verts, &idxs);
    r.bind_mesh_albedo_arc(mesh, albedo);
    out.mesh = Some(mesh);
    // W337: the ghast fireball - crossed unit quads with the
    // item/fire_charge.png sprite (what MC's ItemEntityRenderer
    // shows for a large fireball at gameplay distance).
    let fp = dir
        .parent()
        .map(|p| p.join("item").join("fire_charge.png"));
    if let Some(fp) = fp
        && let Ok(img) = image::open(&fp)
    {
        let img = img.to_rgba8();
        let albedo = r.make_mesh_albedo(img.width(), img.height(), &img);
        let mut verts: Vec<MeshVertex> = Vec::new();
        let mut idxs: Vec<u32> = Vec::new();
        let mut quad = |corners: [[f32; 3]; 4], normal: [f32; 3]| {
            let base = verts.len() as u32;
            let uvs = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
            for i in 0..4 {
                verts.push(
                    MeshVertex::new(corners[i], normal)
                        .with_uv(uvs[i])
                        .with_light(0.0, 1.0),
                );
            }
            idxs.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
            idxs.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
        };
        const S: f32 = 0.5;
        quad(
            [[-S, -S, 0.0], [S, -S, 0.0], [S, S, 0.0], [-S, S, 0.0]],
            [0.0, 0.0, 1.0],
        );
        quad(
            [[0.0, -S, -S], [0.0, -S, S], [0.0, S, S], [0.0, S, -S]],
            [1.0, 0.0, 0.0],
        );
        let mesh = r.create_mesh("voxel_demo_mp::fireball", &verts, &idxs);
        r.bind_mesh_albedo_arc(mesh, albedo);
        out.fireball = Some(mesh);
    }
    tracing::info!("arrow projectile mesh baked");
    out
}

/// Draw every live fireball. Crossed quads read from any angle, so
/// no per-frame billboarding is needed.
pub(crate) fn draw_fireballs(
    r: &mut Renderer,
    meshes: &ArrowMeshes,
    fireballs: &[crate::projectile::Fireball],
    remote: &std::collections::HashMap<u32, crate::net::FireballMsg>,
) {
    let Some(mesh) = meshes.fireball else { return };
    let white = [1.0f32, 1.0, 1.0, 1.0];
    let mut draw = |pos: Vec3| {
        let m = Mat4::from_translation(pos);
        r.draw_mesh_pbr(mesh, m.to_cols_array(), white, 0.0, 0.9);
    };
    for f in fireballs {
        draw(f.pos);
    }
    for msg in remote.values() {
        draw(Vec3::from(msg.pos));
    }
}

/// Orient the arrow along `vel`: yaw from the horizontal heading,
/// pitch from the vertical component.
fn arrow_matrix(pos: Vec3, vel: Vec3) -> Mat4 {
    let dh = (vel.x * vel.x + vel.z * vel.z).sqrt();
    let yaw = (-vel.z).atan2(vel.x);
    let pitch = vel.y.atan2(dh);
    Mat4::from_translation(pos)
        * Mat4::from_rotation_y(yaw)
        * Mat4::from_rotation_z(pitch)
}

/// Draw every live arrow (host list + client snapshots).
pub(crate) fn draw_arrows(
    r: &mut Renderer,
    meshes: &ArrowMeshes,
    arrows: &[crate::projectile::Arrow],
    remote: &std::collections::HashMap<u32, crate::net::ArrowMsg>,
) {
    let Some(mesh) = meshes.mesh else { return };
    let white = [1.0f32, 1.0, 1.0, 1.0];
    for a in arrows {
        let m = arrow_matrix(a.pos, a.vel);
        r.draw_mesh_pbr(mesh, m.to_cols_array(), white, 0.0, 0.9);
    }
    for msg in remote.values() {
        let m = arrow_matrix(Vec3::from(msg.pos), Vec3::from(msg.vel));
        r.draw_mesh_pbr(mesh, m.to_cols_array(), white, 0.0, 0.9);
    }
}
