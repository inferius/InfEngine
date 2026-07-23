//! W366 - bed block-entity renderer (MC `BedRenderer`).
//!
//! Beds leave the static chunk mesh exactly like chests: a bed cell
//! with a recorded part/facing state emits no chunk geometry
//! (`emit_special` returns early) and this pass draws every loaded
//! bed piece per frame with the real `entity/bed/<color>.png`
//! sheets. Cells WITHOUT a state row (pre-W366 saves) keep the old
//! wool-cube fallback so nothing turns invisible.
//!
//! MC source (`BedRenderer.createHeadLayer/createFootLayer`): each
//! piece is a 16x16x6 plate + two 3x3x3 legs on a 64x64 sheet,
//! authored upright and laid flat by a `translate(0, 9/16, 0) *
//! rotX(90)` model transform. Rather than porting the transform
//! chain, the boxes here are the RESULTING world-space cuboids
//! (mattress 16x6x16 spanning y 3..9 px, legs 3x3x3 at the outer
//! corners) with the sheet tiles assigned per world face:
//!
//!   head texOffs(0,0)  -> top (6,6)-(22,22)   bottom (28,6)-(44,22)
//!                         sides (0,6)/(22,6)  outer edge (6,0)-(22,6)
//!   foot texOffs(0,22) -> the same layout shifted +22 in V
//!   legs texOffs(50,v) -> standard 3x3x3 unwrap
//!
//! Local convention: +Z points from the foot toward the head. The
//! HEAD piece carries its legs at the +Z (pillow) edge, the FOOT
//! piece at the -Z edge; `draw_beds` spins both with the same
//! facing yaw so the pair lines up across the two cells.

use glam::Vec3;
use infengine_render::{MeshVertex, Renderer};

use crate::entity_model::{self, BakedBone, BakedModel, Bone, BonePose};

/// Facing bits 0-1 of the bed state byte (door convention:
/// 0=+X, 1=+Z, 2=-X, 3=-Z = the foot->head direction); bit 2 marks
/// the HEAD half (shared definition in block/mod.rs - the lib-side
/// template carve writes it).
pub(crate) use crate::block::BED_HEAD_BIT;

/// Baked head+foot models per dye color (MC dye order, ids 576+i;
/// the legacy red bed id 35 maps to `red`).
#[derive(Default)]
pub(crate) struct BedMeshes {
    pub(crate) by_color: Vec<Option<(BakedModel, BakedModel)>>,
}

impl BedMeshes {
    pub(crate) fn loaded(&self) -> bool {
        self.by_color.iter().any(|c| c.is_some())
    }
}

/// MC dye order - matches the `white_bed..black_bed` id run 576..591
/// and the `entity/bed/*.png` sheet names.
pub(crate) const BED_COLORS: [&str; 16] = [
    "white", "orange", "magenta", "light_blue", "yellow", "lime",
    "pink", "gray", "light_gray", "cyan", "purple", "blue", "brown",
    "green", "red", "black",
];

/// Dye index for a bed block id; the legacy `bed` (35) renders red.
pub(crate) fn bed_color_index(id: u16) -> usize {
    if (576..=591).contains(&id) {
        (id - 576) as usize
    } else {
        14 // red
    }
}

pub(crate) fn load_bed_meshes(
    r: &mut Renderer,
    entity_dir: Option<&std::path::Path>,
) -> BedMeshes {
    let mut out = BedMeshes { by_color: Vec::with_capacity(16) };
    let Some(dir) = entity_dir else {
        out.by_color.resize_with(16, || None);
        return out;
    };
    let bed_dir = dir.join("bed");
    for name in BED_COLORS {
        let p = bed_dir.join(format!("{name}.png"));
        let baked = match image::open(&p) {
            Ok(img) => {
                let img = img.to_rgba8();
                Some((bake_bed(r, &img, true), bake_bed(r, &img, false)))
            }
            Err(err) => {
                tracing::warn!(error = %err, path = %p.display(), "bed sheet load failed");
                None
            }
        };
        out.by_color.push(baked);
    }
    tracing::info!(loaded = out.loaded(), "bed entity meshes baked");
    out
}

