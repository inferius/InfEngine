// Sprite shader for Phase 1.3.
//
// A single textured quad rendered with a 2D transform stored in a UBO.
// The transform packs (offset_x, offset_y, scale_x, scale_y) into a vec4
// so the whole uniform fits in 16 bytes.

struct SpriteUniform {
    transform: vec4<f32>,
};

@group(0) @binding(0) var<uniform> sprite: SpriteUniform;
@group(0) @binding(1) var sprite_texture: texture_2d<f32>;
@group(0) @binding(2) var sprite_sampler: sampler;

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
    let scale = sprite.transform.zw;
    let p = in.position * scale + offset;

    var out: VertexOutput;
    out.clip_position = vec4<f32>(p, 0.0, 1.0);
    out.uv = in.uv;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(sprite_texture, sprite_sampler, in.uv);
}
