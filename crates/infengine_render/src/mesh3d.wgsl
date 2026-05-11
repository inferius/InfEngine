// Basic 3D mesh shader with one directional light and a per-instance
// model matrix + tint.

struct CameraUniform {
    view_proj: mat4x4<f32>,
    camera_pos: vec4<f32>,
};

@group(0) @binding(0) var<uniform> camera: CameraUniform;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    // Per-instance model matrix supplied as 4 rows of vec4.
    @location(2) inst_row0: vec4<f32>,
    @location(3) inst_row1: vec4<f32>,
    @location(4) inst_row2: vec4<f32>,
    @location(5) inst_row3: vec4<f32>,
    @location(6) inst_color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip: vec4<f32>,
    @location(0) world_normal: vec3<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    // glam serializes Mat4 column-major; instance rows are columns of
    // the model matrix.
    let model = mat4x4<f32>(in.inst_row0, in.inst_row1, in.inst_row2, in.inst_row3);
    let world = model * vec4<f32>(in.position, 1.0);

    let normal_mat = mat3x3<f32>(
        model[0].xyz,
        model[1].xyz,
        model[2].xyz,
    );

    var out: VertexOutput;
    out.clip = camera.view_proj * world;
    out.world_normal = normalize(normal_mat * in.normal);
    out.color = in.inst_color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let light_dir = normalize(vec3<f32>(0.4, 0.9, 0.5));
    let n = normalize(in.world_normal);
    let n_dot_l = max(dot(n, light_dir), 0.0);
    let ambient = 0.25;
    let lit = in.color.rgb * (ambient + (1.0 - ambient) * n_dot_l);
    return vec4<f32>(lit, in.color.a);
}
