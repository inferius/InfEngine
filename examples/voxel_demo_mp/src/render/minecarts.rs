//! W134 - minecart entity renderer.
//!
//! Carts are free entities (not blocks): this pass draws every
//! local cart (`Voxel::carts`, offline/host simulation) plus every
//! remote snapshot (`Voxel::remote_carts`, client view of the
//! host's `vx.cart` stream) through the shared `entity_model`
//! pipeline with MC's `textures/entity/minecart.png`.

use glam::Vec3;
use infengine_render::Renderer;

use crate::entity_model::{self, BakedModel, BonePose};

/// Baked minecart model. `Default` = nothing loaded (fallback
/// pack) - the draw pass just skips.
#[derive(Default)]
pub(crate) struct MinecartMeshes {
    pub(crate) model: Option<BakedModel>,
}

impl MinecartMeshes {
    pub(crate) fn loaded(&self) -> bool {
        self.model.is_some()
    }
}

/// Load + bake the minecart model from `textures/entity/`.
pub(crate) fn load_minecart_meshes(
    r: &mut Renderer,
    entity_dir: Option<&std::path::Path>,
) -> MinecartMeshes {
    let mut out = MinecartMeshes::default();
    let Some(dir) = entity_dir else { return out };
    let p = dir.join("minecart.png");
    match image::open(&p) {
        Ok(img) => {
            out.model = Some(entity_model::bake(
                r,
                &entity_model::minecart::MODEL,
                &img.to_rgba8(),
            ));
        }
        Err(err) => {
            tracing::warn!(
                error = %err,
                path = %p.display(),
                "minecart sheet load failed",
            );
        }
    }
    tracing::info!(loaded = out.loaded(), "minecart entity mesh baked");
    out
}

/// Draw every cart. Local carts are authoritative on offline/host;
/// clients hold none and draw the broadcast snapshots instead.
pub(crate) fn draw_minecarts(
    r: &mut Renderer,
    meshes: &MinecartMeshes,
    carts: &[crate::minecart::Minecart],
    remote: &std::collections::HashMap<u32, crate::net::CartMsg>,
) {
    let Some(model) = &meshes.model else { return };
    for c in carts {
        entity_model::draw(r, model, c.pos, c.yaw, [1.0; 4], |_| {
            BonePose::default()
        });
    }
    for m in remote.values() {
        entity_model::draw(r, model, Vec3::from(m.pos), m.yaw, [1.0; 4], |_| {
            BonePose::default()
        });
    }
}