/// Bake one bed piece (head or foot) from a 64x64 sheet. Boxes are
/// in the engine's entity convention (Y-down MC pixels, origin at
/// the block's bottom-center; `bone_matrix` flips X+Y and scales
/// 1/16), i.e. world y0..y1 px becomes engine -y1..-y0.
fn bake_bed(r: &mut Renderer, img: &image::RgbaImage, head: bool) -> BakedModel {
    let albedo = r.make_mesh_albedo(img.width(), img.height(), img);
    let (tw, th) = (img.width() as f32, img.height() as f32);
    let mut verts: Vec<MeshVertex> = Vec::new();
    let mut idxs: Vec<u32> = Vec::new();

    let uv = |pu: f32, pv: f32| -> [f32; 2] { [pu / tw, pv / th] };
    let mut add = |corners: [[f32; 3]; 4], normal: [f32; 3], uvs: [[f32; 2]; 4]| {
        let base = verts.len() as u32;
        for i in 0..4 {
            verts.push(
                MeshVertex::new(corners[i], normal)
                    .with_uv(uvs[i])
                    .with_light(0.0, 1.0),
            );
        }
        idxs.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
    };

    // ---- mattress plate: world (0,3,0)..(16,9,16) px -------------
    let (x0, x1) = (-8.0f32, 8.0f32);
    let (y0, y1) = (-9.0f32, -3.0f32); // engine y (world 9..3)
    let (z0, z1) = (-8.0f32, 8.0f32);
    // Sheet V origin: head plate at texOffs(0,0), foot at (0,22).
    let tv = if head { 0.0 } else { 22.0 };
    // World-TOP (blanket/pillow art, the 16x16 "north" tile). V runs
    // toward the head (+Z at V0) so the pillow sits at the head edge.
    add(
        [[x1, y0, z1], [x1, y0, z0], [x0, y0, z0], [x0, y0, z1]],
        [0.0, -1.0, 0.0],
        [uv(6.0, tv + 6.0), uv(6.0, tv + 22.0), uv(22.0, tv + 22.0), uv(22.0, tv + 6.0)],
    );
    // World-BOTTOM (the 16x16 "south" tile).
    add(
        [[x0, y1, z1], [x0, y1, z0], [x1, y1, z0], [x1, y1, z1]],
        [0.0, 1.0, 0.0],
        [uv(28.0, tv + 6.0), uv(28.0, tv + 22.0), uv(44.0, tv + 22.0), uv(44.0, tv + 6.0)],
    );
    // Long sides (6px tall x 16px run, the "west"/"east" tiles; U =
    // mattress height, V = run along Z).
    add(
        [[x1, y1, z1], [x1, y1, z0], [x1, y0, z0], [x1, y0, z1]],
        [1.0, 0.0, 0.0],
        [uv(0.0, tv + 6.0), uv(0.0, tv + 22.0), uv(6.0, tv + 22.0), uv(6.0, tv + 6.0)],
    );
    add(
        [[x0, y1, z0], [x0, y1, z1], [x0, y0, z1], [x0, y0, z0]],
        [-1.0, 0.0, 0.0],
        [uv(22.0, tv + 22.0), uv(22.0, tv + 6.0), uv(28.0, tv + 6.0), uv(28.0, tv + 22.0)],
    );
    // Outer end edge (16x6 "up" tile): +Z for the head piece
    // (behind the pillow), -Z for the foot piece.
    if head {
        add(
            [[x0, y1, z1], [x1, y1, z1], [x1, y0, z1], [x0, y0, z1]],
            [0.0, 0.0, 1.0],
            [uv(6.0, tv + 6.0), uv(22.0, tv + 6.0), uv(22.0, tv), uv(6.0, tv)],
        );
    } else {
        add(
            [[x1, y1, z0], [x0, y1, z0], [x0, y0, z0], [x1, y0, z0]],
            [0.0, 0.0, -1.0],
            [uv(6.0, tv + 6.0), uv(22.0, tv + 6.0), uv(22.0, tv), uv(6.0, tv)],
        );
    }

    // ---- legs: 3x3x3 at the two outer corners --------------------
    // Sheet rows per MC texOffs: head legs at v=6/18, foot at v=0/12.
    let (leg_v_a, leg_v_b) = if head { (6.0, 18.0) } else { (0.0, 12.0) };
    // Outer edge z (engine): head legs sit at +Z, foot legs at -Z.
    let (lz0, lz1) = if head { (5.0, 8.0) } else { (-8.0, -5.0) };
    let mut leg = |lx0: f32, v: f32| {
        let (lx1, ly0, ly1) = (lx0 + 3.0, -3.0f32, 0.0f32);
        // Standard 3x3x3 unwrap: up tile (53,v)-(56,v+3); side strip
        // v+3..v+6 with west/north/east/south at u=50/53/56/59.
        // World-TOP of the leg is hidden under the mattress; the
        // world-BOTTOM (engine +y) shows the "up" tile.
        add(
            [[lx0, ly1, lz1], [lx0, ly1, lz0], [lx1, ly1, lz0], [lx1, ly1, lz1]],
            [0.0, 1.0, 0.0],
            [uv(53.0, v), uv(53.0, v + 3.0), uv(56.0, v + 3.0), uv(56.0, v)],
        );
        // Sides (axis-true; the MC per-leg rotations permute these
        // 3px tiles - visually identical wood grain).
        add(
            [[lx1, ly1, lz1], [lx1, ly1, lz0], [lx1, ly0, lz0], [lx1, ly0, lz1]],
            [1.0, 0.0, 0.0],
            [uv(56.0, v + 6.0), uv(59.0, v + 6.0), uv(59.0, v + 3.0), uv(56.0, v + 3.0)],
        );
        add(
            [[lx0, ly1, lz0], [lx0, ly1, lz1], [lx0, ly0, lz1], [lx0, ly0, lz0]],
            [-1.0, 0.0, 0.0],
            [uv(50.0, v + 6.0), uv(53.0, v + 6.0), uv(53.0, v + 3.0), uv(50.0, v + 3.0)],
        );
        add(
            [[lx0, ly1, lz1], [lx1, ly1, lz1], [lx1, ly0, lz1], [lx0, ly0, lz1]],
            [0.0, 0.0, 1.0],
            [uv(53.0, v + 6.0), uv(56.0, v + 6.0), uv(56.0, v + 3.0), uv(53.0, v + 3.0)],
        );
        add(
            [[lx1, ly1, lz0], [lx0, ly1, lz0], [lx0, ly0, lz0], [lx1, ly0, lz0]],
            [0.0, 0.0, -1.0],
            [uv(59.0, v + 6.0), uv(62.0, v + 6.0), uv(62.0, v + 3.0), uv(59.0, v + 3.0)],
        );
    };
    leg(-8.0, leg_v_a);
    leg(5.0, leg_v_b);

    let mesh = r.create_mesh(if head { "bed_head" } else { "bed_foot" }, &verts, &idxs);
    r.bind_mesh_albedo_arc(mesh, albedo.clone());
    const BED_BONE: Bone = Bone {
        parent: None,
        name: "bed",
        pivot: [0.0, 0.0, 0.0],
        rotation: [0.0, 0.0, 0.0],
        cubes: &[],
    };
    BakedModel {
        bones: vec![BakedBone { bone: BED_BONE, mesh }],
        ground_offset: 0.0,
        model_scale: 1.0,
    }
}

