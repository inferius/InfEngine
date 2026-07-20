// Baseline PBR-ish mesh shader.
//
// One directional sun + a tiny ambient term. Diffuse uses Lambert;
// specular uses Cook-Torrance with GGX distribution and a Schlick
// Fresnel. F0 lerps from 0.04 (dielectric) to the albedo for metals.
// Albedo is the per-instance color modulated by a sampled texture
// (defaults to 1x1 white so untextured meshes look unchanged).

struct CameraUniform {
    view_proj: mat4x4<f32>,
    view_proj_inv: mat4x4<f32>,
    camera_pos: vec4<f32>,
    // x = wall-clock seconds since app start.
    // y = Phase L day_factor: sky-light multiplier in [0, 1]
    //     (1.0 noon, 0.04 midnight, smoothstep through dawn/dusk).
    // z/w = reserved.
    time: vec4<f32>,
    // W30.5: fog parameters.
    // x = fog_start (world units, distance where fog begins).
    // y = fog_end   (world units, distance where fog is fully opaque).
    // fog is disabled when fog_start == fog_end == 0.
    // z/w = reserved.
    fog: vec4<f32>,
    // W30.5: sky/fog colour (linear RGB). Used as the fog target colour
    // when fog_end > fog_start.
    sky_color: vec4<f32>,
};

@group(0) @binding(0) var<uniform> camera: CameraUniform;

@group(1) @binding(0) var albedo_tex: texture_2d<f32>;
@group(1) @binding(1) var albedo_smp: sampler;

struct ShadowUniform {
    view_proj: mat4x4<f32>,
    light_dir: vec4<f32>,
};

@group(2) @binding(0) var<uniform> shadow: ShadowUniform;
@group(2) @binding(1) var shadow_map: texture_depth_2d;
@group(2) @binding(2) var shadow_smp: sampler_comparison;

// W46.27: bumped 8 -> 32. Matches the CPU-side MAX_LIGHTS in
// lights.rs. Modern desktop GPUs run the per-fragment 32-light
// loop with no visible cost.
const MAX_LIGHTS: u32 = 32u;

struct PointLight {
    position: vec4<f32>,
    color: vec4<f32>,
    params: vec4<f32>,
};

struct LightsUniform {
    count: vec4<u32>,
    lights: array<PointLight, 32>,
};

@group(3) @binding(0) var<uniform> point_lights: LightsUniform;

// Wave 9.y: per-tile animation table. The fragment shader picks
// `entries[anim_id - 1]` when a vertex's `anim_id != 0` and
// scrolls UV.v through the frame strip. Slot 0 is the static
// sentinel ("do not scroll") so the array is sized for 128 live
// animations + the index math stays 1-based. (W320: 32 -> 128,
// keep in sync with tile_anims::MAX_ANIM_SLOTS.)
struct TileAnim {
    frame_count: u32,
    frametime_ms: u32,
    v_step: f32,
    _pad: f32,
};

struct TileAnimsUniform {
    entries: array<TileAnim, 128>,
};

@group(4) @binding(0) var<uniform> tile_anims: TileAnimsUniform;

// W240: per-BLOCK draw parameters. Chunk meshes are merged into one
// draw per render layer; each vertex carries its block id in the
// HIGH bits of `anim_id` (low 8 bits stay the anim slot). Block id
// 0 = "no override" (non-chunk meshes: mobs, drops, liquids, UI) -
// those keep using the per-instance color/material.
struct BlockParam {
    color: vec4<f32>,
    // (metallic, roughness, emissive, _unused)
    material: vec4<f32>,
};

struct BlockParamsUniform {
    entries: array<BlockParam, 1024>,
};

@group(4) @binding(1) var<storage, read> block_params: BlockParamsUniform;

// Compute the active animated UV for a vertex's static (frame-0)
// UV rect. `anim_id == 0` short-circuits to the static UV so the
// shader's hot path stays a single branch. Otherwise we step
// `time_ms / frametime_ms` (modulo `frame_count`) and shift
// `v` by `frame * v_step`.
fn animated_uv(base_uv: vec2<f32>, anim_id: u32, time_ms: f32) -> vec2<f32> {
    if (anim_id == 0u) {
        return base_uv;
    }
    let anim = tile_anims.entries[anim_id - 1u];
    if (anim.frame_count <= 1u) {
        return base_uv;
    }
    let ft = max(f32(anim.frametime_ms), 1.0);
    let frame = u32(floor(time_ms / ft)) % anim.frame_count;
    return vec2<f32>(base_uv.x, base_uv.y + f32(frame) * anim.v_step);
}

