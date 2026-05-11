// Batched sprite shader.
//
// Group 0: camera view-projection (mat4x4, vertex stage).
// Group 1: sprite texture + sampler (fragment stage).
//
// The local quad mesh spans [-0.5, +0.5] in both axes. Per-instance
// attributes provide world position, size, color tint, and a UV
// sub-rectangle so a single draw can render N sprites that share a
// texture (or an atlas).

struct CameraUniform {
    view_proj: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> camera: CameraUniform;

@group(1) @binding(0) var sprite_texture: texture_2d<f32>;
@group(1) @binding(1) var sprite_sampler: sampler;

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,

    @location(2) inst_position: vec2<f32>,
    @location(3) inst_size: vec2<f32>,
    @location(4) inst_color: vec4<f32>,
    @location(5) inst_uv_min: vec2<f32>,
    @location(6) inst_uv_max: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    let world = in.position * in.inst_size + in.inst_position;
    let uv = mix(in.inst_uv_min, in.inst_uv_max, in.uv);

    var out: VertexOutput;
    out.clip_position = camera.view_proj * vec4<f32>(world, 0.0, 1.0);
    out.uv = uv;
    out.color = in.inst_color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let sample = textureSample(sprite_texture, sprite_sampler, in.uv);
    return sample * in.color;
}
