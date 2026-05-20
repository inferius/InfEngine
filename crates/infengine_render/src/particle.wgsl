// Textured billboard particle shader (Wave 18.1).
//
// One instance = one world-space billboard quad that always faces
// the camera (spherical billboard).  The vertex shader synthesises
// 4 corners from gl_vertex_index without a separate geometry buffer.
//
// Bind groups:
//   @group(0) - camera_3d uniform (shared with mesh3d pipeline)
//   @group(1) - particle atlas texture + sampler

struct CameraUniform {
    view_proj: mat4x4<f32>,
    view_proj_inv: mat4x4<f32>,
    camera_pos: vec4<f32>,
    time: vec4<f32>,
};

@group(0) @binding(0) var<uniform> camera: CameraUniform;

@group(1) @binding(0) var atlas_tex: texture_2d<f32>;
@group(1) @binding(1) var atlas_smp: sampler;

// Per-instance vertex attributes (step_mode = Instance).
struct Instance {
    @location(0) center_world: vec3<f32>,
    @location(1) size: f32,
    @location(2) uv_min: vec2<f32>,
    @location(3) uv_max: vec2<f32>,
    @location(4) color: vec4<f32>,
};

struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

// Corner offsets in camera-right / camera-up space.
// TriangleStrip order: bottom-left, top-left, bottom-right, top-right.
const CORNER_X: array<f32, 4> = array<f32, 4>(-1.0,  -1.0,  1.0,  1.0);
const CORNER_Y: array<f32, 4> = array<f32, 4>(-1.0,   1.0, -1.0,  1.0);

@vertex
fn vs_main(
    inst: Instance,
    @builtin(vertex_index) vi: u32,
) -> VsOut {
    // Camera right and up are the first three elements of view-proj
    // row-vectors (columns in the transposed view matrix).
    // Extract them from the camera's view matrix embedded in
    // view_proj_inv: the right and up world-space vectors are the
    // first two columns of the inverse (or equivalently the first
    // two rows of the view matrix).  For a pure look-at camera these
    // are the pure right/up axes.
    let vpi = camera.view_proj_inv;
    // vpi * (1,0,0,0) = first column = world right axis.
    let cam_right = normalize(vec3<f32>(vpi[0][0], vpi[0][1], vpi[0][2]));
    // vpi * (0,1,0,0) = second column = world up axis.
    let cam_up    = normalize(vec3<f32>(vpi[1][0], vpi[1][1], vpi[1][2]));

    let offset = (CORNER_X[vi] * cam_right + CORNER_Y[vi] * cam_up) * inst.size;
    let world_pos = vec4<f32>(inst.center_world + offset, 1.0);

    // UV: bottom-left = (uv_min.x, uv_max.y), top-right = (uv_max.x, uv_min.y).
    // Map corner index to the correct UV corner.
    let u = select(inst.uv_min.x, inst.uv_max.x, CORNER_X[vi] > 0.0);
    let v = select(inst.uv_max.y, inst.uv_min.y, CORNER_Y[vi] > 0.0);

    var out: VsOut;
    out.clip_pos = camera.view_proj * world_pos;
    out.uv       = vec2<f32>(u, v);
    out.color    = inst.color;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let texel = textureSample(atlas_tex, atlas_smp, in.uv);
    // Discard fully-transparent pixels so depth sorting artefacts
    // don't show transparent holes from the rear side.
    if texel.a * in.color.a < 0.01 {
        discard;
    }
    // Standard non-premultiplied alpha blend with instance color tint.
    return vec4<f32>(texel.rgb * in.color.rgb, texel.a * in.color.a);
}