// W30.1: packed vertex input. CPU-side types and offsets:
//   loc 0: position   [f32;3]  Float32x3  -> vec3<f32>   (passthrough)
//   loc 1: normal     [i8;4]   Sint8x4    -> vec4<i32>   (decode: .xyz / 127.0)
//   loc 2: uv         [u16;2]  Uint16x2   -> vec2<u32>   (decode: f32(c) / 65535.0) legacy
//   loc 9: light      [u8;4]   Unorm8x4   -> vec4<f32>   (already [0,1]; use .xy)
//  loc 10: anim_id    u32      Uint32     -> u32          (passthrough)
//  loc 11: tile_uv0   [u16;2]  Uint16x2   -> vec2<u32>   (atlas origin / 65535.0)
//  loc 12: tile_size  [u16;2]  Uint16x2   -> vec2<u32>   (atlas extents / 65535.0)
//  loc 13: quad_uv    [u16;2]  Uint16x2   -> vec2<u32>   (per-corner uv / 65535.0 * 16.0)
struct VertexInput {
    @location(0) position: vec3<f32>,
    // Sint8x4 -> vec4<i32> in WGSL.
    @location(1) normal_packed: vec4<i32>,
    // Uint16x2 -> vec2<u32> in WGSL. Legacy - superseded by tile_uv0/tile_size/quad_uv.
    @location(2) uv_packed: vec2<u32>,
    @location(3) inst_row0: vec4<f32>,
    @location(4) inst_row1: vec4<f32>,
    @location(5) inst_row2: vec4<f32>,
    @location(6) inst_row3: vec4<f32>,
    @location(7) inst_color: vec4<f32>,
    @location(8) inst_material: vec4<f32>,
    // Unorm8x4 -> vec4<f32> already in [0, 1]. .x = block, .y = sky.
    @location(9) light_packed: vec4<f32>,
    @location(10) anim_id: u32,
    // W30.1: tile atlas bounds. tile_uv0 = atlas origin; tile_size = atlas extents.
    // Both packed as u16 = component * 65535.
    @location(11) tile_uv0_packed: vec2<u32>,
    @location(12) tile_size_packed: vec2<u32>,
    // W30.1: per-corner UV in [0, 16] voxel coords.
    // Packed as u16 = (component / 16.0) * 65535.0.
    @location(13) quad_uv_packed: vec2<u32>,
};