/// Facing code (0=+X, 1=+Z, 2=-X, 3=-Z) -> yaw so local +Z points
/// along the facing (foot -> head direction).
fn yaw_for_facing(f: u8) -> f32 {
    match f & 3 {
        0 => -std::f32::consts::FRAC_PI_2, // +X
        1 => 0.0,                          // +Z
        2 => std::f32::consts::FRAC_PI_2,  // -X
        _ => std::f32::consts::PI,         // -Z
    }
}

/// Draw every bed piece with a recorded state. `states` is the bed
/// slice of the orientation map: facing bits 0-1, BED_HEAD_BIT for
/// the head half.
pub(crate) fn draw_beds(
    r: &mut Renderer,
    meshes: &BedMeshes,
    states: &std::collections::HashMap<[i32; 3], u8>,
    world: &infengine_voxel::World,
    player_pos: Vec3,
) {
    if !meshes.loaded() {
        return;
    }
    const DRAW_RANGE_SQ: f32 = 96.0 * 96.0;
    for (&pos, &state) in states {
        let id = world.get(pos).0;
        if !crate::block::is_bed_block(id) {
            continue;
        }
        let center = Vec3::new(
            pos[0] as f32 + 0.5,
            pos[1] as f32,
            pos[2] as f32 + 0.5,
        );
        if (center - player_pos).length_squared() > DRAW_RANGE_SQ {
            continue;
        }
        let Some(Some((head, foot))) =
            meshes.by_color.get(bed_color_index(id))
        else {
            continue;
        };
        let baked = if state & BED_HEAD_BIT != 0 { head } else { foot };
        entity_model::draw(
            r,
            baked,
            center,
            yaw_for_facing(state),
            [1.0, 1.0, 1.0, 1.0],
            |_| BonePose::default(),
        );
    }
}
