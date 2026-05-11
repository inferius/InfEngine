// Infinite ground-plane grid drawn at y = 0. Renders a fullscreen
// triangle, unprojects two world-space rays per fragment (near + far),
// intersects with y = 0, and shades the resulting world XZ point with
// a multi-frequency grid pattern plus coloured world axes. Distance
// fade hides aliasing near the horizon.

struct CameraUniform {
    view_proj: mat4x4<f32>,
    view_proj_inv: mat4x4<f32>,
    camera_pos: vec4<f32>,
};

@group(0) @binding(0) var<uniform> camera: CameraUniform;

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) near_world: vec3<f32>,
    @location(1) far_world: vec3<f32>,
};

fn unproject(ndc_xy: vec2<f32>, ndc_z: f32) -> vec3<f32> {
    let world = camera.view_proj_inv * vec4<f32>(ndc_xy, ndc_z, 1.0);
    return world.xyz / world.w;
}

@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> VOut {
    // Fullscreen triangle: covers [-1, 1] x [-1, 1] with no scissor gap.
    var pos = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    let p = pos[idx];
    var out: VOut;
    out.clip = vec4<f32>(p, 0.0, 1.0);
    // wgpu NDC: z in [0, 1]. 0 = near plane, 1 = far plane.
    out.near_world = unproject(p, 0.0);
    out.far_world = unproject(p, 1.0);
    return out;
}

struct FOut {
    @builtin(frag_depth) depth: f32,
    @location(0) color: vec4<f32>,
};

fn grid_alpha(coord: vec2<f32>, scale: f32) -> f32 {
    let scaled = coord * scale;
    let d = fwidth(scaled);
    let g = abs(fract(scaled - 0.5) - 0.5) / max(d, vec2<f32>(1e-4));
    let line = min(g.x, g.y);
    return 1.0 - min(line, 1.0);
}

@fragment
fn fs_main(in: VOut) -> FOut {
    let dir = in.far_world - in.near_world;
    // Avoid divide-by-zero when the view direction is parallel to the
    // ground plane.
    if (abs(dir.y) < 1e-6) {
        discard;
    }
    let t = -in.near_world.y / dir.y;
    if (t < 0.0 || t > 1.0) {
        discard;
    }
    let world = in.near_world + dir * t;

    // Major grid every 10 units, minor grid every 1 unit.
    let minor = grid_alpha(world.xz, 1.0) * 0.45;
    let major = grid_alpha(world.xz, 0.1) * 0.85;
    var alpha = max(minor, major);

    // Distance fade so the horizon stops aliasing into a moire mess.
    let cam_xz = camera.camera_pos.xz;
    let dist = length(world.xz - cam_xz);
    let fade = clamp(1.0 - dist / 60.0, 0.0, 1.0);
    alpha *= fade;

    if (alpha <= 0.001) {
        discard;
    }

    var color = vec3<f32>(0.55, 0.58, 0.62);
    // World axis colouring: X axis red along z=0, Z axis blue along x=0.
    let aw = fwidth(world.xz);
    if (abs(world.z) < aw.y * 1.5) {
        color = vec3<f32>(0.95, 0.25, 0.25);
        alpha = max(alpha, 0.9 * fade);
    }
    if (abs(world.x) < aw.x * 1.5) {
        color = vec3<f32>(0.30, 0.45, 1.0);
        alpha = max(alpha, 0.9 * fade);
    }

    // Reproject the world hit point so the grid integrates with the
    // 3D depth buffer (geometry occludes the grid where it should).
    let clip = camera.view_proj * vec4<f32>(world, 1.0);
    let depth = clip.z / clip.w;

    var out: FOut;
    out.depth = depth;
    out.color = vec4<f32>(color, alpha);
    return out;
}