struct VertexOutput {
    @builtin(position) clip: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) material: vec4<f32>,
    // W30 fix: uv carries tile_uv0 (atlas origin); the fract-based tile
    // wrap is computed in the fragment shader per-pixel using the
    // interpolated quad_uv. Vertex-shader fract was wrong because fract
    // is per-vertex - at integer voxel coords it always returned 0 so
    // every vertex got the same UV and the texture stretched.
    @location(4) uv: vec2<f32>,
    // .xy = (block, sky) light; .z = W122 ambient-occlusion (1 = none).
    @location(5) light: vec3<f32>,
    @location(7) tile_uv0_out: vec2<f32>,
    @location(8) tile_size_out: vec2<f32>,
    @location(9) quad_uv_out: vec2<f32>,
    // Wave 9.y: animation slot forwarded so the fragment shader
    // can pick the right tile_anims entry. Flat-interpolated
    // because every vertex of a quad shares the same slot.
    @location(6) @interpolate(flat) anim_id: u32,
    // W199: per-vertex water-column depth (light byte 3, normalized to
    // [0,1] over ~12 blocks). Drives depth-based surface opacity so deep
    // ocean reads opaque (no see-through "void") while shallow water /
    // ponds stay translucent. 0 for all non-water meshes.
    @location(10) water_depth: f32,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    let model = mat4x4<f32>(in.inst_row0, in.inst_row1, in.inst_row2, in.inst_row3);
    let world4 = model * vec4<f32>(in.position, 1.0);
    let normal_mat = mat3x3<f32>(model[0].xyz, model[1].xyz, model[2].xyz);

    // W30.1: decode packed inputs.
    // Sint8x4 components are i32 in WGSL [-128, 127]; scale to [-1, 1].
    let normal = vec3<f32>(
        f32(in.normal_packed.x) / 127.0,
        f32(in.normal_packed.y) / 127.0,
        f32(in.normal_packed.z) / 127.0,
    );
    // Unorm8x4 already in [0, 1]; .xy = (block, sky), .z = AO (W122).
    let light = in.light_packed.xyz;

    // W30 fix: only DECODE the packed tile/quad attrs here; fract() is
    // applied in the fragment shader where it sees the interpolated
    // per-pixel value. Passing fract-result from vertex shader would
    // collapse to 0 at integer voxel coords - the actual bug observed.
    let tile_uv0 = vec2<f32>(
        f32(in.tile_uv0_packed.x) / 65535.0,
        f32(in.tile_uv0_packed.y) / 65535.0,
    );
    let tile_size = vec2<f32>(
        f32(in.tile_size_packed.x) / 65535.0,
        f32(in.tile_size_packed.y) / 65535.0,
    );
    let quad_uv = vec2<f32>(
        f32(in.quad_uv_packed.x) / 65535.0 * 16.0,
        f32(in.quad_uv_packed.y) / 65535.0 * 16.0,
    );

    var out: VertexOutput;
    out.clip = camera.view_proj * world4;
    out.world_pos = world4.xyz;
    out.world_normal = normalize(normal_mat * normal);
    out.color = in.inst_color;
    out.material = in.inst_material;
    // Legacy `uv` slot keeps tile_uv0 so any code path that still reads
    // `out.uv` without the tile-fract logic falls back to the tile origin.
    out.uv = tile_uv0;
    out.light = light;
    out.anim_id = in.anim_id;
    // W199: forward the packed water depth (light byte 3). 0 for meshes
    // that never set it (with_water_depth), i.e. everything but water.
    out.water_depth = in.light_packed.w;
    out.tile_uv0_out = tile_uv0;
    out.tile_size_out = tile_size;
    out.quad_uv_out = quad_uv;
    return out;
}

const PI: f32 = 3.14159265358979323846;

fn distribution_ggx(n_dot_h: f32, roughness: f32) -> f32 {
    let a = roughness * roughness;
    let a2 = a * a;
    let denom = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;
    return a2 / (PI * denom * denom + 1e-7);
}

fn geometry_smith_ggx(n_dot_v: f32, n_dot_l: f32, roughness: f32) -> f32 {
    let r = roughness + 1.0;
    let k = (r * r) / 8.0;
    let gv = n_dot_v / (n_dot_v * (1.0 - k) + k);
    let gl = n_dot_l / (n_dot_l * (1.0 - k) + k);
    return gv * gl;
}

fn fresnel_schlick(cos_theta: f32, f0: vec3<f32>) -> vec3<f32> {
    let one_minus = 1.0 - cos_theta;
    return f0 + (vec3<f32>(1.0) - f0) * pow(one_minus, 5.0);
}

fn sample_shadow(world_pos: vec3<f32>, n_dot_l: f32) -> f32 {
    let clip = shadow.view_proj * vec4<f32>(world_pos, 1.0);
    if (clip.w <= 0.0) {
        return 1.0;
    }
    let proj = clip.xyz / clip.w;
    // wgpu NDC: x,y in [-1,1] with y up; depth z in [0,1]. Sample uv has
    // y down.
    let uv = vec2<f32>(proj.x * 0.5 + 0.5, -proj.y * 0.5 + 0.5);
    if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0 || proj.z > 1.0 || proj.z < 0.0) {
        return 1.0;
    }
    // Slope-scaled bias on top of the pipeline depth bias.
    // W355: halved (0.0008 -> 0.0004) - stacked with the pipeline
    // bias it detached shadows from block feet, leaving a bright
    // seam between the wall and its shadow on the ground (the R17
    // screenshot). PCF 3x3 keeps acne in check at the lower bias.
    let bias = max(0.0004 * (1.0 - n_dot_l), 0.0001);
    let depth = proj.z - bias;
    // PCF 3x3.
    let texel = 1.0 / 2048.0;
    var sum = 0.0;
    for (var x = -1; x <= 1; x = x + 1) {
        for (var y = -1; y <= 1; y = y + 1) {
            let off = vec2<f32>(f32(x), f32(y)) * texel;
            sum = sum + textureSampleCompare(shadow_map, shadow_smp, uv + off, depth);
        }
    }
    return sum / 9.0;
}

