// Baseline PBR-ish mesh shader.
//
// One directional sun + a tiny ambient term. Diffuse uses Lambert;
// specular uses Cook-Torrance with GGX distribution and a Schlick
// Fresnel. F0 lerps from 0.04 (dielectric) to the albedo for metals.
// Albedo is the per-instance color modulated by a sampled texture
// (defaults to 1x1 white so untextured meshes look unchanged).

struct CameraUniform {
    view_proj: mat4x4<f32>,
    camera_pos: vec4<f32>,
};

@group(0) @binding(0) var<uniform> camera: CameraUniform;

@group(1) @binding(0) var albedo_tex: texture_2d<f32>;
@group(1) @binding(1) var albedo_smp: sampler;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) inst_row0: vec4<f32>,
    @location(4) inst_row1: vec4<f32>,
    @location(5) inst_row2: vec4<f32>,
    @location(6) inst_row3: vec4<f32>,
    @location(7) inst_color: vec4<f32>,
    @location(8) inst_material: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) material: vec4<f32>,
    @location(4) uv: vec2<f32>,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    let model = mat4x4<f32>(in.inst_row0, in.inst_row1, in.inst_row2, in.inst_row3);
    let world4 = model * vec4<f32>(in.position, 1.0);
    let normal_mat = mat3x3<f32>(model[0].xyz, model[1].xyz, model[2].xyz);

    var out: VertexOutput;
    out.clip = camera.view_proj * world4;
    out.world_pos = world4.xyz;
    out.world_normal = normalize(normal_mat * in.normal);
    out.color = in.inst_color;
    out.material = in.inst_material;
    out.uv = in.uv;
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

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let metallic = clamp(in.material.x, 0.0, 1.0);
    let roughness = clamp(in.material.y, 0.04, 1.0);
    let tex = textureSample(albedo_tex, albedo_smp, in.uv);
    let albedo = in.color.rgb * tex.rgb;
    let alpha = in.color.a * tex.a;

    let n = normalize(in.world_normal);
    let v = normalize(camera.camera_pos.xyz - in.world_pos);
    let light_dir = normalize(vec3<f32>(0.4, 0.9, 0.5));
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

    let specular = (d * g * f) / (4.0 * n_dot_v * n_dot_l + 1e-4);

    // Energy conservation: diffuse only for non-metals.
    let k_d = (vec3<f32>(1.0) - f) * (1.0 - metallic);
    let diffuse = k_d * albedo / PI;

    let sun_intensity = 3.0;
    let direct = (diffuse + specular) * n_dot_l * sun_intensity;

    // Hemispheric ambient: a small sky + ground term.
    let ambient_up = vec3<f32>(0.20, 0.22, 0.28);
    let ambient_down = vec3<f32>(0.08, 0.08, 0.10);
    let amb_t = (n.y * 0.5) + 0.5;
    let ambient = mix(ambient_down, ambient_up, amb_t) * albedo;

    return vec4<f32>(direct + ambient, alpha);
}
