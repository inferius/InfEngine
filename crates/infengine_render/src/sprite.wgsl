// Sprite shader.
//
// Group 0: camera view-projection (mat4x4, vertex stage).
// Group 1: per-sprite transform (vec4 = offset_xy + size_xy in world units),
//          plus the sprite texture and a sampler.
//
// The local quad spans [-0.5, +0.5] in both axes, so multiplying by
// `size_xy` and adding `offset_xy` places the sprite in world space.

struct CameraUniform {
    view_proj: mat4x4<f32>,
};

struct SpriteUniform {
    transform: vec4<f32>,
};

@group(0) @binding(0) var<uniform> camera: CameraUniform;

@group(1) @binding(0) var<uniform> sprite: SpriteUniform;
@group(1) @binding(1) var sprite_texture: texture_2d<f32>;
@group(1) @binding(2) var sprite_sampler: sampler;

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    let offset = sprite.transform.xy;
    let size = sprite.transform.zw;
    let world = in.position * size + offset;

    var out: VertexOutput;
    out.clip_position = camera.view_proj * vec4<f32>(world, 0.0, 1.0);
    out.uv = in.uv;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(sprite_texture, sprite_sampler, in.uv);
}