// W200: the procedural water ripple (hash noise + scrolling octaves)
// is gone. Liquids now carry real atlas UVs for MC's animated
// water_still / lava_still textures and sample them like any other
// animated tile - the texture animation IS the water movement,
// exactly as in MC. The shader keeps only the water-specific alpha
// (depth-based opacity) and material handling.

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // W240: split the packed vertex word - low 8 bits = anim slot,
    // high bits = block id (0 = non-chunk mesh, use instance params).
    let anim_slot = in.anim_id & 0xFFu;
    let block_id = in.anim_id >> 8u;
    var inst_color = in.color;
    var inst_material = in.material;
    if (block_id != 0u) {
        let bp = block_params.entries[min(block_id, 1023u)];
        inst_color = bp.color;
        // Keep material.w (the liquid flag) from the instance - the
        // block table never carries it.
        inst_material = vec4<f32>(bp.material.xyz, in.material.w);
    }
    let metallic = clamp(inst_material.x, 0.0, 1.0);
    let roughness = clamp(inst_material.y, 0.04, 1.0);
    // material.w >= 0.5 marks the liquid-surface meshes (water + lava).
    // W200: they sample the atlas exactly like chunk meshes now (their
    // vertices carry the water_still / lava_still tile rect + anim slot);
    // the flag only drives the water-specific alpha below.
    let is_water = in.material.w >= 0.5;
    // W30 fix: per-fragment tile-fract UV. Interpolated quad_uv
    // gives true fractional values across the merged greedy quad;
    // fract() wraps it within [0, 1] per tile, then we map back
    // into the atlas rect [tile_uv0, tile_uv0 + tile_size]. When
    // tile_size = 0 (per-voxel mesher set up via with_uv), the
    // formula collapses to in.uv = tile_uv0 = legacy UV.
    //
    // W31.3: clamp the result to the interior of the tile to prevent
    // the 1-pixel seam artifact at integer voxel boundaries inside
    // merged greedy quads. fract(N.0) can land on exactly 0.0 or
    // 1.0 at tile edges due to floating-point precision, causing
    // the sampler to bleed into the adjacent tile. Clamping to
    // [tile_uv0 + half_texel, tile_uv0 + tile_size - half_texel]
    // keeps the UV strictly inside this tile's texels.
    let tile_size = in.tile_size_out;
    var final_uv: vec2<f32>;
    if tile_size.x > 0.0 && tile_size.y > 0.0 {
        let raw_uv = in.tile_uv0_out + fract(in.quad_uv_out) * tile_size;
        let atlas_dim = vec2<f32>(textureDimensions(albedo_tex));
        let half_texel = 0.5 / atlas_dim;
        final_uv = clamp(
            raw_uv,
            in.tile_uv0_out + half_texel,
            in.tile_uv0_out + tile_size - half_texel,
        );
    } else {
        final_uv = in.uv;
    }
    // Wave 9.y: scroll UV.v through the active frame of an
    // animated tile. `camera.time.x` is seconds since app
    // start; the slot stores frametime_ms, so we convert to
    // ms before the per-frame floor.
    let time_ms = camera.time.x * 1000.0;
    let sampled_uv = animated_uv(final_uv, anim_slot, time_ms);
    let tex = textureSample(albedo_tex, albedo_smp, sampled_uv);
    var albedo = inst_color.rgb * tex.rgb;
    let alpha = inst_color.a * tex.a;
    // Simple alpha cutout for foliage / leaves. The default 1x1
    // white texture has alpha=1 so opaque blocks are unaffected;
    // atlas tiles with see-through pixels (e.g. leaves) get the
    // gaps discarded.
    if alpha < 0.5 {
        discard;
    }

    let n = normalize(in.world_normal);
    let v = normalize(camera.camera_pos.xyz - in.world_pos);
    let light_dir = normalize(shadow.light_dir.xyz);
    let l = light_dir;
    let h = normalize(v + l);

    let n_dot_v = max(dot(n, v), 1e-4);
    let n_dot_l = max(dot(n, l), 0.0);
    let n_dot_h = max(dot(n, h), 0.0);
    let v_dot_h = max(dot(v, h), 0.0);

    let f0 = mix(vec3<f32>(0.04), albedo, metallic);
    let f = fresnel_schlick(v_dot_h, f0);
    let d = distribution_ggx(n_dot_h, roughness);
    let g = geometry_smith_ggx(n_dot_v, n_dot_l, roughness);

    // W154: matte-terrain specular kill. MC terrain has NO specular
    // at all, but GGX at grazing angles (Fresnel -> 1) still painted
    // a bright streak on fully-rough faces - glaring on dark albedo
    // like obsidian (the user's "weird unstructured reflection").
    // Scaling the whole lobe by (1-roughness)^2 zeroes it for matte
    // blocks (default r=0.95, obsidian r=1.0) while glass / iron /
    // water keep their shine.
    let spec_scale = (1.0 - roughness) * (1.0 - roughness);
    let specular = (d * g * f) / (4.0 * n_dot_v * n_dot_l + 1e-4) * spec_scale;

    // Energy conservation: diffuse only for non-metals.
    let k_d = (vec3<f32>(1.0) - f) * (1.0 - metallic);
    let diffuse = k_d * albedo / PI;

    let shadow_factor = sample_shadow(in.world_pos, n_dot_l);
    // Intensity is packed into the w channel of `shadow.light_dir`
    // by the CPU side (`Renderer::set_sun_intensity`). Default
    // 3.0 mirrors the previous hardcoded constant; drivers can
    // taper this toward zero for night.
    let sun_intensity = shadow.light_dir.w;
    var direct = (diffuse + specular) * n_dot_l * sun_intensity * shadow_factor;

    // Accumulate point-light contributions. Range-based smooth
    // falloff: attenuation = clamp(1 - (d/range)^2, 0, 1)^2 (a
    // cheap-but-decent Frostbite-style window).
    let count = point_lights.count.x;
    for (var i: u32 = 0u; i < count; i = i + 1u) {
        if (i >= MAX_LIGHTS) {
            break;
        }
        let pl = point_lights.lights[i];
        let lp = pl.position.xyz - in.world_pos;
        let dist = length(lp);
        let range = max(pl.params.x, 1e-3);
        let t = clamp(1.0 - (dist / range) * (dist / range), 0.0, 1.0);
        let attenuation = t * t;
        if (attenuation <= 0.001) {
            continue;
        }
        let ldir = normalize(lp);
        let hp = normalize(v + ldir);
        let n_dot_lp = max(dot(n, ldir), 0.0);
        let n_dot_hp = max(dot(n, hp), 0.0);
        let v_dot_hp = max(dot(v, hp), 0.0);
        let fp = fresnel_schlick(v_dot_hp, f0);
        let dp = distribution_ggx(n_dot_hp, roughness);
        let gp = geometry_smith_ggx(n_dot_v, n_dot_lp, roughness);
        // W154: same matte kill as the sun lobe above.
        let spec_p = (dp * gp * fp) / (4.0 * n_dot_v * n_dot_lp + 1e-4)
            * spec_scale;
        let k_dp = (vec3<f32>(1.0) - fp) * (1.0 - metallic);
        let diff_p = k_dp * albedo / PI;
        let intensity = pl.color.w;
        direct = direct
            + (diff_p + spec_p) * n_dot_lp * pl.color.rgb * intensity * attenuation;
    }

    // Hemispheric ambient: a small sky + ground term.
    // W35.1: raised and warmed to match MC's more saturated look.
    // Previous values (0.20, 0.22, 0.28) were too cold and dark,
    // desaturating colours like obsidian. New values are brighter
    // with a neutral-warm tint instead of the blue tint.
    let ambient_up = vec3<f32>(0.34, 0.36, 0.40);
    let ambient_down = vec3<f32>(0.18, 0.17, 0.16);
    let amb_t = (n.y * 0.5) + 0.5;
    let ambient = mix(ambient_down, ambient_up, amb_t) * albedo;

    // Phase L.5: fold per-voxel light into the final color.
    // `light.x` is the block-emitter-propagated value (torch/
    // glowstone/lava/fire flood), `light.y * day_factor` is the
    // daylight-modulated sky-light value. Take the max so torches
    // stay bright at night and sky-lit cells dim with the day
    // clock. A small `0.05` floor keeps cells visible in pitch
    // black so the player isn't truly blinded.
    let day_factor = clamp(camera.time.y, 0.0, 1.0);
    let block_light_norm = clamp(in.light.x, 0.0, 1.0);
    let sky_light_norm = clamp(in.light.y, 0.0, 1.0);
    let effective_light = max(block_light_norm, sky_light_norm * day_factor);
    // W122: vertex ambient occlusion. `in.light.z` interpolates per
    // pixel across the face (1 = no occlusion). Fold it into the
    // light multiplier so concave corners darken the way MC's smooth
    // lighting does. The 0.05 floor stays AO-independent so occluded
    // corners never go fully black.
    let ao = clamp(in.light.z, 0.0, 1.0);
    let light_mult = 0.05 + 0.95 * effective_light * ao;
    var lit_color = (direct + ambient) * light_mult;

    // W46.19: emissive boost. material.z = strength packed by
    // MeshInstance::with_emissive. Adds `strength * albedo` to the
    // lit result so self-illuminated blocks (torch tip, glowstone,
    // lava, lit redstone) read at full colour even when the cell is
    // unlit. Skipped (strength = 0) for normal blocks.
    let emissive_strength = clamp(inst_material.z, 0.0, 4.0);
    if emissive_strength > 0.0 {
        lit_color = lit_color + albedo * emissive_strength;
    }

    // W229.2: MC's two-term fog (26.1.2 fog.glsl total_fog_value,
    // LINEAR - not smoothstep):
    //  - environmental band (fog.xy): SPHERICAL distance - the gentle
    //    atmospheric haze (overworld default 0..1024, barely there),
    //    also reused for the short cave / underwater bands.
    //  - render-distance mask (fog.zw): CYLINDRICAL distance
    //    max(|dxz|, |dy|) - a narrow curtain right at the chunk-load
    //    edge. Cylindrical means looking DOWN from altitude stays
    //    clear (the old single spherical smoothstep milked out the
    //    whole view from any height).
    // W200: EVERYTHING fogs to the sky colour, water included. The
    // W197.2 water-fogs-to-deep-blue special case made the far ocean a
    // saturated blue sheet against the hazy horizon - the whole scene
    // faded into atmosphere except the water, which visibly "did not
    // belong" (the user's words). MC fogs water into the same
    // atmospheric haze as land.
    let dpos = in.world_pos - camera.camera_pos.xyz;
    let spherical_dist = length(dpos);
    let cylindrical_dist = max(length(dpos.xz), abs(dpos.y));
    var fog_factor = 0.0;
    if camera.fog.y > camera.fog.x {
        fog_factor = clamp(
            (spherical_dist - camera.fog.x) / (camera.fog.y - camera.fog.x),
            0.0,
            1.0,
        );
    }
    if camera.fog.w > camera.fog.z {
        let rd = clamp(
            (cylindrical_dist - camera.fog.z) / (camera.fog.w - camera.fog.z),
            0.0,
            1.0,
        );
        fog_factor = max(fog_factor, rd);
    }
    if fog_factor > 0.0 {
        lit_color = mix(lit_color, camera.sky_color.rgb, fog_factor);
    }

    var out_alpha = alpha;
    if is_water {
        // W205: uniform MC-style translucency. The W199 depth-based
        // opacity made deep water a solid sheet you could never see
        // into from the surface - but in MC you CAN look into water
        // from above; the deep ocean reads dark because sky light
        // attenuates per water block (now implemented in the lighting
        // engine), not because the surface goes opaque. `water_depth`
        // stays in the vertex stream for future use but no longer
        // drives alpha. The far-distance backstop remains: it hides
        // unloaded-boundary holes that fog alone misses at grazing
        // angles.
        //
        // W286: the backstop is FOG-RELATIVE now. The fixed 40-110 m
        // ramp made any lake viewed from a hilltop a fully opaque
        // sheet (docx img17 - in MC the same view still reads into
        // the water). Tie it to the far fog edge instead: holes can
        // only exist near the streaming boundary, which sits at the
        // fog range, so opacify only as the surface approaches it.
        let far_fog = max(camera.fog.y, camera.fog.w);
        var dist_opacify = 0.0;
        if far_fog > 0.0 {
            dist_opacify = smoothstep(0.55 * far_fog, 0.9 * far_fog, spherical_dist);
        }
        out_alpha = mix(alpha, 1.0, dist_opacify);
    }

    return vec4<f32>(lit_color, out_alpha);
}
