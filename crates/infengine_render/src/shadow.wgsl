// Depth-only pass for the directional shadow map. Renders mesh
// vertices through the light's orthographic projection; the fragment
// stage is omitted so only depth is written.

struct ShadowUniform {
    view_proj: mat4x4<f32>,
    light_dir: vec4<f32>,
};

@group(0) @binding(0) var<uniform> shadow: ShadowUniform;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) inst_row0: vec4<f32>,
    @location(2) inst_row1: vec4<f32>,
    @location(3) inst_row2: vec4<f32>,
    @location(4) inst_row3: vec4<f32>,
};

@vertex
fn vs_main(in: VertexInput) -> @builtin(position) vec4<f32> {
    let model = mat4x4<f32>(in.inst_row0, in.inst_row1, in.inst_row2, in.inst_row3);
    let world = model * vec4<f32>(in.position, 1.0);
    return shadow.view_proj * world;
}
