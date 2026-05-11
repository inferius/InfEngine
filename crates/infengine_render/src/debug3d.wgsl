// Plain coloured line list in 3D world space. Used for editor /
// runtime overlays (gizmos, voxel outlines, debug bounds).

struct CameraUniform {
    view_proj: mat4x4<f32>,
    view_proj_inv: mat4x4<f32>,
    camera_pos: vec4<f32>,
};

@group(0) @binding(0) var<uniform> camera: CameraUniform;

struct VIn {
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
};

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(in: VIn) -> VOut {
    var out: VOut;
    out.clip = camera.view_proj * vec4<f32>(in.position, 1.0);
    out.color = in.color;
    return out;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    return in.color;
}
